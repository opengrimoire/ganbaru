//! Work assignment cancellation, retry, and dispatch recovery.

use super::common::{
    chat_writers, new_id, parse_folder_capability, parse_runtime_approval_policy, this_device_id,
    wire_work_state,
};
use super::context::{GrantedAccess, freeze_context_package, read_granted_access};
use super::dispatch::{deliver_pending_assignment_inputs, dispatch_assignment_job};
use super::reads::{read_active_or_latest_assignment, read_assignment};
use super::workflow::{
    NewAssignment, PostingDevice, create_assignment, end_assignment_dispatch_jobs,
    read_assignment_executing_device, require_participating_teammate, set_assignment_state,
};
use super::{chat_pool, i64_value, identifier_error, now_timestamp, persistence_error, timestamp};
use crate::chat::device_state::{
    ChatDeviceScope, assignment_is_accepted, read_active_device_scope,
    record_assignment_acceptance, update_active_device_scope,
};
use crate::chat::settings::device_state_error;
use crate::chat::sync::SyncedAssignmentWork;
use ganbaru_chat::coordination::acceptance::{AcceptanceSubject, acceptance_digest};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

pub(super) async fn recover_assignment_dispatch_jobs(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<u32> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let writers = chat_writers(&app, &pool).await?;
    let device = PostingDevice::this_device(&device_id, &writers);
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let now = now_timestamp()?;
    take_over_stranded_assignments(&pool, device, &scope, &now).await?;
    recover_stranded_follow_up_assignments(&pool, device, &scope, &now).await?;
    adopt_accepted_queued_assignments(&pool, device, &scope).await?;
    sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs
         SET state = 'queued', claimed_at = NULL, claim_token = NULL, updated_at = ?
         WHERE state = 'claimed'
           AND claimed_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-5 minutes')",
    )
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    let working_assignment_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT assignment.id
         FROM chat_work_assignments assignment
         JOIN chat_agent_runs run ON run.assignment_id = assignment.id
         JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
         WHERE assignment.state = 'working' AND run.state = 'working'
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
         ORDER BY assignment.updated_at, assignment.id LIMIT 32",
    )
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    for assignment_id in working_assignment_ids {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        let delivery_app = app.clone();
        let delivery_db_url = db_url.clone();
        let delivery_pool = pool.clone();
        tauri::async_runtime::spawn(async move {
            let _ = deliver_pending_assignment_inputs(
                delivery_app,
                delivery_db_url,
                &delivery_pool,
                &assignment_id,
            )
            .await;
        });
    }
    let assignment_ids = sqlx::query_scalar::<_, String>(
        "SELECT assignment_id FROM chat_assignment_dispatch_jobs
         WHERE state = 'queued' AND available_at <= ?
         ORDER BY created_at, id LIMIT 32",
    )
    .bind(now.as_str())
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let count = u32::try_from(assignment_ids.len()).unwrap_or(32);
    for assignment_id in assignment_ids {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        let worker_app = app.clone();
        let worker_db_url = db_url.clone();
        tauri::async_runtime::spawn(async move {
            let _ = dispatch_assignment_job(worker_app, worker_db_url, assignment_id).await;
        });
    }
    Ok(count)
}

/// Reason an assignment records when its executing device can no longer write Chat and this
/// device moves its unstarted work onto a successor it runs.
pub(super) const REROUTED_REASON: &str = "Moved to a device that can run it";
/// Reason a started assignment records when its executing device can no longer write Chat, so
/// it would never settle; Retry or Start runs it again on a device that can.
pub(super) const STRANDED_REASON: &str =
    "Its device can no longer run Chat work. Retry to run it on this device";

/// Whether `executing_device_id` names a device that cannot write Chat, so the work it executes
/// would stay queued or active forever.
fn stranded_executor(executing_device_id: Option<&str>, device: PostingDevice<'_>) -> bool {
    executing_device_id.is_some_and(|executor| !device.chat_writers.includes(executor))
}

/// Takes over the active assignments whose executing device can no longer write Chat, as after
/// an unlink or a revoked writer, when `device` can run new work. Unstarted work moves to a
/// successor this device runs; started work fails with a reason, since its provider session
/// lives on the old device, and Retry or Start runs it again here. Unstarted work `scope` did
/// not accept stays where it is until the user starts it here.
pub(super) async fn take_over_stranded_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
    now: &UtcTimestamp,
) -> ChatResult<u32> {
    if !device.runs_new_work() {
        return Ok(0);
    }
    let writers = device.chat_writers.json()?;
    let mut after_id = String::new();
    let mut taken_over = 0_u32;
    loop {
        let stranded = sqlx::query(
            "SELECT assignment.id,
                    assignment.state = 'queued' AND NOT EXISTS (
                      SELECT 1 FROM chat_agent_runs run WHERE run.assignment_id = assignment.id
                    ) AS unstarted
             FROM chat_work_assignments assignment
             WHERE assignment.state IN
                     ('queued', 'working', 'waiting_for_answer', 'waiting_for_approval')
               AND assignment.executing_device_id IS NOT NULL
               AND assignment.executing_device_id NOT IN (SELECT value FROM json_each(?))
               AND assignment.id > ?
             ORDER BY assignment.id LIMIT ?",
        )
        .bind(writers.as_str())
        .bind(after_id.as_str())
        .bind(i64::from(TAKEOVER_BATCH_SIZE))
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
        let batch_len = stranded.len();
        for row in stranded {
            let assignment_id = ChatWorkAssignmentId::new(
                row.try_get::<String, _>("id").map_err(persistence_error)?,
            )
            .map_err(identifier_error)?;
            let unstarted: bool = row.try_get("unstarted").map_err(persistence_error)?;
            if take_over_stranded_assignment(pool, &assignment_id, unstarted, device, scope, now)
                .await?
            {
                taken_over = taken_over.saturating_add(1);
            }
            after_id = assignment_id.as_str().to_owned();
        }
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE) {
            return Ok(taken_over);
        }
    }
}

/// Number of stranded assignments one takeover query reads at a time; takeover keeps reading
/// batches until every stranded assignment was visited.
const TAKEOVER_BATCH_SIZE: u16 = 32;

/// Takes over the stranded assignment `assignment_id`, moving unstarted work to a successor
/// `device` runs and failing started work, reporting whether it changed. Unstarted work `scope`
/// did not accept stays unchanged.
async fn take_over_stranded_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    unstarted: bool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let assignment = read_assignment(pool, assignment_id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if unstarted && !assignment_subject_is_accepted(&mut transaction, assignment_id, scope).await? {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(false);
    }
    let moved = if unstarted {
        reroute_assignment(&mut transaction, &assignment, device, now)
            .await?
            .is_some()
    } else {
        settle_stranded_assignment(
            &mut transaction,
            &assignment,
            ChatWorkAssignmentState::Failed,
            STRANDED_REASON,
            now,
        )
        .await?
    };
    transaction.commit().await.map_err(persistence_error)?;
    Ok(moved)
}

/// Settles the active assignment `assignment` as `state` with `reason`, unless it changed since
/// it was read, and ends its local dispatch jobs, reporting whether it settled.
async fn settle_stranded_assignment(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment: &ChatWorkAssignmentRead,
    state: ChatWorkAssignmentState,
    reason: &str,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    let settled = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = ?, state_reason = ?, settled_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ? AND state IN
                 ('queued', 'working', 'waiting_for_answer', 'waiting_for_approval')",
    )
    .bind(wire_work_state(state))
    .bind(reason)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(assignment.id.as_str())
    .bind(i64_value(assignment.revision)?)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if settled.rows_affected() != 1 {
        return Ok(false);
    }
    end_assignment_dispatch_jobs(&mut **transaction, &assignment.id, state, now).await?;
    Ok(true)
}

/// Moves the work of `assignment`, whose executing device can no longer write Chat, onto a
/// successor that `device` runs: an active `assignment` is cancelled first, and its pending
/// inputs follow the trigger in order. The successor keeps the trigger of `assignment` and
/// names it as its predecessor, so its id derives from `assignment` alone and every device
/// and path that moves the same work merges into one successor. Returns the successor, or
/// none when `assignment` changed since it was read or already has a successor.
async fn reroute_assignment(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment: &ChatWorkAssignmentRead,
    device: PostingDevice<'_>,
    now: &UtcTimestamp,
) -> ChatResult<Option<ChatWorkAssignmentId>> {
    let has_successor: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_work_assignments WHERE previous_assignment_id = ?)",
    )
    .bind(assignment.id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if has_successor {
        return Ok(None);
    }
    let mut previous = assignment.clone();
    if previous.state.accepts_continuation() {
        if !settle_stranded_assignment(
            transaction,
            assignment,
            ChatWorkAssignmentState::Cancelled,
            REROUTED_REASON,
            now,
        )
        .await?
        {
            return Ok(None);
        }
        previous.state = ChatWorkAssignmentState::Cancelled;
    }
    let successor_id = create_assignment(
        transaction,
        NewAssignment {
            reply_thread_id: &assignment.reply_thread_id,
            message_item_id: &assignment.triggering_message_item_id,
            teammate_id: &assignment.teammate.id,
            latest_assignment: Some(&previous),
            executing_device_id: device.id,
            execution_target: None,
        },
        device,
        now,
    )
    .await?;
    let pending_input_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_work_assignment_inputs
         WHERE assignment_id = ? AND delivery_state = 'pending'
         ORDER BY ordinal",
    )
    .bind(assignment.id.as_str())
    .fetch_all(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    move_pending_inputs(transaction, &pending_input_ids, &successor_id).await?;
    Ok(Some(successor_id))
}

/// Moves the pending inputs `input_ids`, in order, behind the trigger of `assignment_id` as
/// queued continuations.
async fn move_pending_inputs(
    transaction: &mut Transaction<'_, Sqlite>,
    input_ids: &[String],
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<()> {
    for (index, input_id) in input_ids.iter().enumerate() {
        let ordinal = i64::try_from(index + 2).map_err(|_| {
            ChatError::new(
                ChatErrorCode::Validation,
                "Too many pending follow-up messages",
                true,
            )
        })?;
        sqlx::query(
            "UPDATE chat_work_assignment_inputs
             SET assignment_id = ?, ordinal = ?, routing_kind = 'queued_continuation'
             WHERE id = ? AND delivery_state = 'pending'",
        )
        .bind(assignment_id.as_str())
        .bind(ordinal)
        .bind(input_id)
        .execute(&mut **transaction)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

/// Reads, past `after_id` and up to `limit`, the assignments with pending replies that arrived
/// after they asked for review, which `device` executes, ran without an executing device, or
/// takes over from an executing device that can no longer write Chat. `only` narrows the read
/// to one assignment.
async fn stranded_follow_up_ids(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    only: Option<&ChatWorkAssignmentId>,
    after_id: &str,
    limit: u16,
) -> ChatResult<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT assignment.id
         FROM chat_work_assignments assignment
         JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
         WHERE assignment.state = 'ready_for_review'
           AND (assignment.executing_device_id = ?1 OR (
             assignment.executing_device_id IS NULL AND EXISTS (
               SELECT 1 FROM chat_agent_runs run WHERE run.assignment_id = assignment.id
             )
           ) OR (?3 AND assignment.executing_device_id NOT IN (
             SELECT value FROM json_each(?2)
           )))
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
           AND (?4 IS NULL OR assignment.id = ?4)
           AND assignment.id > ?5
         ORDER BY assignment.id LIMIT ?6",
    )
    .bind(device.id)
    .bind(device.chat_writers.json()?)
    .bind(device.runs_new_work())
    .bind(only.map(ChatWorkAssignmentId::as_str))
    .bind(after_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)
}

/// Moves the pending inputs that reached an assignment after it asked for review onto a new
/// follow-up assignment, for the assignments `device` executes, ran without an executing
/// device, or takes over from an executing device that can no longer write Chat. The inputs
/// keep their ids, which derive from their messages, so a device that routed them concurrently
/// merges into the same rows. Only work whose current subject `scope` accepted continues; the
/// replies of other work stay pending until the user starts it here.
pub(super) async fn recover_stranded_follow_up_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
    now: &UtcTimestamp,
) -> ChatResult<u32> {
    let mut after_id = String::new();
    let mut recovered = 0_u32;
    loop {
        let assignment_ids =
            stranded_follow_up_ids(pool, device, None, &after_id, TAKEOVER_BATCH_SIZE).await?;
        let batch_len = assignment_ids.len();
        for assignment_id in assignment_ids {
            let assignment_id =
                ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
            if recover_follow_up(pool, &assignment_id, device, scope, now)
                .await?
                .is_some()
            {
                recovered = recovered.saturating_add(1);
            }
            after_id = assignment_id.as_str().to_owned();
        }
        // Unaccepted rows stay in place, so the bound counts recoveries and not rows read.
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE)
            || recovered >= u32::from(TAKEOVER_BATCH_SIZE)
        {
            return Ok(recovered);
        }
    }
}

/// Moves the pending inputs of `assignment_id` onto a follow-up assignment `device` runs, when
/// `scope` accepted the current subject, and returns the follow-up.
async fn recover_follow_up(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
    now: &UtcTimestamp,
) -> ChatResult<Option<ChatWorkAssignmentId>> {
    let previous = read_assignment(pool, assignment_id).await?;
    let reply_thread_id = previous.reply_thread_id.clone();
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if !assignment_subject_is_accepted(&mut transaction, assignment_id, scope).await? {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(None);
    }
    let stranded = sqlx::query(
        "SELECT input.id, input.message_item_id
         FROM chat_work_assignment_inputs input
         JOIN chat_work_assignments assignment ON assignment.id = input.assignment_id
         WHERE input.assignment_id = ? AND assignment.state = 'ready_for_review'
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
         ORDER BY input.ordinal",
    )
    .bind(assignment_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let Some(first) = stranded.first() else {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(None);
    };
    let first_message_id = ChatConversationItemId::new(
        first
            .try_get::<String, _>("message_item_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let teammate_id = previous.teammate.id.clone();
    let recovered_assignment_id = create_assignment(
        &mut transaction,
        NewAssignment {
            reply_thread_id: &reply_thread_id,
            message_item_id: &first_message_id,
            teammate_id: &teammate_id,
            latest_assignment: Some(&previous),
            executing_device_id: device.id,
            execution_target: None,
        },
        device,
        now,
    )
    .await?;
    let following_input_ids = stranded
        .iter()
        .skip(1)
        .map(|input| input.try_get::<String, _>("id").map_err(persistence_error))
        .collect::<ChatResult<Vec<_>>>()?;
    move_pending_inputs(
        &mut transaction,
        &following_input_ids,
        &recovered_assignment_id,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(Some(recovered_assignment_id))
}

pub(super) async fn cancel_assignment(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<ChatWorkAssignmentRead> {
    let stop_app = app.clone();
    let stop_db_url = db_url.clone();
    let pool = chat_pool(app, db_url).await?;
    let cancelled = set_assignment_state(
        &pool,
        &assignment_id,
        expected_revision,
        ChatWorkAssignmentState::Cancelled,
        Some("Cancelled by the user"),
    )
    .await?;
    let provider_thread_id: Option<String> = sqlx::query_scalar(
        "SELECT provider_thread_id FROM chat_agent_runs
         WHERE assignment_id = ? AND provider_thread_id IS NOT NULL
         ORDER BY run_ordinal DESC LIMIT 1",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?
    .flatten();
    if let Some(provider_thread_id) = provider_thread_id {
        let thread_id = ChatThreadId::new(provider_thread_id).map_err(identifier_error)?;
        let client_command_id =
            ChatCommandId::new(format!("assignment-cancel:{}", assignment_id.as_str()))
                .map_err(identifier_error)?;
        tauri::async_runtime::spawn(async move {
            let _ = super::super::interaction_commands::chat_stop_session(
                stop_app,
                stop_db_url,
                thread_id,
                false,
                client_command_id,
            )
            .await;
        });
    }
    Ok(cancelled)
}

/// Refuses active writes to `assignment_id` from a device other than its executing device. An
/// assignment without one belongs to the device that holds its device-local dispatch job.
pub(super) async fn require_executing_device(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device_id: &str,
) -> ChatResult<()> {
    let executes_here = match read_assignment_executing_device(pool, assignment_id.as_str()).await?
    {
        Some(executing_device_id) => executing_device_id == device_id,
        None => sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?)",
        )
        .bind(assignment_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?,
    };
    if executes_here {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This work runs on another device",
            true,
        ))
    }
}

pub(super) async fn retry_assignment(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<ChatWorkAssignmentRead> {
    let dispatch_app = app.clone();
    let dispatch_db_url = db_url.clone();
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let writers = chat_writers(&app, &pool).await?;
    let now = now_timestamp()?;
    let retried_id = persist_retry(
        &pool,
        &assignment_id,
        expected_revision,
        PostingDevice::this_device(&device_id, &writers),
        &now,
    )
    .await?;
    let executor = read_assignment_executing_device(&pool, retried_id.as_str()).await?;
    if executor.as_deref() == Some(device_id.as_str()) {
        record_gesture_acceptance(&app, &pool, &retried_id).await;
    }
    let retried = read_assignment(&pool, &retried_id).await?;
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_assignment_job(dispatch_app, dispatch_db_url, retried_id).await;
    });
    Ok(retried)
}

/// Queues the failed or cancelled assignment `assignment_id` again on `device`, returning the
/// assignment that runs. Work whose executing device can no longer write Chat moves to a
/// successor `device` runs, since an executing device never changes.
pub(super) async fn persist_retry(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    expected_revision: u64,
    device: PostingDevice<'_>,
    now: &UtcTimestamp,
) -> ChatResult<ChatWorkAssignmentId> {
    let current = read_assignment(pool, assignment_id).await?;
    if !matches!(
        current.state,
        ChatWorkAssignmentState::Failed | ChatWorkAssignmentState::Cancelled
    ) {
        return Err(ChatError::new(
            ChatErrorCode::InvalidStateTransition,
            "Only failed or cancelled work can be retried",
            true,
        ));
    }
    let executing_device_id =
        read_assignment_executing_device(pool, assignment_id.as_str()).await?;
    if device.runs_new_work() && stranded_executor(executing_device_id.as_deref(), device) {
        if current.revision != expected_revision {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "The assignment changed before retry",
                true,
            ));
        }
        let mut transaction = pool.begin().await.map_err(persistence_error)?;
        let successor = reroute_assignment(&mut transaction, &current, device, now)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "This work already moved to another assignment",
                    true,
                )
            })?;
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(successor);
    }
    require_executing_device(pool, assignment_id, device.id).await?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let updated = sqlx::query(
        "UPDATE chat_work_assignments
         SET state = 'queued', state_reason = NULL, settled_at = NULL,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The assignment changed before retry",
            true,
        ));
    }
    queue_dispatch_here(&mut transaction, assignment_id, now).await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(assignment_id.clone())
}

/// Queues the dispatch of `assignment_id` on this device, first freezing its context when the
/// assignment arrived from the device that created it. The job consents to the subject read
/// here, so a later change returns the work to Start here at claim.
async fn queue_dispatch_here(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT assignment.reply_thread_id, assignment.teammate_id,
                assignment.triggering_message_item_id,
                EXISTS(
                  SELECT 1 FROM chat_assignment_context_packages package
                  WHERE package.assignment_id = assignment.id
                ) AS has_context
         FROM chat_work_assignments assignment WHERE assignment.id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    if !row
        .try_get::<bool, _>("has_context")
        .map_err(persistence_error)?
    {
        let reply_thread_id = ChatReplyThreadId::new(
            row.try_get::<String, _>("reply_thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        let teammate_id = ChatParticipantId::new(
            row.try_get::<String, _>("teammate_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        let message_item_id = ChatConversationItemId::new(
            row.try_get::<String, _>("triggering_message_item_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        freeze_context_package(
            transaction,
            assignment_id,
            &reply_thread_id,
            &message_item_id,
            &teammate_id,
            None,
            now,
        )
        .await?;
    }
    let consented_digest = read_assignment_subject(transaction, assignment_id)
        .await?
        .digest();
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, consented_digest, available_at, created_at, updated_at)
         VALUES (?, ?, 'queued', ?, ?, ?, ?)
         ON CONFLICT(assignment_id) DO UPDATE SET
            state = 'queued', consented_digest = excluded.consented_digest,
            available_at = excluded.available_at,
            claimed_at = NULL, claim_token = NULL, last_error = NULL,
            updated_at = excluded.updated_at",
    )
    .bind(new_id("dispatch")?)
    .bind(assignment_id.as_str())
    .bind(&consented_digest)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Reason a synced assignment records when its executing device cannot freeze its context.
pub(super) const ADOPTION_FAILED_REASON: &str = "The work could not start on its executing device";

/// What `assignment_id` would run on this device, read from current rows.
pub(super) struct AssignmentSubject {
    teammate_id: ChatParticipantId,
    pub(super) instructions: String,
    provider_instance_id: String,
    access: GrantedAccess,
}

impl AssignmentSubject {
    pub(super) fn digest(&self) -> String {
        acceptance_digest(&AcceptanceSubject {
            teammate_id: self.teammate_id.as_str(),
            policy_revision_id: self.access.policy_revision_id.as_str(),
            instructions: &self.instructions,
            provider_instance_id: &self.provider_instance_id,
            grants: &self.access.grants,
            scratch_runtime_approval: self.access.scratch_runtime_approval,
        })
    }
}

pub(super) async fn read_assignment_subject(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<AssignmentSubject> {
    let row = sqlx::query(
        "SELECT assignment.teammate_id, assignment.triggering_message_item_id,
                thread.conversation_id, teammate.instructions
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_teammates teammate ON teammate.participant_id = assignment.teammate_id
         WHERE assignment.id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Work assignment was not found",
            true,
        )
    })?;
    let teammate_id = ChatParticipantId::new(
        row.try_get::<String, _>("teammate_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let conversation_id = ChatConversationId::new(
        row.try_get::<String, _>("conversation_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let message_item_id = ChatConversationItemId::new(
        row.try_get::<String, _>("triggering_message_item_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    let access = read_granted_access(
        transaction,
        &conversation_id,
        &message_item_id,
        &teammate_id,
    )
    .await?;
    let provider_instance_id: String = sqlx::query_scalar(
        "SELECT provider_instance_id FROM chat_teammate_policy_revisions WHERE id = ?",
    )
    .bind(access.policy_revision_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(AssignmentSubject {
        teammate_id,
        instructions: row.try_get("instructions").map_err(persistence_error)?,
        provider_instance_id,
        access,
    })
}

/// Whether the synced references of `assignment_id` hold together: its trigger belongs to its
/// reply thread and its teammate still participates in the conversation.
async fn assignment_references_hold(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_assignments assignment
           JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
           JOIN chat_conversation_items trigger_item
             ON trigger_item.id = assignment.triggering_message_item_id
            AND trigger_item.conversation_id = thread.conversation_id
           JOIN chat_conversation_memberships membership
             ON membership.conversation_id = thread.conversation_id
            AND membership.participant_id = assignment.teammate_id
            AND membership.removed_at IS NULL
           JOIN chat_participants participant
             ON participant.id = assignment.teammate_id AND participant.archived_at IS NULL
           WHERE assignment.id = ?
             AND (trigger_item.reply_thread_id = thread.id OR thread.root_item_id = trigger_item.id)
         )",
    )
    .bind(assignment_id.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(persistence_error)
}

/// The refusal of an assignment whose synced references do not hold together.
fn broken_references_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Conflict,
        "This work names a message or teammate outside its reply thread",
        true,
    )
}

/// Whether `scope` accepted what `assignment_id` would run. Work that cannot run here counts as
/// not accepted, so one bad row does not stop a recovery pass.
async fn assignment_subject_is_accepted(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    scope: &ChatDeviceScope,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Ok(false);
    }
    match read_assignment_subject(transaction, assignment_id).await {
        Ok(subject) => Ok(assignment_is_accepted(scope, &subject.digest())),
        Err(error) if error.code == ChatErrorCode::Persistence => Err(error),
        Err(_) => Ok(false),
    }
}

/// Hex acceptance digest of what `assignment_id` would run.
pub(super) async fn assignment_acceptance_digest(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<String> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if !assignment_references_hold(&mut transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let digest = read_assignment_subject(&mut transaction, assignment_id)
        .await?
        .digest();
    transaction.rollback().await.map_err(persistence_error)?;
    Ok(digest)
}

/// Whether `seen_digest` still names what `assignment_id` would run.
pub(super) async fn seen_digest_is_current(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    seen_digest: &str,
) -> ChatResult<bool> {
    Ok(assignment_acceptance_digest(pool, assignment_id).await? == seen_digest)
}

/// Reads, past `after_id` and up to `limit`, the queued assignments with no local dispatch that
/// `device` executes or could take over from a device that can no longer write Chat. `only`
/// narrows the read to one assignment.
async fn unstarted_work_ids(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    only: Option<&ChatWorkAssignmentId>,
    after_id: &str,
    limit: u16,
) -> ChatResult<Vec<String>> {
    sqlx::query_scalar::<_, String>(
        "SELECT assignment.id
         FROM chat_work_assignments assignment
         WHERE assignment.state = 'queued'
           AND (assignment.executing_device_id = ?1 OR (
             assignment.executing_device_id IS NOT NULL
             AND assignment.executing_device_id NOT IN (SELECT value FROM json_each(?2))
           ))
           AND NOT EXISTS (
             SELECT 1 FROM chat_assignment_dispatch_jobs job
             WHERE job.assignment_id = assignment.id
           )
           AND (?3 IS NULL OR assignment.id = ?3)
           AND assignment.id > ?4
         ORDER BY assignment.id LIMIT ?5",
    )
    .bind(device.id)
    .bind(device.chat_writers.json()?)
    .bind(only.map(ChatWorkAssignmentId::as_str))
    .bind(after_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)
}

/// Queues the dispatch of the queued work `device` executes whose subject `scope` accepted,
/// returning the assignments it queued. Work with another subject is left untouched, so one
/// acceptance never settles or starts anything the user did not see.
pub(super) async fn adopt_accepted_queued_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<Vec<ChatWorkAssignmentId>> {
    if !device.runs_new_work() {
        return Ok(Vec::new());
    }
    let mut adopted = Vec::new();
    let mut after_id = String::new();
    loop {
        let assignment_ids =
            unstarted_work_ids(pool, device, None, &after_id, TAKEOVER_BATCH_SIZE).await?;
        let batch_len = assignment_ids.len();
        for assignment_id in assignment_ids {
            let assignment_id =
                ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
            let mut transaction = pool.begin().await.map_err(persistence_error)?;
            let accepted =
                assignment_subject_is_accepted(&mut transaction, &assignment_id, scope).await?;
            transaction.rollback().await.map_err(persistence_error)?;
            if accepted && adopt_synced_assignment(pool, &assignment_id, device.id, scope).await? {
                adopted.push(assignment_id.clone());
            }
            after_id = assignment_id.as_str().to_owned();
        }
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE) {
            return Ok(adopted);
        }
    }
}

/// The folders, provider, and approvals behind the acceptance digest of `assignment_id`.
async fn build_acceptance_offer(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<ChatAssignmentAcceptanceOfferRead> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let subject = read_assignment_subject(transaction, assignment_id).await?;
    let mut folders = Vec::with_capacity(subject.access.grants.len());
    for grant in &subject.access.grants {
        let display_name: String =
            sqlx::query_scalar("SELECT display_name FROM project_working_folders WHERE id = ?")
                .bind(grant.working_folder_id.as_str())
                .fetch_one(&mut **transaction)
                .await
                .map_err(persistence_error)?;
        folders.push(ChatAssignmentAcceptanceFolderRead {
            display_name,
            capability: parse_folder_capability(&grant.capability)?,
            runtime_approval: parse_runtime_approval_policy(&grant.runtime_approval)?,
        });
    }
    Ok(ChatAssignmentAcceptanceOfferRead {
        digest: subject.digest(),
        provider_instance_id: subject.provider_instance_id.clone(),
        folders,
        scratch_runtime_approval: parse_runtime_approval_policy(
            subject.access.scratch_runtime_approval,
        )?,
    })
}

/// Reads what the user would accept by starting `assignment_id`. Work that cannot run here has
/// no offer, since Start refuses it; a failing read still surfaces.
pub(super) async fn read_acceptance_offer(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<ChatAssignmentAcceptanceOfferRead>> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let offer = build_acceptance_offer(&mut transaction, assignment_id).await;
    transaction.rollback().await.map_err(persistence_error)?;
    match offer {
        Ok(offer) => Ok(Some(offer)),
        Err(error) if error.code == ChatErrorCode::Persistence => Err(error),
        Err(_) => Ok(None),
    }
}

/// Most waiting assignments one read returns, newest first.
const WAITING_ASSIGNMENT_LIMIT: usize = 32;

/// Reads where the waiting work `assignment_id` lives, when its channel is still in use.
async fn read_waiting_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<Option<ChatWaitingAssignmentRead>> {
    let Some(row) = sqlx::query(
        "SELECT assignment.reply_thread_id, assignment.created_at, channel.id AS channel_id,
                channel.project_id, channel.name AS channel_name,
                participant.display_name AS teammate_display_name
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         JOIN chat_participants participant ON participant.id = assignment.teammate_id
         WHERE assignment.id = ? AND channel.archived_at IS NULL",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    else {
        return Ok(None);
    };
    Ok(Some(ChatWaitingAssignmentRead {
        assignment_id: assignment_id.clone(),
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        channel_id: ChatChannelId::new(
            row.try_get::<String, _>("channel_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        channel_name: row.try_get("channel_name").map_err(persistence_error)?,
        reply_thread_id: ChatReplyThreadId::new(
            row.try_get::<String, _>("reply_thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?,
        teammate_display_name: row
            .try_get("teammate_display_name")
            .map_err(persistence_error)?,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
    }))
}

/// Appends to `waiting` the rows of `assignment_ids` the user could start. With `accepted_by`,
/// work whose subject that scope accepted is skipped, since recovery continues it unasked.
async fn append_waiting_rows(
    pool: &SqlitePool,
    assignment_ids: Vec<String>,
    waiting: &mut Vec<ChatWaitingAssignmentRead>,
    accepted_by: Option<&ChatDeviceScope>,
) -> ChatResult<()> {
    for assignment_id in assignment_ids {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        if let Some(scope) = accepted_by {
            let mut transaction = pool.begin().await.map_err(persistence_error)?;
            let accepted =
                assignment_subject_is_accepted(&mut transaction, &assignment_id, scope).await?;
            transaction.rollback().await.map_err(persistence_error)?;
            if accepted {
                continue;
            }
        }
        if read_acceptance_offer(pool, &assignment_id).await?.is_none() {
            continue;
        }
        if let Some(row) = read_waiting_assignment(pool, &assignment_id).await? {
            waiting.push(row);
        }
    }
    Ok(())
}

/// Reads the work that waits for a local action on `device`, across every reply thread and
/// newest first, so it reaches the user without opening each thread.
pub(super) async fn waiting_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<Vec<ChatWaitingAssignmentRead>> {
    if !device.runs_new_work() {
        return Ok(Vec::new());
    }
    let mut waiting = Vec::new();
    let mut after_id = String::new();
    loop {
        let assignment_ids =
            unstarted_work_ids(pool, device, None, &after_id, TAKEOVER_BATCH_SIZE).await?;
        let batch_len = assignment_ids.len();
        if let Some(last) = assignment_ids.last() {
            after_id.clone_from(last);
        }
        append_waiting_rows(pool, assignment_ids, &mut waiting, None).await?;
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE) {
            break;
        }
    }
    after_id.clear();
    loop {
        let assignment_ids =
            stranded_follow_up_ids(pool, device, None, &after_id, TAKEOVER_BATCH_SIZE).await?;
        let batch_len = assignment_ids.len();
        if let Some(last) = assignment_ids.last() {
            after_id.clone_from(last);
        }
        append_waiting_rows(pool, assignment_ids, &mut waiting, Some(scope)).await?;
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE) {
            break;
        }
    }
    waiting.sort_by(|left, right| {
        right
            .created_at
            .as_str()
            .cmp(left.created_at.as_str())
            .then_with(|| {
                left.assignment_id
                    .as_str()
                    .cmp(right.assignment_id.as_str())
            })
    });
    waiting.truncate(WAITING_ASSIGNMENT_LIMIT);
    Ok(waiting)
}

/// Reads the work that waits for a local action on this device.
pub(super) async fn list_waiting_assignments(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<Vec<ChatWaitingAssignmentRead>> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let writers = chat_writers(&app, &pool).await?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    waiting_assignments(
        &pool,
        PostingDevice::this_device(&device_id, &writers),
        &scope,
    )
    .await
}

/// Whether `assignment_id` waits for a local action to start on `device`: it is queued with no
/// local dispatch, on `device` or on a device that can no longer write Chat, or it holds pending
/// replies that recovery leaves in place because `scope` did not accept its current subject.
pub(super) async fn assignment_awaits_local_acceptance(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<bool> {
    if !device.runs_new_work() {
        return Ok(false);
    }
    if !unstarted_work_ids(pool, device, Some(assignment_id), "", 1)
        .await?
        .is_empty()
    {
        return Ok(true);
    }
    if stranded_follow_up_ids(pool, device, Some(assignment_id), "", 1)
        .await?
        .is_empty()
    {
        return Ok(false);
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let accepted = assignment_subject_is_accepted(&mut transaction, assignment_id, scope).await?;
    transaction.rollback().await.map_err(persistence_error)?;
    Ok(!accepted)
}

/// Fills what the assignment of `page` offers when it waits for a local action on this device.
pub(super) async fn mark_local_acceptance(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    mut page: ChatReplyThreadPageRead,
) -> ChatResult<ChatReplyThreadPageRead> {
    if let Some(assignment) = page.assignment.as_mut() {
        let device_id = this_device_id(app)?;
        let writers = chat_writers(app, pool).await?;
        let scope = read_active_device_scope(app).map_err(device_state_error)?;
        let awaits = assignment_awaits_local_acceptance(
            pool,
            &assignment.id,
            PostingDevice::this_device(&device_id, &writers),
            &scope,
        )
        .await?;
        if awaits {
            assignment.local_acceptance = read_acceptance_offer(pool, &assignment.id).await?;
        }
    }
    Ok(page)
}

/// Records on this device the acceptance of what `assignment_id` would run.
async fn record_local_acceptance(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let digest = assignment_acceptance_digest(pool, assignment_id).await?;
    update_active_device_scope(app, |scope| {
        record_assignment_acceptance(scope, digest, now.clone());
        Ok(())
    })
    .map_err(device_state_error)
}

/// Records the acceptance a local post, Start, or Retry gives to what `assignment_id` runs, so
/// its replies replay after a restart until a synced change alters that subject. The gesture
/// already queued its work, so a failure only makes those replies ask again.
pub(super) async fn record_gesture_acceptance(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) {
    let recorded = match now_timestamp() {
        Ok(now) => record_local_acceptance(app, pool, assignment_id, &now).await,
        Err(error) => Err(error),
    };
    if let Err(error) = recorded {
        eprintln!(
            "Chat assignment {} was not recorded as accepted: {error}",
            assignment_id.as_str()
        );
    }
}

/// Records on this device the acceptance `digest` that a post gives to the work
/// `assignment_id` it dispatched. The post already queued that work, so a failure only makes its
/// replies ask again.
pub(super) fn record_post_acceptance(
    app: &tauri::AppHandle,
    assignment_id: &ChatWorkAssignmentId,
    digest: String,
) {
    let recorded = now_timestamp().and_then(|now| {
        update_active_device_scope(app, |scope| {
            record_assignment_acceptance(scope, digest, now);
            Ok(())
        })
        .map_err(device_state_error)
    });
    if let Err(error) = recorded {
        eprintln!(
            "Chat assignment {} was not recorded as accepted: {error}",
            assignment_id.as_str()
        );
    }
}

/// Starts on this device the assignment `assignment_id` that waits for a local action, and
/// records the acceptance of `seen_digest` so work with the same subject, queued now or later,
/// runs without asking. Pending replies move to a follow-up this device runs, and work stranded
/// on a device that can no longer write Chat moves to a successor this device runs. Returns
/// `None` with nothing recorded or started when the subject is no longer the one the user saw,
/// so the caller refreshes what it shows.
pub(super) async fn accept_assignment_here(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    seen_digest: String,
) -> ChatResult<Option<ChatWorkAssignmentRead>> {
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let writers = chat_writers(&app, &pool).await?;
    let device = PostingDevice::this_device(&device_id, &writers);
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    if !assignment_awaits_local_acceptance(&pool, &assignment_id, device, &scope).await? {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This work no longer waits to start on this device",
            true,
        ));
    }
    if !seen_digest_is_current(&pool, &assignment_id, &seen_digest).await? {
        return Ok(None);
    }
    let now = now_timestamp()?;
    update_active_device_scope(&app, |scope| {
        record_assignment_acceptance(scope, seen_digest, now.clone());
        Ok(())
    })
    .map_err(device_state_error)?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let waiting = read_assignment(&pool, &assignment_id).await?;
    let executor = read_assignment_executing_device(&pool, assignment_id.as_str()).await?;
    let started_id = if waiting.state == ChatWorkAssignmentState::ReadyForReview {
        recover_follow_up(&pool, &assignment_id, device, &scope, &now)
            .await?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "This work changed before it could start on this device",
                    true,
                )
            })?
    } else if executor.as_deref() == Some(device_id.as_str()) {
        if !adopt_synced_assignment(&pool, &assignment_id, &device_id, &scope).await? {
            return Ok(None);
        }
        assignment_id
    } else {
        take_over_routed_assignment(
            &pool,
            &assignment_id,
            &waiting.teammate.id,
            device,
            Some(&scope),
            &now,
        )
        .await?
    };
    let started = read_assignment(&pool, &started_id).await?;
    let worker_app = app.clone();
    let worker_db_url = db_url.clone();
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_assignment_job(worker_app, worker_db_url, started_id).await;
    });
    tauri::async_runtime::spawn(async move {
        if let Err(error) = recover_assignment_dispatch_jobs(app, db_url).await {
            eprintln!("Chat work accepted with this subject could not start: {error}");
        }
    });
    Ok(Some(started))
}

/// Checks the synced assignment `assignment_id` and queues its dispatch when `scope` accepted
/// what it would run, reporting whether it queued.
async fn queue_accepted_dispatch(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    scope: &ChatDeviceScope,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let digest = read_assignment_subject(transaction, assignment_id)
        .await?
        .digest();
    if !assignment_is_accepted(scope, &digest) {
        return Ok(false);
    }
    queue_dispatch_here(transaction, assignment_id, now).await?;
    Ok(true)
}

/// Queues the dispatch of the synced queued assignment `assignment_id` when the device
/// `device_id` executes it, has not queued it yet, and `scope` accepted what it would run,
/// reporting whether it did. Unaccepted work, and work of an archived teammate, stays queued
/// and visible, so a restore makes it actionable again. An assignment whose
/// references do not hold or whose context cannot be frozen here fails with a reason instead of
/// staying queued.
pub(super) async fn adopt_synced_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device_id: &str,
    scope: &ChatDeviceScope,
) -> ChatResult<bool> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let adoptable: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_assignments assignment
           WHERE assignment.id = ? AND assignment.state = 'queued'
             AND assignment.executing_device_id = ?
             AND NOT EXISTS (
               SELECT 1 FROM chat_assignment_dispatch_jobs job
               WHERE job.assignment_id = assignment.id
             )
             AND NOT EXISTS (
               SELECT 1 FROM chat_participants participant
               WHERE participant.id = assignment.teammate_id
                 AND participant.archived_at IS NOT NULL
             )
         )",
    )
    .bind(assignment_id.as_str())
    .bind(device_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if !adoptable {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(false);
    }
    match queue_accepted_dispatch(&mut transaction, assignment_id, scope, &now).await {
        Ok(true) => {
            transaction.commit().await.map_err(persistence_error)?;
            Ok(true)
        }
        Ok(false) => {
            transaction.rollback().await.map_err(persistence_error)?;
            Ok(false)
        }
        Err(error) => {
            transaction.rollback().await.map_err(persistence_error)?;
            eprintln!(
                "Synced Chat assignment {} could not start: {error}",
                assignment_id.as_str()
            );
            sqlx::query(
                "UPDATE chat_work_assignments
                 SET state = 'failed', state_reason = ?, settled_at = ?,
                     revision = revision + 1, updated_at = ?
                 WHERE id = ? AND state = 'queued'",
            )
            .bind(ADOPTION_FAILED_REASON)
            .bind(now.as_str())
            .bind(now.as_str())
            .bind(assignment_id.as_str())
            .execute(pool)
            .await
            .map_err(persistence_error)?;
            Ok(false)
        }
    }
}

/// Starts the assignment work sync handed to this device: queued assignments it executes
/// dispatch, and pending inputs reach the runs it holds. Work for other devices is left alone.
pub(crate) fn start_synced_assignment_work(app: &tauri::AppHandle, work: SyncedAssignmentWork) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_synced_assignment_work(app, work).await {
            eprintln!("Synced Chat assignment work did not start: {error}");
        }
    });
}

/// Starts the assignment an unattended post created on this device. It takes the path of synced
/// work, so it dispatches only when its teammate already holds a local acceptance.
pub(super) fn start_unattended_assignment(
    app: &tauri::AppHandle,
    assignment_id: &ChatWorkAssignmentId,
) {
    let work = SyncedAssignmentWork {
        queued: BTreeSet::from([assignment_id.as_str().to_owned()]),
        ..SyncedAssignmentWork::default()
    };
    start_synced_assignment_work(app, work);
}

/// Reason recorded on this device's authorizations of an assignment whose concurrent starts
/// merged onto another executing device.
pub(super) const MOVED_EXECUTOR_REASON: &str = "The work assignment runs on another device";

/// Stops this device's work on `assignment_id` when sync merged it onto another executing
/// device, as when two devices started the same mention concurrently: it ends the device-local
/// dispatch job and revokes the device-local authorizations with their revocation jobs, so the
/// drain stops the run this device started. Both are device-local, so the executing device keeps
/// its run. Returns whether anything ended.
pub(super) async fn release_moved_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device_id: &str,
) -> ChatResult<bool> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let moved: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_work_assignments
           WHERE id = ? AND executing_device_id IS NOT NULL AND executing_device_id <> ?
         )",
    )
    .bind(assignment_id.as_str())
    .bind(device_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if !moved {
        transaction.rollback().await.map_err(persistence_error)?;
        return Ok(false);
    }
    let ended_jobs = sqlx::query(
        "UPDATE chat_assignment_dispatch_jobs SET state = 'cancelled', updated_at = ?
         WHERE assignment_id = ? AND state IN ('queued', 'claimed')",
    )
    .bind(now.as_str())
    .bind(assignment_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    let revoked = ganbaru_chat::coordination::revocation::revoke_assignment_authorizations(
        &mut transaction,
        assignment_id.as_str(),
        now.as_str(),
        MOVED_EXECUTOR_REASON,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ended_jobs > 0 || revoked > 0)
}

async fn run_synced_assignment_work(
    app: tauri::AppHandle,
    work: SyncedAssignmentWork,
) -> ChatResult<()> {
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let scope = read_active_device_scope(&app).map_err(device_state_error)?;
    let mut released = false;
    for assignment_id in work.queued.iter().chain(&work.started) {
        let assignment_id =
            ChatWorkAssignmentId::new(assignment_id.clone()).map_err(identifier_error)?;
        released |= release_moved_assignment(&pool, &assignment_id, &device_id).await?;
    }
    if released {
        crate::chat::sync::RevocationWake::wake_revocation_drain(&app);
    }
    for assignment_id in work.queued {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        if adopt_synced_assignment(&pool, &assignment_id, &device_id, &scope).await? {
            dispatch_assignment_job(app.clone(), db_url.clone(), assignment_id).await?;
        }
    }
    for assignment_id in work.routed {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        if let Err(error) =
            deliver_pending_assignment_inputs(app.clone(), db_url.clone(), &pool, &assignment_id)
                .await
        {
            eprintln!(
                "Synced input of Chat assignment {} was not delivered: {error}",
                assignment_id.as_str()
            );
        }
    }
    Ok(())
}

/// Starts the work of the mention `message_item_id` of `teammate_id` on this device: the Start
/// action for a mention whose post recorded no assignment, since its teammate has no home
/// device and the posting device does not execute assignments.
pub(super) async fn start_mentioned_assignment(
    app: tauri::AppHandle,
    db_url: String,
    message_item_id: ChatConversationItemId,
    teammate_id: ChatParticipantId,
) -> ChatResult<ChatWorkAssignmentRead> {
    let dispatch_app = app.clone();
    let dispatch_db_url = db_url.clone();
    let device_id = this_device_id(&app)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    let writers = chat_writers(&app, &pool).await?;
    let now = now_timestamp()?;
    let assignment_id = persist_started_assignment(
        &pool,
        &message_item_id,
        &teammate_id,
        PostingDevice::this_device(&device_id, &writers),
        &now,
    )
    .await?;
    record_gesture_acceptance(&app, &pool, &assignment_id).await;
    let started = read_assignment(&pool, &assignment_id).await?;
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_assignment_job(dispatch_app, dispatch_db_url, assignment_id).await;
    });
    Ok(started)
}

/// Writes the assignment the Start action creates for the mention `message_item_id` of
/// `teammate_id`, executed by `device`. A mention that already routed to work starts again
/// only when that work's executing device can no longer write Chat: its work then moves to a
/// successor `device` runs.
pub(super) async fn persist_started_assignment(
    pool: &SqlitePool,
    message_item_id: &ChatConversationItemId,
    teammate_id: &ChatParticipantId,
    device: PostingDevice<'_>,
    now: &UtcTimestamp,
) -> ChatResult<ChatWorkAssignmentId> {
    let row = sqlx::query(
        "SELECT item.conversation_id,
                coalesce(item.reply_thread_id, (
                  SELECT thread.id FROM chat_reply_threads thread
                  WHERE thread.root_item_id = item.id
                )) AS reply_thread_id,
                EXISTS(
                  SELECT 1
                  FROM chat_communication_messages message
                  JOIN chat_message_references reference
                    ON reference.message_revision_id = message.current_revision_id
                  JOIN chat_participant_reference_targets target
                    ON target.reference_id = reference.id
                  WHERE message.item_id = item.id AND target.participant_id = ?
                ) AS mentions_teammate,
                (
                  SELECT input.assignment_id FROM chat_work_assignment_inputs input
                  WHERE input.message_item_id = item.id
                ) AS routed_assignment_id
         FROM chat_conversation_items item WHERE item.id = ?",
    )
    .bind(teammate_id.as_str())
    .bind(message_item_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| ChatError::new(ChatErrorCode::NotFound, "Message not found", true))?;
    let reply_thread_id: Option<String> =
        row.try_get("reply_thread_id").map_err(persistence_error)?;
    let (Some(reply_thread_id), true) = (
        reply_thread_id,
        row.try_get::<bool, _>("mentions_teammate")
            .map_err(persistence_error)?,
    ) else {
        return Err(ChatError::validation(
            "messageItemId",
            "This message does not mention the teammate in a reply thread",
        ));
    };
    let routed_assignment_id: Option<String> = row
        .try_get("routed_assignment_id")
        .map_err(persistence_error)?;
    let reply_thread_id = ChatReplyThreadId::new(reply_thread_id).map_err(identifier_error)?;
    let conversation_id = ChatConversationId::new(
        row.try_get::<String, _>("conversation_id")
            .map_err(persistence_error)?,
    )
    .map_err(identifier_error)?;
    require_participating_teammate(pool, &conversation_id, teammate_id).await?;
    if let Some(routed_assignment_id) = routed_assignment_id {
        let routed_assignment_id =
            ChatWorkAssignmentId::new(routed_assignment_id).map_err(identifier_error)?;
        return take_over_routed_assignment(
            pool,
            &routed_assignment_id,
            teammate_id,
            device,
            None,
            now,
        )
        .await;
    }
    let latest = read_active_or_latest_assignment(pool, &reply_thread_id).await?;
    if latest
        .as_ref()
        .is_some_and(|assignment| assignment.state.accepts_continuation())
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This reply thread already has active work",
            true,
        ));
    }
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let assignment_id = create_assignment(
        &mut transaction,
        NewAssignment {
            reply_thread_id: &reply_thread_id,
            message_item_id,
            teammate_id,
            latest_assignment: latest.as_ref(),
            executing_device_id: device.id,
            execution_target: None,
        },
        device,
        now,
    )
    .await?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(assignment_id)
}

/// Moves the work of `assignment_id`, which a mention of `teammate_id` already routed to, onto
/// a successor `device` runs. Only work that is still active and whose executing device can no
/// longer write Chat moves; finished work and work a Chat writer runs refuse the Start. With
/// `accepted_by`, the move also requires that scope to accept what the work would run when the
/// move commits.
async fn take_over_routed_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    teammate_id: &ChatParticipantId,
    device: PostingDevice<'_>,
    accepted_by: Option<&ChatDeviceScope>,
    now: &UtcTimestamp,
) -> ChatResult<ChatWorkAssignmentId> {
    let routed = read_assignment(pool, assignment_id).await?;
    let executor = read_assignment_executing_device(pool, assignment_id.as_str()).await?;
    if routed.teammate.id != *teammate_id
        || !routed.state.accepts_continuation()
        || !device.runs_new_work()
        || !stranded_executor(executor.as_deref(), device)
    {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This mention already started work",
            true,
        ));
    }
    let moved_conflict = || {
        ChatError::new(
            ChatErrorCode::Conflict,
            "This work changed before it moved to this device",
            true,
        )
    };
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if let Some(scope) = accepted_by {
        if !assignment_subject_is_accepted(&mut transaction, assignment_id, scope).await? {
            return Err(moved_conflict());
        }
    }
    let successor = reroute_assignment(&mut transaction, &routed, device, now)
        .await?
        .ok_or_else(moved_conflict)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(successor)
}

//! Work assignment cancellation, retry, and dispatch recovery.

use super::common::new_id;
use super::dispatch::{deliver_pending_assignment_inputs, dispatch_assignment_job};
use super::reads::read_assignment;
use super::workflow::{ResolvedInvocation, persist_assignment_routing, set_assignment_state};
use super::{chat_pool, i64_value, identifier_error, now_timestamp, persistence_error};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

pub(super) async fn recover_assignment_dispatch_jobs(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<u32> {
    let pool = chat_pool(app.clone(), db_url.clone()).await?;
    let now = now_timestamp()?;
    recover_stranded_follow_up_assignments(&pool, &now).await?;
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

pub(super) async fn recover_stranded_follow_up_assignments(
    pool: &SqlitePool,
    now: &UtcTimestamp,
) -> ChatResult<u32> {
    let assignment_ids = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT assignment.id
         FROM chat_work_assignments assignment
         JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
         WHERE assignment.state = 'ready_for_review'
           AND input.routing_kind IN ('steer', 'queued_continuation')
           AND input.delivery_state = 'pending'
         ORDER BY assignment.updated_at, assignment.id LIMIT 32",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut recovered = 0_u32;
    for assignment_id in assignment_ids {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        let previous = read_assignment(pool, &assignment_id).await?;
        let reply_thread_id = previous.reply_thread_id.clone();
        let mut transaction = pool.begin().await.map_err(persistence_error)?;
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
            continue;
        };
        let first_message_id = ChatConversationItemId::new(
            first
                .try_get::<String, _>("message_item_id")
                .map_err(persistence_error)?,
        )
        .map_err(identifier_error)?;
        for input in &stranded {
            sqlx::query(
                "DELETE FROM chat_work_assignment_inputs
                 WHERE id = ? AND assignment_id = ? AND delivery_state = 'pending'",
            )
            .bind(
                input
                    .try_get::<String, _>("id")
                    .map_err(persistence_error)?,
            )
            .bind(assignment_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        }
        let invocation = ResolvedInvocation {
            teammate_id: previous.teammate.id.clone(),
            active_assignment: None,
            latest_assignment: Some(previous),
        };
        let write = persist_assignment_routing(
            &mut transaction,
            &reply_thread_id,
            &first_message_id,
            Some(&invocation),
            None,
            now,
        )
        .await?;
        let recovered_assignment_id = write.assignment_id.ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Recovered follow-up did not create a work assignment",
                true,
            )
        })?;
        for (index, input) in stranded.iter().enumerate().skip(1) {
            let ordinal = i64::try_from(index + 1).map_err(|_| {
                ChatError::new(
                    ChatErrorCode::Validation,
                    "Too many stranded follow-up messages",
                    true,
                )
            })?;
            sqlx::query(
                "INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
                 VALUES (?, ?, ?, ?, 'queued_continuation', ?)",
            )
            .bind(new_id("assignment-input"))
            .bind(recovered_assignment_id.as_str())
            .bind(
                input
                    .try_get::<String, _>("message_item_id")
                    .map_err(persistence_error)?,
            )
            .bind(ordinal)
            .bind(now.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        }
        transaction.commit().await.map_err(persistence_error)?;
        recovered = recovered.saturating_add(1);
    }
    Ok(recovered)
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

pub(super) async fn retry_assignment(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    expected_revision: u64,
) -> ChatResult<ChatWorkAssignmentRead> {
    let dispatch_app = app.clone();
    let dispatch_db_url = db_url.clone();
    let pool = chat_pool(app, db_url).await?;
    let current = read_assignment(&pool, &assignment_id).await?;
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
    let now = now_timestamp()?;
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
    sqlx::query(
        "INSERT INTO chat_assignment_dispatch_jobs
            (id, assignment_id, state, available_at, created_at, updated_at)
         VALUES (?, ?, 'queued', ?, ?, ?)
         ON CONFLICT(assignment_id) DO UPDATE SET
            state = 'queued', available_at = excluded.available_at,
            claimed_at = NULL, claim_token = NULL, last_error = NULL,
            updated_at = excluded.updated_at",
    )
    .bind(new_id("dispatch"))
    .bind(assignment_id.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    let retried = read_assignment(&pool, &assignment_id).await?;
    tauri::async_runtime::spawn(async move {
        let _ = dispatch_assignment_job(dispatch_app, dispatch_db_url, assignment_id).await;
    });
    Ok(retried)
}

//! Acceptance of work on this device: the subject an acceptance covers, its digest, adoption of
//! accepted queued work, the acceptance offer, the waiting list, and recording local acceptance.

use super::super::common::{
    chat_writers, parse_folder_capability, parse_runtime_approval_policy, this_device_id,
};
use super::super::context::{GrantedAccess, read_granted_access};
use super::super::continuation::model_selection_sha256;
use super::super::dispatch::dispatch_assignment_job;
use super::super::provider_agents::{DeviceProviders, read_device_providers};
use super::super::reads::read_assignment;
use super::super::workflow::{
    PostingDevice, parse_model_selection, read_assignment_executing_device, take_requested_target,
};
use super::super::{chat_pool, identifier_error, now_timestamp, persistence_error, timestamp};
use super::{
    TAKEOVER_BATCH_SIZE, deliver_delegation_outcomes_of_assignments, queue_dispatch_here,
    recover_assignment_dispatch_jobs, recover_follow_up, skip_recovery_row, stranded_follow_up_ids,
    take_over_routed_assignment,
};
use crate::chat::device_state::{
    ChatDeviceScope, assignment_is_accepted, read_active_device_scope,
    record_assignment_acceptance, update_active_device_scope,
};
use crate::chat::settings::device_state_error;
use ganbaru_chat::coordination::acceptance::{AcceptanceSubject, acceptance_digest};
use ganbaru_chat::coordination::access::instructions_digest;
use ganbaru_chat::coordination::delegation::post_delegation_outcome;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::collections::BTreeSet;

/// Reason a synced assignment records when its executing device cannot freeze its context.
pub(in crate::chat::organization) const ADOPTION_FAILED_REASON: &str =
    "The work could not start on its executing device";

/// What `assignment_id` would run on this device: the access its channel resolves to here, and
/// the model and instructions it froze when it was created.
pub(in crate::chat::organization) struct AssignmentSubject {
    agent_id: ChatParticipantId,
    pub(in crate::chat::organization) display_name: String,
    /// The vault, project, and channel instructions the assignment froze.
    pub(in crate::chat::organization) instructions: String,
    /// The model the assignment froze.
    pub(in crate::chat::organization) model_selection: ChatModelSelection,
    model_selection_sha256: String,
    access: GrantedAccess,
}

impl AssignmentSubject {
    /// The provider installation that runs the agent on this device, empty when none is
    /// configured.
    pub(in crate::chat::organization) fn provider_instance_id(&self) -> &str {
        &self.access.provider_instance_id
    }

    pub(in crate::chat::organization) fn digest(&self) -> String {
        acceptance_digest(&AcceptanceSubject {
            agent_id: self.agent_id.as_str(),
            access_level: self.access.access_level,
            instructions_sha256: &instructions_digest(&self.instructions),
            model_selection_sha256: &self.model_selection_sha256,
            provider_instance_id: &self.access.provider_instance_id,
            grants: &self.access.grants,
            scratch_runtime_approval: self.access.scratch_runtime_approval,
        })
    }
}

pub(in crate::chat::organization) async fn read_assignment_subject(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
) -> ChatResult<AssignmentSubject> {
    let row = sqlx::query(
        "SELECT assignment.agent_id, assignment.triggering_message_item_id,
                assignment.model_selection_data, assignment.instructions,
                thread.conversation_id, participant.display_name
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_participants participant ON participant.id = assignment.agent_id
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
    let agent_id = ChatParticipantId::new(
        row.try_get::<String, _>("agent_id")
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
    let model_selection = parse_model_selection(
        row.try_get("model_selection_data")
            .map_err(persistence_error)?,
    )?;
    let model_selection_sha256 = model_selection_sha256(
        model_selection.provider_managed_model,
        model_selection.model_id.as_ref(),
        &model_selection.model_options,
    )?;
    let access = read_granted_access(
        transaction,
        &conversation_id,
        &message_item_id,
        &agent_id,
        providers,
    )
    .await?;
    Ok(AssignmentSubject {
        agent_id,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        instructions: row.try_get("instructions").map_err(persistence_error)?,
        model_selection,
        model_selection_sha256,
        access,
    })
}

/// Whether the synced references of `assignment_id` hold together: its trigger belongs to its
/// reply thread and its agent still participates in the conversation.
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
            AND membership.participant_id = assignment.agent_id
            AND membership.removed_at IS NULL
           JOIN chat_participants participant
             ON participant.id = assignment.agent_id AND participant.archived_at IS NULL
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
        "This work names a message or agent outside its reply thread",
        true,
    )
}

/// Whether `scope` accepted what `assignment_id` would run. Work that cannot run here counts as
/// not accepted, so one bad row does not stop a recovery pass.
pub(super) async fn assignment_subject_is_accepted(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    scope: &ChatDeviceScope,
    providers: &DeviceProviders,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Ok(false);
    }
    match read_assignment_subject(transaction, assignment_id, providers).await {
        Ok(subject) => Ok(assignment_is_accepted(scope, &subject.digest())),
        Err(error) if error.code == ChatErrorCode::Persistence => Err(error),
        Err(_) => Ok(false),
    }
}

/// Hex acceptance digest of what `assignment_id` would run.
pub(in crate::chat::organization) async fn assignment_acceptance_digest(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
) -> ChatResult<String> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if !assignment_references_hold(&mut transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let digest = read_assignment_subject(&mut transaction, assignment_id, providers)
        .await?
        .digest();
    transaction.rollback().await.map_err(persistence_error)?;
    Ok(digest)
}

/// Whether `seen_digest` still names what `assignment_id` would run.
pub(in crate::chat::organization) async fn seen_digest_is_current(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
    seen_digest: &str,
) -> ChatResult<bool> {
    Ok(assignment_acceptance_digest(pool, assignment_id, providers).await? == seen_digest)
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
pub(in crate::chat::organization) async fn adopt_accepted_queued_assignments(
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
        for raw_id in assignment_ids {
            let outcome = match ChatWorkAssignmentId::new(raw_id.as_str()) {
                Ok(assignment_id) => {
                    adopt_accepted_queued_assignment(pool, &assignment_id, device, scope)
                        .await
                        .map(|adopted| adopted.then_some(assignment_id))
                }
                Err(error) => Err(identifier_error(error)),
            };
            match outcome {
                Ok(Some(assignment_id)) => adopted.push(assignment_id),
                Ok(None) => {}
                Err(error) => skip_recovery_row("Chat queued work adoption", &raw_id, &error),
            }
            after_id = raw_id;
        }
        if batch_len < usize::from(TAKEOVER_BATCH_SIZE) {
            return Ok(adopted);
        }
    }
}

/// Adopts the queued synced assignment `assignment_id` when `scope` accepted its subject,
/// reporting whether it queued a local dispatch.
async fn adopt_accepted_queued_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<bool> {
    let providers = device.chat_writers.providers();
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let accepted =
        assignment_subject_is_accepted(&mut transaction, assignment_id, scope, providers).await?;
    transaction.rollback().await.map_err(persistence_error)?;
    if !accepted {
        return Ok(false);
    }
    adopt_synced_assignment(pool, assignment_id, providers, device.id, scope).await
}

/// The folders, provider, and approvals behind the acceptance digest of `assignment_id`.
async fn build_acceptance_offer(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
) -> ChatResult<ChatAssignmentAcceptanceOfferRead> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let subject = read_assignment_subject(transaction, assignment_id, providers).await?;
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
    let access = subject.access.effective.ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Permission,
            "This agent cannot work on this device",
            true,
        )
    })?;
    Ok(ChatAssignmentAcceptanceOfferRead {
        digest: subject.digest(),
        provider_instance_id: subject.access.provider_instance_id.clone(),
        access,
        folders,
    })
}

/// Reads what the user would accept by starting `assignment_id`. Work that cannot run here has
/// no offer, since Start refuses it; a failing read still surfaces.
pub(in crate::chat::organization) async fn read_acceptance_offer(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
) -> ChatResult<Option<ChatAssignmentAcceptanceOfferRead>> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let offer = build_acceptance_offer(&mut transaction, assignment_id, providers).await;
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
                participant.display_name AS agent_display_name
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         JOIN chat_participants participant ON participant.id = assignment.agent_id
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
        agent_display_name: row
            .try_get("agent_display_name")
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
    providers: &DeviceProviders,
) -> ChatResult<()> {
    for assignment_id in assignment_ids {
        let assignment_id = ChatWorkAssignmentId::new(assignment_id).map_err(identifier_error)?;
        if let Some(scope) = accepted_by {
            let mut transaction = pool.begin().await.map_err(persistence_error)?;
            let accepted =
                assignment_subject_is_accepted(&mut transaction, &assignment_id, scope, providers)
                    .await?;
            transaction.rollback().await.map_err(persistence_error)?;
            if accepted {
                continue;
            }
        }
        if read_acceptance_offer(pool, &assignment_id, providers)
            .await?
            .is_none()
        {
            continue;
        }
        if let Some(row) = read_waiting_assignment(pool, &assignment_id).await? {
            waiting.push(row);
        }
    }
    Ok(())
}

/// Reads the work that waits for a local action on `device`, across every reply thread and
/// newest first, so it reaches the user without opening each thread. The list is capped at
/// [`WAITING_ASSIGNMENT_LIMIT`] and `total` counts every waiting assignment.
pub(in crate::chat::organization) async fn waiting_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<ChatWaitingAssignmentsRead> {
    let providers = device.chat_writers.providers();
    if !device.runs_new_work() {
        return Ok(ChatWaitingAssignmentsRead {
            items: Vec::new(),
            total: 0,
        });
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
        append_waiting_rows(pool, assignment_ids, &mut waiting, None, providers).await?;
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
        append_waiting_rows(pool, assignment_ids, &mut waiting, Some(scope), providers).await?;
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
    let total = u32::try_from(waiting.len()).unwrap_or(u32::MAX);
    waiting.truncate(WAITING_ASSIGNMENT_LIMIT);
    Ok(ChatWaitingAssignmentsRead {
        items: waiting,
        total,
    })
}

/// Reads the work that waits for a local action on this device.
pub(in crate::chat::organization) async fn list_waiting_assignments(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatWaitingAssignmentsRead> {
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
pub(in crate::chat::organization) async fn assignment_awaits_local_acceptance(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    device: PostingDevice<'_>,
    scope: &ChatDeviceScope,
) -> ChatResult<bool> {
    let providers = device.chat_writers.providers();
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
    let accepted =
        assignment_subject_is_accepted(&mut transaction, assignment_id, scope, providers).await?;
    transaction.rollback().await.map_err(persistence_error)?;
    Ok(!accepted)
}

/// Fills what the assignment of `page` offers when it waits for a local action on this device.
pub(in crate::chat::organization) async fn mark_local_acceptance(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    mut page: ChatReplyThreadPageRead,
) -> ChatResult<ChatReplyThreadPageRead> {
    let providers = &read_device_providers(app);
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
            assignment.local_acceptance =
                read_acceptance_offer(pool, &assignment.id, providers).await?;
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
    let providers = &read_device_providers(app);
    let digest = assignment_acceptance_digest(pool, assignment_id, providers).await?;
    update_active_device_scope(app, |scope| {
        record_assignment_acceptance(scope, digest, now.clone());
        Ok(())
    })
    .map_err(device_state_error)
}

/// Records the acceptance a local post, Start, or Retry gives to what `assignment_id` runs, so
/// its replies replay after a restart until a synced change alters that subject. The gesture
/// already queued its work, so a failure only makes those replies ask again.
pub(in crate::chat::organization) async fn record_gesture_acceptance(
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
pub(in crate::chat::organization) fn record_post_acceptance(
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
pub(in crate::chat::organization) async fn accept_assignment_here(
    app: tauri::AppHandle,
    db_url: String,
    assignment_id: ChatWorkAssignmentId,
    seen_digest: String,
) -> ChatResult<Option<ChatWorkAssignmentRead>> {
    let providers = &read_device_providers(&app);
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
    if !seen_digest_is_current(&pool, &assignment_id, providers, &seen_digest).await? {
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
        if !adopt_synced_assignment(&pool, &assignment_id, providers, &device_id, &scope).await? {
            deliver_delegation_outcomes_of_assignments(
                &app,
                &pool,
                BTreeSet::from([assignment_id.as_str().to_owned()]),
            );
            return Ok(None);
        }
        assignment_id
    } else {
        take_over_routed_assignment(
            &pool,
            &assignment_id,
            &waiting.agent.id,
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
    providers: &DeviceProviders,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let digest = read_assignment_subject(transaction, assignment_id, providers)
        .await?
        .digest();
    if !assignment_is_accepted(scope, &digest) {
        return Ok(false);
    }
    queue_dispatch_here(transaction, assignment_id, providers, now).await?;
    Ok(true)
}

/// Queues the dispatch of the synced queued assignment `assignment_id` when the device
/// `device_id` executes it, has not queued it yet, and `scope` accepted what it would run,
/// reporting whether it did. Unaccepted work, and work of an archived agent, stays queued
/// and visible, so a restore makes it actionable again. An assignment whose
/// references do not hold or whose context cannot be frozen here fails with a reason instead of
/// staying queued.
pub(in crate::chat::organization) async fn adopt_synced_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
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
               WHERE participant.id = assignment.agent_id
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
    match queue_accepted_dispatch(&mut transaction, assignment_id, scope, providers, &now).await {
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
            let mut transaction = pool.begin().await.map_err(persistence_error)?;
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
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            // A chosen target that no longer resolves would fail every Retry, so Retry
            // resolves the target again from the current grants.
            take_requested_target(&mut transaction, assignment_id).await?;
            post_delegation_outcome(&mut transaction, assignment_id.as_str(), now.as_str()).await?;
            transaction.commit().await.map_err(persistence_error)?;
            Ok(false)
        }
    }
}

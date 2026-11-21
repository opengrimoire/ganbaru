//! Adoption of queued work on this device: the subject an assignment would run, whether it can
//! run here, and the dispatch of synced queued work this device executes.

use super::super::context::{GrantedAccess, read_granted_access};
use super::super::dispatch::queued_assignment_failure_sql;
use super::super::provider_agents::DeviceProviders;
use super::super::workflow::{PostingDevice, parse_model_selection};
use super::super::{identifier_error, now_timestamp, persistence_error};
use super::{TAKEOVER_BATCH_SIZE, queue_dispatch_here, skip_recovery_row};
use ganbaru_chat::coordination::delegation::post_delegation_outcome;
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

/// Reason a synced assignment records when its executing device cannot freeze its context.
pub(in crate::chat::organization) const ADOPTION_FAILED_REASON: &str =
    "The work could not start on its executing device";

/// What `assignment_id` would run on this device: the access its channel resolves to here, and
/// the model and instructions it froze when it was created.
pub(in crate::chat::organization) struct AssignmentSubject {
    pub(in crate::chat::organization) display_name: String,
    /// The vault, project, and channel instructions the assignment froze.
    pub(in crate::chat::organization) instructions: String,
    /// The model the assignment froze.
    pub(in crate::chat::organization) model_selection: ChatModelSelection,
    access: GrantedAccess,
}

impl AssignmentSubject {
    /// The provider installation that runs the agent on this device, empty when none is
    /// configured.
    pub(in crate::chat::organization) fn provider_instance_id(&self) -> &str {
        &self.access.provider_instance_id
    }

    /// Whether the agent can run here: its channel lets it work on this device and a provider
    /// installation runs it.
    fn runs_here(&self) -> bool {
        self.access.effective.is_some() && !self.access.provider_instance_id.is_empty()
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
    let access = read_granted_access(
        transaction,
        &conversation_id,
        &message_item_id,
        &agent_id,
        providers,
    )
    .await?;
    Ok(AssignmentSubject {
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        instructions: row.try_get("instructions").map_err(persistence_error)?,
        model_selection,
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

/// Whether the agent of `assignment_id` can run it on this device: its references hold, its
/// channel lets it work here, and a provider installation runs it. Work that cannot run here
/// counts as not runnable, so one bad row does not stop a recovery pass; a failing read still
/// surfaces.
pub(super) async fn assignment_runs_here(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Ok(false);
    }
    match read_assignment_subject(transaction, assignment_id, providers).await {
        Ok(subject) => Ok(subject.runs_here()),
        Err(error) if error.code == ChatErrorCode::Persistence => Err(error),
        Err(_) => Ok(false),
    }
}

/// Reads, past `after_id` and up to `limit`, the queued assignments with no local dispatch that
/// `device` executes or could take over from a device that can no longer write Chat.
async fn unstarted_work_ids(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
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
           AND assignment.id > ?3
         ORDER BY assignment.id LIMIT ?4",
    )
    .bind(device.id)
    .bind(device.chat_writers.json()?)
    .bind(after_id)
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)
}

/// Queues the dispatch of the queued work `device` executes, returning the assignments it
/// queued. Work whose agent cannot run here stays queued and visible.
pub(in crate::chat::organization) async fn adopt_queued_assignments(
    pool: &SqlitePool,
    device: PostingDevice<'_>,
) -> ChatResult<Vec<ChatWorkAssignmentId>> {
    if !device.runs_new_work() {
        return Ok(Vec::new());
    }
    let providers = device.chat_writers.providers();
    let mut adopted = Vec::new();
    let mut after_id = String::new();
    loop {
        let assignment_ids =
            unstarted_work_ids(pool, device, &after_id, TAKEOVER_BATCH_SIZE).await?;
        let batch_len = assignment_ids.len();
        for raw_id in assignment_ids {
            let outcome = match ChatWorkAssignmentId::new(raw_id.as_str()) {
                Ok(assignment_id) => {
                    adopt_synced_assignment(pool, &assignment_id, providers, device.id)
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

/// Checks the synced assignment `assignment_id` and queues its dispatch when its agent can run
/// here, reporting whether it queued.
async fn queue_runnable_dispatch(
    transaction: &mut Transaction<'_, Sqlite>,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
    now: &UtcTimestamp,
) -> ChatResult<bool> {
    if !assignment_references_hold(transaction, assignment_id).await? {
        return Err(broken_references_error());
    }
    let runs_here = match read_assignment_subject(transaction, assignment_id, providers).await {
        Ok(subject) => subject.runs_here(),
        // A channel or provider that does not let the agent work here leaves the work queued.
        Err(error) if error.code == ChatErrorCode::Permission => false,
        Err(error) => return Err(error),
    };
    if !runs_here {
        return Ok(false);
    }
    queue_dispatch_here(transaction, assignment_id, providers, now).await?;
    Ok(true)
}

/// Queues the dispatch of the synced queued assignment `assignment_id` when the device
/// `device_id` executes it, has not queued it yet, and its agent can run here, reporting
/// whether it did. Work whose agent cannot run here, and work of an archived agent, stays
/// queued and visible, so configuring the agent or restoring it makes the work run. An
/// assignment whose references do not hold or whose context cannot be frozen here fails with a
/// reason instead of staying queued.
pub(in crate::chat::organization) async fn adopt_synced_assignment(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    providers: &DeviceProviders,
    device_id: &str,
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
    match queue_runnable_dispatch(&mut transaction, assignment_id, providers, &now).await {
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
            let fail = queued_assignment_failure_sql();
            sqlx::query(sqlx::AssertSqlSafe(fail.as_str()))
                .bind(ADOPTION_FAILED_REASON)
                .bind(now.as_str())
                .bind(now.as_str())
                .bind(assignment_id.as_str())
                .execute(&mut *transaction)
                .await
                .map_err(persistence_error)?;
            post_delegation_outcome(&mut transaction, assignment_id.as_str(), now.as_str()).await?;
            transaction.commit().await.map_err(persistence_error)?;
            Ok(false)
        }
    }
}

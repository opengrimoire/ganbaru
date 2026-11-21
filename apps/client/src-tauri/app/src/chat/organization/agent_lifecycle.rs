use super::*;
#[cfg(desktop)]
use ganbaru_sync_contracts::chat_agents::is_named_agent_key;
use sqlx::{Executor, Sqlite};
#[cfg(desktop)]
use sqlx::{SqlitePool, Transaction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AgentLifecycleState {
    pub active_assignment_count: u64,
}

/// Reads the work that keeps `agent_id` from being archived.
///
/// Started work counts wherever it runs. Queued work counts only when this device holds a
/// live (queued or claimed) dispatch job for it. Queued work with no live dispatch here has
/// not reached its executing device yet, or was moved to another executor, so a peer's row
/// cannot block the archive. It stays queued, does not start while the agent is archived, and
/// becomes actionable again after a restore.
pub(super) async fn read_agent_lifecycle<'e, E>(
    executor: E,
    agent_id: &ChatParticipantId,
) -> ChatResult<AgentLifecycleState>
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(
        "SELECT
            (SELECT count(*) FROM chat_work_assignments assignment
             WHERE assignment.agent_id = ?
               AND (
                 assignment.state IN (
                   'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'
                 )
                 OR (
                   assignment.state = 'queued'
                   AND EXISTS (
                     SELECT 1 FROM chat_assignment_dispatch_jobs job
                     WHERE job.assignment_id = assignment.id
                       AND job.state IN ('queued', 'claimed')
                   )
                 )
               )) AS active_assignment_count",
    )
    .bind(agent_id.as_str())
    .fetch_one(executor)
    .await
    .map_err(persistence_error)?;
    Ok(AgentLifecycleState {
        active_assignment_count: u64_value(
            row.try_get("active_assignment_count")
                .map_err(persistence_error)?,
        )?,
    })
}

#[cfg(desktop)]
pub(super) async fn set_agent_archived(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    expected_revision: u64,
    archived: bool,
    local_instances: &reads::LocalProviderInstances,
) -> ChatResult<ChatAiAgentRead> {
    let now = now_timestamp()?;
    let archived_at = archived.then(|| now.as_str());
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let updated = sqlx::query(
        "UPDATE chat_participants
         SET archived_at = ?1,
             hidden_at = CASE WHEN ?1 IS NULL THEN NULL ELSE hidden_at END,
             revision = revision + 1, updated_at = ?2
         WHERE id = ?3 AND participant_kind = 'ai_agent' AND revision = ?4",
    )
    .bind(archived_at)
    .bind(now.as_str())
    .bind(agent_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The agent changed before the update",
            true,
        ));
    }
    if archived {
        let lifecycle = read_agent_lifecycle(&mut *transaction, agent_id).await?;
        if lifecycle.active_assignment_count > 0 {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Stop the agent's active work before archiving it",
                true,
            ));
        }
    }
    ganbaru_chat::sync_effects::revoke_lapsed_push_approvals(&mut transaction, now.as_str())
        .await?;
    ganbaru_chat::sync_effects::revoke_lapsed_remembered_approvals(&mut transaction, now.as_str())
        .await?;
    transaction.commit().await.map_err(persistence_error)?;
    read_agent(pool, agent_id, local_instances).await
}

/// Whether anything the vault keeps names `agent_id` as its author, worker, reviewer,
/// requester, approver, or mention target, or this device recorded its usage, scratch space,
/// or an access revocation for it.
///
/// An agent is used once it posted a message, was assigned work in any state, reviewed work,
/// requested an authorization, approved a push or was approved to push, proposed or edited a
/// channel note, was mentioned in a message or a scheduled message, or ran on this device.
/// Its own row, its policy revisions (model edits), its channel and conversation memberships,
/// and its read cursors are configuration rather than use, so they leave with a deletion.
#[cfg(desktop)]
pub(super) async fn agent_was_used<'e, E>(
    executor: E,
    agent_id: &ChatParticipantId,
) -> ChatResult<bool>
where
    E: Executor<'e, Database = Sqlite>,
{
    let used: i64 = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM chat_communication_messages
                        WHERE author_participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_work_assignments
                        WHERE agent_id = ?1 OR reviewed_by_participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_assignment_authorization_revisions
                        WHERE requester_participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_push_approvals
                        WHERE agent_id = ?1 OR approved_by_participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_channel_notes
                        WHERE proposed_by_participant_id = ?1 OR edited_by_participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_participant_reference_targets
                        WHERE participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_scheduled_message_references
                        WHERE participant_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_agent_usage_daily WHERE agent_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_scratch_scopes WHERE agent_id = ?1)
             OR EXISTS (SELECT 1 FROM chat_access_revocation_jobs WHERE agent_id = ?1)",
    )
    .bind(agent_id.as_str())
    .fetch_one(executor)
    .await
    .map_err(persistence_error)?;
    Ok(used != 0)
}

/// Refuses `agent_id` unless it is an AI agent a person named at `expected_revision`.
///
/// Only a named agent's random id is one sync lets a person delete, so the check reads the same
/// key rule every replica admits a deletion by.
#[cfg(desktop)]
async fn require_custom_agent(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    expected_revision: u64,
) -> ChatResult<()> {
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT participant.revision FROM chat_participants participant
         JOIN chat_agents agent ON agent.participant_id = participant.id
         WHERE participant.id = ? AND participant.participant_kind = 'ai_agent'",
    )
    .bind(agent_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    let Some(revision) = revision else {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The AI agent was not found",
            true,
        ));
    };
    if !is_named_agent_key(agent_id.as_str()) {
        return Err(ChatError::new(
            ChatErrorCode::Validation,
            "Only an agent a person named can be removed or restored",
            false,
        ));
    }
    if revision != i64_value(expected_revision)? {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The agent changed before the update",
            true,
        ));
    }
    Ok(())
}

/// Removes the agent a person named `agent_id` at `expected_revision`, in one transaction.
///
/// An agent [`agent_was_used`] finds unused is deleted with its configuration, and sync carries
/// the deletion to every device. A used agent is archived and hidden instead, under the same
/// active work guard as archiving, so its messages and work keep their author.
#[cfg(desktop)]
pub(super) async fn remove_custom_agent(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    expected_revision: u64,
    local_instances: &reads::LocalProviderInstances,
) -> ChatResult<ChatCustomAgentRemoval> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    require_custom_agent(&mut transaction, agent_id, expected_revision).await?;
    if !agent_was_used(&mut *transaction, agent_id).await? {
        sqlx::query("DELETE FROM chat_participants WHERE id = ?")
            .bind(agent_id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(ChatCustomAgentRemoval::Deleted);
    }
    let lifecycle = read_agent_lifecycle(&mut *transaction, agent_id).await?;
    if lifecycle.active_assignment_count > 0 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Stop the agent's active work before archiving it",
            true,
        ));
    }
    sqlx::query(
        "UPDATE chat_participants
         SET archived_at = coalesce(archived_at, ?1), hidden_at = ?1,
             revision = revision + 1, updated_at = ?1
         WHERE id = ?2",
    )
    .bind(now.as_str())
    .bind(agent_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    ganbaru_chat::sync_effects::revoke_lapsed_push_approvals(&mut transaction, now.as_str())
        .await?;
    ganbaru_chat::sync_effects::revoke_lapsed_remembered_approvals(&mut transaction, now.as_str())
        .await?;
    transaction.commit().await.map_err(persistence_error)?;
    let agent = read_agent(pool, agent_id, local_instances).await?;
    Ok(ChatCustomAgentRemoval::Archived {
        agent: Box::new(agent),
    })
}

/// Brings the hidden agent a person named `agent_id` at `expected_revision` back to the agent
/// list. It stays archived, so it stays off until a person turns it on.
#[cfg(desktop)]
pub(super) async fn restore_custom_agent(
    pool: &SqlitePool,
    agent_id: &ChatParticipantId,
    expected_revision: u64,
    local_instances: &reads::LocalProviderInstances,
) -> ChatResult<ChatAiAgentRead> {
    let now = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    require_custom_agent(&mut transaction, agent_id, expected_revision).await?;
    sqlx::query(
        "UPDATE chat_participants
         SET hidden_at = NULL, revision = revision + 1, updated_at = ?1
         WHERE id = ?2 AND hidden_at IS NOT NULL",
    )
    .bind(now.as_str())
    .bind(agent_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    read_agent(pool, agent_id, local_instances).await
}

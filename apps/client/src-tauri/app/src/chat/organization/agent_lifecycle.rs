use super::*;
#[cfg(desktop)]
use sqlx::SqlitePool;
use sqlx::{Executor, Sqlite};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct AgentLifecycleState {
    pub active_assignment_count: u64,
}

/// Reads the work that keeps `agent_id` from being archived.
///
/// Started work counts wherever it runs. Queued work counts only when this device holds a
/// live (queued or claimed) dispatch job for it, which exists only after a local post or Start
/// accepted it. Queued work with no live dispatch here waits for an acceptance this device
/// cannot see, or was moved to another executor, so a peer's row cannot block the archive. It
/// stays queued, does not start while the agent is archived, and becomes actionable again
/// after a restore.
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
         SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND participant_kind = 'ai_agent' AND revision = ?",
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
    transaction.commit().await.map_err(persistence_error)?;
    read_agent(pool, agent_id, local_instances).await
}

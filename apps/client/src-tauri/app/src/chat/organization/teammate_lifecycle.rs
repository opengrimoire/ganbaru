use super::*;
#[cfg(desktop)]
use sqlx::SqlitePool;
use sqlx::{Executor, Sqlite};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TeammateLifecycleState {
    pub active_assignment_count: u64,
}

pub(super) async fn read_teammate_lifecycle<'e, E>(
    executor: E,
    teammate_id: &ChatParticipantId,
) -> ChatResult<TeammateLifecycleState>
where
    E: Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(
        "SELECT
            (SELECT count(*) FROM chat_work_assignments assignment
             WHERE assignment.teammate_id = ?
               AND assignment.state IN (
                 'queued', 'working', 'waiting_for_answer',
                 'waiting_for_approval', 'ready_for_review'
               )) AS active_assignment_count",
    )
    .bind(teammate_id.as_str())
    .fetch_one(executor)
    .await
    .map_err(persistence_error)?;
    Ok(TeammateLifecycleState {
        active_assignment_count: u64_value(
            row.try_get("active_assignment_count")
                .map_err(persistence_error)?,
        )?,
    })
}

#[cfg(desktop)]
pub(super) async fn set_teammate_archived(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    expected_revision: u64,
    archived: bool,
) -> ChatResult<ChatAiTeammateRead> {
    let now = now_timestamp()?;
    let archived_at = archived.then(|| now.as_str());
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let updated = sqlx::query(
        "UPDATE chat_participants
         SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND participant_kind = 'ai_teammate' AND revision = ?",
    )
    .bind(archived_at)
    .bind(now.as_str())
    .bind(teammate_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The teammate changed before the update",
            true,
        ));
    }
    if archived {
        let lifecycle = read_teammate_lifecycle(&mut *transaction, teammate_id).await?;
        if lifecycle.active_assignment_count > 0 {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Stop the teammate's active work before archiving it",
                true,
            ));
        }
    }
    transaction.commit().await.map_err(persistence_error)?;
    read_teammate(pool, teammate_id).await
}

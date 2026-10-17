use super::*;
#[cfg(desktop)]
use sqlx::SqlitePool;
use sqlx::{Executor, Sqlite};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TeammateLifecycleState {
    pub active_assignment_count: u64,
    pub has_durable_history: bool,
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
               )) AS active_assignment_count,
            (EXISTS(
               SELECT 1 FROM chat_communication_messages message
               WHERE message.author_participant_id = ?
             ) OR EXISTS(
               SELECT 1
               FROM chat_participant_reference_targets reference
               WHERE reference.participant_id = ?
             ) OR EXISTS(
               SELECT 1 FROM chat_work_assignments assignment
               WHERE assignment.teammate_id = ?
             )) AS has_durable_history",
    )
    .bind(teammate_id.as_str())
    .bind(teammate_id.as_str())
    .bind(teammate_id.as_str())
    .bind(teammate_id.as_str())
    .fetch_one(executor)
    .await
    .map_err(persistence_error)?;
    Ok(TeammateLifecycleState {
        active_assignment_count: u64_value(
            row.try_get("active_assignment_count")
                .map_err(persistence_error)?,
        )?,
        has_durable_history: row
            .try_get::<i64, _>("has_durable_history")
            .map_err(persistence_error)?
            != 0,
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

#[cfg(desktop)]
pub(super) async fn delete_unused_teammate(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
    expected_revision: u64,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let locked = sqlx::query(
        "UPDATE chat_participants SET updated_at = updated_at
         WHERE id = ? AND participant_kind = 'ai_teammate'
           AND archived_at IS NOT NULL AND revision = ?",
    )
    .bind(teammate_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if locked.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The archived teammate changed before deletion",
            true,
        ));
    }
    let lifecycle = read_teammate_lifecycle(&mut *transaction, teammate_id).await?;
    if lifecycle.has_durable_history {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "A teammate with communication or work history cannot be permanently deleted",
            true,
        ));
    }
    sqlx::query("DELETE FROM chat_conversation_memberships WHERE participant_id = ?")
        .bind(teammate_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_teammate_policy_revisions WHERE teammate_id = ?")
        .bind(teammate_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_teammates WHERE participant_id = ?")
        .bind(teammate_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    let deleted = sqlx::query(
        "DELETE FROM chat_participants
         WHERE id = ? AND participant_kind = 'ai_teammate'
           AND archived_at IS NOT NULL AND revision = ?",
    )
    .bind(teammate_id.as_str())
    .bind(i64_value(expected_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if deleted.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The archived teammate changed before deletion",
            true,
        ));
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(())
}

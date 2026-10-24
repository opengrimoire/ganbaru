//! Revocation of a teammate's active assignment authorizations.
//!
//! Commands that reduce access and the sync materialize hook share these statements, so a
//! reduction made on this device and one applied from another device revoke the same rows.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::SqliteConnection;

/// Revokes every allowed, unrevoked authorization of `teammate_id` and queues one access
/// revocation job for each, unless a queued or claimed job already covers it.
///
/// `now` is the revocation and job timestamp. Returns the number of authorizations revoked.
pub async fn revoke_teammate_authorizations(
    connection: &mut SqliteConnection,
    teammate_id: &str,
    now: &str,
    reason: &str,
) -> ChatResult<u64> {
    let revoked = sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?, revoked_at = ?
         WHERE id IN (
             SELECT authorization.id
             FROM chat_assignment_authorization_revisions authorization
             JOIN chat_work_assignments assignment
               ON assignment.id = authorization.assignment_id
             WHERE assignment.teammate_id = ?
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
         )",
    )
    .bind(reason)
    .bind(now)
    .bind(teammate_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    sqlx::query(
        "INSERT INTO chat_access_revocation_jobs
            (id, teammate_id, authorization_revision_id, reason,
             available_at, created_at, updated_at)
         SELECT 'access-revocation:' || lower(hex(randomblob(16))), ?, authorization.id, ?, ?, ?, ?
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.teammate_id = ? AND authorization.revoked_at = ?
           AND NOT EXISTS (
               SELECT 1 FROM chat_access_revocation_jobs existing
               WHERE existing.authorization_revision_id = authorization.id
                 AND existing.state IN ('queued', 'claimed')
           )",
    )
    .bind(teammate_id)
    .bind(reason)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(teammate_id)
    .bind(now)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(revoked)
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat access could not be revoked",
        true,
    )
}

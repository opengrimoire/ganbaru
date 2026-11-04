//! Revocation of active assignment authorizations.
//!
//! Commands that reduce access and the sync materialize hook share these statements, so a
//! reduction made on this device and one applied from another device revoke the same rows.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::SqliteConnection;

/// The authorizations one revocation covers.
#[derive(Clone, Copy, Debug)]
enum RevocationScope<'a> {
    /// Every authorization of one teammate.
    Teammate(&'a str),
    /// Every authorization of one work assignment.
    Assignment(&'a str),
}

impl<'a> RevocationScope<'a> {
    fn teammate_id(self) -> Option<&'a str> {
        match self {
            Self::Teammate(teammate_id) => Some(teammate_id),
            Self::Assignment(_) => None,
        }
    }

    fn assignment_id(self) -> Option<&'a str> {
        match self {
            Self::Teammate(_) => None,
            Self::Assignment(assignment_id) => Some(assignment_id),
        }
    }
}

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
    revoke_authorizations(
        connection,
        RevocationScope::Teammate(teammate_id),
        now,
        reason,
    )
    .await
}

/// Revokes every allowed, unrevoked authorization of the work assignment `assignment_id` and
/// queues one access revocation job for each, unless a queued or claimed job already covers it,
/// so the next drain stops the runs it granted.
///
/// `now` is the revocation and job timestamp. Returns the number of authorizations revoked.
pub async fn revoke_assignment_authorizations(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &str,
    reason: &str,
) -> ChatResult<u64> {
    revoke_authorizations(
        connection,
        RevocationScope::Assignment(assignment_id),
        now,
        reason,
    )
    .await
}

async fn revoke_authorizations(
    connection: &mut SqliteConnection,
    scope: RevocationScope<'_>,
    now: &str,
    reason: &str,
) -> ChatResult<u64> {
    let revoked = sqlx::query(
        "UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?1, revoked_at = ?2
         WHERE id IN (
             SELECT authorization.id
             FROM chat_assignment_authorization_revisions authorization
             JOIN chat_work_assignments assignment
               ON assignment.id = authorization.assignment_id
             WHERE (?3 IS NULL OR assignment.teammate_id = ?3)
               AND (?4 IS NULL OR assignment.id = ?4)
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
         )",
    )
    .bind(reason)
    .bind(now)
    .bind(scope.teammate_id())
    .bind(scope.assignment_id())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    sqlx::query(
        "INSERT INTO chat_access_revocation_jobs
            (id, teammate_id, authorization_revision_id, reason,
             available_at, created_at, updated_at)
         SELECT 'access-revocation:' || lower(hex(randomblob(16))), assignment.teammate_id,
                authorization.id, ?1, ?2, ?2, ?2
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE (?3 IS NULL OR assignment.teammate_id = ?3)
           AND (?4 IS NULL OR assignment.id = ?4)
           AND authorization.revoked_at = ?2
           AND NOT EXISTS (
               SELECT 1 FROM chat_access_revocation_jobs existing
               WHERE existing.authorization_revision_id = authorization.id
                 AND existing.state IN ('queued', 'claimed')
           )",
    )
    .bind(reason)
    .bind(now)
    .bind(scope.teammate_id())
    .bind(scope.assignment_id())
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

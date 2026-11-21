//! Revocation of active assignment authorizations.
//!
//! Commands that narrow access and the sync materialize hook share these statements, so a
//! narrowing made on this device and one applied from another device revoke the same rows.

use crate::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::SqliteConnection;

/// The authorizations one revocation covers.
#[derive(Clone, Copy, Debug)]
enum RevocationScope<'a> {
    /// Every authorization of one agent.
    Agent(&'a str),
    /// Every authorization of one work assignment.
    Assignment(&'a str),
    /// One authorization.
    Authorization(&'a str),
}

impl<'a> RevocationScope<'a> {
    fn agent_id(self) -> Option<&'a str> {
        match self {
            Self::Agent(agent_id) => Some(agent_id),
            Self::Assignment(_) | Self::Authorization(_) => None,
        }
    }

    fn assignment_id(self) -> Option<&'a str> {
        match self {
            Self::Assignment(assignment_id) => Some(assignment_id),
            Self::Agent(_) | Self::Authorization(_) => None,
        }
    }

    fn authorization_id(self) -> Option<&'a str> {
        match self {
            Self::Authorization(authorization_id) => Some(authorization_id),
            Self::Agent(_) | Self::Assignment(_) => None,
        }
    }
}

/// State reason of a delegated assignment cancelled because an ancestor lost the authority it
/// was delegated under.
pub const PARENT_AUTHORITY_REVOKED_REASON: &str = "parent_authority_revoked";

/// Revokes every allowed, unrevoked authorization of `agent_id` and queues one access
/// revocation job for each, unless a queued or claimed job already covers it. Work delegated
/// from the agent's assignments loses its authorizations too, with
/// [`PARENT_AUTHORITY_REVOKED_REASON`], since a child never holds more than its parent.
///
/// `now` is the revocation and job timestamp. Returns the number of authorizations revoked,
/// the delegated ones included.
pub async fn revoke_agent_authorizations(
    connection: &mut SqliteConnection,
    agent_id: &str,
    now: &str,
    reason: &str,
) -> ChatResult<u64> {
    let revoked =
        revoke_authorizations(connection, RevocationScope::Agent(agent_id), now, reason).await?;
    let delegated = revoke_descendants(connection, RevocationScope::Agent(agent_id), now).await?;
    Ok(revoked + delegated)
}

/// Revokes the allowed, unrevoked authorizations of the work delegated from `assignment_id`,
/// down to the delegation depth limit, with [`PARENT_AUTHORITY_REVOKED_REASON`], and queues one
/// access revocation job for each unless a queued or claimed job already covers it.
///
/// `now` is the revocation and job timestamp. Returns the number of authorizations revoked.
pub async fn revoke_delegated_authorizations(
    connection: &mut SqliteConnection,
    assignment_id: &str,
    now: &str,
) -> ChatResult<u64> {
    revoke_descendants(connection, RevocationScope::Assignment(assignment_id), now).await
}

/// The ids of the assignments delegated from the assignments `scope` covers, through
/// `parent_assignment_id`, bound by the depth the schema allows. Binds `?3` (agent) and
/// `?4` (assignment) like [`revoke_authorizations`].
const DELEGATED_DESCENDANTS_SQL: &str = "WITH RECURSIVE descendants(id, depth) AS (
       SELECT child.id, 1
       FROM chat_work_assignments child
       JOIN chat_work_assignments parent ON parent.id = child.parent_assignment_id
       WHERE (?3 IS NULL OR parent.agent_id = ?3)
         AND (?4 IS NULL OR parent.id = ?4)
       UNION
       SELECT child.id, descendants.depth + 1
       FROM chat_work_assignments child
       JOIN descendants ON child.parent_assignment_id = descendants.id
       WHERE descendants.depth < 2
     )";

async fn revoke_descendants(
    connection: &mut SqliteConnection,
    scope: RevocationScope<'_>,
    now: &str,
) -> ChatResult<u64> {
    let update = format!(
        "{DELEGATED_DESCENDANTS_SQL}
         UPDATE chat_assignment_authorization_revisions
         SET decision_state = 'revoked', reason = ?1, revoked_at = ?2
         WHERE assignment_id IN (SELECT id FROM descendants)
           AND decision_state = 'allowed' AND revoked_at IS NULL"
    );
    let revoked = sqlx::query(sqlx::AssertSqlSafe(update))
        .bind(PARENT_AUTHORITY_REVOKED_REASON)
        .bind(now)
        .bind(scope.agent_id())
        .bind(scope.assignment_id())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?
        .rows_affected();
    if revoked == 0 {
        return Ok(0);
    }
    let jobs = format!(
        "{DELEGATED_DESCENDANTS_SQL}
         INSERT INTO chat_access_revocation_jobs
            (id, agent_id, authorization_revision_id, reason,
             available_at, created_at, updated_at)
         SELECT 'access-revocation:' || lower(hex(randomblob(16))), assignment.agent_id,
                authorization.id, ?1, ?2, ?2, ?2
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.id IN (SELECT id FROM descendants)
           AND authorization.revoked_at = ?2
           AND authorization.reason = ?1
           AND NOT EXISTS (
               SELECT 1 FROM chat_access_revocation_jobs existing
               WHERE existing.authorization_revision_id = authorization.id
                 AND existing.state IN ('queued', 'claimed')
           )"
    );
    sqlx::query(sqlx::AssertSqlSafe(jobs))
        .bind(PARENT_AUTHORITY_REVOKED_REASON)
        .bind(now)
        .bind(scope.agent_id())
        .bind(scope.assignment_id())
        .execute(&mut *connection)
        .await
        .map_err(persistence_error)?;
    Ok(revoked)
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

/// Revokes the authorization `authorization_id` when it is allowed and unrevoked, and queues an
/// access revocation job for it unless a queued or claimed job already covers it. Work delegated
/// from its assignment loses its authorizations too, with [`PARENT_AUTHORITY_REVOKED_REASON`].
///
/// `now` is the revocation and job timestamp. Returns the number of authorizations revoked, the
/// delegated ones included, so a repeated call returns zero.
pub async fn revoke_authorization(
    connection: &mut SqliteConnection,
    authorization_id: &str,
    now: &str,
    reason: &str,
) -> ChatResult<u64> {
    let revoked = revoke_authorizations(
        connection,
        RevocationScope::Authorization(authorization_id),
        now,
        reason,
    )
    .await?;
    if revoked == 0 {
        return Ok(0);
    }
    let assignment_id: String = sqlx::query_scalar(
        "SELECT assignment_id FROM chat_assignment_authorization_revisions WHERE id = ?",
    )
    .bind(authorization_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let delegated =
        revoke_descendants(connection, RevocationScope::Assignment(&assignment_id), now).await?;
    Ok(revoked + delegated)
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
             WHERE (?3 IS NULL OR assignment.agent_id = ?3)
               AND (?4 IS NULL OR assignment.id = ?4)
               AND (?5 IS NULL OR authorization.id = ?5)
               AND authorization.decision_state = 'allowed'
               AND authorization.revoked_at IS NULL
         )",
    )
    .bind(reason)
    .bind(now)
    .bind(scope.agent_id())
    .bind(scope.assignment_id())
    .bind(scope.authorization_id())
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?
    .rows_affected();
    sqlx::query(
        "INSERT INTO chat_access_revocation_jobs
            (id, agent_id, authorization_revision_id, reason,
             available_at, created_at, updated_at)
         SELECT 'access-revocation:' || lower(hex(randomblob(16))), assignment.agent_id,
                authorization.id, ?1, ?2, ?2, ?2
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE (?3 IS NULL OR assignment.agent_id = ?3)
           AND (?4 IS NULL OR assignment.id = ?4)
           AND (?5 IS NULL OR authorization.id = ?5)
           AND authorization.revoked_at = ?2
           AND NOT EXISTS (
               SELECT 1 FROM chat_access_revocation_jobs existing
               WHERE existing.authorization_revision_id = authorization.id
                 AND existing.state IN ('queued', 'claimed')
           )",
    )
    .bind(reason)
    .bind(now)
    .bind(scope.agent_id())
    .bind(scope.assignment_id())
    .bind(scope.authorization_id())
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

//! Device-local claims that let exactly one attempt push a push request.
//!
//! A person's confirmation and a standing push after a turn can answer the same request at the
//! same time. Each attempt claims the request with one conditional write before any Git work,
//! and only the attempt whose write took effect pushes; the other is refused. A claim names the
//! Ganbaru process that holds it, so a claim a stopped process left is taken over by the next
//! attempt instead of blocking the request forever, and it records the commit being pushed, so
//! that attempt can ask the remote whether the interrupted push arrived.

use super::PushRequestRefusal;
use crate::git::{
    GitFolder, GitPushTarget, advance_branch, is_object_id, remote_branch_names_object,
};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use sqlx::SqlitePool;

/// What an attempt that won the claim of a push request has to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PushClaim {
    /// No earlier attempt pushed or began pushing the request: push it.
    Fresh,
    /// An earlier attempt began pushing and stopped before recording the result, because its
    /// process ended or the push failed. `object_id` is the commit it was pushing, when it got
    /// that far; the remote may already have it.
    Interrupted { object_id: Option<String> },
    /// An earlier attempt pushed `object_id` but stopped before posting the confirmation reply.
    Pushed { object_id: String },
}

/// The row an attempt took over from an earlier one.
#[derive(sqlx::FromRow)]
struct TakenOverClaim {
    state: String,
    object_id: Option<String>,
}

/// Claims the push request `item_id` for the attempt of the process `claim_token` at `now`.
/// The claim succeeds when no claim exists, or the existing one was released or names another
/// process, which therefore stopped. Each step is one conditional write, so of attempts racing
/// for the request exactly one wins.
///
/// # Errors
///
/// Returns [`PushRequestRefusal::InProgress`] when another attempt of this process holds the
/// claim, [`PushRequestRefusal::AlreadyPushed`] when that attempt already pushed, and a
/// persistence error when a write fails.
pub async fn claim_push_request(
    pool: &SqlitePool,
    item_id: &str,
    claim_token: &str,
    now: &str,
) -> ChatResult<PushClaim> {
    let inserted = sqlx::query(
        "INSERT INTO chat_push_request_claims
            (request_item_id, state, claim_token, object_id, claimed_at, updated_at)
         VALUES (?, 'pushing', ?, NULL, ?, ?)
         ON CONFLICT (request_item_id) DO NOTHING",
    )
    .bind(item_id)
    .bind(claim_token)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if inserted.rows_affected() == 1 {
        return Ok(PushClaim::Fresh);
    }
    let taken: Option<TakenOverClaim> = sqlx::query_as(
        "UPDATE chat_push_request_claims
         SET claim_token = ?, claimed_at = ?, updated_at = ?
         WHERE request_item_id = ? AND claim_token IS NOT ?
         RETURNING state, object_id",
    )
    .bind(claim_token)
    .bind(now)
    .bind(now)
    .bind(item_id)
    .bind(claim_token)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    if let Some(taken) = taken {
        return Ok(match (taken.state.as_str(), taken.object_id) {
            ("pushed", Some(object_id)) => PushClaim::Pushed { object_id },
            (_, object_id) => PushClaim::Interrupted { object_id },
        });
    }
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM chat_push_request_claims WHERE request_item_id = ?")
            .bind(item_id)
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?;
    Err(match state.as_deref() {
        Some("pushed") => PushRequestRefusal::AlreadyPushed,
        _ => PushRequestRefusal::InProgress,
    }
    .into_error())
}

/// Records that the attempt holding the claim of `item_id` is about to push `object_id`, so an
/// attempt that takes the claim over after a crash can ask the remote for it.
///
/// # Errors
///
/// Returns a validation error for a malformed object id, a conflict when the attempt no longer
/// holds the claim, and a persistence error when the write fails.
pub async fn record_push_object(
    pool: &SqlitePool,
    item_id: &str,
    claim_token: &str,
    object_id: &str,
    now: &str,
) -> ChatResult<()> {
    set_claim_object(pool, item_id, claim_token, object_id, "pushing", now).await
}

/// Records that the attempt holding the claim of `item_id` pushed `object_id`. The claim stays
/// held, so another attempt of this process is refused while the confirmation reply posts.
///
/// # Errors
///
/// Returns the errors of [`record_push_object`].
pub async fn mark_push_request_pushed(
    pool: &SqlitePool,
    item_id: &str,
    claim_token: &str,
    object_id: &str,
    now: &str,
) -> ChatResult<()> {
    set_claim_object(pool, item_id, claim_token, object_id, "pushed", now).await
}

/// Releases the claim of `item_id` the attempt of `claim_token` holds after it failed, keeping
/// its state and commit, so the next attempt takes it over as interrupted or pushed. Releasing a
/// claim the attempt no longer holds changes nothing.
///
/// # Errors
///
/// Returns a persistence error when the write fails.
pub async fn release_push_claim(
    pool: &SqlitePool,
    item_id: &str,
    claim_token: &str,
    now: &str,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_push_request_claims SET claim_token = NULL, updated_at = ?
         WHERE request_item_id = ? AND claim_token = ?",
    )
    .bind(now)
    .bind(item_id)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Settles an interrupted push of `item_id` whose claim the attempt of `claim_token` took over:
/// asks the remote whether the destination branch of `pushed` names the commit the earlier
/// attempt was pushing, and when it does, marks the claim pushed and, for a commit Ganbaru made
/// on `committed_on`, moves the local branch to it on a best effort basis. Returns whether the
/// commit had arrived; when it had not, the claim stays held and the caller pushes again.
///
/// # Errors
///
/// Returns the errors of [`remote_branch_names_object`] when the remote cannot be read, and the
/// errors of [`mark_push_request_pushed`].
pub async fn recover_interrupted_push(
    pool: &SqlitePool,
    folder: GitFolder<'_>,
    item_id: &str,
    claim_token: &str,
    pushed: &GitPushTarget,
    committed_on: Option<&str>,
    now: &str,
) -> ChatResult<bool> {
    if !remote_branch_names_object(folder, pushed).await? {
        return Ok(false);
    }
    mark_push_request_pushed(pool, item_id, claim_token, &pushed.object_id, now).await?;
    if let Some(parent) = committed_on {
        // The remote holds the commit, so the push stands even when the branch cannot follow.
        let _ = advance_branch(folder, &pushed.local_branch, &pushed.object_id, parent).await;
    }
    Ok(true)
}

async fn set_claim_object(
    pool: &SqlitePool,
    item_id: &str,
    claim_token: &str,
    object_id: &str,
    state: &str,
    now: &str,
) -> ChatResult<()> {
    if !is_object_id(object_id) {
        return Err(ChatError::validation(
            "objectId",
            "Git object ID is invalid",
        ));
    }
    let updated = sqlx::query(
        "UPDATE chat_push_request_claims SET state = ?, object_id = ?, updated_at = ?
         WHERE request_item_id = ? AND claim_token = ?",
    )
    .bind(state)
    .bind(object_id)
    .bind(now)
    .bind(item_id)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() == 1 {
        Ok(())
    } else {
        Err(ChatError::new(
            ChatErrorCode::Conflict,
            "The push request claim was taken over",
            false,
        ))
    }
}

fn persistence_error(_error: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat data could not be saved or read",
        true,
    )
}

#[cfg(test)]
mod tests;

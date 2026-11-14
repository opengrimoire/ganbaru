//! Standing push approvals a person lists and revokes in teammate settings.
//!
//! Approvals are made only by the confirm path when a person pushes and chooses not to be asked
//! again, so this module only reads and revokes them. A revocation keeps the row with its time,
//! which wins over a concurrent approval made on another device.

use super::{identifier_error, persistence_error};
use crate::chat::channels::now_timestamp;
use ganbaru_chat::coordination::contracts::{ChatPushApprovalRead, RevokeChatPushApprovalCommand};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

/// Most approvals one list returns.
const MAX_LISTED_PUSH_APPROVALS: i64 = 200;

/// Longest approval id, in bytes, matching the replicated key bound.
const MAX_APPROVAL_ID_BYTES: usize = 128;

/// Lists `teammate_id`'s active approvals by folder name, push URL, and branch.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub(super) async fn list_push_approvals(
    pool: &SqlitePool,
    teammate_id: &ChatParticipantId,
) -> ChatResult<Vec<ChatPushApprovalRead>> {
    let rows = sqlx::query(
        "SELECT approval.sync_key, approval.teammate_id, approval.working_folder_id,
                folder.display_name, approval.push_url, approval.destination_branch,
                approval.approved_at
         FROM chat_push_approvals approval
         JOIN project_working_folders folder ON folder.id = approval.working_folder_id
         WHERE approval.teammate_id = ? AND approval.revoked_at IS NULL
         ORDER BY folder.display_name COLLATE NOCASE, approval.push_url,
                  approval.destination_branch
         LIMIT ?",
    )
    .bind(teammate_id.as_str())
    .bind(MAX_LISTED_PUSH_APPROVALS)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.iter().map(approval_read).collect()
}

/// Revokes the approval `request.approval_id` names. Revoking one that is already revoked or
/// gone changes nothing.
///
/// # Errors
///
/// Returns a validation error for a malformed id and a persistence error when the write fails.
pub(super) async fn revoke_push_approval(
    pool: &SqlitePool,
    request: &RevokeChatPushApprovalCommand,
) -> ChatResult<()> {
    let approval_id = request.approval_id.as_str();
    if approval_id.trim().is_empty() || approval_id.len() > MAX_APPROVAL_ID_BYTES {
        return Err(ChatError::validation(
            "approvalId",
            "The push approval id is malformed",
        ));
    }
    let now = now_timestamp()?;
    sqlx::query(
        "UPDATE chat_push_approvals SET revoked_at = ?, revision = revision + 1
         WHERE sync_key = ? AND revoked_at IS NULL",
    )
    .bind(now.as_str())
    .bind(approval_id)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn approval_read(row: &sqlx::sqlite::SqliteRow) -> ChatResult<ChatPushApprovalRead> {
    let text = |column: &str| row.try_get::<String, _>(column).map_err(persistence_error);
    Ok(ChatPushApprovalRead {
        approval_id: text("sync_key")?,
        teammate_id: ChatParticipantId::new(text("teammate_id")?).map_err(identifier_error)?,
        working_folder_id: ProjectWorkingFolderId::new(text("working_folder_id")?)
            .map_err(identifier_error)?,
        working_folder_name: text("display_name")?,
        push_url: text("push_url")?,
        destination_branch: text("destination_branch")?,
        approved_at: text("approved_at")?,
    })
}

#[cfg(all(test, desktop))]
mod tests;

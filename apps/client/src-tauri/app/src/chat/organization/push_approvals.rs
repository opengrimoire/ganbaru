//! Standing push approvals a person lists and revokes in a channel's Agents section.
//!
//! Approvals are made only by the confirm path when a person pushes and chooses not to be asked
//! again, so this module only reads and revokes them. A revocation keeps the row with its time,
//! which wins over a concurrent approval made on another device.

use super::persistence_error;
use crate::chat::channels::now_timestamp;
use ganbaru_chat::coordination::agent_settings::read_channel_push_approvals;
use ganbaru_chat::coordination::contracts::RevokeChatPushApprovalCommand;
use ganbaru_chat_contracts::models::*;
use sqlx::SqlitePool;

/// Longest approval id, in bytes, matching the replicated key bound.
const MAX_APPROVAL_ID_BYTES: usize = 128;

/// Lists the active approvals of the channel whose conversation is `conversation_id`, newest
/// first.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub(super) async fn list_push_approvals(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
) -> ChatResult<Vec<ChatChannelPushApprovalRead>> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    read_channel_push_approvals(&mut connection, conversation_id.as_str()).await
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

#[cfg(all(test, desktop))]
mod tests;

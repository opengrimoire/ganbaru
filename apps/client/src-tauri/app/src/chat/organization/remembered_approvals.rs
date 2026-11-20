//! Remembered agent approvals a person lists and revokes in a channel's Agents section.
//!
//! Approvals are made only when a person allows an agent request once and chooses not to be
//! asked again, so this module only reads and revokes them. A revocation keeps the row with its
//! time, which wins over a concurrent approval made on another device.

use super::persistence_error;
use crate::chat::channels::now_timestamp;
use ganbaru_chat::coordination::contracts::RevokeChatRememberedApprovalCommand;
use ganbaru_chat::coordination::remembered_approvals::{
    read_channel_remembered_approvals, revoke_remembered_approval,
};
use ganbaru_chat_contracts::models::*;
use sqlx::SqlitePool;

/// Longest approval id, in bytes, matching the replicated key bound.
const MAX_APPROVAL_ID_BYTES: usize = 128;

/// Lists the active remembered approvals of the channel whose conversation is
/// `conversation_id`, newest first.
///
/// # Errors
///
/// Returns a persistence error when the read fails.
pub(super) async fn list_remembered_approvals(
    pool: &SqlitePool,
    conversation_id: &ChatConversationId,
) -> ChatResult<Vec<ChatChannelRememberedApprovalRead>> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    read_channel_remembered_approvals(&mut connection, conversation_id.as_str()).await
}

/// Revokes the remembered approval `request.approval_id` names. Revoking one that is already
/// revoked or gone changes nothing.
///
/// # Errors
///
/// Returns a validation error for a malformed id and a persistence error when the write fails.
pub(super) async fn revoke(
    pool: &SqlitePool,
    request: &RevokeChatRememberedApprovalCommand,
) -> ChatResult<()> {
    let approval_id = request.approval_id.as_str();
    if approval_id.trim().is_empty() || approval_id.len() > MAX_APPROVAL_ID_BYTES {
        return Err(ChatError::validation(
            "approvalId",
            "The remembered approval id is malformed",
        ));
    }
    let now = now_timestamp()?;
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    revoke_remembered_approval(&mut connection, approval_id, now.as_str()).await
}

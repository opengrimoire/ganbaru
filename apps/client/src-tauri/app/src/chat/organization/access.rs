//! Agent access in channels: the channel roster, membership removal previews, and membership
//! removal, with the settings commands and target discovery in submodules.

mod resolve;
mod settings;
mod targets;

pub(in crate::chat::organization) use resolve::*;
pub use settings::*;
pub use targets::*;

use super::*;
use ganbaru_chat::coordination::agent_settings::agent_is_present;

#[tauri::command]
pub async fn chat_read_channel_roster(
    app: tauri::AppHandle,
    db_url: String,
    channel_id: ChatChannelId,
) -> ChatResult<ChatChannelRosterRead> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::super::channels::read_channel(&pool, &channel_id).await?;
    let audience_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM chat_conversation_audience_state WHERE conversation_id = ?",
    )
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatChannelRosterRead {
        channel_id,
        conversation_id: channel.conversation_id.clone(),
        audience_revision: u64_value(audience_revision)?,
        memberships: read_memberships_for_conversation(&pool, &channel.conversation_id, false)
            .await?,
    })
}

/// Counts the work that removing the agent from the channel would revoke.
#[tauri::command]
pub async fn chat_preview_channel_membership_removal(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatChannelMembershipRemovalCommand,
) -> ChatResult<ChatChannelMembershipRemovalPreview> {
    let pool = chat_pool(app, db_url).await?;
    let channel = super::super::channels::read_channel(&pool, &request.channel_id).await?;
    {
        let mut connection = pool.acquire().await.map_err(persistence_error)?;
        if !agent_is_present(
            &mut connection,
            channel.conversation_id.as_str(),
            request.agent_id.as_str(),
        )
        .await?
        {
            return Err(ChatError::new(
                ChatErrorCode::NotFound,
                "The agent is not a member of this channel",
                true,
            ));
        }
    }
    let active_assignment_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM chat_work_assignments assignment
         JOIN chat_reply_threads thread ON thread.id = assignment.reply_thread_id
         WHERE assignment.agent_id = ? AND thread.conversation_id = ?
           AND assignment.state IN (
             'queued', 'working', 'waiting_for_answer',
             'waiting_for_approval', 'ready_for_review'
           )",
    )
    .bind(request.agent_id.as_str())
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    let active_authorization_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
         FROM chat_assignment_authorization_revisions authorization
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE assignment.agent_id = ?
           AND authorization.destination_conversation_id = ?
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL",
    )
    .bind(request.agent_id.as_str())
    .bind(channel.conversation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatChannelMembershipRemovalPreview {
        agent_id: request.agent_id,
        channel_id: request.channel_id,
        active_assignment_count: u64_value(active_assignment_count)?,
        active_authorization_count: u64_value(active_authorization_count)?,
        will_revoke_active_work: active_assignment_count > 0 || active_authorization_count > 0,
    })
}

/// Removes an agent from one channel conversation: marks its agent channel membership and
/// conversation membership removed.
///
/// The rows stay, so a removal is an ordinary replicated update rather than a delete, and adding
/// the agent back clears the removal.
pub(in crate::chat::organization) async fn remove_agent_from_conversation(
    transaction: &mut Transaction<'_, Sqlite>,
    agent_id: &ChatParticipantId,
    conversation_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_agent_channel_memberships SET removed_at = ?, updated_at = ?
         WHERE conversation_id = ? AND agent_id = ? AND removed_at IS NULL",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(conversation_id)
    .bind(agent_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_conversation_memberships
         SET removed_at = ?, revision = revision + 1, updated_at = ?
         WHERE conversation_id = ? AND participant_id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(conversation_id)
    .bind(agent_id.as_str())
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

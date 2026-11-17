use super::{
    ChatChannelId, ChatChannelNoteId, ChatConversationId, ChatConversationItemId,
    ChatConversationMembershipRead, ChatParticipantId, ChatWorkAssignmentState, UtcTimestamp,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelRead {
    pub id: ChatChannelId,
    pub conversation_id: ChatConversationId,
    pub project_id: String,
    pub name: String,
    pub topic: String,
    pub is_default: bool,
    pub memberships: Vec<ChatConversationMembershipRead>,
    pub message_count: u64,
    /// Top-level messages from others after the local read cursor.
    pub unread_count: u64,
    /// Replies from others in threads the local person took part in, posted since the channel
    /// was last opened and not read in their thread.
    pub unread_reply_count: u64,
    /// Whether a reply thread of the channel ends in a failure the local person has not seen,
    /// neither by opening the channel after it nor by reading its thread.
    pub unseen_failure: bool,
    pub latest_preview: Option<String>,
    pub last_activity_at: UtcTimestamp,
    pub attention_state: Option<ChatWorkAssignmentState>,
    pub revision: u64,
    pub archived_at: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

/// A live channel note: a short fact a person saved in a channel, with the agent update and
/// agent it came from when a person accepted a proposal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelNoteRead {
    pub id: ChatChannelNoteId,
    pub channel_id: ChatChannelId,
    pub body: String,
    pub source_item_id: Option<ChatConversationItemId>,
    pub proposed_by: Option<ChatParticipantId>,
    pub edited_by: Option<ChatParticipantId>,
    pub revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

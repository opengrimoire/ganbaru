use super::{
    ChatChannelId, ChatConversationId, ChatConversationMembershipRead, ChatWorkAssignmentState,
    UtcTimestamp,
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
    pub unread_count: u64,
    pub latest_preview: Option<String>,
    pub last_activity_at: UtcTimestamp,
    pub attention_state: Option<ChatWorkAssignmentState>,
    pub revision: u64,
    pub archived_at: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

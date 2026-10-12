//! Stable wire contracts for organizational Chat commands.

use super::super::models::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammatePolicyInput {
    pub provider_instance_id: ProviderInstanceId,
    pub safety_mode: ChatApprovalPolicy,
    #[serde(default)]
    pub provider_managed_model: bool,
    pub model_id: Option<ModelId>,
    #[serde(default)]
    pub model_options: Vec<ModelOptionSelection>,
    pub effort: Option<String>,
    pub speed: Option<String>,
    pub provider_options: VersionedJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateChatTeammateCommand {
    pub teammate_id: ChatParticipantId,
    pub display_name: String,
    pub avatar: VersionedJson,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub instructions: String,
    pub policy: ChatTeammatePolicyInput,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessProfileRevisionInput {
    pub default_channel_capabilities: ChatChannelCapabilities,
    pub default_history_boundary: ChatHistoryBoundary,
    pub maximum_folder_capability: ChatFolderCapability,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatAccessProfileCommand {
    pub access_profile_id: ChatAccessProfileId,
    pub display_name: String,
    pub revision: ChatAccessProfileRevisionInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateChatAccessProfileCommand {
    pub source_access_profile_id: ChatAccessProfileId,
    pub access_profile_id: ChatAccessProfileId,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishChatAccessProfileRevisionCommand {
    pub access_profile_id: ChatAccessProfileId,
    pub expected_revision: u64,
    pub revision: ChatAccessProfileRevisionInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChatAccessProfileRevisionCommand {
    pub access_profile_id: ChatAccessProfileId,
    pub expected_revision: u64,
    pub revision: ChatAccessProfileRevisionInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveChatAccessProfileCommand {
    pub access_profile_id: ChatAccessProfileId,
    pub expected_revision: u64,
    pub archived: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatFolderGrantInput {
    pub working_folder_id: ProjectWorkingFolderId,
    pub capability: ChatFolderCapability,
    #[serde(default)]
    pub is_default: bool,
    pub runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammateChannelAccessInput {
    pub channel_id: ChatChannelId,
    pub access_profile_id: ChatAccessProfileId,
    pub access_profile_revision: u64,
    pub capabilities: ChatChannelCapabilities,
    pub history_boundary: ChatHistoryBoundary,
    pub runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub scratch_runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    #[serde(default)]
    pub folder_grants: Vec<ChatFolderGrantInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreviewChatTeammateAccessCommand {
    pub teammate_id: ChatParticipantId,
    pub expected_access_revision: u64,
    pub teammate_default_runtime_approval: ChatRuntimeApprovalPolicy,
    #[serde(default)]
    pub channels: Vec<ChatTeammateChannelAccessInput>,
    #[serde(default)]
    pub teammate_profile: Option<UpdateChatTeammateProfileCommand>,
    #[serde(default)]
    pub policy: Option<ChatTeammatePolicyInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceChatTeammateAccessCommand {
    pub teammate_id: ChatParticipantId,
    pub expected_access_revision: u64,
    pub teammate_default_runtime_approval: ChatRuntimeApprovalPolicy,
    #[serde(default)]
    pub channels: Vec<ChatTeammateChannelAccessInput>,
    #[serde(default)]
    pub teammate_profile: Option<UpdateChatTeammateProfileCommand>,
    #[serde(default)]
    pub policy: Option<ChatTeammatePolicyInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChatChannelMembershipRemovalCommand {
    pub teammate_id: ChatParticipantId,
    pub channel_id: ChatChannelId,
    pub expected_access_revision: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChatAssignmentTargetsCommand {
    pub teammate_id: ChatParticipantId,
    pub channel_id: ChatChannelId,
    pub reply_thread_id: Option<ChatReplyThreadId>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseChatScratchGenerationCommand {
    pub scratch_generation_id: ChatScratchGenerationId,
    #[serde(default)]
    pub relative_path: String,
    pub cursor: Option<String>,
    pub limit: u32,
    #[serde(default)]
    pub allow_restricted_inspection: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChatScratchPromotionDestinationInput {
    WorkingFolder {
        channel_id: ChatChannelId,
        working_folder_id: ProjectWorkingFolderId,
        relative_path: String,
    },
    ManagedAttachment {
        channel_id: ChatChannelId,
        attachment_id: ChatAttachmentId,
        display_name: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromoteChatScratchFileCommand {
    pub promotion_id: ChatScratchPromotionId,
    pub scratch_generation_id: ChatScratchGenerationId,
    pub source_relative_path: String,
    pub expected_content_revision: String,
    pub destination: ChatScratchPromotionDestinationInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChatScratchCleanupCommand {
    pub scratch_generation_id: ChatScratchGenerationId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupChatScratchCommand {
    pub scratch_generation_id: ChatScratchGenerationId,
    pub expected_scope_revision: u64,
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChatTeammateProfileCommand {
    pub teammate_id: ChatParticipantId,
    pub display_name: String,
    pub avatar: VersionedJson,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub instructions: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostChatMessageCommand {
    pub client_command_id: ChatCommandId,
    pub channel_id: ChatChannelId,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub normalized_markdown: String,
    pub rich_content: VersionedJson,
    #[serde(default)]
    pub attachment_ids: Vec<ChatAttachmentId>,
    #[serde(default)]
    pub references: Vec<ChatMessageReference>,
    #[serde(default)]
    pub execution_target: Option<ChatExecutionTarget>,
    #[serde(default)]
    pub also_send_to_channel: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleChatMessageCommand {
    pub scheduled_message_id: ChatScheduledMessageId,
    pub scheduled_for: UtcTimestamp,
    pub message: PostChatMessageCommand,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostChatMessageResult {
    pub message: ChatMessageRead,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub assignment: Option<ChatWorkAssignmentRead>,
    pub assignment_input_queued: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        ChatScratchPromotionDestinationInput, CreateChatTeammateCommand,
        PreviewChatTeammateAccessCommand,
    };
    use serde_json::json;

    fn teammate_payload() -> serde_json::Value {
        json!({
            "teammateId": "teammate:reviewer",
            "displayName": "Reviewer",
            "avatar": { "schemaVersion": 1, "value": { "kind": "initials" } },
            "role": "Code reviewer",
            "instructions": "",
            "policy": {
                "providerInstanceId": "provider:test",
                "safetyMode": "ask_for_approval",
                "providerManagedModel": true,
                "modelId": null,
                "modelOptions": [],
                "effort": null,
                "speed": null,
                "providerOptions": { "schemaVersion": 1, "value": {} }
            }
        })
    }

    #[test]
    fn standalone_teammate_creation_has_no_access_input() {
        let payload = teammate_payload();
        assert!(serde_json::from_value::<CreateChatTeammateCommand>(payload).is_ok());

        let mut legacy_payload = teammate_payload();
        legacy_payload["memberships"] = json!([{
            "channelId": "channel:general"
        }]);
        assert!(serde_json::from_value::<CreateChatTeammateCommand>(legacy_payload).is_err());
    }

    #[test]
    fn managed_scratch_attachment_destination_has_no_folder_authority() {
        let destination = json!({
            "kind": "managedAttachment",
            "channelId": "channel:general",
            "attachmentId": "attachment:result",
            "displayName": "result.md"
        });
        assert!(
            serde_json::from_value::<ChatScratchPromotionDestinationInput>(destination).is_ok()
        );

        let legacy_destination = json!({
            "kind": "managedAttachment",
            "channelId": "channel:general",
            "workingFolderId": "folder:implicit",
            "attachmentId": "attachment:result",
            "displayName": "result.md"
        });
        assert!(
            serde_json::from_value::<ChatScratchPromotionDestinationInput>(legacy_destination)
                .is_err()
        );
    }

    #[test]
    fn teammate_access_preview_accepts_the_complete_atomic_draft() {
        let payload = json!({
            "teammateId": "teammate:reviewer",
            "expectedAccessRevision": 4,
            "teammateDefaultRuntimeApproval": "ask",
            "channels": [],
            "teammateProfile": {
                "teammateId": "teammate:reviewer",
                "displayName": "Reviewer",
                "avatar": { "schemaVersion": 1, "value": { "kind": "initials" } },
                "role": "Code reviewer",
                "instructions": "Review changes",
                "expectedRevision": 7
            },
            "policy": {
                "providerInstanceId": "provider:test",
                "safetyMode": "ask_for_approval",
                "providerManagedModel": true,
                "modelId": null,
                "modelOptions": [],
                "effort": null,
                "speed": null,
                "providerOptions": { "schemaVersion": 1, "value": {} }
            }
        });
        let command = serde_json::from_value::<PreviewChatTeammateAccessCommand>(payload.clone())
            .expect("the complete draft should deserialize");
        assert!(command.teammate_profile.is_some());
        assert!(command.policy.is_some());

        let mut unknown = payload;
        unknown["legacyMemberships"] = json!([]);
        assert!(serde_json::from_value::<PreviewChatTeammateAccessCommand>(unknown).is_err());
    }
}

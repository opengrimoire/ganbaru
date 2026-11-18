//! Stable wire contracts for organizational Chat commands.

use super::super::models::*;
use crate::scheduling::RecurrenceSpec;
use serde::{Deserialize, Serialize};

/// An agent's default model, as Settings > Agents edits it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatAgentPolicyInput {
    #[serde(default)]
    pub provider_managed_model: bool,
    pub model_id: Option<ModelId>,
    /// Options of the model, effort among them.
    #[serde(default)]
    pub model_options: Vec<ModelOptionSelection>,
    pub provider_options: VersionedJson,
    #[serde(default = "default_interaction_mode")]
    pub interaction_mode: InteractionMode,
}

fn default_interaction_mode() -> InteractionMode {
    InteractionMode::Build
}

impl ChatAgentPolicyInput {
    /// The model selection the policy stores.
    pub fn model_selection(&self) -> ChatModelSelection {
        ChatModelSelection {
            provider_managed_model: self.provider_managed_model,
            model_id: self.model_id.clone(),
            model_options: self.model_options.clone(),
        }
    }
}

/// Saves a new default model for an agent.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetChatAgentPolicyCommand {
    pub agent_id: ChatParticipantId,
    pub policy: ChatAgentPolicyInput,
}

/// Saves the app scope of agent settings.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateChatAppAgentSettingsCommand {
    pub access: ChatAgentAccess,
    #[serde(default)]
    pub instructions: String,
    pub expected_revision: u64,
}

/// Saves the project scope of agent settings; an absent access inherits the app access.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateChatProjectAgentSettingsCommand {
    pub project_id: String,
    pub access: Option<ChatAgentAccess>,
    #[serde(default)]
    pub instructions: String,
    /// Zero while the project has never saved its own settings.
    pub expected_revision: u64,
}

/// Saves the channel scope of agent settings; an absent access inherits the project access.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateChatChannelAgentSettingsCommand {
    pub channel_id: ChatChannelId,
    pub access: Option<ChatAgentAccess>,
    #[serde(default)]
    pub instructions: String,
    pub expected_revision: u64,
}

/// Sets or clears the model one agent starts on in one channel.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetChatChannelModelOverrideCommand {
    pub channel_id: ChatChannelId,
    pub agent_id: ChatParticipantId,
    /// `None` clears the override, so the agent's default model applies.
    pub model: Option<ChatModelSelection>,
}

/// Adds an agent to a channel, where it works under the channel's access at once.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddChatChannelAgentCommand {
    pub channel_id: ChatChannelId,
    pub agent_id: ChatParticipantId,
}

/// Removes an agent from a channel, which revokes its running work there and its push
/// approvals.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RemoveChatChannelAgentCommand {
    pub channel_id: ChatChannelId,
    pub agent_id: ChatParticipantId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChatChannelMembershipRemovalCommand {
    pub agent_id: ChatParticipantId,
    pub channel_id: ChatChannelId,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChatAssignmentTargetsCommand {
    pub agent_id: ChatParticipantId,
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
    /// How a reply to a thread with active work reaches that work; `None` steers working
    /// work and queues anything else. A scheduled post refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_routing: Option<ChatFollowupRouting>,
}

/// How a reply reaches the active work of its thread.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFollowupRouting {
    /// Into the running provider turn, when the work is working; queued otherwise.
    Steer,
    /// After the current provider turn ends, as the next turn of the work.
    Queue,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleChatMessageCommand {
    pub scheduled_message_id: ChatScheduledMessageId,
    /// Delivery time of a one-time message, or the anchor of a repeating one: its first
    /// occurrence is the first the rule names at or after this time.
    pub scheduled_for: UtcTimestamp,
    pub message: PostChatMessageCommand,
    /// Repeat rule; `None` sends the message once.
    #[serde(default)]
    pub recurrence: Option<RecurrenceSpec>,
}

/// New content, delivery time, and repeat rule for a message still waiting to be sent. The
/// message keeps its id, client command id, channel, reply thread, and executing device.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledChatMessageCommand {
    pub scheduled_message_id: ChatScheduledMessageId,
    /// Delivery time of a one-time message, or the time from which a repeating one next posts.
    pub scheduled_for: UtcTimestamp,
    pub normalized_markdown: String,
    pub rich_content: VersionedJson,
    #[serde(default)]
    pub attachment_ids: Vec<ChatAttachmentId>,
    #[serde(default)]
    pub references: Vec<ChatMessageReference>,
    #[serde(default)]
    pub also_send_to_channel: bool,
    /// Repeat rule; `None` sends the message once.
    #[serde(default)]
    pub recurrence: Option<RecurrenceSpec>,
}

/// A message waiting to be sent, once or on a repeat rule.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScheduledMessageRead {
    pub id: ChatScheduledMessageId,
    pub channel_id: ChatChannelId,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub normalized_markdown: String,
    pub rich_content: VersionedJson,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub references: Vec<ChatMessageReference>,
    pub also_send_to_channel: bool,
    pub state: String,
    /// Delivery time of a one-time message, or the anchor of a repeating one.
    pub scheduled_for: UtcTimestamp,
    pub last_error: Option<String>,
    pub created_at: UtcTimestamp,
    /// Repeat rule and progress, or `None` for a one-time message.
    pub recurrence: Option<ChatScheduledRecurrenceRead>,
}

/// The repeat rule of a scheduled message and how far its series has come.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScheduledRecurrenceRead {
    /// Rule, zone, and quiet policy.
    #[serde(flatten)]
    pub spec: RecurrenceSpec,
    /// When the next occurrence is due.
    pub next_occurrence_at: UtcTimestamp,
    /// Occurrences the series has posted.
    pub fired_count: u64,
    /// Occurrences this device skipped because they were missed or quiet; device-local.
    pub skipped_occurrences: u64,
    /// End of the quiet period the pending occurrence waits for on this device, if any.
    pub deferred_until: Option<UtcTimestamp>,
}

/// A person's confirmation of an agent's schedule proposal, with the time, rule, and text
/// they chose. The confirmed message mentions the proposing agent in the proposal's reply
/// thread.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmScheduleProposalCommand {
    /// The conversation item that carries the proposal.
    pub proposal_item_id: ChatConversationItemId,
    /// Delivery time of a one-time message, or the anchor of a repeating one.
    pub scheduled_for: UtcTimestamp,
    /// Repeat rule; `None` sends the message once.
    #[serde(default)]
    pub recurrence: Option<RecurrenceSpec>,
    /// Edited message text without the agent mention the confirmation adds; `None` keeps
    /// the proposed text.
    #[serde(default)]
    pub normalized_markdown: Option<String>,
}

/// Where a schedule proposal stands on this device.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatScheduleProposalState {
    /// Nobody confirmed it and this device did not dismiss it.
    Pending,
    /// A person confirmed it on some device: its scheduled message exists or posted.
    Confirmed,
    /// This device dismissed it.
    Dismissed,
}

/// The state of one schedule proposal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScheduleProposalStateRead {
    pub proposal_item_id: ChatConversationItemId,
    pub state: ChatScheduleProposalState,
}

/// A person's confirmation of an agent's push request. After the push, the confirmation posts
/// `normalized_markdown` after a mention of the asking agent in the request's reply thread,
/// which answers the request.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmPushRequestCommand {
    /// The conversation item that carries the push request.
    pub request_item_id: ChatConversationItemId,
    /// The reply text without the agent mention the confirmation adds.
    pub normalized_markdown: String,
    /// Whether the person also approves later pushes of the agent from this folder to the
    /// same push URL and destination branch, recorded only once this push succeeds.
    #[serde(default)]
    pub remember_approval: bool,
}

/// Where a push request stands.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPushRequestState {
    /// The asking run is still working, so nobody can confirm it yet.
    Waiting,
    /// A person can confirm it.
    Open,
    /// A person confirmed it on some device and the push succeeded.
    Pushed,
    /// It was answered, superseded, or its run did not complete.
    Closed,
}

/// The state of one push request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPushRequestStateRead {
    pub request_item_id: ChatConversationItemId,
    pub state: ChatPushRequestState,
}

/// Revokes a standing push approval, so later matching push requests ask again.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RevokeChatPushApprovalCommand {
    pub approval_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostChatMessageResult {
    pub message: ChatMessageRead,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub assignment: Option<ChatWorkAssignmentRead>,
    pub assignment_input_queued: bool,
    /// How the post reached active work as an input, or `None` when it reached no active
    /// work. A steer request on work that is not working reports `Queue`.
    #[serde(default)]
    pub routed_as: Option<ChatFollowupRouting>,
}

#[cfg(test)]
mod tests {
    use super::{
        ChatAgentAccess, ChatScratchPromotionDestinationInput, InteractionMode,
        SetChatAgentPolicyCommand, SetChatChannelModelOverrideCommand,
        UpdateChatChannelAgentSettingsCommand, UpdateChatProjectAgentSettingsCommand,
    };
    use serde_json::json;

    fn policy_payload() -> serde_json::Value {
        json!({
            "agentId": "agent:codex",
            "policy": {
                "providerManagedModel": true,
                "modelId": null,
                "modelOptions": [],
                "providerOptions": { "schemaVersion": 1, "value": {} }
            }
        })
    }

    #[test]
    fn agent_policy_keeps_only_the_model_and_provider_options() {
        let command = serde_json::from_value::<SetChatAgentPolicyCommand>(policy_payload())
            .expect("a model policy should deserialize");
        assert_eq!(command.policy.interaction_mode, InteractionMode::Build);
        assert!(command.policy.model_selection().provider_managed_model);

        for removed in ["safetyMode", "effort", "speed", "providerInstanceId"] {
            let mut payload = policy_payload();
            payload["policy"][removed] = json!("ask_for_approval");
            assert!(
                serde_json::from_value::<SetChatAgentPolicyCommand>(payload).is_err(),
                "{removed} is no longer part of an agent policy"
            );
        }
    }

    #[test]
    fn scope_settings_inherit_with_an_absent_access() {
        let project = serde_json::from_value::<UpdateChatProjectAgentSettingsCommand>(json!({
            "projectId": "project:a",
            "access": null,
            "instructions": "Use the house style",
            "expectedRevision": 0
        }))
        .unwrap();
        assert_eq!(project.access, None);

        let channel = serde_json::from_value::<UpdateChatChannelAgentSettingsCommand>(json!({
            "channelId": "channel:general",
            "access": "talk",
            "expectedRevision": 3
        }))
        .unwrap();
        assert_eq!(channel.access, Some(ChatAgentAccess::Talk));
        assert_eq!(channel.instructions, "");

        assert!(
            serde_json::from_value::<UpdateChatChannelAgentSettingsCommand>(json!({
                "channelId": "channel:general",
                "access": "unattended",
                "expectedRevision": 3
            }))
            .is_err()
        );
    }

    #[test]
    fn channel_model_override_sets_or_clears() {
        let cleared = serde_json::from_value::<SetChatChannelModelOverrideCommand>(json!({
            "channelId": "channel:general",
            "agentId": "agent:codex",
            "model": null
        }))
        .unwrap();
        assert!(cleared.model.is_none());

        let set = serde_json::from_value::<SetChatChannelModelOverrideCommand>(json!({
            "channelId": "channel:general",
            "agentId": "agent:codex",
            "model": { "modelId": "gpt-5", "modelOptions": [] }
        }))
        .unwrap();
        assert_eq!(set.model.unwrap().model_id.unwrap().as_str(), "gpt-5");
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
}

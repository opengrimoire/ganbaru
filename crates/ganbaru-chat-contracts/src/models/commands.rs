use super::{
    ApprovalDecisionKind, ChatAttachmentId, ChatCheckpointId, ChatCommandId, ChatRequestId,
    ChatThreadId, ChatTurnId, ContinuationGroupId, ModelId, ModelOptionSelection,
    ProjectWorkingFolderId, ProviderInstanceId, ProviderRequestId, ProviderSessionId,
    ProviderThreadId, RepositoryKind, TurnModeSnapshot, VersionedJson,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCommandContext {
    pub client_command_id: ChatCommandId,
    pub expected_thread_revision: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedWorkspaceContext {
    pub working_folder_id: ProjectWorkingFolderId,
    pub canonical_path: String,
    pub repository_kind: RepositoryKind,
    pub repository_identity: Option<String>,
    /// Whether the provider may load project configuration that can run commands.
    #[serde(default)]
    pub project_configuration: ProjectConfigurationAccess,
}

/// Whether a provider loads the workspace's project configuration, such as
/// Claude project settings, hooks, skills, and MCP servers.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConfigurationAccess {
    /// This device trusts the folder's current project provider configuration.
    Trusted,
    /// Project configuration that can run commands stays unloaded.
    #[default]
    Restricted,
}

/// How a provider honors a restricted project configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectConfigurationRestriction {
    /// The launch arguments keep project configuration out of the run.
    Enforced,
    /// No verified switch exists, so restricted folders with configuration cannot run.
    Unavailable,
}

/// Project provider configuration path, relative to the folder root, whose
/// content can run commands when a provider starts there.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProviderConfigurationPath {
    /// `.claude/settings.json`.
    ClaudeSettings,
    /// `.claude/settings.local.json`.
    ClaudeLocalSettings,
    /// `.mcp.json`.
    ProjectMcp,
    /// `opencode.json`.
    OpenCodeJson,
    /// `opencode.jsonc`.
    OpenCodeJsonc,
    /// `.cursorrules`.
    CursorRules,
    /// `.claude/skills`.
    ClaudeSkills,
    /// `.claude/agents`.
    ClaudeAgents,
    /// `.claude/commands`.
    ClaudeCommands,
    /// `.codex`, which holds Codex project configuration, hooks, and rules.
    Codex,
    /// `.opencode`, which holds OpenCode plugins and the packages they load.
    OpenCode,
    /// `.cursor`, which holds Cursor CLI permissions, hooks, MCP servers,
    /// rules, commands, and skills.
    Cursor,
    /// `.grok`, which holds Grok configuration with MCP servers, plugins, and
    /// permissions, and Grok hooks, skills, and plugins.
    Grok,
}

impl ProviderConfigurationPath {
    /// Every path, files first and then directories.
    pub const ALL: &'static [Self] = &[
        Self::ClaudeSettings,
        Self::ClaudeLocalSettings,
        Self::ProjectMcp,
        Self::OpenCodeJson,
        Self::OpenCodeJsonc,
        Self::CursorRules,
        Self::ClaudeSkills,
        Self::ClaudeAgents,
        Self::ClaudeCommands,
        Self::Codex,
        Self::OpenCode,
        Self::Cursor,
        Self::Grok,
    ];

    /// Path relative to the folder root, with `/` separators.
    pub const fn relative_path(self) -> &'static str {
        match self {
            Self::ClaudeSettings => ".claude/settings.json",
            Self::ClaudeLocalSettings => ".claude/settings.local.json",
            Self::ProjectMcp => ".mcp.json",
            Self::OpenCodeJson => "opencode.json",
            Self::OpenCodeJsonc => "opencode.jsonc",
            Self::CursorRules => ".cursorrules",
            Self::ClaudeSkills => ".claude/skills",
            Self::ClaudeAgents => ".claude/agents",
            Self::ClaudeCommands => ".claude/commands",
            Self::Codex => ".codex",
            Self::OpenCode => ".opencode",
            Self::Cursor => ".cursor",
            Self::Grok => ".grok",
        }
    }

    /// True when the path names a directory whose whole tree counts.
    pub const fn is_directory(self) -> bool {
        matches!(
            self,
            Self::ClaudeSkills
                | Self::ClaudeAgents
                | Self::ClaudeCommands
                | Self::Codex
                | Self::OpenCode
                | Self::Cursor
                | Self::Grok
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptAttachmentReference {
    pub attachment_id: ChatAttachmentId,
    pub kind: String,
    pub display_name: String,
    pub managed_relative_path: String,
    pub resource_uri: String,
    pub mime_type: Option<String>,
    pub byte_size: u64,
    pub local_path: Option<String>,
    pub text_content: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMentionReference {
    pub relative_path: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSessionRequest {
    pub thread_id: ChatThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub provider_instance_id: ProviderInstanceId,
    pub modes: TurnModeSnapshot,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    /// Instructions of the session, which providers place at system level when the session
    /// opens. Direct threads carry none.
    #[serde(default)]
    pub developer_instructions: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeSessionRequest {
    pub thread_id: ChatThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub provider_instance_id: ProviderInstanceId,
    pub provider_thread_id: ProviderThreadId,
    pub continuation_group_id: ContinuationGroupId,
    pub resume_cursor: VersionedJson,
    pub modes: TurnModeSnapshot,
    /// Instructions of the resumed session, placed at system level like on start.
    #[serde(default)]
    pub developer_instructions: Option<String>,
    /// Whether the native session already received `developer_instructions` on an earlier
    /// turn. Providers that deliver instructions in prompt text then omit them from the
    /// resumed session's turns; providers that set them at system level ignore it.
    #[serde(default)]
    pub developer_instructions_delivered: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderThreadLifecycleRequest {
    pub provider_thread_id: ProviderThreadId,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderForkThreadRequest {
    pub provider_thread_id: ProviderThreadId,
    pub workspace: VerifiedWorkspaceContext,
    pub last_provider_turn_id: Option<String>,
}

/// Largest `SendTurnRequest::developer_instructions`, in UTF-8 bytes, that every provider
/// accepts. The Claude and Codex protocols carry the instructions under this cap.
pub const MAX_DEVELOPER_INSTRUCTIONS_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
    pub prompt: String,
    pub attachments: Vec<PromptAttachmentReference>,
    pub mentions: Vec<WorkspaceMentionReference>,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub modes: TurnModeSnapshot,
    pub developer_instructions: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
    pub prompt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterruptTurnRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub turn_id: ChatTurnId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDecision {
    pub kind: ApprovalDecisionKind,
    pub provider_option_id: Option<String>,
    pub updated_tool_input: Option<VersionedJson>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveApprovalRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub decision: ApprovalDecision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInputAnswer {
    pub question_id: String,
    pub selected_option_ids: Vec<String>,
    pub free_form_text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveUserInputRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub request_id: ChatRequestId,
    pub provider_request_id: ProviderRequestId,
    pub answers: Vec<UserInputAnswer>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackRequest {
    pub command: ChatCommandContext,
    pub session_id: ProviderSessionId,
    pub checkpoint_id: Option<ChatCheckpointId>,
    pub provider_cursor: Option<VersionedJson>,
    pub target_turn_id: Option<ChatTurnId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadHistoryRequest {
    pub session_id: ProviderSessionId,
    pub cursor: Option<String>,
    pub limit: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSessionRequest {
    pub session_id: ProviderSessionId,
    pub force: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinuationGroupRequest {
    pub provider_instance_id: ProviderInstanceId,
    pub normalized_provider_home: Option<String>,
    pub account_identity: Option<String>,
    pub server_identity: Option<String>,
    pub provider_fields: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::{ResumeSessionRequest, StartSessionRequest};
    use serde_json::{Value, json};

    fn start_json() -> Value {
        json!({
            "threadId": "thread-1",
            "workspace": {
                "workingFolderId": "folder-1",
                "canonicalPath": "/workspace",
                "repositoryKind": "git",
                "repositoryIdentity": null,
                "projectConfiguration": "restricted"
            },
            "providerInstanceId": "claude-personal",
            "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" },
            "modelId": null,
            "modelOptions": []
        })
    }

    fn resume_json() -> Value {
        json!({
            "threadId": "thread-1",
            "workspace": {
                "workingFolderId": "folder-1",
                "canonicalPath": "/workspace",
                "repositoryKind": "git",
                "repositoryIdentity": null,
                "projectConfiguration": "restricted"
            },
            "providerInstanceId": "claude-personal",
            "providerThreadId": "provider-thread-1",
            "continuationGroupId": "group-1",
            "resumeCursor": { "schemaVersion": 1, "value": {} },
            "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" }
        })
    }

    #[test]
    fn session_requests_round_trip_developer_instructions() {
        let mut start = start_json();
        start["developerInstructions"] = json!("Review the plan first.");
        let parsed: StartSessionRequest = serde_json::from_value(start.clone()).unwrap();
        assert_eq!(
            parsed.developer_instructions.as_deref(),
            Some("Review the plan first.")
        );
        assert_eq!(serde_json::to_value(&parsed).unwrap(), start);

        let mut resume = resume_json();
        resume["developerInstructions"] = json!("Review the plan first.");
        resume["developerInstructionsDelivered"] = json!(true);
        let parsed: ResumeSessionRequest = serde_json::from_value(resume.clone()).unwrap();
        assert_eq!(
            parsed.developer_instructions.as_deref(),
            Some("Review the plan first.")
        );
        assert!(parsed.developer_instructions_delivered);
        assert_eq!(serde_json::to_value(&parsed).unwrap(), resume);
    }

    #[test]
    fn session_requests_without_instructions_read_as_none_and_write_null() {
        let start: StartSessionRequest = serde_json::from_value(start_json()).unwrap();
        assert_eq!(start.developer_instructions, None);
        assert_eq!(
            serde_json::to_value(&start).unwrap()["developerInstructions"],
            Value::Null
        );
        let resume: ResumeSessionRequest = serde_json::from_value(resume_json()).unwrap();
        assert_eq!(resume.developer_instructions, None);
        assert!(
            !resume.developer_instructions_delivered,
            "a resume that does not say otherwise has not received the instructions"
        );
        assert_eq!(
            serde_json::to_value(&resume).unwrap()["developerInstructions"],
            Value::Null
        );
    }
}

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use ganbaru_working_folders::RepositoryKind;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownProviderFamily {
    Codex,
    Claude,
    Cursor,
    Grok,
    OpenCode,
    /// Any agent that speaks the Agent Client Protocol without a dedicated driver.
    Acp,
}

impl KnownProviderFamily {
    pub fn id(self) -> &'static str {
        match self {
            Self::Acp => "acp",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Cursor => "cursor",
            Self::Grok => "grok",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyMode {
    AskForApproval,
    ApproveForMe,
    FullAccess,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionMode {
    Build,
    Plan,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatThreadState {
    Draft,
    Active,
    Waiting,
    Idle,
    Error,
    Archived,
    Closed,
}

impl ChatThreadState {
    pub const ALL: [Self; 7] = [
        Self::Draft,
        Self::Active,
        Self::Waiting,
        Self::Idle,
        Self::Error,
        Self::Archived,
        Self::Closed,
    ];

    /// Returns the persisted and wire literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Idle => "idle",
            Self::Error => "error",
            Self::Archived => "archived",
            Self::Closed => "closed",
        }
    }

    /// Returns whether the thread has ended and accepts no further state changes.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Closed)
    }

    /// Returns whether the state can be stored in `chat_threads.state`.
    ///
    /// Archiving is recorded only by `archived_at`, and reads report `Archived` from that
    /// timestamp, so unarchiving restores the activity state the thread already had.
    pub fn is_persisted(self) -> bool {
        !matches!(self, Self::Archived)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSessionState {
    Stopped,
    Starting,
    Ready,
    Active,
    WaitingForApproval,
    WaitingForUserInput,
    Stopping,
    Failed,
}

impl ProviderSessionState {
    pub const ALL: [Self; 8] = [
        Self::Stopped,
        Self::Starting,
        Self::Ready,
        Self::Active,
        Self::WaitingForApproval,
        Self::WaitingForUserInput,
        Self::Stopping,
        Self::Failed,
    ];
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTurnState {
    Pending,
    Dispatching,
    Active,
    WaitingForApproval,
    WaitingForUserInput,
    Completed,
    Interrupted,
    Failed,
}

impl ChatTurnState {
    pub const ALL: [Self; 8] = [
        Self::Pending,
        Self::Dispatching,
        Self::Active,
        Self::WaitingForApproval,
        Self::WaitingForUserInput,
        Self::Completed,
        Self::Interrupted,
        Self::Failed,
    ];

    /// Returns the persisted and wire literal of the state.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Dispatching => "dispatching",
            Self::Active => "active",
            Self::WaitingForApproval => "waiting_for_approval",
            Self::WaitingForUserInput => "waiting_for_user_input",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Interrupted | Self::Failed)
    }

    /// Returns whether a provider holds the turn: it has left `Pending` and is not terminal.
    pub fn is_running(self) -> bool {
        !self.is_terminal() && self != Self::Pending
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStreamingState {
    Pending,
    Streaming,
    Complete,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalItemKind {
    UserMessage,
    AssistantMessage,
    Reasoning,
    Plan,
    CommandExecution,
    FileChange,
    McpToolCall,
    DynamicToolCall,
    CollaborationTask,
    WebSearch,
    ImageView,
    ReviewTransition,
    ContextCompaction,
    Error,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentStreamKind {
    AssistantText,
    ReasoningText,
    ReasoningSummary,
    PlanText,
    CommandOutput,
    FileChangeOutput,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStatus {
    Pending,
    Active,
    Waiting,
    Completed,
    Interrupted,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalRequestKind {
    CommandExecution,
    FileRead,
    FileChange,
    Patch,
    ExecutableCommand,
    ToolInput,
    DynamicTool,
    AuthenticationRefresh,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestResolutionState {
    Open,
    Resolved,
    Stale,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecisionKind {
    AllowOnce,
    AllowSession,
    Deny,
    Cancel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    Healthy,
    ExecutableMissing,
    UnsupportedVersion,
    AuthenticationRequired,
    ConfigurationInvalid,
    TransportUnavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailability {
    Available,
    Unavailable,
    Deprecated,
    Stale,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogSource {
    Provider,
    Cache,
    User,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapability {
    NativeResume,
    NativeRollback,
    NativePlan,
    Steering,
    QueuedFollowUp,
    DynamicModelChange,
    Images,
    FileReferences,
    Skills,
    SlashCommands,
    Approvals,
    StructuredQuestions,
    ReasoningSummaries,
    StructuredPlans,
    ContextUsage,
    CostReporting,
    McpStatus,
    AccountStatus,
    RateLimitStatus,
    ProviderDiffs,
    TaskActivity,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderImplementationStatus {
    MetadataOnly,
    Available,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMaturity {
    Stable,
    Beta,
    Experimental,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionedJson {
    pub schema_version: u32,
    pub value: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnModeSnapshot {
    pub safety_mode: SafetyMode,
    pub interaction_mode: InteractionMode,
}

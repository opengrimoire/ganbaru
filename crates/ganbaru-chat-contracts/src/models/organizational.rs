use super::{
    ChatAccessProfileId, ChatAccessProfileRevisionId, ChatAgentRunId, ChatAssignmentInputId,
    ChatAttachmentId, ChatChannelId, ChatConversationId, ChatConversationItemId,
    ChatExecutionEnvironmentId, ChatMessageReferenceId, ChatMessageRevisionId, ChatParticipantId,
    ChatReplyThreadId, ChatScratchCleanupJobId, ChatScratchGenerationId, ChatScratchPromotionId,
    ChatScratchScopeId, ChatTeammatePolicyRevisionId, ChatThreadId, ChatTurnId,
    ChatWorkAssignmentId, ModelId, ModelOptionSelection, ProjectWorkingFolderId,
    ProviderInstanceId, UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatParticipantKind {
    LocalUser,
    AiTeammate,
    Human,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTeammateConfigurationState {
    Healthy,
    NeedsSetup,
    ProviderUnavailable,
    FolderAccessMissing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatWorkAssignmentState {
    Queued,
    Working,
    WaitingForAnswer,
    WaitingForApproval,
    ReadyForReview,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAssignmentReviewOutcome {
    /// The reviewer accepted the work as finished.
    Accepted,
    /// The reviewer asked for more work, which continues in a follow-up assignment.
    ChangesRequested,
}

/// How a person ended ready-for-review work: recorded only on completed assignments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentReviewRead {
    pub outcome: ChatAssignmentReviewOutcome,
    pub reviewed_at: UtcTimestamp,
    pub reviewed_by_participant_id: ChatParticipantId,
}

/// A review of ready-for-review work.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ChatAssignmentReviewRequest {
    /// Completes the work as accepted.
    Accept,
    /// Posts `note` as a reply that continues the work in a follow-up assignment.
    RequestChanges { note: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatApprovalPolicy {
    AskForApproval,
    ApproveForMe,
    FullAccess,
    Custom,
}

/// Independent capabilities granted to an AI teammate in one channel.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelCapabilities {
    pub read_history: bool,
    pub participate: bool,
}

/// The portion of channel history visible through one membership.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatHistoryBoundary {
    Entire,
    FromGrant {
        /// Captured by the server. Commands may omit this value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lower_order_key: Option<String>,
    },
}

impl ChatHistoryBoundary {
    /// Returns whether this boundary includes all retained channel history.
    pub fn is_entire(&self) -> bool {
        matches!(self, Self::Entire)
    }
}

/// Ordered authority over one logical project working folder.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatFolderCapability {
    None,
    Read,
    Edit,
    Execute,
    Publish,
}

impl ChatFolderCapability {
    /// Returns a stable rank used when intersecting access ceilings.
    pub const fn rank(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Read => 1,
            Self::Edit => 2,
            Self::Execute => 3,
            Self::Publish => 4,
        }
    }

    /// Returns the narrower of two folder capabilities.
    pub const fn intersect(self, other: Self) -> Self {
        if self.rank() <= other.rank() {
            self
        } else {
            other
        }
    }
}

/// Runtime approval behavior after organizational authority has been resolved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatRuntimeApprovalPolicy {
    Ask,
    AutoApprove,
    Unattended,
    ProviderCustom,
}

/// Stable keys for application-provided access profile recipes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatAccessProfileBuiltinKey {
    ConversationOnly,
    ReadOnly,
    EditFiles,
    BuildAndTest,
    PublishChanges,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessProfileRevision {
    pub id: ChatAccessProfileRevisionId,
    pub access_profile_id: ChatAccessProfileId,
    pub revision: u64,
    pub default_channel_capabilities: ChatChannelCapabilities,
    pub default_history_boundary: ChatHistoryBoundary,
    pub maximum_folder_capability: ChatFolderCapability,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessProfileRead {
    pub id: ChatAccessProfileId,
    pub builtin_key: Option<ChatAccessProfileBuiltinKey>,
    pub display_name: String,
    pub latest_revision: ChatAccessProfileRevision,
    pub revision: u64,
    pub archived_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatFolderGrantRead {
    pub working_folder_id: ProjectWorkingFolderId,
    pub display_name: String,
    pub capability: ChatFolderCapability,
    pub is_default: bool,
    pub runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub revision: u64,
    pub revoked_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammateChannelAccessRead {
    pub channel_id: ChatChannelId,
    pub project_id: String,
    /// Group of the channel's project, absent when the project is outside every group.
    pub group_id: Option<String>,
    pub conversation_id: ChatConversationId,
    pub channel_name: String,
    pub access_profile_id: ChatAccessProfileId,
    pub access_profile_revision_id: ChatAccessProfileRevisionId,
    pub access_profile_revision: u64,
    pub capabilities: ChatChannelCapabilities,
    pub history_boundary: ChatHistoryBoundary,
    pub runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub scratch_runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub folder_grants: Vec<ChatFolderGrantRead>,
    pub membership_revision: u64,
    pub removed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammateAccessRead {
    pub teammate_id: ChatParticipantId,
    pub access_revision: u64,
    pub teammate_default_runtime_approval: ChatRuntimeApprovalPolicy,
    pub channels: Vec<ChatTeammateChannelAccessRead>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAccessIssueCode {
    ArchivedChannel,
    ArchivedFolder,
    CrossProjectFolder,
    DuplicateChannel,
    DuplicateFolder,
    MultipleDefaultFolders,
    ProfileCeilingExceeded,
    ProfileNotFound,
    RetainedReferenceDisclosure,
    TeammateNotFound,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessValidationIssue {
    pub code: ChatAccessIssueCode,
    pub field_path: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammateAccessPreview {
    pub proposed: Option<ChatTeammateAccessRead>,
    pub is_expansion: bool,
    pub added_channel_ids: Vec<ChatChannelId>,
    pub removed_channel_ids: Vec<ChatChannelId>,
    pub issues: Vec<ChatAccessValidationIssue>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAccessProfileImpactPreview {
    pub access_profile_id: ChatAccessProfileId,
    pub current_revision: ChatAccessProfileRevision,
    pub is_expansion: bool,
    pub is_reduction: bool,
    pub affected_teammate_ids: Vec<ChatParticipantId>,
    pub affected_channel_ids: Vec<ChatChannelId>,
    pub active_authorization_count: u64,
    pub issues: Vec<ChatAccessValidationIssue>,
}

/// The one native environment selected for an agent run.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatExecutionTarget {
    Scratch {
        scratch_scope_id: ChatScratchScopeId,
        scratch_generation_id: ChatScratchGenerationId,
        execution_environment_id: ChatExecutionEnvironmentId,
    },
    WorkingFolder {
        working_folder_id: ProjectWorkingFolderId,
        execution_environment_id: ChatExecutionEnvironmentId,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatAssignmentTargetKind {
    Scratch,
    CurrentFolder,
    Worktree,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatAssignmentTargetBindingState {
    Ready,
    Locate,
    Relink,
    Clone,
    Missing,
    UnavailableOnThisDevice,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAssignmentTargetLifecycleState {
    Creating,
    Available,
    Missing,
    CleanupPending,
    CleanupFailed,
    Removed,
}

/// One provider-neutral candidate for the run's single native target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentTargetRead {
    /// The existing environment this target names. Discovery lists only environments that exist, so it is set on
    /// every listed target.
    pub execution_target: Option<ChatExecutionTarget>,
    pub kind: ChatAssignmentTargetKind,
    pub display_name: String,
    pub folder_capability: Option<ChatFolderCapability>,
    pub effective_runtime_approval: ChatRuntimeApprovalPolicy,
    pub is_default: bool,
    pub binding_state: ChatAssignmentTargetBindingState,
    pub lifecycle_state: ChatAssignmentTargetLifecycleState,
    pub is_busy: bool,
    pub is_dirty: Option<bool>,
    pub eligible: bool,
    pub unavailable_reason: Option<String>,
}

/// Durable lifecycle of one private scratch scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchScopeLifecycleState {
    Active,
    Archived,
    CleanupPending,
    CleanupFailed,
    Removed,
}

/// Durable lifecycle of one private scratch generation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchGenerationLifecycleState {
    Active,
    Quarantined,
    CleanupPending,
    CleanupFailed,
    Removed,
}

/// Device-local availability without exposing the managed absolute path.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchDeviceAvailability {
    Available,
    UnavailableOnThisDevice,
}

/// One frozen source constraint retained by a scratch generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchSourceSummaryRead {
    pub channel_id: ChatChannelId,
    pub channel_name: String,
    pub lower_order_key: String,
    pub high_order_key: String,
    pub audience_revision: u64,
}

/// One logical generation and its derived state on the current device.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchGenerationRead {
    pub id: ChatScratchGenerationId,
    pub execution_environment_id: ChatExecutionEnvironmentId,
    pub generation: u64,
    pub lifecycle_state: ChatScratchGenerationLifecycleState,
    pub byte_size: u64,
    pub entry_count: u64,
    pub size_truncated: bool,
    pub device_availability: ChatScratchDeviceAvailability,
    pub retained_sources: Vec<ChatScratchSourceSummaryRead>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

/// Private scratch storage owned by one teammate in one reply thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchScopeRead {
    pub id: ChatScratchScopeId,
    pub reply_thread_id: ChatReplyThreadId,
    pub teammate_id: ChatParticipantId,
    pub teammate_name: String,
    pub channel_id: ChatChannelId,
    pub channel_name: String,
    pub project_id: String,
    pub project_name: String,
    /// Group of the channel's project, absent when the project is outside every group.
    pub group_id: Option<String>,
    /// Name of that group, absent with it.
    pub group_name: Option<String>,
    pub lifecycle_state: ChatScratchScopeLifecycleState,
    pub revision: u64,
    pub generations: Vec<ChatScratchGenerationRead>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchDirectoryEntryKind {
    Directory,
    File,
}

/// One safe, relative entry in a bounded scratch directory page.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchDirectoryEntryRead {
    pub relative_path: String,
    pub display_name: String,
    pub kind: ChatScratchDirectoryEntryKind,
    pub byte_size: Option<u64>,
    pub content_revision: Option<String>,
    pub promotable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchDirectoryPageRead {
    pub scratch_generation_id: ChatScratchGenerationId,
    pub relative_path: String,
    pub entries: Vec<ChatScratchDirectoryEntryRead>,
    pub next_cursor: Option<String>,
}

/// Exact logical destination of a completed scratch artifact promotion.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatScratchPromotionDestinationRead {
    WorkingFolder {
        working_folder_id: ProjectWorkingFolderId,
        relative_path: String,
    },
    ManagedAttachment {
        channel_id: ChatChannelId,
        attachment_id: ChatAttachmentId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchPromotionResultRead {
    pub id: ChatScratchPromotionId,
    pub scratch_generation_id: ChatScratchGenerationId,
    pub source_relative_path: String,
    pub source_sha256: String,
    pub destination: ChatScratchPromotionDestinationRead,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchCleanupPreviewRead {
    pub scratch_scope_id: ChatScratchScopeId,
    pub scratch_generation_id: ChatScratchGenerationId,
    pub expected_scope_revision: u64,
    pub lifecycle_state: ChatScratchGenerationLifecycleState,
    pub byte_size: u64,
    pub entry_count: u64,
    pub size_truncated: bool,
    pub device_availability: ChatScratchDeviceAvailability,
    pub active_run_count: u64,
    pub will_remove_scope: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchCleanupJobState {
    Pending,
    Running,
    Completed,
    Failed,
    UnavailableOnThisDevice,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchCleanupResultRead {
    pub job_id: ChatScratchCleanupJobId,
    pub scratch_scope_id: ChatScratchScopeId,
    pub scratch_generation_id: ChatScratchGenerationId,
    pub scope_revision: u64,
    pub state: ChatScratchCleanupJobState,
    pub removed_bytes: u64,
    pub device_availability: ChatScratchDeviceAvailability,
    pub completed_at: Option<UtcTimestamp>,
}

/// Why a scratch generation is safe to reclaim, in reporting priority order.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatScratchReclaimReason {
    /// The scope's reply thread work is archived.
    ScopeArchived,
    /// An earlier cleanup of the scope failed and can be retried.
    ScopeCleanupFailed,
    /// An earlier cleanup of the generation failed and can be retried.
    GenerationCleanupFailed,
    /// The generation was quarantined after its authority narrowed.
    GenerationQuarantined,
    /// No run or generation change happened within the idle period.
    Idle,
}

/// One scratch generation that the existing confirmed cleanup can remove.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchReclaimRow {
    pub scratch_scope_id: ChatScratchScopeId,
    pub scratch_generation_id: ChatScratchGenerationId,
    /// Reply thread that owns the scope, used to open its scratch manager.
    pub reply_thread_id: ChatReplyThreadId,
    pub generation: u64,
    pub teammate_name: String,
    pub channel_name: String,
    pub project_name: String,
    pub group_name: Option<String>,
    pub reason: ChatScratchReclaimReason,
    /// Bounded size on this device, or the stored size when the generation is unavailable here.
    pub byte_size: u64,
    pub size_truncated: bool,
    pub device_availability: ChatScratchDeviceAvailability,
    /// Latest generation change or run update.
    pub last_activity_at: UtcTimestamp,
}

/// Largest reclaimable scratch generations with totals over every qualifying generation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScratchReclaimReportRead {
    /// Rows sorted by size, largest first, capped by the report row limit.
    pub rows: Vec<ChatScratchReclaimRow>,
    /// Sum of the sizes available on this device across every qualifying generation.
    pub total_bytes: u64,
    /// Whether any counted size hit its measurement bound.
    pub total_truncated: bool,
    /// Qualifying generations left out of `rows` by the row limit.
    pub omitted: u64,
    /// Days without activity after which active scratch counts as idle.
    pub idle_days: u64,
}

impl ChatWorkAssignmentState {
    /// Returns whether the assignment still occupies the thread's active work slot.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued
                | Self::Working
                | Self::WaitingForAnswer
                | Self::WaitingForApproval
                | Self::ReadyForReview
        )
    }

    /// Returns whether a new message can still be delivered to this assignment.
    pub fn accepts_continuation(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Working | Self::WaitingForAnswer | Self::WaitingForApproval
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ChatAssignmentReviewOutcome, ChatAssignmentReviewRead, ChatAssignmentReviewRequest,
        ChatAttachmentId, ChatChannelId, ChatExecutionTarget, ChatHistoryBoundary,
        ChatMessageReference, ChatParticipantId, ChatReviewQueueKind, ChatReviewQueueRead,
        ChatScratchPromotionDestinationRead, ChatWorkAssignmentState, UtcTimestamp,
    };
    use serde_json::json;

    #[test]
    fn review_queue_round_trips_with_camel_case_fields_and_snake_case_kinds() {
        let wire = json!({
            "items": [{
                "assignmentId": "assignment:review",
                "revision": 3,
                "kind": "review",
                "projectId": "project:alpha",
                "projectName": "Alpha",
                "channelId": "channel:general",
                "channelName": "general",
                "replyThreadId": "reply-thread:review",
                "teammateParticipantId": "participant:reviewer",
                "teammateDisplayName": "Reviewer",
                "stateReason": null,
                "changedFileCount": 2,
                "executedHere": true,
                "updatedAt": "2026-11-12T10:00:00.000Z"
            }],
            "total": 201,
            "truncated": true
        });

        let queue = serde_json::from_value::<ChatReviewQueueRead>(wire.clone()).unwrap();

        assert_eq!(queue.items[0].kind, ChatReviewQueueKind::Review);
        assert_eq!(serde_json::to_value(&queue).unwrap(), wire);
        let mut unknown_kind = wire;
        unknown_kind["items"][0]["kind"] = json!("ready_for_review");
        assert!(serde_json::from_value::<ChatReviewQueueRead>(unknown_kind).is_err());
    }

    #[test]
    fn review_queue_kinds_map_only_the_states_that_wait_for_the_person() {
        let kinds = [
            (ChatReviewQueueKind::Answer, "answer"),
            (ChatReviewQueueKind::Approval, "approval"),
            (ChatReviewQueueKind::Review, "review"),
            (ChatReviewQueueKind::Failure, "failure"),
        ];
        for (kind, wire) in kinds {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
            assert_eq!(
                serde_json::from_value::<ChatReviewQueueKind>(json!(wire)).unwrap(),
                kind
            );
        }
        assert_eq!(
            ChatReviewQueueKind::from_state(ChatWorkAssignmentState::WaitingForAnswer),
            Some(ChatReviewQueueKind::Answer)
        );
        assert_eq!(
            ChatReviewQueueKind::from_state(ChatWorkAssignmentState::WaitingForApproval),
            Some(ChatReviewQueueKind::Approval)
        );
        assert_eq!(
            ChatReviewQueueKind::from_state(ChatWorkAssignmentState::ReadyForReview),
            Some(ChatReviewQueueKind::Review)
        );
        assert_eq!(
            ChatReviewQueueKind::from_state(ChatWorkAssignmentState::Failed),
            Some(ChatReviewQueueKind::Failure)
        );
        for state in [
            ChatWorkAssignmentState::Queued,
            ChatWorkAssignmentState::Working,
            ChatWorkAssignmentState::Completed,
            ChatWorkAssignmentState::Cancelled,
        ] {
            assert_eq!(ChatReviewQueueKind::from_state(state), None);
        }
    }

    #[test]
    fn assignment_review_requests_round_trip_with_a_kind_tag() {
        let accept = ChatAssignmentReviewRequest::Accept;
        let request_changes = ChatAssignmentReviewRequest::RequestChanges {
            note: "Cover the empty case".to_owned(),
        };

        assert_eq!(
            serde_json::to_value(&accept).unwrap(),
            json!({ "kind": "accept" })
        );
        assert_eq!(
            serde_json::to_value(&request_changes).unwrap(),
            json!({ "kind": "requestChanges", "note": "Cover the empty case" })
        );
        for request in [accept, request_changes] {
            let wire = serde_json::to_value(&request).unwrap();
            assert_eq!(
                serde_json::from_value::<ChatAssignmentReviewRequest>(wire).unwrap(),
                request
            );
        }
        assert!(
            serde_json::from_value::<ChatAssignmentReviewRequest>(
                json!({ "kind": "requestChanges" })
            )
            .is_err(),
            "a change request carries its note"
        );
        assert!(
            serde_json::from_value::<ChatAssignmentReviewRequest>(json!({ "kind": "approve" }))
                .is_err()
        );
    }

    #[test]
    fn assignment_review_reads_use_wire_names() {
        let review = ChatAssignmentReviewRead {
            outcome: ChatAssignmentReviewOutcome::ChangesRequested,
            reviewed_at: UtcTimestamp::new("2026-11-11T10:00:00.000Z").unwrap(),
            reviewed_by_participant_id: ChatParticipantId::new("participant:local-owner").unwrap(),
        };

        let wire = serde_json::to_value(&review).unwrap();
        assert_eq!(
            wire,
            json!({
                "outcome": "changes_requested",
                "reviewedAt": "2026-11-11T10:00:00.000Z",
                "reviewedByParticipantId": "participant:local-owner",
            })
        );
        assert_eq!(
            serde_json::from_value::<ChatAssignmentReviewRead>(wire).unwrap(),
            review
        );
    }

    #[test]
    fn review_work_is_active_but_requires_a_new_follow_up_assignment() {
        let state = ChatWorkAssignmentState::ReadyForReview;

        assert!(state.is_active());
        assert!(!state.accepts_continuation());
        assert!(ChatWorkAssignmentState::Working.accepts_continuation());
    }

    #[test]
    fn managed_scratch_attachment_result_exposes_channel_not_folder_authority() {
        let destination = ChatScratchPromotionDestinationRead::ManagedAttachment {
            channel_id: ChatChannelId::new("channel:general").unwrap(),
            attachment_id: ChatAttachmentId::new("attachment:result").unwrap(),
        };

        assert_eq!(
            serde_json::to_value(destination).unwrap(),
            json!({
                "kind": "managedAttachment",
                "channelId": "channel:general",
                "attachmentId": "attachment:result"
            })
        );
    }

    #[test]
    fn tagged_organizational_contracts_use_camel_case_fields() {
        let history = serde_json::from_value::<ChatHistoryBoundary>(json!({
            "kind": "fromGrant",
            "lowerOrderKey": "c007"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(history).unwrap(),
            json!({ "kind": "fromGrant", "lowerOrderKey": "c007" })
        );

        let target = serde_json::from_value::<ChatExecutionTarget>(json!({
            "kind": "scratch",
            "scratchScopeId": "scratch-scope:test",
            "scratchGenerationId": "scratch-generation:test",
            "executionEnvironmentId": "environment:test"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(target).unwrap(),
            json!({
                "kind": "scratch",
                "scratchScopeId": "scratch-scope:test",
                "scratchGenerationId": "scratch-generation:test",
                "executionEnvironmentId": "environment:test"
            })
        );

        let reference = serde_json::from_value::<ChatMessageReference>(json!({
            "kind": "channel",
            "metadata": {
                "referenceId": "reference:test",
                "labelSnapshot": "#general",
                "startOffset": 0,
                "endOffset": 8,
                "plainTextProjection": "#general"
            },
            "channelId": "channel:general"
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(reference).unwrap(),
            json!({
                "kind": "channel",
                "metadata": {
                    "referenceId": "reference:test",
                    "labelSnapshot": "#general",
                    "startOffset": 0,
                    "endOffset": 8,
                    "plainTextProjection": "#general"
                },
                "channelId": "channel:general"
            })
        );
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatParticipantRead {
    pub id: ChatParticipantId,
    pub kind: ChatParticipantKind,
    pub display_name: String,
    pub avatar: VersionedJson,
    pub revision: u64,
    pub archived_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTeammatePolicyRead {
    pub id: ChatTeammatePolicyRevisionId,
    pub teammate_id: ChatParticipantId,
    pub revision: u64,
    pub provider_instance_id: ProviderInstanceId,
    pub safety_mode: ChatApprovalPolicy,
    pub provider_managed_model: bool,
    pub model_id: Option<ModelId>,
    pub model_options: Vec<ModelOptionSelection>,
    pub effort: Option<String>,
    pub speed: Option<String>,
    pub provider_options: VersionedJson,
    /// Token limit a new assignment of this teammate starts with when its post sets no budget.
    pub default_assignment_token_limit: Option<u64>,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAiTeammateRead {
    pub participant: ChatParticipantRead,
    pub role: String,
    pub instructions: String,
    pub configuration_state: ChatTeammateConfigurationState,
    pub latest_policy: Option<ChatTeammatePolicyRead>,
    pub channel_count: u64,
    pub active_assignment_count: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversationMembershipRead {
    pub conversation_id: ChatConversationId,
    pub participant: ChatParticipantRead,
    pub ai_access: Option<ChatChannelRosterAiSummary>,
    pub revision: u64,
    pub removed_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelRosterAiSummary {
    pub access_profile_id: ChatAccessProfileId,
    pub access_profile_revision: u64,
    pub access_profile_builtin_key: Option<ChatAccessProfileBuiltinKey>,
    pub access_profile_name: String,
    pub capabilities: ChatChannelCapabilities,
    pub history_boundary: ChatHistoryBoundary,
    pub runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub scratch_runtime_approval_override: Option<ChatRuntimeApprovalPolicy>,
    pub folder_grants: Vec<ChatFolderGrantRead>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelRosterRead {
    pub channel_id: ChatChannelId,
    pub conversation_id: ChatConversationId,
    pub audience_revision: u64,
    pub memberships: Vec<ChatConversationMembershipRead>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelMembershipRemovalPreview {
    pub teammate_id: ChatParticipantId,
    pub channel_id: ChatChannelId,
    pub active_assignment_count: u64,
    pub active_authorization_count: u64,
    pub will_revoke_active_work: bool,
    pub proposed_access: ChatTeammateAccessRead,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProjectPrimaryWorkingFolderRead {
    pub project_id: String,
    /// `None` when the project has no usable folder on this replica.
    pub working_folder_id: Option<ProjectWorkingFolderId>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReferenceMetadata {
    pub reference_id: ChatMessageReferenceId,
    pub label_snapshot: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub plain_text_projection: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatWorkspacePathKind {
    File,
    Folder,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatMessageReference {
    Participant {
        metadata: ChatReferenceMetadata,
        participant_id: ChatParticipantId,
        participant_kind: ChatParticipantKind,
    },
    Channel {
        metadata: ChatReferenceMetadata,
        channel_id: ChatChannelId,
    },
    WorkingFolder {
        metadata: ChatReferenceMetadata,
        working_folder_id: ProjectWorkingFolderId,
    },
    WorkspacePath {
        metadata: ChatReferenceMetadata,
        working_folder_id: ProjectWorkingFolderId,
        path_kind: ChatWorkspacePathKind,
        relative_path: String,
    },
    ExecutionEnvironment {
        metadata: ChatReferenceMetadata,
        execution_environment_id: ChatExecutionEnvironmentId,
    },
}

impl ChatMessageReference {
    /// Returns the immutable text metadata shared by every reference kind.
    pub fn metadata(&self) -> &ChatReferenceMetadata {
        match self {
            Self::Participant { metadata, .. }
            | Self::Channel { metadata, .. }
            | Self::WorkingFolder { metadata, .. }
            | Self::WorkspacePath { metadata, .. }
            | Self::ExecutionEnvironment { metadata, .. } => metadata,
        }
    }

    /// Returns mutable access to the common metadata for server-side copy rekeying.
    pub fn metadata_mut(&mut self) -> &mut ChatReferenceMetadata {
        match self {
            Self::Participant { metadata, .. }
            | Self::Channel { metadata, .. }
            | Self::WorkingFolder { metadata, .. }
            | Self::WorkspacePath { metadata, .. }
            | Self::ExecutionEnvironment { metadata, .. } => metadata,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReplyThreadSummaryRead {
    pub id: ChatReplyThreadId,
    pub reply_count: u64,
    pub last_activity_at: UtcTimestamp,
    pub participants: Vec<ChatParticipantRead>,
    pub unread: bool,
    /// Whether the thread's current work failed and the local person has not read the failure
    /// in the thread.
    pub unseen_failure: bool,
    pub work_state: Option<ChatWorkAssignmentState>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageRead {
    pub item_id: ChatConversationItemId,
    pub conversation_id: ChatConversationId,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub revision_id: ChatMessageRevisionId,
    pub revision: u64,
    /// Author identity, absent when the message carries only its author label snapshot.
    pub author: Option<ChatParticipantRead>,
    pub author_label_snapshot: String,
    pub normalized_markdown: String,
    pub rich_content: VersionedJson,
    pub attachment_ids: Vec<ChatAttachmentId>,
    pub references: Vec<ChatMessageReference>,
    pub reply_thread: Option<ChatReplyThreadSummaryRead>,
    /// Position of the item within its scope; items order by `(order_key, item_id)`.
    pub order_key: String,
    pub edited_at: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatScheduledMessageDispatchRead {
    pub processed_count: u32,
    pub dispatched_count: u32,
    pub dispatched_channel_ids: Vec<ChatChannelId>,
    pub next_dispatch_at: Option<UtcTimestamp>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChannelPageRead {
    pub channel_id: ChatChannelId,
    pub messages: Vec<ChatMessageRead>,
    pub previous_cursor: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWorkAssignmentRead {
    pub id: ChatWorkAssignmentId,
    pub reply_thread_id: ChatReplyThreadId,
    pub teammate: ChatParticipantRead,
    pub triggering_message_item_id: ChatConversationItemId,
    pub previous_assignment_id: Option<ChatWorkAssignmentId>,
    pub state: ChatWorkAssignmentState,
    pub state_reason: Option<String>,
    /// Where this work came from when another teammate delegated it; absent for work a person
    /// started.
    pub delegation: Option<ChatAssignmentDelegationRead>,
    pub revision: u64,
    pub settled_at: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    /// How a person ended the work, present only when it is completed after review.
    pub review: Option<ChatAssignmentReviewRead>,
    /// What the reading device would accept by starting this work, present when it waits for a
    /// local action there. Computed on read and never persisted.
    pub local_acceptance: Option<ChatAssignmentAcceptanceOfferRead>,
    /// Provider thread of the newest run of this reply thread that ran on the reading device, so
    /// the composer can show its usage; absent when no run of the thread executed here. Computed
    /// on read from device-local runs and never persisted.
    pub latest_run_provider_thread_id: Option<ChatThreadId>,
    /// Token and working time limits of this work with what it used, absent when it runs
    /// without a limit.
    pub budget: Option<ChatAssignmentBudgetRead>,
}

/// The budget of one assignment: its limits and what its executing device recorded as used.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentBudgetRead {
    pub assignment_id: ChatWorkAssignmentId,
    pub token_limit: Option<u64>,
    pub working_seconds_limit: Option<u64>,
    /// The person who set the limits; `None` when a teammate default or a delegating parent
    /// gave them.
    pub set_by_participant_id: Option<ChatParticipantId>,
    pub set_at: UtcTimestamp,
    /// Input, cache write, and output tokens, leaving out cache reads.
    pub used_counted_tokens: u64,
    /// Seconds runs spent working, leaving out time waiting for a person.
    pub used_working_seconds: u64,
}

/// One folder a waiting assignment would run in, with the access it would hold there.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentAcceptanceFolderRead {
    pub display_name: String,
    pub capability: ChatFolderCapability,
    pub runtime_approval: ChatRuntimeApprovalPolicy,
}

/// What a device accepts by starting a waiting assignment.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentAcceptanceOfferRead {
    /// Acceptance digest of this subject, returned with the Start so a change refuses it.
    pub digest: String,
    pub provider_instance_id: String,
    pub folders: Vec<ChatAssignmentAcceptanceFolderRead>,
}

/// Where delegated work came from and what it was asked to return.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentDelegationRead {
    pub parent_assignment_id: ChatWorkAssignmentId,
    /// Reply thread of the parent work, absent while the parent row has not reached this device.
    pub parent_reply_thread_id: Option<ChatReplyThreadId>,
    /// How many delegations separate this work from work a person started, from 1.
    pub depth: u8,
    pub objective: String,
    pub expected_result: String,
}

/// Work a teammate delegated from an assignment, with its live state.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDelegatedChildRead {
    pub assignment_id: ChatWorkAssignmentId,
    pub parent_assignment_id: ChatWorkAssignmentId,
    pub reply_thread_id: ChatReplyThreadId,
    pub teammate: ChatParticipantRead,
    pub objective: String,
    pub state: ChatWorkAssignmentState,
    pub state_reason: Option<String>,
    pub created_at: UtcTimestamp,
}

/// Work that waits for a local action on the reading device, wherever its reply thread is.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWaitingAssignmentRead {
    pub assignment_id: ChatWorkAssignmentId,
    pub project_id: String,
    pub channel_id: ChatChannelId,
    pub channel_name: String,
    pub reply_thread_id: ChatReplyThreadId,
    pub teammate_display_name: String,
    pub created_at: UtcTimestamp,
}

/// Work that waits for a local action on the reading device, with how much waits in all. The
/// list is capped, so `total` can exceed the number of items.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWaitingAssignmentsRead {
    pub items: Vec<ChatWaitingAssignmentRead>,
    pub total: u32,
}

/// What a review queue item asks of the person, from the assignment state that put it there.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatReviewQueueKind {
    /// The teammate waits for an answer to a question (`waiting_for_answer`).
    Answer,
    /// The teammate waits for an approval (`waiting_for_approval`).
    Approval,
    /// Finished work waits for review (`ready_for_review`).
    Review,
    /// The work failed recently and nothing continues it (`failed`).
    Failure,
}

impl ChatReviewQueueKind {
    /// The queue kind of an assignment in `state`, or `None` when that state needs nothing
    /// from the person.
    #[must_use]
    pub fn from_state(state: ChatWorkAssignmentState) -> Option<Self> {
        match state {
            ChatWorkAssignmentState::WaitingForAnswer => Some(Self::Answer),
            ChatWorkAssignmentState::WaitingForApproval => Some(Self::Approval),
            ChatWorkAssignmentState::ReadyForReview => Some(Self::Review),
            ChatWorkAssignmentState::Failed => Some(Self::Failure),
            _ => None,
        }
    }
}

/// One assignment that waits for the person, in any channel of any project.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewQueueItemRead {
    pub assignment_id: ChatWorkAssignmentId,
    /// The assignment revision the item was read at, for stale checks on accept.
    pub revision: u64,
    pub kind: ChatReviewQueueKind,
    pub project_id: String,
    pub project_name: String,
    pub channel_id: ChatChannelId,
    pub channel_name: String,
    pub reply_thread_id: ChatReplyThreadId,
    pub teammate_participant_id: ChatParticipantId,
    pub teammate_display_name: String,
    /// The assignment state reason, clipped for display.
    pub state_reason: Option<String>,
    /// Distinct files the assignment's review updates report as changed.
    pub changed_file_count: u32,
    /// Whether this installation executed the assignment.
    pub executed_here: bool,
    pub updated_at: UtcTimestamp,
}

/// The review queue: the first items by priority and recency, with how many match in all.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewQueueRead {
    pub items: Vec<ChatReviewQueueItemRead>,
    pub total: u32,
    /// How many of `total` are failures. Notifications announce them, while the sidebar leaves
    /// them to the channel and thread markers.
    pub failure_count: u32,
    /// Whether `total` exceeds the items read.
    pub truncated: bool,
}

/// A reply routed to an assignment after the message that started it, with where its delivery
/// stands. The preview is the start of the message's current revision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentInputRead {
    pub input_id: ChatAssignmentInputId,
    pub message_item_id: ChatConversationItemId,
    pub ordinal: u64,
    /// `steer`, `queued_continuation`, or `follow_up`.
    pub routing_kind: String,
    /// `pending`, `delivered`, `failed`, or `ignored`.
    pub delivery_state: String,
    /// Whether a person holds the pending input back from delivery until they resume it.
    pub held: bool,
    pub delivered_at: Option<UtcTimestamp>,
    /// The message revision the run received, present once the input is delivered.
    pub delivered_revision_id: Option<ChatMessageRevisionId>,
    pub current_revision_id: Option<ChatMessageRevisionId>,
    pub preview: String,
    /// Whether the message continues past the preview.
    pub preview_truncated: bool,
    pub author_participant_id: Option<ChatParticipantId>,
}

/// New text for a pending assignment input, which replaces its message's current revision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAssignmentInputEditRequest {
    pub normalized_markdown: String,
    pub rich_content: VersionedJson,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAgentRunRead {
    pub id: ChatAgentRunId,
    pub assignment_id: ChatWorkAssignmentId,
    pub project_id: String,
    pub working_folder_id: Option<ProjectWorkingFolderId>,
    pub execution_environment_id: ChatExecutionEnvironmentId,
    pub scratch_generation_id: Option<ChatScratchGenerationId>,
    pub teammate_policy_revision_id: ChatTeammatePolicyRevisionId,
    pub effort: Option<String>,
    pub provider_execution_turn_id: ChatTurnId,
    pub provider_execution_thread_id: Option<ChatThreadId>,
    pub state: String,
    pub run_ordinal: u64,
    /// Why this run exists: `fresh`, `retry`, or `handoff`.
    pub attempt_reason: String,
    /// Why a handoff run replaced its native session; set exactly when the run is a handoff.
    pub handoff_reason: Option<String>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReplyThreadPageRead {
    pub thread: ChatReplyThreadSummaryRead,
    pub root_message: ChatMessageRead,
    pub replies: Vec<ChatMessageRead>,
    pub assignment: Option<ChatWorkAssignmentRead>,
    pub agent_runs: Vec<ChatAgentRunRead>,
    /// Work delegated from this reply thread's assignments, oldest first, with its live state.
    pub delegated_children: Vec<ChatDelegatedChildRead>,
    pub previous_cursor: Option<String>,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageSearchResultRead {
    pub project_id: String,
    pub channel_id: ChatChannelId,
    pub channel_name: String,
    pub conversation_id: ChatConversationId,
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub message_item_id: ChatConversationItemId,
    /// Page cursor of the page that ends with this message.
    pub anchor_cursor: String,
    /// Author identity, absent when the message carries only its author label snapshot.
    pub author_participant_id: Option<ChatParticipantId>,
    /// Author kind, absent together with the author identity.
    pub author_kind: Option<ChatParticipantKind>,
    /// Author display name, or the author label snapshot when the identity is absent.
    pub author_display_name: String,
    pub excerpt: String,
    pub created_at: UtcTimestamp,
}

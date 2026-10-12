//! Stable wire contracts for Chat review snapshots and actions.

use super::super::models::{ChatThreadId, ChatTurnId, ProjectWorkingFolderId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewWorkingTreeMode {
    Staged,
    Unstaged,
    All,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCheckpointRange {
    Turn,
    Thread,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewBranchComparison {
    MergeBase,
    Direct,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum ReviewDiffSource {
    WorkingTree {
        mode: ReviewWorkingTreeMode,
    },
    Checkpoint {
        range: ReviewCheckpointRange,
        turn_id: Option<ChatTurnId>,
    },
    Commit {
        revision: String,
    },
    Branch {
        base_ref: Option<String>,
        head_ref: String,
        comparison: ReviewBranchComparison,
    },
    ChangeRequest {
        provider: String,
        repository_slug: String,
        number: u64,
    },
    ProviderTurn {
        turn_id: ChatTurnId,
    },
}

impl ReviewDiffSource {
    pub fn kind_wire(&self) -> &'static str {
        match self {
            Self::WorkingTree { .. } => "working_tree",
            Self::Checkpoint { .. } => "checkpoint",
            Self::Commit { .. } => "commit",
            Self::Branch { .. } => "branch",
            Self::ChangeRequest { .. } => "change_request",
            Self::ProviderTurn { .. } => "provider_turn",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenChatReviewRequest {
    pub thread_id: Option<ChatThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub source: ReviewDiffSource,
    #[serde(default)]
    pub ignore_whitespace: bool,
    #[serde(default = "default_context_lines")]
    pub context_lines: u32,
    pub preferred_relative_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadChatReviewPatchesRequest {
    pub thread_id: Option<ChatThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub snapshot_id: String,
    pub review_revision: String,
    pub file_ids: Vec<String>,
    pub continuation_cursor: Option<String>,
    pub byte_limit: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Stage,
    Unstage,
    Discard,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyChatReviewActionRequest {
    pub thread_id: Option<ChatThreadId>,
    pub snapshot_id: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub expected_review_revision: String,
    pub file_id: Option<String>,
    #[serde(default)]
    pub hunk_ids: Vec<String>,
    pub operation: ReviewAction,
    #[serde(default)]
    pub confirmed: bool,
    pub client_operation_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFileFlagsRead {
    pub binary: bool,
    pub submodule: bool,
    pub conflict: bool,
    pub mode_only: bool,
    pub pure_rename: bool,
    pub untracked: bool,
    pub symlink: bool,
    pub provider_reported: bool,
    pub git_observed: bool,
    pub read_only: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFileCapabilitiesRead {
    pub stage: bool,
    pub unstage: bool,
    pub discard: bool,
    pub comment: bool,
    pub open_editor: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFileCapabilityReasonsRead {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unstage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discard: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_editor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFileRead {
    pub file_id: String,
    pub relative_path: String,
    pub previous_relative_path: Option<String>,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub flags: ReviewFileFlagsRead,
    pub capabilities: ReviewFileCapabilitiesRead,
    pub capability_reasons: ReviewFileCapabilityReasonsRead,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewTotalsRead {
    pub files: u64,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFreshness {
    Current,
    Outdated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewOpenRead {
    pub snapshot_id: String,
    pub review_revision: String,
    pub source: ReviewDiffSource,
    pub source_label: String,
    pub files: Vec<ReviewFileRead>,
    pub totals: ReviewTotalsRead,
    pub preferred_patch: Option<ReviewFilePatchRead>,
    pub freshness: ReviewFreshness,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewPatchState {
    Complete,
    Partial,
    Binary,
    OversizedHunk,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewHunkRead {
    pub hunk_id: String,
    pub old_start: u64,
    pub old_count: u64,
    pub new_start: u64,
    pub new_count: u64,
    pub state: ReviewPatchState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFilePatchRead {
    pub file_id: String,
    pub patch: Option<String>,
    pub hunks: Vec<ReviewHunkRead>,
    pub continuation_cursor: Option<String>,
    pub state: ReviewPatchState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewPatchesRead {
    pub patches: Vec<ReviewFilePatchRead>,
    pub continuation_cursor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatReviewActionResultRead {
    pub snapshot: ChatReviewOpenRead,
}

fn default_context_lines() -> u32 {
    3
}

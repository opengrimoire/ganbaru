//! Git checkpoint capture, persistence, restoration, and bounded diff facade.

use super::events::ChangedFileSummary;
use super::models::{ChatCheckpointId, ChatError, ChatErrorCode, ChatThreadId};

mod diff;
mod git;
mod repository;
mod service;
#[cfg(test)]
mod tests;

pub use diff::ChatChangedFileRead;
pub use diff::diff_files;
pub use git::delete_exact_ref;
pub use git::{current_git_snapshot, restore_git_snapshot, verify_checkpoint};
pub use repository::read_stored_checkpoint;
pub use service::capture_and_store;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckpointKind {
    Initial,
    PreTurn,
    PostTurn,
    Recovery,
}

impl CheckpointKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::PreTurn => "pre_turn",
            Self::PostTurn => "post_turn",
            Self::Recovery => "recovery",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CapturedCheckpoint {
    pub id: ChatCheckpointId,
    pub hidden_ref_name: String,
    pub git_object_id: String,
    pub index_commit_oid: String,
    pub index_tree_oid: String,
    pub worktree_tree_oid: String,
    pub head_oid: Option<String>,
    pub head_ref: Option<String>,
    pub index_fingerprint: String,
    pub changed_files: Vec<ChangedFileSummary>,
}

#[derive(Clone, Debug)]
pub struct StoredCheckpoint {
    pub id: ChatCheckpointId,
    pub thread_id: ChatThreadId,
    pub turn_count: u64,
    pub repository_identity: String,
    pub hidden_ref_name: String,
    pub git_object_id: String,
    pub index_tree_oid: String,
    pub worktree_tree_oid: String,
    pub head_oid: Option<String>,
    pub head_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentGitSnapshot {
    pub worktree_commit_oid: String,
    pub worktree_tree_oid: String,
    pub index_tree_oid: String,
    pub index_fingerprint: String,
    pub head_oid: Option<String>,
    pub head_ref: Option<String>,
}

/// Immutable Git trees used to compare the current index and worktree without
/// creating the synthetic commits required by restore previews.
pub struct CurrentGitTrees {
    pub head_oid: Option<String>,
    pub index_tree_oid: String,
    pub worktree_tree_oid: String,
}

pub(super) fn checkpoint_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Git checkpoint operation failed safely",
        true,
    )
}

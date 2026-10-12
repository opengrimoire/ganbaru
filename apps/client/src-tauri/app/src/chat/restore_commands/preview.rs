use super::*;

pub(super) struct StoredRestorePreview {
    pub(super) id: String,
    pub(super) thread_id: ChatThreadId,
    pub(super) checkpoint_id: ChatCheckpointId,
    pub(super) expected_thread_revision: u64,
    pub(super) repository_identity: String,
    pub(super) current: CurrentGitSnapshot,
    pub(super) expires_at: UtcTimestamp,
}

pub(super) fn transient_checkpoint(
    target: &StoredCheckpoint,
    current: &CurrentGitSnapshot,
) -> StoredCheckpoint {
    StoredCheckpoint {
        id: target.id.clone(),
        thread_id: target.thread_id.clone(),
        turn_count: target.turn_count,
        repository_identity: target.repository_identity.clone(),
        hidden_ref_name: target.hidden_ref_name.clone(),
        git_object_id: current.worktree_commit_oid.clone(),
        index_tree_oid: current.index_tree_oid.clone(),
        worktree_tree_oid: current.worktree_tree_oid.clone(),
        head_oid: current.head_oid.clone(),
        head_ref: current.head_ref.clone(),
    }
}

pub(super) fn require_head_context(
    current: &CurrentGitSnapshot,
    target: &StoredCheckpoint,
) -> ChatResult<()> {
    if current.head_oid != target.head_oid || current.head_ref != target.head_ref {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Current Git HEAD context differs from the checkpoint",
            true,
        ));
    }
    Ok(())
}

pub(super) fn preview_id(
    thread_id: &ChatThreadId,
    checkpoint_id: &ChatCheckpointId,
    current: &CurrentGitSnapshot,
    now: &UtcTimestamp,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_str());
    hasher.update(checkpoint_id.as_str());
    hasher.update(&current.worktree_tree_oid);
    hasher.update(&current.index_tree_oid);
    hasher.update(now.as_str());
    format!("restore-preview:{:x}", hasher.finalize())
}

pub(super) fn add_duration(
    timestamp: &UtcTimestamp,
    duration: chrono::Duration,
) -> ChatResult<UtcTimestamp> {
    let parsed = chrono::DateTime::parse_from_rfc3339(timestamp.as_str())
        .map_err(|_| corrupt_data_error())?
        .with_timezone(&Utc)
        + duration;
    UtcTimestamp::new(parsed.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| corrupt_data_error())
}

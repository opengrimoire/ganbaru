//! Checkpoint capture orchestration across Git and SQLite.

use super::git::{capture_git, delete_exact_ref};
use super::repository::{
    StoreCheckpointRequest, available_checkpoint_exists, insert_checkpoint_and_associate_turn,
    latest_checkpoint_oid,
};
use super::{CapturedCheckpoint, CheckpointKind};
use crate::models::{
    ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId, UtcTimestamp,
};
use crate::workspace::AuthorizedWorkingFolder;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub async fn capture_and_store(
    pool: &SqlitePool,
    authorized: &AuthorizedWorkingFolder,
    thread_id: &ChatThreadId,
    turn_id: Option<&ChatTurnId>,
    turn_count: u64,
    kind: CheckpointKind,
    now: &UtcTimestamp,
) -> ChatResult<CapturedCheckpoint> {
    let repository_identity = authorized.repository_identity.as_deref().ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::CapabilityUnsupported,
            "Git checkpoints are unavailable for this workspace",
            true,
        )
    })?;
    if available_checkpoint_exists(pool, thread_id, turn_count).await? {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "This Chat checkpoint already exists",
            true,
        ));
    }
    let checkpoint_id = checkpoint_id(thread_id, turn_id, turn_count, now)?;
    let previous_oid = latest_checkpoint_oid(pool, thread_id).await?;
    let root = authorized.canonical_path.clone();
    let thread_for_git = thread_id.clone();
    let checkpoint_for_git = checkpoint_id.clone();
    let captured = tokio::task::spawn_blocking(move || {
        capture_git(
            &root,
            &thread_for_git,
            &checkpoint_for_git,
            previous_oid.as_deref(),
        )
    })
    .await
    .map_err(|_| super::checkpoint_error("Git checkpoint worker stopped"))??;
    let persistence = insert_checkpoint_and_associate_turn(
        pool,
        StoreCheckpointRequest {
            captured: &captured,
            thread_id,
            turn_id,
            turn_count,
            repository_identity,
            kind,
            now,
        },
    )
    .await;
    if let Err(error) = persistence {
        let _ = delete_exact_ref(
            &authorized.canonical_path,
            &captured.hidden_ref_name,
            &captured.git_object_id,
        );
        return Err(error);
    }
    Ok(captured)
}

fn checkpoint_id(
    thread_id: &ChatThreadId,
    turn_id: Option<&ChatTurnId>,
    turn_count: u64,
    now: &UtcTimestamp,
) -> ChatResult<ChatCheckpointId> {
    let mut hasher = Sha256::new();
    hasher.update(thread_id.as_str());
    hasher.update([0]);
    hasher.update(turn_id.map(ChatTurnId::as_str).unwrap_or("initial"));
    hasher.update([0]);
    hasher.update(turn_count.to_le_bytes());
    hasher.update(now.as_str());
    ChatCheckpointId::new(format!("checkpoint:{:x}", hasher.finalize()))
        .map_err(super::checkpoint_error)
}

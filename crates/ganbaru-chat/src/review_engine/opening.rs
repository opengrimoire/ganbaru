//! Review snapshot opening and source-specific material resolution.

use super::super::checkpoints::{StoredCheckpoint, read_stored_checkpoint, verify_checkpoint};
use super::super::events::{CanonicalEvent, ChangedFileSummary};
use super::super::models::{
    ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, ChatTurnId,
};
use super::super::workspace::{AuthorizedWorkingFolder, WorkingFolderAuthorizationOperation};
use super::ReviewWorkspaceAuthorizer;
use super::authorization::authorize_review_request;
use super::contracts::*;
use super::material::{enrich_working_tree_files, git_files, working_tree_material};
use super::registry::{
    ChatReviewRegistry, ReviewFileInternal, ReviewSnapshot, file_id, review_revision, snapshot_id,
    snapshot_read,
};
use super::validation::{thread_required_error, validate_path, validate_reference};
use super::{corrupt_data_error, empty_tree, git_text, persistence_error, review_error};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum MissingReviewSource {
    CheckpointPair,
    ProviderTurn,
}

fn missing_review_source_error(reason: MissingReviewSource, message: &str) -> ChatError {
    let mut error = ChatError::new(ChatErrorCode::NotFound, message, true);
    error.details = Some(Box::new(serde_json::json!({ "reason": reason })));
    error
}

pub async fn open_review(
    authorizer: &dyn ReviewWorkspaceAuthorizer,
    state: &ChatReviewRegistry,
    pool: &SqlitePool,
    database_identity: &str,
    request: OpenChatReviewRequest,
) -> ChatResult<ChatReviewOpenRead> {
    let (working_folder_id, environment_id, authorized) = authorize_review_request(
        authorizer,
        pool,
        &request,
        WorkingFolderAuthorizationOperation::Diff,
    )
    .await?;
    let (before_oid, after_oid, label, files, provider_patches, object_store) =
        match &request.source {
            ReviewDiffSource::WorkingTree { mode } => {
                let material = working_tree_material(&authorized.canonical_path, *mode).await?;
                let mut files = git_files(
                    &authorized.canonical_path,
                    &material.before_oid,
                    &material.after_oid,
                    &request.source,
                    request.ignore_whitespace,
                    material.object_store.as_deref(),
                )
                .await?;
                enrich_working_tree_files(&mut files, &material.status, &request.source);
                (
                    Some(material.before_oid),
                    Some(material.after_oid),
                    material.label,
                    files,
                    HashMap::new(),
                    material.object_store,
                )
            }
            ReviewDiffSource::Checkpoint { range, turn_id } => {
                let thread_id = request
                    .thread_id
                    .as_ref()
                    .ok_or_else(thread_required_error)?;
                let (pre, post) = checkpoint_pair(pool, thread_id, *range, turn_id.as_ref())
                    .await?
                    .ok_or_else(|| {
                        missing_review_source_error(
                            MissingReviewSource::CheckpointPair,
                            "A settled checkpoint pair is not available for this review",
                        )
                    })?;
                let pre = read_stored_checkpoint(pool, &pre).await?;
                let post = read_stored_checkpoint(pool, &post).await?;
                verify_checkpoint_pair(&authorized, thread_id, &pre, &post).await?;
                let files = git_files(
                    &authorized.canonical_path,
                    &pre.git_object_id,
                    &post.git_object_id,
                    &request.source,
                    request.ignore_whitespace,
                    None,
                )
                .await?;
                (
                    Some(pre.git_object_id),
                    Some(post.git_object_id),
                    match range {
                        ReviewCheckpointRange::Turn => "Agent turn",
                        ReviewCheckpointRange::Thread => "Entire chat",
                    }
                    .to_string(),
                    files,
                    HashMap::new(),
                    None,
                )
            }
            ReviewDiffSource::Commit { revision } => {
                validate_reference(revision, "revision")?;
                let commit = git_text(
                    &authorized.canonical_path,
                    &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
                    None,
                )
                .await?;
                let parents = git_text(
                    &authorized.canonical_path,
                    &["rev-list", "--parents", "-n", "1", &commit],
                    None,
                )
                .await?;
                let parent = parents
                    .split_whitespace()
                    .nth(1)
                    .map(ToOwned::to_owned)
                    .unwrap_or(empty_tree(&authorized.canonical_path).await?);
                let files = git_files(
                    &authorized.canonical_path,
                    &parent,
                    &commit,
                    &request.source,
                    request.ignore_whitespace,
                    None,
                )
                .await?;
                (
                    Some(parent),
                    Some(commit),
                    format!("Commit {revision}"),
                    files,
                    HashMap::new(),
                    None,
                )
            }
            ReviewDiffSource::Branch {
                base_ref,
                head_ref,
                comparison,
            } => {
                let base_ref = base_ref.as_deref().unwrap_or("HEAD");
                validate_reference(base_ref, "baseRef")?;
                validate_reference(head_ref, "headRef")?;
                let base = git_text(
                    &authorized.canonical_path,
                    &["rev-parse", "--verify", &format!("{base_ref}^{{commit}}")],
                    None,
                )
                .await?;
                let head = git_text(
                    &authorized.canonical_path,
                    &["rev-parse", "--verify", &format!("{head_ref}^{{commit}}")],
                    None,
                )
                .await?;
                let before = if *comparison == ReviewBranchComparison::MergeBase {
                    git_text(
                        &authorized.canonical_path,
                        &["merge-base", &base, &head],
                        None,
                    )
                    .await?
                } else {
                    base
                };
                let files = git_files(
                    &authorized.canonical_path,
                    &before,
                    &head,
                    &request.source,
                    request.ignore_whitespace,
                    None,
                )
                .await?;
                (
                    Some(before),
                    Some(head),
                    format!("{base_ref} to {head_ref}"),
                    files,
                    HashMap::new(),
                    None,
                )
            }
            ReviewDiffSource::ProviderTurn { turn_id } => {
                let thread_id = request
                    .thread_id
                    .as_ref()
                    .ok_or_else(thread_required_error)?;
                let (changed, patch) = provider_turn_patch(pool, thread_id, turn_id).await?;
                let mut provider_patches = HashMap::new();
                let mut files = Vec::new();
                for mut summary in changed {
                    if validate_path(&summary.relative_path).is_err() {
                        continue;
                    }
                    if summary
                        .previous_relative_path
                        .as_deref()
                        .is_some_and(|previous| validate_path(previous).is_err())
                    {
                        summary.previous_relative_path = None;
                    }
                    if let Some(file_patch) = extract_provider_file_patch(
                        &patch,
                        &summary.relative_path,
                        summary.previous_relative_path.as_deref(),
                    ) {
                        provider_patches.insert(summary.relative_path.clone(), file_patch);
                    }
                    files.push(file_from_summary(String::new(), summary, &request.source));
                }
                if files.is_empty() {
                    return Err(missing_review_source_error(
                        MissingReviewSource::ProviderTurn,
                        "The provider did not report usable workspace-relative paths for this turn",
                    ));
                }
                (
                    None,
                    None,
                    "Provider-reported turn".to_string(),
                    files,
                    provider_patches,
                    None,
                )
            }
            ReviewDiffSource::ChangeRequest { .. } => {
                return Err(ChatError::unsupported(
                    "Hosted change-request review is unavailable for this source-control adapter",
                ));
            }
        };
    let revision = if let (Some(before), Some(after)) = (&before_oid, &after_oid) {
        review_revision(
            request.thread_id.as_ref(),
            &environment_id,
            &request.source,
            before,
            after,
            request.ignore_whitespace,
            request.context_lines,
        )?
    } else {
        let mut hasher = Sha256::new();
        hasher.update(
            request
                .thread_id
                .as_ref()
                .map(ChatThreadId::as_str)
                .unwrap_or("workspace-draft"),
        );
        hasher.update([0]);
        hasher.update(environment_id.as_bytes());
        hasher.update([0]);
        hasher.update(serde_json::to_vec(&request.source).map_err(|_| corrupt_data_error())?);
        let mut patches = provider_patches.iter().collect::<Vec<_>>();
        patches.sort_by(|left, right| left.0.cmp(right.0));
        for (path, patch) in patches {
            hasher.update(path);
            hasher.update([0]);
            hasher.update(patch);
        }
        format!("{:x}", hasher.finalize())
    };
    let mut files = files
        .into_iter()
        .map(|mut file| {
            file.read.file_id = file_id(
                &revision,
                &file.read.relative_path,
                file.read.previous_relative_path.as_deref(),
            );
            ReviewFileInternal { read: file.read }
        })
        .collect::<Vec<_>>();
    reconcile_review_comment_applicability(
        pool,
        request.thread_id.as_ref(),
        &request.source,
        &revision,
    )
    .await?;
    let provider_patches = if provider_patches.is_empty() {
        provider_patches
    } else {
        files
            .iter()
            .filter_map(|file| {
                provider_patches
                    .get(&file.read.relative_path)
                    .cloned()
                    .map(|patch| (file.read.file_id.clone(), patch))
            })
            .collect()
    };
    if matches!(&request.source, ReviewDiffSource::ProviderTurn { .. }) {
        for file in &mut files {
            if !provider_patches.contains_key(&file.read.file_id) {
                file.read.capabilities.comment = false;
                file.read.capability_reasons.comment =
                    Some("The provider did not include patch content for this file".to_string());
            }
        }
    }
    let snapshot_id = snapshot_id(
        database_identity,
        request.thread_id.as_ref(),
        &environment_id,
        &revision,
    );
    let snapshot = Arc::new(ReviewSnapshot {
        database_identity: database_identity.to_string(),
        snapshot_id,
        review_revision: revision,
        thread_id: request.thread_id,
        working_folder_id,
        environment_id,
        root: authorized.canonical_path,
        source: request.source,
        source_label: label,
        before_oid,
        after_oid,
        context_lines: request.context_lines,
        ignore_whitespace: request.ignore_whitespace,
        files,
        provider_patches,
        patch_cache: tokio::sync::Mutex::new(HashMap::new()),
        object_store,
        created_at: Instant::now(),
    });
    state.insert(snapshot.clone())?;
    snapshot_read(&snapshot, request.preferred_relative_path.as_deref()).await
}

async fn reconcile_review_comment_applicability(
    pool: &SqlitePool,
    thread_id: Option<&ChatThreadId>,
    source: &ReviewDiffSource,
    review_revision: &str,
) -> ChatResult<()> {
    let Some(thread_id) = thread_id else {
        return Ok(());
    };
    let source_data = serde_json::to_string(source).map_err(|_| corrupt_data_error())?;
    sqlx::query(
        "UPDATE chat_review_comments
         SET applicability = CASE
               WHEN review_revision = ? THEN 'current'
               ELSE 'outdated'
             END
         WHERE thread_id = ? AND source_data = ? AND state IN ('open', 'resolved')",
    )
    .bind(review_revision)
    .bind(thread_id.as_str())
    .bind(source_data)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn checkpoint_pair(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    range: ReviewCheckpointRange,
    turn_id: Option<&ChatTurnId>,
) -> ChatResult<Option<(ChatCheckpointId, ChatCheckpointId)>> {
    let row = match range {
        ReviewCheckpointRange::Turn => match turn_id {
            Some(turn_id) => {
                sqlx::query(
                    "SELECT pre_checkpoint_id, post_checkpoint_id FROM chat_turns
                 WHERE id = ? AND thread_id = ? AND invalidated_at IS NULL",
                )
                .bind(turn_id.as_str())
                .bind(thread_id.as_str())
                .fetch_optional(pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT pre_checkpoint_id, post_checkpoint_id FROM chat_turns
                 WHERE thread_id = ? AND invalidated_at IS NULL
                 ORDER BY ordinal DESC LIMIT 1",
                )
                .bind(thread_id.as_str())
                .fetch_optional(pool)
                .await
            }
        },
        ReviewCheckpointRange::Thread => {
            sqlx::query(
                "SELECT
               (SELECT pre_checkpoint_id FROM chat_turns
                WHERE thread_id = ? AND invalidated_at IS NULL
                ORDER BY ordinal ASC LIMIT 1) AS pre_checkpoint_id,
               (SELECT post_checkpoint_id FROM chat_turns
                WHERE thread_id = ? AND invalidated_at IS NULL
                ORDER BY ordinal DESC LIMIT 1) AS post_checkpoint_id",
            )
            .bind(thread_id.as_str())
            .bind(thread_id.as_str())
            .fetch_optional(pool)
            .await
        }
    }
    .map_err(persistence_error)?;
    let Some(row) = row else { return Ok(None) };
    let pre: Option<String> = row
        .try_get("pre_checkpoint_id")
        .map_err(persistence_error)?;
    let post: Option<String> = row
        .try_get("post_checkpoint_id")
        .map_err(persistence_error)?;
    match (pre, post) {
        (Some(pre), Some(post)) => Ok(Some((
            ChatCheckpointId::new(pre).map_err(|_| corrupt_data_error())?,
            ChatCheckpointId::new(post).map_err(|_| corrupt_data_error())?,
        ))),
        _ => Ok(None),
    }
}

async fn verify_checkpoint_pair(
    authorized: &AuthorizedWorkingFolder,
    thread_id: &ChatThreadId,
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
) -> ChatResult<()> {
    if &pre.thread_id != thread_id
        || &post.thread_id != thread_id
        || pre.repository_identity != post.repository_identity
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Checkpoint pair belongs to another Chat thread",
            false,
        ));
    }
    let authorized = authorized.clone();
    let pre = pre.clone();
    let post = post.clone();
    tokio::task::spawn_blocking(move || {
        verify_checkpoint(&authorized, &pre)?;
        verify_checkpoint(&authorized, &post)
    })
    .await
    .map_err(|_| review_error("Git checkpoint verification worker stopped"))?
}

pub async fn provider_turn_patch(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
) -> ChatResult<(Vec<ChangedFileSummary>, String)> {
    let payload: String = sqlx::query_scalar(
        "SELECT payload_data FROM chat_events
         WHERE thread_id = ? AND turn_id = ? AND event_type = 'diff_updated'
           AND json_extract(payload_data, '$.payload.providerDiff') IS NOT NULL
           AND instr(json_extract(payload_data, '$.payload.providerDiff'), 'diff --git ') > 0
         ORDER BY sequence DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        missing_review_source_error(
            MissingReviewSource::ProviderTurn,
            "The provider did not report a patch for this turn",
        )
    })?;
    match serde_json::from_str::<CanonicalEvent>(&payload).map_err(|_| corrupt_data_error())? {
        CanonicalEvent::DiffUpdated(event) => event
            .provider_diff
            .filter(|patch| !patch.is_empty())
            .map(|patch| (event.files, patch))
            .ok_or_else(|| {
                missing_review_source_error(
                    MissingReviewSource::ProviderTurn,
                    "The provider did not report patch content for this turn",
                )
            }),
        _ => Err(corrupt_data_error()),
    }
}

pub fn file_contract(
    source: &ReviewDiffSource,
    status: &str,
    additions: Option<u64>,
    deletions: Option<u64>,
    binary: bool,
) -> (
    ReviewFileFlagsRead,
    ReviewFileCapabilitiesRead,
    ReviewFileCapabilityReasonsRead,
) {
    let read_only = !matches!(source, ReviewDiffSource::WorkingTree { .. });
    let conflict = status == "conflicted";
    let pure_rename = status == "renamed" && additions == Some(0) && deletions == Some(0);
    let mode_only = status == "type_changed";
    let untracked = matches!(
        source,
        ReviewDiffSource::WorkingTree {
            mode: ReviewWorkingTreeMode::Unstaged
        }
    ) && status == "added";
    let mut capabilities = ReviewFileCapabilitiesRead {
        stage: false,
        unstage: false,
        discard: false,
        comment: !binary,
        open_editor: status != "deleted",
    };
    let mut reasons = ReviewFileCapabilityReasonsRead::default();
    if conflict {
        reasons.stage = Some("Resolve this conflict in the editor before staging".to_string());
        reasons.unstage = Some("Conflicted entries cannot be changed from Review".to_string());
        reasons.discard = Some("Conflicted entries cannot be discarded from Review".to_string());
    } else {
        match source {
            ReviewDiffSource::WorkingTree {
                mode: ReviewWorkingTreeMode::Staged,
            } => {
                capabilities.unstage = true;
                reasons.stage = Some("Already staged".to_string());
                reasons.discard = Some("Unstage this change before discarding it".to_string());
            }
            ReviewDiffSource::WorkingTree {
                mode: ReviewWorkingTreeMode::Unstaged,
            } => {
                capabilities.stage = true;
                capabilities.discard = true;
                reasons.unstage = Some("Not staged".to_string());
            }
            ReviewDiffSource::WorkingTree {
                mode: ReviewWorkingTreeMode::All,
            } => {
                capabilities.stage = true;
                reasons.unstage = Some("Open Staged changes to unstage".to_string());
                reasons.discard = Some("Open Unstaged changes to discard".to_string());
            }
            _ => {
                let reason = "This review source is read-only".to_string();
                reasons.stage = Some(reason.clone());
                reasons.unstage = Some(reason.clone());
                reasons.discard = Some(reason);
            }
        }
    }
    if binary {
        capabilities.comment = false;
        reasons.comment = Some("Binary files do not support line comments".to_string());
    }
    if !capabilities.open_editor {
        reasons.open_editor = Some("Deleted files cannot be opened in the editor".to_string());
    }
    (
        ReviewFileFlagsRead {
            binary,
            submodule: false,
            conflict,
            mode_only,
            pure_rename,
            untracked,
            symlink: false,
            provider_reported: read_only && matches!(source, ReviewDiffSource::ProviderTurn { .. }),
            git_observed: !matches!(source, ReviewDiffSource::ProviderTurn { .. }),
            read_only,
        },
        capabilities,
        reasons,
    )
}

fn file_from_summary(
    file_id: String,
    summary: ChangedFileSummary,
    source: &ReviewDiffSource,
) -> ReviewFileInternal {
    let normalized_status = normalize_file_status(&summary.status);
    let (mut flags, capabilities, capability_reasons) = file_contract(
        source,
        normalized_status,
        summary.additions,
        summary.deletions,
        summary.binary,
    );
    flags.provider_reported = true;
    flags.git_observed = false;
    flags.conflict = summary.status == "conflicted";
    ReviewFileInternal {
        read: ReviewFileRead {
            file_id,
            relative_path: summary.relative_path,
            previous_relative_path: summary.previous_relative_path,
            status: normalized_status.to_string(),
            additions: summary.additions,
            deletions: summary.deletions,
            flags,
            capabilities,
            capability_reasons,
        },
    }
}

fn normalize_file_status(status: &str) -> &str {
    match status {
        "added" | "modified" | "deleted" | "renamed" | "type_changed" => status,
        _ => "unknown",
    }
}

fn extract_provider_file_patch(
    patch: &str,
    relative_path: &str,
    previous_relative_path: Option<&str>,
) -> Option<String> {
    let expected = format!(
        "diff --git a/{} b/{relative_path}",
        previous_relative_path.unwrap_or(relative_path)
    );
    let start = patch.match_indices("diff --git ").find_map(|(index, _)| {
        patch[index..]
            .lines()
            .next()
            .is_some_and(|header| header == expected)
            .then_some(index)
    })?;
    let remainder = &patch[start..];
    let end = remainder[1..]
        .find("\ndiff --git ")
        .map(|index| index + 2)
        .unwrap_or(remainder.len());
    Some(remainder[..end].to_string())
}

#[cfg(test)]
mod source_error_tests {
    use super::*;

    #[test]
    fn missing_sources_keep_stable_recovery_reasons_when_messages_change() {
        for (reason, expected) in [
            (MissingReviewSource::CheckpointPair, "checkpoint_pair"),
            (MissingReviewSource::ProviderTurn, "provider_turn"),
        ] {
            let value =
                serde_json::to_value(missing_review_source_error(reason, "Changed diagnostic"))
                    .unwrap();
            assert_eq!(value["code"], "not_found");
            assert_eq!(value["details"]["reason"], expected);
            assert_eq!(value["message"], "Changed diagnostic");
        }
    }

    #[tokio::test]
    async fn absent_provider_patch_is_recoverable_but_database_failure_is_not() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let thread = ChatThreadId::new("thread:missing-patch").unwrap();
        let turn = ChatTurnId::new("turn:missing-patch").unwrap();
        let failure = provider_turn_patch(&pool, &thread, &turn)
            .await
            .unwrap_err();
        assert_ne!(failure.code, ChatErrorCode::NotFound);
        sqlx::query("CREATE TABLE chat_events (thread_id TEXT, turn_id TEXT, sequence INTEGER, event_type TEXT, payload_data TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        let missing = provider_turn_patch(&pool, &thread, &turn)
            .await
            .unwrap_err();
        assert_eq!(missing.code, ChatErrorCode::NotFound);
        assert_eq!(missing.details.unwrap()["reason"], "provider_turn");
        pool.close().await;
    }
}

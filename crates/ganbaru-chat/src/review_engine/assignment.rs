//! Checkpoint pairs that cover every local run of one work assignment.

use super::super::checkpoints::{StoredCheckpoint, read_stored_checkpoint, verify_checkpoint};
use super::super::models::{
    ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatWorkAssignmentId,
    ProjectWorkingFolderId,
};
use super::super::workspace::AuthorizedWorkingFolder;
use super::opening::{MissingReviewSource, missing_review_source_error};
use super::{corrupt_data_error, persistence_error, review_error};
use serde::Deserialize;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

/// Most changed file names an executed elsewhere error carries for the panel to list.
pub(super) const MAX_ELSEWHERE_CHANGED_FILES: usize = 200;

/// The first pre checkpoint and the last settled post checkpoint of an assignment's runs.
#[derive(Clone, Debug)]
pub(super) struct AssignmentCheckpointPair {
    pub pre: StoredCheckpoint,
    pub post: StoredCheckpoint,
    /// Working folder of every run, in run order; `None` for a scratch run.
    pub working_folder_ids: Vec<Option<String>>,
}

impl AssignmentCheckpointPair {
    /// Revision scope of the assignment review, keyed by the latest post checkpoint so a later
    /// run invalidates a review opened before it.
    pub fn revision_scope(&self, assignment_id: &ChatWorkAssignmentId) -> String {
        format!(
            "assignment:{}:{}",
            assignment_id.as_str(),
            self.post.id.as_str()
        )
    }
}

struct RunCheckpoints {
    working_folder_id: Option<String>,
    pre_checkpoint_id: Option<String>,
    post_checkpoint_id: Option<String>,
}

/// Loads the checkpoint pair of every local run of `assignment_id`, ordered by run ordinal.
///
/// The pair starts at the first run's pre checkpoint and ends at the last run with a post
/// checkpoint. An assignment with no local runs ran on another device, and that error
/// carries the executing device and the replicated changed file names instead of a diff.
pub(super) async fn assignment_checkpoint_pair(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> ChatResult<AssignmentCheckpointPair> {
    let executing_device_id: Option<String> =
        sqlx::query_scalar("SELECT executing_device_id FROM chat_work_assignments WHERE id = ?")
            .bind(assignment_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::NotFound,
                    "Work assignment was not found",
                    true,
                )
            })?;
    let rows = sqlx::query(
        "SELECT run.working_folder_id, turn.pre_checkpoint_id, turn.post_checkpoint_id
         FROM chat_agent_runs run
         JOIN chat_turns turn ON turn.id = run.provider_turn_id
         WHERE run.assignment_id = ?
         ORDER BY run.run_ordinal",
    )
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let runs = rows
        .iter()
        .map(|row| {
            Ok(RunCheckpoints {
                working_folder_id: row
                    .try_get("working_folder_id")
                    .map_err(persistence_error)?,
                pre_checkpoint_id: row
                    .try_get("pre_checkpoint_id")
                    .map_err(persistence_error)?,
                post_checkpoint_id: row
                    .try_get("post_checkpoint_id")
                    .map_err(persistence_error)?,
            })
        })
        .collect::<ChatResult<Vec<_>>>()?;
    if runs.is_empty() {
        return Err(executed_elsewhere_error(pool, assignment_id, executing_device_id).await?);
    }
    let pre = runs.first().and_then(|run| run.pre_checkpoint_id.clone());
    let post = runs
        .iter()
        .rev()
        .find_map(|run| run.post_checkpoint_id.clone());
    let (Some(pre), Some(post)) = (pre, post) else {
        return Err(missing_checkpoints_error());
    };
    let pre = load_checkpoint(pool, pre).await?;
    let post = load_checkpoint(pool, post).await?;
    if pre.repository_identity != post.repository_identity {
        return Err(missing_review_source_error(
            MissingReviewSource::AssignmentRepositoryChanged,
            "The assignment's runs used different repositories",
        ));
    }
    Ok(AssignmentCheckpointPair {
        pre,
        post,
        working_folder_ids: runs.into_iter().map(|run| run.working_folder_id).collect(),
    })
}

/// Loads one run checkpoint, reporting a checkpoint that is no longer available as a missing
/// assignment checkpoint rather than a generic not found error.
async fn load_checkpoint(pool: &SqlitePool, id: String) -> ChatResult<StoredCheckpoint> {
    let id = ChatCheckpointId::new(id).map_err(|_| corrupt_data_error())?;
    match read_stored_checkpoint(pool, &id).await {
        Err(error) if error.code == ChatErrorCode::NotFound => Err(missing_checkpoints_error()),
        result => result,
    }
}

fn missing_checkpoints_error() -> ChatError {
    missing_review_source_error(
        MissingReviewSource::AssignmentCheckpoints,
        "The assignment's runs do not have a settled checkpoint pair",
    )
}

/// Requires every run to have used `working_folder_id` and both checkpoints to belong to the
/// authorized repository. Provider threads may differ, because handoffs change threads.
pub(super) async fn verify_assignment_checkpoint_pair(
    authorized: &AuthorizedWorkingFolder,
    working_folder_id: &ProjectWorkingFolderId,
    pair: &AssignmentCheckpointPair,
) -> ChatResult<()> {
    if pair
        .working_folder_ids
        .iter()
        .any(|run_folder| run_folder.as_deref() != Some(working_folder_id.as_str()))
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Work assignment ran in another working folder",
            false,
        ));
    }
    let authorized = authorized.clone();
    let pre = pair.pre.clone();
    let post = pair.post.clone();
    tokio::task::spawn_blocking(move || {
        verify_checkpoint(&authorized, &pre)?;
        verify_checkpoint(&authorized, &post)
    })
    .await
    .map_err(|_| review_error("Git checkpoint verification worker stopped"))?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewPayload {
    #[serde(default)]
    changed_files: Vec<ChangedFilePath>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangedFilePath {
    relative_path: String,
}

/// Builds the executed elsewhere error with the executing device and the distinct changed
/// file names from the assignment's replicated review updates, capped for the panel.
async fn executed_elsewhere_error(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
    executing_device_id: Option<String>,
) -> ChatResult<ChatError> {
    let payloads: Vec<String> = sqlx::query_scalar(
        "SELECT payload_data FROM chat_work_semantic_updates
         WHERE assignment_id = ? AND update_kind = 'review'",
    )
    .bind(assignment_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut paths = BTreeSet::new();
    for payload in payloads {
        // Review payloads are validated when written; an unreadable one adds no names.
        if let Ok(payload) = serde_json::from_str::<ReviewPayload>(&payload) {
            paths.extend(
                payload
                    .changed_files
                    .into_iter()
                    .map(|file| file.relative_path),
            );
        }
    }
    let truncated = paths.len() > MAX_ELSEWHERE_CHANGED_FILES;
    let changed_files = paths
        .into_iter()
        .take(MAX_ELSEWHERE_CHANGED_FILES)
        .collect::<Vec<_>>();
    let mut error = missing_review_source_error(
        MissingReviewSource::ExecutedElsewhere,
        "The assignment ran on another device",
    );
    error.details = Some(Box::new(serde_json::json!({
        "reason": MissingReviewSource::ExecutedElsewhere,
        "executingDeviceId": executing_device_id,
        "changedFiles": changed_files,
        "changedFilesTruncated": truncated,
    })));
    Ok(error)
}

#[cfg(test)]
mod tests;

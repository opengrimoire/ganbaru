//! Authored path filtering for checkpoint and assignment reviews.
//!
//! A checkpoint diff spans everything that changed in the workspace between two captures,
//! including files a pull, merge, or rebase brought in while the agent worked. This module
//! separates the files the reviewed work changed from those Git operations imported.

use super::super::checkpoints::StoredCheckpoint;
use super::super::events::CanonicalEvent;
use super::super::models::{
    ChatCheckpointId, ChatResult, ChatThreadId, ChatTurnId, ChatWorkAssignmentId,
};
use super::contracts::ReviewAuthoredPathsStatus;
use super::persistence_error;
use super::registry::ReviewFileInternal;
use super::validation::validate_path;
use crate::git::{self, GitFolder};
use chrono::DateTime;
use sqlx::SqlitePool;
use std::collections::BTreeSet;

/// Output cap for each name-only list that builds the authored set.
pub(super) const MAX_AUTHORED_PATH_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// Most provider diff events read for one review scope.
const MAX_PROVIDER_REPORT_EVENTS: i64 = 4_096;

/// The authored file set of a checkpoint pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AuthoredPaths {
    /// HEAD did not move, so every changed file came from the working tree.
    NotNeeded,
    /// Files the reviewed work changed, by repository-relative path.
    Computed(BTreeSet<String>),
    /// The set could not be built, so no file is hidden.
    Unavailable(AuthoredPathsUnavailable),
}

/// Why the authored set could not be built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthoredPathsUnavailable {
    /// One of the name-only lists exceeded [`MAX_AUTHORED_PATH_OUTPUT_BYTES`].
    OutputTooLarge,
    /// Git refused one of the lists, or a stored object ID or capture time was unusable.
    GitFailed,
}

impl AuthoredPaths {
    /// The wire status reported with the snapshot.
    pub(super) fn status(&self) -> ReviewAuthoredPathsStatus {
        match self {
            Self::NotNeeded => ReviewAuthoredPathsStatus::NotNeeded,
            Self::Computed(_) => ReviewAuthoredPathsStatus::Computed,
            Self::Unavailable(_) => ReviewAuthoredPathsStatus::Unavailable,
        }
    }

    /// Whether a changed file counts as authored. The provider-reported union keeps
    /// provider-observed work visible even when its commit date falls outside the run.
    pub(super) fn is_authored(
        &self,
        relative_path: &str,
        previous_relative_path: Option<&str>,
        provider_reported: bool,
    ) -> bool {
        match self {
            Self::NotNeeded | Self::Unavailable(_) => true,
            Self::Computed(paths) => {
                provider_reported
                    || paths.contains(relative_path)
                    || previous_relative_path.is_some_and(|previous| paths.contains(previous))
            }
        }
    }
}

/// Builds the authored set of a checkpoint pair.
///
/// When HEAD moved between the captures, the set is the union of the uncommitted changes at
/// each capture, the commits HEAD dropped, and the commits HEAD gained that were made after
/// `captured_at`. Commits merged in from before the run are therefore excluded.
pub(super) async fn authored_paths(
    folder: GitFolder<'_>,
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
    captured_at: &str,
) -> AuthoredPaths {
    authored_paths_with_limit(
        folder,
        pre,
        post,
        captured_at,
        MAX_AUTHORED_PATH_OUTPUT_BYTES,
    )
    .await
}

/// [`authored_paths`] with an explicit output cap for each name-only list.
async fn authored_paths_with_limit(
    folder: GitFolder<'_>,
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
    captured_at: &str,
    maximum_output_bytes: usize,
) -> AuthoredPaths {
    let (Some(pre_head), Some(post_head)) = (pre.head_oid.as_deref(), post.head_oid.as_deref())
    else {
        return AuthoredPaths::NotNeeded;
    };
    if pre_head == post_head {
        return AuthoredPaths::NotNeeded;
    }
    let Some(since) = since_argument(captured_at) else {
        return AuthoredPaths::Unavailable(AuthoredPathsUnavailable::GitFailed);
    };
    if ![pre_head, post_head, &pre.git_object_id, &post.git_object_id]
        .iter()
        .all(|oid| is_object_id(oid))
    {
        return AuthoredPaths::Unavailable(AuthoredPathsUnavailable::GitFailed);
    }
    let dropped = format!("{post_head}..{pre_head}");
    let gained = format!("{pre_head}..{post_head}");
    let lists: [&[&str]; 4] = [
        &["diff", pre_head, &pre.git_object_id, "--"],
        &["diff", post_head, &post.git_object_id, "--"],
        &["log", "--diff-merges=remerge", "--format=", &dropped, "--"],
        &[
            "log",
            "--diff-merges=remerge",
            "--format=",
            &since,
            &gained,
            "--",
        ],
    ];
    let mut paths = BTreeSet::new();
    for list in lists {
        match name_list(folder, list, maximum_output_bytes).await {
            Ok(names) => paths.extend(names),
            Err(reason) => return AuthoredPaths::Unavailable(reason),
        }
    }
    AuthoredPaths::Computed(paths)
}

/// Runs one name-only list with output that does not depend on user configuration.
async fn name_list(
    folder: GitFolder<'_>,
    command: &[&str],
    maximum_output_bytes: usize,
) -> Result<BTreeSet<String>, AuthoredPathsUnavailable> {
    let (subcommand, rest) = command
        .split_first()
        .ok_or(AuthoredPathsUnavailable::GitFailed)?;
    let mut arguments = vec![
        "-c",
        "diff.relative=false",
        "-c",
        "log.showSignature=false",
        subcommand,
        "--name-only",
        "-z",
        "--no-renames",
        "--no-ext-diff",
    ];
    arguments.extend_from_slice(rest);
    let output = git::review_output_unless_too_large(folder, &arguments, maximum_output_bytes)
        .await
        .map_err(|_| AuthoredPathsUnavailable::GitFailed)?
        .ok_or(AuthoredPathsUnavailable::OutputTooLarge)?;
    Ok(parse_name_list(&output))
}

/// Splits NUL-separated name output. With an empty commit format, `git log` prints only the
/// names of each commit, so both commands share this shape.
fn parse_name_list(output: &[u8]) -> BTreeSet<String> {
    output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .map(|field| String::from_utf8_lossy(field).into_owned())
        .collect()
}

/// Formats a capture time as a raw Git date, so `--since` does not depend on date parsing
/// heuristics for fractional seconds or time zones.
fn since_argument(captured_at: &str) -> Option<String> {
    let captured = DateTime::parse_from_rfc3339(captured_at).ok()?;
    Some(format!("--since=@{} +0000", captured.timestamp()))
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The changes a review covers, for reading the paths its provider reported.
pub(super) enum ProviderReportScope<'a> {
    /// One turn, or the thread's latest turn when `turn_id` is `None`.
    Turn {
        thread_id: &'a ChatThreadId,
        turn_id: Option<&'a ChatTurnId>,
    },
    /// Every turn of a thread that has not been invalidated.
    Thread(&'a ChatThreadId),
    /// Every local run of a work assignment.
    Assignment(&'a ChatWorkAssignmentId),
}

/// Paths the provider reported changing within `scope`, from its diff events. Unreadable
/// events and invalid paths add nothing.
pub(super) async fn provider_reported_paths(
    pool: &SqlitePool,
    scope: ProviderReportScope<'_>,
) -> ChatResult<BTreeSet<String>> {
    let payloads: Vec<String> = match scope {
        ProviderReportScope::Turn {
            thread_id,
            turn_id: Some(turn_id),
        } => {
            sqlx::query_scalar(
                "SELECT payload_data FROM chat_events
                 WHERE thread_id = ? AND turn_id = ? AND event_type = 'diff_updated'
                 ORDER BY sequence DESC LIMIT ?",
            )
            .bind(thread_id.as_str())
            .bind(turn_id.as_str())
            .bind(MAX_PROVIDER_REPORT_EVENTS)
            .fetch_all(pool)
            .await
        }
        ProviderReportScope::Turn {
            thread_id,
            turn_id: None,
        } => {
            sqlx::query_scalar(
                "SELECT payload_data FROM chat_events
                 WHERE event_type = 'diff_updated'
                   AND turn_id = (
                     SELECT id FROM chat_turns
                     WHERE thread_id = ? AND invalidated_at IS NULL
                     ORDER BY ordinal DESC LIMIT 1
                   )
                 ORDER BY sequence DESC LIMIT ?",
            )
            .bind(thread_id.as_str())
            .bind(MAX_PROVIDER_REPORT_EVENTS)
            .fetch_all(pool)
            .await
        }
        ProviderReportScope::Thread(thread_id) => {
            sqlx::query_scalar(
                "SELECT event.payload_data FROM chat_events event
                 JOIN chat_turns turn ON turn.id = event.turn_id
                 WHERE event.thread_id = ? AND event.event_type = 'diff_updated'
                   AND turn.invalidated_at IS NULL
                 ORDER BY event.sequence DESC LIMIT ?",
            )
            .bind(thread_id.as_str())
            .bind(MAX_PROVIDER_REPORT_EVENTS)
            .fetch_all(pool)
            .await
        }
        ProviderReportScope::Assignment(assignment_id) => {
            sqlx::query_scalar(
                "SELECT payload_data FROM chat_events
                 WHERE event_type = 'diff_updated'
                   AND turn_id IN (
                     SELECT provider_turn_id FROM chat_agent_runs WHERE assignment_id = ?
                   )
                 ORDER BY sequence DESC LIMIT ?",
            )
            .bind(assignment_id.as_str())
            .bind(MAX_PROVIDER_REPORT_EVENTS)
            .fetch_all(pool)
            .await
        }
    }
    .map_err(persistence_error)?;
    let mut paths = BTreeSet::new();
    for payload in payloads {
        let Ok(CanonicalEvent::DiffUpdated(event)) = serde_json::from_str(&payload) else {
            continue;
        };
        for file in event.files {
            paths.extend(
                std::iter::once(file.relative_path)
                    .chain(file.previous_relative_path)
                    .filter(|path| validate_path(path).is_ok()),
            );
        }
    }
    Ok(paths)
}

/// Marks provider-reported and authored files of a checkpoint or assignment review and
/// returns the filter status for the snapshot.
pub(super) async fn apply_authored_flags(
    pool: &SqlitePool,
    folder: GitFolder<'_>,
    files: &mut [ReviewFileInternal],
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
    scope: ProviderReportScope<'_>,
) -> ChatResult<ReviewAuthoredPathsStatus> {
    let reported = provider_reported_paths(pool, scope).await?;
    let captured_at = checkpoint_captured_at(pool, &pre.id).await?;
    let authored = authored_paths(folder, pre, post, &captured_at).await;
    for file in files {
        let read = &mut file.read;
        if reported.contains(&read.relative_path)
            || read
                .previous_relative_path
                .as_ref()
                .is_some_and(|previous| reported.contains(previous))
        {
            read.flags.provider_reported = true;
        }
        read.flags.authored = authored.is_authored(
            &read.relative_path,
            read.previous_relative_path.as_deref(),
            read.flags.provider_reported,
        );
    }
    Ok(authored.status())
}

async fn checkpoint_captured_at(pool: &SqlitePool, id: &ChatCheckpointId) -> ChatResult<String> {
    sqlx::query_scalar("SELECT created_at FROM chat_checkpoints WHERE id = ?")
        .bind(id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)
}

#[cfg(test)]
mod tests;

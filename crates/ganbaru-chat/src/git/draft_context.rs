//! Bounded Git context a provider reads to draft commit and change request text.
//!
//! Every limit is explicit in the read: a patch past its byte limit is cut at a character
//! boundary with `patch_truncated` set, and paths past the file limit are counted in
//! `omitted_files`, so a draft request never describes a silently shortened change.

use super::{
    ChatError, ChatResult, GitCall, GitFolder, MAX_GIT_OUTPUT_BYTES, remotes, run,
    run_unless_too_large, validate_branch_name,
};
use serde::{Deserialize, Serialize};

/// Longest patch, in bytes, a draft context carries.
pub const COMMIT_DRAFT_MAX_PATCH_BYTES: usize = 64 * 1024;
/// Most changed paths a draft context lists.
pub const COMMIT_DRAFT_MAX_FILES: usize = 200;
/// Most recent commit subjects a draft context carries as style examples.
pub const COMMIT_DRAFT_RECENT_SUBJECTS: usize = 20;

/// The changes a commit or change request describes, bounded for a provider prompt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCommitDraftContextRead {
    /// Changed repository-relative paths, at most [`COMMIT_DRAFT_MAX_FILES`].
    pub staged_paths: Vec<String>,
    /// The patch, at most [`COMMIT_DRAFT_MAX_PATCH_BYTES`].
    pub patch: String,
    /// Whether `patch` was cut, or left out because Git output passed its bound.
    pub patch_truncated: bool,
    /// Changed paths left out of `staged_paths`.
    pub omitted_files: u32,
    /// Commit subjects, newest first: recent history for a commit, the range for a change request.
    pub recent_subjects: Vec<String>,
}

/// Reads the staged changes of `folder` and its recent commit subjects.
///
/// Fails with a validation error when nothing is staged. A repository without commits has
/// no recent subjects.
pub async fn commit_draft_context(folder: GitFolder<'_>) -> ChatResult<ChatCommitDraftContextRead> {
    let paths = changed_paths(folder, &["diff", "--cached", "--name-only", "-z"]).await?;
    if paths.is_empty() {
        return Err(ChatError::validation(
            "staged",
            "Stage changes before drafting a commit message",
        ));
    }
    let patch = bounded_patch(
        folder,
        &[
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
        ],
    )
    .await?;
    let recent_subjects = if head_exists(folder).await? {
        subjects(folder, "HEAD").await?
    } else {
        Vec::new()
    };
    Ok(context(paths, patch, recent_subjects))
}

/// Reads the changes `head` adds over its merge base with `base`, and the subjects of the
/// commits in that range.
///
/// Both names must be valid branch names. Each resolves to a local branch, then a
/// remote-tracking ref spelled `remote/name`, then the same branch on a configured remote,
/// and the range uses the resolved commit IDs, so neither name reaches Git as an option or a
/// range expression.
pub async fn change_request_draft_context(
    folder: GitFolder<'_>,
    base: &str,
    head: &str,
) -> ChatResult<ChatCommitDraftContextRead> {
    let base = resolve_branch_commit(folder, base, "base").await?;
    let head = resolve_branch_commit(folder, head, "head").await?;
    let merge_range = format!("{base}...{head}");
    let paths = changed_paths(folder, &["diff", "--name-only", "-z", &merge_range]).await?;
    let patch = bounded_patch(
        folder,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            &merge_range,
        ],
    )
    .await?;
    let recent_subjects = subjects(folder, &format!("{base}..{head}")).await?;
    Ok(context(paths, patch, recent_subjects))
}

/// A bounded patch and whether it was cut.
struct BoundedPatch {
    text: String,
    truncated: bool,
}

fn context(
    mut paths: Vec<String>,
    patch: BoundedPatch,
    recent_subjects: Vec<String>,
) -> ChatCommitDraftContextRead {
    let omitted = paths.len().saturating_sub(COMMIT_DRAFT_MAX_FILES);
    paths.truncate(COMMIT_DRAFT_MAX_FILES);
    ChatCommitDraftContextRead {
        staged_paths: paths,
        patch: patch.text,
        patch_truncated: patch.truncated,
        omitted_files: u32::try_from(omitted).unwrap_or(u32::MAX),
        recent_subjects,
    }
}

/// Runs a `--name-only -z` diff and returns its paths, decoded lossily.
async fn changed_paths(folder: GitFolder<'_>, arguments: &[&str]) -> ChatResult<Vec<String>> {
    let output = run(folder, arguments, GitCall::Internal).await?;
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

/// Runs a diff and keeps at most [`COMMIT_DRAFT_MAX_PATCH_BYTES`] of it. Output past the Git
/// output bound leaves the patch empty and marked truncated instead of failing the read.
async fn bounded_patch(folder: GitFolder<'_>, arguments: &[&str]) -> ChatResult<BoundedPatch> {
    let Some(output) = run_unless_too_large(folder, arguments, MAX_GIT_OUTPUT_BYTES).await? else {
        return Ok(BoundedPatch {
            text: String::new(),
            truncated: true,
        });
    };
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(truncate_patch(text, COMMIT_DRAFT_MAX_PATCH_BYTES))
}

/// Cuts `text` to at most `limit` bytes at a character boundary.
fn truncate_patch(mut text: String, limit: usize) -> BoundedPatch {
    if text.len() <= limit {
        return BoundedPatch {
            text,
            truncated: false,
        };
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    BoundedPatch {
        text,
        truncated: true,
    }
}

async fn head_exists(folder: GitFolder<'_>) -> ChatResult<bool> {
    commit_id(folder, "HEAD").await.map(|id| id.is_some())
}

/// The commit ID `revision` names, or `None` when it names no commit.
async fn commit_id(folder: GitFolder<'_>, revision: &str) -> ChatResult<Option<String>> {
    let target = format!("{revision}^{{commit}}");
    match run(
        folder,
        &["rev-parse", "--verify", "--quiet", &target],
        GitCall::Internal,
    )
    .await
    {
        Ok(output) => {
            let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok((!id.is_empty()).then_some(id))
        }
        Err(error) if error.code == super::ChatErrorCode::Conflict => Ok(None),
        Err(error) => Err(error),
    }
}

/// Resolves a branch name to a commit ID through fully qualified refs only.
async fn resolve_branch_commit(
    folder: GitFolder<'_>,
    name: &str,
    field: &'static str,
) -> ChatResult<String> {
    validate_branch_name(name).map_err(|error| ChatError::validation(field, error.message))?;
    let mut candidates = vec![format!("refs/heads/{name}"), format!("refs/remotes/{name}")];
    candidates.extend(
        remotes(folder)
            .await?
            .into_iter()
            .map(|remote| format!("refs/remotes/{}/{name}", remote.name)),
    );
    for candidate in candidates {
        if let Some(id) = commit_id(folder, &candidate).await? {
            return Ok(id);
        }
    }
    Err(ChatError::validation(
        field,
        "Git branch is not available in this repository",
    ))
}

/// The subjects of at most [`COMMIT_DRAFT_RECENT_SUBJECTS`] commits reachable as `revisions`.
async fn subjects(folder: GitFolder<'_>, revisions: &str) -> ChatResult<Vec<String>> {
    let count = format!("--max-count={COMMIT_DRAFT_RECENT_SUBJECTS}");
    let output = run(
        folder,
        &[
            "log",
            "--no-show-signature",
            "--format=%s",
            &count,
            revisions,
        ],
        GitCall::Internal,
    )
    .await?;
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|subject| !subject.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests;

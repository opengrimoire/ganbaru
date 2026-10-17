//! Guarded Git mutations for immutable review snapshots.

use super::super::models::{ChatError, ChatErrorCode, ChatResult};
use super::contracts::{ApplyChatReviewActionRequest, ReviewAction, ReviewWorkingTreeMode};
use super::material::deterministic_diff_arguments;
use super::patch_parser::parse_patch;
use super::registry::{ReviewFileInternal, ReviewSnapshot};
use super::validation::validate_path;
use super::{review_error, review_output};
use crate::git;
use std::path::Path;
use std::sync::Arc;

pub async fn apply_action(
    snapshot: &Arc<ReviewSnapshot>,
    request: &ApplyChatReviewActionRequest,
    mode: ReviewWorkingTreeMode,
) -> ChatResult<()> {
    if request.operation == ReviewAction::Discard && !request.confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Discarding reviewed changes requires explicit confirmation",
            true,
        ));
    }
    if snapshot.files.is_empty() {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The review snapshot has no changes to apply",
            true,
        ));
    }
    let file = request
        .file_id
        .as_deref()
        .map(|file_id| {
            snapshot
                .files
                .iter()
                .find(|file| file.read.file_id == file_id)
                .ok_or_else(|| {
                    ChatError::new(ChatErrorCode::NotFound, "Review file was not found", true)
                })
        })
        .transpose()?;
    if file.is_none()
        && snapshot.files.iter().any(|file| match request.operation {
            ReviewAction::Stage => !file.read.capabilities.stage,
            ReviewAction::Unstage => !file.read.capabilities.unstage,
            ReviewAction::Discard => !file.read.capabilities.discard,
        })
    {
        return Err(ChatError::unsupported(
            "One or more reviewed files do not support this whole-scope action",
        ));
    }
    require_action_allowed(mode, request.operation, file, &request.hunk_ids)?;
    if request.operation == ReviewAction::Stage && request.hunk_ids.is_empty() {
        let owned_paths = action_paths(snapshot, request.file_id.as_deref());
        if owned_paths.is_empty() {
            return Err(ChatError::new(
                ChatErrorCode::NotFound,
                "The review snapshot has no paths to stage",
                true,
            ));
        }
        stage_owned_paths(&snapshot.root, &owned_paths).await?;
        return Ok(());
    }
    if request.hunk_ids.is_empty() {
        if let Some(file) = file {
            return apply_exact_file_action(snapshot, file, request.operation, mode).await;
        }
        return apply_exact_scope_action(snapshot, request.operation, mode).await;
    }
    let patch = action_patch(snapshot, file, &request.hunk_ids).await?;
    let mut check = vec!["apply", "--check", "--recount", "--whitespace=nowarn"];
    let mut apply = vec!["apply", "--recount", "--whitespace=nowarn"];
    match request.operation {
        ReviewAction::Stage => {
            check.push("--cached");
            apply.push("--cached");
        }
        ReviewAction::Unstage => {
            check.extend(["--cached", "--reverse"]);
            apply.extend(["--cached", "--reverse"]);
        }
        ReviewAction::Discard => {
            check.push("--reverse");
            apply.push("--reverse");
        }
    }
    git::review_output(&snapshot.root, &check, Some(patch.as_bytes())).await?;
    git::review_output(&snapshot.root, &apply, Some(patch.as_bytes())).await?;
    Ok(())
}

async fn apply_exact_file_action(
    snapshot: &ReviewSnapshot,
    file: &ReviewFileInternal,
    operation: ReviewAction,
    mode: ReviewWorkingTreeMode,
) -> ChatResult<()> {
    let before = snapshot
        .before_oid
        .as_deref()
        .ok_or_else(|| ChatError::unsupported("This review source cannot be changed"))?;
    let current = file.read.relative_path.as_str();
    match operation {
        ReviewAction::Stage => {
            let paths = action_paths(snapshot, Some(&file.read.file_id));
            if paths.is_empty() {
                return Err(ChatError::new(
                    ChatErrorCode::NotFound,
                    "The review file has no path to stage",
                    true,
                ));
            }
            stage_owned_paths(&snapshot.root, &paths).await?;
        }
        ReviewAction::Unstage => {
            if mode != ReviewWorkingTreeMode::Staged {
                return Err(ChatError::unsupported(
                    "This file is not in the staged review scope",
                ));
            }
            if file.read.status == "added" {
                git::review_output(
                    &snapshot.root,
                    &["rm", "--cached", "--ignore-unmatch", "--", current],
                    None,
                )
                .await?;
            } else if let Some(previous) = file.read.previous_relative_path.as_deref() {
                git::review_output(
                    &snapshot.root,
                    &["restore", "--staged", "--source", before, "--", previous],
                    None,
                )
                .await?;
                git::review_output(
                    &snapshot.root,
                    &["rm", "--cached", "--ignore-unmatch", "--", current],
                    None,
                )
                .await?;
            } else {
                git::review_output(
                    &snapshot.root,
                    &["restore", "--staged", "--source", before, "--", current],
                    None,
                )
                .await?;
            }
        }
        ReviewAction::Discard => {
            if mode != ReviewWorkingTreeMode::Unstaged {
                return Err(ChatError::unsupported(
                    "Open Unstaged changes before discarding this file",
                ));
            }
            if let Some(previous) = file.read.previous_relative_path.as_deref() {
                git::review_output(
                    &snapshot.root,
                    &["restore", "--worktree", "--source", before, "--", previous],
                    None,
                )
                .await?;
                git::review_output(&snapshot.root, &["clean", "-f", "--", current], None).await?;
            } else if file.read.flags.untracked {
                git::review_output(&snapshot.root, &["clean", "-f", "--", current], None).await?;
            } else {
                git::review_output(
                    &snapshot.root,
                    &["restore", "--worktree", "--source", before, "--", current],
                    None,
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn apply_exact_scope_action(
    snapshot: &ReviewSnapshot,
    operation: ReviewAction,
    mode: ReviewWorkingTreeMode,
) -> ChatResult<()> {
    let before = snapshot
        .before_oid
        .as_deref()
        .ok_or_else(|| ChatError::unsupported("This review source cannot be changed"))?;
    match operation {
        ReviewAction::Stage => unreachable!("whole-scope staging returns before this helper"),
        ReviewAction::Unstage if mode == ReviewWorkingTreeMode::Staged => {
            restore_owned_paths(&snapshot.root, before, true, &action_paths(snapshot, None)).await
        }
        ReviewAction::Discard if mode == ReviewWorkingTreeMode::Unstaged => {
            discard_owned_files(snapshot, before).await
        }
        _ => Err(ChatError::unsupported(
            "This action is unavailable for the selected review scope",
        )),
    }
}

async fn discard_owned_files(snapshot: &ReviewSnapshot, before: &str) -> ChatResult<()> {
    let mut restore_paths = Vec::new();
    let mut clean_paths = Vec::new();
    for file in &snapshot.files {
        if let Some(previous) = file.read.previous_relative_path.as_ref() {
            restore_paths.push(previous.clone());
            clean_paths.push(file.read.relative_path.clone());
            continue;
        }
        if file.read.flags.untracked {
            clean_paths.push(file.read.relative_path.clone());
            continue;
        }
        restore_paths.push(file.read.relative_path.clone());
    }
    restore_paths.sort();
    restore_paths.dedup();
    clean_paths.sort();
    clean_paths.dedup();
    restore_owned_paths(&snapshot.root, before, false, &restore_paths).await?;
    for chunk in bounded_path_chunks(&clean_paths) {
        let mut arguments = vec!["clean", "-f", "--"];
        arguments.extend(chunk.iter().map(String::as_str));
        git::review_output(&snapshot.root, &arguments, None).await?;
    }
    Ok(())
}

async fn restore_owned_paths(
    root: &Path,
    source: &str,
    staged: bool,
    paths: &[String],
) -> ChatResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let pathspec = pathspec_bytes(paths)?;
    let mut arguments = vec!["restore"];
    if staged {
        arguments.push("--staged");
    } else {
        arguments.push("--worktree");
    }
    arguments.extend([
        "--source",
        source,
        "--pathspec-from-file=-",
        "--pathspec-file-nul",
    ]);
    git::review_output(root, &arguments, Some(&pathspec)).await?;
    Ok(())
}

async fn stage_owned_paths(root: &Path, paths: &[String]) -> ChatResult<()> {
    let pathspec = pathspec_bytes(paths)?;
    git::review_output(
        root,
        &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
        Some(&pathspec),
    )
    .await?;
    Ok(())
}

fn pathspec_bytes(paths: &[String]) -> ChatResult<Vec<u8>> {
    let mut pathspec = Vec::new();
    for path in paths {
        validate_path(path)?;
        pathspec.extend_from_slice(path.as_bytes());
        pathspec.push(0);
    }
    Ok(pathspec)
}

fn bounded_path_chunks(paths: &[String]) -> Vec<&[String]> {
    const MAX_PATHS_PER_COMMAND: usize = 64;
    const MAX_PATH_BYTES_PER_COMMAND: usize = 16 * 1024;
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < paths.len() {
        let mut end = start;
        let mut bytes = 0_usize;
        while end < paths.len() && end - start < MAX_PATHS_PER_COMMAND {
            let next = paths[end].len().saturating_add(1);
            if end > start && bytes.saturating_add(next) > MAX_PATH_BYTES_PER_COMMAND {
                break;
            }
            bytes = bytes.saturating_add(next);
            end += 1;
        }
        chunks.push(&paths[start..end]);
        start = end;
    }
    chunks
}

pub fn action_paths(snapshot: &ReviewSnapshot, file_id: Option<&str>) -> Vec<String> {
    let mut paths = snapshot
        .files
        .iter()
        .filter(|file| file_id.is_none_or(|selected| file.read.file_id == selected))
        .flat_map(|file| {
            std::iter::once(file.read.relative_path.clone())
                .chain(file.read.previous_relative_path.clone())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

async fn action_patch(
    snapshot: &Arc<ReviewSnapshot>,
    file: Option<&ReviewFileInternal>,
    selected_hunks: &[String],
) -> ChatResult<String> {
    let before = snapshot
        .before_oid
        .as_deref()
        .ok_or_else(|| ChatError::unsupported("Provider-reported patches cannot be applied"))?;
    let after = snapshot
        .after_oid
        .as_deref()
        .ok_or_else(|| ChatError::unsupported("Provider-reported patches cannot be applied"))?;
    let context = format!("--unified={}", snapshot.context_lines.max(3));
    let mut arguments = deterministic_diff_arguments(
        !selected_hunks.is_empty() && snapshot.ignore_whitespace,
        Some(&context),
    );
    arguments.extend([before, after]);
    if let Some(file) = file {
        arguments.extend(["--", &file.read.relative_path]);
        if let Some(previous) = file.read.previous_relative_path.as_deref() {
            arguments.push(previous);
        }
    }
    let patch = String::from_utf8(
        review_output(
            &snapshot.root,
            &arguments,
            None,
            snapshot.object_store.as_deref(),
        )
        .await?,
    )
    .map_err(|_| review_error("Git diff output is not valid UTF-8"))?;
    if selected_hunks.is_empty() {
        return Ok(patch);
    }
    let file_id = file
        .map(|file| file.read.file_id.as_str())
        .ok_or_else(|| ChatError::validation("fileId", "Hunk actions require one file"))?;
    let parsed = parse_patch(&patch, file_id)?;
    let selected = selected_hunks
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let mut result = parsed.preamble;
    let mut found = 0;
    for hunk in parsed.hunks {
        if selected.contains(hunk.read.hunk_id.as_str()) {
            result.push_str(&hunk.text);
            found += 1;
        }
    }
    if found != selected.len() {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "One or more reviewed hunks are no longer available",
            true,
        ));
    }
    Ok(result)
}

pub fn require_action_allowed(
    mode: ReviewWorkingTreeMode,
    operation: ReviewAction,
    file: Option<&ReviewFileInternal>,
    hunk_ids: &[String],
) -> ChatResult<()> {
    let scope_allowed = match (mode, operation) {
        (ReviewWorkingTreeMode::Staged, ReviewAction::Unstage) => true,
        (ReviewWorkingTreeMode::Unstaged, ReviewAction::Stage | ReviewAction::Discard) => true,
        (ReviewWorkingTreeMode::All, ReviewAction::Stage) if hunk_ids.is_empty() => true,
        _ => false,
    };
    let file_allowed = file.map(|file| match operation {
        ReviewAction::Stage => file.read.capabilities.stage,
        ReviewAction::Unstage => file.read.capabilities.unstage,
        ReviewAction::Discard => file.read.capabilities.discard,
    });
    if !scope_allowed || file_allowed == Some(false) {
        return Err(ChatError::unsupported(
            "This action is unavailable for the selected review scope",
        ));
    }
    if !hunk_ids.is_empty()
        && file.is_some_and(|file| {
            file.read.flags.binary
                || file.read.flags.submodule
                || file.read.flags.conflict
                || file.read.flags.mode_only
                || file.read.flags.pure_rename
                || file.read.flags.untracked
                || file.read.flags.symlink
        })
    {
        return Err(ChatError::unsupported(
            "This file only supports whole-file review actions",
        ));
    }
    Ok(())
}

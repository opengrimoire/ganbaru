//! Bounded checkpoint metadata and patch generation.

use super::git::git_output;
use super::{StoredCheckpoint, checkpoint_error};
use crate::events::ChangedFileSummary;
use crate::models::{ChatResult, ChatThreadId};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[cfg(test)]
const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;
type DiffLineCounts = BTreeMap<String, (Option<u64>, Option<u64>)>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatChangedFileRead {
    pub relative_path: String,
    pub previous_relative_path: Option<String>,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub provider_reported: bool,
    pub git_observed: bool,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ChatCheckpointFileDiffRead {
    pub relative_path: String,
    pub patch: Option<String>,
    pub binary: bool,
    pub truncated: bool,
    pub byte_size: u64,
}

pub fn diff_files(
    root: &Path,
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
) -> ChatResult<Vec<ChatChangedFileRead>> {
    let status = git_output(
        root,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-commit-id",
            "--find-renames",
            "--name-status",
            &pre.git_object_id,
            &post.git_object_id,
        ],
        None,
        None,
    )?;
    let stats = diff_numstat(root, &pre.git_object_id, &post.git_object_id)?;
    parse_name_status(&status.stdout, &stats)
}

#[cfg(test)]
pub(super) fn file_diff(
    root: &Path,
    pre: &StoredCheckpoint,
    post: &StoredCheckpoint,
    relative_path: &str,
    ignore_whitespace: bool,
) -> ChatResult<ChatCheckpointFileDiffRead> {
    validate_diff_path(relative_path)?;
    let mut arguments = vec![
        "diff",
        "--full-index",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        "--diff-algorithm=histogram",
        "--unified=3",
    ];
    if ignore_whitespace {
        arguments.push("-w");
    }
    arguments.extend([
        pre.git_object_id.as_str(),
        post.git_object_id.as_str(),
        "--",
        relative_path,
    ]);
    let output = git_output(root, &arguments, None, None)?;
    let byte_size = output.stdout.len() as u64;
    let truncated = output.stdout.len() > MAX_DIFF_BYTES;
    let patch = String::from_utf8(output.stdout)
        .map_err(|_| checkpoint_error("Git diff output is not valid UTF-8"))?;
    let binary = patch.contains("Binary files ") || patch.contains("GIT binary patch");
    let bounded = if truncated {
        truncate_patch_at_hunk_boundary(&patch, MAX_DIFF_BYTES)
    } else {
        patch
    };
    Ok(ChatCheckpointFileDiffRead {
        relative_path: relative_path.to_string(),
        patch: (!binary).then_some(bounded),
        binary,
        truncated,
        byte_size,
    })
}

pub(super) fn changed_file_summaries(
    root: &Path,
    pre_oid: &str,
    post_oid: &str,
) -> ChatResult<Vec<ChangedFileSummary>> {
    let pre = StoredCheckpoint {
        id: crate::models::ChatCheckpointId::new("checkpoint:pre").map_err(checkpoint_error)?,
        thread_id: ChatThreadId::new("thread:pre").map_err(checkpoint_error)?,
        turn_count: 0,
        repository_identity: String::new(),
        hidden_ref_name: String::new(),
        git_object_id: pre_oid.to_string(),
        index_tree_oid: String::new(),
        worktree_tree_oid: String::new(),
        head_oid: None,
        head_ref: None,
    };
    let post = StoredCheckpoint {
        git_object_id: post_oid.to_string(),
        ..pre.clone()
    };
    Ok(diff_files(root, &pre, &post)?
        .into_iter()
        .map(|file| ChangedFileSummary {
            relative_path: file.relative_path,
            previous_relative_path: file.previous_relative_path,
            additions: file.additions,
            deletions: file.deletions,
            binary: file.binary,
            status: file.status,
        })
        .collect())
}

#[cfg(test)]
fn truncate_patch_at_hunk_boundary(patch: &str, maximum_bytes: usize) -> String {
    if patch.len() <= maximum_bytes {
        return patch.to_string();
    }
    let mut boundaries = patch
        .match_indices("\n@@ ")
        .map(|(index, _)| index + 1)
        .collect::<Vec<_>>();
    if patch.starts_with("@@ ") {
        boundaries.insert(0, 0);
    }
    let first_hunk = boundaries.first().copied().unwrap_or(patch.len());
    let mut end = if first_hunk <= maximum_bytes {
        first_hunk
    } else {
        0
    };
    for (index, start) in boundaries.iter().copied().enumerate() {
        let next = boundaries.get(index + 1).copied().unwrap_or(patch.len());
        if next > maximum_bytes || start > end {
            break;
        }
        end = next;
    }
    patch[..end].to_string()
}

fn diff_numstat(root: &Path, pre_oid: &str, post_oid: &str) -> ChatResult<DiffLineCounts> {
    let output = git_output(
        root,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
            "--find-renames",
            pre_oid,
            post_oid,
        ],
        None,
        None,
    )?;
    let mut result = BTreeMap::new();
    let mut fields = output.stdout.split(|byte| *byte == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(field);
        let mut parts = text.splitn(3, '\t');
        let additions = parse_stat(parts.next());
        let deletions = parse_stat(parts.next());
        let path = parts.next().unwrap_or_default();
        let destination_path = if path.is_empty() {
            let _source_path = fields.next();
            fields
                .next()
                .and_then(|value| std::str::from_utf8(value).ok())
                .unwrap_or_default()
        } else {
            path
        };
        if !destination_path.is_empty() {
            result.insert(destination_path.to_string(), (additions, deletions));
        }
    }
    Ok(result)
}

fn parse_name_status(bytes: &[u8], stats: &DiffLineCounts) -> ChatResult<Vec<ChatChangedFileRead>> {
    let fields = bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0;
    let mut result = Vec::new();
    while index < fields.len() {
        let status = std::str::from_utf8(fields[index]).map_err(checkpoint_error)?;
        index += 1;
        let previous_relative_path = if status.starts_with('R') || status.starts_with('C') {
            let previous = field_path(fields.get(index))?;
            index += 1;
            Some(previous)
        } else {
            None
        };
        let relative_path = field_path(fields.get(index))?;
        index += 1;
        let (additions, deletions) = stats.get(&relative_path).copied().unwrap_or((None, None));
        result.push(ChatChangedFileRead {
            relative_path,
            previous_relative_path,
            status: match status.chars().next() {
                Some('A') => "added",
                Some('M') => "modified",
                Some('D') => "deleted",
                Some('R') | Some('C') => "renamed",
                Some('T') => "type_changed",
                _ => "unknown",
            }
            .to_string(),
            additions,
            deletions,
            binary: additions.is_none() || deletions.is_none(),
            provider_reported: false,
            git_observed: true,
        });
    }
    Ok(result)
}

fn validate_diff_path(path: &str) -> ChatResult<()> {
    if path.is_empty()
        || path.len() > 4_096
        || Path::new(path).is_absolute()
        || path.contains('\u{005c}')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || path.chars().any(char::is_control)
    {
        return Err(crate::models::ChatError::validation(
            "relativePath",
            "Diff path must be a normalized workspace-relative path",
        ));
    }
    Ok(())
}

fn field_path(field: Option<&&[u8]>) -> ChatResult<String> {
    let path = field.ok_or_else(|| checkpoint_error("Git diff metadata is incomplete"))?;
    let path = std::str::from_utf8(path).map_err(checkpoint_error)?;
    validate_diff_path(path)?;
    Ok(path.to_string())
}

fn parse_stat(value: Option<&str>) -> Option<u64> {
    value.and_then(|value| value.parse().ok())
}

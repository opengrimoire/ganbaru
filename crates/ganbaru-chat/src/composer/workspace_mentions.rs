//! Bounded workspace path discovery and mention safety policy.

use super::super::models::{ChatError, ChatErrorCode, ChatResult};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_PATH_RESULTS: u32 = 100;
const MAX_PATH_SCAN: usize = 20_000;
const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolderPathRead {
    pub relative_path: String,
    pub display_name: String,
    pub kind: String,
    pub ignored: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectWorkingFolderPathPage {
    pub entries: Vec<ProjectWorkingFolderPathRead>,
    pub next_cursor: Option<String>,
}

pub fn search_workspace_paths(
    root: &Path,
    query: &str,
    include_ignored: bool,
    cursor: Option<&str>,
    limit: u32,
) -> ChatResult<ProjectWorkingFolderPathPage> {
    let query = query.trim().to_lowercase();
    if query.len() > 500 || query.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "query",
            "Workspace path query is invalid",
        ));
    }
    let offset = parse_offset(cursor)?;
    let visible = git_visible_paths(root).unwrap_or_default();
    let mut entries = scan_paths(root, include_ignored, &visible)?;
    entries.retain(|entry| fuzzy_path_matches(&entry.relative_path, &query));
    entries.sort_by(|left, right| {
        path_rank(&left.relative_path, &query)
            .cmp(&path_rank(&right.relative_path, &query))
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    let page_size = limit.clamp(1, MAX_PATH_RESULTS) as usize;
    let page = entries
        .into_iter()
        .skip(offset)
        .take(page_size + 1)
        .collect::<Vec<_>>();
    let has_more = page.len() > page_size;
    Ok(ProjectWorkingFolderPathPage {
        entries: page.into_iter().take(page_size).collect(),
        next_cursor: has_more.then(|| (offset + page_size).to_string()),
    })
}

pub fn workspace_mention_is_safety_excluded(relative_path: &str) -> bool {
    let first = relative_path.split('/').next().unwrap_or(relative_path);
    matches!(first, ".git" | ".ssh" | ".gnupg" | ".aws")
        || relative_path == ".env"
        || (relative_path.starts_with(".env.") && relative_path != ".env.example")
        || relative_path == ".codex/auth.json"
        || relative_path == ".claude/.credentials.json"
}

fn scan_paths(
    root: &Path,
    include_ignored: bool,
    visible: &BTreeSet<String>,
) -> ChatResult<Vec<ProjectWorkingFolderPathRead>> {
    let mut result = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(workspace_io_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(workspace_io_error)?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if result.len() >= MAX_PATH_SCAN {
                return Ok(result);
            }
            let path = entry.path();
            let file_type = entry.file_type().map_err(workspace_io_error)?;
            if file_type.is_symlink() || entry.file_name() == ".git" {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| workspace_io_error(()))?
                .to_string_lossy()
                .replace('\\', "/");
            if workspace_mention_is_safety_excluded(&relative) {
                continue;
            }
            let ignored = !visible.is_empty()
                && if file_type.is_dir() {
                    let prefix = format!("{relative}/");
                    !visible.iter().any(|entry| entry.starts_with(&prefix))
                } else {
                    !visible.contains(&relative)
                };
            if include_ignored || !ignored {
                result.push(ProjectWorkingFolderPathRead {
                    display_name: entry.file_name().to_string_lossy().into_owned(),
                    relative_path: relative,
                    kind: if file_type.is_dir() {
                        "directory"
                    } else {
                        "file"
                    }
                    .to_string(),
                    ignored,
                });
            }
            if file_type.is_dir() && (include_ignored || !ignored) {
                pending.push(path);
            }
        }
    }
    Ok(result)
}

fn git_visible_paths(root: &Path) -> ChatResult<BTreeSet<String>> {
    if !root.join(".git").exists() {
        return Ok(BTreeSet::new());
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .output()
        .map_err(workspace_io_error)?;
    if !output.status.success() || output.stdout.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(workspace_io_error(()));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|path| String::from_utf8(path.to_vec()).ok())
        .collect())
}

fn fuzzy_path_matches(path: &str, query: &str) -> bool {
    if query.is_empty() || path.to_lowercase().contains(query) {
        return true;
    }
    let mut query_chars = query.chars();
    let mut wanted = query_chars.next();
    for character in path.chars().flat_map(char::to_lowercase) {
        if wanted == Some(character) {
            wanted = query_chars.next();
        }
    }
    wanted.is_none()
}

fn path_rank(path: &str, query: &str) -> (u8, usize, usize) {
    let normalized = path.to_lowercase();
    let name = normalized.rsplit('/').next().unwrap_or(&normalized);
    if name == query {
        (0, path.len(), 0)
    } else if name.starts_with(query) {
        (1, path.len(), 0)
    } else if let Some(index) = normalized.find(query) {
        (2, path.len(), index)
    } else {
        (3, path.len(), usize::MAX)
    }
}

fn parse_offset(cursor: Option<&str>) -> ChatResult<usize> {
    cursor
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| ChatError::validation("cursor", "Workspace search cursor is invalid"))
}

fn workspace_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Project working-folder paths could not be read",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching_and_safety_exclusions_are_bounded() {
        assert!(fuzzy_path_matches("src/calendar/view.ts", "scv"));
        assert!(
            path_rank("src/calendar", "calendar") < path_rank("docs/my-calendar.md", "calendar")
        );
        assert!(workspace_mention_is_safety_excluded(".git/config"));
        assert!(workspace_mention_is_safety_excluded(".env.local"));
        assert!(!workspace_mention_is_safety_excluded(".env.example"));
        assert!(!workspace_mention_is_safety_excluded("src/config.ts"));
        assert!(parse_offset(Some("not-a-number")).is_err());
    }
}

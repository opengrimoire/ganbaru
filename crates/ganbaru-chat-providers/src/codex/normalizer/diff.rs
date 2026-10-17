use super::value::*;
use super::*;

pub(super) fn file_change_metadata(item: &Map<String, Value>) -> Vec<Value> {
    item.get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_FILE_CHANGES)
        .filter_map(Value::as_object)
        .filter_map(|change| {
            let path = text(change, "path")?;
            Some(json!({
                "path": bounded_text(path, 4 * 1024),
                "kind": text(change, "kind").map(|value| bounded_text(value, 128)),
                "diff": text(change, "diff")
                    .map(|value| bounded_text(value, MAX_PROVIDER_TEXT_BYTES)),
            }))
        })
        .collect()
}

pub(super) fn changed_files_from_item(item: &Map<String, Value>) -> Vec<ChangedFileSummary> {
    item.get("changes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(MAX_FILE_CHANGES)
        .filter_map(Value::as_object)
        .filter_map(|change| {
            let path = text(change, "path")?;
            let kind = text(change, "kind").unwrap_or("modified");
            let diff = text(change, "diff").unwrap_or("");
            let (additions, deletions) = unified_diff_line_counts(diff);
            Some(ChangedFileSummary {
                relative_path: bounded_text(path, 4 * 1024),
                previous_relative_path: None,
                additions: Some(additions),
                deletions: Some(deletions),
                binary: unified_diff_is_binary(diff),
                status: normalize_file_change_status(kind).to_string(),
            })
        })
        .collect()
}

pub(super) fn changed_files_from_unified_diff(diff: &str) -> Vec<ChangedFileSummary> {
    #[derive(Default)]
    struct PendingFile {
        path: String,
        previous_path: Option<String>,
        additions: u64,
        deletions: u64,
        binary: bool,
        status: String,
    }

    fn finish(files: &mut Vec<ChangedFileSummary>, current: Option<PendingFile>) {
        let Some(current) = current.filter(|file| !file.path.is_empty()) else {
            return;
        };
        files.push(ChangedFileSummary {
            relative_path: current.path,
            previous_relative_path: current.previous_path,
            additions: Some(current.additions),
            deletions: Some(current.deletions),
            binary: current.binary,
            status: if current.status.is_empty() {
                "modified".to_string()
            } else {
                current.status
            },
        });
    }

    let mut files = Vec::new();
    let mut current: Option<PendingFile> = None;
    for line in diff.lines() {
        if let Some(paths) = line.strip_prefix("diff --git ") {
            finish(&mut files, current.take());
            let mut parts = paths.split_whitespace();
            let previous = parts.next().map(normalize_diff_path);
            let path = parts.next().map(normalize_diff_path).unwrap_or_default();
            current = Some(PendingFile {
                previous_path: previous.filter(|value| value != &path),
                path,
                status: "modified".to_string(),
                ..PendingFile::default()
            });
            continue;
        }
        let file = current.get_or_insert_with(PendingFile::default);
        if line.starts_with("new file mode ") {
            file.status = "added".to_string();
        } else if line.starts_with("deleted file mode ") {
            file.status = "deleted".to_string();
        } else if let Some(path) = line.strip_prefix("rename from ") {
            file.previous_path = Some(path.to_string());
            file.status = "renamed".to_string();
        } else if let Some(path) = line.strip_prefix("rename to ") {
            file.path = path.to_string();
            file.status = "renamed".to_string();
        } else if let Some(path) = line.strip_prefix("+++ ") {
            let path = normalize_diff_path(path);
            if path != "/dev/null" && file.path.is_empty() {
                file.path = path;
            }
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
        } else if line.starts_with('+') && !line.starts_with("+++") {
            file.additions = file.additions.saturating_add(1);
        } else if line.starts_with('-') && !line.starts_with("---") {
            file.deletions = file.deletions.saturating_add(1);
        }
    }
    finish(&mut files, current);
    files.truncate(MAX_FILE_CHANGES);
    files
}

pub(super) fn normalize_diff_path(path: &str) -> String {
    path.trim()
        .trim_matches('"')
        .strip_prefix("a/")
        .or_else(|| path.trim().trim_matches('"').strip_prefix("b/"))
        .unwrap_or(path.trim().trim_matches('"'))
        .to_string()
}

pub(super) fn unified_diff_line_counts(diff: &str) -> (u64, u64) {
    diff.lines()
        .fold((0_u64, 0_u64), |(additions, deletions), line| {
            if line.starts_with('+') && !line.starts_with("+++") {
                (additions.saturating_add(1), deletions)
            } else if line.starts_with('-') && !line.starts_with("---") {
                (additions, deletions.saturating_add(1))
            } else {
                (additions, deletions)
            }
        })
}

pub(super) fn unified_diff_is_binary(diff: &str) -> bool {
    diff.lines()
        .any(|line| line.starts_with("Binary files ") || line == "GIT binary patch")
}

pub(super) fn normalize_file_change_status(kind: &str) -> &str {
    match kind {
        "add" | "added" | "create" | "created" => "added",
        "delete" | "deleted" | "remove" | "removed" => "deleted",
        "rename" | "renamed" | "move" | "moved" => "renamed",
        _ => "modified",
    }
}

pub(super) fn merge_changed_files(
    current: &mut Vec<ChangedFileSummary>,
    incoming: &[ChangedFileSummary],
) {
    for file in incoming {
        if let Some(existing) = current
            .iter_mut()
            .find(|candidate| candidate.relative_path == file.relative_path)
        {
            *existing = file.clone();
        } else {
            current.push(file.clone());
        }
    }
}

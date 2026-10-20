//! Immutable Git material capture and review file discovery.

use super::super::models::{ChatError, ChatErrorCode, ChatResult};
use super::contracts::{ReviewDiffSource, ReviewFileRead, ReviewWorkingTreeMode};
use super::opening::file_contract;
use super::patch_store::{ReviewObjectStore, review_storage_limit_error};
use super::registry::ReviewFileInternal;
use super::validation::validate_path;
use super::{
    corrupt_data_error, empty_tree, git_text, optional_head, path_field, review_error,
    review_output, review_storage_text,
};
use crate::git::{self, GitChangedPathRead, GitFolder};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Largest changed file copied into a working tree review. Larger files are
/// listed as too large to review, so one oversized file never blocks a review.
const MAX_REVIEW_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Largest total size of the changed files copied into a working tree review,
/// measured before any content reaches temporary storage.
const MAX_REVIEW_CAPTURE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone)]
pub struct SnapshotMaterial {
    pub before_oid: String,
    pub after_oid: String,
    pub label: String,
    pub status: git::GitStatusRead,
    pub object_store: Option<Arc<ReviewObjectStore>>,
    /// Changed files left out of the captured tree because they are too large.
    pub too_large_paths: BTreeSet<String>,
}

/// Size limits applied to changed files before a working tree capture.
#[derive(Clone, Copy, Debug)]
pub struct ReviewCaptureLimits {
    pub file_bytes: u64,
    pub total_bytes: u64,
}

impl ReviewCaptureLimits {
    pub const DEFAULT: Self = Self {
        file_bytes: MAX_REVIEW_FILE_BYTES,
        total_bytes: MAX_REVIEW_CAPTURE_BYTES,
    };
}

/// Changed paths a working tree capture copies, and those it leaves out.
#[derive(Debug, Default, Eq, PartialEq)]
struct CapturePlan {
    /// NUL-separated literal pathspecs relative to the working folder.
    pathspecs: Vec<u8>,
    /// Repository-relative paths of files over the per-file limit.
    too_large_paths: BTreeSet<String>,
}

#[derive(Default)]
struct RawFileMetadata {
    old_mode: String,
    new_mode: String,
}

pub type ReviewNumstat = BTreeMap<String, (Option<u64>, Option<u64>)>;

pub async fn working_tree_material(
    folder: GitFolder<'_>,
    mode: ReviewWorkingTreeMode,
) -> ChatResult<SnapshotMaterial> {
    working_tree_material_with_limits(folder, mode, ReviewCaptureLimits::DEFAULT).await
}

pub async fn working_tree_material_with_limits(
    folder: GitFolder<'_>,
    mode: ReviewWorkingTreeMode,
    limits: ReviewCaptureLimits,
) -> ChatResult<SnapshotMaterial> {
    let before_status = git::status(folder).await?;
    let has_conflicts = before_status.files.iter().any(|file| file.conflicted);
    if has_conflicts && mode != ReviewWorkingTreeMode::All {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Staged and unstaged review scopes are unavailable while the Git index has conflicts. Open all changes or resolve the conflicts.",
            true,
        ));
    }
    let head = optional_head(folder).await?;
    if mode == ReviewWorkingTreeMode::Staged {
        let index_tree = git_text(folder, &["write-tree"], None).await?;
        return Ok(SnapshotMaterial {
            before_oid: match head {
                Some(head) => head,
                None => empty_tree(folder).await?,
            },
            after_oid: index_tree,
            label: "Staged changes".to_string(),
            status: before_status,
            object_store: None,
            too_large_paths: BTreeSet::new(),
        });
    }
    // The capture starts from the real index, so a file left out as too large
    // keeps its staged content and shows no unstaged change.
    let index_tree = git_text(folder, &["write-tree"], None).await?;
    let prefix = git_text(folder, &["rev-parse", "--show-prefix"], None).await?;
    let root = folder.root.to_path_buf();
    let changed = before_status.files.clone();
    let plan = tokio::task::spawn_blocking(move || plan_capture(&root, &prefix, &changed, limits))
        .await
        .map_err(|_| review_error("Review capture size worker stopped"))??;
    let store = Arc::new(ReviewObjectStore::new(folder).await?);
    let empty_tree_oid = if head.is_none() {
        Some(review_storage_text(folder, &["mktree"], Some(&[]), Some(&store)).await?)
    } else {
        None
    };
    review_storage_text(folder, &["read-tree", &index_tree], None, Some(&store)).await?;
    if !plan.pathspecs.is_empty() {
        review_storage_text(
            folder,
            &["add", "-A", "--pathspec-from-file=-", "--pathspec-file-nul"],
            Some(&plan.pathspecs),
            Some(&store),
        )
        .await?;
    }
    let worktree_tree = review_storage_text(folder, &["write-tree"], None, Some(&store)).await?;
    let store_for_size = store.clone();
    tokio::task::spawn_blocking(move || store_for_size.verify_size())
        .await
        .map_err(|_| review_error("Review object size worker stopped"))??;
    let head = head
        .or(empty_tree_oid)
        .ok_or_else(|| review_error("Git HEAD is unavailable"))?;
    Ok(match mode {
        ReviewWorkingTreeMode::Unstaged => SnapshotMaterial {
            before_oid: index_tree,
            after_oid: worktree_tree,
            label: "Unstaged changes".to_string(),
            status: before_status,
            object_store: Some(store),
            too_large_paths: plan.too_large_paths,
        },
        ReviewWorkingTreeMode::All => SnapshotMaterial {
            before_oid: head,
            after_oid: worktree_tree,
            label: "All uncommitted changes".to_string(),
            status: before_status,
            object_store: Some(store),
            too_large_paths: plan.too_large_paths,
        },
        ReviewWorkingTreeMode::Staged => unreachable!("staged review returned above"),
    })
}

/// Sizes the changed files Git status reports before any content is copied.
///
/// Status paths are repository-relative while pathspecs are relative to the
/// working folder, so paths outside the folder's `prefix` are skipped, as
/// `git add -A -- .` would. Files that vanished or are not regular files cost
/// nothing; Git records deletions and links without reading large content.
fn plan_capture(
    root: &Path,
    prefix: &str,
    changed: &[GitChangedPathRead],
    limits: ReviewCaptureLimits,
) -> ChatResult<CapturePlan> {
    let mut plan = CapturePlan::default();
    let mut total = 0_u64;
    for file in changed
        .iter()
        .filter(|file| !file.ignored && (file.untracked || file.worktree_status != "."))
    {
        let Some(relative) = file.relative_path.strip_prefix(prefix) else {
            continue;
        };
        if relative.is_empty() {
            continue;
        }
        if let Ok(metadata) = fs::symlink_metadata(root.join(relative))
            && metadata.is_file()
        {
            if metadata.len() > limits.file_bytes {
                plan.too_large_paths.insert(file.relative_path.clone());
                continue;
            }
            total = total.saturating_add(metadata.len());
            if total > limits.total_bytes {
                return Err(review_storage_limit_error());
            }
        }
        plan.pathspecs.extend_from_slice(relative.as_bytes());
        plan.pathspecs.push(0);
    }
    Ok(plan)
}

pub async fn git_files(
    folder: GitFolder<'_>,
    before: &str,
    after: &str,
    source: &ReviewDiffSource,
    ignore_whitespace: bool,
    object_store: Option<&ReviewObjectStore>,
) -> ChatResult<Vec<ReviewFileInternal>> {
    let mut name_arguments = deterministic_diff_arguments(ignore_whitespace, None);
    name_arguments.extend(["--name-status", "-z", before, after]);
    let names = review_output(folder, &name_arguments, None, object_store).await?;
    let mut stat_arguments = deterministic_diff_arguments(ignore_whitespace, None);
    stat_arguments.extend(["--numstat", "-z", before, after]);
    let stats = review_output(folder, &stat_arguments, None, object_store).await?;
    let stats = parse_numstat(&stats)?;
    let mut raw_arguments = deterministic_diff_arguments(ignore_whitespace, None);
    raw_arguments.extend(["--raw", "--no-abbrev", "-z", before, after]);
    let raw = review_output(folder, &raw_arguments, None, object_store).await?;
    let raw = parse_raw_metadata(&raw);
    let mut files = parse_name_status(&names, &stats, source)?;
    for file in &mut files {
        let Some(metadata) = raw.get(&file.read.relative_path) else {
            continue;
        };
        file.read.flags.submodule = metadata.old_mode == "160000" || metadata.new_mode == "160000";
        file.read.flags.symlink = metadata.old_mode == "120000" || metadata.new_mode == "120000";
        file.read.flags.mode_only = metadata.old_mode != metadata.new_mode
            && file.read.additions == Some(0)
            && file.read.deletions == Some(0)
            && !file.read.flags.pure_rename;
        if file.read.flags.submodule
            || file.read.flags.mode_only
            || file.read.flags.pure_rename
            || file.read.flags.symlink
        {
            file.read.capabilities.comment = false;
            file.read.capability_reasons.comment =
                Some("This change does not expose stable text lines for comments".to_string());
        }
        if file.read.flags.submodule || file.read.flags.symlink {
            file.read.capabilities.open_editor = false;
            file.read.capability_reasons.open_editor =
                Some("This path is not an editable regular workspace file".to_string());
        }
    }
    Ok(files)
}

fn parse_raw_metadata(bytes: &[u8]) -> HashMap<String, RawFileMetadata> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut result = HashMap::new();
    while let Some(field) = fields.next() {
        let Ok(header) = std::str::from_utf8(field) else {
            continue;
        };
        if !header.starts_with(':') {
            continue;
        }
        let parts = header[1..].split_whitespace().collect::<Vec<_>>();
        if parts.len() < 5 {
            continue;
        }
        let is_rename_or_copy = parts[4].starts_with('R') || parts[4].starts_with('C');
        let destination_path = if is_rename_or_copy {
            let _ = fields.next();
            fields.next()
        } else {
            fields.next()
        };
        let Some(destination_path) = destination_path else {
            break;
        };
        let Ok(destination_path) = std::str::from_utf8(destination_path) else {
            continue;
        };
        result.insert(
            destination_path.to_string(),
            RawFileMetadata {
                old_mode: parts[0].to_string(),
                new_mode: parts[1].to_string(),
            },
        );
    }
    result
}

pub fn enrich_working_tree_files(
    files: &mut Vec<ReviewFileInternal>,
    status: &git::GitStatusRead,
    source: &ReviewDiffSource,
) {
    for git_file in &status.files {
        let matching = files.iter().position(|file| {
            file.read.relative_path == git_file.relative_path
                || git_file
                    .original_relative_path
                    .as_deref()
                    .is_some_and(|original| {
                        file.read.previous_relative_path.as_deref() == Some(original)
                    })
        });
        if matching.is_none() && git_file.conflicted {
            let (flags, capabilities, capability_reasons) =
                file_contract(source, "conflicted", None, None, false);
            files.push(ReviewFileInternal {
                read: ReviewFileRead {
                    file_id: String::new(),
                    relative_path: git_file.relative_path.clone(),
                    previous_relative_path: git_file.original_relative_path.clone(),
                    status: "modified".to_string(),
                    additions: None,
                    deletions: None,
                    flags,
                    capabilities,
                    capability_reasons,
                },
            });
        }
        let index = match matching {
            Some(index) => index,
            None if git_file.conflicted => files.len().saturating_sub(1),
            None => continue,
        };
        let Some(file) = files.get_mut(index) else {
            continue;
        };
        file.read.flags.untracked = git_file.untracked;
        file.read.flags.conflict = git_file.conflicted;
        if git_file.conflicted {
            file.read.capabilities.stage = false;
            file.read.capabilities.unstage = false;
            file.read.capabilities.discard = false;
            let reason = "Resolve this conflict in the editor before changing Git state";
            file.read.capability_reasons.stage = Some(reason.to_string());
            file.read.capability_reasons.unstage = Some(reason.to_string());
            file.read.capability_reasons.discard = Some(reason.to_string());
        }
    }
}

/// Lists changed files a capture left out as too large. A file with a staged
/// change already has an entry and is flagged in place; any other one gets an
/// entry without line counts, so every changed file stays visible and can be
/// staged or discarded as a whole.
pub fn add_too_large_files(
    files: &mut Vec<ReviewFileInternal>,
    status: &git::GitStatusRead,
    source: &ReviewDiffSource,
    too_large_paths: &BTreeSet<String>,
) {
    for path in too_large_paths {
        let Some(git_file) = status.files.iter().find(|file| &file.relative_path == path) else {
            continue;
        };
        if let Some(file) = files
            .iter_mut()
            .find(|file| &file.read.relative_path == path)
        {
            mark_too_large(file);
            continue;
        }
        let status_name = if git_file.untracked {
            "added"
        } else {
            "modified"
        };
        let (flags, capabilities, capability_reasons) =
            file_contract(source, status_name, None, None, false);
        let mut file = ReviewFileInternal {
            read: ReviewFileRead {
                file_id: String::new(),
                relative_path: path.clone(),
                previous_relative_path: None,
                status: status_name.to_string(),
                additions: None,
                deletions: None,
                flags,
                capabilities,
                capability_reasons,
            },
        };
        file.read.flags.untracked = git_file.untracked;
        mark_too_large(&mut file);
        files.push(file);
    }
}

fn mark_too_large(file: &mut ReviewFileInternal) {
    file.read.flags.too_large = true;
    file.read.additions = None;
    file.read.deletions = None;
    file.read.capabilities.comment = false;
    file.read.capability_reasons.comment =
        Some("Files too large to review do not support line comments".to_string());
}

pub fn deterministic_diff_arguments(
    ignore_whitespace: bool,
    context_argument: Option<&str>,
) -> Vec<&str> {
    let mut arguments = vec![
        "diff",
        "--binary",
        "--full-index",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        "--diff-algorithm=histogram",
    ];
    if ignore_whitespace {
        arguments.push("--ignore-all-space");
    }
    if let Some(context_argument) = context_argument {
        arguments.push(context_argument);
    }
    arguments
}

pub fn parse_name_status(
    bytes: &[u8],
    stats: &ReviewNumstat,
    source: &ReviewDiffSource,
) -> ChatResult<Vec<ReviewFileInternal>> {
    let mut fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(status_field) = fields.next() {
        let status = std::str::from_utf8(status_field).map_err(|_| corrupt_data_error())?;
        let previous = if status.starts_with('R') || status.starts_with('C') {
            Some(path_field(fields.next())?)
        } else {
            None
        };
        let path = path_field(fields.next())?;
        let (additions, deletions) = stats.get(&path).copied().unwrap_or((None, None));
        let binary = additions.is_none() || deletions.is_none();
        let normalized = match status.chars().next() {
            Some('A') => "added",
            Some('M') => "modified",
            Some('D') => "deleted",
            Some('R') | Some('C') => "renamed",
            Some('T') => "type_changed",
            Some('U') => "modified",
            _ => "unknown",
        };
        let (flags, capabilities, capability_reasons) =
            file_contract(source, normalized, additions, deletions, binary);
        files.push(ReviewFileInternal {
            read: ReviewFileRead {
                file_id: String::new(),
                relative_path: path,
                previous_relative_path: previous,
                status: normalized.to_string(),
                additions,
                deletions,
                flags,
                capabilities,
                capability_reasons,
            },
        });
    }
    Ok(files)
}

fn parse_numstat(bytes: &[u8]) -> ChatResult<ReviewNumstat> {
    let mut values = BTreeMap::new();
    let mut fields = bytes.split(|byte| *byte == 0).peekable();
    while let Some(field) = fields.next() {
        if field.is_empty() {
            continue;
        }
        let text = std::str::from_utf8(field).map_err(|_| corrupt_data_error())?;
        let mut parts = text.splitn(3, '\t');
        let additions = parts.next().and_then(|value| value.parse().ok());
        let deletions = parts.next().and_then(|value| value.parse().ok());
        let path = parts.next().unwrap_or_default();
        let destination_path = if path.is_empty() {
            let _ = fields.next();
            fields
                .next()
                .and_then(|value| std::str::from_utf8(value).ok())
                .unwrap_or_default()
        } else {
            path
        };
        if !destination_path.is_empty() {
            validate_path(destination_path)?;
            values.insert(destination_path.to_string(), (additions, deletions));
        }
    }
    Ok(values)
}

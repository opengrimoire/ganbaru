//! The commit a push request asks Ganbaru to make from the folder's working tree when the
//! person confirms.
//!
//! A teammate's sandbox has neither the person's Git identity nor their signing keys, so a push
//! request may carry a commit message instead of a finished commit. Ganbaru snapshots the working
//! tree into a private index when the teammate asks, records the tree, the author, whether the
//! person's configuration signs, and the changed paths, and at confirmation snapshots again and
//! commits that exact tree on the recorded parent with the person's identity and signing. Every
//! run is a confirmed teammate request (see [`crate::git::hardened::GitOrigin::Requested`]), so no hook,
//! filter, or signing program from the repository's own configuration runs.

use super::super::{
    GitCall, GitFolder, GitOutput, GitStorage, MAX_GIT_OUTPUT_BYTES, run, run_bounded,
    run_bounded_with_storage,
};
use super::{is_object_id, unexpected_output};
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Longest commit message a push request may carry, in characters.
pub const MAX_COMMIT_MESSAGE_CHARS: usize = 5_000;
/// Most changed paths a push request lists; the count covers the rest.
pub const MAX_LISTED_COMMIT_PATHS: usize = 50;
/// Longest listed path or author, in bytes.
pub const MAX_COMMIT_FIELD_BYTES: usize = 1_024;

/// What committing the working tree on a parent would record, read from the folder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeCommit {
    /// The tree the commit records.
    pub tree_id: String,
    /// `Name <email>` of the author and committer the person's configuration names.
    pub author: String,
    /// Whether the person's configuration signs commits; signing never falls back to unsigned.
    pub signed: bool,
    /// The first [`MAX_LISTED_COMMIT_PATHS`] changed paths, with control characters replaced.
    pub changed_paths: Vec<String>,
    /// How many paths change in all.
    pub changed_path_count: u32,
}

impl GitWorktreeCommit {
    /// Whether every field has the shape [`worktree_commit`] reads.
    pub fn is_well_formed(&self) -> bool {
        let text = |value: &str| {
            !value.is_empty()
                && value.len() <= MAX_COMMIT_FIELD_BYTES
                && !value.chars().any(char::is_control)
        };
        is_object_id(&self.tree_id)
            && text(&self.author)
            && self.changed_paths.len() <= MAX_LISTED_COMMIT_PATHS
            && self.changed_paths.iter().all(|path| text(path))
            && usize::try_from(self.changed_path_count)
                .is_ok_and(|count| count >= self.changed_paths.len().max(1))
    }
}

/// Why the working tree cannot be committed.
#[derive(Debug)]
pub enum GitWorktreeCommitError {
    /// The working tree records the parent's tree.
    NothingToCommit,
    /// The person's configuration names no author.
    NoIdentity,
    /// Git failed or printed something unexpected.
    Failed(ChatError),
}

impl From<ChatError> for GitWorktreeCommitError {
    fn from(error: ChatError) -> Self {
        Self::Failed(error)
    }
}

/// Returns `message` without surrounding blank space when it is a commit message a push request
/// may carry: not empty, at most [`MAX_COMMIT_MESSAGE_CHARS`] characters, and without control
/// characters other than line breaks and tabs.
pub fn normalize_commit_message(message: &str) -> Option<&str> {
    let message = message.trim();
    (!message.is_empty()
        && message.chars().count() <= MAX_COMMIT_MESSAGE_CHARS
        && !message
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t')))
    .then_some(message)
}

/// Reads what committing every change under the folder, untracked files that are not ignored
/// included, on `parent` would record. Paths outside the folder keep the parent's content.
///
/// # Errors
///
/// Returns a [`GitWorktreeCommitError`] naming why there is nothing to commit.
pub async fn worktree_commit(
    folder: GitFolder<'_>,
    parent: &str,
) -> Result<GitWorktreeCommit, GitWorktreeCommitError> {
    if !is_object_id(parent) {
        return Err(unexpected_output().into());
    }
    let tree_id = worktree_tree(folder, parent).await?;
    let parent_tree = object_id_of(folder, &format!("{parent}^{{tree}}")).await?;
    if tree_id == parent_tree {
        return Err(GitWorktreeCommitError::NothingToCommit);
    }
    let author = author_identity(folder)
        .await
        .ok_or(GitWorktreeCommitError::NoIdentity)?;
    let signed = signs_commits(folder).await?;
    let (changed_paths, changed_path_count) = changed_paths(folder, &parent_tree, &tree_id).await?;
    Ok(GitWorktreeCommit {
        tree_id,
        author,
        signed,
        changed_paths,
        changed_path_count,
    })
}

/// Commits `tree_id` on `parent` with `message` and the person's identity, signed with the
/// person's signing configuration when `signed`, and returns the new commit id. No ref moves.
/// `commit-tree` ignores `commit.gpgSign`, so signing is always chosen explicitly.
///
/// # Errors
///
/// Returns a conflict carrying Git's output when committing or signing fails; a commit that
/// cannot be signed is never made unsigned.
pub async fn commit_tree(
    folder: GitFolder<'_>,
    parent: &str,
    tree_id: &str,
    message: &str,
    signed: bool,
) -> ChatResult<String> {
    if !is_object_id(parent) || !is_object_id(tree_id) {
        return Err(unexpected_output());
    }
    let mut input = message.as_bytes().to_vec();
    input.push(b'\n');
    let output = run_bounded(
        folder,
        &[
            "commit-tree",
            if signed { "-S" } else { "--no-gpg-sign" },
            "-p",
            parent,
            "-F",
            "-",
            tree_id,
        ],
        GitCall::Requested,
        Some(&input),
        MAX_GIT_OUTPUT_BYTES,
    )
    .await?;
    object_id_text(&output.stdout)
}

/// Moves `branch` from `parent` to `object_id` and resets the index to it, keeping the working
/// tree, so the folder shows the pushed commit with nothing left to commit.
///
/// # Errors
///
/// Returns a conflict carrying Git's output when the branch no longer points at `parent`.
pub async fn advance_branch(
    folder: GitFolder<'_>,
    branch: &str,
    object_id: &str,
    parent: &str,
) -> ChatResult<()> {
    let reference = format!("{}{branch}", super::BRANCH_REF_PREFIX);
    run(
        folder,
        &[
            "update-ref",
            "-m",
            "push request",
            "--",
            &reference,
            object_id,
            parent,
        ],
        GitCall::Requested,
    )
    .await?;
    run(folder, &["reset", "-q", "--mixed"], GitCall::Requested)
        .await
        .map(|_| ())
}

/// The tree a private index holds after loading `parent` and adding every change under the
/// folder.
async fn worktree_tree(folder: GitFolder<'_>, parent: &str) -> ChatResult<String> {
    let index = TemporaryIndex::new()?;
    run_indexed(folder, index.path(), &["read-tree", parent]).await?;
    run_indexed(folder, index.path(), &["add", "-A", "--", "."]).await?;
    let output = run_indexed(folder, index.path(), &["write-tree"]).await?;
    object_id_text(&output.stdout)
}

async fn run_indexed(
    folder: GitFolder<'_>,
    index_file: &Path,
    arguments: &[&str],
) -> ChatResult<GitOutput> {
    run_bounded_with_storage(
        folder,
        arguments,
        GitCall::Requested,
        None,
        MAX_GIT_OUTPUT_BYTES,
        None,
        GitStorage {
            index_file: Some(index_file),
            ..GitStorage::default()
        },
    )
    .await
}

async fn object_id_of(folder: GitFolder<'_>, revision: &str) -> ChatResult<String> {
    let output = run(
        folder,
        &["rev-parse", "--verify", "--end-of-options", revision],
        GitCall::Requested,
    )
    .await?;
    object_id_text(&output.stdout)
}

/// `Name <email>` of the author Git would record, or `None` when it names none.
async fn author_identity(folder: GitFolder<'_>) -> Option<String> {
    let output = run(folder, &["var", "GIT_AUTHOR_IDENT"], GitCall::Requested)
        .await
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut parts = text.trim_end_matches('\n').rsplitn(3, ' ');
    let (_zone, _time, identity) = (parts.next()?, parts.next()?, parts.next()?);
    (!identity.is_empty()
        && identity.len() <= MAX_COMMIT_FIELD_BYTES
        && !identity.chars().any(char::is_control))
    .then(|| identity.to_string())
}

async fn signs_commits(folder: GitFolder<'_>) -> ChatResult<bool> {
    let output = run(
        folder,
        &[
            "config",
            "--type=bool",
            "--default=false",
            "--get",
            "commit.gpgSign",
        ],
        GitCall::Requested,
    )
    .await?;
    match output.stdout.trim_ascii() {
        b"true" => Ok(true),
        b"false" => Ok(false),
        _ => Err(unexpected_output()),
    }
}

/// The first [`MAX_LISTED_COMMIT_PATHS`] paths that differ between two trees, and their count.
async fn changed_paths(
    folder: GitFolder<'_>,
    from_tree: &str,
    to_tree: &str,
) -> ChatResult<(Vec<String>, u32)> {
    let output = run(
        folder,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--no-renames",
            "--name-only",
            from_tree,
            to_tree,
        ],
        GitCall::Requested,
    )
    .await?;
    let paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    let count = u32::try_from(paths.len()).map_err(|_| unexpected_output())?;
    let listed = paths
        .into_iter()
        .take(MAX_LISTED_COMMIT_PATHS)
        .map(listed_path)
        .collect();
    Ok((listed, count))
}

/// A path as text safe to show: control characters replaced and cut to
/// [`MAX_COMMIT_FIELD_BYTES`] on a character boundary.
fn listed_path(raw: &[u8]) -> String {
    let mut path = String::new();
    for character in String::from_utf8_lossy(raw).chars() {
        let character = if character.is_control() {
            char::REPLACEMENT_CHARACTER
        } else {
            character
        };
        if path.len() + character.len_utf8() > MAX_COMMIT_FIELD_BYTES {
            break;
        }
        path.push(character);
    }
    path
}

fn object_id_text(stdout: &[u8]) -> ChatResult<String> {
    let text = std::str::from_utf8(stdout).map_err(|_| unexpected_output())?;
    let object_id = text.trim();
    if is_object_id(object_id) {
        Ok(object_id.to_string())
    } else {
        Err(unexpected_output())
    }
}

/// An index file inside a new private, randomly named directory, removed when dropped.
struct TemporaryIndex {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryIndex {
    fn new() -> ChatResult<Self> {
        let directory = ganbaru_atomic_files::create_private_temporary_directory(
            &std::env::temp_dir(),
            "ganbaru-push-index-",
        )
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Could not create a private Git index",
                true,
            )
        })?;
        let path = directory.join("index");
        Ok(Self { directory, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

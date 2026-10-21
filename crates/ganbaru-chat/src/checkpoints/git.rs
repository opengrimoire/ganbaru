//! Git checkpoint capture, verification, restoration, and process plumbing.

use super::diff::changed_file_summaries;
use super::{
    CapturedCheckpoint, CurrentGitSnapshot, CurrentGitTrees, StoredCheckpoint, checkpoint_error,
};
use crate::git::GitFolder;
use crate::git::hardened::{
    self, GIT_CHECKPOINT_DEADLINE, GitPolicy, GitRun, GitRunError, GitStdout,
};
use crate::models::{ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatThreadId};
use crate::workspace::AuthorizedWorkingFolder;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
/// Fixed identity for checkpoint commits so their object IDs depend only on
/// content.
const CHECKPOINT_IDENTITY_ENVIRONMENT: &[(&str, &str)] = &[
    ("GIT_AUTHOR_NAME", "Ganbaru"),
    ("GIT_AUTHOR_EMAIL", "local@ganbaru.invalid"),
    ("GIT_COMMITTER_NAME", "Ganbaru"),
    ("GIT_COMMITTER_EMAIL", "local@ganbaru.invalid"),
    ("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z"),
];
const REF_PREFIX: &str = "refs/ganbaru/chat/";

/// Verifies that a stored checkpoint belongs to the authorized repository and that every Git
/// object it names is the one capture recorded: the hidden ref resolves to the checkpoint
/// commit, the commit's tree is the worktree tree, and its parent's tree is the index tree. The
/// stored row travels with the vault, so restore and its preview never pass a stored value to
/// Git that this check has not tied to the local checkpoint commit.
pub fn verify_checkpoint(
    authorized: &AuthorizedWorkingFolder,
    checkpoint: &StoredCheckpoint,
) -> ChatResult<()> {
    if authorized.repository_identity.as_deref() != Some(&checkpoint.repository_identity)
        || !is_valid_hidden_ref(&checkpoint.hidden_ref_name)
    {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Checkpoint repository identity no longer matches",
            true,
        ));
    }
    if ![
        checkpoint.git_object_id.as_str(),
        checkpoint.worktree_tree_oid.as_str(),
        checkpoint.index_tree_oid.as_str(),
    ]
    .into_iter()
    .chain(checkpoint.head_oid.as_deref())
    .all(is_object_id)
    {
        return Err(changed_checkpoint_error());
    }
    let folder = authorized.git_folder();
    let resolve =
        |revision: String| optional_git_text(folder, &["rev-parse", "-q", "--verify", &revision]);
    let resolved = resolve(format!("{}^{{commit}}", checkpoint.hidden_ref_name))?;
    if resolved.as_deref() != Some(checkpoint.git_object_id.as_str()) {
        return Err(changed_checkpoint_error());
    }
    let worktree_tree = resolve(format!("{}^{{tree}}", checkpoint.git_object_id))?;
    let index_tree = resolve(format!("{}^1^{{tree}}", checkpoint.git_object_id))?;
    if worktree_tree.as_deref() != Some(checkpoint.worktree_tree_oid.as_str())
        || index_tree.as_deref() != Some(checkpoint.index_tree_oid.as_str())
    {
        return Err(changed_checkpoint_error());
    }
    Ok(())
}

fn changed_checkpoint_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Checkpoint Git ref is missing or changed",
        true,
    )
}

/// Whether `value` is a full, non-null hexadecimal Git object ID (SHA-1 or SHA-256).
fn is_object_id(value: &str) -> bool {
    (40..=64).contains(&value.len())
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !value.bytes().all(|byte| byte == b'0')
}

pub fn delete_exact_ref(
    folder: GitFolder<'_>,
    reference: &str,
    expected_oid: &str,
) -> ChatResult<()> {
    if !is_valid_hidden_ref(reference) || !is_object_id(expected_oid) {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Checkpoint cleanup target is invalid",
            false,
        ));
    }
    let Some(actual_oid) = optional_git_text(folder, &["rev-parse", "-q", "--verify", reference])?
    else {
        return Ok(());
    };
    if actual_oid != expected_oid {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Checkpoint ref changed before cleanup",
            true,
        ));
    }
    git_output(
        folder,
        &["update-ref", "-d", reference, expected_oid],
        None,
        None,
    )
    .map(|_| ())
}

pub fn current_git_snapshot(folder: GitFolder<'_>) -> ChatResult<CurrentGitSnapshot> {
    let CurrentGitTrees {
        head_oid,
        index_tree_oid,
        worktree_tree_oid,
    } = current_git_trees(folder)?;
    let head_ref = optional_git_text(folder, &["symbolic-ref", "-q", "HEAD"])?;
    let index_fingerprint =
        hash_bytes(&git_output(folder, &["ls-files", "--stage", "-z"], None, None)?.stdout);
    let index_commit_oid =
        git_commit_tree(folder, &index_tree_oid, None, "Ganbaru Chat preview index")?;
    let worktree_commit_oid = git_commit_tree(
        folder,
        &worktree_tree_oid,
        Some(&index_commit_oid),
        "Ganbaru Chat restore preview",
    )?;
    Ok(CurrentGitSnapshot {
        worktree_commit_oid,
        worktree_tree_oid,
        index_tree_oid,
        index_fingerprint,
        head_oid,
        head_ref,
    })
}

/// Captures the current HEAD, index, and worktree trees without creating
/// synthetic commit objects.
pub fn current_git_trees(folder: GitFolder<'_>) -> ChatResult<CurrentGitTrees> {
    verify_git_root(folder)?;
    let head_oid = optional_git_text(folder, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])?;
    let index_tree_oid = git_text(folder, &["write-tree"], None)?;
    let worktree_tree_oid = capture_worktree_tree(folder, head_oid.as_deref())?;
    Ok(CurrentGitTrees {
        head_oid,
        index_tree_oid,
        worktree_tree_oid,
    })
}

fn capture_worktree_tree(folder: GitFolder<'_>, head_oid: Option<&str>) -> ChatResult<String> {
    let temporary_index = TemporaryIndex::new()?;
    if let Some(head_oid) = head_oid {
        git_output(
            folder,
            &["read-tree", head_oid],
            Some(temporary_index.path()),
            None,
        )?;
    } else {
        git_output(
            folder,
            &["read-tree", "--empty"],
            Some(temporary_index.path()),
            None,
        )?;
    }
    git_output(
        folder,
        &["add", "-A", "--", "."],
        Some(temporary_index.path()),
        None,
    )?;
    git_text(folder, &["write-tree"], Some(temporary_index.path()))
}

pub fn restore_git_snapshot(
    folder: GitFolder<'_>,
    current: &CurrentGitSnapshot,
    target: &StoredCheckpoint,
) -> ChatResult<()> {
    verify_git_root(folder)?;
    let before = current_git_snapshot(folder)?;
    if &before != current {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Workspace changed after the restore preview",
            true,
        ));
    }
    let restore_index = TemporaryIndex::new()?;
    git_output(
        folder,
        &["read-tree", &current.worktree_tree_oid],
        Some(restore_index.path()),
        None,
    )?;
    let worktree_result = git_output(
        folder,
        &["read-tree", "--reset", "-u", &target.worktree_tree_oid],
        Some(restore_index.path()),
        None,
    );
    if worktree_result.is_err() {
        return Err(checkpoint_error("Workspace checkpoint restore failed"));
    }
    if let Err(error) = git_output(
        folder,
        &["read-tree", "--reset", &target.index_tree_oid],
        None,
        None,
    ) {
        let _ = recover_git_snapshot(folder, current);
        return Err(error);
    }
    let restored = current_git_snapshot(folder)?;
    if restored.worktree_tree_oid != target.worktree_tree_oid
        || restored.index_tree_oid != target.index_tree_oid
        || restored.head_oid != current.head_oid
        || restored.head_ref != current.head_ref
    {
        let _ = recover_git_snapshot(folder, current);
        return Err(checkpoint_error(
            "Checkpoint restore verification failed and recovery was attempted",
        ));
    }
    Ok(())
}

fn recover_git_snapshot(folder: GitFolder<'_>, snapshot: &CurrentGitSnapshot) -> ChatResult<()> {
    let recovery_index = TemporaryIndex::new()?;
    if let Some(head_oid) = snapshot.head_oid.as_deref() {
        git_output(
            folder,
            &["read-tree", head_oid],
            Some(recovery_index.path()),
            None,
        )?;
    } else {
        git_output(
            folder,
            &["read-tree", "--empty"],
            Some(recovery_index.path()),
            None,
        )?;
    }
    git_output(
        folder,
        &["add", "-A", "--", "."],
        Some(recovery_index.path()),
        None,
    )?;
    git_output(
        folder,
        &["read-tree", "--reset", "-u", &snapshot.worktree_tree_oid],
        Some(recovery_index.path()),
        None,
    )?;
    git_output(
        folder,
        &["read-tree", "--reset", &snapshot.index_tree_oid],
        None,
        None,
    )?;
    Ok(())
}

pub(super) fn capture_git(
    folder: GitFolder<'_>,
    thread_id: &ChatThreadId,
    checkpoint_id: &ChatCheckpointId,
    previous_oid: Option<&str>,
) -> ChatResult<CapturedCheckpoint> {
    verify_git_root(folder)?;
    let before = repository_fingerprint(folder)?;
    let head_ref = optional_git_text(folder, &["symbolic-ref", "-q", "HEAD"])?;
    let head_oid = optional_git_text(folder, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])?;
    let index_tree_oid = git_text(folder, &["write-tree"], None)?;
    let index_fingerprint =
        hash_bytes(&git_output(folder, &["ls-files", "--stage", "-z"], None, None)?.stdout);
    let temporary_index = TemporaryIndex::new()?;
    if let Some(head_oid) = head_oid.as_deref() {
        git_output(
            folder,
            &["read-tree", head_oid],
            Some(temporary_index.path()),
            None,
        )?;
    } else {
        git_output(
            folder,
            &["read-tree", "--empty"],
            Some(temporary_index.path()),
            None,
        )?;
    }
    git_output(
        folder,
        &["add", "-A", "--", "."],
        Some(temporary_index.path()),
        None,
    )?;
    let worktree_tree_oid = git_text(folder, &["write-tree"], Some(temporary_index.path()))?;
    let index_commit_oid = git_commit_tree(
        folder,
        &index_tree_oid,
        None,
        "Ganbaru Chat index checkpoint",
    )?;
    let git_object_id = git_commit_tree(
        folder,
        &worktree_tree_oid,
        Some(&index_commit_oid),
        "Ganbaru Chat worktree checkpoint",
    )?;
    let hidden_ref_name = checkpoint_ref(thread_id, checkpoint_id);
    git_output(
        folder,
        &["update-ref", &hidden_ref_name, &git_object_id],
        None,
        None,
    )?;
    let after = repository_fingerprint(folder)?;
    if before != after {
        let _ = delete_exact_ref(folder, &hidden_ref_name, &git_object_id);
        return Err(checkpoint_error(
            "Git checkpoint capture changed the branch, index, or worktree",
        ));
    }
    let changed_files = previous_oid
        .map(|previous| changed_file_summaries(folder, previous, &git_object_id))
        .transpose()?
        .unwrap_or_default();
    Ok(CapturedCheckpoint {
        id: checkpoint_id.clone(),
        hidden_ref_name,
        git_object_id,
        index_commit_oid,
        index_tree_oid,
        worktree_tree_oid,
        head_oid,
        head_ref,
        index_fingerprint,
        changed_files,
    })
}

pub(super) fn repository_fingerprint(folder: GitFolder<'_>) -> ChatResult<String> {
    let mut hasher = Sha256::new();
    for arguments in [
        vec!["symbolic-ref", "-q", "HEAD"],
        vec!["rev-parse", "-q", "--verify", "HEAD^{commit}"],
        vec!["ls-files", "--stage", "-z"],
        vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    ] {
        let output = git_output(folder, &arguments, None, Some(&[0, 1]))?;
        hasher.update(&output.stdout);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_git_root(folder: GitFolder<'_>) -> ChatResult<()> {
    let toplevel = git_text(
        folder,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        None,
    )?;
    let canonical = fs::canonicalize(toplevel).map_err(checkpoint_error)?;
    if canonical != folder.root {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Chat checkpoints require the bound folder to be the Git worktree folder",
            true,
        ));
    }
    Ok(())
}

fn git_commit_tree(
    folder: GitFolder<'_>,
    tree_oid: &str,
    parent_oid: Option<&str>,
    message: &str,
) -> ChatResult<String> {
    let mut arguments = vec!["commit-tree", tree_oid];
    if let Some(parent) = parent_oid {
        arguments.extend(["-p", parent]);
    }
    git_text_with_input(folder, &arguments, format!("{message}\n").as_bytes())
}

fn git_text(folder: GitFolder<'_>, arguments: &[&str], index: Option<&Path>) -> ChatResult<String> {
    let output = git_output(folder, arguments, index, None)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(checkpoint_error)
}

fn git_text_with_input(
    folder: GitFolder<'_>,
    arguments: &[&str],
    input: &[u8],
) -> ChatResult<String> {
    let output = git_output_with_input(folder, arguments, None, input)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(checkpoint_error)
}

fn optional_git_text(folder: GitFolder<'_>, arguments: &[&str]) -> ChatResult<Option<String>> {
    let output = git_output(folder, arguments, None, Some(&[0, 1]))?;
    if output.status.success() {
        Ok(Some(
            String::from_utf8(output.stdout)
                .map_err(checkpoint_error)?
                .trim()
                .to_string(),
        ))
    } else {
        Ok(None)
    }
}

pub(super) fn git_output(
    folder: GitFolder<'_>,
    arguments: &[&str],
    index: Option<&Path>,
    accepted_codes: Option<&[i32]>,
) -> ChatResult<Output> {
    git_output_with_input_codes(folder, arguments, index, None, accepted_codes)
}

fn git_output_with_input(
    folder: GitFolder<'_>,
    arguments: &[&str],
    index: Option<&Path>,
    input: &[u8],
) -> ChatResult<Output> {
    git_output_with_input_codes(folder, arguments, index, Some(input), None)
}

fn git_output_with_input_codes(
    folder: GitFolder<'_>,
    arguments: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    accepted_codes: Option<&[i32]>,
) -> ChatResult<Output> {
    let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
    let mut environment = CHECKPOINT_IDENTITY_ENVIRONMENT
        .iter()
        .map(|(name, value)| (*name, OsStr::new(value)))
        .collect::<Vec<_>>();
    if let Some(index) = index {
        environment.push(("GIT_INDEX_FILE", index.as_os_str()));
    }
    let output = hardened::run_blocking(GitRun {
        root: folder.root,
        arguments: &arguments,
        policy: GitPolicy::internal(folder.trust),
        deadline: GIT_CHECKPOINT_DEADLINE,
        input,
        stdout: GitStdout::Memory(MAX_GIT_OUTPUT_BYTES),
        environment: &environment,
        cancel: None,
    })
    .map_err(|error| match error {
        GitRunError::TimedOut | GitRunError::Cancelled => ChatError::from(error),
        GitRunError::Missing
        | GitRunError::Unavailable
        | GitRunError::Configuration
        | GitRunError::OutputTooLarge => checkpoint_error(error),
    })?;
    let code = output.status.code().unwrap_or(-1);
    if !output.status.success() && !accepted_codes.is_some_and(|codes| codes.contains(&code)) {
        return Err(checkpoint_error("Git checkpoint command failed"));
    }
    Ok(Output {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// Index file inside a new private, randomly named directory, so another local
/// user cannot guess, pre-create, or read it in the shared temporary directory.
pub(super) struct TemporaryIndex {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryIndex {
    pub(super) fn new() -> ChatResult<Self> {
        let directory = ganbaru_atomic_files::create_private_temporary_directory(
            &std::env::temp_dir(),
            "ganbaru-chat-index-",
        )
        .map_err(checkpoint_error)?;
        let path = directory.join("index");
        Ok(Self { directory, path })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn checkpoint_ref(thread_id: &ChatThreadId, checkpoint_id: &ChatCheckpointId) -> String {
    format!(
        "{REF_PREFIX}{}/{}",
        short_hash(thread_id.as_str()),
        short_hash(checkpoint_id.as_str())
    )
}

fn short_hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))[..32].to_string()
}

fn is_valid_hidden_ref(reference: &str) -> bool {
    reference.starts_with(REF_PREFIX)
        && !reference[REF_PREFIX.len()..].is_empty()
        && !reference.chars().any(char::is_whitespace)
        && !reference.contains("..")
        && !reference.contains("@{")
        && !reference.ends_with('/')
}

fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

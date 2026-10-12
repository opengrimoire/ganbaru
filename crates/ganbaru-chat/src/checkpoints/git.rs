//! Git checkpoint capture, verification, restoration, and process plumbing.

use super::diff::changed_file_summaries;
use super::{
    CapturedCheckpoint, CurrentGitSnapshot, CurrentGitTrees, StoredCheckpoint, checkpoint_error,
};
use crate::models::{ChatCheckpointId, ChatError, ChatErrorCode, ChatResult, ChatThreadId};
use crate::workspace::AuthorizedWorkingFolder;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const REF_PREFIX: &str = "refs/ganbaru-ai/chat/";
static TEMPORARY_INDEX_SEQUENCE: AtomicU64 = AtomicU64::new(1);

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
    let resolved = optional_git_text(
        &authorized.canonical_path,
        &[
            "rev-parse",
            "-q",
            "--verify",
            &format!("{}^{{commit}}", checkpoint.hidden_ref_name),
        ],
    )?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::NotFound,
            "Checkpoint Git ref is missing or changed",
            true,
        )
    })?;
    if resolved != checkpoint.git_object_id {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Checkpoint Git ref is missing or changed",
            true,
        ));
    }
    Ok(())
}

pub fn delete_exact_ref(root: &Path, reference: &str, expected_oid: &str) -> ChatResult<()> {
    if !is_valid_hidden_ref(reference)
        || !(40..=64).contains(&expected_oid.len())
        || !expected_oid.bytes().all(|byte| byte.is_ascii_hexdigit())
        || expected_oid.bytes().all(|byte| byte == b'0')
    {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Checkpoint cleanup target is invalid",
            false,
        ));
    }
    let Some(actual_oid) = optional_git_text(root, &["rev-parse", "-q", "--verify", reference])?
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
        root,
        &["update-ref", "-d", reference, expected_oid],
        None,
        None,
    )
    .map(|_| ())
}

pub fn current_git_snapshot(root: &Path) -> ChatResult<CurrentGitSnapshot> {
    let CurrentGitTrees {
        head_oid,
        index_tree_oid,
        worktree_tree_oid,
    } = current_git_trees(root)?;
    let head_ref = optional_git_text(root, &["symbolic-ref", "-q", "HEAD"])?;
    let index_fingerprint =
        hash_bytes(&git_output(root, &["ls-files", "--stage", "-z"], None, None)?.stdout);
    let index_commit_oid =
        git_commit_tree(root, &index_tree_oid, None, "Ganbaru Chat preview index")?;
    let worktree_commit_oid = git_commit_tree(
        root,
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
pub fn current_git_trees(root: &Path) -> ChatResult<CurrentGitTrees> {
    verify_git_root(root)?;
    let head_oid = optional_git_text(root, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])?;
    let index_tree_oid = git_text(root, &["write-tree"], None)?;
    let worktree_tree_oid = capture_worktree_tree(root, head_oid.as_deref())?;
    Ok(CurrentGitTrees {
        head_oid,
        index_tree_oid,
        worktree_tree_oid,
    })
}

fn capture_worktree_tree(root: &Path, head_oid: Option<&str>) -> ChatResult<String> {
    let temporary_index = TemporaryIndex::new()?;
    if let Some(head_oid) = head_oid {
        git_output(
            root,
            &["read-tree", head_oid],
            Some(temporary_index.path()),
            None,
        )?;
    } else {
        git_output(
            root,
            &["read-tree", "--empty"],
            Some(temporary_index.path()),
            None,
        )?;
    }
    git_output(
        root,
        &["add", "-A", "--", "."],
        Some(temporary_index.path()),
        None,
    )?;
    git_text(root, &["write-tree"], Some(temporary_index.path()))
}

pub fn restore_git_snapshot(
    root: &Path,
    current: &CurrentGitSnapshot,
    target: &StoredCheckpoint,
) -> ChatResult<()> {
    verify_git_root(root)?;
    let before = current_git_snapshot(root)?;
    if &before != current {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Workspace changed after the restore preview",
            true,
        ));
    }
    let restore_index = TemporaryIndex::new()?;
    git_output(
        root,
        &["read-tree", &current.worktree_tree_oid],
        Some(restore_index.path()),
        None,
    )?;
    let worktree_result = git_output(
        root,
        &["read-tree", "--reset", "-u", &target.worktree_tree_oid],
        Some(restore_index.path()),
        None,
    );
    if worktree_result.is_err() {
        return Err(checkpoint_error("Workspace checkpoint restore failed"));
    }
    if let Err(error) = git_output(
        root,
        &["read-tree", "--reset", &target.index_tree_oid],
        None,
        None,
    ) {
        let _ = recover_git_snapshot(root, current);
        return Err(error);
    }
    let restored = current_git_snapshot(root)?;
    if restored.worktree_tree_oid != target.worktree_tree_oid
        || restored.index_tree_oid != target.index_tree_oid
        || restored.head_oid != current.head_oid
        || restored.head_ref != current.head_ref
    {
        let _ = recover_git_snapshot(root, current);
        return Err(checkpoint_error(
            "Checkpoint restore verification failed and recovery was attempted",
        ));
    }
    Ok(())
}

fn recover_git_snapshot(root: &Path, snapshot: &CurrentGitSnapshot) -> ChatResult<()> {
    let recovery_index = TemporaryIndex::new()?;
    if let Some(head_oid) = snapshot.head_oid.as_deref() {
        git_output(
            root,
            &["read-tree", head_oid],
            Some(recovery_index.path()),
            None,
        )?;
    } else {
        git_output(
            root,
            &["read-tree", "--empty"],
            Some(recovery_index.path()),
            None,
        )?;
    }
    git_output(
        root,
        &["add", "-A", "--", "."],
        Some(recovery_index.path()),
        None,
    )?;
    git_output(
        root,
        &["read-tree", "--reset", "-u", &snapshot.worktree_tree_oid],
        Some(recovery_index.path()),
        None,
    )?;
    git_output(
        root,
        &["read-tree", "--reset", &snapshot.index_tree_oid],
        None,
        None,
    )?;
    Ok(())
}

pub(super) fn capture_git(
    root: &Path,
    thread_id: &ChatThreadId,
    checkpoint_id: &ChatCheckpointId,
    previous_oid: Option<&str>,
) -> ChatResult<CapturedCheckpoint> {
    verify_git_root(root)?;
    let before = repository_fingerprint(root)?;
    let head_ref = optional_git_text(root, &["symbolic-ref", "-q", "HEAD"])?;
    let head_oid = optional_git_text(root, &["rev-parse", "-q", "--verify", "HEAD^{commit}"])?;
    let index_tree_oid = git_text(root, &["write-tree"], None)?;
    let index_fingerprint =
        hash_bytes(&git_output(root, &["ls-files", "--stage", "-z"], None, None)?.stdout);
    let temporary_index = TemporaryIndex::new()?;
    if let Some(head_oid) = head_oid.as_deref() {
        git_output(
            root,
            &["read-tree", head_oid],
            Some(temporary_index.path()),
            None,
        )?;
    } else {
        git_output(
            root,
            &["read-tree", "--empty"],
            Some(temporary_index.path()),
            None,
        )?;
    }
    git_output(
        root,
        &["add", "-A", "--", "."],
        Some(temporary_index.path()),
        None,
    )?;
    let worktree_tree_oid = git_text(root, &["write-tree"], Some(temporary_index.path()))?;
    let index_commit_oid =
        git_commit_tree(root, &index_tree_oid, None, "Ganbaru Chat index checkpoint")?;
    let git_object_id = git_commit_tree(
        root,
        &worktree_tree_oid,
        Some(&index_commit_oid),
        "Ganbaru Chat worktree checkpoint",
    )?;
    let hidden_ref_name = checkpoint_ref(thread_id, checkpoint_id);
    git_output(
        root,
        &["update-ref", &hidden_ref_name, &git_object_id],
        None,
        None,
    )?;
    let after = repository_fingerprint(root)?;
    if before != after {
        let _ = delete_exact_ref(root, &hidden_ref_name, &git_object_id);
        return Err(checkpoint_error(
            "Git checkpoint capture changed the branch, index, or worktree",
        ));
    }
    let changed_files = previous_oid
        .map(|previous| changed_file_summaries(root, previous, &git_object_id))
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

pub(super) fn repository_fingerprint(root: &Path) -> ChatResult<String> {
    let mut hasher = Sha256::new();
    for arguments in [
        vec!["symbolic-ref", "-q", "HEAD"],
        vec!["rev-parse", "-q", "--verify", "HEAD^{commit}"],
        vec!["ls-files", "--stage", "-z"],
        vec!["status", "--porcelain=v2", "-z", "--untracked-files=all"],
    ] {
        let output = git_output(root, &arguments, None, Some(&[0, 1]))?;
        hasher.update(&output.stdout);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify_git_root(root: &Path) -> ChatResult<()> {
    let toplevel = git_text(
        root,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        None,
    )?;
    let canonical = fs::canonicalize(toplevel).map_err(checkpoint_error)?;
    if canonical != root {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Chat checkpoints require the bound folder to be the Git worktree root",
            true,
        ));
    }
    Ok(())
}

fn git_commit_tree(
    root: &Path,
    tree_oid: &str,
    parent_oid: Option<&str>,
    message: &str,
) -> ChatResult<String> {
    let mut arguments = vec!["commit-tree", tree_oid];
    if let Some(parent) = parent_oid {
        arguments.extend(["-p", parent]);
    }
    git_text_with_input(root, &arguments, format!("{message}\n").as_bytes())
}

fn git_text(root: &Path, arguments: &[&str], index: Option<&Path>) -> ChatResult<String> {
    let output = git_output(root, arguments, index, None)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(checkpoint_error)
}

fn git_text_with_input(root: &Path, arguments: &[&str], input: &[u8]) -> ChatResult<String> {
    let output = git_output_with_input(root, arguments, None, input)?;
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(checkpoint_error)
}

fn optional_git_text(root: &Path, arguments: &[&str]) -> ChatResult<Option<String>> {
    let output = git_output(root, arguments, None, Some(&[0, 1]))?;
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
    root: &Path,
    arguments: &[&str],
    index: Option<&Path>,
    accepted_codes: Option<&[i32]>,
) -> ChatResult<Output> {
    git_output_with_input_codes(root, arguments, index, None, accepted_codes)
}

fn git_output_with_input(
    root: &Path,
    arguments: &[&str],
    index: Option<&Path>,
    input: &[u8],
) -> ChatResult<Output> {
    git_output_with_input_codes(root, arguments, index, Some(input), None)
}

fn git_output_with_input_codes(
    root: &Path,
    arguments: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    accepted_codes: Option<&[i32]>,
) -> ChatResult<Output> {
    let mut command = Command::new("git");
    command
        .args(["-C"])
        .arg(root)
        .args(arguments)
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .env("GIT_AUTHOR_NAME", "Ganbaru AI")
        .env("GIT_AUTHOR_EMAIL", "local@ganbaru.invalid")
        .env("GIT_COMMITTER_NAME", "Ganbaru AI")
        .env("GIT_COMMITTER_EMAIL", "local@ganbaru.invalid")
        .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn().map_err(checkpoint_error)?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(input).map_err(checkpoint_error)?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| checkpoint_error("Git stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| checkpoint_error("Git stderr unavailable"))?;
    let stdout_reader = bounded_reader(stdout);
    let stderr_reader = bounded_reader(stderr);
    let status = child.wait().map_err(checkpoint_error)?;
    let stdout = stdout_reader
        .join()
        .map_err(checkpoint_error)?
        .map_err(checkpoint_error)?;
    let stderr = stderr_reader
        .join()
        .map_err(checkpoint_error)?
        .map_err(checkpoint_error)?;
    if stdout.len() > MAX_GIT_OUTPUT_BYTES || stderr.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(checkpoint_error("Git output exceeds the safety limit"));
    }
    let code = status.code().unwrap_or(-1);
    if !status.success() && !accepted_codes.is_some_and(|codes| codes.contains(&code)) {
        return Err(checkpoint_error("Git checkpoint command failed"));
    }
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn bounded_reader<R: std::io::Read + Send + 'static>(
    reader: R,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        reader
            .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}
struct TemporaryIndex {
    path: PathBuf,
}

impl TemporaryIndex {
    fn new() -> ChatResult<Self> {
        let sequence = TEMPORARY_INDEX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-index-{}-{sequence}",
            std::process::id()
        ));
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(checkpoint_error)?;
        drop(file);
        fs::remove_file(&path).map_err(checkpoint_error)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryIndex {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let lock_path = self.path.with_extension("lock");
        let _ = fs::remove_file(lock_path);
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

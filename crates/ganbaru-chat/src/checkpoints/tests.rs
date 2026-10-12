use super::diff::file_diff;
use super::git::{capture_git, repository_fingerprint};
use super::{
    CapturedCheckpoint, StoredCheckpoint, current_git_snapshot, delete_exact_ref, diff_files,
    restore_git_snapshot, verify_checkpoint,
};
use crate::models::{
    ChatCheckpointId, ChatErrorCode, ChatThreadId, ProjectWorkingFolderId, RepositoryKind,
};
use crate::workspace::AuthorizedWorkingFolder;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestRepository(PathBuf);

impl TestRepository {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-checkpoint-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test repository should be created");
        run_git(&path, &["init", "-q"]);
        run_git(&path, &["config", "user.name", "Ganbaru Test"]);
        run_git(&path, &["config", "user.email", "test@ganbaru.invalid"]);
        Self(fs::canonicalize(path).expect("test repository path should canonicalize"))
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("test parent should be created");
        }
        fs::write(path, bytes).expect("test file should be written");
    }

    fn commit_all(&self) {
        run_git(&self.0, &["add", "-A", "--", "."]);
        run_git(&self.0, &["commit", "-q", "-m", "fixture"]);
    }

    fn authorized(&self, identity: &str) -> AuthorizedWorkingFolder {
        AuthorizedWorkingFolder {
            working_folder_id: ProjectWorkingFolderId::new("workspace:test")
                .expect("workspace ID should be valid"),
            canonical_path: self.0.clone(),
            repository_kind: RepositoryKind::Git,
            repository_identity: Some(identity.to_string()),
            repository_storage_identity: Some(identity.to_string()),
        }
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("Git fixture command should start");
    assert!(
        output.status.success(),
        "Git fixture command failed: {arguments:?}"
    );
    String::from_utf8(output.stdout)
        .expect("Git fixture output should be UTF-8")
        .trim()
        .to_string()
}

fn capture(
    repository: &TestRepository,
    suffix: &str,
    previous: Option<&str>,
) -> CapturedCheckpoint {
    let thread = ChatThreadId::new("thread:test").expect("thread ID should be valid");
    let checkpoint = ChatCheckpointId::new(format!("checkpoint:{suffix}"))
        .expect("checkpoint ID should be valid");
    capture_git(&repository.0, &thread, &checkpoint, previous)
        .expect("checkpoint capture should succeed")
}

fn stored(captured: &CapturedCheckpoint, identity: &str, turn_count: u64) -> StoredCheckpoint {
    StoredCheckpoint {
        id: captured.id.clone(),
        thread_id: ChatThreadId::new("thread:test").expect("thread ID should be valid"),
        turn_count,
        repository_identity: identity.to_string(),
        hidden_ref_name: captured.hidden_ref_name.clone(),
        git_object_id: captured.git_object_id.clone(),
        index_tree_oid: captured.index_tree_oid.clone(),
        worktree_tree_oid: captured.worktree_tree_oid.clone(),
        head_oid: captured.head_oid.clone(),
        head_ref: captured.head_ref.clone(),
    }
}

#[test]
fn checkpoint_capture_preserves_head_index_staging_and_worktree() {
    let repository = TestRepository::new();
    repository.write("tracked.txt", b"base\n");
    repository.commit_all();
    repository.write("tracked.txt", b"staged\n");
    run_git(&repository.0, &["add", "tracked.txt"]);
    repository.write("unstaged.txt", b"untracked\n");
    let before = repository_fingerprint(&repository.0).expect("fingerprint should succeed");
    let head = run_git(&repository.0, &["rev-parse", "HEAD"]);
    let captured = capture(&repository, "preserve", None);
    let after = repository_fingerprint(&repository.0).expect("fingerprint should succeed");

    assert_eq!(before, after);
    assert_eq!(head, run_git(&repository.0, &["rev-parse", "HEAD"]));
    assert_eq!(
        captured.git_object_id,
        run_git(&repository.0, &["rev-parse", &captured.hidden_ref_name])
    );
    assert_eq!(
        run_git(&repository.0, &["diff", "--cached", "--name-only"]),
        "tracked.txt"
    );
    assert!(repository.0.join("unstaged.txt").exists());
}

#[cfg(unix)]
#[test]
fn diff_covers_staged_unstaged_untracked_deleted_renamed_mode_binary_and_whitespace() {
    use std::os::unix::fs::PermissionsExt;

    let repository = TestRepository::new();
    for name in [
        "staged.txt",
        "unstaged.txt",
        "deleted.txt",
        "renamed.txt",
        "mode.sh",
        "whitespace.txt",
    ] {
        repository.write(name, format!("{name} base\n").as_bytes());
    }
    repository.write("binary.bin", &[0, 1, 2, 3]);
    repository.commit_all();
    let pre = capture(&repository, "pre", None);

    repository.write("staged.txt", b"staged change\n");
    run_git(&repository.0, &["add", "staged.txt"]);
    repository.write("unstaged.txt", b"unstaged change\n");
    repository.write("untracked.txt", b"new\n");
    fs::remove_file(repository.0.join("deleted.txt")).expect("file should be deleted");
    run_git(&repository.0, &["mv", "renamed.txt", "moved.txt"]);
    let mut permissions = fs::metadata(repository.0.join("mode.sh"))
        .expect("mode fixture should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(repository.0.join("mode.sh"), permissions).expect("mode should change");
    repository.write("binary.bin", &[0, 9, 8, 7]);
    repository.write("whitespace.txt", b"whitespace.txt base   \n");

    let before = repository_fingerprint(&repository.0).expect("fingerprint should succeed");
    let post = capture(&repository, "post", Some(&pre.git_object_id));
    assert_eq!(
        before,
        repository_fingerprint(&repository.0).expect("fingerprint should succeed")
    );

    let files = diff_files(
        &repository.0,
        &stored(&pre, "identity", 0),
        &stored(&post, "identity", 1),
    )
    .expect("diff should succeed");
    let paths = files
        .iter()
        .map(|file| file.relative_path.as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "binary.bin",
        "deleted.txt",
        "mode.sh",
        "moved.txt",
        "staged.txt",
        "unstaged.txt",
        "untracked.txt",
        "whitespace.txt",
    ] {
        assert!(paths.contains(expected), "missing {expected}");
    }
    let renamed = files
        .iter()
        .find(|file| file.relative_path == "moved.txt")
        .expect("rename should exist");
    assert_eq!(renamed.status, "renamed");
    assert_eq!(
        renamed.previous_relative_path.as_deref(),
        Some("renamed.txt")
    );
    assert!(
        files
            .iter()
            .find(|file| file.relative_path == "binary.bin")
            .expect("binary should exist")
            .binary
    );

    let normal = file_diff(
        &repository.0,
        &stored(&pre, "identity", 0),
        &stored(&post, "identity", 1),
        "whitespace.txt",
        false,
    )
    .expect("normal diff should succeed");
    let ignored = file_diff(
        &repository.0,
        &stored(&pre, "identity", 0),
        &stored(&post, "identity", 1),
        "whitespace.txt",
        true,
    )
    .expect("whitespace diff should succeed");
    assert!(
        normal
            .patch
            .as_deref()
            .is_some_and(|patch| !patch.is_empty())
    );
    assert_eq!(ignored.patch.as_deref(), Some(""));
}

#[test]
fn restore_reinstates_worktree_and_real_index_without_moving_head() {
    let repository = TestRepository::new();
    repository.write("staged.txt", b"base\n");
    repository.write("worktree.txt", b"base\n");
    repository.commit_all();
    repository.write("staged.txt", b"checkpoint staged\n");
    run_git(&repository.0, &["add", "staged.txt"]);
    repository.write("worktree.txt", b"checkpoint worktree\n");
    repository.write("checkpoint-only.txt", b"checkpoint\n");
    let target = capture(&repository, "restore-target", None);

    repository.write("staged.txt", b"later staged\n");
    run_git(&repository.0, &["add", "staged.txt"]);
    repository.write("worktree.txt", b"later worktree\n");
    fs::remove_file(repository.0.join("checkpoint-only.txt")).expect("file should be removed");
    repository.write("later-only.txt", b"later\n");
    let current = current_git_snapshot(&repository.0).expect("current snapshot should succeed");
    let head = current.head_oid.clone();

    restore_git_snapshot(&repository.0, &current, &stored(&target, "identity", 0))
        .expect("restore should succeed");
    let restored = current_git_snapshot(&repository.0).expect("restored snapshot should succeed");
    assert_eq!(restored.worktree_tree_oid, target.worktree_tree_oid);
    assert_eq!(restored.index_tree_oid, target.index_tree_oid);
    assert_eq!(restored.head_oid, head);
    assert_eq!(
        fs::read(repository.0.join("worktree.txt")).expect("file should read"),
        b"checkpoint worktree\n"
    );
    assert!(repository.0.join("checkpoint-only.txt").exists());
    assert!(!repository.0.join("later-only.txt").exists());
}

#[test]
fn ref_validation_rejects_identity_changes_missing_refs_and_wrong_cleanup_oids() {
    let repository = TestRepository::new();
    repository.write("tracked.txt", b"base\n");
    repository.commit_all();
    let captured = capture(&repository, "validation", None);
    let checkpoint = stored(&captured, "identity-a", 0);

    let identity_error = verify_checkpoint(&repository.authorized("identity-b"), &checkpoint)
        .expect_err("identity mismatch should fail");
    assert_eq!(identity_error.code, ChatErrorCode::ConfigurationInvalid);
    let wrong_oid = run_git(&repository.0, &["rev-parse", "HEAD"]);
    assert!(delete_exact_ref(&repository.0, &captured.hidden_ref_name, &wrong_oid).is_err());
    verify_checkpoint(&repository.authorized("identity-a"), &checkpoint)
        .expect("wrong expected OID must retain ref");
    delete_exact_ref(
        &repository.0,
        &captured.hidden_ref_name,
        &captured.git_object_id,
    )
    .expect("exact cleanup should succeed");
    let missing = verify_checkpoint(&repository.authorized("identity-a"), &checkpoint)
        .expect_err("missing ref should fail");
    assert_eq!(missing.code, ChatErrorCode::NotFound);
}

#[test]
fn stale_restore_snapshot_aborts_without_changing_files() {
    let repository = TestRepository::new();
    repository.write("tracked.txt", b"base\n");
    repository.commit_all();
    let target = capture(&repository, "stale-target", None);
    repository.write("tracked.txt", b"preview state\n");
    let preview = current_git_snapshot(&repository.0).expect("preview should succeed");
    repository.write("tracked.txt", b"changed after preview\n");

    let error = restore_git_snapshot(&repository.0, &preview, &stored(&target, "identity", 0))
        .expect_err("stale preview should abort");
    assert_eq!(error.code, ChatErrorCode::StaleRevision);
    assert_eq!(
        fs::read(repository.0.join("tracked.txt")).expect("file should read"),
        b"changed after preview\n"
    );
}

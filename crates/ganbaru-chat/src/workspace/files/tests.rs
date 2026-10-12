use super::*;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-file-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("test directory should be created");
        Self(fs::canonicalize(path).expect("test path should canonicalize"))
    }

    fn authorized(&self, kind: RepositoryKind) -> AuthorizedWorkingFolder {
        AuthorizedWorkingFolder {
            working_folder_id: ProjectWorkingFolderId::new("workspace:file-test")
                .expect("workspace ID should be valid"),
            canonical_path: self.0.clone(),
            repository_kind: kind,
            repository_identity: None,
            repository_storage_identity: None,
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn internal_artifact_names_cover_platform_recovery_shapes_only() {
    let private_token = "01".repeat(32);
    for name in [
        ".ganbaru.123.4.567.tmp".to_string(),
        ".ganbaru.123.4.567.backup".to_string(),
        ".ganbaru.123.4.567.recovery".to_string(),
        ".document.ganbaru.123.4.tmp".to_string(),
        "document.ganbaru.123.4.backup".to_string(),
        format!(".ganbaru.backup.{private_token}"),
        format!(".ganbaru.recovery.{private_token}"),
    ] {
        assert!(
            ganbaru_internal_artifact_segment(&name),
            "internal artifact was visible: {name}"
        );
    }

    for name in [
        "document.tmp",
        "document.ganbaru.notes.backup",
        ".ganbaru.backup.short",
        ".ganbaru.recovery.not-hex",
        ".ganbaru.123.4.tmp.extra",
    ] {
        assert!(
            !ganbaru_internal_artifact_segment(name),
            "ordinary name was hidden: {name}"
        );
    }
}

#[test]
fn listing_is_on_demand_and_respects_common_and_git_ignores() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("visible.txt"), "visible\n").expect("file should write");
    fs::write(directory.0.join("ignored.log"), "ignored\n").expect("file should write");
    fs::write(directory.0.join(".gitignore"), "ignored.log\n").expect("ignore file should write");
    fs::create_dir(directory.0.join("node_modules")).expect("ignored directory should exist");
    let status = Command::new("git")
        .arg("-C")
        .arg(&directory.0)
        .args(["init", "-q"])
        .status()
        .expect("Git should start");
    assert!(status.success());
    let authorized = directory.authorized(RepositoryKind::Git);

    let visible = list_workspace_directory(&authorized, "", false).expect("listing should succeed");
    assert!(
        visible
            .entries
            .iter()
            .any(|entry| entry.relative_path == "visible.txt")
    );
    assert!(
        !visible
            .entries
            .iter()
            .any(|entry| entry.relative_path == "ignored.log")
    );
    assert!(
        !visible
            .entries
            .iter()
            .any(|entry| entry.relative_path == "node_modules")
    );
    let all = list_workspace_directory(&authorized, "", true)
        .expect("listing with ignored files should succeed");
    assert!(
        all.entries
            .iter()
            .any(|entry| entry.relative_path == "ignored.log" && entry.ignored)
    );
    assert!(
        all.entries
            .iter()
            .any(|entry| entry.relative_path == "node_modules" && entry.ignored)
    );
}

#[cfg(unix)]
#[test]
fn directory_listing_skips_non_utf8_and_special_entries() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::net::UnixListener;

    let directory = TestDirectory::new();
    fs::write(directory.0.join("visible.txt"), "visible\n").expect("regular file should write");
    let non_utf8_name = OsString::from_vec(vec![b'n', b'a', b'm', b'e', 0xff]);
    fs::write(directory.0.join(non_utf8_name), "hidden\n").expect("non-UTF-8 file should write");
    let socket_path = directory.0.join("service.sock");
    let _socket = UnixListener::bind(&socket_path).expect("Unix socket should bind");
    let authorized = directory.authorized(RepositoryKind::None);

    let listing = list_workspace_directory(&authorized, "", true).expect("listing should succeed");

    assert_eq!(
        listing
            .entries
            .iter()
            .map(|entry| entry.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["visible.txt"]
    );
}

#[test]
fn preview_bounds_text_and_rejects_binary_traversal_and_symlinks() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("sample.rs"), "fn main() {}\n").expect("file should write");
    fs::write(directory.0.join("binary.bin"), [0, 1, 2]).expect("binary should write");
    fs::write(
        directory.0.join("large.txt"),
        vec![b'a'; MAX_PREVIEW_BYTES as usize + 1],
    )
    .expect("large file should write");
    let authorized = directory.authorized(RepositoryKind::None);

    let text = preview_workspace_file(&authorized, "sample.rs").expect("text should preview");
    assert_eq!(text.line_count, Some(1));
    assert!(text.content_revision.is_some());
    assert!(
        preview_workspace_file(&authorized, "binary.bin")
            .expect("binary metadata should read")
            .binary
    );
    assert!(
        preview_workspace_file(&authorized, "large.txt")
            .expect("large metadata should read")
            .oversized
    );
    assert!(preview_workspace_file(&authorized, "../outside.txt").is_err());

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(directory.0.join("sample.rs"), directory.0.join("link.rs"))
            .expect("symlink should be created");
        assert!(preview_workspace_file(&authorized, "link.rs").is_err());
    }
}

#[test]
fn managed_artifact_reads_binary_bytes_without_following_links() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("artifact.bin"), [0, 1, 2, 3]).expect("artifact should write");

    assert_eq!(
        read_managed_artifact_bytes(&directory.0, "artifact.bin")
            .expect("regular artifact should read"),
        [0, 1, 2, 3]
    );
    assert!(read_managed_artifact_bytes(&directory.0, "../artifact.bin").is_err());

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            directory.0.join("artifact.bin"),
            directory.0.join("artifact-link.bin"),
        )
        .expect("artifact link should be created");
        assert!(read_managed_artifact_bytes(&directory.0, "artifact-link.bin").is_err());
    }
}

#[test]
fn save_requires_the_current_revision_and_preserves_file_permissions() {
    let directory = TestDirectory::new();
    let path = directory.0.join("sample.rs");
    fs::write(&path, "fn before() {}\n").expect("file should write");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("permissions should update");
    }
    let authorized = directory.authorized(RepositoryKind::None);
    let original = preview_workspace_file(&authorized, "sample.rs").expect("text should preview");
    let revision = original
        .content_revision
        .as_deref()
        .expect("text should have a revision");

    let saved = save_workspace_file(&authorized, "sample.rs", "fn after() {}\n", revision)
        .expect("matching revision should save");
    assert_eq!(saved.text.as_deref(), Some("fn after() {}\n"));
    assert_ne!(saved.content_revision, original.content_revision);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path)
                .expect("metadata should read")
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }

    let error = save_workspace_file(&authorized, "sample.rs", "fn stale() {}\n", revision)
        .expect_err("stale revision should conflict");
    assert_eq!(error.code, ChatErrorCode::Conflict);
    assert_eq!(
        fs::read_to_string(path).expect("saved file should read"),
        "fn after() {}\n"
    );
}

#[test]
fn save_copy_creates_a_new_file_without_overwriting_an_existing_target() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("sample.rs"), "fn original() {}\n")
        .expect("source file should write");
    let authorized = directory.authorized(RepositoryKind::None);

    let copy = save_workspace_file_copy(
        &authorized,
        "sample.rs",
        "sample.ganbaru-copy.rs",
        "fn local_edit() {}\n",
    )
    .expect("separate copy should save");
    assert_eq!(copy.text.as_deref(), Some("fn local_edit() {}\n"));
    assert_eq!(
        fs::read_to_string(directory.0.join("sample.rs")).expect("source should read"),
        "fn original() {}\n"
    );
    let error = save_workspace_file_copy(
        &authorized,
        "sample.rs",
        "sample.ganbaru-copy.rs",
        "fn overwritten() {}\n",
    )
    .expect_err("an existing copy must not be overwritten");
    assert_eq!(error.code, ChatErrorCode::Conflict);
}

#[test]
fn recreate_requires_confirmation_and_never_overwrites_a_reappeared_file() {
    let directory = TestDirectory::new();
    let authorized = directory.authorized(RepositoryKind::None);

    assert!(recreate_workspace_file(&authorized, "restored.txt", "preserved\n", false).is_err());
    let recreated = recreate_workspace_file(&authorized, "restored.txt", "preserved\n", true)
        .expect("confirmed recreation should succeed");
    assert_eq!(recreated.text.as_deref(), Some("preserved\n"));
    let conflict = recreate_workspace_file(&authorized, "restored.txt", "overwrite\n", true)
        .expect_err("recreation must not overwrite an existing file");
    assert_eq!(conflict.code, ChatErrorCode::Conflict);
    assert_eq!(
        fs::read_to_string(directory.0.join("restored.txt")).expect("recreated file should read"),
        "preserved\n"
    );
}

#[test]
fn delete_requires_the_current_revision_and_removes_only_the_selected_file() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("delete.txt"), "delete me\n").expect("file should write");
    fs::write(directory.0.join("keep.txt"), "keep me\n").expect("file should write");
    let authorized = directory.authorized(RepositoryKind::None);
    let revision = preview_workspace_file(&authorized, "delete.txt")
        .expect("preview should succeed")
        .content_revision
        .expect("text preview should have a revision");

    let stale = delete_workspace_file(&authorized, "delete.txt", "stale")
        .expect_err("stale deletion should fail");
    assert_eq!(stale.code, ChatErrorCode::Conflict);
    assert!(directory.0.join("delete.txt").is_file());

    delete_workspace_file(&authorized, "delete.txt", &revision)
        .expect("current revision should delete");
    assert!(!directory.0.join("delete.txt").exists());
    assert_eq!(
        fs::read_to_string(directory.0.join("keep.txt")).expect("other file should remain"),
        "keep me\n"
    );
}

#[test]
fn save_rejects_binary_oversized_excluded_and_symbolic_files() {
    let directory = TestDirectory::new();
    fs::write(directory.0.join("binary.bin"), [0, 1, 2]).expect("binary should write");
    fs::create_dir(directory.0.join(".git")).expect("excluded directory should exist");
    fs::write(directory.0.join(".git/config"), "config\n").expect("excluded file should write");
    let authorized = directory.authorized(RepositoryKind::None);

    assert!(save_workspace_file(&authorized, "binary.bin", "text", "missing").is_err());
    assert!(save_workspace_file(&authorized, ".git/config", "text", "missing").is_err());
    assert!(
        save_workspace_file(
            &authorized,
            "binary.bin",
            &"x".repeat(MAX_PREVIEW_BYTES as usize + 1),
            "missing",
        )
        .is_err()
    );

    #[cfg(unix)]
    {
        fs::write(directory.0.join("target.txt"), "target\n").expect("target should write");
        std::os::unix::fs::symlink(directory.0.join("target.txt"), directory.0.join("link.txt"))
            .expect("symlink should be created");
        assert!(save_workspace_file(&authorized, "link.txt", "changed\n", "missing").is_err());

        fs::create_dir(directory.0.join("real-parent")).expect("real parent should be created");
        fs::write(directory.0.join("real-parent/nested.txt"), "nested\n")
            .expect("nested file should write");
        std::os::unix::fs::symlink(
            directory.0.join("real-parent"),
            directory.0.join("linked-parent"),
        )
        .expect("parent symlink should be created");
        assert!(preview_workspace_file(&authorized, "linked-parent/nested.txt").is_err());
        assert!(list_workspace_directory(&authorized, "linked-parent", true).is_err());
        assert!(
            save_workspace_file(
                &authorized,
                "linked-parent/nested.txt",
                "changed\n",
                "missing",
            )
            .is_err()
        );
        assert!(
            recreate_workspace_file(&authorized, "linked-parent/new.txt", "new\n", true,).is_err()
        );
        assert_eq!(
            fs::read_to_string(directory.0.join("real-parent/nested.txt"))
                .expect("nested file should remain readable"),
            "nested\n"
        );
        assert!(!directory.0.join("real-parent/new.txt").exists());
    }
}

#[cfg(unix)]
#[test]
fn descriptor_relative_file_open_does_not_follow_a_replaced_parent_path() {
    let directory = TestDirectory::new();
    let original = directory.0.join("original");
    let replacement = directory.0.join("replacement");
    fs::create_dir(&original).expect("original directory should be created");
    fs::create_dir(&replacement).expect("replacement directory should be created");
    fs::write(original.join("sample.txt"), "original\n").expect("original file should be written");
    fs::write(replacement.join("sample.txt"), "replacement\n")
        .expect("replacement file should be written");

    let parent = secure_workspace_parent(&directory.0, "original/sample.txt")
        .expect("secure parent should open");
    let moved = directory.0.join("moved-original");
    fs::rename(&original, &moved).expect("original directory should move");
    std::os::unix::fs::symlink(&replacement, &original)
        .expect("replacement symlink should be created");

    let mut file = open_regular_file_at(&parent).expect("descriptor-relative file should open");
    let mut text = String::new();
    file.read_to_string(&mut text)
        .expect("descriptor-relative file should read");
    assert_eq!(text, "original\n");
}

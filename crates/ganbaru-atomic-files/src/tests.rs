use super::*;

fn test_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ganbaru-atomic-files-{label}-{}",
        random_hex().expect("random test directory")
    ));
    fs::create_dir_all(&path).expect("create test directory");
    path
}

fn entry_names(directory: &Path) -> Vec<String> {
    let mut names = fs::read_dir(directory)
        .expect("read test directory")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn replaces_contents_and_leaves_only_the_target() {
    let directory = test_directory("replace");
    let path = directory.join("config.json");
    write_file_atomically(&path, b"first", FileAccess::OwnerOnly).unwrap();
    write_file_atomically(&path, b"second", FileAccess::OwnerOnly).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"second");
    assert_eq!(entry_names(&directory), vec!["config.json".to_string()]);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_writes_keep_the_previous_target_and_remove_the_temporary_file() {
    let directory = test_directory("failure");
    let path = directory.join("export.csv");
    fs::write(&path, b"previous").unwrap();
    let result: io::Result<()> = write_file_atomically_with(&path, FileAccess::Default, |file| {
        file.write_all(b"partial")?;
        Err(io::Error::other("generation failed"))
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"previous");
    assert_eq!(entry_names(&directory), vec!["export.csv".to_string()]);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn staged_files_replace_the_target_only_when_committed() {
    let directory = test_directory("staged-commit");
    let path = directory.join("asset.png");
    fs::write(&path, b"previous").unwrap();
    let mut staged = StagedFile::create(&path, FileAccess::OwnerOnly).unwrap();
    staged.write_all(b"first ").unwrap();
    staged.write_all(b"second").unwrap();
    assert_eq!(staged.written(), 12);
    assert_eq!(staged.target(), path);
    assert_eq!(fs::read(&path).unwrap(), b"previous");
    assert_eq!(entry_names(&directory).len(), 2);
    staged.commit().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"first second");
    assert_eq!(entry_names(&directory), vec!["asset.png".to_string()]);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn dropped_staged_files_leave_no_temporary_file_or_target() {
    let directory = test_directory("staged-drop");
    let path = directory.join("asset.png");
    let mut staged = StagedFile::create(&path, FileAccess::Default).unwrap();
    staged.write_all(b"partial").unwrap();
    drop(staged);
    assert!(entry_names(&directory).is_empty());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_staged_commits_keep_the_target_and_remove_the_temporary_file() {
    let directory = test_directory("staged-failure");
    // A non-empty directory as the target makes the rename fail.
    let path = directory.join("occupied");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("inner"), b"kept").unwrap();
    let mut staged = StagedFile::create(&path, FileAccess::Default).unwrap();
    staged.write_all(b"bytes").unwrap();
    assert!(staged.commit().is_err());
    assert_eq!(entry_names(&directory), vec!["occupied".to_string()]);
    assert_eq!(fs::read(path.join("inner")).unwrap(), b"kept");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn rejects_paths_without_a_parent_directory() {
    let error =
        write_file_atomically(Path::new("relative.txt"), b"data", FileAccess::Default).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn creates_missing_directories_within_the_root() {
    let root = test_directory("create");
    let path = root.join("assets/notes/files/a.png");
    write_file_atomically_within(&root, &path, b"image", FileAccess::Default).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"image");
    let outside = std::env::temp_dir().join("elsewhere/a.png");
    let error =
        write_file_atomically_within(&root, &outside, b"image", FileAccess::Default).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn removal_within_the_root_tolerates_missing_files_and_directories() {
    let root = test_directory("remove");
    let path = root.join("assets/chat/a.png");
    write_file_atomically_within(&root, &path, b"image", FileAccess::Default).unwrap();
    remove_file_within(&root, &path).unwrap();
    assert!(fs::symlink_metadata(&path).is_err());
    remove_file_within(&root, &path).unwrap();
    remove_file_within(&root, &root.join("missing/dir/a.png")).unwrap();
    let error = remove_file_within(&root, &std::env::temp_dir().join("a.png")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reject_symlink_accepts_missing_and_regular_paths() {
    let directory = test_directory("reject");
    reject_symlink(&directory.join("missing")).unwrap();
    fs::write(directory.join("regular"), b"").unwrap();
    reject_symlink(&directory.join("regular")).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn private_temporary_directories_are_new_and_unpredictable() {
    let parent = test_directory("private");
    let first = create_private_temporary_directory(&parent, "review-").unwrap();
    let second = create_private_temporary_directory(&parent, "review-").unwrap();
    assert_ne!(first, second);
    for directory in [&first, &second] {
        assert_eq!(directory.parent(), Some(parent.as_path()));
        let name = directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let suffix = name.strip_prefix("review-").unwrap();
        assert_eq!(suffix.len(), TEMPORARY_NONCE_BYTES * 2);
        assert!(
            suffix
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );
        assert!(fs::symlink_metadata(directory).unwrap().is_dir());
    }
    fs::remove_dir_all(parent).unwrap();
}

#[test]
fn private_temporary_directories_report_a_missing_parent() {
    let parent = test_directory("private-missing").join("missing");
    let error = create_private_temporary_directory(&parent, "index-").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    fs::remove_dir_all(parent.parent().unwrap()).unwrap();
}

#[test]
fn bounded_reads_accept_the_limit_and_refuse_one_byte_more() {
    let directory = test_directory("bounded-read");
    let path = directory.join("asset.bin");
    fs::write(&path, [7_u8; 16]).unwrap();
    for mode in [FinalSymlink::Follow, FinalSymlink::Refuse] {
        assert_eq!(
            read_regular_file_capped(&path, 16, mode).unwrap(),
            [7_u8; 16]
        );
        let error = read_regular_file_capped(&path, 15, mode).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn bounded_reads_refuse_directories_and_missing_files() {
    let directory = test_directory("bounded-read-kind");
    let error = read_regular_file_capped(&directory, 16, FinalSymlink::Refuse).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let error =
        read_regular_file_capped(&directory.join("missing"), 16, FinalSymlink::Follow).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn private_temporary_directories_are_owner_only() {
        let parent = test_directory("private-mode");
        let directory = create_private_temporary_directory(&parent, "objects-").unwrap();
        let mode = fs::metadata(&directory).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn private_directories_are_owner_only_and_keep_existing_ancestors() {
        let parent = test_directory("private-tree");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        let leaf = parent.join("home").join("sessions");
        create_private_directories(&leaf).unwrap();
        create_private_directories(&leaf).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&parent.join("home")), 0o700);
        assert_eq!(mode(&leaf), 0o700);
        assert_eq!(mode(&parent), 0o755);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn ownership_matches_only_the_current_user() {
        let directory = test_directory("ownership");
        assert!(is_owned_by_current_user(&directory).unwrap());
        if rustix::process::geteuid().as_raw() != 0 {
            assert!(!is_owned_by_current_user(Path::new("/")).unwrap());
        }
        assert!(is_owned_by_current_user(&directory.join("missing")).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn single_private_directories_and_files_are_new_and_owner_only() {
        let parent = test_directory("private-single");
        let directory = parent.join("staging");
        create_private_directory(&directory).unwrap();
        let error = create_private_directory(&directory).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let file = directory.join("ganbaru.sqlite");
        create_private_file(&file).unwrap();
        let error = create_private_file(&file).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&directory), 0o700);
        assert_eq!(mode(&file), 0o600);
        fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn restricting_to_the_owner_clears_only_group_and_other_bits() {
        let directory = test_directory("restrict");
        let file = directory.join("ganbaru.sqlite-wal");
        fs::write(&file, b"").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o664)).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        restrict_to_owner(&file).unwrap();
        restrict_to_owner(&directory).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&directory), 0o700);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o400)).unwrap();
        restrict_to_owner(&file).unwrap();
        assert_eq!(mode(&file), 0o400);
        let error = restrict_to_owner(&directory.join("missing")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn owner_only_files_are_private_and_default_files_follow_the_umask() {
        let directory = test_directory("modes");
        let private = directory.join("vault.json");
        write_file_atomically(&private, b"{}", FileAccess::OwnerOnly).unwrap();
        let mode = fs::metadata(&private).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let shared = directory.join("export.ics");
        write_file_atomically(&shared, b"", FileAccess::Default).unwrap();
        let reference = directory.join("reference");
        fs::write(&reference, b"").unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&shared), mode(&reference));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_symlinked_target_is_replaced_without_touching_the_link_destination() {
        let directory = test_directory("target-link");
        let sentinel = directory.join("sentinel");
        fs::write(&sentinel, b"untouched").unwrap();
        let path = directory.join("config.json");
        symlink(&sentinel, &path).unwrap();
        write_file_atomically(&path, b"{}", FileAccess::OwnerOnly).unwrap();
        assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(metadata.file_type().is_file());
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_planted_legacy_temporary_link_is_never_written_through() {
        let directory = test_directory("temporary-link");
        let sentinel = directory.join("sentinel");
        fs::write(&sentinel, b"untouched").unwrap();
        symlink(&sentinel, directory.join("config.json.tmp")).unwrap();
        let path = directory.join("config.json");
        write_file_atomically(&path, b"{}", FileAccess::OwnerOnly).unwrap();
        assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
        assert!(fs::symlink_metadata(&path).unwrap().file_type().is_file());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn symlinked_directories_below_the_root_are_refused() {
        let root = test_directory("dir-link");
        let outside = test_directory("dir-link-outside");
        fs::create_dir_all(root.join("assets")).unwrap();
        symlink(&outside, root.join("assets/profile")).unwrap();
        let path = root.join("assets/profile/a.png");
        let error =
            write_file_atomically_within(&root, &path, b"image", FileAccess::Default).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(entry_names(&outside).is_empty());
        reject_symlink(&root.join("assets/profile")).unwrap_err();
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn removal_never_reaches_through_a_symlinked_directory() {
        let root = test_directory("remove-link");
        let outside = test_directory("remove-link-outside");
        fs::write(outside.join("a.png"), b"keep").unwrap();
        fs::create_dir_all(root.join("assets")).unwrap();
        symlink(&outside, root.join("assets/chat")).unwrap();
        let error = remove_file_within(&root, &root.join("assets/chat/a.png")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(fs::read(outside.join("a.png")).unwrap(), b"keep");
        require_directories_within(&root, &root.join("assets/chat")).unwrap_err();

        fs::remove_file(root.join("assets/chat")).unwrap();
        fs::create_dir(root.join("assets/chat")).unwrap();
        symlink(outside.join("a.png"), root.join("assets/chat/a.png")).unwrap();
        remove_file_within(&root, &root.join("assets/chat/a.png")).unwrap();
        assert!(fs::symlink_metadata(root.join("assets/chat/a.png")).is_err());
        assert_eq!(fs::read(outside.join("a.png")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn a_symlinked_root_chosen_by_the_user_still_works() {
        let real = test_directory("real-root");
        let alias = std::env::temp_dir().join(format!(
            "ganbaru-atomic-files-alias-{}",
            random_hex().unwrap()
        ));
        symlink(&real, &alias).unwrap();
        let path = alias.join("assets/project-icons/a.png");
        write_file_atomically_within(&alias, &path, b"icon", FileAccess::Default).unwrap();
        assert_eq!(
            fs::read(real.join("assets/project-icons/a.png")).unwrap(),
            b"icon"
        );
        fs::remove_file(alias).unwrap();
        fs::remove_dir_all(real).unwrap();
    }

    #[test]
    fn bounded_reads_follow_or_refuse_a_final_symlink_by_channel() {
        let directory = test_directory("bounded-read-link");
        let outside = directory.join("outside.txt");
        fs::write(&outside, b"secret").unwrap();
        let link = directory.join("asset.png");
        symlink(&outside, &link).unwrap();
        assert_eq!(
            read_regular_file_capped(&link, 16, FinalSymlink::Follow).unwrap(),
            b"secret"
        );
        let error = read_regular_file_capped(&link, 16, FinalSymlink::Refuse).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bounded_reads_refuse_devices() {
        for mode in [FinalSymlink::Follow, FinalSymlink::Refuse] {
            let error = read_regular_file_capped(Path::new("/dev/null"), 16, mode).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }
}

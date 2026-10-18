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
fn reject_symlink_accepts_missing_and_regular_paths() {
    let directory = test_directory("reject");
    reject_symlink(&directory.join("missing")).unwrap();
    fs::write(directory.join("regular"), b"").unwrap();
    reject_symlink(&directory.join("regular")).unwrap();
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

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
}

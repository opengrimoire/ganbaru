use super::{
    AcpTerminalCallbacks, AcpTerminalState, append_terminal_bytes, atomic_write_text,
    verified_existing_file, verified_write_path,
};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be valid")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-acp-callback-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory should be created");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn file_callbacks_reject_cross_workspace_and_preserve_atomic_contents() {
    let workspace = TestDirectory::new();
    let outside = TestDirectory::new();
    let file = workspace.0.join("source.txt");
    fs::write(&file, "before").expect("test file should be written");
    assert_eq!(
        verified_existing_file(&workspace.0, &file).expect("file should be authorized"),
        file
    );
    assert!(verified_existing_file(&workspace.0, &outside.0.join("secret.txt")).is_err());
    let write_path = verified_write_path(&workspace.0, &workspace.0.join("created.txt"))
        .expect("new file should be authorized");
    atomic_write_text(&write_path, b"after").expect("atomic write should succeed");
    assert_eq!(fs::read_to_string(write_path).unwrap(), "after");
}

#[cfg(unix)]
#[test]
fn file_callbacks_reject_symlink_substitution() {
    use std::os::unix::fs::symlink;
    let workspace = TestDirectory::new();
    let outside = TestDirectory::new();
    let outside_file = outside.0.join("secret.txt");
    fs::write(&outside_file, "secret").unwrap();
    let link = workspace.0.join("link.txt");
    symlink(&outside_file, &link).unwrap();
    assert!(verified_existing_file(&workspace.0, &link).is_err());
    assert!(verified_write_path(&workspace.0, &link).is_err());
}

#[test]
fn terminal_callbacks_bound_output_and_session_ownership() {
    let callbacks = AcpTerminalCallbacks::new(PathBuf::from("/workspace"), "session-a".into());
    assert!(callbacks.verify_session("session-a").is_ok());
    assert!(callbacks.verify_session("session-b").is_err());
    let mut state = AcpTerminalState {
        output: Vec::new(),
        output_limit: 4,
        truncated: false,
        exit_status: None,
    };
    append_terminal_bytes(&mut state, b"abcdef");
    assert_eq!(state.output, b"cdef");
    assert!(state.truncated);
}

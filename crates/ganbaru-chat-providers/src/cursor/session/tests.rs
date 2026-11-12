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

fn internal_mcp(organizational_authority: bool) -> crate::models::ProviderInternalMcpConfig {
    crate::models::ProviderInternalMcpConfig {
        name: "ganbaru-chat".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "test-token".to_string(),
        organizational_authority,
    }
}

#[test]
fn internal_mcp_server_goes_only_to_agents_that_take_http_mcp() {
    let direct = internal_mcp(false);
    let servers = super::configuration::acp_mcp_servers("Agent", true, Some(&direct)).unwrap();
    assert_eq!(servers[0]["type"], "http");
    assert_eq!(servers[0]["url"], "http://127.0.0.1:41234/mcp");
    assert_eq!(servers[0]["headers"][0]["value"], "Bearer test-token");

    let omitted = super::configuration::acp_mcp_servers("Agent", false, Some(&direct)).unwrap();
    assert_eq!(omitted, serde_json::json!([]));
    let none = super::configuration::acp_mcp_servers("Agent", false, None).unwrap();
    assert_eq!(none, serde_json::json!([]));
}

#[test]
fn organizational_session_without_http_mcp_fails_closed() {
    let organizational = internal_mcp(true);
    let error =
        super::configuration::acp_mcp_servers("Agent", false, Some(&organizational)).unwrap_err();
    assert_eq!(
        error.code,
        crate::models::ChatErrorCode::CapabilityUnsupported
    );
    assert!(error.message.contains("host tools"));
    assert!(
        super::configuration::acp_mcp_servers("Agent", true, Some(&organizational))
            .unwrap()
            .as_array()
            .is_some_and(|servers| servers.len() == 1)
    );
}

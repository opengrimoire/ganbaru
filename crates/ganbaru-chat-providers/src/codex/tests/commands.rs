use super::*;

#[test]
fn native_slash_commands_only_capture_supported_standalone_requests() {
    let session_id = ProviderSessionId::new("session-command").unwrap();
    let mut request = fixture_turn(&session_id, "build");
    request.developer_instructions = None;
    request.prompt = "/compact".to_string();
    assert_eq!(
        codex_native_command(&request),
        Some(CodexNativeCommand::Compact)
    );
    request.prompt = "/review focus on persistence".to_string();
    assert_eq!(
        codex_native_command(&request),
        Some(CodexNativeCommand::Review(
            "focus on persistence".to_string()
        ))
    );
    request.prompt = "Explain /compact".to_string();
    assert_eq!(codex_native_command(&request), None);
    request.prompt = "/compact".to_string();
    request.mentions.push(WorkspaceMentionReference {
        relative_path: "src/lib.rs".to_string(),
        kind: "file".to_string(),
    });
    assert_eq!(codex_native_command(&request), None);
}

#[test]
fn goal_commands_validate_objectives_and_map_status_actions() {
    let (method, params, _) = goal_request("thread-1", "pause").unwrap();
    assert_eq!(method, "thread/goal/set");
    assert_eq!(params["status"], "paused");
    let (method, params, _) = goal_request("thread-1", "Finish the migration").unwrap();
    assert_eq!(method, "thread/goal/set");
    assert_eq!(params["objective"], "Finish the migration");
    assert!(goal_request("thread-1", &"x".repeat(4_001)).is_err());
}

#[test]
fn mcp_status_pages_preserve_authentication_and_enabled_state() {
    let page = parse_mcp_status_page(&json!({
        "data": [
            { "name": "node_repl", "authStatus": "unsupported", "enabled": true, "status": "ready" },
            { "name": "docs", "authStatus": "notAuthenticated", "enabled": false }
        ],
        "nextCursor": "page-2"
    }))
    .unwrap();

    assert_eq!(page.next_cursor.as_deref(), Some("page-2"));
    assert_eq!(page.servers.len(), 2);
    assert_eq!(page.servers[0].name, "node_repl");
    assert_eq!(page.servers[0].auth_status.as_deref(), Some("unsupported"));
    assert!(page.servers[0].enabled);
    assert_eq!(page.servers[0].runtime_status.as_deref(), Some("ready"));
    assert!(!page.servers[1].enabled);
}

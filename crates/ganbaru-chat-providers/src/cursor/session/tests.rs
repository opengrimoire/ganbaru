use super::callbacks::{
    TerminalInvocation, create_acp_terminal, plan_write_refusal, refuse_git_metadata_write,
    terminal_invocation, validate_terminal_command, windows_command_shell,
};
use super::{
    AcpTerminalCallbacks, AcpTerminalState, TerminalStreamDecoder, append_terminal_bytes,
    atomic_write_text, verified_existing_file, verified_write_path,
};
use crate::models::{ChatErrorCode, InteractionMode};
use agent_client_protocol::schema::v1::CreateTerminalRequest;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
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

fn terminal_state(output_limit: usize) -> AcpTerminalState {
    AcpTerminalState {
        output: Vec::new(),
        output_limit,
        truncated: false,
        exit_status: None,
    }
}

/// A character split across reads of one stream survives bytes the other stream writes between
/// the two reads, and an unfinished character at the end of a stream stays visible.
#[test]
fn terminal_streams_keep_characters_split_across_interleaved_reads() {
    let mut state = terminal_state(1024);
    let mut stdout = TerminalStreamDecoder::default();
    let mut stderr = TerminalStreamDecoder::default();
    let snowman = "\u{2603}".as_bytes();
    stdout.append(&mut state, &[b'a', snowman[0]]);
    stderr.append(&mut state, b"x");
    stdout.append(&mut state, &snowman[1..2]);
    stderr.append(&mut state, &[0xc3]);
    stdout.append(&mut state, &snowman[2..]);
    stderr.append(&mut state, &[0xa9, 0xff, b'y']);
    stdout.append(&mut state, &[0xe2, 0x82]);
    stdout.finish(&mut state);
    stderr.finish(&mut state);
    assert_eq!(
        String::from_utf8_lossy(&state.output),
        "ax\u{2603}\u{e9}\u{fffd}y\u{fffd}"
    );
    assert!(!state.truncated);
}

/// Truncating the front of the output never leaves half a character behind.
#[test]
fn terminal_truncation_drops_whole_characters() {
    let mut state = terminal_state(4);
    append_terminal_bytes(&mut state, "\u{e9}\u{e9}a".as_bytes());
    assert_eq!(state.output, "\u{e9}a".as_bytes());
    assert!(state.truncated);
}

fn internal_mcp(organizational_authority: bool) -> crate::models::ProviderInternalMcpConfig {
    crate::models::ProviderInternalMcpConfig {
        name: "ganbaru-chat".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "test-token".to_string(),
        organizational_authority,
        pre_approved_tools: Vec::new(),
        organizational_scope: organizational_authority
            .then_some(crate::models::ProviderOrganizationalScope::Conversation),
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

fn terminal_request(command: &str, args: &[&str], env: &[(&str, &str)]) -> CreateTerminalRequest {
    serde_json::from_value(serde_json::json!({
        "sessionId": "session-a",
        "command": command,
        "args": args,
        "env": env
            .iter()
            .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
            .collect::<Vec<_>>(),
    }))
    .expect("terminal request should parse")
}

#[test]
fn terminal_environment_refuses_loader_variables() {
    for name in [
        "LD_PRELOAD",
        "LD_AUDIT",
        "LD_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
    ] {
        let error = validate_terminal_command(&terminal_request("true", &[], &[(name, "x")]))
            .expect_err("loader variable should be refused");
        assert_eq!(error.code, ChatErrorCode::Permission);
        assert!(
            error
                .message
                .starts_with("acp_terminal_environment_refused")
        );
        assert!(error.message.contains(name));
    }
    assert!(
        validate_terminal_command(&terminal_request("true", &[], &[("RUST_LOG", "debug")])).is_ok()
    );
}

#[test]
fn terminal_requests_without_arguments_run_as_shell_command_lines() {
    let grok = terminal_request("/bin/bash -lc 'ls'", &[], &[]);
    assert_eq!(
        terminal_invocation(&grok),
        TerminalInvocation::Shell("/bin/bash -lc 'ls'")
    );
    let omitted: CreateTerminalRequest = serde_json::from_value(serde_json::json!({
        "sessionId": "session-a",
        "command": "git status --short",
    }))
    .expect("a request without args should parse");
    assert_eq!(
        terminal_invocation(&omitted),
        TerminalInvocation::Shell("git status --short")
    );
    let exec = terminal_request("sh", &["-c", "exit 3"], &[]);
    assert_eq!(
        terminal_invocation(&exec),
        TerminalInvocation::Exec("sh", &["-c".to_string(), "exit 3".to_string()])
    );
}

#[test]
fn terminal_commands_refuse_blank_and_nul_command_lines() {
    for command in ["", "   ", "\t\n", "ls\0rm"] {
        let error = validate_terminal_command(&terminal_request(command, &[], &[]))
            .expect_err("command should be refused");
        assert_eq!(error.code, ChatErrorCode::Validation);
    }
    assert!(validate_terminal_command(&terminal_request(" ", &["-c", "true"], &[])).is_err());
    assert!(validate_terminal_command(&terminal_request(&"x".repeat(4_097), &[], &[])).is_err());
    assert!(validate_terminal_command(&terminal_request("ls -la", &[], &[])).is_ok());
}

#[test]
fn windows_command_shell_requires_an_absolute_path() {
    use std::ffi::OsStr;
    let comspec = PathBuf::from("/opt/shell/cmd.exe");
    assert_eq!(
        windows_command_shell(Some(comspec.as_os_str()), Some(OsStr::new("/windows"))),
        Some(comspec)
    );
    assert_eq!(
        windows_command_shell(Some(OsStr::new("cmd.exe")), Some(OsStr::new("/windows"))),
        Some(PathBuf::from("/windows").join("System32").join("cmd.exe"))
    );
    assert_eq!(
        windows_command_shell(Some(OsStr::new("cmd.exe")), Some(OsStr::new("windows"))),
        None
    );
    assert_eq!(windows_command_shell(None, None), None);
}

#[cfg(unix)]
async fn finished_output(
    callbacks: &AcpTerminalCallbacks,
    request: CreateTerminalRequest,
) -> (String, Option<u32>) {
    let id = create_acp_terminal(callbacks, request)
        .await
        .expect("terminal should start");
    let terminal = callbacks.terminal(&id).unwrap();
    let status = reported_exit(&terminal).await;
    let output = String::from_utf8_lossy(&terminal.state.lock().unwrap().output).into_owned();
    (output, status.exit_code)
}

#[cfg(unix)]
#[test]
fn terminal_command_lines_run_through_the_shell_in_the_workspace() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        let (output, code) = finished_output(
            &callbacks,
            terminal_request("sh -c 'printf \"%s|\" \"one two\" three; exit 4'", &[], &[]),
        )
        .await;
        assert_eq!(output, "one two|three|");
        assert_eq!(code, Some(4));

        let (output, code) = finished_output(
            &callbacks,
            terminal_request(
                "printf '%s' \"$GANBARU_PROBE\" && pwd -P",
                &[],
                &[("GANBARU_PROBE", "env|")],
            ),
        )
        .await;
        let canonical = fs::canonicalize(&workspace.0).unwrap();
        assert_eq!(output.trim_end(), format!("env|{}", canonical.display()));
        assert_eq!(code, Some(0));

        let (_, code) =
            finished_output(&callbacks, terminal_request("'unbalanced", &[], &[])).await;
        assert_ne!(
            code,
            Some(0),
            "a malformed command line should fail in the shell"
        );
        callbacks.shutdown();
    });
}

#[cfg(unix)]
#[test]
fn terminal_arguments_are_never_reparsed_by_a_shell() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        let literal = "$(echo injected); 'quoted' \"double\" *";
        let (output, code) = finished_output(
            &callbacks,
            terminal_request("printf", &["%s", literal], &[]),
        )
        .await;
        assert_eq!(output, literal);
        assert_eq!(code, Some(0));
        let error = create_acp_terminal(
            &callbacks,
            terminal_request("printf %s", &["exec form"], &[]),
        )
        .await
        .expect_err("a program name with arguments is not split into words");
        assert_eq!(error.code, ChatErrorCode::DriverUnavailable);
        callbacks.shutdown();
    });
}

#[test]
fn file_writes_are_refused_in_plan_mode() {
    let error = plan_write_refusal(InteractionMode::Plan).expect_err("Plan should refuse");
    assert!(error.message.starts_with("acp_write_refused_in_plan"));
    assert!(plan_write_refusal(InteractionMode::Build).is_ok());
}

#[cfg(unix)]
#[test]
fn terminal_exit_is_reported_without_polling() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        let id = create_acp_terminal(&callbacks, terminal_request("sh", &["-c", "exit 3"], &[]))
            .await
            .expect("terminal should start");
        let terminal = callbacks.terminal(&id).unwrap();
        let status = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = terminal.completed.notified();
                if let Some(status) = terminal.state.lock().unwrap().exit_status.clone() {
                    break status;
                }
                notified.await;
            }
        })
        .await
        .expect("exit should be reported");
        assert_eq!(status.exit_code, Some(3));
        callbacks.shutdown();
    });
}

#[cfg(unix)]
#[test]
fn released_terminal_stops_background_descendants() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        let id = create_acp_terminal(
            &callbacks,
            terminal_request("sh", &["-c", "sleep 30 & echo $!; exit 0"], &[]),
        )
        .await
        .expect("terminal should start");
        let terminal = callbacks.terminal(&id).unwrap();
        let background = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let output = String::from_utf8_lossy(&terminal.state.lock().unwrap().output)
                    .trim()
                    .to_string();
                if let Ok(pid) = output.parse::<u32>() {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("background process ID should be printed");
        assert!(process_exists(background));
        callbacks
            .terminals
            .lock()
            .unwrap()
            .remove(&id)
            .expect("terminal should be registered")
            .terminate();
        tokio::time::timeout(Duration::from_secs(5), async {
            while process_exists(background) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("background descendant should stop with the terminal");
    });
}

#[cfg(unix)]
async fn reported_exit(
    terminal: &super::AcpTerminal,
) -> agent_client_protocol::schema::v1::TerminalExitStatus {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let notified = terminal.completed.notified();
            if let Some(status) = terminal.state.lock().unwrap().exit_status.clone() {
                break status;
            }
            notified.await;
        }
    })
    .await
    .expect("exit should be reported")
}

#[cfg(unix)]
async fn wait_until_gone(process_id: u32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while process_exists(process_id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("process should be reaped");
}

#[cfg(unix)]
#[test]
fn exited_terminal_keeps_its_group_reserved_until_release() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        for (script, kill_first, exit_code) in [
            ("echo $$; exit 3", false, Some(3)),
            ("echo $$; kill -TERM $$", false, None),
            ("echo $$; exit 0", true, Some(0)),
        ] {
            let id = create_acp_terminal(&callbacks, terminal_request("sh", &["-c", script], &[]))
                .await
                .expect("terminal should start");
            let terminal = callbacks.terminal(&id).unwrap();
            let status = reported_exit(&terminal).await;
            assert_eq!(status.exit_code, exit_code);
            assert_eq!(status.signal.is_some(), exit_code.is_none());
            let leader = String::from_utf8_lossy(&terminal.state.lock().unwrap().output)
                .trim()
                .parse::<u32>()
                .expect("leader process ID should be printed");
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert!(
                process_exists(leader),
                "an exited command must stay unreaped while the terminal is held"
            );
            assert!(!terminal.tree.is_retired());
            if kill_first {
                terminal.terminate();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            callbacks
                .terminals
                .lock()
                .unwrap()
                .remove(&id)
                .expect("terminal should be registered")
                .terminate();
            wait_until_gone(leader).await;
            assert!(
                terminal.tree.is_retired(),
                "a reaped command's group must never be signaled again"
            );
        }
    });
}

#[cfg(unix)]
#[test]
fn killed_running_terminal_is_reaped_and_retired() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
        let id = create_acp_terminal(
            &callbacks,
            terminal_request("sh", &["-c", "echo $$; exec sleep 30"], &[]),
        )
        .await
        .expect("terminal should start");
        let terminal = callbacks.terminal(&id).unwrap();
        let leader = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let output = String::from_utf8_lossy(&terminal.state.lock().unwrap().output)
                    .trim()
                    .to_string();
                if let Ok(pid) = output.parse::<u32>() {
                    break pid;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("leader process ID should be printed");
        terminal.terminate();
        let status = reported_exit(&terminal).await;
        assert_eq!(status.exit_code, None);
        assert!(status.signal.is_some());
        wait_until_gone(leader).await;
        assert!(terminal.tree.is_retired());
        callbacks.shutdown();
    });
}

#[cfg(unix)]
#[test]
fn dropped_terminal_supervisor_retires_the_tree() {
    let workspace = TestDirectory::new();
    let callbacks = AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into());
    let terminal = crate::test_block_on(async {
        let id = create_acp_terminal(&callbacks, terminal_request("sleep", &["30"], &[]))
            .await
            .expect("terminal should start");
        callbacks.terminal(&id).unwrap()
    });
    assert!(
        terminal.tree.is_retired(),
        "a tree whose command the runtime reaps must never be signaled again"
    );
    callbacks.shutdown();
}

#[cfg(unix)]
fn process_exists(process_id: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &process_id.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn file_writes_never_reach_git_metadata() {
    let workspace = PathBuf::from("/workspace");
    for refused in [
        "/workspace/.git/config",
        "/workspace/.git/hooks/pre-commit",
        "/workspace/vendor/lib/.git/config",
        "/workspace/.GIT/config",
        "/workspace/.git",
    ] {
        let error = refuse_git_metadata_write(&workspace, &PathBuf::from(refused)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission, "{refused}");
    }
    for allowed in [
        "/workspace/.gitignore",
        "/workspace/src/git/main.rs",
        "/workspace/.github/ci.yml",
    ] {
        refuse_git_metadata_write(&workspace, &PathBuf::from(allowed)).unwrap();
    }
}

fn grok_folder_callbacks(
    workspace: &TestDirectory,
    capability: crate::models::ProviderFolderCapability,
) -> AcpTerminalCallbacks {
    let helper: &'static ganbaru_sandbox::SandboxHelper = Box::leak(Box::new(
        ganbaru_sandbox::SandboxHelper::at("/usr/lib/ganbaru/ganbaru-sandbox"),
    ));
    let internal_mcp = crate::models::ProviderInternalMcpConfig {
        name: "ganbaru".to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "token".to_string(),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: Some(crate::models::ProviderOrganizationalScope::Folder {
            capability,
        }),
    };
    let confinement = super::super::confinement::AcpCallbackConfinement::with_helper(
        super::super::driver::AcpProviderFlavor::Grok,
        Some(&internal_mcp),
        &workspace.0,
        Some(helper),
    )
    .unwrap();
    assert!(confinement.is_some());
    AcpTerminalCallbacks::new(workspace.0.clone(), "session-a".into()).with_confinement(confinement)
}

#[test]
fn agents_without_command_access_never_start_or_hold_terminals() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new();
        for capability in [
            crate::models::ProviderFolderCapability::Read,
            crate::models::ProviderFolderCapability::Edit,
        ] {
            let callbacks = grok_folder_callbacks(&workspace, capability);
            let refused = callbacks.verify_runs_commands().unwrap_err();
            assert!(refused.message.starts_with("acp_command_outside_scope"));
            let refused = create_acp_terminal(&callbacks, terminal_request("true", &[], &[]))
                .await
                .unwrap_err();
            assert!(refused.message.starts_with("acp_command_outside_scope"));
            assert!(callbacks.terminals.lock().unwrap().is_empty());
        }
    });
}

#[test]
fn held_terminal_commands_run_only_on_a_single_allow() {
    use super::super::interactions::{
        TERMINAL_DECLINE_OPTION, TERMINAL_RUN_OPTION, terminal_approval_decisions,
        terminal_approval_runs,
    };
    use crate::models::{ApprovalDecision, ApprovalDecisionKind};
    let decision = |kind, option: Option<&str>| ApprovalDecision {
        kind,
        provider_option_id: option.map(str::to_string),
        updated_tool_input: None,
    };
    assert!(
        terminal_approval_decisions()
            .iter()
            .all(|option| option.decision_kind != ApprovalDecisionKind::AllowSession)
    );
    assert!(terminal_approval_runs(&decision(ApprovalDecisionKind::AllowOnce, None)).unwrap());
    assert!(
        terminal_approval_runs(&decision(
            ApprovalDecisionKind::AllowOnce,
            Some(TERMINAL_RUN_OPTION)
        ))
        .unwrap()
    );
    for declined in [
        decision(ApprovalDecisionKind::Deny, None),
        decision(ApprovalDecisionKind::Deny, Some(TERMINAL_DECLINE_OPTION)),
        decision(ApprovalDecisionKind::Cancel, None),
    ] {
        assert!(!terminal_approval_runs(&declined).unwrap());
    }
    for refused in [
        decision(ApprovalDecisionKind::AllowSession, None),
        decision(
            ApprovalDecisionKind::AllowOnce,
            Some(TERMINAL_DECLINE_OPTION),
        ),
        decision(ApprovalDecisionKind::Deny, Some(TERMINAL_RUN_OPTION)),
    ] {
        assert!(terminal_approval_runs(&refused).is_err());
    }
}

//! Organizational Claude launch surface tests.

use super::{TestDirectory, configuration, modes};
use crate::ProviderDriver;
use crate::claude::driver::ClaudeProviderDriver;
use crate::claude::home::resolve_claude_home;
use crate::claude::organizational::*;
use crate::claude::protocol::*;
use crate::models::*;

const SESSION_UUID: &str = "11111111-1111-4111-8111-111111111111";
const SERVER_NAME: &str = "ganbaru-chat";
const SERVER_URL: &str = "http://127.0.0.1:41234/mcp";

fn internal_server() -> ClaudeInternalMcpLaunch<'static> {
    ClaudeInternalMcpLaunch {
        name: SERVER_NAME,
        url: SERVER_URL,
    }
}

fn options(modes: TurnModeSnapshot, organizational: bool) -> ClaudeLaunchOptions<'static> {
    ClaudeLaunchOptions {
        fresh_session_uuid: Some(SESSION_UUID),
        resume_session_uuid: None,
        last_assistant_uuid: None,
        model: None,
        effort: Some("high"),
        fast_mode: Some(false),
        modes,
        project_configuration: ProjectConfigurationAccess::Restricted,
        internal_mcp: Some(internal_server()),
        organizational: organizational.then_some(ClaudeOrganizationalLaunch {
            internal_server_name: SERVER_NAME,
        }),
        system_prompt: None,
        cli_version: None,
    }
}

fn ask(interaction_mode: InteractionMode) -> TurnModeSnapshot {
    modes(SafetyMode::AskForApproval, interaction_mode)
}

fn launch(configured: &[&str], options: ClaudeLaunchOptions<'static>) -> ChatResult<Vec<String>> {
    let configured = configured
        .iter()
        .map(|argument| argument.to_string())
        .collect::<Vec<_>>();
    launch_arguments(vec!["claude-prefix".to_string()], &configured, options)
}

fn flag_value<'a>(arguments: &'a [String], flag: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

#[test]
fn organizational_launch_uses_strict_mcp_and_denies_tools() {
    let arguments = launch(&[], options(ask(InteractionMode::Build), true)).unwrap();
    let mcp_config = internal_mcp_config_argument(SERVER_NAME, SERVER_URL).unwrap();
    assert_eq!(
        arguments,
        [
            "claude-prefix",
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--permission-prompt-tool",
            "stdio",
            "--include-partial-messages",
            "--replay-user-messages",
            "--permission-mode",
            "default",
            "--session-id",
            SESSION_UUID,
            "--effort",
            "high",
            "--settings",
            r#"{"fastMode":false}"#,
            "--setting-sources",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            mcp_config.as_str(),
            "--tools",
            "Read,Edit,Write,Glob,Grep,NotebookEdit,ExitPlanMode,AskUserQuestion,TodoWrite",
            "--disallowed-tools",
            "Task,Agent,WebFetch,WebSearch,Bash",
        ]
    );
    assert!(
        mcp_config.contains("${GANBARU_CHAT_MCP_TOKEN}"),
        "the CLI expands the token from the environment"
    );
}

#[test]
fn organizational_launch_maps_ask_for_approval_to_default() {
    let build = launch(&[], options(ask(InteractionMode::Build), true)).unwrap();
    assert_eq!(flag_value(&build, "--permission-mode"), Some("default"));
    let plan = launch(&[], options(ask(InteractionMode::Plan), true)).unwrap();
    assert_eq!(flag_value(&plan, "--permission-mode"), Some("plan"));
    for safety_mode in [
        SafetyMode::ApproveForMe,
        SafetyMode::FullAccess,
        SafetyMode::Custom,
    ] {
        for interaction_mode in [InteractionMode::Build, InteractionMode::Plan] {
            let error =
                launch(&[], options(modes(safety_mode, interaction_mode), true)).unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Permission, "{safety_mode:?}");
            assert!(!error.recoverable);
        }
    }
}

#[test]
fn direct_launch_keeps_accept_edits() {
    let direct = launch(&[], options(ask(InteractionMode::Build), false)).unwrap();
    assert_eq!(
        flag_value(&direct, "--permission-mode"),
        Some("acceptEdits")
    );
    assert_eq!(flag_value(&direct, "--setting-sources"), Some("user"));
    assert!(!direct.iter().any(|argument| {
        matches!(
            argument.as_str(),
            "--strict-mcp-config" | "--tools" | "--disallowed-tools"
        )
    }));
}

#[test]
fn organizational_launch_refuses_every_configured_argument() {
    for configured in [
        vec!["--verbose"],
        vec!["--fallback-model", "sonnet"],
        vec!["--tools", "default"],
        vec!["--debug"],
        vec!["prompt text"],
    ] {
        let error = launch(&configured, options(ask(InteractionMode::Build), true)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation, "{configured:?}");
        assert_eq!(error.field.as_deref(), Some("launchArguments"));
    }
}

#[test]
fn every_launch_refuses_session_selecting_arguments() {
    let session = "22222222-2222-4222-8222-222222222222";
    let attached_resume = format!("-r{session}");
    for organizational in [false, true] {
        for configured in [
            vec![attached_resume.as_str()],
            vec!["-pc"],
            vec!["-cp"],
            vec!["-dr"],
            vec!["--from-pr", "12"],
            vec!["--from-pr=12"],
            vec!["--teleport"],
            vec!["--cloud", session],
            vec!["--environment=ccpool_1"],
            vec!["--bg"],
            vec!["--background"],
            vec!["--desktop"],
            vec!["-w"],
            vec!["--worktree=other"],
            vec!["--no-session-persistence"],
        ] {
            let error = launch(
                &configured,
                options(ask(InteractionMode::Build), organizational),
            )
            .unwrap_err();
            assert_eq!(
                error.message, "Claude launch arguments conflict with the managed transport",
                "{configured:?} organizational {organizational}"
            );
        }
    }
}

#[test]
fn direct_launch_keeps_unrelated_short_and_similar_long_arguments() {
    let direct = launch(
        &["-d", "--tools-extra", "--bgcolor", "-"],
        options(ask(InteractionMode::Build), false),
    )
    .unwrap();
    for kept in ["-d", "--tools-extra", "--bgcolor", "-"] {
        assert!(
            direct.iter().any(|argument| argument == kept),
            "only exact long names and clusters with a protected letter are refused: {direct:?}"
        );
    }
}

#[test]
fn direct_launch_keeps_configured_setting_and_tool_arguments() {
    let configured = [
        "--setting-sources",
        "user,project",
        "--allowedTools",
        "Read",
    ];
    let direct = launch(&configured, options(ask(InteractionMode::Build), false)).unwrap();
    let mcp_config = internal_mcp_config_argument(SERVER_NAME, SERVER_URL).unwrap();
    assert!(
        direct.ends_with(&[
            "--setting-sources".to_string(),
            "user".to_string(),
            "--setting-sources".to_string(),
            "user,project".to_string(),
            "--allowedTools".to_string(),
            "Read".to_string(),
            "--mcp-config".to_string(),
            mcp_config,
        ]),
        "direct launches keep configured arguments, with the internal server after them: {direct:?}"
    );
}

#[test]
fn organizational_launch_requires_the_matching_internal_server() {
    let mut missing = options(ask(InteractionMode::Build), true);
    missing.internal_mcp = None;
    assert_eq!(
        launch(&[], missing).unwrap_err().code,
        ChatErrorCode::Permission
    );
    let mut renamed = options(ask(InteractionMode::Build), true);
    renamed.organizational = Some(ClaudeOrganizationalLaunch {
        internal_server_name: "other-server",
    });
    assert_eq!(
        launch(&[], renamed).unwrap_err().code,
        ChatErrorCode::Permission
    );
}

#[test]
fn continuation_group_differs_for_organizational_runs() {
    let home = TestDirectory::new("home-organizational");
    let home = resolve_claude_home(&configuration(home.path())).unwrap();
    let direct = home.continuation_group().unwrap();
    assert_eq!(
        direct,
        home.continuation_group_with_authority(false).unwrap()
    );
    assert_ne!(
        direct,
        home.continuation_group_with_authority(true).unwrap()
    );
}

#[test]
fn claude_support_bits_keep_organizational_runs_conversation_only() {
    let home = TestDirectory::new("home-support");
    let driver = ClaudeProviderDriver::new(configuration(home.path())).unwrap();
    let support = driver.authority_support();
    assert!(
        !support.confined_commands && !support.read_only_root && !support.writable_root,
        "raising these admits folder targets, so the organizational Bash denial must be revisited"
    );
    assert!(support.isolated_conversation && support.internal_host_tools);
}

fn server(name: &str, status: &str) -> ClaudeMcpServerStatus {
    ClaudeMcpServerStatus {
        name: name.to_string(),
        status: status.to_string(),
    }
}

fn init_report(servers: Vec<ClaudeMcpServerStatus>, tools: &[&str]) -> ClaudeInitReport {
    ClaudeInitReport {
        mcp_servers: servers,
        tools: tools.iter().map(|tool| tool.to_string()).collect(),
    }
}

fn assert_permission(result: ChatResult<()>) {
    let error = result.unwrap_err();
    assert_eq!(error, organizational_permission_error());
}

#[test]
fn organizational_mcp_status_rejects_extra_server() {
    assert_permission(verify_organizational_mcp(
        &[
            server(SERVER_NAME, "connected"),
            server("user-server", "connected"),
        ],
        SERVER_NAME,
    ));
    assert_permission(verify_organizational_mcp(
        &[
            server("user-server", "disabled"),
            server(SERVER_NAME, "connected"),
        ],
        SERVER_NAME,
    ));
    assert_permission(verify_organizational_mcp(
        &[
            server(SERVER_NAME, "connected"),
            server(SERVER_NAME, "connected"),
        ],
        SERVER_NAME,
    ));
    assert_permission(verify_organizational_mcp(&[], SERVER_NAME));
}

#[test]
fn organizational_mcp_status_rejects_failed_internal_server() {
    for status in ["failed", "needs-auth", "disabled", ""] {
        assert_permission(verify_organizational_mcp(
            &[server(SERVER_NAME, status)],
            SERVER_NAME,
        ));
    }
    for status in ["connected", "pending"] {
        verify_organizational_mcp(&[server(SERVER_NAME, status)], SERVER_NAME).unwrap();
    }
}

#[test]
fn organizational_mcp_status_parses_only_names_and_statuses() {
    let response = serde_json::json!({
        "mcpServers": [{
            "name": SERVER_NAME,
            "status": "connected",
            "config": { "headers": { "Authorization": "Bearer redacted" } },
            "tools": [{ "name": "ping" }]
        }]
    });
    assert_eq!(
        parse_mcp_status(&response).unwrap(),
        [server(SERVER_NAME, "connected")]
    );
    for malformed in [
        serde_json::json!({}),
        serde_json::json!({ "mcpServers": {} }),
        serde_json::json!({ "mcpServers": [{ "name": SERVER_NAME }] }),
        serde_json::json!({ "mcpServers": [{ "name": 1, "status": "connected" }] }),
    ] {
        let error = parse_mcp_status(&malformed).unwrap_err();
        assert_eq!(error, organizational_permission_error());
    }
}

#[test]
fn organizational_init_accepts_internal_only() {
    verify_organizational_init(
        &init_report(
            vec![server(SERVER_NAME, "connected")],
            &["Read", "Edit", "AskUserQuestion", "mcp__ganbaru-chat__ping"],
        ),
        SERVER_NAME,
    )
    .unwrap();
}

#[test]
fn organizational_init_requires_a_connected_internal_server() {
    for status in ["pending", "failed", "needs-auth"] {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, status)], &["Read"]),
            SERVER_NAME,
        ));
    }
    assert_permission(verify_organizational_init(
        &init_report(
            vec![
                server(SERVER_NAME, "connected"),
                server("claude.ai Drive", "connected"),
            ],
            &["Read"],
        ),
        SERVER_NAME,
    ));
}

#[test]
fn organizational_init_rejects_foreign_mcp_tool() {
    for tool in [
        "mcp__user-server__fetch",
        "mcp__ganbaru-chat-extra__ping",
        "mcp__ganbaru-chat",
    ] {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], &["Read", tool]),
            SERVER_NAME,
        ));
    }
}

#[test]
fn organizational_init_rejects_disallowed_tool() {
    for tool in ORGANIZATIONAL_DISALLOWED_TOOLS {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], &["Read", tool]),
            SERVER_NAME,
        ));
    }
}

#[test]
fn organizational_init_rejects_built_in_tools_outside_the_allowlist() {
    for tool in [
        "Skill",
        "ToolSearch",
        "CronCreate",
        "SendMessage",
        "EnterWorktree",
        "RemoteTrigger",
        "read",
        "",
    ] {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], &["Read", tool]),
            SERVER_NAME,
        ));
    }
}

#[test]
fn organizational_init_accepts_any_subset_of_the_allowlist() {
    let mut tools: Vec<&str> = ORGANIZATIONAL_ALLOWED_TOOLS.to_vec();
    tools.push("mcp__ganbaru-chat__ping");
    for report in [&tools[..], &["mcp__ganbaru-chat__ping"][..], &[][..]] {
        verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], report),
            SERVER_NAME,
        )
        .unwrap();
    }
}

#[test]
fn organizational_allowlist_never_names_a_denied_tool() {
    for tool in ORGANIZATIONAL_ALLOWED_TOOLS {
        assert!(
            !ORGANIZATIONAL_DISALLOWED_TOOLS.contains(&tool),
            "{tool} is both allowed and denied"
        );
    }
}

#[test]
fn organizational_init_report_requires_both_lists() {
    let complete = serde_json::json!({
        "type": "system",
        "subtype": "init",
        "tools": ["Read"],
        "mcp_servers": [{ "name": SERVER_NAME, "status": "connected", "source": "dynamic" }]
    });
    assert_eq!(
        parse_init_report(complete.as_object().unwrap()).unwrap(),
        init_report(vec![server(SERVER_NAME, "connected")], &["Read"])
    );
    for key in ["tools", "mcp_servers"] {
        let mut missing = complete.as_object().unwrap().clone();
        missing.remove(key);
        assert_eq!(
            parse_init_report(&missing).unwrap_err(),
            organizational_permission_error(),
            "{key}"
        );
    }
    let mut numeric_tool = complete.as_object().unwrap().clone();
    numeric_tool.insert("tools".to_string(), serde_json::json!(["Read", 7]));
    assert_eq!(
        parse_init_report(&numeric_tool).unwrap_err(),
        organizational_permission_error()
    );
}

//! Organizational Claude launch surface tests.

use super::{TestDirectory, configuration, modes};
use crate::ProviderDriver;
use crate::claude::driver::ClaudeProviderDriver;
use crate::claude::home::resolve_claude_home;
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
    let arguments = launch(&["--verbose"], options(ask(InteractionMode::Build), true)).unwrap();
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
            "--disallowed-tools",
            "Task,Agent,WebFetch,WebSearch,Bash",
            "--verbose",
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
    assert!(
        !direct.iter().any(|argument| {
            argument == "--strict-mcp-config" || argument == "--disallowed-tools"
        })
    );
}

#[test]
fn organizational_launch_refuses_widening_arguments() {
    for flag in ORGANIZATIONAL_PROTECTED_ARGUMENTS {
        for configured in [
            vec![flag.to_string(), "value".to_string()],
            vec![format!("{flag}=value")],
        ] {
            let configured = configured.iter().map(String::as_str).collect::<Vec<_>>();
            let error =
                launch(&configured, options(ask(InteractionMode::Build), true)).unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Validation, "{configured:?}");
            assert_eq!(
                error.message, "This launch argument is not allowed for organizational runs",
                "{configured:?}"
            );
        }
    }
    let similar = launch(
        &["--tools-extra", "--filesystem"],
        options(ask(InteractionMode::Build), true),
    )
    .unwrap();
    assert!(
        similar.ends_with(&["--tools-extra".to_string(), "--filesystem".to_string()]),
        "only the exact flag names are refused"
    );
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

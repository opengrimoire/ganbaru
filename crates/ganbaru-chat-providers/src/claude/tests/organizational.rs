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
const EDIT_SCOPE: ProviderOrganizationalScope = ProviderOrganizationalScope::Folder {
    capability: ProviderFolderCapability::Edit,
};

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
            pre_approved_tools: &[],
            scope: EDIT_SCOPE,
            commands_confined: false,
        }),
        system_prompt: None,
        cli_version: None,
    }
}

fn ask(interaction_mode: InteractionMode) -> TurnModeSnapshot {
    modes(SafetyMode::AskForApproval, interaction_mode)
}

fn launch(configured: &[&str], options: ClaudeLaunchOptions<'_>) -> ChatResult<Vec<String>> {
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
            r#"{"fastMode":false,"permissions":{"blockReadsOutsideWorkingDirectories":true,"blockWritesOutsideWorkingDirectories":true}}"#,
            "--restricted",
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

const NON_COMMAND_SCOPES: [ProviderOrganizationalScope; 3] = [
    ProviderOrganizationalScope::Conversation,
    ProviderOrganizationalScope::Folder {
        capability: ProviderFolderCapability::Read,
    },
    EDIT_SCOPE,
];

const COMMAND_SCOPES: [ProviderOrganizationalScope; 3] = [
    ProviderOrganizationalScope::Folder {
        capability: ProviderFolderCapability::Execute,
    },
    ProviderOrganizationalScope::Folder {
        capability: ProviderFolderCapability::Publish,
    },
    ProviderOrganizationalScope::Scratch,
];

fn scoped_options(
    modes: TurnModeSnapshot,
    scope: ProviderOrganizationalScope,
) -> ClaudeLaunchOptions<'static> {
    let mut options = options(modes, true);
    options.organizational = Some(ClaudeOrganizationalLaunch {
        internal_server_name: SERVER_NAME,
        pre_approved_tools: &[],
        scope,
        commands_confined: false,
    });
    options
}

#[test]
fn organizational_launch_maps_ask_for_approval_to_default() {
    for scope in NON_COMMAND_SCOPES.into_iter().chain(COMMAND_SCOPES) {
        let build = launch(&[], scoped_options(ask(InteractionMode::Build), scope)).unwrap();
        assert_eq!(flag_value(&build, "--permission-mode"), Some("default"));
        let plan = launch(&[], scoped_options(ask(InteractionMode::Plan), scope)).unwrap();
        assert_eq!(flag_value(&plan, "--permission-mode"), Some("plan"));
    }
}

#[test]
fn organizational_launch_without_commands_keeps_every_safety_mode_asking() {
    for scope in NON_COMMAND_SCOPES {
        for safety_mode in [
            SafetyMode::ApproveForMe,
            SafetyMode::FullAccess,
            SafetyMode::Custom,
        ] {
            for (interaction_mode, expected) in [
                (InteractionMode::Build, "default"),
                (InteractionMode::Plan, "plan"),
            ] {
                let arguments = launch(
                    &[],
                    scoped_options(modes(safety_mode, interaction_mode), scope),
                )
                .unwrap();
                assert_eq!(
                    flag_value(&arguments, "--permission-mode"),
                    Some(expected),
                    "{scope:?} {safety_mode:?}"
                );
            }
        }
    }
}

#[test]
fn organizational_launch_with_commands_requires_ask_for_approval() {
    for scope in COMMAND_SCOPES {
        for safety_mode in [
            SafetyMode::ApproveForMe,
            SafetyMode::FullAccess,
            SafetyMode::Custom,
        ] {
            for interaction_mode in [InteractionMode::Build, InteractionMode::Plan] {
                let error = launch(
                    &[],
                    scoped_options(modes(safety_mode, interaction_mode), scope),
                )
                .unwrap_err();
                assert_eq!(error.code, ChatErrorCode::Permission, "{scope:?}");
                assert!(!error.recoverable);
            }
        }
    }
}

#[test]
fn organizational_launch_loads_the_tools_of_each_scope() {
    const BLOCKS: &str = r#""permissions":{"blockReadsOutsideWorkingDirectories":true,"blockWritesOutsideWorkingDirectories":true"#;
    for (scope, tools, disallowed) in [
        (
            ProviderOrganizationalScope::Conversation,
            "Read,Glob,Grep,ExitPlanMode,AskUserQuestion,TodoWrite",
            "Task,Agent,WebFetch,WebSearch,Bash,Edit,Write,NotebookEdit",
        ),
        (
            ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Read,
            },
            "Read,Glob,Grep,ExitPlanMode,AskUserQuestion,TodoWrite",
            "Task,Agent,WebFetch,WebSearch,Bash,Edit,Write,NotebookEdit",
        ),
        (
            EDIT_SCOPE,
            "Read,Edit,Write,Glob,Grep,NotebookEdit,ExitPlanMode,AskUserQuestion,TodoWrite",
            "Task,Agent,WebFetch,WebSearch,Bash",
        ),
        (
            COMMAND_SCOPES[0],
            "Read,Edit,Write,Glob,Grep,NotebookEdit,ExitPlanMode,AskUserQuestion,TodoWrite,Bash",
            "Task,Agent,WebFetch,WebSearch",
        ),
        (
            COMMAND_SCOPES[1],
            "Read,Edit,Write,Glob,Grep,NotebookEdit,ExitPlanMode,AskUserQuestion,TodoWrite,Bash",
            "Task,Agent,WebFetch,WebSearch",
        ),
        (
            COMMAND_SCOPES[2],
            "Read,Edit,Write,Glob,Grep,NotebookEdit,ExitPlanMode,AskUserQuestion,TodoWrite,Bash",
            "Task,Agent,WebFetch,WebSearch",
        ),
    ] {
        let arguments = launch(&[], scoped_options(ask(InteractionMode::Build), scope)).unwrap();
        assert_eq!(flag_value(&arguments, "--tools"), Some(tools), "{scope:?}");
        assert_eq!(
            flag_value(&arguments, "--disallowed-tools"),
            Some(disallowed),
            "{scope:?}"
        );
        let settings = flag_value(&arguments, "--settings").unwrap();
        assert!(settings.contains(BLOCKS), "{scope:?} {settings}");
        let runs_commands = COMMAND_SCOPES.contains(&scope);
        assert_eq!(
            settings.contains(r#""ask":["Bash"]"#),
            runs_commands,
            "{scope:?} {settings}"
        );
        assert_eq!(
            settings.contains(r#""useAutoModeDuringPlan":false"#),
            runs_commands,
            "{scope:?} {settings}"
        );
        assert!(
            flag_value(&arguments, "--allowedTools")
                .is_none_or(|allowed| !allowed.split(',').any(|tool| tool == "Bash")),
            "the shell is never pre-approved"
        );
    }
}

#[test]
fn organizational_launch_never_pre_approves_the_shell() {
    let tools = ["Bash".to_string()];
    for scope in COMMAND_SCOPES {
        let mut options = scoped_options(ask(InteractionMode::Build), scope);
        options.organizational = Some(ClaudeOrganizationalLaunch {
            internal_server_name: SERVER_NAME,
            pre_approved_tools: &tools,
            scope,
            commands_confined: false,
        });
        assert_eq!(
            launch(&[], options).unwrap_err().code,
            ChatErrorCode::Permission,
            "{scope:?}"
        );
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
            "--restricted"
                | "--strict-mcp-config"
                | "--allowedTools"
                | "--tools"
                | "--disallowed-tools"
        )
    }));
}

#[test]
fn organizational_launch_refuses_every_configured_argument() {
    for configured in [
        vec!["--verbose"],
        vec!["--fallback-model", "sonnet"],
        vec!["--tools", "default"],
        vec![
            "--allowedTools",
            "mcp__ganbaru-chat__chat_delete_workspace_file",
        ],
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
        pre_approved_tools: &[],
        scope: EDIT_SCOPE,
        commands_confined: false,
    });
    assert_eq!(
        launch(&[], renamed).unwrap_err().code,
        ChatErrorCode::Permission
    );
}

fn pre_approving(tools: &[String]) -> ClaudeLaunchOptions<'_> {
    let mut options = options(ask(InteractionMode::Build), true);
    options.organizational = Some(ClaudeOrganizationalLaunch {
        internal_server_name: SERVER_NAME,
        pre_approved_tools: tools,
        scope: EDIT_SCOPE,
        commands_confined: false,
    });
    options
}

#[test]
fn organizational_launch_pre_approves_only_the_listed_internal_tools() {
    let tools = [
        "mcp__ganbaru-chat__chat_read_workspace_file".to_string(),
        "mcp__ganbaru-chat__chat_report".to_string(),
    ];
    let arguments = launch(&[], pre_approving(&tools)).unwrap();
    let allowed = arguments
        .iter()
        .position(|argument| argument == "--allowedTools")
        .expect("organizational launches pass the pre-approved tools");
    assert_eq!(
        arguments[allowed + 1],
        "mcp__ganbaru-chat__chat_read_workspace_file,mcp__ganbaru-chat__chat_report"
    );
    assert_eq!(
        arguments[allowed + 2],
        "--tools",
        "the variadic list ends at the built-in tool allowlist: {arguments:?}"
    );
    assert_eq!(
        arguments
            .iter()
            .filter(|argument| *argument == "--allowedTools")
            .count(),
        1
    );
}

#[test]
fn organizational_launch_refuses_pre_approving_anything_but_internal_tools() {
    for tool in [
        "Bash",
        "mcp__other-server__chat_read_workspace_file",
        "mcp__ganbaru-chat__",
        "mcp__ganbaru-chat__chat_read_workspace_file,Bash",
        "mcp__ganbaru-chat__chat read",
    ] {
        let tools = [
            "mcp__ganbaru-chat__chat_report".to_string(),
            tool.to_string(),
        ];
        let error = launch(&[], pre_approving(&tools)).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission, "{tool}");
    }
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
fn claude_support_relays_commands_to_the_person_without_confining_them() {
    let home = TestDirectory::new("home-support");
    let driver = ClaudeProviderDriver::new(configuration(home.path())).unwrap();
    let support = driver.authority_support();
    assert!(support.isolated_conversation && support.internal_host_tools);
    assert!(support.deny_shell && support.read_only_root && support.writable_root);
    assert!(support.network_boundary && support.person_approved_commands);
    assert!(
        !support.confined_commands && !support.classified_publish,
        "the shell is unconfined once approved, and publication is unclassified"
    );
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
        EDIT_SCOPE,
    )
    .unwrap();
}

#[test]
fn organizational_init_requires_a_connected_internal_server() {
    for status in ["pending", "failed", "needs-auth"] {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, status)], &["Read"]),
            SERVER_NAME,
            EDIT_SCOPE,
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
        EDIT_SCOPE,
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
            EDIT_SCOPE,
        ));
    }
}

#[test]
fn organizational_init_rejects_disallowed_tool() {
    for tool in organizational_tools(EDIT_SCOPE).disallowed {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], &["Read", tool]),
            SERVER_NAME,
            EDIT_SCOPE,
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
            EDIT_SCOPE,
        ));
    }
}

#[test]
fn organizational_init_accepts_any_subset_of_the_allowlist() {
    let mut tools: Vec<&str> = organizational_tools(EDIT_SCOPE).allowed.to_vec();
    tools.push("mcp__ganbaru-chat__ping");
    for report in [&tools[..], &["mcp__ganbaru-chat__ping"][..], &[][..]] {
        verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], report),
            SERVER_NAME,
            EDIT_SCOPE,
        )
        .unwrap();
    }
}

#[test]
fn organizational_allowlist_never_names_a_denied_tool() {
    for scope in NON_COMMAND_SCOPES.into_iter().chain(COMMAND_SCOPES) {
        let tools = organizational_tools(scope);
        for tool in tools.allowed {
            assert!(
                !tools.disallowed.contains(tool),
                "{tool} is both allowed and denied in {scope:?}"
            );
        }
    }
}

#[test]
fn organizational_init_rejects_tools_beyond_the_scope() {
    let read = ProviderOrganizationalScope::Folder {
        capability: ProviderFolderCapability::Read,
    };
    for (scope, tool) in [
        (read, "Edit"),
        (read, "Write"),
        (read, "NotebookEdit"),
        (read, "Bash"),
        (ProviderOrganizationalScope::Conversation, "Write"),
        (ProviderOrganizationalScope::Conversation, "Bash"),
        (EDIT_SCOPE, "Bash"),
    ] {
        assert_permission(verify_organizational_init(
            &init_report(vec![server(SERVER_NAME, "connected")], &["Read", tool]),
            SERVER_NAME,
            scope,
        ));
    }
    for scope in COMMAND_SCOPES {
        verify_organizational_init(
            &init_report(
                vec![server(SERVER_NAME, "connected")],
                &["Read", "Edit", "Bash"],
            ),
            SERVER_NAME,
            scope,
        )
        .unwrap();
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

#[test]
fn organizational_launch_requires_the_teammate_floor() {
    let refused = ensure_organizational_version(parse_version("2.1.313").unwrap()).unwrap_err();
    assert_eq!(refused.code, ChatErrorCode::UnsupportedVersion);
    assert!(refused.recoverable);
    assert!(refused.message.contains("2.1.314"), "{}", refused.message);
    assert_eq!(
        crate::registry::refused_version(&refused),
        Some((
            ProviderVersionAdmission::BelowMinimum,
            "2.1.313".to_string()
        ))
    );
    for admitted in ["2.1.314", "2.1.322", "2.2.0"] {
        assert!(
            ensure_organizational_version(parse_version(admitted).unwrap()).is_ok(),
            "{admitted}"
        );
    }
    assert!(
        ensure_supported_version(parse_version("2.1.304").unwrap()).is_ok(),
        "direct threads keep the family minimum"
    );
}

fn policy(
    scope: ProviderOrganizationalScope,
    root: &std::path::Path,
) -> ClaudeOrganizationalPermissions {
    ClaudeOrganizationalPermissions::new(scope, root.to_path_buf(), SERVER_NAME)
}

#[test]
fn organizational_routing_admits_paths_inside_the_working_directory() {
    let root = TestDirectory::new("scope-inside");
    std::fs::create_dir_all(root.path().join("notes")).unwrap();
    std::fs::write(root.path().join("notes/a.md"), "a").unwrap();
    let policy = policy(EDIT_SCOPE, root.path());
    let absolute = root.path().join("notes/a.md").display().to_string();
    for (tool, input) in [
        ("Read", serde_json::json!({ "file_path": "notes/a.md" })),
        ("Read", serde_json::json!({ "file_path": absolute })),
        (
            "Write",
            serde_json::json!({ "file_path": "notes/new/b.md" }),
        ),
        (
            "Edit",
            serde_json::json!({ "file_path": "notes/../notes/a.md" }),
        ),
        (
            "NotebookEdit",
            serde_json::json!({ "notebook_path": "book.ipynb" }),
        ),
        ("Glob", serde_json::json!({ "pattern": "**/*.md" })),
        (
            "Glob",
            serde_json::json!({ "pattern": "*.md", "path": "notes" }),
        ),
        ("Grep", serde_json::json!({ "pattern": "todo" })),
        (
            "Grep",
            serde_json::json!({ "pattern": "todo", "path": null }),
        ),
        ("mcp__ganbaru-chat__ping", serde_json::json!({})),
        ("TodoWrite", serde_json::json!({ "todos": [] })),
    ] {
        assert_eq!(policy.denial(tool, &input), None, "{tool} {input}");
    }
}

#[test]
fn organizational_routing_denies_paths_that_leave_the_working_directory() {
    let parent = TestDirectory::new("scope-outside");
    let root = parent.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(parent.path().join("secret.txt"), "secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(parent.path(), root.join("escape")).unwrap();
    let policy = policy(EDIT_SCOPE, &root);
    let mut denied = vec![
        ("Read", serde_json::json!({ "file_path": "/etc/hostname" })),
        ("Read", serde_json::json!({ "file_path": "../secret.txt" })),
        (
            "Write",
            serde_json::json!({ "file_path": "../outside.txt" }),
        ),
        (
            "Write",
            serde_json::json!({ "file_path": "missing/../../outside.txt" }),
        ),
        ("Edit", serde_json::json!({ "file_path": "~/.bashrc" })),
        ("Edit", serde_json::json!({ "file_path": "" })),
        ("Edit", serde_json::json!({})),
        ("Write", serde_json::json!({ "file_path": 7 })),
        (
            "NotebookEdit",
            serde_json::json!({ "file_path": "book.ipynb" }),
        ),
        ("Grep", serde_json::json!({ "pattern": "x", "path": "/" })),
        ("Grep", serde_json::json!({ "pattern": "x", "path": 1 })),
        ("Glob", serde_json::json!({ "pattern": "../**" })),
        ("Glob", serde_json::json!({ "pattern": "/etc/*" })),
        ("Glob", serde_json::json!({ "pattern": "~/*" })),
        ("Glob", serde_json::json!({})),
    ];
    #[cfg(unix)]
    denied.extend([
        (
            "Read",
            serde_json::json!({ "file_path": "escape/secret.txt" }),
        ),
        (
            "Write",
            serde_json::json!({ "file_path": "escape/new.txt" }),
        ),
    ]);
    for (tool, input) in denied {
        assert!(policy.denial(tool, &input).is_some(), "{tool} {input}");
    }
}

#[test]
fn organizational_routing_denies_tools_outside_the_scope() {
    let root = TestDirectory::new("scope-tools");
    let read = policy(
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Read,
        },
        root.path(),
    );
    for tool in [
        "Edit",
        "Write",
        "NotebookEdit",
        "Bash",
        "WebFetch",
        "Task",
        "Skill",
    ] {
        assert!(
            read.denial(tool, &serde_json::json!({ "file_path": "a.md" }))
                .is_some(),
            "{tool}"
        );
    }
    assert!(
        policy(EDIT_SCOPE, root.path())
            .denial("Bash", &serde_json::json!({ "command": "ls" }))
            .is_some()
    );
    assert!(
        policy(ProviderOrganizationalScope::Conversation, root.path())
            .denial("mcp__other-server__fetch", &serde_json::json!({}))
            .is_some()
    );
    for scope in COMMAND_SCOPES {
        assert_eq!(
            policy(scope, root.path()).denial("Bash", &serde_json::json!({ "command": "ls /" })),
            None,
            "the person decides every command in {scope:?}"
        );
    }
}

fn can_use_tool(tool: &str, input: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "subtype": "can_use_tool",
        "tool_name": tool,
        "input": input,
        "permission_suggestions": [{
            "type": "addRules",
            "rules": [{ "toolName": tool }],
            "behavior": "allow",
            "destination": "session"
        }]
    })
}

#[test]
fn organizational_routing_denies_outside_requests_without_asking() {
    let root = TestDirectory::new("scope-route-deny");
    let policy = policy(EDIT_SCOPE, root.path());
    let request = can_use_tool("Read", serde_json::json!({ "file_path": "/etc/hostname" }));
    let routed = crate::claude::session::route_scoped_control_request(
        "ctrl-1",
        request.as_object().unwrap(),
        Some(&policy),
        ask(InteractionMode::Build),
    )
    .unwrap();
    assert!(routed.pending.is_none());
    let crate::events::CanonicalEvent::Unknown(event) = routed.event else {
        panic!("a denial is not a request");
    };
    assert_eq!(event.source_type, "claude/organizational/denied");
    let response = routed.immediate_response.unwrap();
    assert_eq!(response["response"]["request_id"], "ctrl-1");
    assert_eq!(response["response"]["response"]["behavior"], "deny");
}

#[test]
fn organizational_routing_offers_no_session_grant() {
    let root = TestDirectory::new("scope-route-grant");
    for (scope, tool, input) in [
        (
            COMMAND_SCOPES[0],
            "Bash",
            serde_json::json!({ "command": "ls" }),
        ),
        (
            EDIT_SCOPE,
            "Edit",
            serde_json::json!({ "file_path": "a.md" }),
        ),
    ] {
        let policy = policy(scope, root.path());
        let request = can_use_tool(tool, input);
        let routed = crate::claude::session::route_scoped_control_request(
            "ctrl-2",
            request.as_object().unwrap(),
            Some(&policy),
            ask(InteractionMode::Build),
        )
        .unwrap();
        assert!(routed.immediate_response.is_none(), "{tool}");
        let crate::events::CanonicalEvent::RequestOpened(opened) = routed.event else {
            panic!("{tool} reaches the person");
        };
        assert!(opened.session_grants.is_empty(), "{tool}");
        assert!(
            !opened
                .allowed_decisions
                .iter()
                .any(|option| option.decision_kind == ApprovalDecisionKind::AllowSession),
            "{tool}"
        );
    }
}

#[test]
fn driver_refuses_organizational_authority_without_a_scope() {
    let home = TestDirectory::new("home-no-scope");
    let mut configuration = configuration(home.path());
    configuration.internal_mcp = Some(ProviderInternalMcpConfig {
        name: SERVER_NAME.to_string(),
        url: SERVER_URL.to_string(),
        bearer_token: "test-token".to_string(),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: None,
    });
    let driver = ClaudeProviderDriver::new(configuration).unwrap();
    assert_eq!(
        driver.organizational_scope().unwrap_err(),
        organizational_permission_error()
    );
}

fn confined_options(
    modes: TurnModeSnapshot,
    scope: ProviderOrganizationalScope,
) -> ClaudeLaunchOptions<'static> {
    let mut options = scoped_options(modes, scope);
    if let Some(organizational) = options.organizational.as_mut() {
        organizational.commands_confined = true;
    }
    options
}

#[test]
fn confined_launch_admits_unattended_safety_modes_without_bypassing_permissions() {
    for scope in COMMAND_SCOPES {
        for safety_mode in [
            SafetyMode::AskForApproval,
            SafetyMode::ApproveForMe,
            SafetyMode::FullAccess,
        ] {
            for (interaction_mode, expected) in [
                (InteractionMode::Build, "default"),
                (InteractionMode::Plan, "plan"),
            ] {
                let arguments = launch(
                    &[],
                    confined_options(modes(safety_mode, interaction_mode), scope),
                )
                .unwrap();
                assert_eq!(
                    flag_value(&arguments, "--permission-mode"),
                    Some(expected),
                    "{scope:?} {safety_mode:?}"
                );
                let settings = flag_value(&arguments, "--settings").unwrap();
                assert!(
                    settings.contains("\"ask\":[\"Bash\"]"),
                    "every command still reaches Ganbaru: {settings}"
                );
            }
        }
        let custom = launch(
            &[],
            confined_options(modes(SafetyMode::Custom, InteractionMode::Build), scope),
        )
        .unwrap_err();
        assert_eq!(custom.code, ChatErrorCode::Permission, "{scope:?}");
    }
}

#[test]
fn confined_permission_mode_matches_the_launch() {
    for scope in COMMAND_SCOPES {
        for safety_mode in [SafetyMode::ApproveForMe, SafetyMode::FullAccess] {
            let turn = modes(safety_mode, InteractionMode::Build);
            assert!(organizational_permission_mode(turn, scope, true).is_ok());
            assert!(organizational_permission_mode(turn, scope, false).is_err());
        }
        let custom = modes(SafetyMode::Custom, InteractionMode::Build);
        assert!(organizational_permission_mode(custom, scope, true).is_err());
    }
}

#[test]
fn confined_shell_requires_the_sandbox_floor() {
    let refused = ensure_confined_shell_version(parse_version("2.1.324").unwrap()).unwrap_err();
    assert_eq!(refused.code, ChatErrorCode::UnsupportedVersion);
    assert!(refused.message.contains("2.1.325"), "{}", refused.message);
    for admitted in ["2.1.325", "2.2.0"] {
        assert!(
            ensure_confined_shell_version(parse_version(admitted).unwrap()).is_ok(),
            "{admitted}"
        );
    }
}

fn route_command(
    policy: &ClaudeOrganizationalPermissions,
    turn: TurnModeSnapshot,
) -> crate::claude::session::RoutedClaudeRequest {
    let request = can_use_tool("Bash", serde_json::json!({ "command": "ls" }));
    crate::claude::session::route_scoped_control_request(
        "ctrl-3",
        request.as_object().unwrap(),
        Some(policy),
        turn,
    )
    .unwrap()
}

#[test]
fn confined_commands_run_unattended_only_in_full_access_build_turns() {
    let root = TestDirectory::new("scope-route-confined");
    for scope in COMMAND_SCOPES {
        let confined = policy(scope, root.path()).with_confined_commands(true);
        let routed = route_command(
            &confined,
            modes(SafetyMode::FullAccess, InteractionMode::Build),
        );
        assert!(routed.pending.is_none(), "{scope:?}");
        let crate::events::CanonicalEvent::Unknown(event) = routed.event else {
            panic!("an unattended command opens no request");
        };
        assert_eq!(event.source_type, "claude/organizational/confined_command");
        let response = routed.immediate_response.unwrap();
        assert_eq!(response["response"]["request_id"], "ctrl-3");
        assert_eq!(response["response"]["response"]["behavior"], "allow");
        assert_eq!(
            response["response"]["response"]["updatedInput"]["command"],
            "ls"
        );
        for turn in [
            ask(InteractionMode::Build),
            modes(SafetyMode::ApproveForMe, InteractionMode::Build),
            modes(SafetyMode::FullAccess, InteractionMode::Plan),
        ] {
            let routed = route_command(&confined, turn);
            assert!(routed.immediate_response.is_none(), "{scope:?} {turn:?}");
            assert!(
                matches!(
                    routed.event,
                    crate::events::CanonicalEvent::RequestOpened(_)
                ),
                "{scope:?} {turn:?}"
            );
        }
        let unconfined = route_command(
            &policy(scope, root.path()),
            modes(SafetyMode::FullAccess, InteractionMode::Build),
        );
        assert!(
            matches!(
                unconfined.event,
                crate::events::CanonicalEvent::RequestOpened(_)
            ),
            "an unconfined command always reaches the person"
        );
    }
}

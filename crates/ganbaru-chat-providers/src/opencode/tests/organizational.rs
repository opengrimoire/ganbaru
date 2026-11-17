//! Organizational OpenCode session rules, routing, and launch environment tests.

use super::configuration;
use crate::ProviderDriver;
use crate::events::CanonicalEvent;
use crate::models::*;
use crate::opencode::cli::*;
use crate::opencode::config::OPENCODE_DISABLE_PROJECT_CONFIG_ENVIRONMENT;
use crate::opencode::driver::OpenCodeProviderDriver;
use crate::opencode::normalizer::{OpenCodeEventNormalizer, OpenCodeRouteState};
use crate::opencode::permissions::*;
use serde_json::json;

const SERVER: &str = "ganbaru-chat";
const READ: ProviderOrganizationalScope = ProviderOrganizationalScope::Folder {
    capability: ProviderFolderCapability::Read,
};
const EDIT: ProviderOrganizationalScope = ProviderOrganizationalScope::Folder {
    capability: ProviderFolderCapability::Edit,
};
const EXECUTE: ProviderOrganizationalScope = ProviderOrganizationalScope::Folder {
    capability: ProviderFolderCapability::Execute,
};
const ALL_SCOPES: [ProviderOrganizationalScope; 6] = [
    ProviderOrganizationalScope::Conversation,
    READ,
    EDIT,
    EXECUTE,
    ProviderOrganizationalScope::Folder {
        capability: ProviderFolderCapability::Publish,
    },
    ProviderOrganizationalScope::Scratch,
];

fn modes(safety_mode: SafetyMode, interaction_mode: InteractionMode) -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode,
        interaction_mode,
    }
}

fn build() -> TurnModeSnapshot {
    modes(SafetyMode::AskForApproval, InteractionMode::Build)
}

fn rules(
    scope: ProviderOrganizationalScope,
    modes: TurnModeSnapshot,
) -> Vec<OpenCodePermissionRule> {
    organizational_rules(scope, modes, SERVER, &[], false).unwrap()
}

/// The action OpenCode applies: the last rule whose permission and pattern both match, or ask
/// when none does.
fn decide(
    rules: &[OpenCodePermissionRule],
    permission: &str,
    pattern: &str,
) -> OpenCodePermissionAction {
    rules
        .iter()
        .rev()
        .find(|rule| {
            wildcard_matches(&rule.permission, permission)
                && wildcard_matches(&rule.pattern, pattern)
        })
        .map_or(OpenCodePermissionAction::Ask, |rule| rule.action)
}

/// Rules a permissive project or user configuration would contribute, which OpenCode places
/// before the session rules.
fn permissive_configuration() -> Vec<OpenCodePermissionRule> {
    [
        "*",
        "bash",
        "edit",
        "external_directory",
        "webfetch",
        "evil_tool",
        "task",
    ]
    .into_iter()
    .map(|permission| OpenCodePermissionRule {
        permission: permission.to_string(),
        pattern: "*".to_string(),
        action: OpenCodePermissionAction::Allow,
    })
    .collect()
}

fn with_configuration(session: Vec<OpenCodePermissionRule>) -> Vec<OpenCodePermissionRule> {
    let mut merged = permissive_configuration();
    merged.extend(session);
    merged
}

#[test]
fn organizational_rules_start_denied_and_serialize_the_deny_action() {
    for scope in ALL_SCOPES {
        let rules = rules(scope, build());
        assert_eq!(
            rules[0],
            OpenCodePermissionRule {
                permission: "*".to_string(),
                pattern: "*".to_string(),
                action: OpenCodePermissionAction::Deny,
            },
            "{scope:?}"
        );
    }
    assert_eq!(
        serde_json::to_value(OpenCodePermissionAction::Deny).unwrap(),
        json!("deny")
    );
}

#[test]
fn read_scopes_deny_the_shell_edits_network_and_outside_paths_over_permissive_configuration() {
    for scope in [ProviderOrganizationalScope::Conversation, READ] {
        for safety_mode in [
            SafetyMode::AskForApproval,
            SafetyMode::ApproveForMe,
            SafetyMode::FullAccess,
            SafetyMode::Custom,
        ] {
            let rules =
                with_configuration(rules(scope, modes(safety_mode, InteractionMode::Build)));
            for permission in ["read", "glob", "grep", "list", "todowrite", "question"] {
                assert_eq!(
                    decide(&rules, permission, "src/main.rs"),
                    OpenCodePermissionAction::Allow,
                    "{scope:?} {safety_mode:?} {permission}"
                );
            }
            for (permission, pattern) in [
                ("edit", "src/main.rs"),
                ("bash", "touch marker"),
                ("external_directory", "/home/someone/*"),
                ("webfetch", "https://example.com"),
                ("websearch", "*"),
                ("codesearch", "*"),
                ("task", "general"),
                ("skill", "*"),
                ("lsp", "*"),
                ("plan_exit", "*"),
                ("doom_loop", "*"),
                ("evil_tool", "*"),
                ("evilwrite", "*"),
            ] {
                assert_eq!(
                    decide(&rules, permission, pattern),
                    OpenCodePermissionAction::Deny,
                    "{scope:?} {safety_mode:?} {permission}"
                );
            }
        }
    }
}

#[test]
fn edit_scope_asks_for_workspace_edits_and_denies_metadata_and_the_shell() {
    let edit_rules = with_configuration(rules(EDIT, build()));
    for pattern in [
        "src/main.rs",
        "notes.md",
        "nested/dir/file.txt",
        ".github/workflows/ci.yml",
    ] {
        assert_eq!(
            decide(&edit_rules, "edit", pattern),
            OpenCodePermissionAction::Ask,
            "{pattern}"
        );
    }
    for pattern in [
        ".git",
        ".git/config",
        ".git/hooks/pre-commit",
        "vendor/.git",
        "vendor/.git/info/x",
    ] {
        assert_eq!(
            decide(&edit_rules, "edit", pattern),
            OpenCodePermissionAction::Deny,
            "{pattern}"
        );
    }
    for permission in ["bash", "external_directory", "webfetch", "task"] {
        assert_eq!(
            decide(&edit_rules, permission, "*"),
            OpenCodePermissionAction::Deny,
            "{permission}"
        );
    }
    let plan = with_configuration(rules(
        EDIT,
        modes(SafetyMode::AskForApproval, InteractionMode::Plan),
    ));
    assert_eq!(
        decide(&plan, "edit", "src/main.rs"),
        OpenCodePermissionAction::Deny,
        "Plan turns never edit"
    );
}

#[test]
fn command_scopes_ask_for_every_command_and_require_ask_for_approval() {
    for scope in [
        EXECUTE,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Publish,
        },
        ProviderOrganizationalScope::Scratch,
    ] {
        for interaction_mode in [InteractionMode::Build, InteractionMode::Plan] {
            let rules = with_configuration(rules(
                scope,
                modes(SafetyMode::AskForApproval, interaction_mode),
            ));
            for command in ["touch marker", "git status", "rm -rf /"] {
                assert_eq!(
                    decide(&rules, "bash", command),
                    OpenCodePermissionAction::Ask,
                    "{scope:?} {interaction_mode:?} {command}"
                );
            }
            assert_eq!(
                decide(&rules, "edit", ".git/config"),
                OpenCodePermissionAction::Deny
            );
            assert_eq!(
                decide(&rules, "external_directory", "/tmp/*"),
                OpenCodePermissionAction::Deny
            );
            assert_eq!(
                decide(&rules, "webfetch", "*"),
                OpenCodePermissionAction::Deny
            );
        }
        for safety_mode in [
            SafetyMode::ApproveForMe,
            SafetyMode::FullAccess,
            SafetyMode::Custom,
        ] {
            let error = organizational_rules(
                scope,
                modes(safety_mode, InteractionMode::Build),
                SERVER,
                &[],
                false,
            )
            .unwrap_err();
            assert_eq!(
                error.code,
                ChatErrorCode::Permission,
                "{scope:?} {safety_mode:?}"
            );
        }
    }
}

#[test]
fn internal_tools_ask_unless_pre_approved_and_other_mcp_tools_stay_denied() {
    let pre_approved = vec![
        format!("mcp__{SERVER}__list_referenced_channels"),
        "mcp__other__post".to_string(),
        format!("mcp__{SERVER}__"),
    ];
    let rules = with_configuration(
        organizational_rules(EDIT, build(), SERVER, &pre_approved, false).unwrap(),
    );
    assert_eq!(
        decide(&rules, "ganbaru-chat_list_referenced_channels", "*"),
        OpenCodePermissionAction::Allow
    );
    assert_eq!(
        decide(&rules, "ganbaru-chat_post_reply", "*"),
        OpenCodePermissionAction::Ask
    );
    assert_eq!(
        decide(&rules, "other_post", "*"),
        OpenCodePermissionAction::Deny
    );
    assert_eq!(
        decide(&rules, "ganbaru-chat", "*"),
        OpenCodePermissionAction::Deny
    );
}

#[test]
fn organizational_policy_admits_only_requests_the_scope_needs() {
    let patterns = |values: &[&str]| {
        values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
    };
    let read = OpenCodeOrganizationalPolicy::new(READ, SERVER);
    let edit = OpenCodeOrganizationalPolicy::new(EDIT, SERVER);
    let execute = OpenCodeOrganizationalPolicy::new(EXECUTE, SERVER);
    let plan = modes(SafetyMode::AskForApproval, InteractionMode::Plan);

    assert!(read.admits("ganbaru-chat_post_reply", &[], build()));
    assert!(!read.admits("ganbaru-chat_", &[], build()));
    assert!(!read.admits("edit", &patterns(&["src/a.rs"]), build()));
    assert!(!read.admits("bash", &patterns(&["ls"]), build()));
    assert!(!edit.admits("bash", &patterns(&["ls"]), build()));
    assert!(execute.admits("bash", &patterns(&["ls"]), build()));
    assert!(execute.admits("bash", &patterns(&["ls"]), plan));

    assert!(edit.admits("edit", &patterns(&["src/a.rs", "./b.rs"]), build()));
    assert!(!edit.admits("edit", &patterns(&["src/a.rs"]), plan));
    for outside in [
        "../escape.txt",
        "src/../../escape.txt",
        "/etc/passwd",
        "~/.bashrc",
        ".git/config",
        "sub/.git/hooks/post-checkout",
        "",
    ] {
        assert!(
            !edit.admits("edit", &patterns(&["src/a.rs", outside]), build()),
            "{outside:?}"
        );
    }
    assert!(
        !edit.admits("edit", &[], build()),
        "an edit without a target"
    );
    for permission in [
        "external_directory",
        "webfetch",
        "task",
        "read",
        "other_tool",
        "doom_loop",
    ] {
        assert!(
            !execute.admits(permission, &patterns(&["*"]), build()),
            "{permission}"
        );
    }
}

#[test]
fn organizational_routing_rejects_outside_requests_and_offers_no_session_grants() {
    let normalizer = OpenCodeEventNormalizer::new(
        ProviderInstanceId::new("opencode-instance").unwrap(),
        ChatThreadId::new("thread-fixture").unwrap(),
        ProviderSessionId::new("local-session-fixture").unwrap(),
    );
    let mut state = OpenCodeRouteState::new("ses_fixture".to_string(), build())
        .with_organizational(OpenCodeOrganizationalPolicy::new(EDIT, SERVER));
    state.active_turn_id = Some(ChatTurnId::new("turn-fixture").unwrap());
    let asked = |id: &str, permission: &str, pattern: &str| {
        json!({
            "type": "permission.asked",
            "properties": {
                "id": id,
                "sessionID": "ses_fixture",
                "permission": permission,
                "patterns": [pattern],
                "always": ["*"],
            }
        })
    };

    let outside = normalizer
        .normalize(&mut state, asked("perm_bash", "bash", "touch marker"))
        .unwrap();
    assert!(outside.is_empty(), "no request reaches the person");
    let metadata = normalizer
        .normalize(&mut state, asked("perm_git", "edit", ".git/config"))
        .unwrap();
    assert!(metadata.is_empty());
    assert_eq!(
        state.take_auto_rejections(),
        vec![
            ProviderRequestId::new("perm_bash").unwrap(),
            ProviderRequestId::new("perm_git").unwrap(),
        ]
    );
    assert!(!state.has_permission(&ProviderRequestId::new("perm_bash").unwrap()));
    assert!(!state.awaiting_user());
    let echo = json!({
        "type": "permission.replied",
        "properties": { "sessionID": "ses_fixture", "requestID": "perm_bash", "reply": "reject" }
    });
    assert!(normalizer.normalize(&mut state, echo).unwrap().is_empty());

    state.grant_session_rules(vec![OpenCodeSessionRule {
        permission: "edit".to_string(),
        pattern: None,
    }]);
    let inside = normalizer
        .normalize(&mut state, asked("perm_edit", "edit", "src/a.rs"))
        .unwrap();
    let request = match &inside[..] {
        [event] => match &event.event {
            CanonicalEvent::RequestOpened(request) => request,
            other => panic!("expected an opened request, got {other:?}"),
        },
        other => panic!("expected one event, got {other:?}"),
    };
    assert!(request.session_grants.is_empty());
    assert_eq!(
        request
            .allowed_decisions
            .iter()
            .map(|decision| decision.id.as_str())
            .collect::<Vec<_>>(),
        ["once", "reject"]
    );
    assert!(
        state.take_auto_answers().is_empty(),
        "session rules never answer"
    );
    assert!(state.take_auto_rejections().is_empty());
}

#[test]
fn organizational_server_environment_skips_project_configuration_whatever_the_trust() {
    let mut provider = configuration(json!({ "mode": "local" }));
    provider.environment.insert(
        OPENCODE_DISABLE_PROJECT_CONFIG_ENVIRONMENT.to_string(),
        "0".to_string(),
    );
    provider.environment.insert(
        OPENCODE_CONFIG_CONTENT_ENVIRONMENT.to_string(),
        r#"{"formatter":true}"#.to_string(),
    );
    let direct = server_environment(&provider, ProjectConfigurationAccess::Trusted, None).unwrap();
    assert_eq!(
        direct
            .get(OPENCODE_DISABLE_PROJECT_CONFIG_ENVIRONMENT)
            .map(String::as_str),
        Some("0"),
        "a direct run keeps the configured environment"
    );
    provider.internal_mcp = Some(internal_mcp(Some(EDIT)));
    for access in [
        ProjectConfigurationAccess::Trusted,
        ProjectConfigurationAccess::Restricted,
    ] {
        let environment = server_environment(&provider, access, None).unwrap();
        assert_eq!(
            environment
                .get(OPENCODE_DISABLE_PROJECT_CONFIG_ENVIRONMENT)
                .map(String::as_str),
            Some("1"),
            "{access:?}"
        );
        assert_eq!(
            environment
                .get(OPENCODE_CONFIG_CONTENT_ENVIRONMENT)
                .map(String::as_str),
            Some(organizational_config_content(None).as_str()),
            "{access:?}"
        );
        assert!(
            !environment.contains_key(ganbaru_sandbox::POLICY_FILE_VARIABLE),
            "{access:?}"
        );
    }
    let content: serde_json::Value =
        serde_json::from_str(&organizational_config_content(None)).unwrap();
    assert_eq!(content, json!({ "formatter": false, "lsp": false }));
}

#[test]
fn the_confined_shell_is_merged_into_the_organizational_configuration() {
    let shell = "/usr/lib/ganbaru/ganbaru-sandbox";
    let content: serde_json::Value =
        serde_json::from_str(&organizational_config_content(Some(shell))).unwrap();
    assert_eq!(
        content,
        json!({ "formatter": false, "lsp": false, "shell": shell })
    );
    let quoted: serde_json::Value =
        serde_json::from_str(&organizational_config_content(Some("/a\"b\\c"))).unwrap();
    assert_eq!(quoted["shell"], json!("/a\"b\\c"));
}

#[test]
fn the_configured_environment_cannot_name_the_sandbox_policy_file() {
    let mut provider = configuration(json!({ "mode": "local" }));
    provider.environment.insert(
        ganbaru_sandbox::POLICY_FILE_VARIABLE.to_string(),
        "/tmp/policy".to_string(),
    );
    let error =
        server_environment(&provider, ProjectConfigurationAccess::Trusted, None).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
}

#[test]
fn confined_commands_run_unattended_only_in_full_access() {
    for scope in [
        EXECUTE,
        ProviderOrganizationalScope::Folder {
            capability: ProviderFolderCapability::Publish,
        },
        ProviderOrganizationalScope::Scratch,
    ] {
        let decide_shell = |safety_mode| {
            organizational_rules(
                scope,
                modes(safety_mode, InteractionMode::Build),
                SERVER,
                &[],
                true,
            )
            .map(|rules| decide(&with_configuration(rules), "bash", "rm -rf build"))
        };
        assert_eq!(
            decide_shell(SafetyMode::AskForApproval).unwrap(),
            OpenCodePermissionAction::Ask
        );
        assert_eq!(
            decide_shell(SafetyMode::ApproveForMe).unwrap(),
            OpenCodePermissionAction::Ask
        );
        assert_eq!(
            decide_shell(SafetyMode::FullAccess).unwrap(),
            OpenCodePermissionAction::Allow
        );
        assert_eq!(
            decide_shell(SafetyMode::Custom).unwrap_err().code,
            ChatErrorCode::Permission
        );
        let full = with_configuration(
            organizational_rules(
                scope,
                modes(SafetyMode::FullAccess, InteractionMode::Build),
                SERVER,
                &[],
                true,
            )
            .unwrap(),
        );
        assert_eq!(
            decide(&full, "edit", "src/main.rs"),
            OpenCodePermissionAction::Ask,
            "edits keep asking"
        );
        for permission in ["external_directory", "webfetch", "task"] {
            assert_eq!(
                decide(&full, permission, "*"),
                OpenCodePermissionAction::Deny
            );
        }
    }
    for scope in [READ, EDIT, ProviderOrganizationalScope::Conversation] {
        let rules = with_configuration(
            organizational_rules(
                scope,
                modes(SafetyMode::FullAccess, InteractionMode::Build),
                SERVER,
                &[],
                true,
            )
            .unwrap(),
        );
        assert_eq!(
            decide(&rules, "bash", "ls"),
            OpenCodePermissionAction::Deny,
            "{scope:?}"
        );
    }
}

#[test]
fn organizational_runs_require_the_verified_opencode_version() {
    ensure_organizational_version(&json!({ "healthy": true, "version": "1.19.5" })).unwrap();
    ensure_organizational_version(&json!({ "version": "1.20.0" })).unwrap();
    let old = ensure_organizational_version(&json!({ "version": "1.19.4" })).unwrap_err();
    assert_eq!(old.code, ChatErrorCode::UnsupportedVersion);
    assert_eq!(agent_minimum_version("1.19.4").as_deref(), Some("1.19.5"));
    assert_eq!(agent_minimum_version("1.19.5"), None);
    assert_eq!(agent_minimum_version("dev"), None);
    for health in [
        json!({ "healthy": true }),
        json!({ "version": 1 }),
        json!({ "version": "dev" }),
    ] {
        assert_eq!(
            ensure_organizational_version(&health).unwrap_err().code,
            ChatErrorCode::Permission,
            "{health}"
        );
    }
}

#[test]
fn only_owned_local_servers_report_folder_authority() {
    let local = OpenCodeProviderDriver::new(configuration(json!({ "mode": "local" }))).unwrap();
    let support = local.authority_support();
    assert!(support.isolated_conversation && support.internal_host_tools);
    assert!(support.deny_shell && support.read_only_root && support.writable_root);
    assert!(support.person_approved_commands && support.network_boundary);
    assert!(!support.confined_commands && support.classified_publish);
    let external = OpenCodeProviderDriver::new(configuration(json!({
        "mode": "external",
        "serverUrl": "http://127.0.0.1:4096",
        "confirmExternalWorkspaceAccess": true,
    })))
    .unwrap();
    assert_eq!(
        external.authority_support(),
        crate::ProviderAuthoritySupport::default()
    );
}

pub(super) fn internal_mcp(
    scope: Option<ProviderOrganizationalScope>,
) -> ProviderInternalMcpConfig {
    ProviderInternalMcpConfig {
        name: SERVER.to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "a".repeat(64),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: scope,
    }
}

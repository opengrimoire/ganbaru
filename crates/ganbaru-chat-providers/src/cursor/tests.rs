//! Cursor ACP provider tests.

use super::driver::{AcpProviderFlavor, CursorProviderDriver};
use super::executable::*;
use super::interactions::*;
use super::normalizer::{CursorEventNormalizer, CursorRouteState};
use super::protocol::*;
use super::session::initialize_session;
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use support::*;

mod integration;
mod support;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityMatrix {
    minimum_cursor_version: String,
    installed_local_version_supported: bool,
    protocol_version: u32,
    transport: String,
    required_cases: CompatibilityCases,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityCases {
    authentication: bool,
    session_resume: bool,
    offered_permissions: bool,
    structured_questions: bool,
    native_plan: bool,
    model_traits: bool,
    cancellation: bool,
    unknown_blocks: bool,
}

#[test]
fn compatibility_matrix_pins_the_native_acp_boundary() {
    let matrix: CompatibilityMatrix =
        serde_json::from_str(include_str!("fixtures/compatibility-matrix.json")).unwrap();
    assert_eq!(matrix.minimum_cursor_version, MINIMUM_CURSOR_VERSION);
    assert_eq!(matrix.protocol_version, ACP_PROTOCOL_VERSION);
    assert_eq!(matrix.transport, "native_rust_acp_stdio");
    assert!(!matrix.installed_local_version_supported);
    assert!(matrix.required_cases.authentication);
    assert!(matrix.required_cases.session_resume);
    assert!(matrix.required_cases.offered_permissions);
    assert!(matrix.required_cases.structured_questions);
    assert!(matrix.required_cases.native_plan);
    assert!(matrix.required_cases.model_traits);
    assert!(matrix.required_cases.cancellation);
    assert!(matrix.required_cases.unknown_blocks);

    let metadata = CursorProviderDriver::metadata_read();
    assert_eq!(
        metadata.implementation_status,
        ProviderImplementationStatus::Available
    );
    assert_eq!(
        metadata.minimum_tested_cli_version.as_deref(),
        Some(MINIMUM_CURSOR_VERSION)
    );
}

#[test]
fn available_command_updates_are_typed_bounded_and_deduplicated() {
    let update = json!({
        "availableCommands": [{
            "name": "review",
            "description": "Review changes",
            "input": { "hint": "[scope]" }
        }]
    });
    let commands = parse_available_commands_update(update.as_object().unwrap()).unwrap();
    assert_eq!(commands[0].name, "review");
    assert_eq!(commands[0].argument_hint.as_deref(), Some("[scope]"));
    let duplicate = json!({
        "availableCommands": [
            { "name": "review", "description": "One" },
            { "name": "/REVIEW", "description": "Two" }
        ]
    });
    assert!(parse_available_commands_update(duplicate.as_object().unwrap()).is_err());
}

#[test]
fn executable_probe_parses_versions_accounts_and_protected_arguments() {
    let parsed = parse_about(
        br#"{"cliVersion":"2026.04.08","userEmail":"fixture@example.test"}"#,
        b"",
        true,
    )
    .unwrap();
    assert_eq!(
        parsed.version,
        parse_version(MINIMUM_CURSOR_VERSION).unwrap()
    );
    assert_eq!(
        parsed.account_label.as_deref(),
        Some("fixture@example.test")
    );
    assert_eq!(parsed.authenticated, Some(true));
    assert_eq!(
        ensure_supported_version(parse_version("2026.04.07").unwrap())
            .unwrap_err()
            .code,
        ChatErrorCode::UnsupportedVersion
    );

    let settings = CursorProviderSettings {
        api_endpoint: Some("https://cursor.example.test/api".to_string()),
    };
    assert_eq!(
        launch_arguments(&["--verbose".to_string()], &settings).unwrap(),
        ["--verbose", "-e", "https://cursor.example.test/api", "acp"]
    );
    assert!(launch_arguments(&["--api-key=secret".to_string()], &settings).is_err());
}

#[test]
fn grok_acp_arguments_models_and_questions_preserve_provider_values() {
    assert_eq!(
        grok_launch_arguments(&["--verbose".to_string()]).unwrap(),
        ["--verbose", "agent", "stdio"]
    );
    assert!(grok_launch_arguments(&["agent".to_string()]).is_err());
    assert!(grok_launch_arguments(&["--api-key=secret".to_string()]).is_err());

    let mut fallback_setup =
        parse_session_setup(json!({ "sessionId": "grok-fallback-session" }), true).unwrap();
    ensure_grok_model_state(&mut fallback_setup);
    let fallback_models = parse_acp_models(&fallback_setup).unwrap();
    assert_eq!(fallback_models[0].id.as_str(), "grok-build");
    assert_eq!(fallback_models[0].display_name, "Grok Build");

    let setup = parse_session_setup(
        json!({
            "sessionId": "grok-session",
            "models": {
                "currentModelId": "grok-build",
                "availableModels": [
                    { "modelId": "grok-build", "name": "Grok Build" },
                    { "modelId": "grok-4.5", "name": "Grok 4.5" }
                ]
            }
        }),
        true,
    )
    .unwrap();
    let models = parse_acp_models(&setup).unwrap();
    assert_eq!(models[0].id.as_str(), "grok-build");
    assert_eq!(models[0].display_name, "Grok Build");
    assert_eq!(models[1].id.as_str(), "grok-4.5");

    let (parsed, questions) = parse_xai_question(&json!({
        "method": "_x.ai/ask_user_question",
        "params": {
            "sessionId": "grok-session",
            "toolCallId": "grok-question",
            "mode": "default",
            "questions": [{
                "id": "strategy",
                "question": "Which strategy?",
                "options": [
                    { "id": "focused", "label": "Focused" },
                    { "id": "broad", "label": "Broad" }
                ],
                "multiSelect": false
            }]
        }
    }))
    .unwrap();
    assert_eq!(parsed.request.questions[0].options[0].label, "Focused");
    let result = resolve_question_result(
        &PendingCursorRequest {
            rpc_id: json!(9),
            kind: PendingCursorRequestKind::XaiUserInput { questions },
        },
        &[UserInputAnswer {
            question_id: "strategy".to_string(),
            selected_option_ids: vec!["focused".to_string()],
            free_form_text: None,
        }],
    )
    .unwrap();
    assert_eq!(result["outcome"], "accepted");
    assert_eq!(result["answers"]["Which strategy?"][0], "Focused");
}

#[test]
fn endpoint_and_continuation_validation_bind_server_home_and_account() {
    let home = TestDirectory::new("identity");
    assert!(
        CursorProviderSettings::parse_for(
            &configuration(home.path(), Some("http://localhost:3000")),
            "Cursor"
        )
        .is_ok()
    );
    assert!(
        CursorProviderSettings::parse_for(
            &configuration(home.path(), Some("http://localhost.evil.test")),
            "Cursor"
        )
        .is_err()
    );
    assert!(
        CursorProviderSettings::parse_for(
            &configuration(home.path(), Some("https://user:secret@cursor.example.test")),
            "Cursor"
        )
        .is_err()
    );

    let default = CursorProviderSettings::default();
    let first = continuation_group(
        &configuration(home.path(), None),
        &default,
        Some("first@example.test"),
    )
    .unwrap();
    let second = continuation_group(
        &configuration(home.path(), None),
        &default,
        Some("second@example.test"),
    )
    .unwrap();
    assert_ne!(first, second);
}

#[test]
fn grok_process_environment_uses_the_configured_provider_home() {
    let home = TestDirectory::new("grok-home");
    let mut provider = configuration(home.path(), None);
    provider.family_id = ProviderFamilyId::new("grok").unwrap();
    let environment = process_environment_for(&provider, "Grok").unwrap();
    assert_eq!(
        environment.get("GROK_HOME").map(String::as_str),
        Some(home.path().to_string_lossy().as_ref())
    );
}

#[test]
fn negotiated_capabilities_and_model_traits_are_truthful() {
    let initialize = parse_initialize(initialize_response(false)).unwrap();
    let setup = parse_session_setup(
        session_setup(Some("cursor-session-fixture"), config_options()),
        true,
    )
    .unwrap();
    let capabilities = negotiated_capabilities_for(AcpProviderFlavor::Cursor, &initialize, &setup);
    for capability in [
        ProviderCapability::NativeResume,
        ProviderCapability::NativePlan,
        ProviderCapability::DynamicModelChange,
        ProviderCapability::Images,
    ] {
        assert!(capabilities.supports(capability));
    }
    let models = parse_available_models(models_response()).unwrap();
    let keys = models[0]
        .options
        .iter()
        .map(|option| match option {
            ModelOptionDefinition::Boolean { key, .. }
            | ModelOptionDefinition::Choice { key, .. }
            | ModelOptionDefinition::MultipleChoice { key, .. }
            | ModelOptionDefinition::IntegerRange { key, .. }
            | ModelOptionDefinition::Text { key, .. }
            | ModelOptionDefinition::Unknown { key, .. } => key.as_str(),
        })
        .collect::<Vec<_>>();
    assert_eq!(keys, ["reasoning", "contextWindow", "fastMode", "thinking"]);

    let disabled = negotiated_capabilities_for(
        AcpProviderFlavor::Cursor,
        &parse_initialize(json!({
            "protocolVersion": 1,
            "agentCapabilities": {
                "loadSession": false,
                "promptCapabilities": { "image": false }
            }
        }))
        .unwrap(),
        &parse_session_setup(
            json!({
                "sessionId": "limited",
                "configOptions": [{
                    "id": "thinking",
                    "name": "Thinking",
                    "type": "boolean",
                    "currentValue": false,
                    "options": []
                }]
            }),
            true,
        )
        .unwrap(),
    );
    assert!(!disabled.supports(ProviderCapability::NativeResume));
    assert!(!disabled.supports(ProviderCapability::NativePlan));
    assert!(!disabled.supports(ProviderCapability::DynamicModelChange));
    assert!(!disabled.supports(ProviderCapability::Images));
}

#[test]
fn configuration_validation_rejects_partial_unintended_values() {
    let setup = parse_session_setup(
        session_setup(Some("cursor-session-fixture"), config_options()),
        true,
    )
    .unwrap();
    let valid = resolve_configuration_updates(
        &setup.config_options,
        Some(&ModelId::new("cursor-large".to_string()).unwrap()),
        &[
            ModelOptionSelection {
                key: "reasoning".to_string(),
                value: ModelOptionValue::Choice("high".to_string()),
            },
            ModelOptionSelection {
                key: "fastMode".to_string(),
                value: ModelOptionValue::Boolean(true),
            },
        ],
    )
    .unwrap();
    assert_eq!(valid.len(), 3);
    let invalid = resolve_configuration_updates(
        &setup.config_options,
        None,
        &[
            ModelOptionSelection {
                key: "reasoning".to_string(),
                value: ModelOptionValue::Choice("high".to_string()),
            },
            ModelOptionSelection {
                key: "fastMode".to_string(),
                value: ModelOptionValue::Choice("true".to_string()),
            },
        ],
    )
    .unwrap_err();
    assert_eq!(invalid.field.as_deref(), Some("modelOptions"));
    assert!(
        parse_config_update_response(
            json!({ "configOptions": config_options() }),
            "fast",
            &json!(true)
        )
        .is_err()
    );
}

#[test]
fn ask_permissions_accept_only_verified_workspace_edits() {
    let workspace = TestDirectory::new("permissions");
    let source = workspace.path().join("source.rs");
    std::fs::write(&source, "fn main() {}\n").unwrap();
    let permission = |kind: &str, path: &str| {
        json!({
            "sessionId": "cursor-session-fixture",
            "toolCall": {
                "toolCallId": format!("tool-{kind}"),
                "kind": kind,
                "title": "Provider request",
                "rawInput": { "path": path },
                "locations": [{ "path": path }]
            },
            "options": [
                { "optionId": "allow-session", "name": "Always allow", "kind": "allow_always" },
                { "optionId": "allow-this", "name": "Allow", "kind": "allow_once" },
                { "optionId": "deny-this", "name": "Deny", "kind": "reject_once" }
            ]
        })
    };
    let edit = parse_permission(
        &permission("edit", source.to_string_lossy().as_ref()),
        workspace.path(),
    )
    .unwrap();
    assert_eq!(
        auto_permission_option(&edit, SafetyMode::AskForApproval)
            .unwrap()
            .option_id,
        "allow-this"
    );
    assert_eq!(
        auto_permission_option(&edit, SafetyMode::FullAccess)
            .unwrap()
            .option_id,
        "allow-session"
    );
    let command = parse_permission(
        &permission("execute", source.to_string_lossy().as_ref()),
        workspace.path(),
    )
    .unwrap();
    assert!(auto_permission_option(&command, SafetyMode::AskForApproval).is_none());
    let external = parse_permission(
        &permission("edit", "/tmp/cursor-external-file"),
        workspace.path(),
    )
    .unwrap();
    assert!(auto_permission_option(&external, SafetyMode::AskForApproval).is_none());

    let pending = PendingCursorRequest {
        rpc_id: json!(4),
        kind: PendingCursorRequestKind::Approval {
            options: edit.options,
        },
    };
    let fabricated = ApprovalDecision {
        kind: ApprovalDecisionKind::AllowOnce,
        provider_option_id: Some("invented".to_string()),
        updated_tool_input: None,
    };
    assert!(resolve_approval_result(&pending, &fabricated).is_err());
}

#[cfg(unix)]
#[test]
fn auto_edit_rejects_a_symlink_escape() {
    use std::os::unix::fs::symlink;

    let workspace = TestDirectory::new("symlink-workspace");
    let external = TestDirectory::new("symlink-external");
    symlink(external.path(), workspace.path().join("escape")).unwrap();
    let request = json!({
        "toolCall": {
            "toolCallId": "tool-symlink",
            "kind": "edit",
            "title": "Escaped edit",
            "rawInput": { "path": "escape/new.rs" }
        },
        "options": [
            { "optionId": "allow", "name": "Allow", "kind": "allow_once" },
            { "optionId": "deny", "name": "Deny", "kind": "reject_once" }
        ]
    });
    let parsed = parse_permission(&request, workspace.path()).unwrap();
    assert!(auto_permission_option(&parsed, SafetyMode::AskForApproval).is_none());
}

#[test]
fn structured_questions_are_separate_and_validate_provider_choices() {
    let parsed = parse_question(&json!({
        "toolCallId": "question-tool",
        "title": "Choose strategy",
        "questions": [{
            "id": "strategy",
            "prompt": "Which strategy?",
            "options": [
                { "id": "focused", "label": "Focused" },
                { "id": "broad", "label": "Broad" }
            ],
            "allowMultiple": false
        }]
    }))
    .unwrap();
    let pending = PendingCursorRequest {
        rpc_id: json!(7),
        kind: PendingCursorRequestKind::UserInput {
            questions: parsed.questions,
        },
    };
    let result = resolve_question_result(
        &pending,
        &[UserInputAnswer {
            question_id: "strategy".to_string(),
            selected_option_ids: vec!["focused".to_string()],
            free_form_text: None,
        }],
    )
    .unwrap();
    assert_eq!(result["answers"]["strategy"], "focused");
    assert!(
        resolve_question_result(
            &pending,
            &[UserInputAnswer {
                question_id: "strategy".to_string(),
                selected_option_ids: vec!["invented".to_string()],
                free_form_text: None,
            }]
        )
        .is_err()
    );
}

#[test]
fn compatibility_fixture_normalizes_core_and_cursor_extensions() {
    let workspace = TestDirectory::new("normalizer");
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    let normalizer = CursorEventNormalizer::new_for_provider(
        ProviderInstanceId::new("cursor-instance-1".to_string()).unwrap(),
        ChatThreadId::new("thread-1".to_string()).unwrap(),
        ProviderSessionId::new("session-1".to_string()).unwrap(),
        "cursor",
        "Cursor",
    );
    let setup = parse_session_setup(
        session_setup(Some("cursor-session-fixture"), config_options()),
        true,
    )
    .unwrap();
    let mut state = CursorRouteState::new(
        "cursor-session-fixture".to_string(),
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        None,
        setup.config_options,
        workspace.path().to_path_buf(),
    );
    state.active_turn_id = Some(ChatTurnId::new("turn-1".to_string()).unwrap());
    let mut events = Vec::new();
    for line in include_str!("fixtures/compatibility.jsonl").lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        match value["method"].as_str().unwrap() {
            "session/update" => events.extend(
                normalizer
                    .normalize_session_update(&mut state, value["params"].clone())
                    .unwrap(),
            ),
            "cursor/update_todos" => events.push(
                normalizer
                    .normalize_todos(&state, &value["params"])
                    .unwrap(),
            ),
            method => panic!("unexpected fixture method {method}"),
        }
    }
    events.extend(normalizer.finish_turn(&mut state, "end_turn").unwrap());
    for expected in [
        "content",
        "reasoning",
        "diff",
        "plan",
        "configured",
        "unknown",
        "completed",
    ] {
        assert!(event_kind_present(&events, expected), "missing {expected}");
    }
    assert!(events.iter().all(|event| {
        event.provider_instance_id.as_str() == "cursor-instance-1"
            && event.thread_id.as_str() == "thread-1"
    }));
    assert!(
        events
            .iter()
            .filter(|event| matches!(event.event, CanonicalEvent::ItemStarted(_)))
            .all(|event| event.provider_item_id.is_some())
    );
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("not-canonicalized")
    );
}

#[test]
fn confirmed_resume_not_found_is_narrow() {
    assert!(confirmed_session_not_found(-32004, "Session not found"));
    assert!(confirmed_session_not_found(
        -32602,
        "Conversation does not exist"
    ));
    assert!(!confirmed_session_not_found(-32603, "Session not found"));
    assert!(!confirmed_session_not_found(-32004, "Executable not found"));
}

#[test]
fn session_start_auth_resume_and_configuration_rollback_are_bounded() {
    crate::test_block_on(async {
        let healthy_state = Arc::new(AcpFixtureState::default());
        let mut healthy = fixture_connection(FixtureScenario::Healthy, Arc::clone(&healthy_state));
        let started = initialize_session(
            &healthy,
            "/fixture/workspace",
            None,
            modes(SafetyMode::AskForApproval, InteractionMode::Plan),
            Some(&ModelId::new("cursor-large".to_string()).unwrap()),
            &[ModelOptionSelection {
                key: "reasoning".to_string(),
                value: ModelOptionValue::Choice("high".to_string()),
            }],
            &context("healthy-start"),
        )
        .await
        .unwrap();
        assert_eq!(started.session_id, "cursor-session-fixture");
        assert_eq!(started.models.len(), 2);
        let healthy_received = healthy_state.received();
        let methods = request_methods(&healthy_received);
        assert_eq!(methods[0..3], ["initialize", "authenticate", "session/new"]);
        assert!(methods.contains(&"session/set_config_option"));
        assert_eq!(methods.last(), Some(&"cursor/list_available_models"));
        healthy
            .stop(Duration::from_millis(20), Duration::from_millis(20))
            .await
            .unwrap();

        let auth_state = Arc::new(AcpFixtureState::default());
        let mut auth = fixture_connection(
            FixtureScenario::AuthenticationRequired,
            Arc::clone(&auth_state),
        );
        let auth_error = initialize_session(
            &auth,
            "/fixture/workspace",
            None,
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            None,
            &[],
            &context("auth"),
        )
        .await
        .unwrap_err();
        assert_eq!(auth_error.code, ChatErrorCode::AuthenticationRequired);
        auth.stop(Duration::ZERO, Duration::from_millis(20))
            .await
            .unwrap();

        let resume_state = Arc::new(AcpFixtureState::default());
        let mut resume =
            fixture_connection(FixtureScenario::ResumeMissing, Arc::clone(&resume_state));
        let resume_error = initialize_session(
            &resume,
            "/fixture/workspace",
            Some("missing-session"),
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            None,
            &[],
            &context("resume"),
        )
        .await
        .unwrap_err();
        assert_eq!(resume_error.code, ChatErrorCode::ResumeNotFound);
        resume
            .stop(Duration::ZERO, Duration::from_millis(20))
            .await
            .unwrap();

        let rollback_state = Arc::new(AcpFixtureState::default());
        let mut rollback =
            fixture_connection(FixtureScenario::RejectFastMode, Arc::clone(&rollback_state));
        let rollback_error = initialize_session(
            &rollback,
            "/fixture/workspace",
            None,
            modes(SafetyMode::AskForApproval, InteractionMode::Build),
            Some(&ModelId::new("cursor-large".to_string()).unwrap()),
            &[ModelOptionSelection {
                key: "fastMode".to_string(),
                value: ModelOptionValue::Boolean(true),
            }],
            &context("rollback"),
        )
        .await
        .unwrap_err();
        assert_eq!(rollback_error.field.as_deref(), Some("modelOptions"));
        let received = rollback_state.received();
        let model_values = received
            .iter()
            .filter(|message| {
                message["method"] == "session/set_config_option"
                    && message["params"]["configId"] == "model"
            })
            .map(|message| message["params"]["value"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(model_values, ["cursor-large", "cursor-small"]);
        assert!(!request_methods(&received).contains(&"session/prompt"));
        rollback
            .stop(Duration::ZERO, Duration::from_millis(20))
            .await
            .unwrap();
    });
}

fn event_kind_present(events: &[CanonicalRuntimeEvent], expected: &str) -> bool {
    events.iter().any(|event| match expected {
        "content" => matches!(event.event, CanonicalEvent::ContentDelta(_)),
        "reasoning" => matches!(
            &event.event,
            CanonicalEvent::ContentDelta(delta)
                if delta.stream_kind == ContentStreamKind::ReasoningSummary
        ),
        "diff" => matches!(event.event, CanonicalEvent::DiffUpdated(_)),
        "plan" => matches!(event.event, CanonicalEvent::PlanUpdated(_)),
        "configured" => matches!(event.event, CanonicalEvent::SessionConfigured(_)),
        "unknown" => matches!(event.event, CanonicalEvent::Unknown(_)),
        "completed" => matches!(event.event, CanonicalEvent::TurnCompleted(_)),
        _ => false,
    })
}

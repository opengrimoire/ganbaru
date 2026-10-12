//! Claude provider compatibility and behavior tests.

use super::home::*;
use super::normalizer::{ClaudeEventNormalizer, ClaudeRouteState};
use super::protocol::*;
use super::session::{PendingClaudeRequestKind, route_control_request};
use super::support::diagnostic_confirms_resume_not_found;
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use crate::{DriverCancellation, DriverFuture, DriverOperationContext, ProviderEventSink};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

mod transport;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

pub(super) struct TestDirectory(PathBuf);

impl TestDirectory {
    pub(super) fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-claude-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
pub(super) struct RecordingSink {
    events: Mutex<Vec<CanonicalRuntimeEvent>>,
}

impl RecordingSink {
    pub(super) fn events(&self) -> Vec<CanonicalRuntimeEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl ProviderEventSink for RecordingSink {
    fn emit<'a>(&'a self, event: CanonicalRuntimeEvent) -> DriverFuture<'a, ()> {
        Box::pin(async move {
            self.events.lock().unwrap().push(event);
            Ok(())
        })
    }
}

pub(super) fn context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + Duration::from_secs(2),
        cancellation: DriverCancellation::default(),
    }
}

#[test]
fn initialize_commands_become_bounded_slash_catalog_entries() {
    let entries = prompt_catalog(&[
        ClaudeCommand {
            name: "compact".to_string(),
            description: Some("Compact context".to_string()),
            argument_hint: Some("[focus]".to_string()),
        },
        ClaudeCommand {
            name: "/compact".to_string(),
            description: Some("Duplicate".to_string()),
            argument_hint: None,
        },
    ]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].value, "/compact");
    assert_eq!(entries[0].argument_hint.as_deref(), Some("[focus]"));
}

#[test]
fn slash_command_message_is_bounded_and_uses_sdk_user_input_shape() {
    let message = build_slash_command_message("/compact").unwrap();
    assert_eq!(
        message
            .pointer("/message/content/0/text")
            .and_then(Value::as_str),
        Some("/compact")
    );
    assert!(build_slash_command_message("compact").is_err());
    assert!(build_slash_command_message("/compact\nnext").is_err());
}

#[test]
fn compact_boundary_is_a_thread_level_activity() {
    let normalizer = ClaudeEventNormalizer::new(
        ProviderInstanceId::new("claude-instance-1".to_string()).unwrap(),
        ChatThreadId::new("thread-1".to_string()).unwrap(),
        ProviderSessionId::new("session-1".to_string()).unwrap(),
    );
    let mut state = route_state();
    state.active_chat_turn_id = Some(ChatTurnId::new("turn-1".to_string()).unwrap());
    let events = normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "system",
                "subtype": "compact_boundary",
                "compact_metadata": { "trigger": "manual" }
            }),
        )
        .unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].turn_id.is_none());
    let CanonicalEvent::ItemCompleted(item) = &events[0].event else {
        panic!("expected completed compaction item");
    };
    assert_eq!(item.kind, CanonicalItemKind::ContextCompaction);
}

pub(super) fn modes(
    safety_mode: SafetyMode,
    interaction_mode: InteractionMode,
) -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode,
        interaction_mode,
    }
}

pub(super) fn configuration(home: &Path) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "claude-instance-1",
        "familyId": "claude",
        "label": "Claude",
        "enabled": true,
        "executable": "claude",
        "providerHome": home,
        "launchArguments": [],
        "environment": {},
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": {
            "schemaVersion": 1,
            "value": {
                "allowCustomModels": false,
                "customModelIds": [],
                "customModelLabels": {}
            }
        }
    }))
    .unwrap()
}

fn route_state() -> ClaudeRouteState {
    ClaudeRouteState::new(
        modes(SafetyMode::AskForApproval, InteractionMode::Build),
        None,
        ClaudeResumeCursor {
            session_uuid: "11111111-1111-4111-8111-111111111111".to_string(),
            last_assistant_uuid: None,
            turn_count: 0,
        },
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityMatrix {
    minimum_claude_code_version: String,
    installed_local_version_supported: bool,
    required_cases: CompatibilityCases,
    decision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityCases {
    partial_messages: bool,
    session_uuid: bool,
    resume: bool,
    approval_callback: bool,
    structured_questions: bool,
    interrupt: bool,
    model_change: bool,
    native_plan: bool,
}

#[test]
fn compatibility_matrix_selects_native_transport_only_for_complete_protocol() {
    let matrix: CompatibilityMatrix =
        serde_json::from_str(include_str!("fixtures/compatibility-matrix.json")).unwrap();
    assert!(
        ensure_supported_version(parse_version(&matrix.minimum_claude_code_version).unwrap())
            .is_ok()
    );
    assert!(matrix.required_cases.partial_messages);
    assert!(matrix.required_cases.session_uuid);
    assert!(matrix.required_cases.resume);
    assert!(matrix.required_cases.approval_callback);
    assert!(matrix.required_cases.structured_questions);
    assert!(matrix.required_cases.interrupt);
    assert!(matrix.required_cases.model_change);
    assert!(matrix.required_cases.native_plan);
    assert_eq!(matrix.decision, "native_rust_stdio");
    assert!(!matrix.installed_local_version_supported);
    assert!(ensure_supported_version(parse_version("1.0.92").unwrap()).is_err());
}

#[test]
fn native_model_metadata_resolves_names_and_supported_effort_levels() {
    let models: Vec<ClaudeModel> = serde_json::from_value(json!([{
        "value": "default",
        "resolvedModel": "claude-opus-4-8",
        "displayName": "Default (recommended)",
        "description": "Use the default model (currently Opus 4.8)",
        "supportsEffort": true,
        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"],
        "supportsFastMode": true
    }, {
        "value": "sonnet",
        "resolvedModel": "claude-sonnet-5",
        "displayName": "Sonnet",
        "description": "Sonnet 5 · Efficient for routine tasks",
        "supportsEffort": true,
        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]
    }, {
        "value": "haiku",
        "resolvedModel": "claude-haiku-4-5-20251001",
        "displayName": "Haiku",
        "description": "Haiku 4.5 · Fastest for quick answers",
        "supportsEffort": false,
        "supportedEffortLevels": []
    }]))
    .unwrap();

    let models = provider_models(models, &[], &BTreeMap::new()).unwrap();

    assert_eq!(models[0].display_name, "Opus 4.8");
    assert_eq!(models[1].display_name, "Sonnet 5");
    assert_eq!(models[2].display_name, "Haiku 4.5");
    let ModelOptionDefinition::Choice {
        options,
        default_value,
        ..
    } = &models[0].options[0]
    else {
        panic!("Claude effort metadata must remain a choice");
    };
    assert_eq!(
        options
            .iter()
            .map(|option| option.value.as_str())
            .collect::<Vec<_>>(),
        vec!["low", "medium", "high", "xhigh", "max"]
    );
    assert_eq!(default_value.as_deref(), Some("high"));
    assert!(matches!(
        models[0].options.get(1),
        Some(ModelOptionDefinition::Boolean {
            key,
            default_value: Some(false),
            ..
        }) if key == "fastMode"
    ));
    assert_eq!(models[1].options.len(), 1);
    assert!(models[2].options.is_empty());
}

#[test]
fn custom_model_names_do_not_infer_fast_mode_support() {
    let models = provider_models(
        Vec::new(),
        &["claude-opus-9".to_string()],
        &BTreeMap::from([(
            "claude-opus-9".to_string(),
            "Default (研究用 Opus 9)".to_string(),
        )]),
    )
    .unwrap();

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].display_name, "Default (研究用 Opus 9)");
    assert!(models[0].options.iter().all(|definition| {
        !matches!(
            definition,
            ModelOptionDefinition::Boolean { key, .. } if key == "fastMode"
        )
    }));
}

#[test]
fn native_model_routes_keep_distinct_capabilities_despite_matching_labels() {
    let input: Vec<ClaudeModel> = serde_json::from_value(json!([{
        "value": "default",
        "displayName": "Opus 9",
        "supportsFastMode": true
    }, {
        "value": "opus",
        "displayName": "Opus 9",
        "supportsFastMode": false
    }, {
        "value": "claude-opus-9",
        "displayName": "Opus 9"
    }]))
    .unwrap();
    let models = provider_models(input, &[], &BTreeMap::new()).unwrap();

    assert_eq!(
        models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["default", "opus", "claude-opus-9"]
    );
    let fast_modes = models
        .iter()
        .map(|model| {
            model.options.iter().any(|option| {
                matches!(option, ModelOptionDefinition::Boolean { key, .. } if key == "fastMode")
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(fast_modes, [true, false, false]);
}

#[test]
fn fast_mode_selection_accepts_only_boolean_values() {
    assert_eq!(
        selected_fast_mode(&[ModelOptionSelection {
            key: "fastMode".to_string(),
            value: ModelOptionValue::Boolean(true),
        }])
        .unwrap(),
        Some(true)
    );
    assert!(
        selected_fast_mode(&[ModelOptionSelection {
            key: "fastMode".to_string(),
            value: ModelOptionValue::Choice("fast".to_string()),
        }])
        .is_err()
    );
}

#[test]
fn launch_arguments_preserve_native_safety_and_resume_semantics() {
    assert_eq!(
        permission_mode(modes(SafetyMode::AskForApproval, InteractionMode::Build)),
        Some("acceptEdits")
    );
    assert_eq!(
        permission_mode(modes(SafetyMode::ApproveForMe, InteractionMode::Build)),
        Some("auto")
    );
    assert_eq!(
        permission_mode(modes(SafetyMode::Custom, InteractionMode::Build)),
        None
    );

    let fresh = launch_arguments(
        Vec::new(),
        &[],
        ClaudeLaunchOptions {
            fresh_session_uuid: Some("11111111-1111-4111-8111-111111111111"),
            resume_session_uuid: None,
            last_assistant_uuid: None,
            model: Some(&ModelId::new("claude-opus-4-8".to_string()).unwrap()),
            effort: Some("high"),
            fast_mode: Some(true),
            modes: modes(SafetyMode::FullAccess, InteractionMode::Build),
        },
    )
    .unwrap();
    assert!(
        fresh
            .windows(2)
            .any(|pair| pair == ["--session-id", "11111111-1111-4111-8111-111111111111"])
    );
    assert!(
        fresh
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "bypassPermissions"])
    );
    assert!(
        fresh
            .iter()
            .any(|value| value == "--allow-dangerously-skip-permissions")
    );
    assert!(fresh.windows(2).any(|pair| pair == ["--effort", "high"]));
    assert!(
        fresh
            .windows(2)
            .any(|pair| pair == ["--settings", r#"{"fastMode":true}"#])
    );

    let custom = launch_arguments(
        Vec::new(),
        &[],
        ClaudeLaunchOptions {
            fresh_session_uuid: Some("33333333-3333-4333-8333-333333333333"),
            resume_session_uuid: None,
            last_assistant_uuid: None,
            model: None,
            effort: None,
            fast_mode: None,
            modes: modes(SafetyMode::Custom, InteractionMode::Build),
        },
    )
    .unwrap();
    assert!(
        !custom
            .iter()
            .any(|argument| argument == "--permission-mode")
    );

    let resumed = launch_arguments(
        Vec::new(),
        &[],
        ClaudeLaunchOptions {
            fresh_session_uuid: None,
            resume_session_uuid: Some("11111111-1111-4111-8111-111111111111"),
            last_assistant_uuid: Some("22222222-2222-4222-8222-222222222222"),
            model: None,
            effort: None,
            fast_mode: None,
            modes: modes(SafetyMode::AskForApproval, InteractionMode::Plan),
        },
    )
    .unwrap();
    assert!(
        resumed
            .windows(2)
            .any(|pair| pair == ["--resume", "11111111-1111-4111-8111-111111111111"])
    );
    assert!(resumed.windows(2).any(|pair| pair
        == [
            "--resume-session-at",
            "22222222-2222-4222-8222-222222222222"
        ]));
    assert!(
        resumed
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "plan"])
    );
}

#[test]
fn protected_transport_arguments_cannot_be_overridden() {
    let error = launch_arguments(
        Vec::new(),
        &["--permission-mode=bypassPermissions".to_string()],
        ClaudeLaunchOptions {
            fresh_session_uuid: Some("11111111-1111-4111-8111-111111111111"),
            resume_session_uuid: None,
            last_assistant_uuid: None,
            model: None,
            effort: None,
            fast_mode: None,
            modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Validation);
}

#[test]
fn continuation_group_changes_with_claude_config_directory() {
    let first = TestDirectory::new("home-first");
    let second = TestDirectory::new("home-second");
    let first_group = resolve_claude_home(&configuration(first.path()))
        .unwrap()
        .continuation_group()
        .unwrap();
    let second_group = resolve_claude_home(&configuration(second.path()))
        .unwrap()
        .continuation_group()
        .unwrap();
    assert_ne!(first_group, second_group);
}

#[test]
fn fixture_normalizes_partial_text_cursor_plan_usage_cost_and_completion() {
    let normalizer = ClaudeEventNormalizer::new(
        ProviderInstanceId::new("claude-instance-1".to_string()).unwrap(),
        ChatThreadId::new("thread-1".to_string()).unwrap(),
        ProviderSessionId::new("session-1".to_string()).unwrap(),
    );
    let mut state = route_state();
    state.active_chat_turn_id = Some(ChatTurnId::new("turn-1".to_string()).unwrap());
    let mut events = Vec::new();
    for line in include_str!("fixtures/compatibility.jsonl").lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        if value.get("type").and_then(Value::as_str) != Some("control_request") {
            events.extend(normalizer.normalize_message(&mut state, value).unwrap());
        }
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, CanonicalEvent::ContentDelta(_)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, CanonicalEvent::PlanUpdated(_)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, CanonicalEvent::McpStatus(_)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, CanonicalEvent::ThreadUsageUpdated(_)))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.event, CanonicalEvent::TurnCompleted(_)))
    );
    assert_eq!(
        state.resume_cursor.last_assistant_uuid.as_deref(),
        Some("22222222-2222-4222-8222-222222222222")
    );
    assert_eq!(state.resume_cursor.turn_count, 1);
}

#[test]
fn approval_question_and_exit_plan_are_distinct_interactions() {
    let requests = include_str!("fixtures/compatibility.jsonl")
        .lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).unwrap();
            (value.get("type").and_then(Value::as_str) == Some("control_request")).then_some(value)
        })
        .collect::<Vec<_>>();
    let mut routed = Vec::new();
    for value in requests {
        let id = value.get("request_id").and_then(Value::as_str).unwrap();
        let request = value.get("request").and_then(Value::as_object).unwrap();
        routed.push(route_control_request(id, request).unwrap());
    }
    assert!(matches!(
        routed[0].pending.as_ref().unwrap().kind,
        PendingClaudeRequestKind::Approval { .. }
    ));
    assert!(matches!(
        routed[1].pending.as_ref().unwrap().kind,
        PendingClaudeRequestKind::UserInput { .. }
    ));
    assert!(matches!(
        routed[1].event,
        CanonicalEvent::UserInputRequested(_)
    ));
    assert!(routed[2].pending.is_none());
    assert!(routed[2].immediate_response.is_some());
    assert!(matches!(
        routed[2].event,
        CanonicalEvent::ProposedPlanCompleted(_)
    ));

    let without_session_scope = route_control_request(
        "approval-once",
        json!({
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": { "command": "printf redacted" }
        })
        .as_object()
        .unwrap(),
    )
    .unwrap();
    let CanonicalEvent::RequestOpened(opened) = without_session_scope.event else {
        panic!("expected an approval request");
    };
    assert!(
        !opened
            .allowed_decisions
            .iter()
            .any(|option| option.decision_kind == ApprovalDecisionKind::AllowSession)
    );
}

#[test]
fn tool_input_completion_waits_for_the_correlated_tool_result() {
    let normalizer = ClaudeEventNormalizer::new(
        ProviderInstanceId::new("claude-instance-1".to_string()).unwrap(),
        ChatThreadId::new("thread-1".to_string()).unwrap(),
        ProviderSessionId::new("session-1".to_string()).unwrap(),
    );
    let mut state = route_state();
    state.active_chat_turn_id = Some(ChatTurnId::new("turn-1".to_string()).unwrap());
    let started = normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "stream_event",
                "event": {
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "tool-bash",
                        "name": "Bash",
                        "input": {}
                    }
                }
            }),
        )
        .unwrap();
    let stopped = normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "stream_event",
                "event": { "type": "content_block_stop", "index": 0 }
            }),
        )
        .unwrap();
    let completed = normalizer
        .normalize_message(
            &mut state,
            json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "tool-bash",
                        "content": "redacted output"
                    }]
                }
            }),
        )
        .unwrap();

    assert!(matches!(started[0].event, CanonicalEvent::ItemStarted(_)));
    assert!(stopped.is_empty());
    assert!(completed.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::ItemCompleted(item)
            if item.kind == CanonicalItemKind::CommandExecution
    )));
}

#[test]
fn resume_not_found_classifier_requires_session_specific_evidence() {
    assert!(diagnostic_confirms_resume_not_found(
        "Claude session 1111 was not found"
    ));
    assert!(!diagnostic_confirms_resume_not_found(
        "Executable was not found"
    ));
    assert!(!diagnostic_confirms_resume_not_found(
        "Session transport disconnected"
    ));
}

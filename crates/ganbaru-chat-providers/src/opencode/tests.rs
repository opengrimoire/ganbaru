//! OpenCode protocol tests that use fixtures and fake executables instead of an installed OpenCode.

use super::cli::*;
use super::config::*;
use super::event_stream::*;
use super::http_client::*;
use super::local_server::*;
use super::normalizer::*;
use super::permissions::*;
use super::protocol::*;
use super::support::{capabilities, capability_kinds, prompt, provider_command};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;

mod driver;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityMatrix {
    minimum_open_code_version: String,
    installed_local_version_supported: bool,
    transport: String,
    source_commit: String,
    required_cases: CompatibilityCases,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CompatibilityCases {
    local_lifecycle: bool,
    external_ownership: bool,
    authentication: bool,
    session_resume: bool,
    native_plan: bool,
    directory_fork: bool,
    permissions: bool,
    structured_questions: bool,
    event_reconnect: bool,
    abort: bool,
    revert: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenApiCompatibilityArtifact {
    open_code_version: String,
    source_commit: String,
    open_api_version: String,
    document_sha256: String,
    operations: Vec<String>,
}

#[test]
fn compatibility_matrix_pins_the_supported_http_boundary() {
    let matrix: CompatibilityMatrix =
        serde_json::from_str(include_str!("fixtures/compatibility-matrix.json")).unwrap();
    assert_eq!(matrix.minimum_open_code_version, MINIMUM_OPENCODE_VERSION);
    assert!(!matrix.installed_local_version_supported);
    assert_eq!(matrix.transport, "rust_http_sse");
    assert_eq!(matrix.source_commit.len(), 40);
    assert!(matrix.required_cases.local_lifecycle);
    assert!(matrix.required_cases.external_ownership);
    assert!(matrix.required_cases.authentication);
    assert!(matrix.required_cases.session_resume);
    assert!(matrix.required_cases.native_plan);
    assert!(matrix.required_cases.directory_fork);
    assert!(matrix.required_cases.permissions);
    assert!(matrix.required_cases.structured_questions);
    assert!(matrix.required_cases.event_reconnect);
    assert!(matrix.required_cases.abort);
    assert!(matrix.required_cases.revert);
}

#[test]
fn openapi_artifact_covers_the_stable_provider_surface() {
    let artifact: OpenApiCompatibilityArtifact =
        serde_json::from_str(include_str!("compat/openapi-1.14.19.json")).unwrap();
    assert_eq!(artifact.open_code_version, MINIMUM_OPENCODE_VERSION);
    assert_eq!(artifact.source_commit.len(), 40);
    assert_eq!(artifact.open_api_version, "3.1.1");
    assert_eq!(artifact.document_sha256.len(), 64);
    for operation in [
        "/agent get",
        "/command get",
        "/formatter get",
        "/lsp get",
        "/mcp post",
        "/permission/{requestID}/reply post",
        "/question/{requestID}/reply post",
        "/session/{sessionID}/fork post",
        "/session/{sessionID}/revert post",
    ] {
        assert!(artifact.operations.iter().any(|entry| entry == operation));
    }
}

#[test]
fn server_origin_policy_requires_tls_or_an_explicit_override() {
    assert!(OpenCodeProviderSettings::parse(&configuration(json!({}))).is_err());
    assert_eq!(
        settings(json!({
            "mode": "local",
            "endpoint": "https://stale.example.test"
        }))
        .connection,
        OpenCodeConnectionMode::Local
    );
    assert!(
        OpenCodeProviderSettings::parse(&configuration(json!({
            "mode": "external",
            "endpoint": "https://unsupported.example.test"
        })))
        .is_err()
    );
    assert!(
        OpenCodeProviderSettings::parse(&configuration(json!({
            "mode": "external"
        })))
        .is_err()
    );
    assert_eq!(
        settings(json!({
            "mode": "external",
            "serverUrl": "HTTP://LOCALHOST:80/",
            "confirmExternalWorkspaceAccess": true
        }))
        .connection,
        OpenCodeConnectionMode::External {
            origin: "http://localhost".to_string(),
            insecure_http: false,
        }
    );
    assert_eq!(
        settings(json!({
            "mode": "external",
            "serverUrl": "https://Example.Test:443",
            "confirmExternalWorkspaceAccess": true
        }))
        .connection,
        OpenCodeConnectionMode::External {
            origin: "https://example.test".to_string(),
            insecure_http: false,
        }
    );
    assert!(
        OpenCodeProviderSettings::parse(&configuration(json!({
            "mode": "external",
            "serverUrl": "https://confirmation-required.example.test"
        })))
        .is_err()
    );
    assert!(
        OpenCodeProviderSettings::parse(&configuration(json!({
            "mode": "external",
            "serverUrl": "http://example.test"
        })))
        .is_err()
    );
    assert_eq!(
        settings(json!({
            "mode": "external",
            "serverUrl": "http://example.test:8080",
            "allowInsecureExternalHttp": true,
            "confirmExternalWorkspaceAccess": true
        }))
        .connection,
        OpenCodeConnectionMode::External {
            origin: "http://example.test:8080".to_string(),
            insecure_http: true,
        }
    );
    for invalid in [
        "ftp://example.test",
        "https://user:secret@example.test",
        "https://example.test/api",
        "https://example.test?token=secret",
        "http://localhost.evil.test",
    ] {
        assert!(
            OpenCodeProviderSettings::parse(&configuration(json!({
                "mode": "external",
                "serverUrl": invalid
            })))
            .is_err()
        );
    }
}

#[test]
fn password_is_extracted_and_redacted_from_driver_configuration() {
    let mut configuration = configuration(json!({
        "mode": "external",
        "serverUrl": "https://example.test",
        "confirmExternalWorkspaceAccess": true
    }));
    configuration.environment.insert(
        OPENCODE_PASSWORD_ENVIRONMENT.to_string(),
        "sentinel-secret".to_string(),
    );
    let password = take_server_password(&mut configuration).unwrap();
    assert_eq!(password.expose(), "sentinel-secret");
    assert_eq!(format!("{password:?}"), "OpenCodeSecret([REDACTED])");
    assert!(
        !configuration
            .environment
            .contains_key(OPENCODE_PASSWORD_ENVIRONMENT)
    );
    assert!(
        !serde_json::to_string(&configuration)
            .unwrap()
            .contains("sentinel-secret")
    );
}

#[test]
fn process_environment_uses_the_configured_opencode_directory() {
    let home = TestDirectory::new("config-directory");
    let mut provider = configuration(json!({ "mode": "local" }));
    provider.provider_home = Some(home.path().to_string_lossy().into_owned());
    let environment = process_environment(&provider).unwrap();
    assert_eq!(
        environment.get("OPENCODE_CONFIG_DIR").map(String::as_str),
        Some(home.path().to_string_lossy().as_ref())
    );
}

#[test]
fn typed_http_scopes_requests_and_redacts_authorization_failures() {
    crate::test_block_on(async {
        let fixture = HttpFixture::start(
            "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 30\r\nConnection: close\r\n\r\n{\"name\":\"AuthenticationError\"}",
        );
        let workspace = TestDirectory::new("http-auth");
        let mut configuration = configuration(json!({ "mode": "local" }));
        configuration.environment.insert(
            OPENCODE_PASSWORD_ENVIRONMENT.to_string(),
            "sentinel-secret".to_string(),
        );
        let password = take_server_password(&mut configuration).unwrap();
        let client =
            OpenCodeHttpClient::new(&fixture.origin, workspace.path(), Some(&password)).unwrap();
        let error = client
            .create_session(Some(&permission_rules(SafetyMode::AskForApproval)))
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::AuthenticationRequired);
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("sentinel-secret")
        );

        let request = fixture.request();
        assert!(request.starts_with("POST /session?directory="));
        assert!(request.contains("authorization: Basic b3BlbmNvZGU6c2VudGluZWwtc2VjcmV0\r\n"));
        assert!(request.contains("content-type: application/json\r\n"));
        assert!(request.contains("\"permission\""));
        assert!(request.contains("\"question\""));
    });
}

#[test]
fn typed_http_reads_history_cursor_and_uses_exact_prompt_endpoint() {
    crate::test_block_on(async {
        let history = HttpFixture::start(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Next-Cursor: cursor-two\r\nContent-Length: 23\r\nConnection: close\r\n\r\n[{\"info\":{\"id\":\"one\"}}]",
        );
        let workspace = TestDirectory::new("http-history");
        let client = OpenCodeHttpClient::new(&history.origin, workspace.path(), None).unwrap();
        let page = client
            .messages("ses_one", 50, Some("cursor-one"))
            .await
            .unwrap();
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("cursor-two"));
        let request = history.request();
        assert!(request.starts_with("GET /session/ses_one/message?directory="));
        assert!(request.contains("&limit=50&before=cursor-one HTTP/1.1\r\n"));

        let prompt = HttpFixture::start(
            "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let client = OpenCodeHttpClient::new(&prompt.origin, workspace.path(), None).unwrap();
        client
            .prompt_async(
                "ses_one",
                &OpenCodePrompt {
                    model: Some(OpenCodePromptModel {
                        provider_id: "anthropic".to_string(),
                        model_id: "claude-sonnet".to_string(),
                    }),
                    agent: Some("build".to_string()),
                    variant: Some("high".to_string()),
                    system: None,
                    parts: vec![OpenCodePromptPart::Text {
                        text: "Hello".to_string(),
                    }],
                },
            )
            .await
            .unwrap();
        let request = prompt.request();
        assert!(request.starts_with("POST /session/ses_one/prompt_async?directory="));
        assert!(request.contains("\"providerId\":\"anthropic\""));
        assert!(request.contains("\"modelId\":\"claude-sonnet\""));
        assert!(request.contains("\"variant\":\"high\""));

        let command = HttpFixture::start(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
        let client = OpenCodeHttpClient::new(&command.origin, workspace.path(), None).unwrap();
        client
            .execute_command("ses_one", "release", "next")
            .await
            .unwrap();
        let request = command.request();
        assert!(request.starts_with("POST /session/ses_one/command?directory="));
        assert!(request.contains("\"command\":\"release\""));
        assert!(request.contains("\"arguments\":\"next\""));

        let lsp = HttpFixture::start(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n[{\"id\":\"rust\"}]",
        );
        let client = OpenCodeHttpClient::new(&lsp.origin, workspace.path(), None).unwrap();
        assert_eq!(
            client.lsp_status().await.unwrap().as_array().unwrap().len(),
            1
        );
        assert!(lsp.request().starts_with("GET /lsp?directory="));

        let formatter = HttpFixture::start(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        );
        let client = OpenCodeHttpClient::new(&formatter.origin, workspace.path(), None).unwrap();
        assert!(client.formatter_status().await.unwrap().is_object());
        assert!(formatter.request().starts_with("GET /formatter?directory="));
    });
}

#[test]
fn event_stream_decoder_handles_chunks_replay_fields_and_bounds() {
    let mut decoder = OpenCodeSseDecoder::default();
    assert!(
        decoder
            .feed(b": heartbeat\r\nid: ignored\r\ndata: {\"type\":\"server.")
            .unwrap()
            .is_empty()
    );
    let events = decoder
        .feed(b"connected\",\"properties\":{}}\r\n\r\n")
        .unwrap();
    assert_eq!(
        events,
        vec![json!({ "type": "server.connected", "properties": {} })]
    );

    let mut multiline = OpenCodeSseDecoder::default();
    assert_eq!(
        multiline
            .feed(b"data: {\"type\":\"server.connected\",\ndata: \"properties\":{}}\n\n")
            .unwrap(),
        vec![json!({ "type": "server.connected", "properties": {} })]
    );
    assert!(
        OpenCodeSseDecoder::default()
            .feed(&vec![b'x'; MAX_EVENT_DATA_BYTES + 64 * 1024 + 1])
            .is_err()
    );
}

#[test]
fn continuation_identity_uses_origin_and_account_but_not_password() {
    let local = settings(json!({ "mode": "local" }));
    let external = settings(json!({
        "mode": "external",
        "serverUrl": "https://example.test",
        "confirmExternalWorkspaceAccess": true
    }));
    let first = continuation_group(&external, Some("account-one")).unwrap();
    let second = continuation_group(&external, Some("account-two")).unwrap();
    assert_ne!(continuation_group(&local, None).unwrap(), first);
    assert_ne!(first, second);
    assert_eq!(
        continuation_group(&external, Some("account-one")).unwrap(),
        first
    );
}

#[test]
fn model_and_agent_fixtures_are_bounded_validated_and_typed() {
    let models = parse_models_cli_output(include_bytes!("fixtures/models.txt")).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].provider_id, "anthropic");
    assert_eq!(models[0].context_limit, Some(200_000));
    assert_eq!(models[0].variants, ["high", "low"]);
    assert_eq!(models[1].availability, ModelAvailability::Deprecated);
    let agents = parse_agents_cli_output(include_bytes!("fixtures/agents.txt")).unwrap();
    assert_eq!(agents.len(), 4);
    assert!(
        agents
            .iter()
            .any(|agent| agent.name == "compaction" && agent.hidden)
    );
    assert!(!agents.iter().any(|agent| agent.name == "broken"));

    let catalog = provider_models(&models, &agents).unwrap();
    assert_eq!(catalog.len(), 2);
    let options = &catalog[1].options;
    assert!(options.iter().any(|option| matches!(
        option,
        ModelOptionDefinition::Choice { key, default_value, .. }
            if key == "variant" && default_value.as_deref() == Some("medium")
    )));
    assert!(options.iter().any(|option| matches!(
        option,
        ModelOptionDefinition::Choice { key, default_value, .. }
            if key == "agent" && default_value.as_deref() == Some("build")
    )));
}

#[test]
fn semantic_version_floor_is_explicit() {
    assert_eq!(
        parse_version("opencode 1.14.19\n").unwrap().to_string(),
        "1.14.19"
    );
    assert_eq!(
        ensure_supported_version(parse_version("v1.14.18").unwrap())
            .unwrap_err()
            .code,
        ChatErrorCode::UnsupportedVersion
    );
    ensure_supported_version(parse_version("1.15.0-beta.1").unwrap()).unwrap();
    assert!(parse_version("OpenCode development").is_err());
}

#[test]
fn ask_rules_allow_workspace_edits_and_keep_broader_actions_reviewed() {
    let ask_for_approval = permission_rules(SafetyMode::AskForApproval);
    assert_eq!(ask_for_approval[0].action, OpenCodePermissionAction::Ask);
    assert!(ask_for_approval.iter().any(|rule| {
        rule.permission == "question" && rule.action == OpenCodePermissionAction::Allow
    }));
    assert!(ask_for_approval.iter().any(|rule| {
        rule.permission == "edit" && rule.action == OpenCodePermissionAction::Allow
    }));
    for permission in ["bash", "webfetch", "external_directory"] {
        assert!(ask_for_approval.iter().any(|rule| {
            rule.permission == permission && rule.action == OpenCodePermissionAction::Ask
        }));
    }
    assert_eq!(
        permission_rules(SafetyMode::FullAccess),
        vec![OpenCodePermissionRule {
            permission: "*".to_string(),
            pattern: "*".to_string(),
            action: OpenCodePermissionAction::Allow,
        }]
    );
    assert_eq!(permission_reply(ApprovalDecisionKind::AllowOnce), "once");
    assert_eq!(permission_override(SafetyMode::Custom), None);
    assert_eq!(
        permission_reply(ApprovalDecisionKind::AllowSession),
        "always"
    );
    assert_eq!(permission_reply(ApprovalDecisionKind::Deny), "reject");
}

#[test]
fn declared_capabilities_include_native_plan_and_plan_selects_the_native_agent() {
    let advertised = capabilities();
    for capability in capability_kinds() {
        assert!(advertised.supports(capability));
    }
    assert!(advertised.supports(ProviderCapability::NativePlan));
    assert!(advertised.supports(ProviderCapability::StructuredPlans));

    let plan_prompt = prompt(&SendTurnRequest {
        command: ChatCommandContext {
            client_command_id: ChatCommandId::new("command-plan").unwrap(),
            expected_thread_revision: None,
        },
        session_id: ProviderSessionId::new("session-plan").unwrap(),
        turn_id: ChatTurnId::new("turn-plan").unwrap(),
        prompt: "Plan the change".to_string(),
        attachments: Vec::new(),
        mentions: Vec::new(),
        model_id: None,
        model_options: Vec::new(),
        modes: TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Plan,
        },
        developer_instructions: None,
    })
    .unwrap();
    assert_eq!(plan_prompt.agent.as_deref(), Some("plan"));
}

#[test]
fn server_commands_are_bounded_and_dispatched_only_as_plain_slash_requests() {
    let commands = parse_commands(json!([{
        "name": "release",
        "description": "Prepare a release",
        "template": "Prepare $ARGUMENTS"
    }, {
        "name": "review",
        "description": "Review a target",
        "template": "Review $1"
    }]))
    .unwrap();
    assert_eq!(commands[0].name, "release");
    assert_eq!(commands[0].argument_hint.as_deref(), Some("[arguments]"));
    assert_eq!(commands[1].argument_hint.as_deref(), Some("[arguments]"));
    let mut request: SendTurnRequest = serde_json::from_value(json!({
        "command": { "clientCommandId": "command-release" },
        "sessionId": "session-release",
        "turnId": "turn-release",
        "prompt": "/release next",
        "attachments": [],
        "mentions": [],
        "modelId": null,
        "modelOptions": [],
        "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" },
        "developerInstructions": null
    }))
    .unwrap();
    assert_eq!(
        provider_command(&request, &commands),
        Some(("release".to_string(), "next".to_string()))
    );
    request.prompt = "Explain /release".to_string();
    assert_eq!(provider_command(&request, &commands), None);
}

#[test]
fn resume_cursor_is_versioned() {
    let cursor = resume_cursor("ses_fixture").unwrap();
    assert_eq!(
        parse_resume_cursor(&cursor).unwrap().session_id,
        "ses_fixture"
    );
}

#[test]
fn owned_server_arguments_and_readiness_are_exact() {
    assert_eq!(
        launch_arguments(&["--log-level=INFO".to_string()]).unwrap(),
        [
            "--log-level=INFO",
            "serve",
            "--hostname=127.0.0.1",
            "--port=0"
        ]
    );
    for argument in [
        "serve",
        "--hostname=0.0.0.0",
        "--port=4096",
        "--password=secret",
    ] {
        assert!(launch_arguments(&[argument.to_string()]).is_err());
    }
    assert_eq!(
        parse_readiness_line("opencode server listening on http://127.0.0.1:43123\n")
            .unwrap()
            .unwrap(),
        "http://127.0.0.1:43123"
    );
    assert!(
        parse_readiness_line("opencode server listening on http://0.0.0.0:43123")
            .unwrap()
            .is_err()
    );
    assert!(parse_readiness_line("unrelated diagnostic").is_none());
}

#[cfg(unix)]
#[test]
fn owned_server_fixture_stops_its_process_tree() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    crate::test_block_on(async {
        let directory = TestDirectory::new("owned-server");
        let executable = directory.path().join("opencode-fixture");
        std::fs::write(
            &executable,
            "#!/bin/sh\nif [ \"$OPENCODE_SERVER_PASSWORD\" != \"sentinel-secret\" ]; then exit 12; fi\ntrap '' TERM\necho 'opencode server listening on http://127.0.0.1:43123'\nwhile :; do /bin/sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut configuration = configuration(json!({ "mode": "local" }));
        configuration.executable = executable.to_string_lossy().into_owned();
        configuration.environment.insert(
            OPENCODE_PASSWORD_ENVIRONMENT.to_string(),
            "sentinel-secret".to_string(),
        );
        let password = take_server_password(&mut configuration).unwrap();
        let mut server =
            OwnedOpenCodeServer::start(&configuration, directory.path(), Some(&password))
                .await
                .unwrap();
        assert_eq!(server.origin, "http://127.0.0.1:43123");
        let process_group_id = libc::pid_t::try_from(server.process_id().unwrap())
            .ok()
            .filter(|pid| *pid > 1)
            .expect("fixture process ID must fit a process-group ID greater than one");
        server.stop().await.unwrap();
        let target = process_group_id
            .checked_neg()
            .expect("fixture process-group ID must be negatable");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            // SAFETY: `target` is the checked negative form of this fixture's group
            // ID, which is greater than one. It cannot be zero or the broad -1
            // selector, and signal zero only probes existence.
            let result = unsafe { libc::kill(target, 0) };
            let error = std::io::Error::last_os_error();
            if result == -1 && error.raw_os_error() == Some(libc::ESRCH) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "provider process group still exists after stop: {error}",
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

#[cfg(unix)]
#[test]
fn owned_server_reports_early_exit_timeout_and_oversized_output() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    crate::test_block_on(async {
        let directory = TestDirectory::new("owned-server-failures");
        for (name, script, expected_code) in [
            (
                "early-exit",
                "#!/bin/sh\necho 'startup failed' >&2\nexit 7\n",
                ChatErrorCode::TransportUnavailable,
            ),
            (
                "startup-timeout",
                "#!/bin/sh\nwhile :; do /bin/sleep 1; done\n",
                ChatErrorCode::Timeout,
            ),
            (
                "oversized-output",
                "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 17000 ]; do printf x; i=$((i + 1)); done\n/bin/sleep 1\n",
                ChatErrorCode::TransportUnavailable,
            ),
        ] {
            let executable = directory.path().join(name);
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut configuration = configuration(json!({ "mode": "local" }));
            configuration.executable = executable.to_string_lossy().into_owned();
            let result = OwnedOpenCodeServer::start_with_timeout(
                &configuration,
                directory.path(),
                None,
                Duration::from_millis(75),
            )
            .await;
            let error = match result {
                Ok(mut server) => {
                    server.stop().await.unwrap();
                    panic!("{name} unexpectedly reached readiness")
                }
                Err(error) => error,
            };
            assert_eq!(error.code, expected_code, "{name}");
        }
    });
}

#[test]
fn event_fixture_normalizes_core_activity_and_deduplicates_replay() {
    let normalizer = OpenCodeEventNormalizer::new(
        ProviderInstanceId::new("opencode-instance").unwrap(),
        ChatThreadId::new("thread-fixture").unwrap(),
        ProviderSessionId::new("local-session-fixture").unwrap(),
    );
    let modes = TurnModeSnapshot {
        safety_mode: SafetyMode::AskForApproval,
        interaction_mode: InteractionMode::Build,
    };
    let mut state = OpenCodeRouteState::new("ses_fixture".to_string(), modes);
    state.active_turn_id = Some(ChatTurnId::new("turn-fixture").unwrap());
    let fixture = include_str!("fixtures/events.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let mut events = Vec::new();
    for envelope in &fixture {
        events.extend(normalizer.normalize(&mut state, envelope.clone()).unwrap());
    }

    for expected in [
        "metadata",
        "configured",
        "assistant",
        "content",
        "tool",
        "usage",
        "approval_opened",
        "approval_resolved",
        "question_opened",
        "question_resolved",
        "plan",
        "diff",
        "mcp",
        "unknown",
        "turn_completed",
    ] {
        assert!(event_kind_present(&events, expected), "missing {expected}");
    }
    let approval = events
        .iter()
        .find_map(|event| match &event.event {
            CanonicalEvent::RequestOpened(request) => Some(request),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        approval
            .allowed_decisions
            .iter()
            .map(|decision| decision.id.as_str())
            .collect::<Vec<_>>(),
        ["once", "always", "reject"]
    );
    let serialized_events = serde_json::to_string(&events).unwrap();
    assert!(!serialized_events.contains("must-redact"));
    assert!(!serialized_events.contains("authorization"));
    let rollback_cursor = events
        .iter()
        .rev()
        .filter_map(|event| event.provider_reference.as_ref())
        .find(|reference| reference.value.get("messageId").is_some())
        .expect("assistant activity should retain a rollback cursor");
    let rollback_cursor = parse_rollback_cursor(rollback_cursor).unwrap();
    assert_eq!(rollback_cursor.message_id, "msg_assistant");
    assert_eq!(rollback_cursor.part_id, None);
    assert!(
        normalizer
            .normalize(&mut state, fixture[1].clone())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn tool_activity_preserves_safe_identity_and_input_for_shared_presentation() {
    let normalizer = OpenCodeEventNormalizer::new(
        ProviderInstanceId::new("opencode-instance").unwrap(),
        ChatThreadId::new("thread-fixture").unwrap(),
        ProviderSessionId::new("local-session-fixture").unwrap(),
    );
    let mut state = OpenCodeRouteState::new(
        "ses_fixture".to_string(),
        TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
    );
    state.active_turn_id = Some(ChatTurnId::new("turn-fixture").unwrap());

    let events = normalizer
        .normalize(
            &mut state,
            json!({
                "type": "message.part.updated",
                "properties": {
                    "sessionID": "ses_fixture",
                    "part": {
                        "id": "part_read",
                        "sessionID": "ses_fixture",
                        "messageID": "msg_assistant",
                        "type": "tool",
                        "callID": "call_read",
                        "tool": "read",
                        "state": {
                            "status": "running",
                            "input": {
                                "file_path": "src/chat.ts",
                                "authorization": "must-redact"
                            },
                            "title": "Read file"
                        }
                    }
                }
            }),
        )
        .unwrap();
    let item = events
        .iter()
        .find_map(|event| match &event.event {
            CanonicalEvent::ItemStarted(item) => Some(item),
            _ => None,
        })
        .unwrap();

    assert_eq!(item.kind, CanonicalItemKind::DynamicToolCall);
    assert_eq!(item.safe_metadata.as_ref().unwrap().value["tool"], "read");
    assert_eq!(
        item.safe_metadata.as_ref().unwrap().value["input"]["file_path"],
        "src/chat.ts"
    );
    assert!(!serde_json::to_string(item).unwrap().contains("must-redact"));
}

#[test]
fn malformed_and_cross_session_events_are_bounded() {
    let normalizer = OpenCodeEventNormalizer::new(
        ProviderInstanceId::new("opencode-instance").unwrap(),
        ChatThreadId::new("thread-fixture").unwrap(),
        ProviderSessionId::new("local-session-fixture").unwrap(),
    );
    let mut state = OpenCodeRouteState::new(
        "ses_fixture".to_string(),
        TurnModeSnapshot {
            safety_mode: SafetyMode::AskForApproval,
            interaction_mode: InteractionMode::Build,
        },
    );
    assert!(normalizer
        .normalize(
            &mut state,
            json!({ "type": "session.status", "properties": { "sessionID": "other", "status": { "type": "busy" } } })
        )
        .unwrap()
        .is_empty());
    assert!(normalizer
        .normalize(
            &mut state,
            json!({ "type": "question.asked", "properties": { "sessionID": "ses_fixture", "id": "request", "questions": [] } })
        )
        .is_err());
    assert!(normalizer
        .normalize(
            &mut state,
            json!({ "type": "message.part.delta", "properties": { "sessionID": "ses_fixture", "partID": "part", "delta": "x".repeat(MAX_EVENT_DATA_BYTES) } })
        )
        .is_err());
}

fn event_kind_present(events: &[CanonicalRuntimeEvent], expected: &str) -> bool {
    events.iter().any(|event| match expected {
        "metadata" => matches!(&event.event, CanonicalEvent::ThreadMetadataUpdated(_)),
        "configured" => matches!(&event.event, CanonicalEvent::SessionConfigured(_)),
        "assistant" => matches!(
            &event.event,
            CanonicalEvent::ItemStarted(item)
                if item.kind == CanonicalItemKind::AssistantMessage
        ),
        "content" => matches!(&event.event, CanonicalEvent::ContentDelta(_)),
        "tool" => matches!(
            &event.event,
            CanonicalEvent::ItemCompleted(item)
                if item.kind == CanonicalItemKind::CommandExecution
        ),
        "usage" => matches!(&event.event, CanonicalEvent::ThreadUsageUpdated(_)),
        "approval_opened" => matches!(&event.event, CanonicalEvent::RequestOpened(_)),
        "approval_resolved" => matches!(&event.event, CanonicalEvent::RequestResolved(_)),
        "question_opened" => matches!(&event.event, CanonicalEvent::UserInputRequested(_)),
        "question_resolved" => matches!(&event.event, CanonicalEvent::UserInputResolved(_)),
        "plan" => matches!(&event.event, CanonicalEvent::PlanUpdated(_)),
        "diff" => matches!(&event.event, CanonicalEvent::DiffUpdated(_)),
        "mcp" => matches!(&event.event, CanonicalEvent::McpStatus(_)),
        "unknown" => matches!(&event.event, CanonicalEvent::Unknown(_)),
        "turn_completed" => matches!(&event.event, CanonicalEvent::TurnCompleted(_)),
        _ => false,
    })
}

fn settings(value: serde_json::Value) -> OpenCodeProviderSettings {
    OpenCodeProviderSettings::parse(&configuration(value)).unwrap()
}

pub(super) fn configuration(provider_config: serde_json::Value) -> ProviderInstanceConfig {
    ProviderInstanceConfig {
        schema_version: 1,
        instance_id: ProviderInstanceId::new("opencode-instance").unwrap(),
        family_id: ProviderFamilyId::new("opencode").unwrap(),
        label: "OpenCode".to_string(),
        enabled: true,
        executable: "opencode".to_string(),
        provider_home: None,
        launch_arguments: Vec::new(),
        environment: BTreeMap::new(),
        credential_references: BTreeMap::new(),
        visible_model_ids: Vec::new(),
        favorite_model_ids: Vec::new(),
        provider_config: VersionedJson {
            schema_version: 1,
            value: provider_config,
        },
        internal_mcp: None,
        unknown_fields: BTreeMap::new(),
    }
}

pub(super) struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    pub(super) fn new(label: &str) -> Self {
        let created_at_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-opencode-{label}-{}-{created_at_nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub(super) fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct HttpFixture {
    origin: String,
    request_receiver: mpsc::Receiver<String>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl HttpFixture {
    fn start(response: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (sender, request_receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let mut expected_length = None;
            loop {
                let bytes_read = connection.read(&mut buffer).unwrap();
                if bytes_read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..bytes_read]);
                if expected_length.is_none() {
                    expected_length = complete_request_length(&request);
                }
                if expected_length.is_some_and(|length| request.len() >= length) {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .unwrap();
            connection.write_all(response.as_bytes()).unwrap();
        });
        Self {
            origin,
            request_receiver,
            thread: Some(thread),
        }
    }

    fn request(mut self) -> String {
        let request = self
            .request_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        self.thread.take().unwrap().join().unwrap();
        request
    }
}

pub(super) fn complete_request_length(request: &[u8]) -> Option<usize> {
    let header_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?
        + 4;
    let headers = String::from_utf8_lossy(&request[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    Some(header_end + content_length)
}

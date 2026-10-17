//! Claude transport, interaction resolution, and driver lifecycle tests.

use super::{RecordingSink, TestDirectory, configuration, context, modes};
use crate::claude::driver::ClaudeProviderDriver;
use crate::claude::home::ClaudeHome;
use crate::claude::protocol::{ClaudeVersion, MINIMUM_CLAUDE_VERSION};
use crate::claude::session::{
    PendingClaudeRequest, PendingClaudeRequestKind, PendingClaudeRequests, resolve_approval,
    resolve_user_input,
};
use crate::claude::transport::read_bounded_line;
use crate::claude::transport::{
    ClaudeInboundMessage, ClaudeJsonlConnection, ClaudeTransportFailure,
};
use crate::events::CanonicalEvent;
use crate::models::*;
use crate::{DriverCancellation, DriverOperationContext, ProviderDriver};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn transport_correlates_out_of_order_responses_and_reports_malformed_lines() {
    crate::test_block_on(async {
        let (driver_reader, mut server_writer) = tokio::io::duplex(32 * 1024);
        let (server_reader, driver_writer) = tokio::io::duplex(32 * 1024);
        let mut connection = ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer);
        let client = connection.client();
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server_reader);
            let mut lines = Vec::new();
            for _ in 0..2 {
                let mut line = String::new();
                reader.read_line(&mut line).await.unwrap();
                lines.push(serde_json::from_str::<Value>(&line).unwrap());
            }
            for value in lines.into_iter().rev() {
                let request_id = value.get("request_id").and_then(Value::as_str).unwrap();
                let subtype = value
                    .get("request")
                    .and_then(|request| request.get("subtype"))
                    .and_then(Value::as_str)
                    .unwrap();
                server_writer
                    .write_all(
                        format!(
                            "{}\n",
                            json!({
                                "type": "control_response",
                                "response": {
                                    "subtype": "success",
                                    "request_id": request_id,
                                    "response": { "for": subtype },
                                }
                            })
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
            server_writer.write_all(b"not-json\n").await.unwrap();
        });
        let first_client = client.clone();
        let first_context = context("first");
        let first = tokio::spawn(async move {
            first_client
                .request("first", json!({}), &first_context)
                .await
        });
        let second_client = client.clone();
        let second_context = context("second");
        let second = tokio::spawn(async move {
            second_client
                .request("second", json!({}), &second_context)
                .await
        });
        assert_eq!(
            first
                .await
                .unwrap()
                .unwrap()
                .get("for")
                .and_then(Value::as_str),
            Some("first")
        );
        assert_eq!(
            second
                .await
                .unwrap()
                .unwrap()
                .get("for")
                .and_then(Value::as_str),
            Some("second")
        );
        let mut inbound = connection.take_inbound().unwrap();
        assert!(matches!(
            inbound.recv().await,
            Some(ClaudeInboundMessage::Malformed { .. })
        ));
        server.await.unwrap();
        connection
            .stop(Duration::ZERO, Duration::from_millis(50))
            .await
            .unwrap();
    });
}

#[test]
fn transport_cancellation_removes_pending_request() {
    crate::test_block_on(async {
        let (driver_reader, _server_writer) = tokio::io::duplex(1024);
        let (_server_reader, driver_writer) = tokio::io::duplex(1024);
        let mut connection = ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer);
        let cancellation = DriverCancellation::default();
        cancellation.cancel();
        let cancelled = DriverOperationContext {
            operation_id: "cancelled".to_string(),
            deadline: Instant::now() + Duration::from_secs(1),
            cancellation,
        };
        let error = connection
            .client()
            .request("interrupt", json!({}), &cancelled)
            .await
            .unwrap_err();
        assert_eq!(error, ClaudeTransportFailure::Cancelled);
        connection
            .stop(Duration::ZERO, Duration::from_millis(50))
            .await
            .unwrap();
    });
}

#[test]
fn oversized_frame_is_drained_without_growing_the_parse_buffer() {
    crate::test_block_on(async {
        let (mut writer, reader) = tokio::io::duplex(8 * 1024);
        let write = tokio::spawn(async move {
            writer.write_all(&vec![b'x'; 4 * 1024]).await.unwrap();
            writer.write_all(b"\n").await.unwrap();
        });
        let mut reader = BufReader::new(reader);
        let mut output = Vec::new();
        let line = read_bounded_line(&mut reader, &mut output, 64)
            .await
            .unwrap()
            .unwrap();

        assert!(line.oversized);
        assert_eq!(line.byte_length, 4 * 1024 + 1);
        assert!(output.len() <= 65);
        write.await.unwrap();
    });
}

#[test]
fn approval_resolution_returns_only_the_selected_native_behavior() {
    crate::test_block_on(async {
        let (driver_reader, _server_writer) = tokio::io::duplex(8 * 1024);
        let (server_reader, driver_writer) = tokio::io::duplex(8 * 1024);
        let connection = ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer);
        let provider_request_id = ProviderRequestId::new("approval-1".to_string()).unwrap();
        let pending: PendingClaudeRequests = Arc::new(Mutex::new(HashMap::from([(
            provider_request_id.clone(),
            PendingClaudeRequest {
                control_request_id: "approval-1".to_string(),
                provider_request_id: provider_request_id.clone(),
                kind: PendingClaudeRequestKind::Approval {
                    tool_input: json!({ "command": "printf redacted" }),
                    permission_suggestions: Some(json!([{
                        "type": "addRules",
                        "rules": [{ "toolName": "Bash", "ruleContent": "printf:*" }],
                        "behavior": "allow",
                        "destination": "session"
                    }])),
                },
            },
        )])));
        let request = ResolveApprovalRequest {
            command: ChatCommandContext {
                client_command_id: ChatCommandId::new("command-approval".to_string()).unwrap(),
                expected_thread_revision: None,
            },
            session_id: ProviderSessionId::new("session-1".to_string()).unwrap(),
            request_id: ChatRequestId::new("request-1".to_string()).unwrap(),
            provider_request_id,
            decision: ApprovalDecision {
                kind: ApprovalDecisionKind::AllowSession,
                provider_option_id: Some("allow_session".to_string()),
                updated_tool_input: None,
            },
        };
        resolve_approval(&connection.client(), &pending, &request)
            .await
            .unwrap();
        let mut reader = BufReader::new(server_reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        let payload = response
            .get("response")
            .and_then(|value| value.get("response"))
            .unwrap();
        assert_eq!(
            payload.get("behavior").and_then(Value::as_str),
            Some("allow")
        );
        assert!(payload.get("updatedPermissions").is_some());
        assert!(pending.lock().unwrap().is_empty());
    });
}

#[test]
fn question_resolution_maps_stable_option_ids_to_exact_provider_labels() {
    crate::test_block_on(async {
        let (driver_reader, _server_writer) = tokio::io::duplex(8 * 1024);
        let (server_reader, driver_writer) = tokio::io::duplex(8 * 1024);
        let connection = ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer);
        let provider_request_id = ProviderRequestId::new("question-1".to_string()).unwrap();
        let pending: PendingClaudeRequests = Arc::new(Mutex::new(HashMap::from([(
            provider_request_id.clone(),
            PendingClaudeRequest {
                control_request_id: "question-1".to_string(),
                provider_request_id: provider_request_id.clone(),
                kind: PendingClaudeRequestKind::UserInput {
                    questions: json!([{
                        "question": "Choose a bounded option",
                        "options": [{ "label": "First" }, { "label": "Second" }]
                    }]),
                    option_labels: HashMap::from([(
                        "question-0".to_string(),
                        HashMap::from([
                            ("option-0-0".to_string(), "First".to_string()),
                            ("option-0-1".to_string(), "Second".to_string()),
                        ]),
                    )]),
                    provider_question_labels: HashMap::from([(
                        "question-0".to_string(),
                        "Choose a bounded option".to_string(),
                    )]),
                },
            },
        )])));
        let request = ResolveUserInputRequest {
            command: ChatCommandContext {
                client_command_id: ChatCommandId::new("command-question".to_string()).unwrap(),
                expected_thread_revision: None,
            },
            session_id: ProviderSessionId::new("session-1".to_string()).unwrap(),
            request_id: ChatRequestId::new("request-question".to_string()).unwrap(),
            provider_request_id,
            answers: vec![UserInputAnswer {
                question_id: "question-0".to_string(),
                selected_option_ids: vec!["option-0-1".to_string()],
                free_form_text: None,
            }],
        };
        resolve_user_input(&connection.client(), &pending, &request)
            .await
            .unwrap();
        let mut reader = BufReader::new(server_reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            response
                .pointer("/response/response/updatedInput/answers/Choose a bounded option")
                .and_then(Value::as_str),
            Some("Second")
        );
        assert!(pending.lock().unwrap().is_empty());
    });
}

#[test]
fn driver_starts_dispatches_and_stops_a_native_session() {
    crate::test_block_on(async {
        let home = TestDirectory::new("driver-home");
        let workspace = TestDirectory::new("driver-workspace");
        let mut driver = ClaudeProviderDriver::new(configuration(home.path())).unwrap();
        let home_path = home.path().to_path_buf();
        let pair = Arc::new(Mutex::new(Some(native_fixture_connection(
            home_path.clone(),
        ))));
        driver.set_connection_factory(Arc::new(move |_| {
            pair.lock().unwrap().take().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "test connection already consumed",
                    false,
                )
            })
        }));
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                StartSessionRequest {
                    thread_id: ChatThreadId::new("thread-1".to_string()).unwrap(),
                    workspace: VerifiedWorkspaceContext {
                        working_folder_id: ProjectWorkingFolderId::new("workspace-1".to_string())
                            .unwrap(),
                        canonical_path: workspace.path().to_string_lossy().into_owned(),
                        repository_kind: RepositoryKind::None,
                        repository_identity: None,
                    },
                    provider_instance_id: ProviderInstanceId::new("claude-instance-1".to_string())
                        .unwrap(),
                    modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
                    model_id: Some(ModelId::new("claude-sonnet-4-5".to_string()).unwrap()),
                    model_options: Vec::new(),
                },
                sink.clone(),
                &context("start"),
            )
            .await
            .unwrap();
        let receipt = driver
            .send_turn(
                SendTurnRequest {
                    command: ChatCommandContext {
                        client_command_id: ChatCommandId::new("command-1".to_string()).unwrap(),
                        expected_thread_revision: None,
                    },
                    session_id: snapshot.session_id.clone(),
                    turn_id: ChatTurnId::new("turn-1".to_string()).unwrap(),
                    prompt: "Respond with the fixture".to_string(),
                    attachments: Vec::new(),
                    mentions: Vec::new(),
                    model_id: Some(ModelId::new("claude-sonnet-4-5".to_string()).unwrap()),
                    model_options: Vec::new(),
                    modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
                    developer_instructions: None,
                },
                &context("turn"),
            )
            .await
            .unwrap();
        assert_eq!(receipt.state, ChatTurnState::Active);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            sink.events()
                .iter()
                .any(|event| matches!(event.event, CanonicalEvent::TurnCompleted(_)))
        );
        driver
            .stop_session(
                StopSessionRequest {
                    session_id: snapshot.session_id,
                    force: false,
                },
                &context("stop"),
            )
            .await
            .unwrap();
        assert!(
            sink.events()
                .iter()
                .any(|event| matches!(event.event, CanonicalEvent::SessionExited(_)))
        );
    });
}

#[test]
fn failed_turn_started_emit_releases_the_active_turn() {
    struct RejectFirstTurnStartSink {
        rejected: std::sync::atomic::AtomicBool,
    }

    impl crate::ProviderEventSink for RejectFirstTurnStartSink {
        fn emit<'a>(
            &'a self,
            event: crate::events::CanonicalRuntimeEvent,
        ) -> crate::DriverFuture<'a, ()> {
            Box::pin(async move {
                if matches!(event.event, CanonicalEvent::TurnStarted(_))
                    && !self
                        .rejected
                        .swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    return Err(ChatError::new(
                        ChatErrorCode::Conflict,
                        "test sink rejected turn start",
                        true,
                    ));
                }
                Ok(())
            })
        }
    }

    crate::test_block_on(async {
        let home = TestDirectory::new("emit-failure-home");
        let workspace = TestDirectory::new("emit-failure-workspace");
        let mut driver = ClaudeProviderDriver::new(configuration(home.path())).unwrap();
        let pair = Arc::new(Mutex::new(Some(native_fixture_connection(
            home.path().to_path_buf(),
        ))));
        driver.set_connection_factory(Arc::new(move |_| {
            pair.lock().unwrap().take().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "test connection already consumed",
                    false,
                )
            })
        }));
        let sink = Arc::new(RejectFirstTurnStartSink {
            rejected: std::sync::atomic::AtomicBool::new(false),
        });
        let snapshot = driver
            .start_session(
                StartSessionRequest {
                    thread_id: ChatThreadId::new("thread-1".to_string()).unwrap(),
                    workspace: VerifiedWorkspaceContext {
                        working_folder_id: ProjectWorkingFolderId::new("workspace-1".to_string())
                            .unwrap(),
                        canonical_path: workspace.path().to_string_lossy().into_owned(),
                        repository_kind: RepositoryKind::None,
                        repository_identity: None,
                    },
                    provider_instance_id: ProviderInstanceId::new("claude-instance-1".to_string())
                        .unwrap(),
                    modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
                    model_id: Some(ModelId::new("claude-sonnet-4-5".to_string()).unwrap()),
                    model_options: Vec::new(),
                },
                sink,
                &context("start"),
            )
            .await
            .unwrap();
        let turn = |turn_id: &str| SendTurnRequest {
            command: ChatCommandContext {
                client_command_id: ChatCommandId::new(format!("command-{turn_id}")).unwrap(),
                expected_thread_revision: None,
            },
            session_id: snapshot.session_id.clone(),
            turn_id: ChatTurnId::new(turn_id.to_string()).unwrap(),
            prompt: "Respond with the fixture".to_string(),
            attachments: Vec::new(),
            mentions: Vec::new(),
            model_id: Some(ModelId::new("claude-sonnet-4-5".to_string()).unwrap()),
            model_options: Vec::new(),
            modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
            developer_instructions: None,
        };

        let rejected = driver
            .send_turn(turn("turn-1"), &context("turn-1"))
            .await
            .unwrap_err();
        assert_eq!(rejected.code, ChatErrorCode::Conflict);

        let receipt = driver
            .send_turn(turn("turn-2"), &context("turn-2"))
            .await
            .unwrap();
        assert_eq!(receipt.state, ChatTurnState::Active);
    });
}

#[test]
fn healthy_probe_exposes_the_native_model_catalog() {
    crate::test_block_on(async {
        let home = TestDirectory::new("probe-model-catalog-home");
        let mut driver = ClaudeProviderDriver::new(configuration(home.path())).unwrap();
        let pair = Arc::new(Mutex::new(Some(native_fixture_connection(
            home.path().to_path_buf(),
        ))));
        driver.set_connection_factory(Arc::new(move |_| {
            pair.lock().unwrap().take().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "test connection already consumed",
                    false,
                )
            })
        }));

        let probe = driver.probe(&context("probe-model-catalog")).await.unwrap();
        let catalog = driver.cached_model_catalog().unwrap();

        assert_eq!(probe.state, ProbeState::Healthy);
        assert_eq!(catalog.instance_id, probe.instance_id);
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].id.as_str(), "claude-sonnet-4-5");
        assert_eq!(catalog.models[0].display_name, "Claude Sonnet 4.5");
    });
}

fn native_fixture_connection(home: PathBuf) -> (ClaudeJsonlConnection, ClaudeHome, ClaudeVersion) {
    let (driver_reader, mut server_writer) = tokio::io::duplex(128 * 1024);
    let (server_reader, driver_writer) = tokio::io::duplex(128 * 1024);
    tokio::spawn(async move {
        let mut reader = BufReader::new(server_reader);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                break;
            }
            let value: Value = serde_json::from_str(&line).unwrap();
            if value.get("type").and_then(Value::as_str) == Some("control_request") {
                let request_id = value.get("request_id").and_then(Value::as_str).unwrap();
                let subtype = value
                    .get("request")
                    .and_then(|request| request.get("subtype"))
                    .and_then(Value::as_str)
                    .unwrap();
                let response = if subtype == "initialize" {
                    json!({
                        "commands": [{ "name": "help", "description": "Help", "argumentHint": null }],
                        "models": [{
                            "value": "claude-sonnet-4-5",
                            "resolvedModel": "claude-sonnet-4-5",
                            "displayName": "Claude Sonnet 4.5",
                            "description": "Fixture model",
                            "supportsEffort": true,
                            "supportedEffortLevels": ["low", "medium", "high", "max"]
                        }],
                        "account": {
                            "email": "redacted@example.invalid",
                            "organization": "Redacted",
                            "subscriptionType": "fixture"
                        },
                        "output_style": "default"
                    })
                } else {
                    json!({})
                };
                write_json_line(
                    &mut server_writer,
                    json!({
                        "type": "control_response",
                        "response": {
                            "subtype": "success",
                            "request_id": request_id,
                            "response": response,
                        }
                    }),
                )
                .await;
                continue;
            }
            if value.get("type").and_then(Value::as_str) == Some("user") {
                for fixture_line in include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/src/claude/fixtures/compatibility.jsonl"
                ))
                .lines()
                {
                    let fixture: Value = serde_json::from_str(fixture_line).unwrap();
                    if fixture.get("type").and_then(Value::as_str) != Some("control_request") {
                        write_json_line(&mut server_writer, fixture).await;
                    }
                }
            }
        }
    });
    (
        ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer),
        ClaudeHome {
            config_directory: home,
        },
        MINIMUM_CLAUDE_VERSION,
    )
}

async fn write_json_line(writer: &mut tokio::io::DuplexStream, value: Value) {
    writer
        .write_all(format!("{value}\n").as_bytes())
        .await
        .unwrap();
}

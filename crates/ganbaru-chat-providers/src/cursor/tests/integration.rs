//! Cursor ACP transport and driver integration fixtures.

use super::support::*;
use crate::cursor::driver::CursorProviderDriver;
use crate::cursor::session::AcpInboundQueue;
use crate::cursor::transport::{AcpInboundMessage, AcpRpcConnection, AcpRpcFailure};
use crate::events::CanonicalEvent;
use crate::models::*;
use crate::{ProviderDriver, ProviderEventSink};
use serde_json::{Value, json};
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn json_rpc_correlates_out_of_order_and_drains_malformed_frames() {
    crate::test_block_on(async {
        let (application, agent) = tokio::io::duplex(5 * 1024 * 1024);
        let (application_reader, application_writer) = tokio::io::split(application);
        let (agent_reader, mut agent_writer) = tokio::io::split(agent);
        let mut connection = AcpRpcConnection::from_test_io(application_reader, application_writer);
        let client = connection.client();
        let first_client = client.clone();
        let first = tokio::spawn(async move {
            first_client
                .request("first", json!({}), &context("first"))
                .await
        });
        let second_client = client.clone();
        let second = tokio::spawn(async move {
            second_client
                .request("second", json!({}), &context("second"))
                .await
        });
        let mut reader = BufReader::new(agent_reader);
        let first_request = read_json_line(&mut reader).await;
        let second_request = read_json_line(&mut reader).await;
        assert_eq!(first_request["jsonrpc"], "2.0");
        let first_id = [&first_request, &second_request]
            .into_iter()
            .find(|request| request["method"] == "first")
            .unwrap()["id"]
            .clone();
        let second_id = [&first_request, &second_request]
            .into_iter()
            .find(|request| request["method"] == "second")
            .unwrap()["id"]
            .clone();
        write_json_line(
            &mut agent_writer,
            json!({
                "jsonrpc": "2.0",
                "id": second_id,
                "result": { "method": "second" }
            }),
        )
        .await;
        write_json_line(
            &mut agent_writer,
            json!({
                "jsonrpc": "2.0",
                "id": first_id,
                "result": { "method": "first" }
            }),
        )
        .await;
        assert_eq!(first.await.unwrap().unwrap()["method"], "first");
        assert_eq!(second.await.unwrap().unwrap()["method"], "second");

        let mut inbound = connection.take_inbound().unwrap();
        let oversized = vec![b'x'; 4 * 1024 * 1024 + 16];
        agent_writer.write_all(&oversized).await.unwrap();
        agent_writer.write_all(b"\n").await.unwrap();
        write_json_line(
            &mut agent_writer,
            json!({
                "jsonrpc": "2.0",
                "method": "fixture/notification",
                "params": { "safe": true }
            }),
        )
        .await;
        assert!(matches!(
            inbound.recv().await.unwrap(),
            AcpInboundMessage::Malformed { .. }
        ));
        assert!(matches!(
            inbound.recv().await.unwrap(),
            AcpInboundMessage::Notification { method, .. }
                if method == "fixture/notification"
        ));

        let cancelled_context = context("cancelled");
        cancelled_context.cancellation.cancel();
        assert_eq!(
            client
                .request("cancelled", json!({}), &cancelled_context)
                .await
                .unwrap_err(),
            AcpRpcFailure::Cancelled
        );
        connection
            .stop(Duration::from_millis(20), Duration::from_millis(20))
            .await
            .unwrap();
    });
}

#[test]
fn json_rpc_prompt_fallback_releases_the_original_response_slot() {
    crate::test_block_on(async {
        let (application, agent) = tokio::io::duplex(64 * 1024);
        let (application_reader, application_writer) = tokio::io::split(application);
        let (agent_reader, mut agent_writer) = tokio::io::split(agent);
        let mut connection = AcpRpcConnection::from_test_io(application_reader, application_writer);
        let client = connection.client();
        let prompt_client = client.clone();
        let (completion_sender, completion_receiver) = tokio::sync::oneshot::channel();
        let prompt = tokio::spawn(async move {
            prompt_client
                .request_with_fallback(
                    "session/prompt",
                    json!({ "sessionId": "grok-session" }),
                    completion_receiver,
                    &context("grok-prompt"),
                )
                .await
        });
        let mut reader = BufReader::new(agent_reader);
        let request = read_json_line(&mut reader).await;
        let abandoned_id = request["id"].clone();
        completion_sender
            .send(json!({ "stopReason": "end_turn" }))
            .unwrap();
        assert_eq!(prompt.await.unwrap().unwrap()["stopReason"], "end_turn");

        write_json_line(
            &mut agent_writer,
            json!({
                "jsonrpc": "2.0",
                "id": abandoned_id,
                "result": { "stopReason": "late" }
            }),
        )
        .await;
        let next_client = client.clone();
        let next = tokio::spawn(async move {
            next_client
                .request("next", json!({}), &context("next"))
                .await
        });
        let next_request = read_json_line(&mut reader).await;
        write_json_line(
            &mut agent_writer,
            json!({
                "jsonrpc": "2.0",
                "id": next_request["id"],
                "result": { "ready": true }
            }),
        )
        .await;
        assert_eq!(next.await.unwrap().unwrap()["ready"], true);
        connection
            .stop(Duration::from_millis(20), Duration::from_millis(20))
            .await
            .unwrap();
    });
}

#[test]
fn driver_fixture_covers_prompt_plan_cancel_and_cleanup() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("driver");
        let state = Arc::new(AcpFixtureState::default());
        let factory_state = Arc::clone(&state);
        let mut driver = CursorProviderDriver::new(configuration(workspace.path(), None)).unwrap();
        driver.set_connection_factory(Arc::new(move |_workspace| {
            Ok((
                fixture_connection(FixtureScenario::Healthy, Arc::clone(&factory_state)),
                about().into(),
            ))
        }));
        let sink = Arc::new(RecordingSink::default());
        let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink_trait,
                &context("driver-start"),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot.provider_thread_id.as_ref().unwrap().as_str(),
            "cursor-session-fixture"
        );
        assert!(
            snapshot
                .capabilities
                .supports(ProviderCapability::NativePlan)
        );
        let receipt = driver
            .send_turn(send_request(&snapshot.session_id), &context("send"))
            .await
            .unwrap();
        assert_eq!(receipt.state, ChatTurnState::Active);
        wait_for_method(&state, "session/prompt").await;
        let stale = serde_json::from_value(json!({
            "command": { "clientCommandId": "cursor-stale-cancel", "expectedThreadRevision": 3 },
            "sessionId": snapshot.session_id,
            "turnId": "different-turn"
        }))
        .unwrap();
        assert_eq!(
            driver
                .interrupt_turn(stale, &context("stale-interrupt"))
                .await
                .unwrap_err()
                .code,
            ChatErrorCode::Conflict
        );
        driver
            .interrupt_turn(
                interrupt_request(&snapshot.session_id),
                &context("interrupt"),
            )
            .await
            .unwrap();
        wait_for_event(&sink, |event| {
            matches!(event, CanonicalEvent::TurnAborted(_))
        })
        .await;
        assert!(
            sink.events()
                .iter()
                .any(|event| matches!(event.event, CanonicalEvent::ProposedPlanCompleted(_)))
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
        wait_for_closed(&state).await;
        assert!(request_methods(&state.received()).contains(&"session/cancel"));
    });
}

#[test]
fn probe_sends_only_initialize_and_stops_the_agent() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("probe");
        let state = Arc::new(AcpFixtureState::default());
        let factory_state = Arc::clone(&state);
        let mut driver = CursorProviderDriver::new(configuration(workspace.path(), None)).unwrap();
        driver.set_connection_factory(Arc::new(move |_workspace| {
            Ok((
                fixture_connection(FixtureScenario::Healthy, Arc::clone(&factory_state)),
                about().into(),
            ))
        }));
        let probe = driver.probe(&context("probe")).await.unwrap();
        wait_for_closed(&state).await;
        assert_eq!(request_methods(&state.received()), ["initialize"]);
        assert_eq!(probe.state, ProbeState::Healthy);
        assert_eq!(probe.negotiated_protocol_version.as_deref(), Some("1"));
        assert!(
            probe
                .capabilities
                .supports(ProviderCapability::NativeResume)
        );
        assert!(probe.capabilities.supports(ProviderCapability::NativePlan));
        assert!(driver.cached_model_catalog().is_none());
    });
}

#[test]
fn load_replay_quiet_window_stops_on_cancellation() {
    crate::test_block_on(async {
        let (application, agent) = tokio::io::duplex(64 * 1024);
        let (application_reader, application_writer) = tokio::io::split(application);
        let (agent_reader, mut agent_writer) = tokio::io::split(agent);
        let mut connection = AcpRpcConnection::from_test_io(application_reader, application_writer);
        let mut reader = BufReader::new(agent_reader);
        let mut inbound = AcpInboundQueue::new(connection.take_inbound().unwrap())
            .with_replay_idle(Duration::from_secs(30));

        let loading = context("cancelled-load");
        let cancellation = loading.cancellation.clone();
        let client = connection.client();
        let load = tokio::spawn(async move {
            inbound
                .load_session(
                    &client,
                    "cursor-session-fixture",
                    json!({ "sessionId": "cursor-session-fixture" }),
                    &loading,
                )
                .await
        });
        let request = read_json_line(&mut reader).await;
        assert_eq!(request["method"], "session/load");
        write_json_line(
            &mut agent_writer,
            json!({ "jsonrpc": "2.0", "id": request["id"], "result": {} }),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!load.is_finished(), "the quiet window is still open");
        let started = std::time::Instant::now();
        cancellation.cancel();
        let result = load.await.unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(result, Err(AcpRpcFailure::Cancelled));

        connection
            .stop(Duration::from_millis(20), Duration::from_millis(20))
            .await
            .unwrap();
    });
}

async fn read_json_line<R>(reader: &mut BufReader<R>) -> Value
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

async fn write_json_line<W>(writer: &mut W, value: Value)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}

fn start_request(workspace: &std::path::Path) -> StartSessionRequest {
    serde_json::from_value(json!({
        "threadId": "cursor-thread-fixture",
        "workspace": {
            "workingFolderId": "cursor-workspace-fixture",
            "canonicalPath": workspace,
            "repositoryKind": "none",
            "repositoryIdentity": null
        },
        "providerInstanceId": "cursor-instance-1",
        "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" },
        "modelId": "cursor-small",
        "modelOptions": []
    }))
    .unwrap()
}

fn send_request(session_id: &ProviderSessionId) -> SendTurnRequest {
    serde_json::from_value(json!({
        "command": { "clientCommandId": "cursor-send", "expectedThreadRevision": 2 },
        "sessionId": session_id,
        "turnId": "cursor-turn-fixture",
        "prompt": "Implement the fixture",
        "attachments": [],
        "mentions": [{ "relativePath": "src/main.rs", "kind": "file" }],
        "modelId": "cursor-small",
        "modelOptions": [],
        "modes": { "safetyMode": "ask_for_approval", "interactionMode": "plan" },
        "developerInstructions": "Follow repository rules"
    }))
    .unwrap()
}

fn interrupt_request(session_id: &ProviderSessionId) -> InterruptTurnRequest {
    serde_json::from_value(json!({
        "command": { "clientCommandId": "cursor-cancel", "expectedThreadRevision": 3 },
        "sessionId": session_id,
        "turnId": "cursor-turn-fixture"
    }))
    .unwrap()
}

async fn wait_for_method(state: &AcpFixtureState, method: &str) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if request_methods(&state.received()).contains(&method) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_event(sink: &RecordingSink, predicate: impl Fn(&CanonicalEvent) -> bool) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if sink.events().iter().any(|event| predicate(&event.event)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_closed(state: &AcpFixtureState) {
    tokio::time::timeout(Duration::from_secs(1), async {
        while !state.closed.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn pending_requests_wake_on_cancellation_and_deadline() {
    crate::test_block_on(async {
        let (application, agent) = tokio::io::duplex(64 * 1024);
        let (application_reader, application_writer) = tokio::io::split(application);
        let (agent_reader, _agent_writer) = tokio::io::split(agent);
        let mut connection = AcpRpcConnection::from_test_io(application_reader, application_writer);
        let mut reader = BufReader::new(agent_reader);

        let cancelled = context("cancelled");
        let cancellation = cancelled.cancellation.clone();
        let client = connection.client();
        let request = tokio::spawn(async move {
            client
                .request("never_answered", json!({}), &cancelled)
                .await
        });
        assert_eq!(
            read_json_line(&mut reader).await["method"],
            "never_answered"
        );
        let started = std::time::Instant::now();
        cancellation.cancel();
        let result = request.await.unwrap();
        assert!(started.elapsed() < Duration::from_millis(50));
        assert_eq!(result, Err(AcpRpcFailure::Cancelled));

        let mut expiring = context("expiring");
        expiring.deadline = std::time::Instant::now() + Duration::from_millis(20);
        let (_fallback_sender, fallback) = tokio::sync::oneshot::channel();
        let started = std::time::Instant::now();
        let result = connection
            .client()
            .request_with_fallback("never_answered", json!({}), fallback, &expiring)
            .await;
        assert_eq!(result, Err(AcpRpcFailure::Timeout));
        assert!(started.elapsed() < Duration::from_millis(500));

        connection
            .stop(Duration::from_millis(20), Duration::from_millis(20))
            .await
            .unwrap();
    });
}

#[test]
fn instructions_reach_each_live_session_once_and_usage_reports_cost() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("instructions");
        let state = Arc::new(AcpFixtureState::default());
        let factory_state = Arc::clone(&state);
        let mut driver = CursorProviderDriver::new(configuration(workspace.path(), None)).unwrap();
        driver.set_connection_factory(Arc::new(move |_workspace| {
            Ok((
                fixture_connection(FixtureScenario::CompletePrompts, Arc::clone(&factory_state)),
                about().into(),
            ))
        }));
        let sink = Arc::new(RecordingSink::default());

        let first = start_completing_session(&mut driver, &sink, workspace.path()).await;
        assert!(
            !first
                .capabilities
                .supports(ProviderCapability::CostReporting)
        );
        let rules = "Follow repository rules";
        complete_turn(&mut driver, &sink, &first.session_id, "turn-1", rules).await;
        complete_turn(
            &mut driver,
            &sink,
            &first.session_id,
            "turn-2",
            "Follow repository rules\r\n",
        )
        .await;
        complete_turn(
            &mut driver,
            &sink,
            &first.session_id,
            "turn-3",
            "Ask before deleting files",
        )
        .await;
        complete_turn(
            &mut driver,
            &sink,
            &first.session_id,
            "turn-4",
            "Ask before deleting files",
        )
        .await;
        assert_eq!(
            prompt_instruction_flags(&state),
            [true, false, true, false],
            "unchanged instructions stay out of later prompts in the same live session"
        );
        assert!(sink.events().iter().any(|event| matches!(
            &event.event,
            CanonicalEvent::ThreadUsageUpdated(usage)
                if usage.context_tokens == Some(FIXTURE_USAGE_USED)
                    && usage.context_limit == Some(FIXTURE_USAGE_SIZE)
                    && usage.cost.as_ref().is_some_and(|cost| cost.currency == "USD")
        )));
        driver
            .stop_session(
                StopSessionRequest {
                    session_id: first.session_id,
                    force: false,
                },
                &context("stop-first"),
            )
            .await
            .unwrap();

        let second = start_completing_session(&mut driver, &sink, workspace.path()).await;
        assert!(
            second
                .capabilities
                .supports(ProviderCapability::CostReporting),
            "an observed session cost is reported for the next session"
        );
        assert!(
            second
                .capabilities
                .supports(ProviderCapability::ContextUsage)
        );
        complete_turn(
            &mut driver,
            &sink,
            &second.session_id,
            "turn-5",
            "Ask before deleting files",
        )
        .await;
        assert_eq!(
            prompt_instruction_flags(&state),
            [true, false, true, false, true],
            "a new live session receives the instructions again"
        );
    });
}

async fn start_completing_session(
    driver: &mut CursorProviderDriver,
    sink: &Arc<RecordingSink>,
    workspace: &std::path::Path,
) -> ProviderSessionSnapshot {
    let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
    driver
        .start_session(start_request(workspace), sink_trait, &context("start"))
        .await
        .unwrap()
}

async fn complete_turn(
    driver: &mut CursorProviderDriver,
    sink: &RecordingSink,
    session_id: &ProviderSessionId,
    turn_id: &str,
    instructions: &str,
) {
    let completed_before = completed_turns(sink);
    let mut request = send_request(session_id);
    request.turn_id = serde_json::from_value(json!(turn_id)).unwrap();
    request.developer_instructions = Some(instructions.to_string());
    driver.send_turn(request, &context(turn_id)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while completed_turns(sink) == completed_before {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn completed_turns(sink: &RecordingSink) -> usize {
    sink.events()
        .iter()
        .filter(|event| matches!(event.event, CanonicalEvent::TurnCompleted(_)))
        .count()
}

/// Whether each `session/prompt` the fixture received carried the instructions block.
fn prompt_instruction_flags(state: &AcpFixtureState) -> Vec<bool> {
    state
        .received()
        .iter()
        .filter(|message| message["method"] == "session/prompt")
        .map(|message| {
            message["params"]["prompt"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|block| block["text"].as_str())
                .any(|text| text.contains("Additional Ganbaru instructions"))
        })
        .collect()
}

/// Starts a generic ACP driver over the fixture `scenario`.
fn generic_driver(
    workspace: &std::path::Path,
    scenario: FixtureScenario,
    auth_method: Option<&str>,
) -> (CursorProviderDriver, Arc<AcpFixtureState>) {
    let state = Arc::new(AcpFixtureState::default());
    let factory_state = Arc::clone(&state);
    let mut driver =
        CursorProviderDriver::new_generic(generic_configuration(workspace, auth_method)).unwrap();
    driver.set_connection_factory(Arc::new(move |_workspace| {
        Ok((
            fixture_connection(scenario, Arc::clone(&factory_state)),
            about().into(),
        ))
    }));
    (driver, state)
}

fn generic_start_request(workspace: &std::path::Path) -> StartSessionRequest {
    let mut request = start_request(workspace);
    request.provider_instance_id = serde_json::from_value(json!("acp-instance-1")).unwrap();
    request.model_id = None;
    request
}

#[test]
fn generic_agent_uses_only_standard_acp_calls() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("generic-agent");
        let (mut driver, state) =
            generic_driver(workspace.path(), FixtureScenario::GenericAgent, None);
        let sink = Arc::new(RecordingSink::default());
        let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
        let session = driver
            .start_session(
                generic_start_request(workspace.path()),
                sink_trait,
                &context("start"),
            )
            .await
            .unwrap();
        for absent in [
            ProviderCapability::StructuredQuestions,
            ProviderCapability::StructuredPlans,
            ProviderCapability::ProviderDiffs,
            ProviderCapability::DynamicModelChange,
        ] {
            assert!(!session.capabilities.supports(absent), "{absent:?}");
        }

        let mut request = send_request(&session.session_id);
        request.model_id = None;
        request.modes = modes(SafetyMode::AskForApproval, InteractionMode::Build);
        driver.send_turn(request, &context("send")).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while completed_turns(&sink) == 0 || unknown_sources(&sink).len() < 3 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();

        let expected_sources = GENERIC_EXTENSION_CALLS
            .iter()
            .map(|(method, request_id)| match request_id {
                Some(_) => format!("acp/request/{method}"),
                None => format!("acp/notification/{method}"),
            })
            .collect::<Vec<_>>();
        assert_eq!(unknown_sources(&sink), expected_sources);

        let received = state.received();
        let methods = request_methods(&received);
        assert!(!methods.contains(&"authenticate"), "{methods:?}");
        assert!(
            methods
                .iter()
                .all(|method| !method.starts_with("cursor/") && *method != "session/set_model"),
            "{methods:?}"
        );
        for (_, request_id) in GENERIC_EXTENSION_CALLS {
            let Some(request_id) = request_id else {
                continue;
            };
            let response = received
                .iter()
                .find(|message| message["id"] == request_id && message.get("method").is_none())
                .unwrap();
            assert_eq!(response["error"]["code"], -32601);
        }

        let prompt = received
            .iter()
            .find(|message| message["method"] == "session/prompt")
            .unwrap();
        let link = prompt["params"]["prompt"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["type"] == "resource_link")
            .unwrap();
        let expected_uri = reqwest::Url::from_file_path(workspace.path().join("src/main.rs"))
            .unwrap()
            .to_string();
        assert_eq!(link["uri"], expected_uri.as_str());
        assert_eq!(link["name"], "main.rs");
        assert_eq!(link["title"], "src/main.rs");
        assert!(
            prompt["params"]["prompt"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|block| block["text"].as_str())
                .all(|text| !text.contains("@src/main.rs"))
        );
    });
}

#[test]
fn generic_agent_authenticates_only_with_the_configured_method() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("generic-auth");
        for configured in [None, Some("password")] {
            let (mut driver, state) = generic_driver(
                workspace.path(),
                FixtureScenario::GenericAgentWithAuth,
                configured,
            );
            let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
            let error = driver
                .start_session(
                    generic_start_request(workspace.path()),
                    sink,
                    &context("start"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::AuthenticationRequired);
            for (id, name) in GENERIC_AUTH_METHODS {
                assert!(
                    error.message.contains(&format!("{name} ({id})")),
                    "{error:?}"
                );
            }
            assert!(!request_methods(&state.received()).contains(&"authenticate"));
        }

        let (mut driver, state) = generic_driver(
            workspace.path(),
            FixtureScenario::GenericAgentWithAuth,
            Some(GENERIC_AUTH_METHODS[1].0),
        );
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
        driver
            .start_session(
                generic_start_request(workspace.path()),
                sink,
                &context("start"),
            )
            .await
            .unwrap();
        let received = state.received();
        let authenticate = received
            .iter()
            .find(|message| message["method"] == "authenticate")
            .unwrap();
        assert_eq!(
            authenticate["params"]["methodId"],
            GENERIC_AUTH_METHODS[1].0
        );
    });
}

/// Source types of the unknown events the sink received, in order.
fn unknown_sources(sink: &RecordingSink) -> Vec<String> {
    sink.events()
        .iter()
        .filter_map(|event| match &event.event {
            CanonicalEvent::Unknown(unknown) => Some(unknown.source_type.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn generic_probe_reports_the_agent_version_without_a_range() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("generic-probe");
        let (mut driver, state) =
            generic_driver(workspace.path(), FixtureScenario::GenericAgent, None);
        let probe = driver.probe(&context("probe")).await.unwrap();
        wait_for_closed(&state).await;
        assert_eq!(request_methods(&state.received()), ["initialize"]);
        assert_eq!(probe.state, ProbeState::Healthy);
        assert_eq!(probe.version.as_deref(), Some("0.4.0"));
        assert_eq!(probe.version_admission, None);
        assert_eq!(probe.detail, None);

        let (mut driver, state) = generic_driver(
            workspace.path(),
            FixtureScenario::GenericAgentWithAuth,
            None,
        );
        let probe = driver.probe(&context("probe")).await.unwrap();
        wait_for_closed(&state).await;
        assert_eq!(request_methods(&state.received()), ["initialize"]);
        assert_eq!(probe.state, ProbeState::AuthenticationRequired);
        let detail = probe.detail.unwrap();
        for (id, name) in GENERIC_AUTH_METHODS {
            assert!(detail.contains(&format!("{name} ({id})")), "{detail}");
        }
    });
}

#[test]
fn generic_agent_switches_only_to_advertised_models() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("generic-model");
        let model = |id: &str| serde_json::from_value::<ModelId>(json!(id)).unwrap();

        let (mut driver, state) =
            generic_driver(workspace.path(), FixtureScenario::GenericAgent, None);
        let mut request = generic_start_request(workspace.path());
        request.model_id = Some(model(GENERIC_MODELS[1]));
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
        let error = driver
            .start_session(request, sink, &context("start"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::CapabilityUnsupported);
        assert!(!request_methods(&state.received()).contains(&"session/set_model"));

        for (requested, switches) in [(GENERIC_MODELS[0], false), (GENERIC_MODELS[1], true)] {
            let (mut driver, state) = generic_driver(
                workspace.path(),
                FixtureScenario::GenericAgentWithModels,
                None,
            );
            let mut request = generic_start_request(workspace.path());
            request.model_id = Some(model(requested));
            let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
            let session = driver
                .start_session(request, sink, &context("start"))
                .await
                .unwrap();
            assert!(
                session
                    .capabilities
                    .supports(ProviderCapability::DynamicModelChange)
            );
            let received = state.received();
            let set_model = received
                .iter()
                .find(|message| message["method"] == "session/set_model");
            assert_eq!(set_model.is_some(), switches, "{requested}");
            if let Some(set_model) = set_model {
                assert_eq!(set_model["params"]["modelId"], requested);
            }
        }

        let (mut driver, _state) = generic_driver(
            workspace.path(),
            FixtureScenario::GenericAgentWithModels,
            None,
        );
        let mut request = generic_start_request(workspace.path());
        request.model_id = Some(model("agent-unknown"));
        let sink: Arc<dyn ProviderEventSink> = Arc::new(RecordingSink::default());
        let error = driver
            .start_session(request, sink, &context("start"))
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
    });
}

#[test]
fn instructions_reach_each_native_session_once_across_resume() {
    assert_instructions_follow_the_native_session(false);
}

#[test]
fn generic_agent_instructions_reach_each_native_session_once_across_resume() {
    assert_instructions_follow_the_native_session(true);
}

/// Opens, resumes, and replaces a native session the way organizational runs do and checks
/// which prompts carry the instructions: a resume the caller reports as delivered omits them
/// unless they change, and every new native session receives them once.
fn assert_instructions_follow_the_native_session(generic: bool) {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("instructions-resume");
        let (mut driver, state) = if generic {
            generic_driver(workspace.path(), FixtureScenario::GenericAgent, None)
        } else {
            let state = Arc::new(AcpFixtureState::default());
            let factory_state = Arc::clone(&state);
            let mut driver =
                CursorProviderDriver::new(configuration(workspace.path(), None)).unwrap();
            driver.set_connection_factory(Arc::new(move |_workspace| {
                Ok((
                    fixture_connection(
                        FixtureScenario::CompletePrompts,
                        Arc::clone(&factory_state),
                    ),
                    about().into(),
                ))
            }));
            (driver, state)
        };
        let sink = Arc::new(RecordingSink::default());
        let rules = "Follow repository rules";
        let start = || {
            if generic {
                generic_start_request(workspace.path())
            } else {
                start_request(workspace.path())
            }
        };

        let first = open_session(&mut driver, &sink, SessionOpen::Start(start())).await;
        instructed_turn(
            &mut driver,
            &sink,
            &first.session_id,
            "turn-1",
            rules,
            generic,
        )
        .await;
        let resumed = |delivered: bool| {
            let mut request: ResumeSessionRequest = serde_json::from_value(json!({
                "threadId": start().thread_id,
                "workspace": start().workspace,
                "providerInstanceId": start().provider_instance_id,
                "providerThreadId": first.provider_thread_id,
                "continuationGroupId": first.continuation_group_id,
                "resumeCursor": first.resume_cursor,
                "modes": start().modes,
                "developerInstructions": rules,
            }))
            .unwrap();
            request.developer_instructions_delivered = delivered;
            SessionOpen::Resume(request)
        };
        let mut session = first.session_id.clone();
        for (turn, delivered, instructions) in [
            ("turn-2", true, rules),
            ("turn-3", true, "Ask before deleting files"),
            ("turn-4", false, rules),
        ] {
            stop(&mut driver, &session).await;
            session = open_session(&mut driver, &sink, resumed(delivered))
                .await
                .session_id;
            instructed_turn(&mut driver, &sink, &session, turn, instructions, generic).await;
        }
        stop(&mut driver, &session).await;
        let handoff = open_session(&mut driver, &sink, SessionOpen::Start(start())).await;
        for turn in ["turn-5", "turn-6"] {
            instructed_turn(
                &mut driver,
                &sink,
                &handoff.session_id,
                turn,
                rules,
                generic,
            )
            .await;
        }

        assert_eq!(
            prompt_instruction_flags(&state),
            [true, false, true, true, true, false],
            "a continued native session gets no instruction text, a change or an unreported \
             resume sends them, and a handoff session receives them once"
        );
    });
}

enum SessionOpen {
    Start(StartSessionRequest),
    Resume(ResumeSessionRequest),
}

async fn open_session(
    driver: &mut CursorProviderDriver,
    sink: &Arc<RecordingSink>,
    open: SessionOpen,
) -> ProviderSessionSnapshot {
    let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
    match open {
        SessionOpen::Start(request) => {
            driver
                .start_session(request, sink_trait, &context("start"))
                .await
        }
        SessionOpen::Resume(request) => {
            driver
                .resume_session(request, sink_trait, &context("resume"))
                .await
        }
    }
    .unwrap()
}

async fn stop(driver: &mut CursorProviderDriver, session_id: &ProviderSessionId) {
    driver
        .stop_session(
            StopSessionRequest {
                session_id: session_id.clone(),
                force: false,
            },
            &context("stop"),
        )
        .await
        .unwrap();
}

/// Sends one turn with `instructions` and waits until it completes.
async fn instructed_turn(
    driver: &mut CursorProviderDriver,
    sink: &RecordingSink,
    session_id: &ProviderSessionId,
    turn_id: &str,
    instructions: &str,
    generic: bool,
) {
    let completed_before = completed_turns(sink);
    let mut request = send_request(session_id);
    request.turn_id = serde_json::from_value(json!(turn_id)).unwrap();
    request.developer_instructions = Some(instructions.to_string());
    if generic {
        request.model_id = None;
        request.modes = modes(SafetyMode::AskForApproval, InteractionMode::Build);
    }
    driver.send_turn(request, &context(turn_id)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while completed_turns(sink) == completed_before {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn a_request_after_the_reader_closed_fails_at_once() {
    crate::test_block_on(async {
        let (application_reader, agent_writer) = tokio::io::duplex(1024);
        drop(agent_writer);
        let mut connection = AcpRpcConnection::from_test_io(application_reader, tokio::io::sink());
        let mut inbound = connection.take_inbound().unwrap();
        assert!(matches!(
            inbound.recv().await.unwrap(),
            AcpInboundMessage::Closed
        ));
        let mut long_wait = context("late");
        long_wait.deadline = std::time::Instant::now() + Duration::from_secs(60);

        let late = tokio::time::timeout(
            Duration::from_secs(5),
            connection.client().request("late", json!({}), &long_wait),
        )
        .await
        .expect("a closed connection refuses the request instead of waiting for its deadline");

        assert_eq!(late, Err(AcpRpcFailure::Closed));
    });
}

/// Starts a driver over the fixture `scenario` and returns it with its sink and first session.
async fn started_fixture_session(
    workspace: &TestDirectory,
    scenario: FixtureScenario,
    state: &Arc<AcpFixtureState>,
) -> (
    CursorProviderDriver,
    Arc<RecordingSink>,
    ProviderSessionSnapshot,
) {
    let factory_state = Arc::clone(state);
    let mut driver = CursorProviderDriver::new(configuration(workspace.path(), None)).unwrap();
    driver.set_connection_factory(Arc::new(move |_workspace| {
        Ok((
            fixture_connection(scenario, Arc::clone(&factory_state)),
            about().into(),
        ))
    }));
    let sink = Arc::new(RecordingSink::default());
    let sink_trait: Arc<dyn ProviderEventSink> = sink.clone();
    let snapshot = driver
        .start_session(
            start_request(workspace.path()),
            sink_trait,
            &context("start"),
        )
        .await
        .unwrap();
    (driver, sink, snapshot)
}

#[test]
fn a_refused_prompt_ends_only_its_turn() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("refused-prompt");
        let state = Arc::new(AcpFixtureState::default());
        let (mut driver, sink, snapshot) =
            started_fixture_session(&workspace, FixtureScenario::RefuseFirstPrompt, &state).await;

        driver
            .send_turn(send_request(&snapshot.session_id), &context("first"))
            .await
            .unwrap();
        wait_for_event(&sink, |event| {
            matches!(event, CanonicalEvent::TurnAborted(_))
        })
        .await;
        let mut second = serde_json::to_value(send_request(&snapshot.session_id)).unwrap();
        second["turnId"] = json!("cursor-turn-second");
        let receipt = driver
            .send_turn(serde_json::from_value(second).unwrap(), &context("second"))
            .await
            .expect("a turn-scoped prompt error leaves the session usable");

        assert_eq!(receipt.state, ChatTurnState::Active);
        wait_for_event(&sink, |event| {
            matches!(event, CanonicalEvent::TurnCompleted(_))
        })
        .await;
        assert!(!sink.events().iter().any(|event| matches!(
            &event.event,
            CanonicalEvent::SessionStateChanged(changed)
                if changed.state == ProviderSessionState::Failed
        )));
    });
}

#[test]
fn a_frame_the_router_cannot_read_fails_the_session_and_stops_the_agent() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("malformed-prompt");
        let state = Arc::new(AcpFixtureState::default());
        let (mut driver, sink, snapshot) =
            started_fixture_session(&workspace, FixtureScenario::MalformedDuringPrompt, &state)
                .await;

        driver
            .send_turn(send_request(&snapshot.session_id), &context("send"))
            .await
            .unwrap();
        wait_for_event(&sink, |event| {
            matches!(
                event,
                CanonicalEvent::SessionStateChanged(changed)
                    if changed.state == ProviderSessionState::Failed
            )
        })
        .await;

        wait_for_closed(&state).await;
        let mut second = serde_json::to_value(send_request(&snapshot.session_id)).unwrap();
        second["turnId"] = json!("cursor-turn-second");
        let refused = driver
            .send_turn(serde_json::from_value(second).unwrap(), &context("second"))
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Protocol);
        assert!(
            sink.events()
                .iter()
                .any(|event| matches!(event.event, CanonicalEvent::TurnAborted(_)))
        );
    });
}

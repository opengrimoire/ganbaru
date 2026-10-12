//! Cursor ACP transport and driver integration fixtures.

use super::support::*;
use crate::cursor::driver::CursorProviderDriver;
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

//! Driver tests for organizational Claude init verification over a scripted connection.

use super::{RecordingSink, TestDirectory, configuration, context, modes};
use crate::ProviderDriver;
use crate::claude::driver::ClaudeProviderDriver;
use crate::claude::home::ClaudeHome;
use crate::claude::protocol::{ClaudeVersion, MINIMUM_CLAUDE_VERSION};
use crate::claude::transport::ClaudeJsonlConnection;
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

const SERVER_NAME: &str = "ganbaru-chat";
const SESSION_UUID: &str = "11111111-1111-4111-8111-111111111111";
const DRIVER_EOF: &str = "driver-eof";

/// The test's view of a scripted Claude process: every line the driver writes, and a queue
/// of frames to send back. A line `{"type": "driver-eof"}` marks the driver closing stdin.
struct ScriptedClaude {
    written: mpsc::UnboundedReceiver<Value>,
    frames: mpsc::UnboundedSender<Value>,
}

impl ScriptedClaude {
    async fn next_written(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(5), self.written.recv())
            .await
            .expect("the driver writes before the timeout")
            .expect("the scripted process is running")
    }

    /// Waits for the next written line of `kind`, collecting every line seen on the way.
    async fn wait_for(&mut self, kind: &str, seen: &mut Vec<Value>) -> Value {
        loop {
            let line = self.next_written().await;
            seen.push(line.clone());
            if line.get("type").and_then(Value::as_str) == Some(kind) {
                return line;
            }
        }
    }

    fn send(&self, frame: Value) {
        self.frames.send(frame).unwrap();
    }
}

/// Builds a connection whose process answers `initialize`, answers `mcp_status` with
/// `mcp_status`, and acknowledges every other control request.
fn scripted_connection(
    home: PathBuf,
    mcp_status: Value,
) -> (
    (ClaudeJsonlConnection, ClaudeHome, ClaudeVersion),
    ScriptedClaude,
) {
    let (driver_reader, mut process_writer) = tokio::io::duplex(128 * 1024);
    let (process_reader, driver_writer) = tokio::io::duplex(128 * 1024);
    let (written_sender, written) = mpsc::unbounded_channel();
    let (frames, mut frame_receiver) = mpsc::unbounded_channel::<Value>();
    tokio::spawn(async move {
        let mut reader = BufReader::new(process_reader);
        let mut line = String::new();
        loop {
            let outgoing = tokio::select! {
                read = reader.read_line(&mut line) => {
                    if read.unwrap_or(0) == 0 {
                        let _ = written_sender.send(json!({ "type": DRIVER_EOF }));
                        break;
                    }
                    let value: Value = serde_json::from_str(&line).unwrap();
                    let _ = written_sender.send(value.clone());
                    line.clear();
                    control_reply(&value, &mcp_status)
                }
                Some(frame) = frame_receiver.recv() => Some(frame),
            };
            if let Some(outgoing) = outgoing {
                let mut bytes = serde_json::to_vec(&outgoing).unwrap();
                bytes.push(b'\n');
                if process_writer.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        }
    });
    let connection = ClaudeJsonlConnection::from_test_io(driver_reader, driver_writer);
    (
        (
            connection,
            ClaudeHome {
                config_directory: home,
            },
            MINIMUM_CLAUDE_VERSION,
        ),
        ScriptedClaude { written, frames },
    )
}

fn control_reply(value: &Value, mcp_status: &Value) -> Option<Value> {
    if value.get("type").and_then(Value::as_str) != Some("control_request") {
        return None;
    }
    let request_id = value.get("request_id")?.clone();
    let response = match value.pointer("/request/subtype").and_then(Value::as_str) {
        Some("initialize") => json!({ "commands": [], "models": [], "account": null }),
        Some("mcp_status") => mcp_status.clone(),
        _ => json!({}),
    };
    Some(json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    }))
}

fn organizational_configuration(home: &Path, organizational: bool) -> ProviderInstanceConfig {
    let mut configuration = configuration(home);
    configuration.internal_mcp = Some(ProviderInternalMcpConfig {
        name: SERVER_NAME.to_string(),
        url: "http://127.0.0.1:41234/mcp".to_string(),
        bearer_token: "test-token".to_string(),
        organizational_authority: organizational,
    });
    configuration
}

fn internal_status(status: &str) -> Value {
    json!({ "mcpServers": [{ "name": SERVER_NAME, "status": status }] })
}

fn init_frame(servers: Value, tools: Value) -> Value {
    json!({
        "type": "system",
        "subtype": "init",
        "session_id": SESSION_UUID,
        "model": "claude-sonnet-4-5",
        "permissionMode": "default",
        "tools": tools,
        "mcp_servers": servers,
    })
}

fn verified_init() -> Value {
    init_frame(
        json!([{ "name": SERVER_NAME, "status": "connected", "source": "dynamic" }]),
        json!(["Read", "Edit", "mcp__ganbaru-chat__ping"]),
    )
}

fn assistant_text(uuid: &str, text: &str) -> Value {
    json!({
        "type": "assistant",
        "uuid": uuid,
        "session_id": SESSION_UUID,
        "message": { "role": "assistant", "content": [{ "type": "text", "text": text }] },
    })
}

fn can_use_tool(request_id: &str) -> Value {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {
            "subtype": "can_use_tool",
            "tool_name": "Edit",
            "input": { "file_path": "notes.txt" },
            "tool_use_id": "tool-edit",
        },
    })
}

struct GateHarness {
    driver: ClaudeProviderDriver,
    claude: Option<ScriptedClaude>,
    sink: Arc<RecordingSink>,
    workspace: TestDirectory,
    _home: TestDirectory,
}

impl GateHarness {
    fn new(organizational: bool, mcp_status: Value) -> Self {
        let home = TestDirectory::new("gate-home");
        let workspace = TestDirectory::new("gate-workspace");
        let mut driver =
            ClaudeProviderDriver::new(organizational_configuration(home.path(), organizational))
                .unwrap();
        let (pair, claude) = scripted_connection(home.path().to_path_buf(), mcp_status);
        let pair = Arc::new(Mutex::new(Some(pair)));
        driver.set_connection_factory(Arc::new(move |_| {
            pair.lock().unwrap().take().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Conflict,
                    "test connection already consumed",
                    false,
                )
            })
        }));
        Self {
            driver,
            claude: Some(claude),
            sink: Arc::new(RecordingSink::default()),
            workspace,
            _home: home,
        }
    }

    fn claude(&mut self) -> &mut ScriptedClaude {
        self.claude.as_mut().unwrap()
    }

    async fn start(&mut self) -> ChatResult<ProviderSessionSnapshot> {
        self.driver
            .start_session(
                StartSessionRequest {
                    thread_id: ChatThreadId::new("thread-1".to_string()).unwrap(),
                    workspace: VerifiedWorkspaceContext {
                        working_folder_id: ProjectWorkingFolderId::new("workspace-1".to_string())
                            .unwrap(),
                        canonical_path: self.workspace.path().to_string_lossy().into_owned(),
                        repository_kind: RepositoryKind::None,
                        repository_identity: None,
                        project_configuration: ProjectConfigurationAccess::Restricted,
                    },
                    provider_instance_id: ProviderInstanceId::new("claude-instance-1".to_string())
                        .unwrap(),
                    modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
                    model_id: None,
                    model_options: Vec::new(),
                    developer_instructions: None,
                },
                self.sink.clone(),
                &context("start"),
            )
            .await
    }

    async fn send_turn(
        &mut self,
        session_id: &ProviderSessionId,
        turn: &str,
    ) -> ChatResult<TurnDispatchReceipt> {
        self.driver
            .send_turn(
                SendTurnRequest {
                    command: ChatCommandContext {
                        client_command_id: ChatCommandId::new(format!("command-{turn}")).unwrap(),
                        expected_thread_revision: None,
                    },
                    session_id: session_id.clone(),
                    turn_id: ChatTurnId::new(turn.to_string()).unwrap(),
                    prompt: "Work on the notes".to_string(),
                    attachments: Vec::new(),
                    mentions: Vec::new(),
                    model_id: None,
                    model_options: Vec::new(),
                    modes: modes(SafetyMode::AskForApproval, InteractionMode::Build),
                    developer_instructions: None,
                },
                &context(turn),
            )
            .await
    }

    fn events(&self) -> Vec<CanonicalRuntimeEvent> {
        self.sink.events()
    }
}

fn is_assistant_output(event: &CanonicalEvent) -> bool {
    matches!(
        event,
        CanonicalEvent::ContentDelta(_)
            | CanonicalEvent::ItemStarted(_)
            | CanonicalEvent::ItemUpdated(_)
            | CanonicalEvent::ItemCompleted(_)
            | CanonicalEvent::RequestOpened(_)
            | CanonicalEvent::TurnCompleted(_)
    )
}

fn assert_failed_closed(events: &[CanonicalRuntimeEvent]) {
    let aborted = events
        .iter()
        .find_map(|event| match &event.event {
            CanonicalEvent::TurnAborted(aborted) => Some(aborted),
            _ => None,
        })
        .expect("the turn is aborted");
    assert_eq!(aborted.state, ChatTurnState::Failed);
    assert!(!aborted.recoverable);
    assert!(events.iter().any(|event| matches!(
        &event.event,
        CanonicalEvent::SessionStateChanged(changed) if changed.state == ProviderSessionState::Failed
    )));
}

fn written_subtypes(lines: &[Value]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| line.pointer("/request/subtype").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

#[test]
fn organizational_gate_one_failure_never_reports_ready() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(
            true,
            json!({ "mcpServers": [
                { "name": SERVER_NAME, "status": "connected" },
                { "name": "user-server", "status": "connected" }
            ] }),
        );
        let error = harness.start().await.unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
        assert!(!error.recoverable);
        let mut seen = Vec::new();
        harness.claude().wait_for(DRIVER_EOF, &mut seen).await;
        assert_eq!(written_subtypes(&seen), ["initialize", "mcp_status"]);
        assert!(
            !harness.events().iter().any(|event| matches!(
                event.event,
                CanonicalEvent::SessionStarted(_) | CanonicalEvent::SessionConfigured(_)
            )),
            "a failed gate never reports the session"
        );
    });
}

#[test]
fn organizational_gate_one_rejects_a_failed_internal_server() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(true, internal_status("needs-auth"));
        assert_eq!(
            harness.start().await.unwrap_err(),
            crate::claude::protocol::organizational_permission_error()
        );
    });
}

#[test]
fn organizational_gate_two_failure_stops_the_process_and_hides_output() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(true, internal_status("pending"));
        let snapshot = harness.start().await.unwrap();
        harness
            .send_turn(&snapshot.session_id, "turn-1")
            .await
            .unwrap();
        let mut seen = Vec::new();
        harness.claude().wait_for("user", &mut seen).await;
        assert!(
            written_subtypes(&seen).contains(&"set_permission_mode".to_string()),
            "{seen:?}"
        );
        let mode = seen
            .iter()
            .find(|line| {
                line.pointer("/request/subtype").and_then(Value::as_str)
                    == Some("set_permission_mode")
            })
            .and_then(|line| line.pointer("/request/mode"))
            .and_then(Value::as_str);
        assert_eq!(mode, Some("default"));
        harness.claude().send(assistant_text(
            "33333333-3333-4333-8333-333333333333",
            "held text",
        ));
        harness.claude().send(can_use_tool("approval-early"));
        let denial = harness
            .claude()
            .wait_for("control_response", &mut seen)
            .await;
        assert_eq!(
            denial
                .pointer("/response/request_id")
                .and_then(Value::as_str),
            Some("approval-early")
        );
        assert_eq!(
            denial
                .pointer("/response/response/behavior")
                .and_then(Value::as_str),
            Some("deny")
        );
        harness.claude().send(init_frame(
            json!([{ "name": SERVER_NAME, "status": "connected" }]),
            json!(["Read", "mcp__user-server__fetch"]),
        ));
        harness.claude().wait_for(DRIVER_EOF, &mut seen).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let events = harness.events();
        assert!(
            !events.iter().any(|event| is_assistant_output(&event.event)),
            "no held output reaches the timeline: {events:?}"
        );
        assert_failed_closed(&events);
        let error = harness
            .send_turn(&snapshot.session_id, "turn-2")
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
    });
}

#[test]
fn organizational_gate_two_times_out_closed() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let mut harness = GateHarness::new(true, internal_status("connected"));
            let snapshot = harness.start().await.unwrap();
            harness
                .send_turn(&snapshot.session_id, "turn-1")
                .await
                .unwrap();
            let mut seen = Vec::new();
            harness.claude().wait_for("user", &mut seen).await;
            tokio::time::sleep(Duration::from_secs(29)).await;
            assert!(
                !harness
                    .events()
                    .iter()
                    .any(|event| matches!(event.event, CanonicalEvent::TurnAborted(_))),
                "the deadline has not passed yet"
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
            harness.claude().wait_for(DRIVER_EOF, &mut seen).await;
            assert_failed_closed(&harness.events());
        });
}

#[test]
fn organizational_gate_routes_held_frames_after_a_verified_init() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(true, internal_status("connected"));
        let snapshot = harness.start().await.unwrap();
        harness
            .send_turn(&snapshot.session_id, "turn-1")
            .await
            .unwrap();
        let mut seen = Vec::new();
        harness.claude().wait_for("user", &mut seen).await;
        harness.claude().send(
            json!({ "type": "rate_limit_event", "rate_limit_info": { "status": "allowed" } }),
        );
        harness.claude().send(verified_init());
        harness.claude().send(can_use_tool("approval-verified"));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let events = harness.events();
        let thread_started = events
            .iter()
            .position(|event| matches!(event.event, CanonicalEvent::ThreadStarted(_)))
            .expect("the verified init is routed");
        let request = events
            .iter()
            .position(|event| matches!(event.event, CanonicalEvent::RequestOpened(_)))
            .expect("a verified session opens approval requests");
        assert!(thread_started < request);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event.event, CanonicalEvent::TurnAborted(_)))
        );
    });
}

#[test]
fn organizational_gate_verifies_every_later_init() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(true, internal_status("connected"));
        let snapshot = harness.start().await.unwrap();
        harness
            .send_turn(&snapshot.session_id, "turn-1")
            .await
            .unwrap();
        let mut seen = Vec::new();
        harness.claude().wait_for("user", &mut seen).await;
        harness.claude().send(verified_init());
        harness.claude().send(init_frame(
            json!([{ "name": SERVER_NAME, "status": "failed" }]),
            json!(["Read"]),
        ));
        harness.claude().wait_for(DRIVER_EOF, &mut seen).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_failed_closed(&harness.events());
    });
}

#[test]
fn direct_sessions_skip_the_mcp_status_gate() {
    crate::test_block_on(async {
        let mut harness = GateHarness::new(false, json!({}));
        let snapshot = harness.start().await.unwrap();
        harness
            .send_turn(&snapshot.session_id, "turn-1")
            .await
            .unwrap();
        let mut seen = Vec::new();
        harness.claude().wait_for("user", &mut seen).await;
        assert!(!written_subtypes(&seen).contains(&"mcp_status".to_string()));
        harness.claude().send(assistant_text(
            "44444444-4444-4444-8444-444444444444",
            "direct text",
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            harness
                .events()
                .iter()
                .any(|event| is_assistant_output(&event.event)),
            "direct sessions route output without an init"
        );
    });
}

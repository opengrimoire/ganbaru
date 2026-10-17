//! Shared Cursor ACP fixture support.

use crate::cursor::executable::{CursorAbout, parse_version};
use crate::cursor::transport::AcpRpcConnection;
use crate::events::CanonicalRuntimeEvent;
use crate::models::*;
use crate::{DriverCancellation, DriverFuture, DriverOperationContext, ProviderEventSink};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

pub struct TestDirectory(PathBuf);

impl TestDirectory {
    pub fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-cursor-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
pub struct RecordingSink {
    events: Mutex<Vec<CanonicalRuntimeEvent>>,
}

impl RecordingSink {
    pub fn events(&self) -> Vec<CanonicalRuntimeEvent> {
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

pub fn context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + Duration::from_secs(3),
        cancellation: DriverCancellation::default(),
    }
}

pub fn modes(safety_mode: SafetyMode, interaction_mode: InteractionMode) -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode,
        interaction_mode,
    }
}

pub fn configuration(home: &Path, endpoint: Option<&str>) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "cursor-instance-1",
        "familyId": "cursor",
        "label": "Cursor",
        "enabled": true,
        "executable": "cursor-agent",
        "providerHome": home,
        "launchArguments": [],
        "environment": {},
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": {
            "schemaVersion": 1,
            "value": { "apiEndpoint": endpoint }
        }
    }))
    .unwrap()
}

pub fn config_options() -> Value {
    json!([
        {
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": "cursor-small",
            "options": [
                { "value": "cursor-small", "name": "Cursor Small" },
                { "value": "cursor-large", "name": "Cursor Large" }
            ]
        },
        {
            "id": "reasoning",
            "name": "Reasoning effort",
            "type": "select",
            "currentValue": "medium",
            "options": [
                { "value": "medium", "name": "Medium" },
                { "value": "high", "name": "High" }
            ]
        },
        {
            "id": "context_size",
            "name": "Context window",
            "type": "select",
            "currentValue": "standard",
            "options": [
                { "value": "standard", "name": "Standard" },
                { "value": "max", "name": "Maximum" }
            ]
        },
        {
            "id": "fast",
            "name": "Fast mode",
            "type": "boolean",
            "currentValue": false,
            "options": []
        },
        {
            "id": "thinking",
            "name": "Thinking",
            "type": "boolean",
            "currentValue": true,
            "options": []
        },
        {
            "id": "mode",
            "name": "Mode",
            "category": "mode",
            "type": "select",
            "currentValue": "code",
            "options": [
                { "value": "code", "name": "Code" },
                { "value": "plan", "name": "Plan" }
            ]
        }
    ])
}

pub fn initialize_response(advertises_auth: bool) -> Value {
    json!({
        "protocolVersion": 1,
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": {
                "image": true,
                "audio": false,
                "embeddedContext": false
            },
            "sessionCapabilities": {}
        },
        "authMethods": if advertises_auth {
            json!([{ "id": "cursor_login", "name": "Cursor login" }])
        } else {
            json!([])
        },
        "agentInfo": {
            "name": "cursor-agent",
            "title": "Cursor Agent",
            "version": "2026.04.08"
        }
    })
}

pub fn session_setup(session_id: Option<&str>, options: Value) -> Value {
    json!({
        "sessionId": session_id,
        "configOptions": options
    })
}

pub fn models_response() -> Value {
    json!({
        "models": [
            {
                "value": "cursor-small",
                "name": "Cursor Small",
                "configOptions": config_options()
            },
            {
                "value": "cursor-large",
                "name": "Cursor Large",
                "configOptions": config_options()
            }
        ]
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureScenario {
    Healthy,
    AuthenticationRequired,
    ResumeMissing,
    RejectFastMode,
}

#[derive(Default)]
pub struct AcpFixtureState {
    received: Mutex<Vec<Value>>,
    pub closed: AtomicBool,
}

impl AcpFixtureState {
    pub fn received(&self) -> Vec<Value> {
        self.received.lock().unwrap().clone()
    }
}

pub fn request_methods(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| message.get("method").and_then(Value::as_str))
        .collect()
}

pub fn fixture_connection(
    scenario: FixtureScenario,
    state: Arc<AcpFixtureState>,
) -> AcpRpcConnection {
    let (application, agent) = tokio::io::duplex(128 * 1024);
    let (application_reader, application_writer) = tokio::io::split(application);
    let (agent_reader, agent_writer) = tokio::io::split(agent);
    tokio::spawn(run_fixture(agent_reader, agent_writer, scenario, state));
    AcpRpcConnection::from_test_io(application_reader, application_writer)
}

pub fn about() -> CursorAbout {
    CursorAbout {
        version: parse_version("2026.04.08").unwrap(),
        account_label: Some("fixture@example.test".to_string()),
        authenticated: Some(true),
    }
}

async fn write_message<W>(writer: &mut W, message: Value)
where
    W: AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(&message).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}

async fn run_fixture<R, W>(
    reader: R,
    mut writer: W,
    scenario: FixtureScenario,
    state: Arc<AcpFixtureState>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut options = config_options();
    let mut pending_prompt = None;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let message: Value = serde_json::from_str(&line).unwrap();
        state.received.lock().unwrap().push(message.clone());
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let id = message.get("id").cloned();
        match method {
            "initialize" => {
                write_result(&mut writer, id, initialize_response(true)).await;
            }
            "authenticate" if scenario == FixtureScenario::AuthenticationRequired => {
                write_error(&mut writer, id, -32000, "Login required").await;
            }
            "authenticate" => write_result(&mut writer, id, json!({})).await,
            "session/load" if scenario == FixtureScenario::ResumeMissing => {
                write_error(&mut writer, id, -32004, "Session not found").await;
            }
            "session/new" => {
                write_result(
                    &mut writer,
                    id,
                    session_setup(Some("cursor-session-fixture"), options.clone()),
                )
                .await;
            }
            "session/load" => {
                write_result(&mut writer, id, session_setup(None, options.clone())).await;
            }
            "session/set_config_option"
                if scenario == FixtureScenario::RejectFastMode
                    && message["params"]["configId"] == "fast" =>
            {
                write_error(&mut writer, id, -32602, "Fast mode unavailable").await;
            }
            "session/set_config_option" => {
                let config_id = message["params"]["configId"].as_str().unwrap();
                let value = message["params"]["value"].clone();
                if let Some(entries) = options.as_array_mut() {
                    if let Some(entry) = entries.iter_mut().find(|entry| entry["id"] == config_id) {
                        entry["currentValue"] = value;
                    }
                }
                write_result(&mut writer, id, json!({ "configOptions": options.clone() })).await;
            }
            "session/set_mode" => write_result(&mut writer, id, json!({})).await,
            "cursor/list_available_models" => {
                write_result(&mut writer, id, models_response()).await;
            }
            "session/prompt" => {
                pending_prompt = id;
                write_message(
                    &mut writer,
                    json!({
                        "jsonrpc": "2.0",
                        "method": "session/update",
                        "params": {
                            "sessionId": "cursor-session-fixture",
                            "update": {
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": "Fixture response" }
                            }
                        }
                    }),
                )
                .await;
                write_message(
                    &mut writer,
                    json!({
                        "jsonrpc": "2.0",
                        "id": "cursor-plan-request",
                        "method": "cursor/create_plan",
                        "params": {
                            "toolCallId": "fixture-plan-tool",
                            "name": "Fixture plan",
                            "overview": "Validate the ACP route",
                            "plan": "1. Inspect\n2. Verify",
                            "todos": []
                        }
                    }),
                )
                .await;
            }
            "session/cancel" => {
                if let Some(prompt_id) = pending_prompt.take() {
                    write_result(
                        &mut writer,
                        Some(prompt_id),
                        json!({ "stopReason": "cancelled" }),
                    )
                    .await;
                }
            }
            _ => {
                if id.is_some() {
                    write_result(&mut writer, id, json!({})).await;
                }
            }
        }
    }
    state.closed.store(true, Ordering::Release);
}

async fn write_result<W>(writer: &mut W, id: Option<Value>, result: Value)
where
    W: AsyncWrite + Unpin,
{
    if let Some(id) = id {
        write_message(
            writer,
            json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        )
        .await;
    }
}

async fn write_error<W>(writer: &mut W, id: Option<Value>, code: i64, message: &str)
where
    W: AsyncWrite + Unpin,
{
    if let Some(id) = id {
        write_message(
            writer,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": code, "message": message }
            }),
        )
        .await;
    }
}

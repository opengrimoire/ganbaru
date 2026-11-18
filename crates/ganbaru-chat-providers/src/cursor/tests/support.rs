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
        "providerConfig": {
            "schemaVersion": 1,
            "value": { "apiEndpoint": endpoint }
        }
    }))
    .unwrap()
}

/// An `acp` instance for a plain ACP agent, with the authentication method it should use.
pub fn generic_configuration(home: &Path, auth_method: Option<&str>) -> ProviderInstanceConfig {
    let mut config = configuration(home, None);
    config.family_id = serde_json::from_value(json!("acp")).unwrap();
    config.instance_id = serde_json::from_value(json!("acp-instance-1")).unwrap();
    config.label = "Fixture agent".to_string();
    config.executable = "fixture-agent".to_string();
    config.provider_config.value = json!({ "authMethod": auth_method });
    config
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
    /// `session/load` replays history, a command list, mode and configuration updates, and a
    /// permission request before and after its response.
    ReplayOnLoad,
    /// Every `session/prompt` reports usage with a cost and ends its turn at once.
    CompletePrompts,
    /// A plain ACP agent: no vendor metadata or models, and a prompt that also sends Cursor
    /// extension calls the generic flavor must refuse.
    GenericAgent,
    /// A plain ACP agent that advertises the `GENERIC_AUTH_METHODS` for `authenticate`.
    /// A plain ACP agent whose sessions advertise the `GENERIC_MODELS` catalog.
    GenericAgentWithModels,
    GenericAgentWithAuth,
}

impl FixtureScenario {
    /// Whether the fixture plays a plain ACP agent instead of Cursor.
    fn is_generic(self) -> bool {
        matches!(
            self,
            Self::GenericAgent | Self::GenericAgentWithAuth | Self::GenericAgentWithModels
        )
    }
}

/// Context tokens the `CompletePrompts` fixture reports in use.
pub const FIXTURE_USAGE_USED: u64 = 1_200;
/// Context window the `CompletePrompts` fixture reports.
pub const FIXTURE_USAGE_SIZE: u64 = 200_000;
/// Authentication methods the `GenericAgentWithAuth` fixture advertises, as id and name.
pub const GENERIC_AUTH_METHODS: [(&str, &str); 2] =
    [("oauth", "Browser sign in"), ("api-key", "API key")];
/// Models the `GenericAgentWithModels` fixture advertises; the first is current.
pub const GENERIC_MODELS: [&str; 2] = ["agent-small", "agent-large"];
/// Cursor extension calls the generic fixture sends during a prompt, as method and request id.
pub const GENERIC_EXTENSION_CALLS: [(&str, Option<&str>); 3] = [
    ("cursor/update_todos", None),
    ("cursor/ask_question", Some("generic-question")),
    ("cursor/create_plan", Some("generic-plan")),
];

/// Replayed updates the `ReplayOnLoad` fixture sends, more than the inbound channel holds.
pub const LOAD_REPLAY_UPDATES: usize = 300;
/// Text of every replayed history update.
pub const REPLAYED_HISTORY_TEXT: &str = "Replayed history";
/// Request id of the permission request replayed during `session/load`.
pub const REPLAYED_PERMISSION_ID: &str = "replayed-permission";
/// Delay between the `session/load` response and the last replayed update, shorter than the
/// quiet period tests use.
pub const LATE_REPLAY_DELAY: Duration = Duration::from_millis(50);
/// Mode the `ReplayOnLoad` fixture replays before and after its response.
pub const REPLAYED_MODE_ID: &str = "ask";
/// Last mode the `ReplayOnLoad` fixture reports after its response.
pub const LATE_MODE_ID: &str = "plan";

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
            "initialize" if scenario.is_generic() => {
                let methods = if scenario == FixtureScenario::GenericAgentWithAuth {
                    GENERIC_AUTH_METHODS
                        .iter()
                        .map(|(id, name)| json!({ "id": id, "name": name }))
                        .collect()
                } else {
                    Vec::new()
                };
                write_result(
                    &mut writer,
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": { "loadSession": true },
                        "authMethods": methods,
                        "agentInfo": { "name": "fixture-agent", "version": "0.4.0" }
                    }),
                )
                .await;
            }
            "initialize" => {
                write_result(&mut writer, id, initialize_response(true)).await;
            }
            "session/new" if scenario.is_generic() => {
                let mut setup = json!({ "sessionId": "cursor-session-fixture" });
                if scenario == FixtureScenario::GenericAgentWithModels {
                    setup["models"] = json!({
                        "currentModelId": GENERIC_MODELS[0],
                        "availableModels": GENERIC_MODELS
                            .iter()
                            .map(|model| json!({ "modelId": model, "name": model }))
                            .collect::<Vec<_>>()
                    });
                }
                write_result(&mut writer, id, setup).await;
            }
            "session/prompt" if scenario.is_generic() => {
                for (extension, request_id) in GENERIC_EXTENSION_CALLS {
                    let mut call = json!({ "jsonrpc": "2.0", "method": extension, "params": {} });
                    if let Some(request_id) = request_id {
                        call["id"] = json!(request_id);
                    }
                    write_message(&mut writer, call).await;
                }
                write_message(
                    &mut writer,
                    session_update(json!({
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "Fixture response" }
                    })),
                )
                .await;
                write_result(&mut writer, id, json!({ "stopReason": "end_turn" })).await;
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
            "session/load" if scenario == FixtureScenario::ReplayOnLoad => {
                for _ in 0..LOAD_REPLAY_UPDATES {
                    write_message(&mut writer, replayed_text_update()).await;
                }
                write_message(
                    &mut writer,
                    session_update(json!({
                        "sessionUpdate": "available_commands_update",
                        "availableCommands": [{ "name": "review", "description": "Review changes" }]
                    })),
                )
                .await;
                write_message(
                    &mut writer,
                    json!({
                        "jsonrpc": "2.0",
                        "id": REPLAYED_PERMISSION_ID,
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": "cursor-session-fixture",
                            "toolCall": { "toolCallId": "replayed-tool", "title": "Replayed edit" },
                            "options": [{ "optionId": "allow", "name": "Allow", "kind": "allow_once" }]
                        }
                    }),
                )
                .await;
                write_message(&mut writer, mode_update(REPLAYED_MODE_ID)).await;
                write_result(&mut writer, id, session_setup(None, options.clone())).await;
                tokio::time::sleep(LATE_REPLAY_DELAY).await;
                write_message(&mut writer, replayed_text_update()).await;
                write_message(
                    &mut writer,
                    session_update(json!({
                        "sessionUpdate": "config_option_update",
                        "configOptions": config_options()
                    })),
                )
                .await;
                write_message(&mut writer, mode_update(REPLAYED_MODE_ID)).await;
                write_message(&mut writer, mode_update(LATE_MODE_ID)).await;
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
            "session/prompt" if scenario == FixtureScenario::CompletePrompts => {
                write_message(
                    &mut writer,
                    session_update(json!({
                        "sessionUpdate": "usage_update",
                        "used": FIXTURE_USAGE_USED,
                        "size": FIXTURE_USAGE_SIZE,
                        "cost": { "amount": 0.25, "currency": "USD" }
                    })),
                )
                .await;
                write_result(&mut writer, id, json!({ "stopReason": "end_turn" })).await;
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

fn session_update(update: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": { "sessionId": "cursor-session-fixture", "update": update }
    })
}

fn mode_update(mode_id: &str) -> Value {
    session_update(json!({ "sessionUpdate": "current_mode_update", "currentModeId": mode_id }))
}

fn replayed_text_update() -> Value {
    session_update(json!({
        "sessionUpdate": "agent_message_chunk",
        "content": { "type": "text", "text": REPLAYED_HISTORY_TEXT }
    }))
}

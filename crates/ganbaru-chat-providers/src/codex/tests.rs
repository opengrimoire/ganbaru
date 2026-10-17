use super::driver::{
    CodexNativeCommand, CodexProviderDriver, codex_native_command, confirmed_resume_not_found,
    goal_request, parse_mcp_status_page, validated_app_server_arguments,
};
use super::home::*;
use super::normalizer::{CodexEventNormalizer, CodexRouteState};
use super::organizational::*;
use super::protocol::*;
use super::session::{
    CodexApprovalResponse, PendingCodexRequest, PendingCodexRequestKind, PendingCodexRequests,
    resolve_codex_approval, resolve_codex_user_input,
};
use super::transport::{CodexInboundMessage, CodexRpcConnection, CodexRpcFailure};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use crate::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

mod commands;
mod driver;
mod home;
mod interactions;
mod models;
mod normalizer;
mod transport;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-codex-{label}-{}-{}",
            std::process::id(),
            NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<CanonicalRuntimeEvent>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<CanonicalRuntimeEvent> {
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

fn context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + Duration::from_secs(2),
        cancellation: DriverCancellation::default(),
    }
}

fn identifier<T>(value: &str, constructor: impl FnOnce(String) -> Result<T, String>) -> T {
    constructor(value.to_string()).unwrap()
}

fn configuration(shared_home: &Path, shadow_home: Option<&Path>) -> ProviderInstanceConfig {
    serde_json::from_value(json!({
        "schemaVersion": 1,
        "instanceId": "codex-instance-1",
        "familyId": "codex",
        "label": "Codex",
        "enabled": true,
        "executable": "codex",
        "providerHome": shared_home,
        "launchArguments": [],
        "environment": {},
        "credentialReferences": {},
        "visibleModelIds": [],
        "favoriteModelIds": [],
        "providerConfig": {
            "schemaVersion": 1,
            "value": {
                "shadowHomePath": shadow_home,
                "refreshMcpBeforeTurn": true,
                "allowCustomModels": false,
                "customModelIds": [],
                "customModelLabels": {}
            }
        }
    }))
    .unwrap()
}

fn modes(safety_mode: SafetyMode, interaction_mode: InteractionMode) -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode,
        interaction_mode,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FixtureScenario {
    Healthy,
    Organizational,
    ResumeMissing,
    ResumeWithoutStarted,
    AuthenticationRequired,
    MalformedInitialize,
    CommandApproval,
    StructuredQuestion,
    TurnNotInProgress,
}

#[derive(Default)]
struct AppServerFixtureState {
    received: Mutex<Vec<Value>>,
    closed: AtomicBool,
    connections: AtomicU64,
}

impl AppServerFixtureState {
    fn received(&self) -> Vec<Value> {
        self.received.lock().unwrap().clone()
    }
}

fn fixture_driver(
    workspace: &Path,
    scenario: FixtureScenario,
) -> (CodexProviderDriver, Arc<AppServerFixtureState>) {
    let mut config = configuration(workspace, None);
    if scenario == FixtureScenario::Organizational {
        config.internal_mcp = Some(ProviderInternalMcpConfig {
            name: "ganbaru-chat".to_string(),
            url: "http://127.0.0.1:41827/mcp".to_string(),
            bearer_token: "fixture-token".to_string(),
            organizational_authority: true,
        });
    }
    let mut driver = CodexProviderDriver::new(config).unwrap();
    let state = Arc::new(AppServerFixtureState::default());
    let factory_state = Arc::clone(&state);
    let home = workspace.to_path_buf();
    driver.set_connection_factory(Arc::new(move |_working_directory| {
        let connection_index = factory_state.connections.fetch_add(1, Ordering::AcqRel);
        let (client_reader, server_writer) = tokio::io::duplex(64 * 1024);
        let (server_reader, client_writer) = tokio::io::duplex(64 * 1024);
        tokio::spawn(run_app_server_fixture(
            server_reader,
            server_writer,
            home.clone(),
            scenario,
            connection_index,
            Arc::clone(&factory_state),
        ));
        Ok((
            CodexRpcConnection::from_test_io(client_reader, client_writer),
            CodexHomeLayout {
                shared_home: home.clone(),
                effective_home: home.clone(),
                shadowed: false,
            },
        ))
    }));
    (driver, state)
}

async fn write_fixture_message<W>(writer: &mut W, message: Value)
where
    W: AsyncWrite + Unpin,
{
    let mut bytes = serde_json::to_vec(&message).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}

async fn run_app_server_fixture<R, W>(
    reader: R,
    mut writer: W,
    home: PathBuf,
    scenario: FixtureScenario,
    connection_index: u64,
    state: Arc<AppServerFixtureState>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut resume_failed = false;
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
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        match method {
            "initialize" if scenario == FixtureScenario::MalformedInitialize => {
                write_fixture_message(
                    &mut writer,
                    json!({ "id": id, "result": { "invalid": true } }),
                )
                .await;
            }
            "initialize" => {
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "userAgent": "codex-cli/0.144.6",
                            "codexHome": home,
                            "platformFamily": "unix",
                            "platformOs": "linux"
                        }
                    }),
                )
                .await;
            }
            "account/read" => {
                let authentication_required = scenario == FixtureScenario::AuthenticationRequired;
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "account": if authentication_required {
                                Value::Null
                            } else {
                                json!({ "type": "chatgpt", "email": "redacted@example.test", "planType": "test" })
                            },
                            "requiresOpenaiAuth": true
                        }
                    }),
                )
                .await;
            }
            "model/list" => {
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "data": [{
                                "id": "gpt-5.4",
                                "model": "gpt-5.4",
                                "displayName": "GPT-5.4",
                                "description": "Fixture model",
                                "hidden": false,
                                "isDefault": true,
                                "defaultReasoningEffort": "medium",
                                "supportedReasoningEfforts": [{
                                    "reasoningEffort": "medium",
                                    "description": "Balanced"
                                }],
                                "inputModalities": ["text", "image"],
                                "serviceTiers": [],
                                "defaultServiceTier": null,
                                "supportsPersonality": false,
                                "upgrade": null
                            }],
                            "nextCursor": null
                        }
                    }),
                )
                .await;
            }
            "mcpServerStatus/list" => {
                let server = if scenario == FixtureScenario::Organizational {
                    json!({
                        "name": "ganbaru-chat",
                        "authStatus": "bearerToken",
                        "enabled": true,
                        "status": "ready"
                    })
                } else {
                    json!({
                        "name": "openaiDeveloperDocs",
                        "authStatus": "unsupported",
                        "enabled": true,
                        "status": "ready"
                    })
                };
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "data": [server],
                            "nextCursor": null
                        }
                    }),
                )
                .await;
            }
            "config/read" => {
                let config = if scenario == FixtureScenario::Organizational {
                    if connection_index == 0 {
                        json!({
                            "mcp_servers": {
                                "openaiDeveloperDocs": { "enabled": true }
                            }
                        })
                    } else {
                        json!({
                            "default_permissions": ORGANIZATIONAL_PERMISSION_PROFILE,
                            "allow_login_shell": false,
                            "web_search": "disabled",
                            "shell_environment_policy": {
                                "inherit": "core",
                                "ignore_default_excludes": false,
                                "experimental_use_profile": false,
                                "set": {}
                            },
                            "permissions": {
                                (ORGANIZATIONAL_PERMISSION_PROFILE): {
                                    "filesystem": {
                                        ":minimal": "read",
                                        ":workspace_roots": { ".": "write" }
                                    },
                                    "network": { "enabled": false }
                                }
                            },
                            "features": {
                                "apps": false,
                                "artifact": false,
                                "auth_elicitation": false,
                                "browser_use": false,
                                "browser_use_external": false,
                                "browser_use_full_cdp_access": false,
                                "code_mode": { "enabled": false },
                                "code_mode_host": false,
                                "computer_use": false,
                                "enable_mcp_apps": false,
                                "external_agent_memory_import": false,
                                "hooks": false,
                                "image_generation": false,
                                "in_app_browser": false,
                                "memories": false,
                                "multi_agent": false,
                                "multi_agent_v2": false,
                                "network_proxy": false,
                                "plugin_sharing": false,
                                "plugins": false,
                                "recommended_plugins": false,
                                "remote_plugin": false,
                                "request_permissions_tool": false,
                                "respect_system_proxy": false,
                                "shell_snapshot": false,
                                "skill_mcp_dependency_install": false,
                                "skill_search": false,
                                "standalone_web_search": false,
                                "use_agent_identity": false,
                                "workspace_dependencies": false
                            },
                            "mcp_servers": {
                                "openaiDeveloperDocs": { "enabled": false },
                                "ganbaru-chat": {
                                    "enabled": true,
                                    "required": true,
                                    "url": "http://127.0.0.1:41827/mcp",
                                    "bearer_token_env_var": "GANBARU_CHAT_MCP_TOKEN"
                                }
                            }
                        })
                    }
                } else {
                    json!({
                        "approval_policy": "on-request",
                        "approvals_reviewer": "user",
                        "sandbox_mode": "workspace-write",
                        "sandbox_workspace_write": {
                            "writable_roots": [],
                            "network_access": false,
                            "exclude_tmpdir_env_var": false,
                            "exclude_slash_tmp": false
                        }
                    })
                };
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": { "config": config }
                    }),
                )
                .await;
            }
            "permissionProfile/list" => {
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "data": [{
                                "id": ORGANIZATIONAL_PERMISSION_PROFILE,
                                "allowed": true
                            }],
                            "nextCursor": null
                        }
                    }),
                )
                .await;
            }
            "thread/resume" if scenario == FixtureScenario::ResumeMissing && !resume_failed => {
                resume_failed = true;
                write_fixture_message(
                    &mut writer,
                    json!({ "id": id, "error": { "code": -32602, "message": "Thread not found" } }),
                )
                .await;
            }
            "thread/start" | "thread/resume" => {
                let thread_id = message["params"]["threadId"]
                    .as_str()
                    .unwrap_or("fixture-thread-new");
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": {
                            "thread": { "id": thread_id },
                            "model": "gpt-5.4",
                            "approvalPolicy": message["params"]["approvalPolicy"].as_str().unwrap_or("on-request"),
                            "approvalsReviewer": message["params"]["approvalsReviewer"].as_str().unwrap_or("user"),
                            "activePermissionProfile": message["params"]["permissions"].as_str().map(|id| json!({
                                "id": id,
                                "extends": null
                            })),
                            "runtimeWorkspaceRoots": message["params"]
                                .get("runtimeWorkspaceRoots")
                                .cloned()
                                .unwrap_or_else(|| json!([])),
                            "sandbox": {
                                "type": match message["params"]["sandbox"].as_str() {
                                    Some("read-only") => "readOnly",
                                    Some("workspace-write") => "workspaceWrite",
                                    Some("danger-full-access") => "dangerFullAccess",
                                    _ => "workspaceWrite"
                                }
                            }
                        }
                    }),
                )
                .await;
                if method != "thread/resume" || scenario != FixtureScenario::ResumeWithoutStarted {
                    write_fixture_message(
                        &mut writer,
                        json!({
                            "method": "thread/started",
                            "params": { "thread": { "id": thread_id, "name": "Fixture provider title" } }
                        }),
                    )
                    .await;
                }
            }
            "config/mcpServer/reload" => {
                write_fixture_message(&mut writer, json!({ "id": id, "result": {} })).await;
            }
            "turn/start" if scenario == FixtureScenario::TurnNotInProgress => {
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": { "turn": { "id": "fixture-provider-turn", "status": "completed" } }
                    }),
                )
                .await;
            }
            "turn/start" => {
                write_fixture_message(
                    &mut writer,
                    json!({
                        "id": id,
                        "result": { "turn": { "id": "fixture-provider-turn", "status": "inProgress" } }
                    }),
                )
                .await;
                write_fixture_message(
                    &mut writer,
                    json!({
                        "method": "turn/started",
                        "params": {
                            "threadId": message["params"]["threadId"],
                            "turn": { "id": "fixture-provider-turn", "status": "inProgress" }
                        }
                    }),
                )
                .await;
                if scenario == FixtureScenario::CommandApproval {
                    write_fixture_message(
                        &mut writer,
                        json!({
                            "id": "fixture-approval",
                            "method": "item/commandExecution/requestApproval",
                            "params": {
                                "threadId": message["params"]["threadId"],
                                "turnId": "fixture-provider-turn",
                                "itemId": "fixture-command-item",
                                "command": "pnpm test",
                                "reason": "Run focused tests"
                            }
                        }),
                    )
                    .await;
                }
                if scenario == FixtureScenario::StructuredQuestion {
                    write_fixture_message(
                        &mut writer,
                        json!({
                            "id": "fixture-question",
                            "method": "item/tool/requestUserInput",
                            "params": {
                                "threadId": message["params"]["threadId"],
                                "turnId": "fixture-provider-turn",
                                "itemId": "fixture-question-item",
                                "questions": [{
                                    "id": "strategy",
                                    "header": "Strategy",
                                    "question": "Which strategy should be used?",
                                    "isOther": true,
                                    "isSecret": false,
                                    "options": [{ "label": "Focused", "description": "Small scope" }]
                                }]
                            }
                        }),
                    )
                    .await;
                }
                if scenario == FixtureScenario::Organizational {
                    write_fixture_message(
                        &mut writer,
                        json!({
                            "id": "fixture-organizational-escalation",
                            "method": "item/commandExecution/requestApproval",
                            "params": {
                                "threadId": message["params"]["threadId"],
                                "turnId": "fixture-provider-turn",
                                "itemId": "fixture-command-item",
                                "command": "curl https://example.test",
                                "additionalPermissions": {
                                    "network": { "enabled": true }
                                }
                            }
                        }),
                    )
                    .await;
                }
            }
            "turn/interrupt" => {
                write_fixture_message(&mut writer, json!({ "id": id, "result": {} })).await;
                write_fixture_message(
                    &mut writer,
                    json!({
                        "method": "turn/completed",
                        "params": {
                            "threadId": message["params"]["threadId"],
                            "turn": { "id": message["params"]["turnId"], "status": "interrupted", "items": [] }
                        }
                    }),
                )
                .await;
            }
            "thread/read" => {
                write_fixture_message(
                    &mut writer,
                    json!({ "id": id, "result": { "thread": { "turns": [] } } }),
                )
                .await;
            }
            _ => {
                write_fixture_message(&mut writer, json!({ "id": id, "result": {} })).await;
            }
        }
    }
    state.closed.store(true, Ordering::Release);
}

fn start_request(workspace: &Path) -> StartSessionRequest {
    serde_json::from_value(json!({
        "threadId": "chat-thread-fixture",
        "workspace": {
            "workingFolderId": "workspace-fixture",
            "canonicalPath": workspace,
            "repositoryKind": "none",
            "repositoryIdentity": null
        },
        "providerInstanceId": "codex-instance-1",
        "modes": { "safetyMode": "ask_for_approval", "interactionMode": "build" },
        "modelId": "gpt-5.4",
        "modelOptions": []
    }))
    .unwrap()
}

fn fixture_turn(session_id: &ProviderSessionId, interaction_mode: &str) -> SendTurnRequest {
    serde_json::from_value(json!({
        "command": { "clientCommandId": "fixture-send", "expectedThreadRevision": 2 },
        "sessionId": session_id,
        "turnId": "chat-turn-fixture",
        "prompt": "Implement the fixture change",
        "attachments": [],
        "mentions": [],
        "modelId": "gpt-5.4",
        "modelOptions": [{
            "key": "reasoning_effort",
            "value": { "kind": "choice", "value": "high" }
        }],
        "modes": { "safetyMode": "ask_for_approval", "interactionMode": interaction_mode },
        "developerInstructions": "Fixture instructions"
    }))
    .unwrap()
}

async fn wait_for_event(
    sink: &RecordingSink,
    predicate: impl Fn(&CanonicalRuntimeEvent) -> bool,
) -> CanonicalRuntimeEvent {
    for _ in 0..100 {
        if let Some(event) = sink.events().into_iter().find(&predicate) {
            return event;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("fixture event was not emitted");
}

async fn wait_for_fixture_message(
    fixture: &AppServerFixtureState,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    for _ in 0..100 {
        if let Some(message) = fixture.received().into_iter().find(&predicate) {
            return message;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("fixture message was not received");
}

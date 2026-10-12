//! OpenCode driver lifecycle tests over a deterministic HTTP server.

use super::{TestDirectory, configuration};
use crate::events::{CanonicalEvent, CanonicalRuntimeEvent};
use crate::models::*;
use crate::opencode::config::{OpenCodeProviderSettings, continuation_group};
use crate::opencode::driver::OpenCodeProviderDriver;
use crate::opencode::http_client::OpenCodeHttpClient;
use crate::{
    DriverCancellation, DriverFuture, DriverOperationContext, ProviderDriver, ProviderEventSink,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

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

struct OpenCodeServerFixture {
    origin: String,
    state: Arc<OpenCodeServerState>,
    task: tokio::task::JoinHandle<()>,
}

struct OpenCodeServerState {
    requests: Mutex<Vec<String>>,
    workspace: String,
    resume_directory: Option<String>,
    close_first_event: bool,
    event_connections: AtomicUsize,
    event_sender: broadcast::Sender<String>,
    stopping: AtomicBool,
}

impl OpenCodeServerFixture {
    async fn start(
        workspace: &std::path::Path,
        resume_directory: Option<String>,
        close_first_event: bool,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (event_sender, _) = broadcast::channel(32);
        let state = Arc::new(OpenCodeServerState {
            requests: Mutex::new(Vec::new()),
            workspace: workspace.to_string_lossy().into_owned(),
            resume_directory,
            close_first_event,
            event_connections: AtomicUsize::new(0),
            event_sender,
            stopping: AtomicBool::new(false),
        });
        let server_state = Arc::clone(&state);
        let task = tokio::spawn(async move {
            loop {
                let Ok((connection, _)) = listener.accept().await else {
                    return;
                };
                let connection_state = Arc::clone(&server_state);
                tokio::spawn(async move {
                    let _ = handle_connection(connection, connection_state).await;
                });
            }
        });
        Self {
            origin,
            state,
            task,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.state.requests.lock().unwrap().clone()
    }

    async fn wait_for_request(&self, fragment: &str) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if self
                    .requests()
                    .iter()
                    .any(|request| request.contains(fragment))
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    async fn shutdown(self) {
        self.state.stopping.store(true, Ordering::Release);
        self.task.abort();
        let _ = self.task.await;
    }
}

async fn handle_connection(
    mut connection: TcpStream,
    state: Arc<OpenCodeServerState>,
) -> std::io::Result<()> {
    let request = read_request(&mut connection).await?;
    state.requests.lock().unwrap().push(request.clone());
    let request_line = request.lines().next().unwrap_or_default();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or_default();
    let target = request_parts.next().unwrap_or_default();
    let path = target.split('?').next().unwrap_or(target);
    if method == "GET" && path == "/event" {
        let connection_index = state.event_connections.fetch_add(1, Ordering::AcqRel);
        connection
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: keep-alive\r\n\r\ndata: {\"type\":\"server.connected\",\"properties\":{}}\n\n",
            )
            .await?;
        connection.flush().await?;
        if state.close_first_event && connection_index == 0 {
            return Ok(());
        }
        let mut events = state.event_sender.subscribe();
        loop {
            if state.stopping.load(Ordering::Acquire) {
                return Ok(());
            }
            match tokio::time::timeout(Duration::from_millis(100), events.recv()).await {
                Ok(Ok(event)) => {
                    connection
                        .write_all(format!("data: {event}\n\n").as_bytes())
                        .await?;
                    connection.flush().await?;
                }
                Ok(Err(_)) => return Ok(()),
                Err(_) => {}
            }
        }
    }
    let response = route_response(method, path, &state);
    connection.write_all(response.as_bytes()).await?;
    connection.flush().await
}

fn route_response(method: &str, path: &str, state: &OpenCodeServerState) -> String {
    match (method, path) {
        ("GET", "/global/health") => json_response(json!({
            "healthy": true,
            "version": "1.14.19"
        })),
        ("GET", "/command") => json_response(json!([{
            "name": "release",
            "description": "Prepare a release",
            "template": "Prepare $ARGUMENTS"
        }])),
        ("POST", "/session") => session_response("ses_new", &state.workspace),
        ("GET", "/session/ses_resume") => session_response(
            "ses_resume",
            state
                .resume_directory
                .as_deref()
                .unwrap_or(&state.workspace),
        ),
        ("POST", "/session/ses_resume/fork") => session_response("ses_fork", &state.workspace),
        ("PATCH", "/session/ses_resume") => session_response("ses_resume", &state.workspace),
        ("PATCH", "/session/ses_fork") => session_response("ses_fork", &state.workspace),
        ("POST", "/session/ses_new/prompt_async")
        | ("POST", "/session/ses_resume/prompt_async")
        | ("POST", "/session/ses_fork/prompt_async") => empty_response(204, "No Content"),
        ("POST", "/session/ses_new/abort")
        | ("POST", "/session/ses_resume/abort")
        | ("POST", "/session/ses_fork/abort")
        | ("POST", "/permission/perm_one/reply")
        | ("POST", "/question/question_one/reply")
        | ("POST", "/question/question_one/reject") => json_response(json!(true)),
        ("POST", "/session/ses_new/revert") => session_response("ses_new", &state.workspace),
        ("GET", "/session/ses_new/message")
        | ("GET", "/session/ses_resume/message")
        | ("GET", "/session/ses_fork/message") => json_response(json!([{
            "info": {
                "id": "msg_one",
                "sessionID": path.split('/').nth(2).unwrap_or("ses_new"),
                "role": "assistant",
                "providerID": "anthropic",
                "modelID": "claude-sonnet"
            },
            "parts": [{
                "id": "part_one",
                "messageID": "msg_one",
                "sessionID": path.split('/').nth(2).unwrap_or("ses_new"),
                "type": "text",
                "text": "Recovered",
                "time": { "start": 1, "end": 2 }
            }]
        }])),
        _ => response(
            404,
            "Not Found",
            "application/json",
            b"{\"name\":\"NotFoundError\"}",
        ),
    }
}

fn session_response(id: &str, directory: &str) -> String {
    json_response(json!({
        "id": id,
        "directory": directory,
        "title": "Fixture session",
        "time": { "created": 1, "updated": 1 }
    }))
}

fn json_response(value: serde_json::Value) -> String {
    let body = serde_json::to_vec(&value).unwrap();
    response(200, "OK", "application/json", &body)
}

fn empty_response(status: u16, reason: &str) -> String {
    response(status, reason, "application/json", &[])
}

fn response(status: u16, reason: &str, content_type: &str, body: &[u8]) -> String {
    format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        String::from_utf8_lossy(body)
    )
}

async fn read_request(connection: &mut TcpStream) -> std::io::Result<String> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut expected_length = None;
    loop {
        let bytes_read = connection.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..bytes_read]);
        if request.len() > 5 * 1024 * 1024 {
            return Err(std::io::Error::other("fixture request exceeded bound"));
        }
        if expected_length.is_none() {
            expected_length = super::complete_request_length(&request);
        }
        if expected_length.is_some_and(|length| request.len() >= length) {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

#[test]
fn external_session_lifecycle_never_stops_the_external_server() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-driver");
        let server = OpenCodeServerFixture::start(workspace.path(), None, false).await;
        let mut driver =
            OpenCodeProviderDriver::new(external_configuration(&server.origin, true)).unwrap();
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink.clone(),
                &context("start"),
            )
            .await
            .unwrap();
        let commands = driver.prompt_catalog().unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].value, "/release");
        assert_eq!(
            commands[0].description.as_deref(),
            Some("Prepare a release")
        );
        assert_eq!(commands[0].argument_hint.as_deref(), Some("[arguments]"));
        assert_eq!(
            snapshot
                .provider_thread_id
                .as_ref()
                .map(ProviderThreadId::as_str),
            Some("ses_new")
        );
        assert!(
            sink.events()
                .iter()
                .any(|event| { matches!(&event.event, CanonicalEvent::SessionStarted(_)) })
        );
        let turn_id = ChatTurnId::new("turn-opencode").unwrap();
        driver
            .send_turn(
                SendTurnRequest {
                    command: command("send"),
                    session_id: snapshot.session_id.clone(),
                    turn_id: turn_id.clone(),
                    prompt: "Build the fixture".to_string(),
                    attachments: vec![],
                    mentions: vec![],
                    model_id: Some(ModelId::new("anthropic/claude-sonnet").unwrap()),
                    model_options: vec![
                        ModelOptionSelection {
                            key: "agent".to_string(),
                            value: ModelOptionValue::Choice("build".to_string()),
                        },
                        ModelOptionSelection {
                            key: "variant".to_string(),
                            value: ModelOptionValue::Choice("high".to_string()),
                        },
                    ],
                    modes: modes(),
                    developer_instructions: Some("Keep the change focused".to_string()),
                },
                &context("send"),
            )
            .await
            .unwrap();
        driver
            .steer_turn(
                SteerTurnRequest {
                    command: command("steer"),
                    session_id: snapshot.session_id.clone(),
                    turn_id: turn_id.clone(),
                    prompt: "Also update the focused test".to_string(),
                },
                &context("steer"),
            )
            .await
            .unwrap();
        exercise_interactions(&mut driver, &snapshot, &server, &sink).await;

        let history = driver
            .read_history(
                ReadHistoryRequest {
                    session_id: snapshot.session_id.clone(),
                    cursor: None,
                    limit: 50,
                },
                &context("history"),
            )
            .await
            .unwrap();
        assert_eq!(history.items.len(), 1);
        driver
            .rollback(
                RollbackRequest {
                    command: command("rollback"),
                    session_id: snapshot.session_id.clone(),
                    checkpoint_id: None,
                    provider_cursor: Some(VersionedJson {
                        schema_version: 1,
                        value: json!({ "messageId": "msg_one", "partId": "part_one" }),
                    }),
                    target_turn_id: None,
                },
                &context("rollback"),
            )
            .await
            .unwrap();
        driver
            .interrupt_turn(
                InterruptTurnRequest {
                    command: command("interrupt"),
                    session_id: snapshot.session_id.clone(),
                    turn_id,
                },
                &context("interrupt"),
            )
            .await
            .unwrap();
        driver
            .stop_session(
                StopSessionRequest {
                    session_id: snapshot.session_id.clone(),
                    force: false,
                },
                &context("stop"),
            )
            .await
            .unwrap();

        let health = OpenCodeHttpClient::new(&server.origin, workspace.path(), None)
            .unwrap()
            .health()
            .await
            .unwrap();
        assert_eq!(
            health.get("healthy").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert!(
            server
                .requests()
                .iter()
                .any(|request| request.starts_with("POST /session/ses_new/abort?"))
        );
        let requests = server.requests();
        assert!(requests.iter().any(|request| {
            request.starts_with("POST /session/ses_new/prompt_async?")
                && request.contains("\"agent\":\"build\"")
                && request.contains("\"variant\":\"high\"")
                && request.contains("\"system\":\"Keep the change focused\"")
        }));
        assert!(requests.iter().any(|request| {
            request.starts_with("POST /permission/perm_one/reply?")
                && request.contains("\"reply\":\"always\"")
        }));
        assert!(requests.iter().any(|request| {
            request.starts_with("POST /question/question_one/reply?")
                && request.contains("\"answers\":[[\"Second\",\"Custom answer\"]]")
        }));
        assert!(requests.iter().any(|request| {
            request.starts_with("POST /session/ses_new/revert?")
                && request.contains("\"messageID\":\"msg_one\"")
                && request.contains("\"partID\":\"part_one\"")
        }));
        server.shutdown().await;
    });
}

#[test]
fn external_workspace_requires_confirmation_before_any_request() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-confirmation");
        let server = OpenCodeServerFixture::start(workspace.path(), None, false).await;
        let mut driver =
            OpenCodeProviderDriver::new(external_configuration(&server.origin, false)).unwrap();
        let error = driver
            .start_session(
                start_request(workspace.path()),
                Arc::new(RecordingSink::default()),
                &context("start"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert!(server.requests().is_empty());
        server.shutdown().await;
    });
}

#[test]
fn resume_forks_changed_directory_reasserts_permissions_and_reconciles_reconnect() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-resume");
        let other_workspace = TestDirectory::new("external-other");
        let server = OpenCodeServerFixture::start(
            workspace.path(),
            Some(other_workspace.path().to_string_lossy().into_owned()),
            true,
        )
        .await;
        let configuration = external_configuration(&server.origin, true);
        let settings = OpenCodeProviderSettings::parse(&configuration).unwrap();
        let continuation_group_id = continuation_group(&settings, None).unwrap();
        let mut driver = OpenCodeProviderDriver::new(configuration).unwrap();
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .resume_session(
                ResumeSessionRequest {
                    thread_id: ChatThreadId::new("thread-opencode").unwrap(),
                    workspace: verified_workspace(workspace.path()),
                    provider_instance_id: ProviderInstanceId::new("opencode-instance").unwrap(),
                    provider_thread_id: ProviderThreadId::new("ses_resume").unwrap(),
                    continuation_group_id,
                    resume_cursor: crate::opencode::protocol::resume_cursor("ses_resume").unwrap(),
                    modes: modes(),
                },
                sink,
                &context("resume"),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .provider_thread_id
                .as_ref()
                .map(ProviderThreadId::as_str),
            Some("ses_fork")
        );
        server
            .wait_for_request("GET /session/ses_fork/message?")
            .await;
        server.wait_for_request("GET /event?").await;
        let requests = server.requests();
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("POST /session/ses_resume/fork?"))
        );
        assert!(requests.iter().any(|request| {
            request.starts_with("PATCH /session/ses_fork?")
                && request.contains("\"permission\"")
                && request.contains("\"question\"")
        }));
        assert!(server.state.event_connections.load(Ordering::Acquire) >= 2);

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
        server.shutdown().await;
    });
}

#[test]
fn resume_creates_a_fresh_session_only_after_confirmed_not_found() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-missing-resume");
        let server = OpenCodeServerFixture::start(workspace.path(), None, false).await;
        let configuration = external_configuration(&server.origin, true);
        let settings = OpenCodeProviderSettings::parse(&configuration).unwrap();
        let continuation_group_id = continuation_group(&settings, None).unwrap();
        let mut driver = OpenCodeProviderDriver::new(configuration).unwrap();
        let snapshot = driver
            .resume_session(
                ResumeSessionRequest {
                    thread_id: ChatThreadId::new("thread-opencode").unwrap(),
                    workspace: verified_workspace(workspace.path()),
                    provider_instance_id: ProviderInstanceId::new("opencode-instance").unwrap(),
                    provider_thread_id: ProviderThreadId::new("ses_missing").unwrap(),
                    continuation_group_id,
                    resume_cursor: crate::opencode::protocol::resume_cursor("ses_missing").unwrap(),
                    modes: modes(),
                },
                Arc::new(RecordingSink::default()),
                &context("resume-missing"),
            )
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .provider_thread_id
                .as_ref()
                .map(ProviderThreadId::as_str),
            Some("ses_new")
        );
        let requests = server.requests();
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("GET /session/ses_missing?"))
        );
        assert!(
            requests
                .iter()
                .any(|request| request.starts_with("POST /session?"))
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
        server.shutdown().await;
    });
}

fn external_configuration(origin: &str, access_confirmed: bool) -> ProviderInstanceConfig {
    configuration(json!({
        "mode": "external",
        "serverUrl": origin,
        "confirmExternalWorkspaceAccess": access_confirmed
    }))
}

fn start_request(workspace: &std::path::Path) -> StartSessionRequest {
    StartSessionRequest {
        thread_id: ChatThreadId::new("thread-opencode").unwrap(),
        workspace: verified_workspace(workspace),
        provider_instance_id: ProviderInstanceId::new("opencode-instance").unwrap(),
        modes: modes(),
        model_id: Some(ModelId::new("anthropic/claude-sonnet").unwrap()),
        model_options: vec![],
    }
}

fn verified_workspace(workspace: &std::path::Path) -> VerifiedWorkspaceContext {
    VerifiedWorkspaceContext {
        working_folder_id: ProjectWorkingFolderId::new("workspace-opencode").unwrap(),
        canonical_path: workspace.to_string_lossy().into_owned(),
        repository_kind: RepositoryKind::None,
        repository_identity: None,
    }
}

fn modes() -> TurnModeSnapshot {
    TurnModeSnapshot {
        safety_mode: SafetyMode::AskForApproval,
        interaction_mode: InteractionMode::Build,
    }
}

fn context(operation_id: &str) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + Duration::from_secs(5),
        cancellation: DriverCancellation::default(),
    }
}

fn command(id: &str) -> ChatCommandContext {
    ChatCommandContext {
        client_command_id: ChatCommandId::new(format!("command-{id}")).unwrap(),
        expected_thread_revision: None,
    }
}

async fn exercise_interactions(
    driver: &mut OpenCodeProviderDriver,
    snapshot: &ProviderSessionSnapshot,
    server: &OpenCodeServerFixture,
    sink: &Arc<RecordingSink>,
) {
    server
        .state
        .event_sender
        .send(
            json!({
                "type": "permission.asked",
                "properties": {
                    "id": "perm_one",
                    "sessionID": "ses_new",
                    "permission": "bash",
                    "patterns": ["pnpm test"],
                    "metadata": {}
                }
            })
            .to_string(),
        )
        .unwrap();
    wait_for_event(sink, |event| {
        matches!(&event.event, CanonicalEvent::RequestOpened(_))
    })
    .await;
    driver
        .resolve_approval(
            ResolveApprovalRequest {
                command: command("approval"),
                session_id: snapshot.session_id.clone(),
                request_id: ChatRequestId::new("approval-local").unwrap(),
                provider_request_id: ProviderRequestId::new("perm_one").unwrap(),
                decision: ApprovalDecision {
                    kind: ApprovalDecisionKind::AllowSession,
                    provider_option_id: Some("always".to_string()),
                    updated_tool_input: None,
                },
            },
            &context("approval"),
        )
        .await
        .unwrap();

    server
        .state
        .event_sender
        .send(
            json!({
                "type": "question.asked",
                "properties": {
                    "id": "question_one",
                    "sessionID": "ses_new",
                    "questions": [{
                        "header": "Choice",
                        "question": "Which option?",
                        "options": [
                            { "label": "First", "description": "One" },
                            { "label": "Second", "description": "Two" }
                        ],
                        "multiple": true,
                        "custom": true
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
    wait_for_event(sink, |event| {
        matches!(&event.event, CanonicalEvent::UserInputRequested(_))
    })
    .await;
    driver
        .resolve_user_input(
            ResolveUserInputRequest {
                command: command("question"),
                session_id: snapshot.session_id.clone(),
                request_id: ChatRequestId::new("question-local").unwrap(),
                provider_request_id: ProviderRequestId::new("question_one").unwrap(),
                answers: vec![UserInputAnswer {
                    question_id: "question-0".to_string(),
                    selected_option_ids: vec!["option-1".to_string()],
                    free_form_text: Some("Custom answer".to_string()),
                }],
            },
            &context("question"),
        )
        .await
        .unwrap();
}

async fn wait_for_event(sink: &RecordingSink, predicate: impl Fn(&CanonicalRuntimeEvent) -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if sink.events().iter().any(&predicate) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

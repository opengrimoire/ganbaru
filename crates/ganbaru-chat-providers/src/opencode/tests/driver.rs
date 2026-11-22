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
    health_version: &'static str,
}

impl OpenCodeServerFixture {
    async fn start(
        workspace: &std::path::Path,
        resume_directory: Option<String>,
        close_first_event: bool,
    ) -> Self {
        Self::start_reporting(workspace, resume_directory, close_first_event, "1.14.19").await
    }

    /// Starts a server whose health check reports `health_version`.
    async fn start_reporting(
        workspace: &std::path::Path,
        resume_directory: Option<String>,
        close_first_event: bool,
        health_version: &'static str,
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
            health_version,
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
    if method == "POST" && path.starts_with("/permission/") && path.ends_with("/reply") {
        return json_response(json!(true));
    }
    match (method, path) {
        ("GET", "/global/health") => json_response(json!({
            "healthy": true,
            "version": state.health_version
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
        ("PATCH", "/session/ses_new") => session_response("ses_new", &state.workspace),
        ("POST", "/session/ses_new/prompt_async")
        | ("POST", "/session/ses_resume/prompt_async")
        | ("POST", "/session/ses_fork/prompt_async") => empty_response(204, "No Content"),
        ("POST", "/session/ses_new/abort")
        | ("POST", "/session/ses_resume/abort")
        | ("POST", "/session/ses_fork/abort")
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
                && request.contains("\"reply\":\"once\"")
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
                    developer_instructions: None,
                    developer_instructions_delivered: false,
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
                    developer_instructions: None,
                    developer_instructions_delivered: false,
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
        developer_instructions: None,
    }
}

fn verified_workspace(workspace: &std::path::Path) -> VerifiedWorkspaceContext {
    VerifiedWorkspaceContext {
        working_folder_id: ProjectWorkingFolderId::new("workspace-opencode").unwrap(),
        canonical_path: workspace.to_string_lossy().into_owned(),
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        project_configuration: ProjectConfigurationAccess::Trusted,
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
    send_event(
        server,
        json!({
            "type": "permission.replied",
            "properties": { "sessionID": "ses_new", "requestID": "perm_one", "reply": "once" }
        }),
    );
    wait_for_event(sink, |event| resolved_request(event, "perm_one").is_some()).await;
    assert_eq!(
        sink.events()
            .iter()
            .find_map(|event| resolved_request(event, "perm_one"))
            .map(|decision| decision.kind),
        Some(ApprovalDecisionKind::AllowSession),
        "the once echo keeps the session decision"
    );
    send_permission_request(server, "perm_two", "bash", &["pnpm test"], None);
    wait_for_event(sink, |event| resolved_request(event, "perm_two").is_some()).await;
    server
        .wait_for_request("POST /permission/perm_two/reply?")
        .await;
    assert!(server.requests().iter().any(|request| {
        request.starts_with("POST /permission/perm_two/reply?")
            && request.contains("\"reply\":\"once\"")
    }));
    let events = sink.events();
    let decision = events
        .iter()
        .find_map(|event| resolved_request(event, "perm_two"))
        .unwrap();
    assert_eq!(decision.kind, ApprovalDecisionKind::AllowOnce);
    assert_eq!(
        decision.provider_option_id.as_deref(),
        Some(crate::opencode::normalizer::SESSION_RULE_OPTION_ID)
    );
    send_event(
        server,
        json!({
            "type": "permission.replied",
            "properties": { "sessionID": "ses_new", "requestID": "perm_two", "reply": "once" }
        }),
    );

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
                    attachment_ids: Vec::new(),
                }],
            },
            &context("question"),
        )
        .await
        .unwrap();
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| resolved_request(event, "perm_two").is_some())
            .count(),
        1,
        "the echo of an automatic answer adds no second resolution"
    );
}

fn send_event(server: &OpenCodeServerFixture, event: serde_json::Value) {
    server.state.event_sender.send(event.to_string()).unwrap();
}

fn send_permission_request(
    server: &OpenCodeServerFixture,
    id: &str,
    permission: &str,
    patterns: &[&str],
    always: Option<Vec<String>>,
) {
    send_event(
        server,
        json!({
            "type": "permission.asked",
            "properties": {
                "id": id,
                "sessionID": "ses_new",
                "permission": permission,
                "patterns": patterns,
                "always": always,
                "metadata": {}
            }
        }),
    );
}

fn opened_request(event: &CanonicalRuntimeEvent, id: &str) -> bool {
    matches!(&event.event, CanonicalEvent::RequestOpened(request) if request.request_id.as_str() == id)
}

fn resolved_request<'a>(
    event: &'a CanonicalRuntimeEvent,
    id: &str,
) -> Option<&'a ApprovalDecision> {
    match &event.event {
        CanonicalEvent::RequestResolved(resolved) if resolved.request_id.as_str() == id => {
            resolved.decision.as_ref()
        }
        _ => None,
    }
}

async fn allow_for_session(
    driver: &mut OpenCodeProviderDriver,
    snapshot: &ProviderSessionSnapshot,
    id: &str,
) {
    driver
        .resolve_approval(
            ResolveApprovalRequest {
                command: command(id),
                session_id: snapshot.session_id.clone(),
                request_id: ChatRequestId::new(format!("approval-{id}")).unwrap(),
                provider_request_id: ProviderRequestId::new(id).unwrap(),
                decision: ApprovalDecision {
                    kind: ApprovalDecisionKind::AllowSession,
                    provider_option_id: Some("always".to_string()),
                    updated_tool_input: None,
                },
            },
            &context(id),
        )
        .await
        .unwrap();
}

#[test]
fn session_approvals_are_capped_with_a_warning_and_end_with_the_session() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-session-rules");
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
        server.wait_for_request("GET /event?").await;
        let trimmed = |sink: &RecordingSink| {
            sink.events()
                .iter()
                .filter(|event| {
                    matches!(&event.event, CanonicalEvent::RuntimeWarning(warning)
                        if warning.code == "opencode_session_rules_trimmed")
                })
                .count()
        };
        for batch in 0..3 {
            let id = format!("perm_batch_{batch}");
            let always = (0..32)
                .map(|index| format!("dir{batch}-{index}/**"))
                .collect::<Vec<_>>();
            send_permission_request(&server, &id, "edit", &["other/file"], Some(always));
            wait_for_event(&sink, |event| opened_request(event, &id)).await;
            allow_for_session(&mut driver, &snapshot, &id).await;
            if batch == 1 {
                assert_eq!(trimmed(&sink), 0, "64 rules fit");
            }
        }
        assert_eq!(
            trimmed(&sink),
            1,
            "rules past 64 drop the oldest with one warning"
        );
        send_permission_request(&server, "perm_forgotten", "edit", &["dir0-0/file"], None);
        wait_for_event(&sink, |event| opened_request(event, "perm_forgotten")).await;
        send_permission_request(&server, "perm_kept", "edit", &["dir2-0/file"], None);
        wait_for_event(&sink, |event| {
            resolved_request(event, "perm_kept").is_some()
        })
        .await;
        assert!(
            !sink
                .events()
                .iter()
                .any(|event| resolved_request(event, "perm_forgotten").is_some()),
            "a dropped rule asks again"
        );

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
        let sink = Arc::new(RecordingSink::default());
        let event_connections = server.state.event_connections.load(Ordering::Acquire);
        driver
            .start_session(
                start_request(workspace.path()),
                sink.clone(),
                &context("restart"),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while server.state.event_connections.load(Ordering::Acquire) <= event_connections {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        send_permission_request(&server, "perm_after_stop", "edit", &["dir2-0/file"], None);
        wait_for_event(&sink, |event| opened_request(event, "perm_after_stop")).await;
        send_permission_request(&server, "perm_barrier", "bash", &["true"], None);
        wait_for_event(&sink, |event| opened_request(event, "perm_barrier")).await;
        assert!(
            !sink
                .events()
                .iter()
                .any(|event| resolved_request(event, "perm_after_stop").is_some()),
            "rules are dropped at session stop"
        );
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.starts_with("POST /permission/perm_after_stop/reply?"))
        );
        server.shutdown().await;
    });
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

#[test]
fn a_silent_stream_during_a_turn_reconnects_then_fails_unless_idle_or_waiting_for_the_user() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("external-silent-stream");
        let server = OpenCodeServerFixture::start(workspace.path(), None, false).await;
        let busy_stream_timeout = Duration::from_millis(150);
        let mut driver = OpenCodeProviderDriver::new(external_configuration(&server.origin, true))
            .unwrap()
            .with_busy_stream_timeout(busy_stream_timeout);
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink.clone(),
                &context("start"),
            )
            .await
            .unwrap();
        server.wait_for_request("GET /event?").await;
        tokio::time::sleep(busy_stream_timeout * 4).await;
        assert_eq!(
            server.state.event_connections.load(Ordering::Acquire),
            1,
            "an idle stream is kept"
        );

        driver
            .send_turn(
                SendTurnRequest {
                    command: command("send"),
                    session_id: snapshot.session_id.clone(),
                    turn_id: ChatTurnId::new("turn-silent").unwrap(),
                    prompt: "Wait for the provider".to_string(),
                    attachments: vec![],
                    mentions: vec![],
                    model_id: None,
                    model_options: vec![],
                    modes: modes(),
                    developer_instructions: None,
                },
                &context("send"),
            )
            .await
            .unwrap();
        send_permission_request(&server, "perm_waiting", "bash", &["pnpm test"], None);
        wait_for_event(&sink, |event| opened_request(event, "perm_waiting")).await;
        tokio::time::sleep(busy_stream_timeout * 4).await;
        assert_eq!(
            server.state.event_connections.load(Ordering::Acquire),
            1,
            "a turn waiting for the user may stay silent"
        );
        send_event(
            &server,
            json!({
                "type": "permission.replied",
                "properties": { "sessionID": "ses_new", "requestID": "perm_waiting", "reply": "once" }
            }),
        );
        wait_for_event(&sink, |event| {
            resolved_request(event, "perm_waiting").is_some()
        })
        .await;
        let silent = |event: &CanonicalRuntimeEvent| {
            matches!(&event.event, CanonicalEvent::RuntimeError(error)
                if error.code == "opencode_event_stream_silent")
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            while !sink.events().iter().any(silent) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(server.state.event_connections.load(Ordering::Acquire) > 1);
        assert!(
            server
                .requests()
                .iter()
                .any(|request| request.starts_with("GET /session/ses_new/message")),
            "each reconnect reconciles history"
        );
        assert_eq!(
            sink.events().iter().filter(|event| silent(event)).count(),
            1
        );
        assert!(
            sink.events().iter().any(|event| matches!(&event.event,
                CanonicalEvent::RuntimeError(error)
                    if error.code == "opencode_event_stream_silent" && !error.recoverable)),
            "a stream that failed for good fails the turn"
        );
        server.shutdown().await;
    });
}

fn organizational_configuration(
    origin: &str,
    scope: Option<ProviderOrganizationalScope>,
) -> ProviderInstanceConfig {
    let mut configuration = external_configuration(origin, true);
    configuration.internal_mcp = Some(super::organizational::internal_mcp(scope));
    configuration
}

fn turn_request(
    snapshot: &ProviderSessionSnapshot,
    turn: &str,
    prompt: &str,
    modes: TurnModeSnapshot,
) -> SendTurnRequest {
    SendTurnRequest {
        command: command(turn),
        session_id: snapshot.session_id.clone(),
        turn_id: ChatTurnId::new(turn).unwrap(),
        prompt: prompt.to_string(),
        attachments: vec![],
        mentions: vec![],
        model_id: None,
        model_options: vec![],
        modes,
        developer_instructions: None,
    }
}

const EDIT_SCOPE: ProviderOrganizationalScope = ProviderOrganizationalScope::Folder {
    capability: ProviderFolderCapability::Edit,
};

#[test]
fn organizational_session_sends_scope_rules_and_rejects_requests_outside_the_scope() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("organizational-driver");
        let server =
            OpenCodeServerFixture::start_reporting(workspace.path(), None, false, "1.19.5").await;
        let mut driver = OpenCodeProviderDriver::new(organizational_configuration(
            &server.origin,
            Some(EDIT_SCOPE),
        ))
        .unwrap();
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink.clone(),
                &context("start"),
            )
            .await
            .unwrap();
        let requests = server.requests();
        let health = requests
            .iter()
            .position(|request| request.starts_with("GET /global/health?"))
            .expect("the version is checked");
        let create = requests
            .iter()
            .position(|request| request.starts_with("POST /session?"))
            .unwrap();
        assert!(
            health < create,
            "the version is checked before the session exists"
        );
        assert!(
            requests[create]
                .contains(r#"{"permission":[{"permission":"*","pattern":"*","action":"deny"}"#)
        );
        assert!(!requests[create].contains(r#""permission":"bash""#));

        driver
            .send_turn(
                turn_request(&snapshot, "turn-command", "/release now", modes()),
                &context("send"),
            )
            .await
            .unwrap();
        server
            .wait_for_request("POST /session/ses_new/prompt_async?")
            .await;
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.starts_with("POST /session/ses_new/command")),
            "provider commands are sent as text"
        );
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.starts_with("PATCH /session/ses_new?")),
            "unchanged modes keep the session rules"
        );

        send_permission_request(&server, "perm_bash", "bash", &["touch marker"], None);
        server
            .wait_for_request("POST /permission/perm_bash/reply?")
            .await;
        assert!(server.requests().iter().any(|request| {
            request.starts_with("POST /permission/perm_bash/reply?")
                && request.contains(r#"{"reply":"reject"}"#)
        }));
        send_permission_request(
            &server,
            "perm_edit",
            "edit",
            &["src/lib.rs"],
            Some(vec!["*".to_string()]),
        );
        wait_for_event(&sink, |event| opened_request(event, "perm_edit")).await;
        assert!(
            !sink
                .events()
                .iter()
                .any(|event| opened_request(event, "perm_bash"))
        );
        let error = driver
            .resolve_approval(
                ResolveApprovalRequest {
                    command: command("session-grant"),
                    session_id: snapshot.session_id.clone(),
                    request_id: ChatRequestId::new("approval-edit").unwrap(),
                    provider_request_id: ProviderRequestId::new("perm_edit").unwrap(),
                    decision: ApprovalDecision {
                        kind: ApprovalDecisionKind::AllowSession,
                        provider_option_id: Some("always".to_string()),
                        updated_tool_input: None,
                    },
                },
                &context("session-grant"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.starts_with("POST /permission/perm_edit/reply?")),
            "a refused session grant sends no reply"
        );
        server.shutdown().await;
    });
}

#[test]
fn organizational_turns_resend_rules_when_modes_change_and_refuse_unenforceable_modes() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("organizational-modes");
        let server =
            OpenCodeServerFixture::start_reporting(workspace.path(), None, false, "1.19.5").await;
        let mut driver = OpenCodeProviderDriver::new(organizational_configuration(
            &server.origin,
            Some(ProviderOrganizationalScope::Folder {
                capability: ProviderFolderCapability::Execute,
            }),
        ))
        .unwrap();
        let sink = Arc::new(RecordingSink::default());
        let snapshot = driver
            .start_session(
                start_request(workspace.path()),
                sink.clone(),
                &context("start"),
            )
            .await
            .unwrap();
        let error = driver
            .send_turn(
                turn_request(
                    &snapshot,
                    "turn-approve",
                    "Run the tests",
                    TurnModeSnapshot {
                        safety_mode: SafetyMode::ApproveForMe,
                        interaction_mode: InteractionMode::Build,
                    },
                ),
                &context("approve"),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.contains("/prompt_async")),
            "a refused turn reaches no prompt"
        );
        driver
            .send_turn(
                turn_request(
                    &snapshot,
                    "turn-plan",
                    "Plan the change",
                    TurnModeSnapshot {
                        safety_mode: SafetyMode::AskForApproval,
                        interaction_mode: InteractionMode::Plan,
                    },
                ),
                &context("plan"),
            )
            .await
            .unwrap();
        let requests = server.requests();
        let patch = requests
            .iter()
            .position(|request| request.starts_with("PATCH /session/ses_new?"))
            .expect("Plan rules are sent");
        let prompt = requests
            .iter()
            .position(|request| request.starts_with("POST /session/ses_new/prompt_async?"))
            .unwrap();
        assert!(patch < prompt, "rules change before the prompt");
        assert!(requests[patch].contains(r#""permission":"bash","pattern":"*","action":"ask""#));
        assert!(!requests[patch].contains(r#""permission":"edit","pattern":"*","action":"ask""#));
        server.shutdown().await;
    });
}

#[test]
fn organizational_sessions_fail_closed_before_creating_a_session() {
    crate::test_block_on(async {
        let workspace = TestDirectory::new("organizational-refusals");
        let server = OpenCodeServerFixture::start(workspace.path(), None, false).await;
        let cases: [(
            Option<ProviderOrganizationalScope>,
            TurnModeSnapshot,
            ChatErrorCode,
        ); 3] = [
            (None, modes(), ChatErrorCode::Permission),
            (Some(EDIT_SCOPE), modes(), ChatErrorCode::UnsupportedVersion),
            (
                Some(ProviderOrganizationalScope::Scratch),
                TurnModeSnapshot {
                    safety_mode: SafetyMode::FullAccess,
                    interaction_mode: InteractionMode::Build,
                },
                ChatErrorCode::Permission,
            ),
        ];
        for (scope, start_modes, code) in cases {
            let mut driver =
                OpenCodeProviderDriver::new(organizational_configuration(&server.origin, scope))
                    .unwrap();
            let mut request = start_request(workspace.path());
            request.modes = start_modes;
            let error = driver
                .start_session(
                    request,
                    Arc::new(RecordingSink::default()),
                    &context("start"),
                )
                .await
                .unwrap_err();
            assert_eq!(error.code, code, "{scope:?}");
        }
        assert!(
            !server
                .requests()
                .iter()
                .any(|request| request.starts_with("POST /session?")),
            "no refused launch creates a session"
        );
        server.shutdown().await;
    });
}

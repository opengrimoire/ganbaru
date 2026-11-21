use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST_HEADER_READ_TIMEOUT: Duration = Duration::from_millis(200);
const TEST_WAIT: Duration = Duration::from_secs(5);

async fn test_endpoint(
    router: Router,
    max_connections: usize,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test endpoint");
    let address = listener.local_addr().expect("test endpoint address");
    let (shutdown, shutdown_receiver) = oneshot::channel();
    let server = tokio::spawn(serve_endpoint(
        listener,
        router,
        EndpointConnectionLimits {
            header_read_timeout: TEST_HEADER_READ_TIMEOUT,
            max_connections,
        },
        shutdown_receiver,
    ));
    (address, shutdown, server)
}

fn root_router() -> Router {
    Router::new().route("/", axum::routing::get(|| async { "ok" }))
}

#[derive(Clone)]
struct EmptyHandler;

impl ServerHandler for EmptyHandler {}

#[tokio::test]
async fn endpoint_refuses_request_bodies_over_the_declared_limit() {
    let token = generate_bearer_token().expect("token");
    let router = endpoint_router(
        EmptyHandler,
        Arc::new(RwLock::new(bearer_token_digest(&token))),
    );
    let (address, _shutdown, _server) = test_endpoint(router, MAX_MCP_CONNECTIONS).await;
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    let body = vec![b' '; MAX_MCP_REQUEST_BYTES + 1];
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    client.write_all(head.as_bytes()).await.unwrap();
    // The server may answer and close before reading the whole body.
    let _ = client.write_all(&body).await;
    let response = read_until_closed(&mut client).await;
    assert!(
        response.starts_with("HTTP/1.1 413"),
        "unexpected response: {}",
        response.lines().next().unwrap_or_default()
    );
}

async fn read_until_closed(stream: &mut tokio::net::TcpStream) -> String {
    let mut response = Vec::new();
    tokio::time::timeout(TEST_WAIT, stream.read_to_end(&mut response))
        .await
        .expect("connection closes in time")
        .ok();
    String::from_utf8_lossy(&response).into_owned()
}

async fn request_root(stream: &mut tokio::net::TcpStream) -> String {
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .await
        .expect("send request");
    read_until_closed(stream).await
}

#[tokio::test]
async fn endpoint_closes_connections_that_send_no_headers_and_still_serves_requests() {
    let (address, shutdown, server) = test_endpoint(root_router(), MAX_MCP_CONNECTIONS).await;
    let mut idle = tokio::net::TcpStream::connect(address).await.unwrap();
    let mut partial = tokio::net::TcpStream::connect(address).await.unwrap();
    partial.write_all(b"GET / HTTP/1.1\r\nHost:").await.unwrap();

    let started = tokio::time::Instant::now();
    assert!(!read_until_closed(&mut idle).await.contains("200 OK"));
    assert!(!read_until_closed(&mut partial).await.contains("200 OK"));
    assert!(started.elapsed() < TEST_WAIT);

    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    assert!(request_root(&mut client).await.contains("200 OK"));

    drop(shutdown);
    tokio::time::timeout(TEST_WAIT, server)
        .await
        .expect("endpoint stops after shutdown")
        .unwrap();
}

#[tokio::test]
async fn endpoint_holds_extra_connections_until_an_open_one_closes() {
    let (address, _shutdown, _server) = test_endpoint(root_router(), 1).await;
    let _idle = tokio::net::TcpStream::connect(address).await.unwrap();
    tokio::time::sleep(TEST_HEADER_READ_TIMEOUT / 4).await;

    let started = tokio::time::Instant::now();
    let mut queued = tokio::net::TcpStream::connect(address).await.unwrap();
    assert!(request_root(&mut queued).await.contains("200 OK"));
    assert!(started.elapsed() >= TEST_HEADER_READ_TIMEOUT / 2);
}

#[test]
fn endpoint_offers_only_revisions_that_open_with_initialize() {
    let versions = session_protocol_versions();
    assert!(versions.contains(&ProtocolVersion::LATEST_WITH_INITIALIZE));
    assert!(versions.iter().all(ProtocolVersion::has_initialize));
}

#[test]
fn endpoint_refuses_new_sessions_at_the_cap_but_serves_existing_ones() {
    let new_session = Request::builder().body(Body::empty()).unwrap();
    let existing_session = Request::builder()
        .header(MCP_SESSION_ID_HEADER, "session")
        .body(Body::empty())
        .unwrap();
    assert!(admits_session_request(&new_session, 0, MAX_MCP_SESSIONS));
    assert!(admits_session_request(
        &new_session,
        MAX_MCP_SESSIONS - 1,
        MAX_MCP_SESSIONS
    ));
    assert!(!admits_session_request(
        &new_session,
        MAX_MCP_SESSIONS,
        MAX_MCP_SESSIONS
    ));
    assert!(admits_session_request(
        &existing_session,
        MAX_MCP_SESSIONS,
        MAX_MCP_SESSIONS
    ));
}

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn pending_work(dropped: &Arc<AtomicBool>) -> impl Future<Output = ChatResult<()>> {
    let flag = DropFlag(Arc::clone(dropped));
    async move {
        let _flag = flag;
        std::future::pending::<ChatResult<()>>().await
    }
}

#[tokio::test]
async fn tool_work_stops_when_its_request_is_cancelled_or_times_out() {
    let dropped = Arc::new(AtomicBool::new(false));
    let cancelled = bounded_work(std::future::ready(()), TEST_WAIT, pending_work(&dropped))
        .await
        .expect_err("cancelled work fails");
    assert_eq!(cancelled.code, ChatErrorCode::Cancelled);
    assert!(dropped.load(Ordering::SeqCst));

    let dropped = Arc::new(AtomicBool::new(false));
    let timed_out = bounded_work(
        std::future::pending(),
        TEST_HEADER_READ_TIMEOUT,
        pending_work(&dropped),
    )
    .await
    .expect_err("slow work fails");
    assert_eq!(timed_out.code, ChatErrorCode::Timeout);
    assert!(dropped.load(Ordering::SeqCst));

    let finished = bounded_work(std::future::pending(), TEST_WAIT, async { Ok(7) }).await;
    assert_eq!(finished.expect("finished work"), 7);
}

#[test]
fn endpoint_descriptors_keep_bearer_tokens_out_of_debug_output() {
    let token = generate_bearer_token().expect("token");
    let endpoint = InternalMcpEndpoint {
        url: "http://127.0.0.1:41000/mcp".to_string(),
        bearer_token: token.clone(),
    };
    let config = ganbaru_chat_contracts::models::ProviderInternalMcpConfig {
        name: "ganbaru".to_string(),
        url: endpoint.url.clone(),
        bearer_token: token.clone(),
        organizational_authority: true,
        pre_approved_tools: Vec::new(),
        organizational_scope: Some(
            ganbaru_chat_contracts::models::ProviderOrganizationalScope::Conversation,
        ),
    };
    for output in [format!("{endpoint:?}"), format!("{config:?}")] {
        assert!(!output.contains(&token), "{output}");
        assert!(
            output.contains("127.0.0.1:41000") && output.contains("[REDACTED]"),
            "{output}"
        );
    }
}

/// The status line an authenticated `/mcp` request with `token` gets.
async fn mcp_status(address: std::net::SocketAddr, token: &str) -> String {
    let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
    let head = format!(
        "POST /mcp HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    );
    client.write_all(head.as_bytes()).await.unwrap();
    let response = read_until_closed(&mut client).await;
    response.lines().next().unwrap_or_default().to_string()
}

#[tokio::test]
async fn rotated_tokens_replace_the_previous_one_and_only_exact_tokens_pass() {
    let first = generate_bearer_token().expect("token");
    let token_digest = Arc::new(RwLock::new(bearer_token_digest(&first)));
    let router = endpoint_router(EmptyHandler, Arc::clone(&token_digest));
    let (address, _shutdown, _server) = test_endpoint(router, MAX_MCP_CONNECTIONS).await;
    let unauthorized = "HTTP/1.1 401 Unauthorized";
    assert_ne!(mcp_status(address, &first).await, unauthorized);
    for wrong in [&first[..63], &format!("{first}0"), ""] {
        assert_eq!(mcp_status(address, wrong).await, unauthorized, "{wrong}");
    }

    let second = replace_bearer_token(&token_digest).await.expect("rotate");
    assert_ne!(second, first);
    assert_eq!(mcp_status(address, &first).await, unauthorized);
    assert_ne!(mcp_status(address, &second).await, unauthorized);
}

/// The registry keeps a digest whose bytes do not contain the token in any encoding.
#[tokio::test]
async fn endpoints_keep_only_a_digest_of_their_token() {
    let token_digest = RwLock::new([0; 32]);
    let token = replace_bearer_token(&token_digest).await.expect("token");
    let stored = *token_digest.read().await;
    assert_eq!(stored, bearer_token_digest(&token));
    let stored_hex: String = stored.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_ne!(stored_hex, token);
    assert!(!token.contains(&stored_hex[..16]));
    let created = EndpointIssue::Created(InternalMcpEndpoint {
        url: "http://127.0.0.1:41000/mcp".to_string(),
        bearer_token: token.clone(),
    });
    assert!(!format!("{created:?}").contains(&token));
}

#[test]
fn bearer_tokens_are_bounded_unique_and_compared_without_prefix_matches() {
    let first = generate_bearer_token().expect("first token");
    let second = generate_bearer_token().expect("second token");
    assert_eq!(first.len(), 64);
    assert_eq!(second.len(), 64);
    assert_ne!(first, second);
    assert!(constant_time_equal(first.as_bytes(), first.as_bytes()));
    assert!(!constant_time_equal(first.as_bytes(), second.as_bytes()));
    assert!(!constant_time_equal(
        first.as_bytes(),
        &first.as_bytes()[..63]
    ));
}

#[test]
fn opaque_handles_use_validated_namespaces_and_256_bits_of_randomness() {
    let handle = generate_opaque_handle("channel-source").expect("opaque handle");
    assert!(handle.starts_with("channel-source:"));
    assert_eq!(handle.len(), "channel-source:".len() + 64);
    assert!(generate_opaque_handle("Channel Source").is_err());
}

#[test]
fn endpoint_accepts_only_loopback_hosts_and_origins() {
    let allowed = Request::builder()
        .header(header::HOST, "127.0.0.1:43123")
        .body(Body::empty())
        .unwrap();
    assert!(request_targets_loopback(&allowed));

    let remote_host = Request::builder()
        .header(header::HOST, "192.0.2.10:43123")
        .body(Body::empty())
        .unwrap();
    assert!(!request_targets_loopback(&remote_host));

    let remote_origin = Request::builder()
        .header(header::HOST, "localhost:43123")
        .header(header::ORIGIN, "https://example.com")
        .body(Body::empty())
        .unwrap();
    assert!(!request_targets_loopback(&remote_origin));
}

#[tokio::test]
async fn pending_organizational_endpoint_denies_tools_until_scope_activation() {
    let state = EndpointState::new(true);
    assert!(state.active_scope().await.is_err());
}

#[tokio::test]
async fn direct_endpoint_admits_tools_only_while_its_turn_runs() {
    let state = EndpointState::new(false);
    let first = ChatTurnId::new("turn-1").unwrap();
    let second = ChatTurnId::new("turn-2").unwrap();
    assert!(state.active_scope().await.unwrap().is_none());
    assert!(state.require_direct_turn().await.is_err());

    state.begin_direct_turn(&first).await;
    assert!(state.require_direct_turn().await.is_ok());
    state.end_direct_turn(Some(&first)).await;
    let error = state.require_direct_turn().await.unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Permission);
    assert!(
        state.active_scope().await.unwrap().is_none(),
        "listings stay available between turns"
    );

    state.begin_direct_turn(&second).await;
    state.end_direct_turn(Some(&first)).await;
    assert!(
        state.require_direct_turn().await.is_ok(),
        "a late settlement of an earlier turn keeps the running turn open"
    );
    state.end_direct_turn(None).await;
    assert!(
        state.require_direct_turn().await.is_err(),
        "a provider exit closes any running turn"
    );
}

#[tokio::test]
async fn organizational_endpoints_ignore_direct_turn_transitions() {
    let state = EndpointState::new(true);
    state
        .begin_direct_turn(&ChatTurnId::new("turn-1").unwrap())
        .await;
    assert!(state.require_direct_turn().await.is_err());
    assert!(matches!(
        &*state.access.read().await,
        EndpointAccessState::OrganizationalPending
    ));
}

#[tokio::test]
async fn revoked_organizational_endpoint_never_falls_back_to_direct_access() {
    let state = EndpointState::default();
    *state.access.write().await =
        EndpointAccessState::OrganizationalActive(Box::new(InternalMcpRunScope {
            run_id: ChatAgentRunId::new("run-1").unwrap(),
            provider_turn_id: ChatTurnId::new("turn-1").unwrap(),
            assignment_id: ChatWorkAssignmentId::new("assignment-1").unwrap(),
            authorization_revision_id: ChatAuthorizationRevisionId::new("authorization-1").unwrap(),
            destination_conversation_id: ChatConversationId::new("conversation-1").unwrap(),
            authorization_scope_digest: "a".repeat(64),
            execution_environment_id: Some("environment-1".to_string()),
            scratch_generation_id: None,
            runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
            channel_sources: Vec::new(),
            folder_sources: Vec::new(),
            delegation_depth: 0,
            delegation_target_available: false,
        }));
    assert!(state.active_scope().await.unwrap().is_some());
    *state.access.write().await = EndpointAccessState::OrganizationalRevoked;
    assert!(state.active_scope().await.is_err());
}

fn run_scope(run: &str) -> InternalMcpRunScope {
    InternalMcpRunScope {
        run_id: ChatAgentRunId::new(run).unwrap(),
        provider_turn_id: ChatTurnId::new("turn-1").unwrap(),
        assignment_id: ChatWorkAssignmentId::new("assignment-1").unwrap(),
        authorization_revision_id: ChatAuthorizationRevisionId::new("authorization-1").unwrap(),
        destination_conversation_id: ChatConversationId::new("conversation-1").unwrap(),
        authorization_scope_digest: "a".repeat(64),
        execution_environment_id: Some("environment-1".to_string()),
        scratch_generation_id: None,
        runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
        channel_sources: Vec::new(),
        folder_sources: Vec::new(),
        delegation_depth: 0,
        delegation_target_available: false,
    }
}

/// A short name for `state`, naming the turn or run it holds.
fn access_label(state: &EndpointAccessState) -> String {
    match state {
        EndpointAccessState::DirectIdle => "direct idle".to_string(),
        EndpointAccessState::DirectActive(turn) => format!("direct {}", turn.as_str()),
        EndpointAccessState::OrganizationalPending => "pending".to_string(),
        EndpointAccessState::OrganizationalActive(scope) => {
            format!("active {}", scope.run_id.as_str())
        }
        EndpointAccessState::OrganizationalRevoked => "revoked".to_string(),
    }
}

#[test]
fn endpoint_access_moves_follow_one_table_that_never_widens_access() {
    let turn = |id: &str| ChatTurnId::new(id).unwrap();
    let states = [
        EndpointAccessState::DirectIdle,
        EndpointAccessState::DirectActive(turn("turn-1")),
        EndpointAccessState::OrganizationalPending,
        EndpointAccessState::OrganizationalActive(Box::new(run_scope("run-1"))),
        EndpointAccessState::OrganizationalRevoked,
    ];
    let moves = [
        (
            "begin turn-2",
            EndpointAccessMove::BeginDirectTurn(turn("turn-2")),
        ),
        (
            "end turn-1",
            EndpointAccessMove::EndDirectTurn(Some(turn("turn-1"))),
        ),
        (
            "end turn-2",
            EndpointAccessMove::EndDirectTurn(Some(turn("turn-2"))),
        ),
        ("end any", EndpointAccessMove::EndDirectTurn(None)),
        (
            "activate run-2",
            EndpointAccessMove::ActivateRun(Box::new(run_scope("run-2"))),
        ),
        ("revoke", EndpointAccessMove::Revoke),
    ];
    // Every pair not listed leaves the state unchanged.
    let applied = [
        ("direct idle", "begin turn-2", "direct turn-2"),
        ("direct idle", "revoke", "revoked"),
        ("direct turn-1", "begin turn-2", "direct turn-2"),
        ("direct turn-1", "end turn-1", "direct idle"),
        ("direct turn-1", "end any", "direct idle"),
        ("direct turn-1", "revoke", "revoked"),
        ("pending", "activate run-2", "active run-2"),
        ("pending", "revoke", "revoked"),
        ("active run-1", "revoke", "revoked"),
        ("revoked", "revoke", "revoked"),
    ];
    for state in &states {
        for (name, change) in &moves {
            let from = access_label(state);
            let expected = applied
                .iter()
                .find(|(source, applied_move, _)| *source == from && applied_move == name)
                .map(|(_, _, target)| target.to_string());
            let after = state.after(change.clone()).map(|next| access_label(&next));
            assert_eq!(after, expected, "{from} then {name}");

            let mut applied_state = state.clone();
            assert_eq!(applied_state.apply(change.clone()), expected.is_some());
            assert_eq!(
                access_label(&applied_state),
                expected.unwrap_or(from.clone()),
                "{from} then {name}"
            );
        }
    }
}

#[tokio::test]
async fn a_run_scope_reaches_only_a_pending_organizational_endpoint() {
    let direct = EndpointState::new(false);
    let error = direct.activate_run(run_scope("run-1")).await.unwrap_err();
    assert_eq!(error.code, ChatErrorCode::InvalidStateTransition);
    assert!(direct.active_scope().await.unwrap().is_none());

    let organizational = EndpointState::new(true);
    organizational
        .activate_run(run_scope("run-1"))
        .await
        .unwrap();
    assert!(
        organizational
            .activate_run(run_scope("run-2"))
            .await
            .is_err()
    );
    organizational.revoke().await;
    assert!(
        organizational
            .activate_run(run_scope("run-3"))
            .await
            .is_err()
    );
    assert!(organizational.active_scope().await.is_err());
    assert!(organizational.require_direct_turn().await.is_err());
}

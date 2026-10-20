use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use base64::{Engine as _, engine::general_purpose};
use ganbaru_chat::repository::resources::{self, ChatResourceKind};
use ganbaru_chat_contracts::models::{
    ChatAgentRunId, ChatAuthorizationRevisionId, ChatConversationId, ChatError, ChatErrorCode,
    ChatFolderCapability, ChatResult, ChatRuntimeApprovalPolicy, ChatThreadId, ChatTurnId,
    ChatWorkAssignmentId, ProjectWorkingFolderId,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerInfo, Tool,
    ToolAnnotations,
};
use rmcp::service::{NotificationContext, RequestContext, RoleServer};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, ServerHandler};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::Manager;
use tokio::sync::{Mutex, Notify, RwLock, Semaphore, oneshot};

const MAX_MCP_REQUEST_BYTES: usize = 1024 * 1024;
const MAX_MCP_CONCURRENCY: usize = 8;
const MCP_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Preview tool work stops at this deadline, before the HTTP request deadline, so the model
/// gets a tool error instead of a gateway timeout and no work outlives its request.
const MCP_PREVIEW_WORK_TIMEOUT: Duration = Duration::from_secs(25);
/// How long a connection may take to send its request headers, including idle keep-alive time.
const MCP_HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// Open connections per endpoint. A provider uses a few; the rest wait in the accept backlog.
const MAX_MCP_CONNECTIONS: usize = 32;
/// Open MCP sessions per endpoint, which leaves room for provider reconnects within rmcp's
/// session keep-alive.
const MAX_MCP_SESSIONS: usize = 16;
/// Pause after a failed accept, such as when the process is out of file descriptors.
const MCP_ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const MCP_INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(15);
const RESOURCE_URI_PREFIX: &str = "ganbaru://chat/resource/";
/// How long a preview tool waits for the turn being dispatched to report its network policy.
const PREVIEW_NETWORK_SETTLE_TIMEOUT: Duration = Duration::from_secs(10);

mod preview_audit;
mod preview_network;

use preview_network::{PreviewNetworkDecision, PreviewNetworkGate};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InternalMcpChannelSource {
    pub source_handle: String,
    pub message_reference_id: String,
    pub conversation_id: String,
    pub label_snapshot: String,
    pub lower_ordinal: u64,
    pub high_ordinal: u64,
    pub source_revision_cutoff_id: String,
    pub destination_audience_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InternalMcpFolderSource {
    pub root_handle: String,
    pub working_folder_id: ProjectWorkingFolderId,
    pub capability: ChatFolderCapability,
    pub is_execution_target: bool,
    pub runtime_approval_policy: ChatRuntimeApprovalPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InternalMcpRunScope {
    pub run_id: ChatAgentRunId,
    pub provider_turn_id: ChatTurnId,
    pub assignment_id: ChatWorkAssignmentId,
    pub authorization_revision_id: ChatAuthorizationRevisionId,
    pub destination_conversation_id: ChatConversationId,
    pub authorization_scope_digest: String,
    pub execution_environment_id: Option<String>,
    pub scratch_generation_id: Option<String>,
    pub runtime_approval_policy: ChatRuntimeApprovalPolicy,
    pub channel_sources: Vec<InternalMcpChannelSource>,
    pub folder_sources: Vec<InternalMcpFolderSource>,
}

#[derive(Default)]
struct EndpointState {
    initialized: AtomicBool,
    initialized_notify: Notify,
    access: RwLock<EndpointAccessState>,
    tools: super::internal_mcp_tools::InternalMcpToolRuntime,
    preview_network: Mutex<PreviewNetworkGate>,
    preview_network_settled: Notify,
}

#[derive(Clone, Debug, Default)]
enum EndpointAccessState {
    /// Direct session between turns: tools and resource reads are refused, listings stay open.
    #[default]
    DirectIdle,
    /// Direct session while the dispatched turn runs.
    DirectActive(ChatTurnId),
    OrganizationalPending,
    OrganizationalActive(Box<InternalMcpRunScope>),
    OrganizationalRevoked,
}

impl EndpointState {
    fn new(organizational_pending: bool) -> Self {
        Self {
            initialized: AtomicBool::new(false),
            initialized_notify: Notify::new(),
            access: RwLock::new(if organizational_pending {
                EndpointAccessState::OrganizationalPending
            } else {
                EndpointAccessState::DirectIdle
            }),
            tools: super::internal_mcp_tools::InternalMcpToolRuntime::default(),
            preview_network: Mutex::new(PreviewNetworkGate::default()),
            preview_network_settled: Notify::new(),
        }
    }

    /// Waits briefly for a turn in dispatch to report its network policy, then decides.
    async fn preview_network_decision(&self) -> PreviewNetworkDecision {
        let deadline = tokio::time::Instant::now() + PREVIEW_NETWORK_SETTLE_TIMEOUT;
        loop {
            let settled = self.preview_network_settled.notified();
            let decision = self.preview_network.lock().await.decision();
            if decision != PreviewNetworkDecision::AwaitingTurn
                || tokio::time::timeout_at(deadline, settled).await.is_err()
            {
                return decision;
            }
        }
    }

    fn mark_initialized(&self) {
        self.initialized.store(true, Ordering::Release);
        self.initialized_notify.notify_waiters();
    }

    async fn wait_until_initialized(&self) -> ChatResult<()> {
        let initialized = self.initialized_notify.notified();
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        tokio::time::timeout(MCP_INITIALIZATION_TIMEOUT, initialized)
            .await
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::Timeout,
                    "The provider did not authenticate its internal host tools in time",
                    true,
                )
            })?;
        if self.initialized.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(ChatError::new(
                ChatErrorCode::TransportUnavailable,
                "The provider internal host tools are unavailable",
                true,
            ))
        }
    }

    /// Opens direct-session tools for `turn_id`; organizational endpoints are unchanged.
    async fn begin_direct_turn(&self, turn_id: &ChatTurnId) {
        let mut access = self.access.write().await;
        if matches!(
            *access,
            EndpointAccessState::DirectIdle | EndpointAccessState::DirectActive(_)
        ) {
            *access = EndpointAccessState::DirectActive(turn_id.clone());
        }
    }

    /// Closes direct-session tools when `turn_id` settles, or for any turn when it is `None`.
    async fn end_direct_turn(&self, turn_id: Option<&ChatTurnId>) {
        let mut access = self.access.write().await;
        if let EndpointAccessState::DirectActive(active) = &*access
            && turn_id.is_none_or(|settled| settled == active)
        {
            *access = EndpointAccessState::DirectIdle;
        }
    }

    /// Admits a direct-session tool call or resource read only while a turn runs, and names
    /// that turn.
    async fn require_direct_turn(&self) -> ChatResult<ChatTurnId> {
        match &*self.access.read().await {
            EndpointAccessState::DirectActive(turn_id) => Ok(turn_id.clone()),
            EndpointAccessState::DirectIdle => Err(ChatError::new(
                ChatErrorCode::Permission,
                "Ganbaru host tools are available only while a Chat turn is running",
                false,
            )),
            EndpointAccessState::OrganizationalPending
            | EndpointAccessState::OrganizationalActive(_)
            | EndpointAccessState::OrganizationalRevoked => Err(ChatError::new(
                ChatErrorCode::Permission,
                "The requested organizational context is unavailable",
                false,
            )),
        }
    }

    async fn active_scope(&self) -> ChatResult<Option<InternalMcpRunScope>> {
        match &*self.access.read().await {
            EndpointAccessState::DirectIdle | EndpointAccessState::DirectActive(_) => Ok(None),
            EndpointAccessState::OrganizationalPending => Err(ChatError::new(
                ChatErrorCode::Permission,
                "The requested organizational context is unavailable",
                false,
            )),
            EndpointAccessState::OrganizationalActive(scope) => Ok(Some(scope.as_ref().clone())),
            EndpointAccessState::OrganizationalRevoked => Err(ChatError::new(
                ChatErrorCode::Permission,
                "The requested organizational context is unavailable",
                false,
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InternalMcpEndpoint {
    pub url: String,
    pub bearer_token: String,
}

struct RunningEndpoint {
    descriptor: InternalMcpEndpoint,
    state: Arc<EndpointState>,
    shutdown: oneshot::Sender<()>,
}

#[derive(Default)]
pub struct InternalMcpRegistry {
    endpoints: Mutex<HashMap<ChatThreadId, RunningEndpoint>>,
}

impl InternalMcpRegistry {
    pub async fn ensure_thread_endpoint(
        &self,
        app: tauri::AppHandle,
        pool: SqlitePool,
        vault_root: PathBuf,
        thread_id: ChatThreadId,
        organizational_pending: bool,
    ) -> ChatResult<InternalMcpEndpoint> {
        if let Some(endpoint) = self.endpoints.lock().await.get(&thread_id) {
            return Ok(endpoint.descriptor.clone());
        }
        let token = generate_bearer_token()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(endpoint_error)?;
        let address = listener.local_addr().map_err(endpoint_error)?;
        let state = Arc::new(EndpointState::new(organizational_pending));
        let handler = ThreadMcpHandler {
            app,
            pool,
            vault_root,
            thread_id: thread_id.clone(),
            state: Arc::clone(&state),
        };
        let router = endpoint_router(handler, token.clone());
        let (shutdown, shutdown_receiver) = oneshot::channel();
        tauri::async_runtime::spawn(serve_endpoint(
            listener,
            router,
            EndpointConnectionLimits::default(),
            shutdown_receiver,
        ));
        let descriptor = InternalMcpEndpoint {
            url: format!("http://{address}/mcp"),
            bearer_token: token,
        };
        let mut endpoints = self.endpoints.lock().await;
        if let Some(existing) = endpoints.get(&thread_id) {
            let _ = shutdown.send(());
            return Ok(existing.descriptor.clone());
        }
        endpoints.insert(
            thread_id,
            RunningEndpoint {
                descriptor: descriptor.clone(),
                state,
                shutdown,
            },
        );
        Ok(descriptor)
    }

    pub async fn activate_run_scope(
        &self,
        thread_id: &ChatThreadId,
        scope: InternalMcpRunScope,
    ) -> ChatResult<()> {
        let state = self
            .endpoints
            .lock()
            .await
            .get(thread_id)
            .map(|endpoint| Arc::clone(&endpoint.state))
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::InvalidStateTransition,
                    "The internal host-tool endpoint is unavailable",
                    true,
                )
            })?;
        state.tools.reset_cursors().await;
        *state.access.write().await = EndpointAccessState::OrganizationalActive(Box::new(scope));
        Ok(())
    }

    pub async fn wait_until_ready(&self, thread_id: &ChatThreadId) -> ChatResult<()> {
        let state = self
            .endpoints
            .lock()
            .await
            .get(thread_id)
            .map(|endpoint| Arc::clone(&endpoint.state))
            .ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::InvalidStateTransition,
                    "The internal host-tool endpoint is unavailable",
                    true,
                )
            })?;
        state.wait_until_initialized().await
    }

    pub async fn revoke_run_scope(&self, thread_id: &ChatThreadId) {
        if let Some(endpoint) = self.endpoints.lock().await.remove(thread_id) {
            *endpoint.state.access.write().await = EndpointAccessState::OrganizationalRevoked;
            let _ = endpoint.shutdown.send(());
        }
    }

    pub async fn revoke_matching_run_scope(
        &self,
        thread_id: &ChatThreadId,
        run_id: &ChatAgentRunId,
        turn_id: &ChatTurnId,
        authorization_revision_id: &ChatAuthorizationRevisionId,
    ) -> bool {
        let mut endpoints = self.endpoints.lock().await;
        let Some(endpoint) = endpoints.get(thread_id) else {
            return false;
        };
        let matches = matches!(
            &*endpoint.state.access.read().await,
            EndpointAccessState::OrganizationalActive(scope)
                if scope.run_id == *run_id
                    && scope.provider_turn_id == *turn_id
                    && scope.authorization_revision_id == *authorization_revision_id
        );
        if !matches {
            return false;
        }
        let Some(endpoint) = endpoints.remove(thread_id) else {
            return false;
        };
        drop(endpoints);
        *endpoint.state.access.write().await = EndpointAccessState::OrganizationalRevoked;
        let _ = endpoint.shutdown.send(());
        true
    }

    async fn endpoint_state(&self, thread_id: &ChatThreadId) -> Option<Arc<EndpointState>> {
        self.endpoints
            .lock()
            .await
            .get(thread_id)
            .map(|endpoint| Arc::clone(&endpoint.state))
    }

    /// Holds preview tools while a turn is dispatched, until `settle_turn_network` runs.
    pub async fn begin_turn_network(&self, thread_id: &ChatThreadId) {
        if let Some(state) = self.endpoint_state(thread_id).await {
            state.preview_network.lock().await.begin_turn();
        }
    }

    /// Opens a direct session's host tools for the turn being dispatched.
    pub async fn begin_direct_turn(&self, thread_id: &ChatThreadId, turn_id: &ChatTurnId) {
        if let Some(state) = self.endpoint_state(thread_id).await {
            state.begin_direct_turn(turn_id).await;
        }
    }

    /// Closes a direct session's host tools when `turn_id` settles, fails to dispatch, or is
    /// interrupted, or for any turn when the provider session exits (`None`).
    pub async fn end_direct_turn(&self, thread_id: &ChatThreadId, turn_id: Option<&ChatTurnId>) {
        if let Some(state) = self.endpoint_state(thread_id).await {
            state.end_direct_turn(turn_id).await;
        }
    }

    /// Records the dispatched turn's sandbox network policy and releases waiting preview tools.
    pub async fn settle_turn_network(
        &self,
        thread_id: &ChatThreadId,
        network_access: Option<bool>,
    ) {
        if let Some(state) = self.endpoint_state(thread_id).await {
            state
                .preview_network
                .lock()
                .await
                .settle_turn(network_access);
            state.preview_network_settled.notify_waiters();
        }
    }

    /// Lets preview tools run for the rest of the thread's provider session even though its
    /// sandbox denies network access.
    pub async fn approve_preview_network(&self, thread_id: &ChatThreadId) {
        if let Some(state) = self.endpoint_state(thread_id).await {
            state.preview_network.lock().await.approve();
        }
    }

    pub async fn stop_thread_endpoint(&self, thread_id: &ChatThreadId) {
        if let Some(endpoint) = self.endpoints.lock().await.remove(thread_id) {
            let _ = endpoint.shutdown.send(());
        }
    }

    pub async fn stop_all(&self) -> u64 {
        let endpoints = std::mem::take(&mut *self.endpoints.lock().await);
        let count = endpoints.len() as u64;
        for endpoint in endpoints.into_values() {
            let _ = endpoint.shutdown.send(());
        }
        count
    }
}

#[derive(Clone)]
struct ThreadMcpHandler {
    app: tauri::AppHandle,
    pool: SqlitePool,
    vault_root: PathBuf,
    thread_id: ChatThreadId,
    state: Arc<EndpointState>,
}

impl ServerHandler for ThreadMcpHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_resources()
                .enable_tools()
                .build(),
        )
        .with_instructions("Thread-scoped Ganbaru Chat resources and workspace preview tools")
    }

    fn on_initialized(
        &self,
        _context: NotificationContext<RoleServer>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.state.mark_initialized();
        std::future::ready(())
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let organizational_scope = self.state.active_scope().await.map_err(mcp_error)?;
        if let Some(scope) = organizational_scope.as_ref() {
            super::internal_mcp_tools::verify_scope(&self.pool, &self.thread_id, scope)
                .await
                .map_err(mcp_error)?;
        }
        if request.and_then(|value| value.cursor).is_some() {
            return Err(McpError::invalid_params(
                "Ganbaru Chat resources fit in one bounded page",
                None,
            ));
        }
        if organizational_scope.is_some() {
            return Ok(ListResourcesResult::with_all_items(Vec::new()));
        }
        let resources = resources::list_thread_resources(&self.pool, &self.thread_id)
            .await
            .map_err(mcp_error)?
            .into_iter()
            .map(|resource| {
                Resource::new(resource.resource_uri, resource.id)
                    .with_title(resource.display_name)
                    .with_mime_type(resource.mime_type)
                    .with_size(resource.byte_size)
            })
            .collect();
        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        self.state.require_direct_turn().await.map_err(mcp_error)?;
        let resource_id = request
            .uri
            .strip_prefix(RESOURCE_URI_PREFIX)
            .ok_or_else(|| {
                McpError::invalid_params("Resource URI is outside this Chat thread", None)
            })?;
        let (resource, bytes) = bounded_work(
            context.ct.cancelled(),
            MCP_PREVIEW_WORK_TIMEOUT,
            resources::read_thread_resource_bytes(
                &self.pool,
                &self.vault_root,
                &self.thread_id,
                resource_id,
            ),
        )
        .await
        .map_err(mcp_error)?;
        let contents = match resource.kind {
            ChatResourceKind::TextSnippet => ResourceContents::text(
                String::from_utf8(bytes).map_err(|_| {
                    McpError::internal_error("Chat text resource is not valid UTF-8", None)
                })?,
                resource.resource_uri,
            )
            .with_mime_type(resource.mime_type),
            ChatResourceKind::Image
            | ChatResourceKind::BrowserScreenshot
            | ChatResourceKind::BrowserRecording => ResourceContents::blob(
                general_purpose::STANDARD.encode(bytes),
                resource.resource_uri,
            )
            .with_mime_type(resource.mime_type),
        };
        Ok(ReadResourceResult::new(vec![contents]).into())
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let organizational_scope = self.state.active_scope().await.map_err(mcp_error)?;
        if let Some(scope) = organizational_scope.as_ref() {
            super::internal_mcp_tools::verify_scope(&self.pool, &self.thread_id, scope)
                .await
                .map_err(mcp_error)?;
        }
        if request.and_then(|value| value.cursor).is_some() {
            return Err(McpError::invalid_params(
                "Ganbaru preview tools fit in one bounded page",
                None,
            ));
        }
        let tools = match organizational_scope {
            Some(scope) => super::internal_mcp_tools::definitions(&scope),
            None => preview_tools(),
        };
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let arguments = request.arguments.unwrap_or_default();
        let name = request.name.as_ref();
        let result = if name.starts_with("chat_") {
            match self.state.active_scope().await {
                Ok(Some(scope)) => {
                    super::internal_mcp_tools::call(
                        super::internal_mcp_tools::HostToolContext {
                            app: &self.app,
                            pool: &self.pool,
                            thread_id: &self.thread_id,
                            scope: &scope,
                            runtime: &self.state.tools,
                        },
                        name,
                        &arguments,
                    )
                    .await
                }
                Ok(None) | Err(_) => Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "The requested organizational context is unavailable",
                    false,
                )),
            }
        } else {
            match self.state.require_direct_turn().await {
                // Organizational host tools above are not cancelled, because each one reserves
                // its audit row before running and must finish that row.
                Ok(turn_id) => {
                    self.audited_preview_call(
                        &turn_id,
                        name,
                        &arguments,
                        context.ct.clone().cancelled_owned(),
                    )
                    .await
                }
                Err(error) => Err(error),
            }
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.message)]).into(),
        })
    }
}

impl ThreadMcpHandler {
    /// Runs one preview tool call under its turn's budget and records it. The work and its
    /// audit settlement run as their own task, so the row settles even when the MCP request
    /// is dropped; cancellation and the work deadline still stop the tool itself.
    async fn audited_preview_call(
        &self,
        turn_id: &ChatTurnId,
        name: &str,
        arguments: &serde_json::Map<String, serde_json::Value>,
        cancelled: impl Future<Output = ()> + Send + 'static,
    ) -> ChatResult<serde_json::Value> {
        let invocation = preview_audit::reserve(&self.pool, turn_id, name, arguments).await?;
        let handler = self.clone();
        let name = name.to_string();
        let arguments = arguments.clone();
        tauri::async_runtime::spawn(async move {
            let result = bounded_work(
                cancelled,
                MCP_PREVIEW_WORK_TIMEOUT,
                handler.call_preview_tool(&name, &arguments),
            )
            .await;
            let (outcome, response_bytes, result) = preview_audit::settle(result);
            preview_audit::complete(&handler.pool, &invocation, outcome, response_bytes).await?;
            result
        })
        .await
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "The preview tool call stopped unexpectedly",
                false,
            )
        })?
    }

    /// Refuses preview tools in a session whose sandbox denies network access until the user
    /// approves them in the Browser panel, and asks for that approval.
    async fn require_preview_network(&self) -> ChatResult<()> {
        match self.state.preview_network_decision().await {
            PreviewNetworkDecision::Allowed => Ok(()),
            PreviewNetworkDecision::AwaitingTurn => Err(ChatError::new(
                ChatErrorCode::Busy,
                "Preview tools are available once the turn has started. Try again",
                true,
            )),
            PreviewNetworkDecision::NeedsApproval => {
                self.app
                    .state::<super::preview::ChatBrowserManager>()
                    .request_approval(
                        &self.app,
                        &self.thread_id,
                        super::preview::approvals::PendingPreviewApproval::Network,
                    );
                Err(super::preview::approvals::approval_pending_error(
                    "Using preview tools while this session's sandbox blocks network access",
                ))
            }
        }
    }

    async fn call_preview_tool(
        &self,
        name: &str,
        arguments: &serde_json::Map<String, serde_json::Value>,
    ) -> ChatResult<serde_json::Value> {
        if name != "preview_status" {
            self.require_preview_network().await?;
        }
        let tab_id = || super::preview::mcp_model_tab(&self.app, &self.thread_id);
        match name {
            "preview_status" => serde_json::to_value(super::preview::mcp_status_reads(
                &self.app,
                &self.thread_id,
            )?)
            .map_err(|_| preview_tool_error("encode preview status")),
            "preview_open" | "preview_navigate" => {
                let url = string_argument(arguments, "url")?;
                serde_json::to_value(
                    super::preview::mcp_navigate_browser(
                        &self.app,
                        &self.pool,
                        &self.thread_id,
                        url,
                    )
                    .await?,
                )
                .map_err(|_| preview_tool_error("encode preview navigation"))
            }
            "preview_resize" => {
                let width = u32_argument(arguments, "width")?;
                let height = u32_argument(arguments, "height")?;
                serde_json::to_value(
                    super::preview::mcp_resize_browser(
                        &self.app,
                        &self.pool,
                        &self.thread_id,
                        width,
                        height,
                    )
                    .await?,
                )
                .map_err(|_| preview_tool_error("encode preview viewport"))
            }
            "preview_snapshot" => {
                let result = super::preview::chat_browser_snapshot(
                    self.app.clone(),
                    self.thread_id.clone(),
                    tab_id()?,
                )
                .await?;
                Ok(serde_json::json!({ "snapshot": result }))
            }
            "preview_screenshot" => {
                let resource = super::preview::mcp_screenshot_browser(
                    &self.app,
                    &self.pool,
                    &self.vault_root,
                    &self.thread_id,
                    &tab_id()?,
                )
                .await?;
                Ok(serde_json::to_value(resource)
                    .map_err(|_| preview_tool_error("encode browser screenshot"))?)
            }
            "preview_click" => {
                super::preview::chat_browser_click(
                    self.app.clone(),
                    self.thread_id.clone(),
                    tab_id()?,
                    string_argument(arguments, "selector")?.to_string(),
                )
                .await?;
                Ok(serde_json::json!({ "accepted": true }))
            }
            "preview_type" => {
                super::preview::chat_browser_type(
                    self.app.clone(),
                    self.thread_id.clone(),
                    tab_id()?,
                    string_argument(arguments, "selector")?.to_string(),
                    string_argument(arguments, "text")?.to_string(),
                )
                .await?;
                Ok(serde_json::json!({ "accepted": true }))
            }
            "preview_press" => {
                super::preview::chat_browser_press(
                    self.app.clone(),
                    self.thread_id.clone(),
                    tab_id()?,
                    string_argument(arguments, "key")?.to_string(),
                )
                .await?;
                Ok(serde_json::json!({ "accepted": true }))
            }
            "preview_scroll" => {
                super::preview::chat_browser_scroll(
                    self.app.clone(),
                    self.thread_id.clone(),
                    tab_id()?,
                    number_argument(arguments, "x")?,
                    number_argument(arguments, "y")?,
                )
                .await?;
                Ok(serde_json::json!({ "accepted": true }))
            }
            "preview_wait_for" => {
                let selector = string_argument(arguments, "selector")?;
                let timeout = u32_argument(arguments, "timeoutMs")?.min(10_000);
                let tab_id = tab_id()?;
                let started = tokio::time::Instant::now();
                loop {
                    super::preview::mcp_require_tab_access(&self.app, &self.thread_id, &tab_id)?;
                    if super::preview::browser_selector_matches(
                        &self.app,
                        &self.thread_id,
                        &tab_id,
                        selector,
                    )
                    .await?
                    {
                        return Ok(serde_json::json!({ "matched": true }));
                    }
                    if started.elapsed() >= Duration::from_millis(u64::from(timeout)) {
                        return Err(ChatError::new(
                            ChatErrorCode::Timeout,
                            "Browser wait condition timed out",
                            true,
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            "preview_evaluate" => Err(ChatError::new(
                ChatErrorCode::Permission,
                "Browser JavaScript evaluation requires separate user approval in Ganbaru",
                true,
            )),
            "preview_recording_start" | "preview_recording_stop" => Err(ChatError::new(
                ChatErrorCode::Permission,
                "Browser recording requires separate user approval in Ganbaru",
                true,
            )),
            _ => Err(ChatError::new(
                ChatErrorCode::NotFound,
                "Unknown Ganbaru preview tool",
                true,
            )),
        }
    }
}

fn preview_tools() -> Vec<Tool> {
    vec![
        preview_tool(
            "preview_status",
            "Read the active thread preview status",
            serde_json::json!({}),
        ),
        preview_tool(
            "preview_open",
            "Open a loopback URL in the active preview",
            string_schema("url"),
        ),
        preview_tool(
            "preview_navigate",
            "Navigate the active preview to a loopback URL",
            string_schema("url"),
        ),
        preview_tool(
            "preview_resize",
            "Resize the active preview viewport in physical pixels; the size is clamped to the window and capture limits and the applied size is returned",
            serde_json::json!({
                "width": { "type": "integer", "minimum": 1, "maximum": 16384 },
                "height": { "type": "integer", "minimum": 1, "maximum": 16384 }
            }),
        ),
        preview_tool(
            "preview_snapshot",
            "Read a bounded semantic DOM snapshot",
            serde_json::json!({}),
        ),
        preview_tool(
            "preview_screenshot",
            "Capture the visible preview as a managed PNG resource",
            serde_json::json!({}),
        ),
        preview_tool(
            "preview_click",
            "Click an element in the active preview",
            string_schema("selector"),
        ),
        preview_tool(
            "preview_type",
            "Type text into an element in the active preview",
            serde_json::json!({
                "selector": { "type": "string", "maxLength": 262144 },
                "text": { "type": "string", "maxLength": 262144 }
            }),
        ),
        preview_tool(
            "preview_press",
            "Send a key to the focused preview element",
            string_schema("key"),
        ),
        preview_tool(
            "preview_scroll",
            "Scroll the active preview",
            serde_json::json!({
                "x": { "type": "number", "minimum": -1000000, "maximum": 1000000 },
                "y": { "type": "number", "minimum": -1000000, "maximum": 1000000 }
            }),
        ),
        preview_tool(
            "preview_wait_for",
            "Wait for a selector in the active preview",
            serde_json::json!({
                "selector": { "type": "string", "maxLength": 262144 },
                "timeoutMs": { "type": "integer", "minimum": 0, "maximum": 10000 }
            }),
        ),
        preview_tool(
            "preview_evaluate",
            "Evaluate approved JavaScript in the active preview",
            string_schema("script"),
        ),
        preview_tool(
            "preview_recording_start",
            "Start bounded preview recording",
            serde_json::json!({}),
        ),
        preview_tool(
            "preview_recording_stop",
            "Stop preview recording",
            serde_json::json!({}),
        ),
    ]
}

fn preview_tool(
    name: &'static str,
    description: &'static str,
    properties: serde_json::Value,
) -> Tool {
    let properties = properties.as_object().cloned().unwrap_or_default();
    let required = properties
        .keys()
        .cloned()
        .map(serde_json::Value::String)
        .collect::<Vec<_>>();
    let schema = serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    Tool::new(name, description, schema).with_annotations(
        ToolAnnotations::new()
            .read_only(matches!(
                name,
                "preview_status" | "preview_snapshot" | "preview_screenshot" | "preview_wait_for"
            ))
            .destructive(false)
            .open_world(matches!(name, "preview_open" | "preview_navigate")),
    )
}

fn string_schema(name: &str) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    properties.insert(
        name.to_string(),
        serde_json::json!({ "type": "string", "maxLength": 262144 }),
    );
    serde_json::Value::Object(properties)
}

fn string_argument<'a>(
    arguments: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> ChatResult<&'a str> {
    arguments
        .get(name)
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() <= 256 * 1024 && !value.contains('\0'))
        .ok_or_else(|| ChatError::validation(name, "Preview tool argument is invalid"))
}

fn u32_argument(
    arguments: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> ChatResult<u32> {
    arguments
        .get(name)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| ChatError::validation(name, "Preview tool argument is invalid"))
}

fn number_argument(
    arguments: &serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> ChatResult<f64> {
    arguments
        .get(name)
        .and_then(serde_json::Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(|| ChatError::validation(name, "Preview tool argument is invalid"))
}

fn preview_tool_error(operation: &str) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        format!("Could not {operation}"),
        true,
    )
}

/// Builds the authenticated `/mcp` route for one endpoint. rmcp reads request bodies itself,
/// so the body cap is set on its service rather than as an axum layer.
fn endpoint_router<H>(handler: H, token: String) -> Router
where
    H: ServerHandler + Clone + Send + Sync + 'static,
{
    let sessions = Arc::new(LocalSessionManager::default());
    let service: StreamableHttpService<H, LocalSessionManager> = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::clone(&sessions),
        StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_max_request_body_bytes(MAX_MCP_REQUEST_BYTES),
    );
    let semaphore = Arc::new(Semaphore::new(MAX_MCP_CONCURRENCY));
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn(move |request, next| {
            authorize_request(
                request,
                next,
                token.clone(),
                semaphore.clone(),
                Arc::clone(&sessions),
            )
        }))
}

/// Connection bounds enforced before any request reaches the bearer-token check.
#[derive(Clone, Copy, Debug)]
struct EndpointConnectionLimits {
    header_read_timeout: Duration,
    max_connections: usize,
}

impl Default for EndpointConnectionLimits {
    fn default() -> Self {
        Self {
            header_read_timeout: MCP_HEADER_READ_TIMEOUT,
            max_connections: MAX_MCP_CONNECTIONS,
        }
    }
}

/// Serves one endpoint over HTTP/1 until `shutdown` resolves or its sender is dropped. Unlike
/// `axum::serve`, every connection must send its request headers within the header timeout,
/// and at most `max_connections` are open at once, so local processes without the token cannot
/// hold connections open indefinitely.
async fn serve_endpoint(
    listener: tokio::net::TcpListener,
    router: Router,
    limits: EndpointConnectionLimits,
    mut shutdown: oneshot::Receiver<()>,
) {
    let connections = Arc::new(Semaphore::new(limits.max_connections));
    let (closing, closed) = tokio::sync::watch::channel(());
    loop {
        let permit = tokio::select! {
            _ = &mut shutdown => break,
            permit = Arc::clone(&connections).acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        let stream = tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => {
                    tokio::time::sleep(MCP_ACCEPT_RETRY_DELAY).await;
                    continue;
                }
            },
        };
        let service = hyper_util::service::TowerToHyperService::new(router.clone());
        let mut closed = closed.clone();
        tokio::spawn(async move {
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(limits.header_read_timeout);
            let connection =
                builder.serve_connection(hyper_util::rt::TokioIo::new(stream), service);
            tokio::pin!(connection);
            tokio::select! {
                _ = connection.as_mut() => {}
                _ = closed.changed() => {
                    connection.as_mut().graceful_shutdown();
                    let _ = connection.await;
                }
            }
            drop(permit);
        });
    }
    drop(closing);
}

/// Whether a request may reach rmcp. A request without a session ID creates a session, so it
/// is refused while the endpoint already holds `max_sessions` open sessions; idle sessions
/// expire through rmcp's keep-alive.
fn admits_session_request(
    request: &Request<Body>,
    open_sessions: usize,
    max_sessions: usize,
) -> bool {
    request.headers().contains_key(MCP_SESSION_ID_HEADER) || open_sessions < max_sessions
}

async fn authorize_request(
    request: Request<Body>,
    next: Next,
    expected_token: String,
    semaphore: Arc<Semaphore>,
    sessions: Arc<LocalSessionManager>,
) -> Response {
    if !request_targets_loopback(&request)
        || !request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .is_some_and(|value| constant_time_equal(value.as_bytes(), expected_token.as_bytes()))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let open_sessions = sessions.sessions.read().await.len();
    if !admits_session_request(&request, open_sessions, MAX_MCP_SESSIONS) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(permit) = semaphore.acquire_owned().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match tokio::time::timeout(MCP_REQUEST_TIMEOUT, next.run(request)).await {
        Ok(response) => {
            drop(permit);
            response
        }
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}

fn request_targets_loopback(request: &Request<Body>) -> bool {
    let host_ok = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("127.0.0.1:") || value.starts_with("localhost:"));
    let origin_ok = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| {
            value.starts_with("http://127.0.0.1:") || value.starts_with("http://localhost:")
        });
    host_ok && origin_ok
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let maximum = left.len().max(right.len());
    for index in 0..maximum {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn generate_bearer_token() -> ChatResult<String> {
    secure_random_hex(32)
}

pub(crate) fn generate_opaque_handle(prefix: &str) -> ChatResult<String> {
    if prefix.is_empty()
        || prefix.len() > 32
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(ChatError::validation(
            "handlePrefix",
            "Internal handle prefix is invalid",
        ));
    }
    Ok(format!("{prefix}:{}", secure_random_hex(32)?))
}

pub(crate) fn secure_random_hex(byte_count: usize) -> ChatResult<String> {
    let mut bytes = vec![0_u8; byte_count];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Operating-system secure randomness is unavailable",
                false,
            )
        })?;
    let mut encoded = String::with_capacity(byte_count.saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Secure token encoding failed",
                false,
            )
        })?;
    }
    Ok(encoded)
}

/// Runs tool work until it finishes, the request is cancelled, or `deadline` passes. Dropping
/// the work stops it at its next await point, so it cannot outlive its request.
async fn bounded_work<T>(
    cancelled: impl Future<Output = ()>,
    deadline: Duration,
    work: impl Future<Output = ChatResult<T>>,
) -> ChatResult<T> {
    tokio::select! {
        biased;
        () = cancelled => Err(ChatError::new(
            ChatErrorCode::Cancelled,
            "The tool call was cancelled",
            false,
        )),
        result = tokio::time::timeout(deadline, work) => result.unwrap_or_else(|_| {
            Err(ChatError::new(
                ChatErrorCode::Timeout,
                "The tool call took too long",
                true,
            ))
        }),
    }
}

fn mcp_error(error: ChatError) -> McpError {
    match error.code {
        ChatErrorCode::NotFound | ChatErrorCode::Permission | ChatErrorCode::Validation => {
            McpError::invalid_params(error.message, None)
        }
        _ => McpError::internal_error(error.message, None),
    }
}

fn endpoint_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Thread resource endpoint could not start",
        true,
    )
}

#[cfg(test)]
mod tests {
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
        let router = endpoint_router(EmptyHandler, token.clone());
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
                authorization_revision_id: ChatAuthorizationRevisionId::new("authorization-1")
                    .unwrap(),
                destination_conversation_id: ChatConversationId::new("conversation-1").unwrap(),
                authorization_scope_digest: "a".repeat(64),
                execution_environment_id: Some("environment-1".to_string()),
                scratch_generation_id: None,
                runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
                channel_sources: Vec::new(),
                folder_sources: Vec::new(),
            }));
        assert!(state.active_scope().await.unwrap().is_some());
        *state.access.write().await = EndpointAccessState::OrganizationalRevoked;
        assert!(state.active_scope().await.is_err());
    }
}

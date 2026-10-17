use axum::Router;
use axum::body::Body;
use axum::extract::DefaultBodyLimit;
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
const MCP_INITIALIZATION_TIMEOUT: Duration = Duration::from_secs(15);
const RESOURCE_URI_PREFIX: &str = "ganbaru://chat/resource/";

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
}

#[derive(Clone, Debug, Default)]
enum EndpointAccessState {
    #[default]
    DirectUnscoped,
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
                EndpointAccessState::DirectUnscoped
            }),
            tools: super::internal_mcp_tools::InternalMcpToolRuntime::default(),
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

    async fn active_scope(&self) -> ChatResult<Option<InternalMcpRunScope>> {
        match &*self.access.read().await {
            EndpointAccessState::DirectUnscoped => Ok(None),
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
        let service: StreamableHttpService<ThreadMcpHandler, LocalSessionManager> =
            StreamableHttpService::new(
                move || Ok(handler.clone()),
                Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default()
                    .with_json_response(true)
                    .with_sse_keep_alive(None),
            );
        let semaphore = Arc::new(Semaphore::new(MAX_MCP_CONCURRENCY));
        let expected_token = token.clone();
        let router = Router::new()
            .nest_service("/mcp", service)
            .layer(DefaultBodyLimit::max(MAX_MCP_REQUEST_BYTES))
            .layer(middleware::from_fn(move |request, next| {
                authorize_request(request, next, expected_token.clone(), semaphore.clone())
            }));
        let (shutdown, shutdown_receiver) = oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_receiver.await;
                })
                .await;
        });
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
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if self
            .state
            .active_scope()
            .await
            .map_err(mcp_error)?
            .is_some()
        {
            return Err(mcp_error(ChatError::new(
                ChatErrorCode::Permission,
                "The requested organizational context is unavailable",
                false,
            )));
        }
        let resource_id = request
            .uri
            .strip_prefix(RESOURCE_URI_PREFIX)
            .ok_or_else(|| {
                McpError::invalid_params("Resource URI is outside this Chat thread", None)
            })?;
        let (resource, bytes) = resources::read_thread_resource_bytes(
            &self.pool,
            &self.vault_root,
            &self.thread_id,
            resource_id,
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
        _context: RequestContext<RoleServer>,
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
            match self.state.active_scope().await {
                Ok(None) => self.call_preview_tool(name, &arguments).await,
                Ok(Some(_)) | Err(_) => Err(ChatError::new(
                    ChatErrorCode::Permission,
                    "The requested organizational context is unavailable",
                    false,
                )),
            }
        };
        Ok(match result {
            Ok(value) => CallToolResult::structured(value).into(),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error.message)]).into(),
        })
    }
}

impl ThreadMcpHandler {
    async fn call_preview_tool(
        &self,
        name: &str,
        arguments: &serde_json::Map<String, serde_json::Value>,
    ) -> ChatResult<serde_json::Value> {
        let tab_id = || super::preview::mcp_active_tab_id(&self.app, &self.thread_id);
        match name {
            "preview_status" => serde_json::to_value(
                self.app
                    .state::<super::preview::ChatBrowserManager>()
                    .thread_reads(&self.thread_id)?,
            )
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
                let script = format!(
                    "Boolean(document.querySelector({}))",
                    serde_json::to_string(selector)
                        .map_err(|_| preview_tool_error("encode wait selector"))?
                );
                let started = tokio::time::Instant::now();
                loop {
                    let result = super::preview::evaluate_script(
                        &self.app,
                        &self.thread_id,
                        &tab_id()?,
                        &script,
                    )
                    .await?;
                    if result == "true" || result == "\"true\"" {
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
            "Resize the active preview viewport",
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

async fn authorize_request(
    request: Request<Body>,
    next: Next,
    expected_token: String,
    semaphore: Arc<Semaphore>,
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

fn secure_random_hex(byte_count: usize) -> ChatResult<String> {
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

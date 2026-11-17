//! Organizational Claude authority verification.
//!
//! An organizational launch proves its tool surface twice. Gate 1 reads the `mcp_status`
//! report right after `initialize`, before the session is ready. Gate 2 reads every
//! `system/init` frame, which the CLI sends at the start of each turn. Until the first init
//! passes, the router holds inbound frames and denies every permission request, and a
//! deadline armed by the first user message bounds the wait. Any failure stops the process.

use super::protocol::{
    control_error, control_success, organizational_permission_error,
    organizational_runs_unattended, organizational_tools,
};
use super::transport::{ClaudeConnectionKill, ClaudeJsonlClient, ClaudeTransportFailure};
use crate::DriverOperationContext;
use crate::models::{ChatResult, ProviderOrganizationalScope, TurnModeSnapshot};
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;

/// Bounds the wait for the first verified `system/init`, counted from the first user message.
pub const ORGANIZATIONAL_INIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Frames held before the first verified init. The CLI sends init first, so a full buffer
/// means the stream is not the expected shape and the run fails closed.
const MAX_BUFFERED_FRAMES: usize = 256;
const MAX_BUFFERED_BYTES: usize = 8 * 1024 * 1024;
const MCP_SERVER_CONNECTED: &str = "connected";
const MCP_SERVER_PENDING: &str = "pending";
const UNVERIFIED_TOOL_DENIAL: &str =
    "Ganbaru has not verified this session's tool surface yet. Do not retry this tool.";
const UNVERIFIED_CONTROL_REQUEST: &str = "Control requests wait for tool surface verification";

/// One MCP server entry. Only the name and status are read; the full reports also carry
/// server configuration, including expanded credentials, which must never be kept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeMcpServerStatus {
    pub name: String,
    pub status: String,
}

/// The tool surface one `system/init` frame reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeInitReport {
    pub mcp_servers: Vec<ClaudeMcpServerStatus>,
    pub tools: Vec<String>,
}

/// Gate 1: the server report holds exactly the internal server, connected or still pending.
pub fn verify_organizational_mcp(
    servers: &[ClaudeMcpServerStatus],
    expected_server: &str,
) -> ChatResult<()> {
    verify_servers(
        servers,
        expected_server,
        &[MCP_SERVER_CONNECTED, MCP_SERVER_PENDING],
    )
}

/// Gate 2: the init report holds exactly the connected internal server, no tool from another
/// MCP server, and only built-in tools the scope allows, never one it denies. An allowed tool
/// may be absent, because the CLI drops names it does not ship.
pub fn verify_organizational_init(
    init: &ClaudeInitReport,
    expected_server: &str,
    scope: ProviderOrganizationalScope,
) -> ChatResult<()> {
    let tools = organizational_tools(scope);
    verify_servers(&init.mcp_servers, expected_server, &[MCP_SERVER_CONNECTED])?;
    let internal_prefix = format!("mcp__{expected_server}__");
    for tool in &init.tools {
        if tool.starts_with(&internal_prefix) {
            continue;
        }
        if !tools.allowed.contains(&tool.as_str()) || tools.disallowed.contains(&tool.as_str()) {
            return Err(organizational_permission_error());
        }
    }
    Ok(())
}

fn verify_servers(
    servers: &[ClaudeMcpServerStatus],
    expected_server: &str,
    accepted_statuses: &[&str],
) -> ChatResult<()> {
    let mut found = false;
    for server in servers {
        if server.name != expected_server || found {
            return Err(organizational_permission_error());
        }
        if !accepted_statuses.contains(&server.status.as_str()) {
            return Err(organizational_permission_error());
        }
        found = true;
    }
    if found {
        Ok(())
    } else {
        Err(organizational_permission_error())
    }
}

/// Reads the `mcp_status` response. A missing or malformed list fails closed, and no part of
/// the response reaches the error.
pub fn parse_mcp_status(response: &Value) -> ChatResult<Vec<ClaudeMcpServerStatus>> {
    parse_servers(response.get("mcpServers"))
}

/// Reads the server and tool lists from a `system/init` frame. Missing lists fail closed.
pub fn parse_init_report(frame: &Map<String, Value>) -> ChatResult<ClaudeInitReport> {
    let tools = frame
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(organizational_permission_error)?
        .iter()
        .map(|tool| {
            tool.as_str()
                .map(str::to_string)
                .ok_or_else(organizational_permission_error)
        })
        .collect::<ChatResult<Vec<_>>>()?;
    Ok(ClaudeInitReport {
        mcp_servers: parse_servers(frame.get("mcp_servers"))?,
        tools,
    })
}

fn parse_servers(value: Option<&Value>) -> ChatResult<Vec<ClaudeMcpServerStatus>> {
    value
        .and_then(Value::as_array)
        .ok_or_else(organizational_permission_error)?
        .iter()
        .map(|server| {
            let field = |key: &str| {
                server
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(organizational_permission_error)
            };
            Ok(ClaudeMcpServerStatus {
                name: field("name")?,
                status: field("status")?,
            })
        })
        .collect()
}

/// Runs gate 1 over an initialized connection. Cancellation and timeouts keep their own
/// errors; every other failure, including a CLI that rejects the request, fails closed.
pub async fn verify_mcp_status(
    client: &ClaudeJsonlClient,
    expected_server: &str,
    context: &DriverOperationContext,
) -> ChatResult<()> {
    let response = client
        .request("mcp_status", json!({}), context)
        .await
        .map_err(|failure| match failure {
            ClaudeTransportFailure::Cancelled | ClaudeTransportFailure::Timeout => {
                failure.to_chat_error("MCP status")
            }
            _ => organizational_permission_error(),
        })?;
    verify_organizational_mcp(&parse_mcp_status(&response)?, expected_server)
}

/// The driver's half of the gate 2 deadline: each user message arms it once.
#[derive(Clone)]
pub struct ClaudeOrganizationalDeadline {
    sender: watch::Sender<Option<Instant>>,
}

impl ClaudeOrganizationalDeadline {
    /// Starts the init deadline unless one is already running. Returns whether this call
    /// started it, so a failed dispatch can clear only its own deadline.
    pub fn arm(&self) -> bool {
        self.sender.send_if_modified(|deadline| {
            if deadline.is_some() {
                return false;
            }
            *deadline = Some(Instant::now() + ORGANIZATIONAL_INIT_TIMEOUT);
            true
        })
    }

    /// Clears the deadline after a user message that never reached the CLI.
    pub fn disarm(&self) {
        self.sender.send_replace(None);
    }
}

/// The router's half of organizational verification.
pub struct ClaudeOrganizationalRoute {
    pub expected_server: String,
    /// The scope whose tool surface each init report must match.
    pub scope: ProviderOrganizationalScope,
    pub deadline: watch::Receiver<Option<Instant>>,
    pub kill: ClaudeConnectionKill,
}

/// Creates the paired deadline handles for one organizational session.
pub fn organizational_route(
    expected_server: String,
    scope: ProviderOrganizationalScope,
    kill: ClaudeConnectionKill,
) -> (ClaudeOrganizationalDeadline, ClaudeOrganizationalRoute) {
    let (sender, deadline) = watch::channel(None);
    (
        ClaudeOrganizationalDeadline { sender },
        ClaudeOrganizationalRoute {
            expected_server,
            scope,
            deadline,
            kill,
        },
    )
}

/// What the router does with one inbound frame under organizational verification.
#[derive(Debug, PartialEq)]
pub enum ClaudeGateAction {
    /// Route these frames in order.
    Route(Vec<Value>),
    /// The frame is held until the first init passes.
    Hold,
    /// Send this reply to an unverified control request and route nothing.
    Reply(Value),
    /// Verification failed; stop the process and fail the turn.
    Fail,
}

/// Verification state for one organizational session.
pub struct ClaudeOrganizationalGate {
    route: ClaudeOrganizationalRoute,
    verified: bool,
    buffer: Vec<Value>,
    buffered_bytes: usize,
}

impl ClaudeOrganizationalGate {
    pub fn new(route: ClaudeOrganizationalRoute) -> Self {
        Self {
            route,
            verified: false,
            buffer: Vec::new(),
            buffered_bytes: 0,
        }
    }

    pub fn verified(&self) -> bool {
        self.verified
    }

    pub fn kill_handle(&self) -> &ClaudeConnectionKill {
        &self.route.kill
    }

    /// Drops every held frame without routing it.
    pub fn discard(&mut self) {
        self.buffer.clear();
        self.buffered_bytes = 0;
    }

    /// Decides how one inbound frame is handled.
    pub fn admit(&mut self, frame: Value) -> ChatResult<ClaudeGateAction> {
        if is_system_init(&frame) {
            return Ok(self.admit_init(frame));
        }
        if self.verified {
            return Ok(ClaudeGateAction::Route(vec![frame]));
        }
        if frame.get("type").and_then(Value::as_str) == Some("control_request") {
            return unverified_reply(&frame).map(ClaudeGateAction::Reply);
        }
        let bytes = serde_json::to_vec(&frame).map_or(usize::MAX, |bytes| bytes.len());
        self.buffered_bytes = self.buffered_bytes.saturating_add(bytes);
        if self.buffer.len() >= MAX_BUFFERED_FRAMES || self.buffered_bytes > MAX_BUFFERED_BYTES {
            self.discard();
            return Ok(ClaudeGateAction::Fail);
        }
        self.buffer.push(frame);
        Ok(ClaudeGateAction::Hold)
    }

    fn admit_init(&mut self, frame: Value) -> ClaudeGateAction {
        let passed = frame
            .as_object()
            .ok_or_else(organizational_permission_error)
            .and_then(parse_init_report)
            .and_then(|init| {
                verify_organizational_init(&init, &self.route.expected_server, self.route.scope)
            })
            .is_ok();
        if !passed {
            self.discard();
            return ClaudeGateAction::Fail;
        }
        self.verified = true;
        self.buffered_bytes = 0;
        let mut frames = std::mem::take(&mut self.buffer);
        frames.push(frame);
        ClaudeGateAction::Route(frames)
    }

    /// Completes when the armed init deadline passes. It never completes while no deadline
    /// is armed, and a later arm or disarm is picked up.
    pub async fn deadline_elapsed(&mut self) {
        loop {
            let current = *self.route.deadline.borrow_and_update();
            let changed = async {
                if self.route.deadline.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            };
            match current {
                Some(deadline) => tokio::select! {
                    () = tokio::time::sleep_until(deadline) => return,
                    () = changed => {}
                },
                None => changed.await,
            }
        }
    }
}

fn is_system_init(frame: &Value) -> bool {
    frame.get("type").and_then(Value::as_str) == Some("system")
        && frame.get("subtype").and_then(Value::as_str) == Some("init")
}

/// Denies a permission request and refuses any other control request without opening a
/// pending entry or an event.
fn unverified_reply(frame: &Value) -> ChatResult<Value> {
    let request_id = frame
        .get("request_id")
        .and_then(Value::as_str)
        .ok_or_else(organizational_permission_error)?;
    let subtype = frame
        .get("request")
        .and_then(|request| request.get("subtype"))
        .and_then(Value::as_str);
    if subtype == Some("can_use_tool") {
        control_success(
            request_id,
            json!({ "behavior": "deny", "message": UNVERIFIED_TOOL_DENIAL }),
        )
    } else {
        control_error(request_id, UNVERIFIED_CONTROL_REQUEST)
    }
}

/// Message a denied organizational permission request returns to the CLI.
const OUTSIDE_SCOPE_DENIAL: &str =
    "This agent run cannot use that tool or path. Stay inside the working directory.";

/// Permission routing of an organizational session, as defense in depth behind the launch
/// surface: tools the scope does not allow and file paths outside the working directory are
/// denied before they reach the person, and nothing grants a session-wide rule.
#[derive(Clone, Debug)]
pub struct ClaudeOrganizationalPermissions {
    scope: ProviderOrganizationalScope,
    root: PathBuf,
    internal_prefix: String,
    /// Whether the session's shell commands run under the Ganbaru sandbox.
    commands_confined: bool,
}

impl ClaudeOrganizationalPermissions {
    /// Builds the routing policy for a session rooted at the working directory. The root is
    /// canonicalized when it exists; a root that cannot be resolved admits no file path.
    pub fn new(scope: ProviderOrganizationalScope, root: PathBuf, expected_server: &str) -> Self {
        Self {
            scope,
            root: std::fs::canonicalize(&root).unwrap_or(root),
            internal_prefix: format!("mcp__{expected_server}__"),
            commands_confined: false,
        }
    }

    /// Records whether the session's shell commands run under the Ganbaru sandbox.
    pub fn with_confined_commands(mut self, commands_confined: bool) -> Self {
        self.commands_confined = commands_confined;
        self
    }

    /// Whether a request for `tool_name` in a turn with `modes` is allowed without asking.
    pub fn runs_unattended(&self, tool_name: &str, modes: TurnModeSnapshot) -> bool {
        organizational_runs_unattended(tool_name, modes, self.commands_confined)
    }

    /// The denial message for a permission request outside the scope, or none when the
    /// request may proceed. Shell commands are never denied here.
    pub fn denial(&self, tool_name: &str, input: &Value) -> Option<&'static str> {
        if tool_name.starts_with(&self.internal_prefix) {
            return None;
        }
        let tools = organizational_tools(self.scope);
        if !tools.allowed.contains(&tool_name) || tools.disallowed.contains(&tool_name) {
            return Some(OUTSIDE_SCOPE_DENIAL);
        }
        let path = match tool_name {
            "Read" | "Edit" | "Write" => Some(input.get("file_path")),
            "NotebookEdit" => Some(input.get("notebook_path")),
            "Glob" | "Grep" => input.get("path").filter(|path| !path.is_null()).map(Some),
            _ => None,
        };
        let path_inside = match path {
            None => true,
            Some(Some(Value::String(path))) => inside_root(&self.root, path),
            Some(_) => false,
        };
        let pattern_inside = tool_name != "Glob"
            || input
                .get("pattern")
                .and_then(Value::as_str)
                .is_some_and(|pattern| pattern_stays_inside(&self.root, pattern));
        (!path_inside || !pattern_inside).then_some(OUTSIDE_SCOPE_DENIAL)
    }
}

/// Whether a Glob pattern stays under `root`: it never climbs with `..` or starts from the
/// home directory, and an absolute pattern starts with the root.
fn pattern_stays_inside(root: &Path, pattern: &str) -> bool {
    let pattern = Path::new(pattern);
    !pattern.as_os_str().is_empty()
        && !pattern.to_string_lossy().starts_with('~')
        && !pattern
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        && (pattern.is_relative() || pattern.starts_with(root))
}

/// Whether `raw` resolves inside `root`. A relative path resolves against the root, and the
/// nearest existing ancestor is canonicalized so symbolic links and parent components cannot
/// lead out. A missing component that climbs, or a path the CLI would expand from the home
/// directory, counts as outside.
fn inside_root(root: &Path, raw: &str) -> bool {
    if raw.is_empty() || raw.starts_with('~') || raw.contains('\0') {
        return false;
    }
    let candidate = root.join(raw);
    let mut existing = candidate.as_path();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            return canonical.starts_with(root);
        }
        // A missing final component that is not a plain name, such as `..`, has no file
        // name, so the walk stops there and the path counts as outside.
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(_)) => existing = parent,
            _ => return false,
        }
    }
}

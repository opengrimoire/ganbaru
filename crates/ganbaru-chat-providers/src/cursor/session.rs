//! Cursor ACP session startup, configuration, and inbound request routing.

use super::driver::AcpProviderFlavor;
use super::interactions::*;
use super::normalizer::{CursorEventNormalizer, CursorRouteState};
use super::protocol::*;
use super::transport::{AcpInboundMessage, AcpRpcClient, AcpRpcConnection, AcpRpcFailure};
use crate::events::*;
use crate::models::*;
use crate::{DriverOperationContext, ProviderEventSink};
use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{
        ClientCapabilities, CreateTerminalRequest, CreateTerminalResponse, FileSystemCapabilities,
        HttpHeader, Implementation, InitializeRequest,
        InitializeResponse as OfficialInitializeResponse, KillTerminalRequest,
        KillTerminalResponse, McpServer, McpServerHttp, ReadTextFileRequest, ReadTextFileResponse,
        ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalOutputRequest,
        TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
        WriteTextFileRequest, WriteTextFileResponse,
    },
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{Notify, mpsc, oneshot};
use tokio::task::JoinHandle;

mod callbacks;
mod configuration;
mod router;

#[cfg(test)]
use callbacks::{
    append_terminal_bytes, atomic_write_text, verified_existing_file, verified_write_path,
};
use callbacks::{callback_permission_error, require_acp_session};
#[cfg(test)]
pub use configuration::initialize_session;
pub use configuration::{
    AcpRequestedConfiguration, AcpSessionInitialization, apply_provider_configuration,
    initialize_provider_session,
};
pub use router::{restore_pending, spawn_cursor_router, take_pending};

const MAX_ACP_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ACP_FILE_LINES: u32 = 100_000;
const DEFAULT_ACP_TERMINAL_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_ACP_TERMINAL_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
static NEXT_ACP_FILE_WRITE: AtomicU64 = AtomicU64::new(1);
static NEXT_ACP_TERMINAL: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct AcpStartedSession {
    pub initialize: AcpInitializeResponse,
    pub setup: AcpSessionSetup,
    pub session_id: String,
    pub models: Vec<ProviderModel>,
}

pub struct CursorRouterResources {
    pub client: AcpRpcClient,
    pub inbound: mpsc::Receiver<AcpInboundMessage>,
    pub normalizer: Arc<CursorEventNormalizer>,
    pub route: Arc<Mutex<CursorRouteState>>,
    pub pending: PendingCursorRequests,
    pub sink: Arc<dyn ProviderEventSink>,
    pub expected_shutdown: Arc<AtomicBool>,
    pub terminal_error: Arc<Mutex<Option<ChatError>>>,
    pub flavor: AcpProviderFlavor,
    pub prompt_completions: PendingPromptCompletions,
    pub terminal_callbacks: AcpTerminalCallbacks,
}

#[derive(Clone)]
pub struct AcpTerminalCallbacks {
    workspace: PathBuf,
    session_id: String,
    terminals: Arc<Mutex<HashMap<String, AcpTerminal>>>,
}

#[derive(Clone)]
struct AcpTerminal {
    state: Arc<Mutex<AcpTerminalState>>,
    kill: mpsc::Sender<()>,
    completed: Arc<Notify>,
}

#[derive(Clone, Default)]
struct AcpTerminalState {
    output: Vec<u8>,
    output_limit: usize,
    truncated: bool,
    exit_status: Option<TerminalExitStatus>,
}

impl AcpTerminalCallbacks {
    pub fn new(workspace: PathBuf, session_id: String) -> Self {
        Self {
            workspace,
            session_id,
            terminals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn terminal(&self, id: &str) -> ChatResult<AcpTerminal> {
        self.terminals
            .lock()
            .map_err(|_| driver_state_error())?
            .get(id)
            .cloned()
            .ok_or_else(callback_permission_error)
    }

    fn verify_session(&self, session_id: &str) -> ChatResult<()> {
        require_acp_session(session_id, &self.session_id)
    }

    fn shutdown(&self) {
        if let Ok(mut terminals) = self.terminals.lock() {
            for terminal in terminals.values() {
                let _ = terminal.kill.try_send(());
            }
            terminals.clear();
        }
    }
}

pub type PendingPromptCompletions = Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>;

#[cfg(test)]
mod tests;

pub fn driver_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "ACP provider driver state is unavailable",
        false,
    )
}

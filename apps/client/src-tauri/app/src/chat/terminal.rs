//! Runtime-only pseudoterminal sessions with bounded replay.
//!
//! Terminal output lives only in memory for the session's lifetime and is never persisted. A shell inherits
//! the user's launch environment without multiplexer, port, and packaging variables, and every replay starts by
//! resetting the terminal modes a client may hold from earlier output.

use base64::{Engine as _, engine::general_purpose};
use ganbaru_chat_contracts::models::{
    ChatChannelId, ChatError, ChatErrorCode, ChatReplyThreadId, ChatResult, ProjectWorkingFolderId,
};
use ganbaru_chat_providers::process::inventory as process_inventory;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Condvar, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const CHAT_TERMINAL_STATE_EVENT: &str = "chat://terminal-state";
const MAX_TERMINALS_PER_CHANNEL: usize = 8;
const MAX_TERMINALS_PER_APP: usize = 32;
const MAX_REPLAY_BYTES: usize = 2 * 1024 * 1024;
/// Written before replayed output so a reattached view drops modes left from earlier output: leaves the alternate
/// screen, pops the kitty keyboard stack and clears its flags, resets cursor key, mouse, focus, and bracketed paste
/// modes, shows the cursor, resets modifyOtherKeys, and resets attributes. A program that is still running
/// reapplies its own modes from the replayed bytes.
pub(crate) const TERMINAL_REPLAY_RESET_PREFIX: &str = "\x1b[?1049l\x1b[<100u\x1b[=0;1u\x1b[?1;9;66;67;1000;1002;1003;1004;1005;1006;1015;2004l\x1b[?25h\x1b[>4m\x1b[0m";
const MAX_OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const MAX_INPUT_BYTES: usize = 1024 * 1024;
/// Output bytes a view may leave unacknowledged before live delivery to it pauses.
const TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES: usize = 1024 * 1024;
/// Views attached to one terminal. A reloaded view cannot detach, so the oldest is evicted past this.
const MAX_TERMINAL_OUTPUT_SUBSCRIBERS: usize = 8;
/// How long the reader holds the shell for full views before treating them as lagging and reading on.
const TERMINAL_OUTPUT_STALL_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TERMINAL_SUBSCRIPTION_ID_BYTES: usize = 128;

/// Receives live output for one attached view.
pub(crate) trait TerminalOutputSink: Send {
    /// Returns false once the view can no longer receive output.
    fn deliver(&self, chunk: &ChatTerminalOutputChunk) -> bool;
}

impl TerminalOutputSink for tauri::ipc::Channel<ChatTerminalOutputChunk> {
    fn deliver(&self, chunk: &ChatTerminalOutputChunk) -> bool {
        self.send(chunk.clone()).is_ok()
    }
}

struct OutputSubscriber {
    id: String,
    sink: Box<dyn TerminalOutputSink>,
    unacked_bytes: usize,
    /// Set after a stall timeout, so a view that stopped acknowledging never holds the shell again.
    lagging: bool,
}

/// Per-view flow control. A full view skips live chunks and recovers through replay once it acknowledges.
#[derive(Default)]
struct OutputSubscribers {
    entries: Vec<OutputSubscriber>,
}

impl OutputSubscribers {
    fn attach(&mut self, id: String, sink: Box<dyn TerminalOutputSink>) {
        self.entries.retain(|entry| entry.id != id);
        if self.entries.len() >= MAX_TERMINAL_OUTPUT_SUBSCRIBERS {
            self.entries.remove(0);
        }
        self.entries.push(OutputSubscriber {
            id,
            sink,
            unacked_bytes: 0,
            lagging: false,
        });
    }

    fn detach(&mut self, id: &str) {
        self.entries.retain(|entry| entry.id != id);
    }

    fn acknowledge(&mut self, id: &str, bytes: usize) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        entry.unacked_bytes = entry.unacked_bytes.saturating_sub(bytes);
        if entry.unacked_bytes < TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES {
            entry.lagging = false;
        }
        true
    }

    fn deliver(&mut self, chunk: &ChatTerminalOutputChunk, bytes: usize) {
        self.entries.retain_mut(|entry| {
            if entry.unacked_bytes >= TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES {
                return true;
            }
            if !entry.sink.deliver(chunk) {
                return false;
            }
            entry.unacked_bytes = entry.unacked_bytes.saturating_add(bytes);
            true
        });
    }

    /// The reader waits only while every attentive view is full, so one responsive view keeps output flowing.
    fn should_wait(&self) -> bool {
        let mut attentive = self
            .entries
            .iter()
            .filter(|entry| !entry.lagging)
            .peekable();
        attentive.peek().is_some()
            && attentive.all(|entry| entry.unacked_bytes >= TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES)
    }

    fn mark_full_as_lagging(&mut self) {
        for entry in &mut self.entries {
            if entry.unacked_bytes >= TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES {
                entry.lagging = true;
            }
        }
    }
}

fn validate_subscription_id(subscription_id: &str) -> ChatResult<()> {
    if subscription_id.is_empty()
        || subscription_id.len() > MAX_TERMINAL_SUBSCRIPTION_ID_BYTES
        || !subscription_id.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(ChatError::validation(
            "subscriptionId",
            "Terminal subscription ID is invalid",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalRead {
    pub id: String,
    pub channel_id: ChatChannelId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub name: String,
    pub shell: String,
    pub columns: u16,
    pub rows: u16,
    pub status: ChatTerminalStatus,
    pub last_sequence: u64,
}

/// Lifecycle of one terminal shell. A terminal starts running and settles once, either stopped by Ganbaru or
/// exited on its own with the shell's exit code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ChatTerminalStatus {
    Running,
    Stopped,
    Exited { exit_code: i32 },
}

impl ChatTerminalStatus {
    pub fn is_running(self) -> bool {
        self == Self::Running
    }

    /// The status after the shell exits with `exit_code`. A terminal Ganbaru already stopped stays stopped,
    /// since the exit code then reports the kill rather than the shell.
    fn after_exit(self, exit_code: i32) -> Self {
        match self {
            Self::Running => Self::Exited { exit_code },
            settled => settled,
        }
    }

    /// The status after Ganbaru stops the shell. A shell that already exited keeps its exit code.
    fn after_stop(self) -> Self {
        match self {
            Self::Running => Self::Stopped,
            settled => settled,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalOutputChunk {
    pub terminal_id: String,
    pub sequence: u64,
    pub data_base64: String,
    pub replay: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalSnapshotRead {
    pub terminal: ChatTerminalRead,
    pub scrollback: Vec<ChatTerminalOutputChunk>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalCloseResult {
    pub closed: bool,
    pub confirmation_required: bool,
}

/// Runtime terminal identifier chosen by the view that creates the terminal. Only a canonical lowercase
/// hyphenated UUID is accepted, so the IPC boundary never carries arbitrary text into the registry or events.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatTerminalId(String);

impl ChatTerminalId {
    pub fn new(value: impl Into<String>) -> ChatResult<Self> {
        let value = value.into();
        if !is_canonical_uuid(&value) {
            return Err(ChatError::validation(
                "terminalId",
                "Chat terminal ID is invalid",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for ChatTerminalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        Self::new(value).map_err(|error| serde::de::Error::custom(error.message))
    }
}

/// Accepts the `8-4-4-4-12` lowercase hexadecimal form that `crypto.randomUUID()` produces.
fn is_canonical_uuid(value: &str) -> bool {
    const HYPHENS: [usize; 4] = [8, 13, 18, 23];
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if HYPHENS.contains(&index) {
                byte == b'-'
            } else {
                matches!(byte, b'0'..=b'9' | b'a'..=b'f')
            }
        })
}

pub(crate) struct ChatTerminalCreateInput {
    pub terminal_id: String,
    pub channel_id: ChatChannelId,
    /// Separates terminals opened from a reply thread tab from the channel's own panels.
    pub reply_thread_id: Option<ChatReplyThreadId>,
    pub working_folder_id: ProjectWorkingFolderId,
    pub workspace_path: PathBuf,
    pub columns: u16,
    pub rows: u16,
}

struct TerminalSpawnSpec {
    terminal_id: String,
    channel_id: ChatChannelId,
    reply_thread_id: Option<ChatReplyThreadId>,
    working_folder_id: ProjectWorkingFolderId,
    workspace_path: PathBuf,
    name: String,
    columns: u16,
    rows: u16,
}

#[derive(Debug)]
struct ReplayChunk {
    sequence: u64,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct TerminalState {
    name: String,
    shell: String,
    columns: u16,
    rows: u16,
    status: ChatTerminalStatus,
    sequence: u64,
    replay_bytes: usize,
    replay: VecDeque<ReplayChunk>,
}

struct TerminalSession {
    id: String,
    channel_id: ChatChannelId,
    /// Panel grouping only; authority always comes from the channel.
    reply_thread_id: Option<ChatReplyThreadId>,
    working_folder_id: ProjectWorkingFolderId,
    state: Mutex<TerminalState>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    /// Locked before `state` wherever both are held.
    subscribers: Mutex<OutputSubscribers>,
    output_credit: Condvar,
}

impl TerminalSession {
    fn read(&self) -> ChatResult<ChatTerminalRead> {
        let state = self.state.lock().map_err(|_| terminal_state_error())?;
        Ok(ChatTerminalRead {
            id: self.id.clone(),
            channel_id: self.channel_id.clone(),
            working_folder_id: self.working_folder_id.clone(),
            name: state.name.clone(),
            shell: state.shell.clone(),
            columns: state.columns,
            rows: state.rows,
            status: state.status,
            last_sequence: state.sequence,
        })
    }

    fn snapshot(&self) -> ChatResult<ChatTerminalSnapshotRead> {
        let state = self.state.lock().map_err(|_| terminal_state_error())?;
        let terminal = ChatTerminalRead {
            id: self.id.clone(),
            channel_id: self.channel_id.clone(),
            working_folder_id: self.working_folder_id.clone(),
            name: state.name.clone(),
            shell: state.shell.clone(),
            columns: state.columns,
            rows: state.rows,
            status: state.status,
            last_sequence: state.sequence,
        };
        let scrollback = state
            .replay
            .iter()
            .enumerate()
            .map(|(index, chunk)| {
                let data_base64 = if index == 0 {
                    let mut bytes =
                        Vec::with_capacity(TERMINAL_REPLAY_RESET_PREFIX.len() + chunk.bytes.len());
                    bytes.extend_from_slice(TERMINAL_REPLAY_RESET_PREFIX.as_bytes());
                    bytes.extend_from_slice(&chunk.bytes);
                    general_purpose::STANDARD.encode(bytes)
                } else {
                    general_purpose::STANDARD.encode(&chunk.bytes)
                };
                ChatTerminalOutputChunk {
                    terminal_id: self.id.clone(),
                    sequence: chunk.sequence,
                    data_base64,
                    replay: true,
                }
            })
            .collect();
        Ok(ChatTerminalSnapshotRead {
            terminal,
            scrollback,
        })
    }

    fn record_output(&self, bytes: Vec<u8>) -> ChatResult<Option<ChatTerminalOutputChunk>> {
        let mut state = self.state.lock().map_err(|_| terminal_state_error())?;
        if bytes.is_empty() {
            return Ok(None);
        }
        state.sequence = state.sequence.saturating_add(1);
        let sequence = state.sequence;
        state.replay_bytes = state.replay_bytes.saturating_add(bytes.len());
        state.replay.push_back(ReplayChunk {
            sequence,
            bytes: bytes.clone(),
        });
        while state.replay_bytes > MAX_REPLAY_BYTES {
            let Some(removed) = state.replay.pop_front() else {
                break;
            };
            state.replay_bytes = state.replay_bytes.saturating_sub(removed.bytes.len());
        }
        Ok(Some(ChatTerminalOutputChunk {
            terminal_id: self.id.clone(),
            sequence,
            data_base64: general_purpose::STANDARD.encode(bytes),
            replay: false,
        }))
    }

    /// Takes the snapshot and subscribes under one lock, so no chunk falls between them; a chunk in both is a
    /// duplicate the view drops by sequence.
    fn attach_output(
        &self,
        subscription_id: String,
        sink: Box<dyn TerminalOutputSink>,
    ) -> ChatResult<ChatTerminalSnapshotRead> {
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| terminal_state_error())?;
        let snapshot = self.snapshot()?;
        subscribers.attach(subscription_id, sink);
        Ok(snapshot)
    }

    fn detach_output(&self, subscription_id: &str) -> ChatResult<()> {
        self.subscribers
            .lock()
            .map_err(|_| terminal_state_error())?
            .detach(subscription_id);
        self.output_credit.notify_all();
        Ok(())
    }

    fn acknowledge_output(&self, subscription_id: &str, bytes: usize) -> ChatResult<()> {
        let acknowledged = self
            .subscribers
            .lock()
            .map_err(|_| terminal_state_error())?
            .acknowledge(subscription_id, bytes);
        self.output_credit.notify_all();
        if acknowledged {
            Ok(())
        } else {
            Err(ChatError::new(
                ChatErrorCode::NotFound,
                "Chat terminal subscription was not found",
                true,
            ))
        }
    }

    fn publish_output(&self, chunk: &ChatTerminalOutputChunk, bytes: usize) -> ChatResult<()> {
        self.subscribers
            .lock()
            .map_err(|_| terminal_state_error())?
            .deliver(chunk, bytes);
        Ok(())
    }

    /// Holds the reader, and through the PTY the shell, while every attentive view is full, for at most
    /// [`TERMINAL_OUTPUT_STALL_TIMEOUT`].
    fn await_output_credit(&self) -> ChatResult<()> {
        let deadline = Instant::now() + TERMINAL_OUTPUT_STALL_TIMEOUT;
        let mut subscribers = self
            .subscribers
            .lock()
            .map_err(|_| terminal_state_error())?;
        while subscribers.should_wait() && self.is_running()? {
            let now = Instant::now();
            if now >= deadline {
                subscribers.mark_full_as_lagging();
                break;
            }
            subscribers = self
                .output_credit
                .wait_timeout(subscribers, deadline - now)
                .map_err(|_| terminal_state_error())?
                .0;
        }
        Ok(())
    }

    fn is_running(&self) -> ChatResult<bool> {
        Ok(self
            .state
            .lock()
            .map_err(|_| terminal_state_error())?
            .status
            .is_running())
    }

    /// Records the shell's exit and returns whether the status changed.
    fn mark_exited(&self, exit_code: i32) -> ChatResult<bool> {
        let mut state = self.state.lock().map_err(|_| terminal_state_error())?;
        let next = state.status.after_exit(exit_code);
        let changed = next != state.status;
        state.status = next;
        Ok(changed)
    }

    fn terminate(&self) -> ChatResult<()> {
        if let Some(mut killer) = self
            .killer
            .lock()
            .map_err(|_| terminal_state_error())?
            .take()
        {
            let _ = killer.kill();
        }
        self.writer
            .lock()
            .map_err(|_| terminal_state_error())?
            .take();
        self.master
            .lock()
            .map_err(|_| terminal_state_error())?
            .take();
        if let Ok(mut state) = self.state.lock() {
            state.status = state.status.after_stop();
        }
        self.output_credit.notify_all();
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Ok(killer) = self.killer.get_mut() {
            if let Some(mut killer) = killer.take() {
                let _ = killer.kill();
            }
        }
    }
}

pub struct ChatTerminalRegistry {
    sessions: Mutex<HashMap<String, Arc<TerminalSession>>>,
    /// Cleared once at application exit.
    accepting: AtomicBool,
    /// Set while a vault transition is quiesced, so no new shell starts inside a frozen vault.
    suspended: AtomicBool,
}

impl Default for ChatTerminalRegistry {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            accepting: AtomicBool::new(true),
            suspended: AtomicBool::new(false),
        }
    }
}

impl ChatTerminalRegistry {
    /// Lists every terminal of a channel working folder, whichever panel opened it.
    pub fn list(
        &self,
        channel_id: &ChatChannelId,
        working_folder_id: &ProjectWorkingFolderId,
    ) -> ChatResult<Vec<ChatTerminalRead>> {
        self.list_matching(channel_id, working_folder_id, |_| true)
    }

    /// Lists the terminals one panel owns: the channel panels when `reply_thread_id` is `None`, or the
    /// panels of that reply thread tab otherwise.
    pub fn list_for_panel(
        &self,
        channel_id: &ChatChannelId,
        working_folder_id: &ProjectWorkingFolderId,
        reply_thread_id: Option<&ChatReplyThreadId>,
    ) -> ChatResult<Vec<ChatTerminalRead>> {
        self.list_matching(channel_id, working_folder_id, |session| {
            session.reply_thread_id.as_ref() == reply_thread_id
        })
    }

    fn list_matching(
        &self,
        channel_id: &ChatChannelId,
        working_folder_id: &ProjectWorkingFolderId,
        panel: impl Fn(&TerminalSession) -> bool,
    ) -> ChatResult<Vec<ChatTerminalRead>> {
        let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        let mut result = sessions
            .values()
            .filter(|session| {
                &session.channel_id == channel_id
                    && &session.working_folder_id == working_folder_id
                    && panel(session)
            })
            .map(|session| session.read())
            .collect::<ChatResult<Vec<_>>>()?;
        result.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(result)
    }

    pub fn create(
        &self,
        app: tauri::AppHandle,
        input: ChatTerminalCreateInput,
    ) -> ChatResult<ChatTerminalSnapshotRead> {
        validate_dimensions(input.columns, input.rows)?;
        let mut sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        self.admit_creation()?;
        if sessions.contains_key(&input.terminal_id) {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Chat terminal ID already exists",
                true,
            ));
        }
        if sessions.len() >= MAX_TERMINALS_PER_APP
            || sessions
                .values()
                .filter(|session| session.channel_id == input.channel_id)
                .count()
                >= MAX_TERMINALS_PER_CHANNEL
        {
            return Err(ChatError::new(
                ChatErrorCode::Busy,
                "The Chat terminal limit has been reached",
                true,
            ));
        }
        let terminal_id = input.terminal_id.clone();
        let terminal_name = default_terminal_name(&input.workspace_path);
        let session = spawn_session(
            app,
            TerminalSpawnSpec {
                terminal_id: input.terminal_id,
                channel_id: input.channel_id,
                reply_thread_id: input.reply_thread_id,
                working_folder_id: input.working_folder_id,
                workspace_path: input.workspace_path,
                name: terminal_name,
                columns: input.columns,
                rows: input.rows,
            },
        )?;
        let snapshot = session.snapshot()?;
        sessions.insert(terminal_id, session);
        Ok(snapshot)
    }

    pub fn snapshot(&self, terminal_id: &str) -> ChatResult<ChatTerminalSnapshotRead> {
        self.session(terminal_id)?.snapshot()
    }

    /// Subscribes one view to live output and returns the replay it continues from.
    pub(crate) fn attach(
        &self,
        terminal_id: &str,
        subscription_id: String,
        sink: Box<dyn TerminalOutputSink>,
    ) -> ChatResult<ChatTerminalSnapshotRead> {
        validate_subscription_id(&subscription_id)?;
        self.session(terminal_id)?
            .attach_output(subscription_id, sink)
    }

    /// Detaching from a terminal that already closed succeeds, since its subscriptions closed with it.
    pub fn detach(&self, terminal_id: &str, subscription_id: &str) -> ChatResult<()> {
        let session = {
            let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
            sessions.get(terminal_id).cloned()
        };
        session.map_or(Ok(()), |session| session.detach_output(subscription_id))
    }

    /// Returns delivery credit to one view after it has written `bytes` of live output.
    ///
    /// Acknowledging output of a terminal that already closed succeeds, since nothing waits for
    /// that credit any more.
    pub fn acknowledge(
        &self,
        terminal_id: &str,
        subscription_id: &str,
        bytes: u64,
    ) -> ChatResult<()> {
        let bytes = usize::try_from(bytes).unwrap_or(usize::MAX);
        let session = {
            let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
            sessions.get(terminal_id).cloned()
        };
        session.map_or(Ok(()), |session| {
            session.acknowledge_output(subscription_id, bytes)
        })
    }

    pub fn require_scope(
        &self,
        terminal_id: &str,
        channel_id: &ChatChannelId,
        working_folder_id: &ProjectWorkingFolderId,
    ) -> ChatResult<()> {
        let session = self.session(terminal_id)?;
        if &session.channel_id != channel_id || &session.working_folder_id != working_folder_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Chat terminal belongs to another channel or working folder",
                false,
            ));
        }
        Ok(())
    }

    pub fn input(&self, terminal_id: &str, text: &str) -> ChatResult<()> {
        if text.len() > MAX_INPUT_BYTES || text.contains('\0') {
            return Err(ChatError::validation(
                "input",
                "Terminal input exceeds the safety limit",
            ));
        }
        validate_paste_framing(text)?;
        let session = self.session(terminal_id)?;
        if !session.read()?.status.is_running() {
            return Err(ChatError::new(
                ChatErrorCode::InvalidStateTransition,
                "Chat terminal is not running",
                true,
            ));
        }
        let mut writer = session.writer.lock().map_err(|_| terminal_state_error())?;
        let writer = writer.as_mut().ok_or_else(terminal_not_running_error)?;
        writer
            .write_all(text.as_bytes())
            .map_err(terminal_io_error)?;
        writer.flush().map_err(terminal_io_error)
    }

    pub fn resize(&self, terminal_id: &str, columns: u16, rows: u16) -> ChatResult<()> {
        validate_dimensions(columns, rows)?;
        let session = self.session(terminal_id)?;
        session
            .master
            .lock()
            .map_err(|_| terminal_state_error())?
            .as_ref()
            .ok_or_else(terminal_not_running_error)?
            .resize(PtySize {
                rows,
                cols: columns,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(terminal_io_error)?;
        let mut state = session.state.lock().map_err(|_| terminal_state_error())?;
        state.columns = columns;
        state.rows = rows;
        Ok(())
    }

    pub fn close(&self, terminal_id: &str, confirmed: bool) -> ChatResult<ChatTerminalCloseResult> {
        let session = self.session(terminal_id)?;
        if session.read()?.status.is_running() && !confirmed {
            return Ok(ChatTerminalCloseResult {
                closed: false,
                confirmation_required: true,
            });
        }
        session.terminate()?;
        self.sessions
            .lock()
            .map_err(|_| terminal_state_error())?
            .remove(terminal_id);
        Ok(ChatTerminalCloseResult {
            closed: true,
            confirmation_required: false,
        })
    }

    pub fn shutdown_all(&self) -> ChatResult<()> {
        self.accepting.store(false, Ordering::Release);
        let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        for session in sessions.values() {
            session.terminate()?;
        }
        Ok(())
    }

    /// Rejects new terminals until [`Self::resume_creation`] runs; running terminals continue.
    pub fn suspend_creation(&self) -> ChatResult<()> {
        // Holding the sessions lock orders this against a create that already passed admission.
        let _sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        self.suspended.store(true, Ordering::Release);
        Ok(())
    }

    /// Admits new terminals again after a vault transition finishes.
    pub fn resume_creation(&self) {
        self.suspended.store(false, Ordering::Release);
    }

    /// Callers hold the sessions lock so admission and insertion stay atomic.
    fn admit_creation(&self) -> ChatResult<()> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(ChatError::new(
                ChatErrorCode::InvalidStateTransition,
                "Chat terminals are shutting down",
                true,
            ));
        }
        if self.suspended.load(Ordering::Acquire) {
            return Err(ChatError::new(
                ChatErrorCode::Busy,
                "Chat terminals are paused while the vault changes",
                true,
            ));
        }
        Ok(())
    }

    pub fn live_count(&self) -> ChatResult<usize> {
        let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        sessions.values().try_fold(0usize, |count, session| {
            Ok(count + usize::from(session.read()?.status.is_running()))
        })
    }

    pub fn stop_all(&self) -> ChatResult<u64> {
        let sessions = {
            let mut registry = self.sessions.lock().map_err(|_| terminal_state_error())?;
            registry
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        let count = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
        for session in sessions {
            session.terminate()?;
        }
        Ok(count)
    }

    pub fn shutdown_workspace(&self, working_folder_id: &ProjectWorkingFolderId) -> ChatResult<()> {
        self.shutdown_matching(|session| &session.working_folder_id == working_folder_id)
    }

    fn shutdown_matching(&self, matches: impl Fn(&TerminalSession) -> bool) -> ChatResult<()> {
        let mut sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        let terminal_ids = sessions
            .values()
            .filter(|session| matches(session))
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        for terminal_id in terminal_ids {
            if let Some(session) = sessions.remove(&terminal_id) {
                session.terminate()?;
            }
        }
        Ok(())
    }

    fn session(&self, terminal_id: &str) -> ChatResult<Arc<TerminalSession>> {
        self.sessions
            .lock()
            .map_err(|_| terminal_state_error())?
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| {
                ChatError::new(ChatErrorCode::NotFound, "Chat terminal was not found", true)
            })
    }
}

fn spawn_session(
    app: tauri::AppHandle,
    spec: TerminalSpawnSpec,
) -> ChatResult<Arc<TerminalSession>> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: spec.rows,
            cols: spec.columns,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(terminal_io_error)?;
    let command = shell_command(&spec.workspace_path);
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(terminal_io_error)?;
    let shell = default_shell_label();
    let registration = child.process_id().map(|pid| {
        process_inventory::register_process(
            pid,
            Path::new(&shell),
            None,
            process_inventory::ChatProcessPurpose::Terminal,
            None,
        )
    });
    let killer = child.clone_killer();
    let reader = pair.master.try_clone_reader().map_err(terminal_io_error)?;
    let writer = pair.master.take_writer().map_err(terminal_io_error)?;
    let session = Arc::new(TerminalSession {
        id: spec.terminal_id,
        channel_id: spec.channel_id,
        reply_thread_id: spec.reply_thread_id,
        working_folder_id: spec.working_folder_id,
        state: Mutex::new(TerminalState {
            name: spec.name,
            shell,
            columns: spec.columns,
            rows: spec.rows,
            status: ChatTerminalStatus::Running,
            sequence: 0,
            replay_bytes: 0,
            replay: VecDeque::new(),
        }),
        master: Mutex::new(Some(pair.master)),
        writer: Mutex::new(Some(writer)),
        killer: Mutex::new(Some(killer)),
        subscribers: Mutex::new(OutputSubscribers::default()),
        output_credit: Condvar::new(),
    });
    spawn_reader(Arc::clone(&session), reader)?;
    let wait_session = Arc::clone(&session);
    if let Err(error) = std::thread::Builder::new()
        .name("chat-terminal-wait".to_string())
        .spawn(move || {
            let exit_code = child
                .wait()
                .map(|status| status.exit_code() as i32)
                .unwrap_or(1);
            drop(registration);
            if wait_session.mark_exited(exit_code).unwrap_or(false) {
                if let Ok(read) = wait_session.read() {
                    let _ = app.emit(CHAT_TERMINAL_STATE_EVENT, read);
                }
            }
        })
    {
        session.terminate()?;
        return Err(terminal_io_error(error));
    }
    Ok(session)
}

/// Live output reaches only the views attached to this terminal, each through its own channel.
fn spawn_reader(session: Arc<TerminalSession>, mut reader: Box<dyn Read + Send>) -> ChatResult<()> {
    std::thread::Builder::new()
        .name("chat-terminal-output".to_string())
        .spawn(move || {
            let mut buffer = vec![0_u8; MAX_OUTPUT_CHUNK_BYTES];
            loop {
                if session.await_output_credit().is_err() {
                    break;
                }
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let bytes = buffer[..count].to_vec();
                        if let Ok(Some(event)) = session.record_output(bytes)
                            && session.publish_output(&event, count).is_err()
                        {
                            break;
                        }
                    }
                }
            }
        })
        .map_err(terminal_io_error)?;
    Ok(())
}

const BRACKETED_PASTE_START: &str = "\x1b[200~";
const BRACKETED_PASTE_END: &str = "\x1b[201~";

/// Rejects input that closes bracketed paste anywhere but at the end of one framed paste. Keys never produce the
/// paste markers (F9 is `ESC[20~`, modified keys use the `ESC[2;5~` form), and xterm.js sends each paste as one
/// input call, so a marker elsewhere means pasted text tried to leave bracketed mode and run as typed input.
fn validate_paste_framing(text: &str) -> ChatResult<()> {
    if !text.contains(BRACKETED_PASTE_END) && !text.contains(BRACKETED_PASTE_START) {
        return Ok(());
    }
    let framed = text
        .strip_prefix(BRACKETED_PASTE_START)
        .and_then(|rest| rest.strip_suffix(BRACKETED_PASTE_END))
        .is_some_and(|body| {
            !body.contains(BRACKETED_PASTE_START) && !body.contains(BRACKETED_PASTE_END)
        });
    if framed {
        return Ok(());
    }
    Err(ChatError::validation(
        "input",
        "Terminal input closes bracketed paste before the paste ends",
    ))
}

fn validate_dimensions(columns: u16, rows: u16) -> ChatResult<()> {
    if !(20..=1_000).contains(&columns) || !(2..=500).contains(&rows) {
        return Err(ChatError::validation(
            "dimensions",
            "Terminal dimensions are outside the supported range",
        ));
    }
    Ok(())
}

/// Terminal type variables for the xterm.js emulator. Shells and tools read them to enable colored prompts and
/// output, and the app's own environment lacks them when it starts outside a terminal. Windows consoles do not use
/// `TERM`, so only the color depth is advertised there.
#[cfg(not(windows))]
const TERMINAL_ENVIRONMENT: &[(&str, &str)] =
    &[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")];
#[cfg(windows)]
const TERMINAL_ENVIRONMENT: &[(&str, &str)] = &[("COLORTERM", "truecolor")];

/// Environment the app was launched with, before dependencies adjust the process environment.
static LAUNCH_ENVIRONMENT: OnceLock<Vec<(OsString, OsString)>> = OnceLock::new();

/// Variables the AppImage runtime sets to describe the mounted image and how it was started.
#[cfg(target_os = "linux")]
const APPIMAGE_RUNTIME_VARIABLES: &[&str] = &["APPIMAGE", "APPDIR", "ARGV0", "OWD"];

/// A change applied to the inherited process environment of a terminal shell.
#[derive(Debug, PartialEq, Eq)]
enum EnvironmentEdit {
    Set(OsString, OsString),
    Remove(OsString),
}

/// Records the environment the app was launched with. Call once at desktop startup, before plugins and the
/// webview start, so terminals inherit the user's environment instead of values dependencies set later.
pub(crate) fn capture_launch_environment() {
    let _ = LAUNCH_ENVIRONMENT.set(std::env::vars_os().collect());
}

/// Variables that describe a terminal multiplexer session or a port the launcher chose. A Chat terminal is not
/// inside the user's multiplexer, and an inherited `PORT` misleads development servers started in it.
const TERMINAL_ENVIRONMENT_BLOCKLIST: &[&str] = &["TMUX", "TMUX_PANE", "STY", "WINDOW", "PORT"];

/// Name prefixes of multiplexer variables dropped from the terminal environment.
const TERMINAL_ENVIRONMENT_BLOCKLIST_PREFIXES: &[&str] = &["ZELLIJ"];

/// The fish variable that toggles optional features at startup. Other shells ignore it.
const FISH_FEATURES_VARIABLE: &str = "fish_features";

/// Feature fish 4.1 and later read to start without waiting for a terminal query reply.
const FISH_NO_QUERY_TERM_FEATURE: &str = "no-query-term";

/// Feature name the user sets, with or without the `no-` prefix, to choose the query behavior themselves.
const FISH_QUERY_TERM_FEATURE: &str = "query-term";

/// Returns the environment a terminal shell should inherit: the launch environment without multiplexer and port
/// variables or the variables the AppImage runtime and its bundled library hooks inject, with fish told not to
/// wait for a terminal query reply unless the user chose that behavior.
fn terminal_environment(launch: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let launch = without_blocklisted_variables(launch);
    #[cfg(target_os = "linux")]
    let launch = without_appimage_variables(launch);
    with_fish_query_term_disabled(launch)
}

/// Compares an environment key with a variable name, ignoring case on Windows where keys are case-insensitive.
fn environment_key_is(key: &OsStr, name: &str) -> bool {
    #[cfg(windows)]
    return key.eq_ignore_ascii_case(name);
    #[cfg(not(windows))]
    return key == name;
}

/// Reports whether an environment key starts with `prefix`, ignoring case on Windows.
fn environment_key_starts_with(key: &OsStr, prefix: &str) -> bool {
    let key = key.as_encoded_bytes();
    let prefix = prefix.as_bytes();
    if key.len() < prefix.len() {
        return false;
    }
    #[cfg(windows)]
    return key[..prefix.len()].eq_ignore_ascii_case(prefix);
    #[cfg(not(windows))]
    return &key[..prefix.len()] == prefix;
}

/// Drops the multiplexer and port variables named by the terminal blocklists.
fn without_blocklisted_variables(
    environment: Vec<(OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    environment
        .into_iter()
        .filter(|(key, _)| {
            !TERMINAL_ENVIRONMENT_BLOCKLIST
                .iter()
                .any(|name| environment_key_is(key, name))
                && !TERMINAL_ENVIRONMENT_BLOCKLIST_PREFIXES
                    .iter()
                    .any(|prefix| environment_key_starts_with(key, prefix))
        })
        .collect()
}

/// Adds the fish `no-query-term` feature, appending it to inherited comma or space separated features, unless
/// they already name `query-term` in either form.
fn with_fish_query_term_disabled(
    mut environment: Vec<(OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let existing = environment
        .iter_mut()
        .find(|(key, _)| environment_key_is(key, FISH_FEATURES_VARIABLE));
    match existing {
        Some((_, value)) => {
            let names_query_term = value
                .to_string_lossy()
                .split(|separator: char| separator == ',' || separator.is_whitespace())
                .map(str::trim)
                .any(|feature| {
                    feature.strip_prefix("no-").unwrap_or(feature) == FISH_QUERY_TERM_FEATURE
                });
            if names_query_term {
                return environment;
            }
            if value.to_string_lossy().trim().is_empty() {
                *value = OsString::from(FISH_NO_QUERY_TERM_FEATURE);
            } else {
                value.push(",");
                value.push(FISH_NO_QUERY_TERM_FEATURE);
            }
        }
        None => environment.push((
            OsString::from(FISH_FEATURES_VARIABLE),
            OsString::from(FISH_NO_QUERY_TERM_FEATURE),
        )),
    }
    environment
}

/// Removes the AppImage runtime variables and every path entry inside the mounted image. Environments that were
/// not started from an AppImage are returned unchanged.
#[cfg(target_os = "linux")]
fn without_appimage_variables(environment: Vec<(OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let lookup = |name: &str| {
        environment
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    };
    if lookup("APPIMAGE").is_none() {
        return environment;
    }
    let Some(appdir) = lookup("APPDIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.parent().is_some())
    else {
        return environment;
    };
    environment
        .into_iter()
        .filter(|(key, _)| !APPIMAGE_RUNTIME_VARIABLES.iter().any(|name| key == name))
        .filter_map(|(key, value)| {
            without_entries_inside(&value, &appdir).map(|value| (key, value))
        })
        .collect()
}

/// Removes the colon-separated entries of `value` that lie inside `directory`. Returns `None` when no non-empty
/// entry remains, because an empty entry in a search path means the current directory.
#[cfg(target_os = "linux")]
fn without_entries_inside(value: &OsStr, directory: &Path) -> Option<OsString> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let is_inside = |entry: &[u8]| Path::new(OsStr::from_bytes(entry)).starts_with(directory);
    let entries: Vec<&[u8]> = value.as_bytes().split(|byte| *byte == b':').collect();
    if !entries.iter().any(|entry| is_inside(entry)) {
        return Some(value.to_os_string());
    }
    let kept: Vec<&[u8]> = entries
        .into_iter()
        .filter(|entry| !is_inside(entry))
        .collect();
    if kept.iter().all(|entry| entry.is_empty()) {
        return None;
    }
    Some(OsString::from_vec(kept.join(&b':')))
}

/// Returns the edits that turn the `current` process environment into the `desired` one.
fn environment_edits(
    desired: &[(OsString, OsString)],
    current: &[(OsString, OsString)],
) -> Vec<EnvironmentEdit> {
    let desired_values: HashMap<&OsStr, &OsStr> = desired
        .iter()
        .map(|(key, value)| (key.as_os_str(), value.as_os_str()))
        .collect();
    let current_values: HashMap<&OsStr, &OsStr> = current
        .iter()
        .map(|(key, value)| (key.as_os_str(), value.as_os_str()))
        .collect();
    let mut edits = Vec::new();
    for (key, value) in current {
        match desired_values.get(key.as_os_str()) {
            None => edits.push(EnvironmentEdit::Remove(key.clone())),
            Some(desired) if *desired != value.as_os_str() => {
                edits.push(EnvironmentEdit::Set(key.clone(), desired.to_os_string()))
            }
            Some(_) => {}
        }
    }
    for (key, value) in desired {
        if !current_values.contains_key(key.as_os_str()) {
            edits.push(EnvironmentEdit::Set(key.clone(), value.clone()));
        }
    }
    edits
}

/// Applies the edits that restore the terminal environment. An edit is skipped when the command no longer holds
/// the process value, because the platform base environment refreshed it, as Windows does from the registry.
fn apply_terminal_environment(command: &mut CommandBuilder) {
    let current: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let launch = LAUNCH_ENVIRONMENT
        .get()
        .cloned()
        .unwrap_or_else(|| current.clone());
    let current_values: HashMap<&OsStr, &OsStr> = current
        .iter()
        .map(|(key, value)| (key.as_os_str(), value.as_os_str()))
        .collect();
    for edit in environment_edits(&terminal_environment(launch), &current) {
        let key = match &edit {
            EnvironmentEdit::Set(key, _) | EnvironmentEdit::Remove(key) => key,
        };
        if command.get_env(key) != current_values.get(key.as_os_str()).copied() {
            continue;
        }
        match edit {
            EnvironmentEdit::Set(key, value) => command.env(key, value),
            EnvironmentEdit::Remove(key) => command.env_remove(key),
        }
    }
}

/// Builds the default shell command for a terminal in the workspace with the user's launch environment,
/// advertising the emulator's capabilities.
fn shell_command(workspace_path: &Path) -> CommandBuilder {
    #[cfg(windows)]
    let mut command = match absolute_windows_shell(
        std::env::var_os("ComSpec").as_deref(),
        std::env::var_os("SystemRoot").as_deref(),
    ) {
        Some(shell) => CommandBuilder::new(shell),
        None => CommandBuilder::new_default_prog(),
    };
    #[cfg(not(windows))]
    let mut command = CommandBuilder::new_default_prog();
    command.cwd(workspace_path);
    apply_terminal_environment(&mut command);
    for (key, value) in TERMINAL_ENVIRONMENT {
        command.env(key, value);
    }
    command
}

/// Resolves the Windows shell by absolute path, because `CreateProcessW` completes a bare application name from
/// the app's current directory. Uses `ComSpec` when it names an absolute path, otherwise `cmd.exe` in the system
/// directory under `SystemRoot`.
#[cfg(any(windows, test))]
fn absolute_windows_shell(comspec: Option<&OsStr>, system_root: Option<&OsStr>) -> Option<PathBuf> {
    comspec
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            system_root
                .map(|root| PathBuf::from(root).join("System32").join("cmd.exe"))
                .filter(|path| path.is_absolute())
        })
}

pub(crate) fn default_shell_label() -> String {
    #[cfg(windows)]
    let key = "ComSpec";
    #[cfg(not(windows))]
    let key = "SHELL";
    std::env::var(key).unwrap_or_else(|_| "Default shell".to_string())
}

fn default_terminal_name(workspace_path: &Path) -> String {
    let user = first_environment_value(&["USER", "USERNAME"]).unwrap_or_else(|| "user".to_string());
    let host = system_hostname()
        .or_else(|| first_environment_value(&["HOSTNAME", "COMPUTERNAME"]))
        .unwrap_or_else(|| user.clone());
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    format_terminal_name(&user, &host, workspace_path, home.as_deref())
}

fn first_environment_value(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = std::env::var(key).ok()?;
        normalized_identity(&value)
    })
}

fn normalized_identity(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty() && !trimmed.chars().any(char::is_control)).then(|| trimmed.to_string())
}

#[cfg(unix)]
#[allow(
    unsafe_code,
    reason = "Retained unsafe boundary; see docs/data/security/unsafe-rust.md."
)]
fn system_hostname() -> Option<String> {
    let mut buffer = [0_u8; 256];
    // SAFETY: `buffer` remains alive and writable for the full length passed to
    // libc. `gethostname` writes at most that many bytes, and the return value is
    // checked before any bytes are decoded.
    let result =
        unsafe { libc::gethostname(buffer.as_mut_ptr().cast::<libc::c_char>(), buffer.len()) };
    hostname_from_call(result, &buffer)
}

#[cfg(unix)]
fn hostname_from_call(result: libc::c_int, buffer: &[u8]) -> Option<String> {
    (result == 0)
        .then(|| hostname_from_buffer(buffer))
        .flatten()
}

#[cfg(unix)]
fn hostname_from_buffer(buffer: &[u8]) -> Option<String> {
    let length = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    normalized_identity(&String::from_utf8_lossy(&buffer[..length]))
}

#[cfg(not(unix))]
fn system_hostname() -> Option<String> {
    let output = std::process::Command::new("hostname").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
        .and_then(|value| normalized_identity(&value))
}

fn format_terminal_name(
    user: &str,
    host: &str,
    workspace_path: &Path,
    home: Option<&Path>,
) -> String {
    let path = home
        .and_then(|home_path| workspace_path.strip_prefix(home_path).ok())
        .map(|relative| {
            if relative.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~{}{}", std::path::MAIN_SEPARATOR, relative.display())
            }
        })
        .unwrap_or_else(|| workspace_path.display().to_string());
    format!("{user}@{host}: {path}")
}

fn terminal_not_running_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::InvalidStateTransition,
        "Chat terminal is not running",
        true,
    )
}

fn terminal_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat terminal state is unavailable",
        true,
    )
}

fn terminal_io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat terminal operation failed",
        true,
    )
}

#[cfg(test)]
mod tests;

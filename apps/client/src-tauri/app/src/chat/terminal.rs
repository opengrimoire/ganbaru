//! Runtime-only pseudoterminal sessions with bounded replay.

use base64::{Engine as _, engine::general_purpose};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId,
};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const CHAT_TERMINAL_STATE_EVENT: &str = "chat://terminal-state";
const MAX_TERMINALS_PER_THREAD: usize = 8;
const MAX_TERMINALS_PER_APP: usize = 32;
const MAX_REPLAY_BYTES: usize = 2 * 1024 * 1024;
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
    pub thread_id: ChatThreadId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub name: String,
    pub shell: String,
    pub columns: u16,
    pub rows: u16,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub generation: u64,
    pub last_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTerminalOutputChunk {
    pub terminal_id: String,
    pub generation: u64,
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

pub(crate) struct ChatTerminalCreateInput {
    pub terminal_id: String,
    pub thread_id: ChatThreadId,
    pub working_folder_id: ProjectWorkingFolderId,
    /// Worktree execution environment the shell starts in, if any.
    pub environment_id: Option<String>,
    pub workspace_path: PathBuf,
    pub columns: u16,
    pub rows: u16,
}

struct TerminalSpawnSpec {
    terminal_id: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
    environment_id: Option<String>,
    workspace_path: PathBuf,
    name: String,
    columns: u16,
    rows: u16,
    generation: u64,
}

#[derive(Debug)]
struct ReplayChunk {
    generation: u64,
    sequence: u64,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct TerminalState {
    name: String,
    shell: String,
    columns: u16,
    rows: u16,
    running: bool,
    exit_code: Option<i32>,
    generation: u64,
    sequence: u64,
    replay_bytes: usize,
    replay: VecDeque<ReplayChunk>,
}

struct TerminalSession {
    id: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
    environment_id: Option<String>,
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
            thread_id: self.thread_id.clone(),
            working_folder_id: self.working_folder_id.clone(),
            name: state.name.clone(),
            shell: state.shell.clone(),
            columns: state.columns,
            rows: state.rows,
            running: state.running,
            exit_code: state.exit_code,
            generation: state.generation,
            last_sequence: state.sequence,
        })
    }

    fn snapshot(&self) -> ChatResult<ChatTerminalSnapshotRead> {
        let state = self.state.lock().map_err(|_| terminal_state_error())?;
        let terminal = ChatTerminalRead {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            working_folder_id: self.working_folder_id.clone(),
            name: state.name.clone(),
            shell: state.shell.clone(),
            columns: state.columns,
            rows: state.rows,
            running: state.running,
            exit_code: state.exit_code,
            generation: state.generation,
            last_sequence: state.sequence,
        };
        let scrollback = state
            .replay
            .iter()
            .map(|chunk| ChatTerminalOutputChunk {
                terminal_id: self.id.clone(),
                generation: chunk.generation,
                sequence: chunk.sequence,
                data_base64: general_purpose::STANDARD.encode(&chunk.bytes),
                replay: true,
            })
            .collect();
        Ok(ChatTerminalSnapshotRead {
            terminal,
            scrollback,
        })
    }

    fn record_output(
        &self,
        generation: u64,
        bytes: Vec<u8>,
    ) -> ChatResult<Option<ChatTerminalOutputChunk>> {
        let mut state = self.state.lock().map_err(|_| terminal_state_error())?;
        if generation != state.generation || bytes.is_empty() {
            return Ok(None);
        }
        state.sequence = state.sequence.saturating_add(1);
        let sequence = state.sequence;
        state.replay_bytes = state.replay_bytes.saturating_add(bytes.len());
        state.replay.push_back(ReplayChunk {
            generation,
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
            generation,
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
            .running)
    }

    fn mark_exited(&self, generation: u64, exit_code: i32) -> ChatResult<bool> {
        let mut state = self.state.lock().map_err(|_| terminal_state_error())?;
        if state.generation != generation {
            return Ok(false);
        }
        state.running = false;
        state.exit_code = Some(exit_code);
        Ok(true)
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
            state.running = false;
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
    pub fn list(
        &self,
        thread_id: &ChatThreadId,
        working_folder_id: &ProjectWorkingFolderId,
    ) -> ChatResult<Vec<ChatTerminalRead>> {
        let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        let mut result = sessions
            .values()
            .filter(|session| {
                &session.thread_id == thread_id && &session.working_folder_id == working_folder_id
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
                .filter(|session| session.thread_id == input.thread_id)
                .count()
                >= MAX_TERMINALS_PER_THREAD
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
                thread_id: input.thread_id,
                working_folder_id: input.working_folder_id,
                environment_id: input.environment_id,
                workspace_path: input.workspace_path,
                name: terminal_name,
                columns: input.columns,
                rows: input.rows,
                generation: 1,
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
        thread_id: &ChatThreadId,
        working_folder_id: &ProjectWorkingFolderId,
    ) -> ChatResult<()> {
        let session = self.session(terminal_id)?;
        if &session.thread_id != thread_id || &session.working_folder_id != working_folder_id {
            return Err(ChatError::new(
                ChatErrorCode::Permission,
                "Chat terminal belongs to another thread or workspace",
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
        if !session.read()?.running {
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
        if session.read()?.running && !confirmed {
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
            Ok(count + usize::from(session.read()?.running))
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

    /// Stops the terminals whose shell started in one worktree execution environment.
    pub fn shutdown_environment(&self, environment_id: &str) -> ChatResult<()> {
        self.shutdown_matching(|session| session.environment_id.as_deref() == Some(environment_id))
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

    /// Returns the threads that currently own at least one terminal.
    pub fn thread_ids(&self) -> ChatResult<std::collections::BTreeSet<ChatThreadId>> {
        let sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        Ok(sessions
            .values()
            .map(|session| session.thread_id.clone())
            .collect())
    }

    pub fn shutdown_thread(&self, thread_id: &ChatThreadId) -> ChatResult<()> {
        self.shutdown_matching(|session| &session.thread_id == thread_id)
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
    let killer = child.clone_killer();
    let reader = pair.master.try_clone_reader().map_err(terminal_io_error)?;
    let writer = pair.master.take_writer().map_err(terminal_io_error)?;
    let session = Arc::new(TerminalSession {
        id: spec.terminal_id,
        thread_id: spec.thread_id,
        working_folder_id: spec.working_folder_id,
        environment_id: spec.environment_id,
        state: Mutex::new(TerminalState {
            name: spec.name,
            shell: default_shell_label(),
            columns: spec.columns,
            rows: spec.rows,
            running: true,
            exit_code: None,
            generation: spec.generation,
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
    spawn_reader(Arc::clone(&session), reader, spec.generation)?;
    let wait_session = Arc::clone(&session);
    if let Err(error) = std::thread::Builder::new()
        .name("chat-terminal-wait".to_string())
        .spawn(move || {
            let exit_code = child
                .wait()
                .map(|status| status.exit_code() as i32)
                .unwrap_or(1);
            if wait_session
                .mark_exited(spec.generation, exit_code)
                .unwrap_or(false)
            {
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
fn spawn_reader(
    session: Arc<TerminalSession>,
    mut reader: Box<dyn Read + Send>,
    generation: u64,
) -> ChatResult<()> {
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
                        if let Ok(Some(event)) = session.record_output(generation, bytes)
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

/// Builds the default shell command for a terminal in the workspace, advertising the emulator's capabilities.
fn shell_command(workspace_path: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new_default_prog();
    command.cwd(workspace_path);
    for (key, value) in TERMINAL_ENVIRONMENT {
        command.env(key, value);
    }
    command
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
mod tests {
    use super::*;

    fn session(running: bool) -> Arc<TerminalSession> {
        session_in("terminal:test", None, running)
    }

    fn session_in(id: &str, environment_id: Option<&str>, running: bool) -> Arc<TerminalSession> {
        Arc::new(TerminalSession {
            id: id.to_string(),
            thread_id: ChatThreadId::new("thread:test").expect("thread ID should be valid"),
            working_folder_id: ProjectWorkingFolderId::new("workspace:test")
                .expect("workspace ID should be valid"),
            environment_id: environment_id.map(str::to_string),
            state: Mutex::new(TerminalState {
                name: "Terminal 1".to_string(),
                shell: "test-shell".to_string(),
                columns: 80,
                rows: 24,
                running,
                exit_code: None,
                generation: 1,
                sequence: 0,
                replay_bytes: 0,
                replay: VecDeque::new(),
            }),
            master: Mutex::new(None),
            writer: Mutex::new(None),
            killer: Mutex::new(None),
            subscribers: Mutex::new(OutputSubscribers::default()),
            output_credit: Condvar::new(),
        })
    }

    #[test]
    fn replay_is_sequenced_generation_scoped_and_byte_bounded() {
        let session = session(true);
        assert!(
            session
                .record_output(0, b"stale".to_vec())
                .expect("stale output should be handled")
                .is_none()
        );
        for byte in [1_u8, 2, 3] {
            session
                .record_output(1, vec![byte; MAX_REPLAY_BYTES / 2])
                .expect("output should record")
                .expect("current generation should emit");
        }
        let snapshot = session.snapshot().expect("snapshot should read");
        assert_eq!(snapshot.terminal.last_sequence, 3);
        assert_eq!(snapshot.scrollback.len(), 2);
        assert_eq!(snapshot.scrollback[0].sequence, 2);
        assert!(
            session
                .state
                .lock()
                .expect("terminal state should lock")
                .replay_bytes
                <= MAX_REPLAY_BYTES
        );
    }

    #[test]
    fn running_terminal_close_requires_confirmation_and_removes_exact_session() {
        let registry = ChatTerminalRegistry::default();
        registry
            .sessions
            .lock()
            .expect("registry should lock")
            .insert("terminal:test".to_string(), session(true));
        let first = registry
            .close("terminal:test", false)
            .expect("close should respond");
        assert!(first.confirmation_required);
        assert!(!first.closed);
        let confirmed = registry
            .close("terminal:test", true)
            .expect("confirmed close should work");
        assert!(confirmed.closed);
        assert!(registry.snapshot("terminal:test").is_err());
    }

    #[test]
    fn removing_a_worktree_stops_only_the_terminals_started_in_it() {
        let registry = ChatTerminalRegistry::default();
        let started = [
            ("terminal:removed", Some("environment:removed")),
            ("terminal:other", Some("environment:other")),
            ("terminal:folder", None),
        ];
        let sessions = started
            .iter()
            .map(|(id, environment_id)| session_in(id, *environment_id, true))
            .collect::<Vec<_>>();
        {
            let mut registered = registry.sessions.lock().expect("registry should lock");
            for session in &sessions {
                registered.insert(session.id.clone(), Arc::clone(session));
            }
        }

        registry
            .shutdown_environment("environment:removed")
            .expect("shutdown should work");

        assert!(registry.snapshot("terminal:removed").is_err());
        assert!(!sessions[0].read().expect("session should read").running);
        for id in ["terminal:other", "terminal:folder"] {
            assert!(
                registry
                    .snapshot(id)
                    .expect("terminal should remain")
                    .terminal
                    .running
            );
        }
    }

    struct RecordingSink {
        delivered: Arc<Mutex<Vec<u64>>>,
        open: bool,
    }

    impl TerminalOutputSink for RecordingSink {
        fn deliver(&self, chunk: &ChatTerminalOutputChunk) -> bool {
            if self.open {
                self.delivered
                    .lock()
                    .expect("sink should lock")
                    .push(chunk.sequence);
            }
            self.open
        }
    }

    fn recording_sink(open: bool) -> (Box<dyn TerminalOutputSink>, Arc<Mutex<Vec<u64>>>) {
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink {
            delivered: Arc::clone(&delivered),
            open,
        };
        (Box::new(sink), delivered)
    }

    fn output_chunk(sequence: u64) -> ChatTerminalOutputChunk {
        ChatTerminalOutputChunk {
            terminal_id: "terminal:test".to_string(),
            generation: 1,
            sequence,
            data_base64: String::new(),
            replay: false,
        }
    }

    #[test]
    fn output_reaches_only_attached_views_and_pauses_at_the_watermark() {
        let session = session(true);
        session
            .record_output(1, b"before".to_vec())
            .expect("output should record");
        let (sink, delivered) = recording_sink(true);
        let snapshot = session
            .attach_output("view:a".to_string(), sink)
            .expect("view should attach");
        assert_eq!(snapshot.scrollback.len(), 1);

        let half = TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES / 2;
        for sequence in 2..=4 {
            session
                .publish_output(&output_chunk(sequence), half)
                .expect("output should publish");
        }
        assert_eq!(
            *delivered.lock().expect("sink should lock"),
            vec![2, 3],
            "a full view skips live chunks instead of queueing them"
        );
        assert!(
            session
                .subscribers
                .lock()
                .expect("subscribers should lock")
                .should_wait()
        );

        session
            .acknowledge_output("view:a", half)
            .expect("acknowledgement should apply");
        session
            .publish_output(&output_chunk(5), half)
            .expect("output should publish");
        assert_eq!(*delivered.lock().expect("sink should lock"), vec![2, 3, 5]);
        assert_eq!(
            session
                .acknowledge_output("view:unknown", half)
                .expect_err("unknown subscriptions are rejected")
                .code,
            ChatErrorCode::NotFound
        );

        session.detach_output("view:a").expect("view should detach");
        session
            .publish_output(&output_chunk(6), 1)
            .expect("output should publish");
        assert_eq!(delivered.lock().expect("sink should lock").len(), 3);
    }

    #[test]
    fn stalled_views_stop_holding_the_shell_and_closed_views_are_dropped() {
        let mut subscribers = OutputSubscribers::default();
        let (stalled, _) = recording_sink(true);
        subscribers.attach("view:stalled".to_string(), stalled);
        subscribers.deliver(&output_chunk(1), TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES);
        assert!(subscribers.should_wait());
        subscribers.mark_full_as_lagging();
        assert!(
            !subscribers.should_wait(),
            "a lagging view never holds the reader again"
        );

        let (responsive, responsive_chunks) = recording_sink(true);
        subscribers.attach("view:responsive".to_string(), responsive);
        assert!(!subscribers.should_wait());
        subscribers.deliver(&output_chunk(2), 1);
        assert_eq!(
            *responsive_chunks.lock().expect("sink should lock"),
            vec![2]
        );
        assert!(subscribers.acknowledge("view:stalled", TERMINAL_OUTPUT_HIGH_WATERMARK_BYTES));
        assert!(subscribers.entries.iter().all(|entry| !entry.lagging));

        let (closed, _) = recording_sink(false);
        subscribers.attach("view:closed".to_string(), closed);
        subscribers.deliver(&output_chunk(3), 1);
        assert!(
            subscribers
                .entries
                .iter()
                .all(|entry| entry.id != "view:closed")
        );

        for index in 0..MAX_TERMINAL_OUTPUT_SUBSCRIBERS {
            let (sink, _) = recording_sink(true);
            subscribers.attach(format!("view:{index}"), sink);
        }
        assert_eq!(subscribers.entries.len(), MAX_TERMINAL_OUTPUT_SUBSCRIBERS);
        assert!(
            subscribers
                .entries
                .iter()
                .all(|entry| entry.id != "view:stalled")
        );
        assert!(validate_subscription_id("view:1").is_ok());
        assert!(validate_subscription_id("").is_err());
        assert!(validate_subscription_id("view one").is_err());
        assert!(
            validate_subscription_id(&"a".repeat(MAX_TERMINAL_SUBSCRIPTION_ID_BYTES + 1)).is_err()
        );
    }

    #[test]
    fn vault_suspension_blocks_creation_and_counts_only_running_terminals() {
        let registry = ChatTerminalRegistry::default();
        {
            let mut sessions = registry.sessions.lock().expect("registry should lock");
            sessions.insert("terminal:running".to_string(), session(true));
            sessions.insert("terminal:exited".to_string(), session(false));
        }
        assert_eq!(registry.live_count().expect("count should read"), 1);
        assert!(registry.admit_creation().is_ok());

        registry
            .suspend_creation()
            .expect("suspension should apply");
        let paused = registry
            .admit_creation()
            .expect_err("a suspended registry must reject new terminals");
        assert_eq!(paused.code, ChatErrorCode::Busy);
        assert_eq!(registry.live_count().expect("count should read"), 1);

        registry.resume_creation();
        assert!(registry.admit_creation().is_ok());
        registry.shutdown_all().expect("shutdown should apply");
        registry.resume_creation();
        assert!(registry.admit_creation().is_err());
    }

    #[test]
    fn terminal_validation_rejects_unsafe_dimensions_and_input() {
        assert!(validate_dimensions(19, 24).is_err());
        assert!(validate_dimensions(80, 1).is_err());
        assert!(validate_dimensions(80, 24).is_ok());
        let registry = ChatTerminalRegistry::default();
        registry
            .sessions
            .lock()
            .expect("registry should lock")
            .insert("terminal:test".to_string(), session(true));
        assert!(registry.input("terminal:test", "bad\0input").is_err());
        assert!(
            registry
                .input("terminal:test", "pasted\x1b[201~rm -rf ~\r")
                .is_err()
        );
        assert!(
            registry
                .input("terminal:test", "\x1b[200~a\x1b[201~b\x1b[201~")
                .is_err()
        );
        assert!(
            registry
                .input("terminal:test", "\x1b[200~a\x1b[200~b\x1b[201~")
                .is_err()
        );
        for keys in ["\x1b[20~", "\x1b[21~", "\x1b[2~", "\x1b[2;5~", "\x1b[20;2~"] {
            assert!(
                validate_paste_framing(keys).is_ok(),
                "{keys:?} is a key sequence"
            );
        }
        assert!(validate_paste_framing("\x1b[200~echo one\recho two\x1b[201~").is_ok());
    }

    #[test]
    fn shell_command_starts_in_the_workspace_and_advertises_a_color_terminal() {
        let workspace = PathBuf::from("workspace");
        let command = shell_command(&workspace);
        assert_eq!(command.get_cwd().map(PathBuf::from), Some(workspace));
        assert_eq!(command.get_env("COLORTERM"), Some("truecolor".as_ref()));
        #[cfg(not(windows))]
        assert_eq!(command.get_env("TERM"), Some("xterm-256color".as_ref()));
    }

    #[test]
    fn terminal_name_uses_the_local_identity_and_home_relative_workspace_path() {
        let home = PathBuf::from("home").join("alice");
        let workspace = home.join("Documents").join("ganbaru");
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(
            format_terminal_name("alice", "workstation", &workspace, Some(&home)),
            format!("alice@workstation: ~{separator}Documents{separator}ganbaru")
        );
    }

    #[test]
    fn terminal_identity_rejects_empty_and_control_character_values() {
        assert_eq!(normalized_identity("vic\ntor"), None);
        assert_eq!(normalized_identity("   "), None);
        assert_eq!(normalized_identity(" alice \n"), Some("alice".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn hostname_decoding_accepts_terminated_and_full_buffers() {
        assert_eq!(
            hostname_from_buffer(b"workstation\0ignored"),
            Some("workstation".to_string())
        );
        assert_eq!(
            hostname_from_buffer(b"full-buffer-name"),
            Some("full-buffer-name".to_string())
        );
        assert_eq!(hostname_from_buffer(b"\0"), None);
        assert_eq!(
            hostname_from_buffer(&[b'w', b'o', b'r', b'k', 0xff, 0]),
            Some("work�".to_string())
        );
        assert_eq!(hostname_from_buffer(b"invalid\nname\0"), None);
        assert_eq!(hostname_from_call(-1, b"ignored\0"), None);
    }
}

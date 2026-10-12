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
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::sync_channel,
};
use tauri::Emitter;

pub const CHAT_TERMINAL_OUTPUT_EVENT: &str = "chat://terminal-output";
pub const CHAT_TERMINAL_STATE_EVENT: &str = "chat://terminal-state";
const MAX_TERMINALS_PER_THREAD: usize = 8;
const MAX_TERMINALS_PER_APP: usize = 32;
const MAX_REPLAY_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_CHUNK_BYTES: usize = 16 * 1024;
const MAX_INPUT_BYTES: usize = 1024 * 1024;

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
    pub workspace_path: PathBuf,
    pub columns: u16,
    pub rows: u16,
}

struct TerminalSpawnSpec {
    terminal_id: String,
    thread_id: ChatThreadId,
    working_folder_id: ProjectWorkingFolderId,
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
    state: Mutex<TerminalState>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
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
    accepting: AtomicBool,
}

impl Default for ChatTerminalRegistry {
    fn default() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            accepting: AtomicBool::new(true),
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
        if !self.accepting.load(Ordering::Acquire) {
            return Err(ChatError::new(
                ChatErrorCode::InvalidStateTransition,
                "Chat terminals are shutting down",
                true,
            ));
        }
        let mut sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
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
        let mut sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        let terminal_ids = sessions
            .values()
            .filter(|session| &session.working_folder_id == working_folder_id)
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        for terminal_id in terminal_ids {
            if let Some(session) = sessions.remove(&terminal_id) {
                session.terminate()?;
            }
        }
        Ok(())
    }

    pub fn shutdown_thread(&self, thread_id: &ChatThreadId) -> ChatResult<()> {
        let mut sessions = self.sessions.lock().map_err(|_| terminal_state_error())?;
        let terminal_ids = sessions
            .values()
            .filter(|session| &session.thread_id == thread_id)
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
    let mut command = CommandBuilder::new_default_prog();
    command.cwd(&spec.workspace_path);
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
    });
    spawn_reader(app.clone(), Arc::clone(&session), reader, spec.generation)?;
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

fn spawn_reader(
    app: tauri::AppHandle,
    session: Arc<TerminalSession>,
    mut reader: Box<dyn Read + Send>,
    generation: u64,
) -> ChatResult<()> {
    let (output_sender, output_receiver) = sync_channel::<ChatTerminalOutputChunk>(64);
    std::thread::Builder::new()
        .name("chat-terminal-events".to_string())
        .spawn(move || {
            while let Ok(event) = output_receiver.recv() {
                let _ = app.emit(CHAT_TERMINAL_OUTPUT_EVENT, event);
            }
        })
        .map_err(terminal_io_error)?;
    std::thread::Builder::new()
        .name("chat-terminal-output".to_string())
        .spawn(move || {
            let mut buffer = vec![0_u8; MAX_OUTPUT_CHUNK_BYTES];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let bytes = buffer[..count].to_vec();
                        if let Ok(Some(event)) = session.record_output(generation, bytes) {
                            if output_sender.send(event).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        })
        .map_err(terminal_io_error)?;
    Ok(())
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

fn default_shell_label() -> String {
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
        Arc::new(TerminalSession {
            id: "terminal:test".to_string(),
            thread_id: ChatThreadId::new("thread:test").expect("thread ID should be valid"),
            working_folder_id: ProjectWorkingFolderId::new("workspace:test")
                .expect("workspace ID should be valid"),
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
    }

    #[test]
    fn terminal_name_uses_the_local_identity_and_home_relative_workspace_path() {
        let home = PathBuf::from("home").join("alice");
        let workspace = home.join("Documents").join("ganbaru-ai");
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(
            format_terminal_name("alice", "workstation", &workspace, Some(&home)),
            format!("alice@workstation: ~{separator}Documents{separator}ganbaru-ai")
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

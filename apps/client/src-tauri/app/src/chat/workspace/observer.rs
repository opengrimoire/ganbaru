//! Bounded, execution-environment scoped workspace change observation.

use super::WorkingFolderAuthorizationOperation;
use super::files::require_workspace_for;
use crate::db;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId,
};
use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::Emitter;

mod changes;
mod git;

use changes::{WorkspaceChangeAccumulator, safe_workspace_relative_path};
use git::{discover_git_metadata_roots, git_watch_paths};

pub const CHAT_WORKSPACE_CHANGE_EVENT: &str = "chat://workspace-change";
const EVENT_CHANNEL_CAPACITY: usize = 1_024;
const MAX_BATCH_PATHS: usize = 512;
const MAX_WATCHED_DIRECTORIES: usize = 4_096;
const QUIET_DEBOUNCE: Duration = Duration::from_millis(100);
const MAX_BATCH_LATENCY: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatWorkspaceObserverMode {
    Native,
    Polling,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWorkspaceObserverStatusRead {
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub generation: u64,
    pub mode: ChatWorkspaceObserverMode,
    pub degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWorkspaceRename {
    pub previous_relative_path: String,
    pub relative_path: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatWorkspaceChangeBatch {
    pub working_folder_id: ProjectWorkingFolderId,
    pub execution_environment_id: Option<String>,
    pub generation: u64,
    pub relative_paths: Vec<String>,
    pub affected_parent_directories: Vec<String>,
    pub renames: Vec<ChatWorkspaceRename>,
    pub git_metadata_changed: bool,
    pub overflowed: bool,
    pub degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObserverScope {
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
    root: PathBuf,
}

enum ActiveWatcher {
    Native(RecommendedWatcher),
    Polling(PollWatcher),
}

impl ActiveWatcher {
    fn watch(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        match self {
            Self::Native(watcher) => watcher.watch(path, mode),
            Self::Polling(watcher) => watcher.watch(path, mode),
        }
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        match self {
            Self::Native(watcher) => watcher.unwatch(path),
            Self::Polling(watcher) => watcher.unwatch(path),
        }
    }
}

enum ObserverMessage {
    Event(Event),
    Error(String),
    Internal {
        relative_paths: Vec<String>,
        git_metadata_changed: bool,
    },
    Stop,
}

struct ObserverSession {
    scope: ObserverScope,
    generation: u64,
    mode: ChatWorkspaceObserverMode,
    degraded_reason: Option<String>,
    sender: SyncSender<ObserverMessage>,
    overflow_signal: Arc<AtomicBool>,
    watcher: Option<Arc<Mutex<Option<ActiveWatcher>>>>,
    worker: Option<JoinHandle<()>>,
    stopped: Arc<AtomicBool>,
    runtime_unavailable: Arc<AtomicBool>,
    runtime_degraded_reason: Arc<Mutex<Option<String>>>,
}

struct ObserverWorkerContext {
    app: tauri::AppHandle,
    scope: ObserverScope,
    generation: u64,
    receiver: Receiver<ObserverMessage>,
    overflow_signal: Arc<AtomicBool>,
    current_generation: Arc<AtomicU64>,
    git_roots: Vec<PathBuf>,
    watcher: Arc<Mutex<Option<ActiveWatcher>>>,
    watched_directories: BTreeSet<PathBuf>,
    stopped: Arc<AtomicBool>,
    runtime_unavailable: Arc<AtomicBool>,
    runtime_degraded_reason: Arc<Mutex<Option<String>>>,
}

impl ObserverSession {
    fn status(&self) -> ChatWorkspaceObserverStatusRead {
        let runtime_unavailable = self.runtime_unavailable.load(Ordering::Acquire);
        let runtime_reason = self
            .runtime_degraded_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone());
        ChatWorkspaceObserverStatusRead {
            working_folder_id: self.scope.working_folder_id.clone(),
            execution_environment_id: self.scope.execution_environment_id.clone(),
            generation: self.generation,
            mode: if runtime_unavailable {
                ChatWorkspaceObserverMode::Unavailable
            } else {
                self.mode
            },
            degraded_reason: runtime_reason.or_else(|| self.degraded_reason.clone()),
        }
    }

    fn stop(mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.sender.try_send(ObserverMessage::Stop);
        if let Some(watcher) = self.watcher.take() {
            if let Ok(mut watcher) = watcher.lock() {
                watcher.take();
            }
        }
        drop(self.worker.take());
    }
}

#[derive(Default)]
struct ObserverRegistryInner {
    session: Option<ObserverSession>,
}

pub struct ChatWorkspaceObserverRegistry {
    inner: Mutex<ObserverRegistryInner>,
    next_generation: AtomicU64,
    current_generation: Arc<AtomicU64>,
}

impl Default for ChatWorkspaceObserverRegistry {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ObserverRegistryInner::default()),
            next_generation: AtomicU64::new(0),
            current_generation: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl ChatWorkspaceObserverRegistry {
    fn start(
        &self,
        app: tauri::AppHandle,
        scope: ObserverScope,
    ) -> ChatResult<ChatWorkspaceObserverStatusRead> {
        let mut inner = self.inner.lock().map_err(|_| observer_state_error())?;
        if let Some(session) = inner.session.as_ref() {
            if session.scope == scope {
                let status = session.status();
                if status.mode != ChatWorkspaceObserverMode::Unavailable {
                    return Ok(status);
                }
            }
        }

        self.current_generation.store(0, Ordering::Release);
        if let Some(previous) = inner.session.take() {
            previous.stop();
        }

        let generation = self
            .next_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        self.current_generation.store(generation, Ordering::Release);
        let session = create_session(app, scope, generation, Arc::clone(&self.current_generation));
        let status = session.status();
        if session.mode != ChatWorkspaceObserverMode::Unavailable {
            inner.session = Some(session);
        } else {
            self.current_generation.store(0, Ordering::Release);
        }
        Ok(status)
    }

    pub fn stop(&self, generation: u64) -> ChatResult<bool> {
        let mut inner = self.inner.lock().map_err(|_| observer_state_error())?;
        if inner
            .session
            .as_ref()
            .is_none_or(|session| session.generation != generation)
        {
            return Ok(false);
        }
        self.current_generation.store(0, Ordering::Release);
        if let Some(session) = inner.session.take() {
            session.stop();
        }
        Ok(true)
    }

    pub fn invalidate_paths(
        &self,
        working_folder_id: &ProjectWorkingFolderId,
        execution_environment_id: Option<&str>,
        relative_paths: Vec<String>,
        git_metadata_changed: bool,
    ) {
        let Ok(inner) = self.inner.lock() else {
            return;
        };
        let Some(session) = inner.session.as_ref() else {
            return;
        };
        if &session.scope.working_folder_id != working_folder_id
            || session.scope.execution_environment_id.as_deref() != execution_environment_id
        {
            return;
        }
        if matches!(
            session.sender.try_send(ObserverMessage::Internal {
                relative_paths,
                git_metadata_changed,
            }),
            Err(TrySendError::Full(_))
        ) {
            session.overflow_signal.store(true, Ordering::Release);
        }
    }
}

#[tauri::command]
pub async fn chat_watch_workspace(
    app: tauri::AppHandle,
    observers: tauri::State<'_, ChatWorkspaceObserverRegistry>,
    db_url: String,
    working_folder_id: ProjectWorkingFolderId,
    execution_environment_id: Option<String>,
) -> ChatResult<ChatWorkspaceObserverStatusRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    let authorized = require_workspace_for(
        &app,
        &pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::FileRead,
    )
    .await?;
    let authorized = crate::chat::execution_environment::resolve_environment_workspace(
        &app,
        &pool,
        authorized,
        execution_environment_id.as_deref(),
    )
    .await?;
    observers.start(
        app,
        ObserverScope {
            working_folder_id,
            execution_environment_id,
            root: authorized.canonical_path,
        },
    )
}

#[tauri::command]
pub fn chat_unwatch_workspace(
    observers: tauri::State<'_, ChatWorkspaceObserverRegistry>,
    generation: u64,
) -> ChatResult<bool> {
    observers.stop(generation)
}

fn create_session(
    app: tauri::AppHandle,
    scope: ObserverScope,
    generation: u64,
    current_generation: Arc<AtomicU64>,
) -> ObserverSession {
    let (sender, receiver) = mpsc::sync_channel(EVENT_CHANNEL_CAPACITY);
    let overflowed = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let runtime_unavailable = Arc::new(AtomicBool::new(false));
    let runtime_degraded_reason = Arc::new(Mutex::new(None));
    let watched_directories = match discover_workspace_watch_directories(&scope.root) {
        Ok(paths) => paths,
        Err(reason) => {
            return unavailable_session(
                scope,
                generation,
                sender,
                stopped,
                runtime_unavailable,
                runtime_degraded_reason,
                reason,
            );
        }
    };

    let native_watcher = create_native_watcher(sender.clone(), Arc::clone(&overflowed))
        .ok()
        .and_then(|mut watcher| {
            watch_all_directories(&mut watcher, &watched_directories)
                .ok()
                .map(|()| ActiveWatcher::Native(watcher))
        });
    let (watcher, actual_mode, mut degraded_reason) = if let Some(watcher) = native_watcher {
        (watcher, ChatWorkspaceObserverMode::Native, None)
    } else {
        let polling_watcher = create_poll_watcher(sender.clone(), Arc::clone(&overflowed))
            .ok()
            .and_then(|mut watcher| {
                watch_all_directories(&mut watcher, &watched_directories)
                    .ok()
                    .map(|()| ActiveWatcher::Polling(watcher))
            });
        let Some(watcher) = polling_watcher else {
            return unavailable_session(
                scope,
                generation,
                sender,
                stopped,
                runtime_unavailable,
                runtime_degraded_reason,
                "watch_unavailable".to_string(),
            );
        };
        (
            watcher,
            ChatWorkspaceObserverMode::Polling,
            Some("native_unavailable".to_string()),
        )
    };

    let git_roots = discover_git_metadata_roots(&scope.root);
    let mut git_watch_partial = false;
    let mut watcher = watcher;
    for (path, recursive) in git_watch_paths(&git_roots) {
        if watcher.watch(&path, recursive).is_ok() {
            continue;
        }
        git_watch_partial = true;
    }
    if git_watch_partial && degraded_reason.is_none() {
        degraded_reason = Some("git_watch_partial".to_string());
    }

    let watcher = Arc::new(Mutex::new(Some(watcher)));
    let worker_scope = scope.clone();
    let worker_watcher = Arc::clone(&watcher);
    let worker_stopped = Arc::clone(&stopped);
    let worker_runtime_unavailable = Arc::clone(&runtime_unavailable);
    let worker_runtime_reason = Arc::clone(&runtime_degraded_reason);
    let worker_overflowed = Arc::clone(&overflowed);
    let worker = thread::Builder::new()
        .name(format!("chat-workspace-watch-{generation}"))
        .spawn(move || {
            run_observer_worker(ObserverWorkerContext {
                app,
                scope: worker_scope,
                generation,
                receiver,
                overflow_signal: worker_overflowed,
                current_generation,
                git_roots,
                watcher: worker_watcher,
                watched_directories,
                stopped: worker_stopped,
                runtime_unavailable: worker_runtime_unavailable,
                runtime_degraded_reason: worker_runtime_reason,
            });
        })
        .ok();
    if worker.is_none() {
        if let Ok(mut active) = watcher.lock() {
            active.take();
        }
        return unavailable_session(
            scope,
            generation,
            sender,
            stopped,
            runtime_unavailable,
            runtime_degraded_reason,
            "worker_unavailable".to_string(),
        );
    }

    ObserverSession {
        scope,
        generation,
        mode: actual_mode,
        degraded_reason,
        sender,
        overflow_signal: overflowed,
        watcher: Some(watcher),
        worker,
        stopped,
        runtime_unavailable,
        runtime_degraded_reason,
    }
}

fn unavailable_session(
    scope: ObserverScope,
    generation: u64,
    sender: SyncSender<ObserverMessage>,
    stopped: Arc<AtomicBool>,
    runtime_unavailable: Arc<AtomicBool>,
    runtime_degraded_reason: Arc<Mutex<Option<String>>>,
    reason: String,
) -> ObserverSession {
    ObserverSession {
        scope,
        generation,
        mode: ChatWorkspaceObserverMode::Unavailable,
        degraded_reason: Some(reason),
        sender,
        overflow_signal: Arc::new(AtomicBool::new(false)),
        watcher: None,
        worker: None,
        stopped,
        runtime_unavailable,
        runtime_degraded_reason,
    }
}

fn discover_workspace_watch_directories(root: &Path) -> Result<BTreeSet<PathBuf>, String> {
    discover_workspace_watch_directories_from(root, root)
}

fn discover_workspace_watch_directories_from(
    root: &Path,
    starting_directory: &Path,
) -> Result<BTreeSet<PathBuf>, String> {
    let mut discovered = BTreeSet::new();
    let mut pending = vec![starting_directory.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if discovered.contains(&directory) {
            continue;
        }
        if discovered.len() >= MAX_WATCHED_DIRECTORIES {
            return Err("watch_scope_too_large".to_string());
        }
        discovered.insert(directory.clone());
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) if directory == starting_directory => {
                return Err("watch_unavailable".to_string());
            }
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let Some(relative) = safe_workspace_relative_path(root, &path) else {
                continue;
            };
            if super::files::observer_excluded(&relative) {
                continue;
            }
            if discovered.len().saturating_add(pending.len()) >= MAX_WATCHED_DIRECTORIES {
                return Err("watch_scope_too_large".to_string());
            }
            pending.push(path);
        }
    }
    Ok(discovered)
}

fn watch_all_directories<W: Watcher>(
    watcher: &mut W,
    directories: &BTreeSet<PathBuf>,
) -> notify::Result<()> {
    for directory in directories {
        watcher.watch(directory, RecursiveMode::NonRecursive)?;
    }
    Ok(())
}

fn create_native_watcher(
    sender: SyncSender<ObserverMessage>,
    overflowed: Arc<AtomicBool>,
) -> notify::Result<RecommendedWatcher> {
    RecommendedWatcher::new(
        move |result| forward_notify_result(result, &sender, &overflowed),
        Config::default().with_follow_symlinks(false),
    )
}

fn create_poll_watcher(
    sender: SyncSender<ObserverMessage>,
    overflowed: Arc<AtomicBool>,
) -> notify::Result<PollWatcher> {
    PollWatcher::new(
        move |result| forward_notify_result(result, &sender, &overflowed),
        Config::default()
            .with_follow_symlinks(false)
            .with_poll_interval(POLL_INTERVAL),
    )
}

fn forward_notify_result(
    result: notify::Result<Event>,
    sender: &SyncSender<ObserverMessage>,
    overflowed: &AtomicBool,
) {
    let message = match result {
        Ok(event) => ObserverMessage::Event(event),
        Err(error) => ObserverMessage::Error(match error.kind {
            notify::ErrorKind::MaxFilesWatch => "watch_limit".to_string(),
            _ => "watch_error".to_string(),
        }),
    };
    if matches!(sender.try_send(message), Err(TrySendError::Full(_))) {
        overflowed.store(true, Ordering::Release);
    }
}

fn run_observer_worker(context: ObserverWorkerContext) {
    let ObserverWorkerContext {
        app,
        scope,
        generation,
        receiver,
        overflow_signal,
        current_generation,
        git_roots,
        watcher,
        mut watched_directories,
        stopped,
        runtime_unavailable,
        runtime_degraded_reason,
    } = context;
    loop {
        if stopped.load(Ordering::Acquire) {
            return;
        }
        let first = match receiver.recv_timeout(QUIET_DEBOUNCE) {
            Ok(ObserverMessage::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => continue,
            Ok(message) => message,
        };
        let started = Instant::now();
        let mut last_event = started;
        let mut accumulator =
            WorkspaceChangeAccumulator::new(scope.clone(), generation, git_roots.clone());
        push_worker_message(
            &mut accumulator,
            first,
            &scope.root,
            &watcher,
            &mut watched_directories,
        );

        loop {
            if stopped.load(Ordering::Acquire) {
                return;
            }
            if overflow_signal.swap(false, Ordering::AcqRel) {
                accumulator.overflowed = true;
            }
            let quiet_remaining = QUIET_DEBOUNCE.saturating_sub(last_event.elapsed());
            let hard_remaining = MAX_BATCH_LATENCY.saturating_sub(started.elapsed());
            if quiet_remaining.is_zero() || hard_remaining.is_zero() {
                break;
            }
            match receiver.recv_timeout(quiet_remaining.min(hard_remaining)) {
                Ok(ObserverMessage::Stop) => return,
                Ok(message) => {
                    push_worker_message(
                        &mut accumulator,
                        message,
                        &scope.root,
                        &watcher,
                        &mut watched_directories,
                    );
                    last_event = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        let fatal_reason = accumulator.degraded_reason.clone();
        let batch = accumulator.finish();
        if let Some(reason) = fatal_reason {
            if let Ok(mut runtime_reason) = runtime_degraded_reason.lock() {
                *runtime_reason = Some(reason);
            }
            runtime_unavailable.store(true, Ordering::Release);
            if let Ok(mut active) = watcher.lock() {
                active.take();
            }
        }
        if current_generation.load(Ordering::Acquire) == generation {
            if let Some(batch) = batch {
                let _ = app.emit(CHAT_WORKSPACE_CHANGE_EVENT, batch);
            }
        }
        if runtime_unavailable.load(Ordering::Acquire) {
            return;
        }
    }
}

fn push_worker_message(
    accumulator: &mut WorkspaceChangeAccumulator,
    message: ObserverMessage,
    root: &Path,
    watcher: &Arc<Mutex<Option<ActiveWatcher>>>,
    watched_directories: &mut BTreeSet<PathBuf>,
) {
    let watch_error = match &message {
        ObserverMessage::Event(event) => {
            maintain_workspace_watch_set(root, event, watcher, watched_directories).err()
        }
        _ => None,
    };
    accumulator.push(message);
    if let Some(reason) = watch_error {
        accumulator.push(ObserverMessage::Error(reason));
    }
}

fn maintain_workspace_watch_set(
    root: &Path,
    event: &Event,
    watcher: &Arc<Mutex<Option<ActiveWatcher>>>,
    watched_directories: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    if matches!(
        event.kind,
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(_))
    ) {
        let removed = watched_directories
            .iter()
            .filter(|watched| watched.as_path() != root && !watched.is_dir())
            .cloned()
            .collect::<Vec<_>>();
        remove_directory_watches(watcher, watched_directories, removed);
    }
    for path in event.paths.iter().filter(|path| path.starts_with(root)) {
        if !path.exists() {
            let removed = watched_directories
                .iter()
                .filter(|watched| {
                    let watched = watched.as_path();
                    watched != root && (watched == path.as_path() || watched.starts_with(path))
                })
                .cloned()
                .collect::<Vec<_>>();
            if !removed.is_empty() {
                remove_directory_watches(watcher, watched_directories, removed);
            }
            continue;
        }
        if watched_directories.contains(path) {
            continue;
        }
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let removed = watched_directories
                    .iter()
                    .filter(|watched| {
                        let watched = watched.as_path();
                        watched != root && (watched == path.as_path() || watched.starts_with(path))
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                remove_directory_watches(watcher, watched_directories, removed);
                continue;
            }
            Err(_) => return Err("watch_error".to_string()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        if safe_workspace_relative_path(root, path).is_none() {
            continue;
        }
        let discovered = discover_workspace_watch_directories_from(root, path)?;
        let additions = discovered
            .into_iter()
            .filter(|directory| !watched_directories.contains(directory))
            .collect::<Vec<_>>();
        if watched_directories.len().saturating_add(additions.len()) > MAX_WATCHED_DIRECTORIES {
            return Err("watch_scope_too_large".to_string());
        }
        let mut active = watcher.lock().map_err(|_| "watch_error".to_string())?;
        let active = active
            .as_mut()
            .ok_or_else(|| "watch_unavailable".to_string())?;
        for directory in &additions {
            active
                .watch(directory, RecursiveMode::NonRecursive)
                .map_err(|_| "watch_error".to_string())?;
        }
        for directory in additions {
            watched_directories.insert(directory);
        }
    }
    Ok(())
}

fn remove_directory_watches(
    watcher: &Arc<Mutex<Option<ActiveWatcher>>>,
    watched_directories: &mut BTreeSet<PathBuf>,
    removed: Vec<PathBuf>,
) {
    if let Ok(mut active) = watcher.lock() {
        if let Some(active) = active.as_mut() {
            for path in &removed {
                let _ = active.unwatch(path);
            }
        }
    }
    for path in removed {
        watched_directories.remove(&path);
    }
}

async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

fn observer_state_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat workspace observer state is unavailable",
        true,
    )
}

#[cfg(test)]
mod tests;

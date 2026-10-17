//! The sync service of the active vault: it imports carried operations, keeps the local writer
//! current, seals local captures after commits, and exchanges with the hub. On the coordinator
//! it applies what linked devices push and revokes the writers of unlinked devices.

use super::access::vault_connection_hook;
use super::carry_forward;
use super::client::{ClientSession, Exchange, ForkPoint, PeerIds};
#[cfg(desktop)]
use super::hub::{HubVault, OpenHubVault, SyncHub};
use super::recovery::{RecoveryChoice, RecoveryRequest};
use super::status::{SyncRole, SyncState, SyncStatusView, emit_applied, emit_status};
use super::transport::PairedTransport;
use super::writer::{
    ActiveWriter, SuccessorPlan, SyncFiles, WriterCheck, WriterKeyStore, check_writer,
    create_writer, read_stored_writer, remove_retired_keys,
};
use crate::db::DatabaseState;
use crate::vault::handoff::pairing::PairingManager;
use ganbaru_db::{DatabaseAccessMode, DatabasePoolRegistry};
use ganbaru_sync::{Engine, SealReport, SpaceContext, SyncError, WriterState, local};
use ganbaru_sync_contracts::VersionVector;
use ganbaru_sync_contracts::op::RevokeReason;
use ganbaru_sync_replica::now_ms;
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{Notify, oneshot, watch};

/// Quiet time after a commit before sealing, so bursts of edits seal together.
const COMMIT_DEBOUNCE: Duration = Duration::from_millis(750);
/// Interval of passes without commits, which retries work that failed or waited.
const FALLBACK_INTERVAL: Duration = Duration::from_secs(60);
/// Delays before retrying failed exchanges, the last repeating.
const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
    Duration::from_secs(300),
];
/// How long a stopping service may finish its pass before it is aborted.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound on apply rounds in one pass; each round is one bounded transaction.
const MAX_APPLY_ROUNDS: usize = 10_000;
/// Fork recoveries one pass may run before it reports an error.
const MAX_FORK_RECOVERIES: usize = 3;
/// Directory under the app config directory that holds writer records and carry bundles.
const SYNC_DIRECTORY: &str = "sync";

/// Managed state of the sync service.
pub(crate) struct SyncRuntime {
    /// Signaled after every commit on a writable vault connection.
    commits: Arc<Notify>,
    /// Signaled to run a pass now.
    now: Arc<Notify>,
    /// Bumped after each pass, so hub waits recheck the log.
    changes: watch::Sender<u64>,
    /// A pause change waiting for the writer record.
    pause_request: Arc<Mutex<Option<bool>>>,
    /// Recovery actions waiting for the next pass.
    recovery: Arc<Mutex<Vec<RecoveryRequest>>>,
    status: Arc<Mutex<SyncStatusView>>,
    task: Mutex<Option<ServiceTask>>,
}

impl Default for SyncRuntime {
    fn default() -> Self {
        Self {
            commits: Arc::default(),
            now: Arc::default(),
            changes: watch::Sender::new(0),
            pause_request: Arc::default(),
            recovery: Arc::default(),
            status: Arc::default(),
            task: Mutex::default(),
        }
    }
}

impl SyncRuntime {
    /// The latest status of the active vault.
    pub(crate) fn status(&self) -> Result<SyncStatusView, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| "sync status lock is unavailable".to_string())
    }

    /// Runs a pass now instead of waiting for a commit or the fallback interval.
    pub(crate) fn sync_now(&self) {
        self.now.notify_one();
    }

    /// Pauses or resumes exchanges on this device. The writer record keeps the choice.
    pub(crate) fn set_paused(&self, paused: bool) -> Result<(), String> {
        *self
            .pause_request
            .lock()
            .map_err(|_| "sync pause lock is unavailable".to_string())? = Some(paused);
        self.now.notify_one();
        Ok(())
    }

    /// Queues a recovery action for the service and wakes it. The receiver gets the id of a
    /// restored row.
    pub(crate) fn request_recovery(
        &self,
        choice: RecoveryChoice,
    ) -> Result<oneshot::Receiver<Result<Option<String>, String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.recovery
            .lock()
            .map_err(|_| "sync recovery lock is unavailable".to_string())?
            .push(RecoveryRequest { choice, reply });
        self.now.notify_one();
        Ok(receiver)
    }

    fn take_recovery_requests(&self) -> Vec<RecoveryRequest> {
        self.recovery
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }
}

struct ServiceTask {
    stop: Arc<Notify>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

/// Installs the vault connection hook and starts the service. Runs once at startup, before
/// the frontend opens the vault.
pub(crate) fn setup<R: Runtime>(app: &AppHandle<R>) {
    let commits = app.state::<SyncRuntime>().commits.clone();
    app.state::<DatabaseState>()
        .set_connection_hook(Some(vault_connection_hook(commits)));
    if let Err(error) = start(app) {
        eprintln!("start sync service: {error}");
    }
}

/// The hub the coordinator listener routes sync requests to, serving the active vault.
#[cfg(desktop)]
pub(crate) fn sync_hub<R: Runtime>(app: &AppHandle<R>) -> SyncHub {
    let opener = app.clone();
    let open: OpenHubVault = Arc::new(move || {
        let app = opener.clone();
        Box::pin(async move { open_hub_vault(&app).await })
    });
    SyncHub::new(open, app.state::<SyncRuntime>().changes.clone())
}

#[cfg(desktop)]
async fn open_hub_vault<R: Runtime>(app: &AppHandle<R>) -> Result<Option<HubVault>, String> {
    if !sync_enabled(app)? {
        return Ok(None);
    }
    let Some(pool) = crate::db::connect_active_vault_for_sync(app).await? else {
        return Ok(None);
    };
    let vault_id = crate::vault::active_vault_id(app)?;
    Ok(Some(HubVault { pool, vault_id }))
}

/// Stops the service before a vault transition, letting a running pass finish first.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let task = app
        .state::<SyncRuntime>()
        .task
        .lock()
        .map_err(|_| "sync service lock is unavailable".to_string())?
        .take();
    let Some(mut task) = task else {
        return Ok(());
    };
    task.stop.notify_one();
    if tokio::time::timeout(STOP_TIMEOUT, &mut task.handle)
        .await
        .is_err()
    {
        task.handle.abort();
    }
    Ok(())
}

/// Restarts the service after a vault transition.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    start(app)
}

fn start<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let runtime = app.state::<SyncRuntime>();
    let mut task = runtime
        .task
        .lock()
        .map_err(|_| "sync service lock is unavailable".to_string())?;
    if task.is_some() {
        return Ok(());
    }
    let stop = Arc::new(Notify::new());
    let signals = Signals {
        stop: stop.clone(),
        commits: runtime.commits.clone(),
        now: runtime.now.clone(),
    };
    let handle = tauri::async_runtime::spawn(run(app.clone(), signals));
    *task = Some(ServiceTask { stop, handle });
    Ok(())
}

/// What ends a wait between passes.
enum Wake {
    Stop,
    /// A commit, debounced before the next pass.
    Commit,
    Pass,
    /// The long poll failed; the next pass waits for the retry delay.
    WaitFailed,
}

/// What the service waits for after a pass.
enum Next {
    /// Commits, a request, or the fallback interval.
    Idle,
    /// The retry delay after a failed exchange.
    Retry,
    /// Linked and current with the hub: its changes, through a long poll.
    Online(Box<OnlineWait>),
    /// An exchange stopped at its byte bound: the next pass continues it right away.
    Continue,
}

struct OnlineWait {
    transport: PairedTransport,
    ids: PeerIds,
    known: VersionVector,
}

struct Signals {
    stop: Arc<Notify>,
    commits: Arc<Notify>,
    now: Arc<Notify>,
}

impl Signals {
    async fn idle(&self, delay: Duration) -> Wake {
        tokio::select! {
            () = self.stop.notified() => Wake::Stop,
            () = self.commits.notified() => Wake::Commit,
            () = self.now.notified() => Wake::Pass,
            () = tokio::time::sleep(delay) => Wake::Pass,
        }
    }

    /// Long polls the hub until it holds operations this replica lacks, a local commit, or a
    /// request. A wait the hub answers without news is repeated.
    async fn online(&self, wait: &OnlineWait) -> Wake {
        loop {
            let result = tokio::select! {
                () = self.stop.notified() => return Wake::Stop,
                () = self.commits.notified() => return Wake::Commit,
                () = self.now.notified() => return Wake::Pass,
                result = super::client::wait(&wait.transport, &wait.ids, &wait.known) => result,
            };
            match result {
                Ok(hub) if !wait.known.dominates(&hub) => return Wake::Pass,
                Ok(_) => {}
                Err(error) => {
                    eprintln!("sync wait: {error}");
                    return Wake::WaitFailed;
                }
            }
        }
    }
}

async fn run<R: Runtime>(app: AppHandle<R>, signals: Signals) {
    let mut state = ServiceState::default();
    let mut failures = 0usize;
    loop {
        let next = state.pass(&app).await;
        let wake = match next {
            Next::Idle => {
                failures = 0;
                signals.idle(FALLBACK_INTERVAL).await
            }
            Next::Retry => {
                failures += 1;
                signals.idle(retry_delay(failures)).await
            }
            Next::Online(wait) => {
                failures = 0;
                signals.online(&wait).await
            }
            Next::Continue => {
                failures = 0;
                Wake::Pass
            }
        };
        match wake {
            Wake::Stop => return,
            Wake::Commit => {
                tokio::select! {
                    () = signals.stop.notified() => return,
                    () = tokio::time::sleep(COMMIT_DEBOUNCE) => {}
                }
            }
            Wake::Pass => {}
            Wake::WaitFailed => {
                failures += 1;
                if let Wake::Stop = signals.idle(retry_delay(failures)).await {
                    return;
                }
            }
        }
    }
}

fn retry_delay(failures: usize) -> Duration {
    RETRY_DELAYS[failures.saturating_sub(1).min(RETRY_DELAYS.len() - 1)]
}

/// State one service run keeps between passes. A vault transition stops the run, so the state
/// always belongs to one vault database.
#[derive(Default)]
struct ServiceState {
    vault_id: Option<String>,
    started: bool,
    writer: Option<ActiveWriter>,
    /// Captures a seal left in place, which only a new local change can make sealable.
    stalled_captures: Option<u64>,
    /// The stored vector at the last apply; apply runs again only after the log grows.
    applied_at: Option<VersionVector>,
    view: SyncStatusView,
}

impl ServiceState {
    async fn pass<R: Runtime>(&mut self, app: &AppHandle<R>) -> Next {
        let mut view = SyncStatusView {
            last_exchange_at_ms: self.view.last_exchange_at_ms,
            ..SyncStatusView::default()
        };
        let mut recovery = app.state::<SyncRuntime>().take_recovery_requests();
        let next = match self.pass_inner(app, &mut view, &mut recovery).await {
            Ok(next) => next,
            Err(error) => {
                eprintln!("sync: {error}");
                view.state = SyncState::Error;
                view.last_error = Some(error);
                Next::Retry
            }
        };
        for request in recovery {
            let _ = request
                .reply
                .send(Err("sync is not ready on this device".to_string()));
        }
        self.publish(app, view);
        app.state::<SyncRuntime>()
            .changes
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        next
    }

    async fn pass_inner<R: Runtime>(
        &mut self,
        app: &AppHandle<R>,
        view: &mut SyncStatusView,
        recovery: &mut Vec<RecoveryRequest>,
    ) -> Result<Next, String> {
        if !sync_enabled(app)? {
            return Ok(Next::Idle);
        }
        let Some(pool) = crate::db::connect_active_vault_for_sync(app).await? else {
            return Ok(Next::Idle);
        };
        let vault_id = crate::vault::active_vault_id(app)?;
        if self.vault_id.as_deref() != Some(vault_id.as_str()) {
            *self = Self {
                vault_id: Some(vault_id.clone()),
                ..Self::default()
            };
        }
        let manager = app.state::<PairingManager>().inner().clone();
        let peer = crate::vault::handoff::transport::sync_peer(&manager)?
            .filter(|(linked_vault, _)| *linked_vault == vault_id);
        view.role = Some(if peer.is_some() {
            SyncRole::Client
        } else {
            SyncRole::Hub
        });
        let session = match Session::open(app, pool.clone(), &vault_id, KeyRelease::Request).await?
        {
            Some(session) => Some(session),
            None if create_identity(app, &pool).await? => {
                Session::open(app, pool, &vault_id, KeyRelease::Request).await?
            }
            None => None,
        };
        let Some(session) = session else {
            view.state = SyncState::WaitingForIdentity;
            return Ok(Next::Idle);
        };
        if !self.started {
            session.start(&mut self.writer).await?;
            self.started = true;
        }
        session
            .seal_pending(&mut self.writer, &mut self.stalled_captures)
            .await?;
        view.state = SyncState::Idle;
        if !session.ensure_writer(&mut self.writer).await? {
            view.state = SyncState::WaitingForIdentity;
        }
        self.apply_pause_request(app)?;
        view.paused = self.paused(app)?;
        let mut next = Next::Idle;
        let is_hub = peer.is_none();
        match peer {
            None => {}
            Some(_) if view.paused => view.state = SyncState::Paused,
            Some((vault_id, device_id)) => {
                self.publish(
                    app,
                    SyncStatusView {
                        state: SyncState::Syncing,
                        ..view.clone()
                    },
                );
                let wait = OnlineWait {
                    transport: PairedTransport(manager.clone()),
                    ids: PeerIds {
                        vault_id,
                        device_id,
                    },
                    known: VersionVector::new(),
                };
                next = self.exchange(&session, wait, view).await?;
            }
        }
        self.apply_new(&session).await?;
        if !recovery.is_empty() && self.writer.is_some() {
            let mut restored = BTreeSet::new();
            for request in recovery.drain(..) {
                let result = session.close_recovery(&mut self.writer, &request).await;
                if let (Ok(Some(_)), Some(table)) = (
                    &result,
                    session.engine.manifest().table(request.choice.table),
                ) {
                    restored.insert(table.name.to_string());
                }
                let _ = request.reply.send(result);
            }
            emit_applied(app, restored.into_iter().collect());
        }
        if is_hub {
            session
                .revoke_unlinked(&manager, &vault_id, &mut self.writer)
                .await?;
        }
        let status = {
            let mut conn = acquire(&session.pool).await?;
            session
                .engine
                .status(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync status: {error}"))?
        };
        view.set_counts(&status);
        Ok(next)
    }

    /// Exchanges with the hub, re-sealing after a fork of this installation's chain.
    async fn exchange<R: Runtime>(
        &mut self,
        session: &Session<'_, R>,
        mut wait: OnlineWait,
        view: &mut SyncStatusView,
    ) -> Result<Next, String> {
        for _ in 0..MAX_FORK_RECOVERIES {
            let own_writers = self
                .writer
                .as_ref()
                .map(ActiveWriter::own_writers)
                .unwrap_or_default();
            let client = ClientSession {
                transport: &wait.transport,
                ids: &wait.ids,
                engine: &session.engine,
                pool: &session.pool,
                ctx: &session.ctx,
                writer: self.writer.as_ref().map(ActiveWriter::id),
                own_writers: &own_writers,
                max_bytes: super::client::MAX_EXCHANGE_BYTES,
            };
            match client.exchange().await {
                Err(error) => {
                    view.state = SyncState::Offline;
                    view.last_error = Some(error);
                    return Ok(Next::Retry);
                }
                Ok(Exchange::Done(done)) => {
                    view.last_exchange_at_ms = Some(now_ms()?);
                    match done.problem {
                        Some(problem) => {
                            view.state = SyncState::Error;
                            view.last_error = Some(problem);
                        }
                        None if done.more => return Ok(Next::Continue),
                        None => {}
                    }
                    wait.known = done.known;
                    return Ok(Next::Online(Box::new(wait)));
                }
                Ok(Exchange::Fork(fork)) => {
                    if !session.recover_fork(&mut self.writer, fork).await? {
                        view.state = SyncState::WaitingForIdentity;
                        return Ok(Next::Idle);
                    }
                    self.applied_at = None;
                }
            }
        }
        Err("sync fork recovery did not settle".to_string())
    }

    /// Applies stored operations once the log grew since the last apply. Apply always commits,
    /// so applying on every pass would wake the service again.
    async fn apply_new<R: Runtime>(&mut self, session: &Session<'_, R>) -> Result<(), String> {
        let stored = {
            let mut conn = acquire(&session.pool).await?;
            session
                .engine
                .stored_vector(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync log: {error}"))?
        };
        if self.applied_at.as_ref() == Some(&stored) {
            return Ok(());
        }
        let tables = session.apply_ready(&mut self.writer).await?;
        emit_applied(session.app, tables);
        self.applied_at = Some(stored);
        Ok(())
    }

    fn apply_pause_request<R: Runtime>(&mut self, app: &AppHandle<R>) -> Result<(), String> {
        let Some(active) = self.writer.as_mut() else {
            return Ok(());
        };
        let request = app
            .state::<SyncRuntime>()
            .pause_request
            .lock()
            .map_err(|_| "sync pause lock is unavailable".to_string())?
            .take();
        match request {
            Some(paused) => active.set_paused(paused),
            None => Ok(()),
        }
    }

    /// Whether exchanges are paused: the recorded choice, or one waiting for the writer.
    fn paused<R: Runtime>(&self, app: &AppHandle<R>) -> Result<bool, String> {
        let pending = *app
            .state::<SyncRuntime>()
            .pause_request
            .lock()
            .map_err(|_| "sync pause lock is unavailable".to_string())?;
        Ok(pending.unwrap_or_else(|| {
            self.writer
                .as_ref()
                .is_some_and(|writer| writer.record().paused)
        }))
    }

    fn publish<R: Runtime>(&mut self, app: &AppHandle<R>, view: SyncStatusView) {
        if self.view == view {
            return;
        }
        if let Ok(mut status) = app.state::<SyncRuntime>().status.lock() {
            status.clone_from(&view);
        }
        emit_status(app, &view);
        self.view = view;
    }
}

/// Creates the person identity the sync space is anchored at when this device generates it:
/// the writable owner without a coordinator pin. Other devices wait for the row to arrive with
/// a vault refresh. Returns whether an identity now exists.
async fn create_identity<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
) -> Result<bool, String> {
    match crate::contacts::identity::ensure_identity(app, pool).await {
        Ok(_) => Ok(true),
        Err(crate::contacts::ContactsError::IdentityUnavailable(_)) => Ok(false),
        Err(error) => Err(format!("create person identity: {}", error.into_message())),
    }
}

/// Whether this device seals for the active vault: a linked owner, or a guarded replica.
fn sync_enabled<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let status = crate::vault::ownership::active_status(app)?;
    if status.replicated_writes {
        return Ok(true);
    }
    Ok(status.can_write
        && app
            .state::<crate::vault::ownership::VaultOwnershipManager>()
            .is_linked(&status.vault_id))
}

/// How a quiesced vault is opened to seal its captures.
pub(crate) struct QuiescedSync {
    vault_id: String,
    path: PathBuf,
    access: DatabaseAccessMode,
}

/// Decides, before a vault transition fences writes, whether the vault must be sealed once its
/// pools close. Sealing depends on the role and links, not on the transfer phase, because
/// preparing a transfer changes the phase before the seal runs.
pub(crate) fn quiesced_sync<R: Runtime>(app: &AppHandle<R>) -> Option<QuiescedSync> {
    let status = crate::vault::ownership::active_status(app).ok()?;
    let linked = app
        .state::<crate::vault::ownership::VaultOwnershipManager>()
        .is_linked(&status.vault_id);
    if !linked {
        return None;
    }
    let access = if status.can_write {
        DatabaseAccessMode::ReadWrite
    } else if status.role == "read-only" {
        DatabaseAccessMode::Guarded
    } else {
        return None;
    };
    let path = crate::vault::active_database_path(app).ok()?;
    Some(QuiescedSync {
        vault_id: status.vault_id,
        path,
        access,
    })
}

/// Seals pending captures of a quiesced vault through a private pool, because the shared
/// connection gate is held. Failures are logged: the captures stay and seal later.
pub(crate) async fn seal_quiesced<R: Runtime>(app: &AppHandle<R>, plan: Option<QuiescedSync>) {
    let Some(plan) = plan else {
        return;
    };
    let registry = DatabasePoolRegistry::default();
    let result = seal_quiesced_with(app, &registry, &plan).await;
    if let Err(error) = result.and(registry.close_all().await) {
        eprintln!("seal sync changes before vault transition: {error}");
    }
}

async fn seal_quiesced_with<R: Runtime>(
    app: &AppHandle<R>,
    registry: &DatabasePoolRegistry,
    plan: &QuiescedSync,
) -> Result<(), String> {
    let pool = match plan.access {
        DatabaseAccessMode::Guarded => match registry.connect_path_guarded(&plan.path).await? {
            (pool, DatabaseAccessMode::Guarded) => pool,
            _ => return Ok(()),
        },
        _ => registry.connect_path(&plan.path).await?,
    };
    let Some(session) = Session::open(app, pool, &plan.vault_id, KeyRelease::Skip).await? else {
        return Ok(());
    };
    let mut writer = None;
    session.start(&mut writer).await?;
    session.seal_pending(&mut writer, &mut None).await
}

/// Exports local operations the staged replacement lacks, before the replacement happens.
pub(crate) async fn export_for_replacement(
    current: &std::path::Path,
    replacement: &std::path::Path,
    files: &SyncFiles,
) -> Result<usize, String> {
    carry_forward::export(&Engine::vault(), current, replacement, files).await
}

/// Whether a missing person key may be requested from the coordinator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyRelease {
    Request,
    /// The vault is quiesced, so the request could not read the vault database.
    Skip,
}

/// One pass over an open vault pool with the space it replicates.
struct Session<'a, R: Runtime> {
    app: &'a AppHandle<R>,
    pool: SqlitePool,
    engine: Engine,
    files: SyncFiles,
    keys: Arc<dyn WriterKeyStore>,
    ctx: SpaceContext,
    key_release: KeyRelease,
}

impl<'a, R: Runtime> Session<'a, R> {
    /// Opens a session, or returns `None` while the vault has no person identity.
    async fn open(
        app: &'a AppHandle<R>,
        pool: SqlitePool,
        vault_id: &str,
        key_release: KeyRelease,
    ) -> Result<Option<Self>, String> {
        let ctx = {
            let mut conn = acquire(&pool).await?;
            local::space_context(&mut conn, vault_id)
                .await
                .map_err(|error| format!("read sync space: {error}"))?
        };
        let Some(ctx) = ctx else {
            return Ok(None);
        };
        Ok(Some(Self {
            app,
            pool,
            engine: Engine::vault(),
            files: sync_files(app, vault_id)?,
            keys: key_store(app)?,
            ctx,
            key_release,
        }))
    }

    /// Start order: clear a stale apply flag, import carried operations, check the writer, and
    /// apply what the import could not apply before local captures were sealed.
    async fn start(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        let imported = {
            let mut conn = acquire(&self.pool).await?;
            local::reset_applying(&mut conn)
                .await
                .map_err(|error| format!("reset sync apply state: {error}"))?;
            self.engine
                .init_space(&mut conn, &self.files.vault_id, now_ms()?)
                .await
                .map_err(|error| format!("initialize sync space: {error}"))?;
            carry_forward::import_pending(
                &self.engine,
                &mut conn,
                &self.ctx,
                &self.files,
                now_ms()?,
            )
            .await?
        };
        if !self.ensure_writer(writer).await? {
            return Ok(());
        }
        if imported.is_some_and(|report| report.pending_captures) {
            emit_applied(self.app, self.apply_ready(writer).await?);
        }
        Ok(())
    }

    /// Seals pending captures. A seal that leaves captures in place is not repeated until the
    /// number of captures changes.
    async fn seal_pending(
        &self,
        writer: &mut Option<ActiveWriter>,
        stalled: &mut Option<u64>,
    ) -> Result<(), String> {
        let pending = self.pending_captures().await?;
        if pending == 0 || *stalled == Some(pending) {
            if pending == 0 {
                *stalled = None;
            }
            return Ok(());
        }
        if !self.ensure_writer(writer).await? {
            return Ok(());
        }
        let report = self.seal(writer).await?;
        let remaining = self.pending_captures().await?;
        *stalled = (remaining > 0).then_some(remaining);
        if !report.invalid_rows.is_empty() {
            eprintln!(
                "sync: {} local rows cannot be sealed",
                report.invalid_rows.len()
            );
        }
        Ok(())
    }

    async fn pending_captures(&self) -> Result<u64, String> {
        let mut conn = acquire(&self.pool).await?;
        local::pending_captures(&mut conn)
            .await
            .map_err(|error| format!("read pending sync changes: {error}"))
    }

    /// Makes sure a writer is ready, checking the recorded one against the database and
    /// creating a successor when needed. Returns false while the person key is unavailable.
    async fn ensure_writer(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        if writer.is_some() {
            return Ok(true);
        }
        let files = self.files.clone();
        let keys = self.keys.clone();
        let stored = blocking(move || read_stored_writer(&files, keys.as_ref())).await?;
        let check = {
            let mut conn = acquire(&self.pool).await?;
            check_writer(&self.engine, &mut conn, &self.ctx, &self.files, stored).await?
        };
        *writer = match check {
            WriterCheck::Ready(ready) => {
                let files = self.files.clone();
                let keys = self.keys.clone();
                Some(
                    blocking(move || {
                        let mut ready = *ready;
                        remove_retired_keys(&mut ready, &files, keys.as_ref())?;
                        Ok(ready)
                    })
                    .await?,
                )
            }
            WriterCheck::Successor(plan) => self.create_successor(plan).await?,
        };
        Ok(writer.is_some())
    }

    /// Creates a successor writer, or returns `None` while the person key is unavailable.
    async fn create_successor(&self, plan: SuccessorPlan) -> Result<Option<ActiveWriter>, String> {
        let person =
            crate::contacts::identity::person_key_matching(self.app, &self.ctx.anchor).await?;
        let Some(person) = person else {
            if self.key_release == KeyRelease::Request {
                let app = self.app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::contacts::identity::try_release(&app).await;
                });
            }
            return Ok(None);
        };
        let device_id = crate::vault::ensure_device_id(self.app)?;
        let files = self.files.clone();
        let keys = self.keys.clone();
        let now = now_ms()?;
        blocking(move || {
            create_writer(&files, keys.as_ref(), plan, &person, &device_id, now).map(Some)
        })
        .await
    }

    /// Replaces a writer the space stopped accepting.
    async fn rotate(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let plan = writer
            .as_ref()
            .map(SuccessorPlan::after)
            .unwrap_or_default();
        *writer = self.create_successor(plan).await?;
        Ok(writer.is_some())
    }

    /// Seals with the writer, rotating once when the space no longer accepts it.
    async fn seal(&self, writer: &mut Option<ActiveWriter>) -> Result<SealReport, String> {
        for attempt in 0..2 {
            let Some(active) = writer.as_mut() else {
                break;
            };
            let result = {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .seal(&mut conn, &self.ctx, &mut active.local(), now_ms()?)
                    .await
            };
            match result {
                Ok(report) => {
                    if let Some(seq) = report.last_seq {
                        active.committed(seq)?;
                    }
                    return Ok(report);
                }
                Err(SyncError::WriterInactive(_)) if attempt == 0 => {
                    if !self.rotate(writer).await? {
                        break;
                    }
                }
                Err(error) => return Err(format!("seal sync changes: {error}")),
            }
        }
        Ok(SealReport::default())
    }

    /// Applies ready operations, re-sealing and rotating the local writer when the space
    /// requires it. Returns the tables whose rows changed.
    async fn apply_ready(&self, writer: &mut Option<ActiveWriter>) -> Result<Vec<String>, String> {
        let mut tables = BTreeSet::new();
        for _ in 0..MAX_APPLY_ROUNDS {
            let report = {
                let mut conn = acquire(&self.pool).await?;
                let mut local = writer.as_mut().map(ActiveWriter::local);
                self.engine
                    .apply(&mut conn, &self.ctx, local.as_mut(), now_ms()?)
                    .await
                    .map_err(|error| format!("apply sync operations: {error}"))?
            };
            tables.extend(
                report
                    .changed_tables
                    .iter()
                    .map(|table| (*table).to_string()),
            );
            if let (Some(active), Some(seq)) = (
                writer.as_mut(),
                report.seal.as_ref().and_then(|seal| seal.last_seq),
            ) {
                active.committed(seq)?;
            }
            if let Some(needed) = report.reseal_needed {
                let current = writer.as_ref().map(ActiveWriter::id);
                if (current == Some(needed.writer) || current.is_none())
                    && !self.rotate(writer).await?
                {
                    return Ok(tables.into_iter().collect());
                }
                let Some(active) = writer.as_mut() else {
                    return Ok(tables.into_iter().collect());
                };
                let resealed = {
                    let mut conn = acquire(&self.pool).await?;
                    self.engine
                        .reseal(
                            &mut conn,
                            &self.ctx,
                            needed.writer,
                            needed.keep_through,
                            &mut active.local(),
                            now_ms()?,
                        )
                        .await
                        .map_err(|error| format!("re-seal sync operations: {error}"))?
                };
                if let Some(seq) = resealed.seal.last_seq {
                    active.committed(seq)?;
                }
            } else if report.local_writer_frozen {
                if !self.rotate(writer).await? {
                    return Ok(tables.into_iter().collect());
                }
            } else if report.pending_captures || !report.more {
                return Ok(tables.into_iter().collect());
            }
        }
        Err("sync apply did not finish".to_string())
    }

    /// Recovers from a fork of one of this installation's chains: the hub holds other
    /// operations at the same sequences. The local copy above the fork point moves to a fresh
    /// writer, so both histories survive. Returns false while the person key is unavailable.
    async fn recover_fork(
        &self,
        writer: &mut Option<ActiveWriter>,
        fork: ForkPoint,
    ) -> Result<bool, String> {
        let current = writer.as_ref().map(ActiveWriter::id);
        if (current == Some(fork.writer) || current.is_none()) && !self.rotate(writer).await? {
            return Ok(false);
        }
        let Some(active) = writer.as_mut() else {
            return Ok(false);
        };
        let resealed = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .reseal_fork(
                    &mut conn,
                    &self.ctx,
                    fork.writer,
                    fork.keep_through,
                    &mut active.local(),
                    now_ms()?,
                )
                .await
                .map_err(|error| format!("re-seal forked sync operations: {error}"))?
        };
        if let Some(seq) = resealed.seal.last_seq {
            active.committed(seq)?;
        }
        Ok(true)
    }

    /// On the hub, revokes the active writers of devices that were unlinked from the vault, so
    /// operations they seal later are refused everywhere. The cutoff keeps every stored
    /// operation; a writer whose genesis is not applied yet waits for a later pass.
    async fn revoke_unlinked(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<(), String> {
        let revoked = manager.revoked_device_ids(vault_id)?;
        if revoked.is_empty() {
            return Ok(());
        }
        let Some(active) = writer.as_mut() else {
            return Ok(());
        };
        let mut conn = acquire(&self.pool).await?;
        let status = self
            .engine
            .status(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync writers: {error}"))?;
        for summary in status.writers {
            if summary.state != WriterState::Active
                || summary.applied == 0
                || summary.writer == active.id()
                || !revoked.contains(&summary.device_id)
            {
                continue;
            }
            let report = self
                .engine
                .seal_revoke(
                    &mut conn,
                    &self.ctx,
                    summary.writer,
                    summary.stored,
                    RevokeReason::Revoked,
                    &mut active.local(),
                    now_ms()?,
                )
                .await
                .map_err(|error| format!("revoke unlinked sync writer: {error}"))?;
            if let Some(seq) = report.last_seq {
                active.committed(seq)?;
            }
        }
        Ok(())
    }

    /// Restores or discards a recovery offer with the local writer.
    async fn close_recovery(
        &self,
        writer: &mut Option<ActiveWriter>,
        request: &RecoveryRequest,
    ) -> Result<Option<String>, String> {
        let Some(active) = writer.as_mut() else {
            return Err("the sync writer is not ready".to_string());
        };
        super::recovery::close(
            &self.engine,
            &self.pool,
            &self.ctx,
            active,
            &request.choice,
            now_ms()?,
        )
        .await
    }
}

fn sync_directory<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?
        .join(SYNC_DIRECTORY);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("create sync directory: {error}"))?;
    Ok(directory)
}

/// Device-local sync files of a vault.
pub(crate) fn sync_files<R: Runtime>(
    app: &AppHandle<R>,
    vault_id: &str,
) -> Result<SyncFiles, String> {
    SyncFiles::new(sync_directory(app)?, vault_id)
}

#[cfg(desktop)]
fn key_store<R: Runtime>(_app: &AppHandle<R>) -> Result<Arc<dyn WriterKeyStore>, String> {
    Ok(Arc::new(super::writer::KeyringStore))
}

#[cfg(mobile)]
fn key_store<R: Runtime>(app: &AppHandle<R>) -> Result<Arc<dyn WriterKeyStore>, String> {
    Ok(Arc::new(super::writer::FileKeyStore {
        directory: sync_directory(app)?,
    }))
}

async fn acquire(pool: &SqlitePool) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
}

/// Runs key store and record work off the async runtime, because key stores may block.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("sync key worker: {error}"))?
}

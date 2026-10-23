//! The sync service of the active vault: it imports carried operations, keeps the local writer
//! current, seals local captures after commits, and exchanges operations and managed asset
//! files with the hub. On the coordinator it applies what linked devices push and revokes the
//! writers of unlinked devices.
//!
//! A pass runs through a [`SyncHost`], so the app service and a headless pass share it. Passes
//! of one process run one at a time behind a gate, and a headless pass hands its work to the
//! app service when one is running.

use super::assets::{AssetMemory, AssetSession, AssetStore, AssetTransfer, MAX_ASSET_PASS_BYTES};
use super::carry_forward;
use super::client::{ClientSession, Exchange, ForkPoint, PeerIds};
#[cfg(mobile)]
use super::host::HeadlessHost;
use super::host::SyncHost;
use super::hub::HubStability;
#[cfg(desktop)]
use super::hub::{HubVault, OpenHubVault, SyncHub};
use super::recovery::{RecoveryChoice, RecoveryRequest};
use super::status::{DegradedReasons, SyncErrorCode, SyncRole, SyncState, SyncStatusView};
use super::transport::PairedTransport;
use super::vault_assets::ActiveVaultAssets;
use super::writer::{
    ActiveWriter, SuccessorPlan, SyncFiles, WriterCheck, WriterKeyStore, check_writer,
    create_writer, read_stored_writer, remove_retired_keys,
};
use crate::vault::handoff::pairing::PairingManager;
use ganbaru_db::{DatabaseAccessMode, DatabasePoolRegistry};
use ganbaru_handoff::protocol::SyncRefusalCode;
use ganbaru_sync::stability::{CountedDevice, DeviceStanding};
use ganbaru_sync::{
    ChangeNotice, Engine, SealReport, SpaceContext, SyncError, WriterState, compaction_limit, hold,
    local,
};
use ganbaru_sync_contracts::op::RevokeReason;
use ganbaru_sync_contracts::{Seq, VersionVector, WriterId};
use ganbaru_sync_replica::now_ms;
use sqlx::SqlitePool;
use std::collections::BTreeSet;
#[cfg(any(mobile, test))]
use std::future::Future;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
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
/// How long a headless pass waits for the running app service to finish the pass it asked for.
#[cfg(mobile)]
const DELEGATED_PASS_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound on apply rounds in one pass; each round is one bounded transaction.
const MAX_APPLY_ROUNDS: usize = 10_000;
/// Bound on seal rounds in one pass; each round is one bounded transaction.
const MAX_SEAL_ROUNDS: usize = 10_000;
/// Fork recoveries one pass may run before it reports an error.
const MAX_FORK_RECOVERIES: usize = 3;

/// Lets one sync pass of this process run at a time, whether the app service, a headless pass,
/// or the seal before a vault transition runs it.
static PASS_GATE: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The service state of the running app, so a headless pass can hand its work to it instead
/// of running a second pass. Cleared when the app exits.
static LIVE_RUNTIME: Mutex<Weak<RuntimeState>> = Mutex::new(Weak::new());

/// Managed state of the sync service.
#[derive(Clone, Default)]
pub(crate) struct SyncRuntime(Arc<RuntimeState>);

impl Deref for SyncRuntime {
    type Target = RuntimeState;

    fn deref(&self) -> &RuntimeState {
        &self.0
    }
}

/// What the sync service and its commands share.
pub(crate) struct RuntimeState {
    /// Signaled to run a pass now.
    now: Arc<Notify>,
    /// Bumped after each pass, so hub waits recheck the log.
    changes: watch::Sender<u64>,
    /// Stability the coordinator pass last computed, reported in every sync state.
    stability: HubStability,
    /// A pause change waiting for the writer record.
    pause_request: Mutex<Option<bool>>,
    /// Recovery actions waiting for the next pass.
    recovery: Mutex<Vec<RecoveryRequest>>,
    status: Mutex<SyncStatusView>,
    task: Mutex<Option<ServiceTask>>,
    /// Number of the latest pass that started.
    passes_started: AtomicU64,
    /// Number of the latest pass that finished.
    passes_done: watch::Sender<u64>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            now: Arc::default(),
            changes: watch::Sender::new(0),
            stability: HubStability::default(),
            pause_request: Mutex::default(),
            recovery: Mutex::default(),
            status: Mutex::default(),
            task: Mutex::default(),
            passes_started: AtomicU64::new(0),
            passes_done: watch::Sender::new(0),
        }
    }
}

impl RuntimeState {
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

    /// Whether the service task is running.
    #[cfg(any(mobile, test))]
    fn running(&self) -> Result<bool, String> {
        self.task
            .lock()
            .map(|task| task.is_some())
            .map_err(|_| "sync service lock is unavailable".to_string())
    }

    /// Marks the start of a pass and returns its number.
    fn pass_started(&self) -> u64 {
        self.passes_started.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Marks the end of pass `number`.
    fn pass_done(&self, number: u64) {
        self.passes_done
            .send_modify(|done| *done = (*done).max(number));
    }
}

struct ServiceTask {
    stop: Arc<Notify>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

/// Registers the service state as the runtime a headless pass hands its work to, and starts
/// the service. Runs once at startup, before the frontend opens the vault. The vault connection
/// hook is installed with the process pool registry.
pub(crate) fn setup<R: Runtime>(app: &AppHandle<R>) {
    let runtime = app.state::<SyncRuntime>();
    match LIVE_RUNTIME.lock() {
        Ok(mut live) => *live = Arc::downgrade(&runtime.0),
        Err(_) => eprintln!("register sync runtime: the runtime lock is unavailable"),
    }
    if let Err(error) = start(app) {
        eprintln!("start sync service: {error}");
    }
}

/// Forgets the running app service, so a later headless pass in this process runs its own
/// pass. Runs when the app exits.
pub(crate) fn release_runtime_handle() {
    match LIVE_RUNTIME.lock() {
        Ok(mut live) => *live = Weak::new(),
        Err(_) => eprintln!("release sync runtime: the runtime lock is unavailable"),
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
    let runtime = app.state::<SyncRuntime>();
    SyncHub::new(open, runtime.changes.clone()).with_stability(runtime.stability.clone())
}

#[cfg(desktop)]
async fn open_hub_vault<R: Runtime>(app: &AppHandle<R>) -> Result<Option<HubVault>, String> {
    if !app.sync_enabled()? {
        return Ok(None);
    }
    let Some(pool) = app.connect_for_sync().await? else {
        return Ok(None);
    };
    let vault_id = app.active_vault_id()?;
    let assets: Arc<dyn AssetStore> = Arc::new(ActiveVaultAssets::open(app, &vault_id)?);
    Ok(Some(HubVault {
        pool,
        device_id: coordinator_device_id(app, &vault_id)?,
        vault_id,
        assets,
    }))
}

/// Stops the service before a vault transition, letting a running pass finish first. A pass
/// of this process that does not finish in time fails the stop, which keeps the vault.
pub(crate) async fn stop_for_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    let task = app
        .state::<SyncRuntime>()
        .task
        .lock()
        .map_err(|_| "sync service lock is unavailable".to_string())?
        .take();
    if let Some(mut task) = task {
        task.stop.notify_one();
        if tokio::time::timeout_at(deadline, &mut task.handle)
            .await
            .is_err()
        {
            task.handle.abort();
            // The aborted task resolves as cancelled once its pass is dropped, which releases
            // the pass gate; the cancellation itself is the expected outcome.
            let _cancelled = task.handle.await;
        }
    }
    tokio::time::timeout_at(deadline, PASS_GATE.lock())
        .await
        .map(drop)
        .map_err(|_| "a sync pass is still running".to_string())
}

/// Restarts the service after a vault transition.
pub(crate) fn resume_after_vault_handoff<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    start(app)
}

fn start<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let runtime = app.state::<SyncRuntime>().inner().clone();
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
        commits: crate::db::vault_commits().clone(),
        now: runtime.now.clone(),
    };
    let handle = tauri::async_runtime::spawn(run(app.clone(), runtime.clone(), signals));
    *task = Some(ServiceTask { stop, handle });
    Ok(())
}

/// What a headless pass did.
#[cfg(any(mobile, test))]
pub(crate) struct HeadlessPass {
    /// The status after the pass.
    pub(crate) status: SyncStatusView,
    /// The running app service ran the pass.
    pub(crate) delegated: bool,
    /// An exchange stopped at its byte bound, so another pass would continue it.
    pub(crate) more: bool,
}

/// Runs one pass for the app config directory `config_dir` without a Tauri runtime: it seals,
/// exchanges with the hub, applies, and moves managed assets, then returns without waiting for
/// the hub. When the app service of this process is running, it runs the pass instead.
#[cfg(mobile)]
pub(crate) async fn headless_pass(config_dir: PathBuf) -> Result<HeadlessPass, String> {
    let open = async move { blocking(move || HeadlessHost::open(&config_dir)).await };
    headless_pass_with(open, &LIVE_RUNTIME, &PASS_GATE, DELEGATED_PASS_TIMEOUT).await
}

/// Runs one headless pass with the host `open` builds, or asks the runtime registered in `live`
/// to run it and waits up to `wait` for it. The host is opened only when no runtime runs.
#[cfg(any(mobile, test))]
async fn headless_pass_with<H, F>(
    open: F,
    live: &Mutex<Weak<RuntimeState>>,
    gate: &tokio::sync::Mutex<()>,
    wait: Duration,
) -> Result<HeadlessPass, String>
where
    H: SyncHost,
    F: Future<Output = Result<H, String>>,
{
    let running = live
        .lock()
        .map_err(|_| "sync runtime lock is unavailable".to_string())?
        .upgrade();
    if let Some(runtime) = running {
        return Ok(HeadlessPass {
            status: delegate_pass(&runtime, wait).await?,
            delegated: true,
            more: false,
        });
    }
    let host = open.await?;
    let runtime = RuntimeState::default();
    let next = ServiceState::default().pass(&host, &runtime, gate).await;
    Ok(HeadlessPass {
        status: runtime.status()?,
        delegated: false,
        more: matches!(next, Next::Continue),
    })
}

/// Asks the running service for a pass and waits up to `wait` for one that started after the
/// request to finish. It never runs a pass itself, so a slow service fails the call.
#[cfg(any(mobile, test))]
async fn delegate_pass(runtime: &RuntimeState, wait: Duration) -> Result<SyncStatusView, String> {
    if !runtime.running()? {
        return Err("the sync service is stopped".to_string());
    }
    let mut done = runtime.passes_done.subscribe();
    let target = runtime.passes_started.load(Ordering::SeqCst) + 1;
    runtime.sync_now();
    let finished = tokio::time::timeout(wait, done.wait_for(|done| *done >= target))
        .await
        .map(|result| result.is_ok());
    match finished {
        Ok(true) => runtime.status(),
        Ok(false) => Err("the sync service stopped".to_string()),
        Err(_) => Err("the sync service did not finish a pass in time".to_string()),
    }
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

    /// Long polls the hub until it holds operations this replica lacks, a local commit, a
    /// request, or `hold` elapses. A wait the hub answers without news is repeated.
    async fn online(&self, wait: &OnlineWait, hold: Option<Duration>) -> Wake {
        let hold_expiry = hold.map(|delay| tokio::time::Instant::now() + delay);
        loop {
            let result = tokio::select! {
                () = self.stop.notified() => return Wake::Stop,
                () = self.commits.notified() => return Wake::Commit,
                () = self.now.notified() => return Wake::Pass,
                () = sleep_until(hold_expiry) => return Wake::Pass,
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

/// Sleeps until `deadline`, or forever without one.
async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn run<H: SyncHost>(host: H, runtime: SyncRuntime, signals: Signals) {
    let mut state = ServiceState::default();
    let mut failures = 0usize;
    loop {
        let next = state.pass(&host, &runtime, &PASS_GATE).await;
        let hold = state.until_hold_expiry();
        let wake = match next {
            Next::Idle => {
                failures = 0;
                signals.idle(capped(FALLBACK_INTERVAL, hold)).await
            }
            Next::Retry => {
                failures += 1;
                signals.idle(capped(retry_delay(failures), hold)).await
            }
            Next::Online(wait) => {
                failures = 0;
                signals.online(&wait, hold).await
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

/// `delay`, shortened to `limit` when one is given.
fn capped(delay: Duration, limit: Option<Duration>) -> Duration {
    limit.map_or(delay, |limit| delay.min(limit))
}

fn retry_delay(failures: usize) -> Duration {
    RETRY_DELAYS[failures.saturating_sub(1).min(RETRY_DELAYS.len() - 1)]
}

fn asset_session<'s, H: SyncHost>(
    session: &'s Session<'_, H>,
    peer: &'s AssetPeer,
) -> AssetSession<'s, PairedTransport> {
    AssetSession {
        transport: &peer.transport,
        ids: &peer.ids,
        engine: &session.engine,
        pool: &session.pool,
        store: &peer.assets,
        max_bytes: MAX_ASSET_PASS_BYTES,
    }
}

/// State one service run keeps between passes. A vault transition stops the run, so the state
/// always belongs to one vault database.
#[derive(Default)]
struct ServiceState {
    vault_id: Option<String>,
    started: bool,
    writer: Option<ActiveWriter>,
    /// Captures a seal left in place, which only a new local change or an expired delete hold
    /// can make sealable.
    stalled_captures: Option<u64>,
    /// When the earliest delete hold expires, in Unix milliseconds; a pass runs then.
    hold_expiry: Option<u64>,
    /// When the last apply ran, so the next one waits for something to change.
    applied: ApplyMark,
    /// Which managed assets the hub holds or lacks, so passes do not ask again.
    assets: AssetMemory,
    /// The head of the active writer the hub last confirmed holding with the same hash, which
    /// bounds compaction of this installation's own writers.
    confirmed: Option<(WriterId, Seq)>,
    view: SyncStatusView,
}

/// When the service last applied stored operations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ApplyMark {
    /// The stored vector at the last apply.
    stored: Option<VersionVector>,
    /// Local captures left when the last apply stopped on captures that sealing did not drain.
    blocked_at: Option<u64>,
}

impl ApplyMark {
    /// Whether an apply is due: the log grew, or local captures changed since an apply
    /// stopped on them. A capture count that stays the same cannot unblock it.
    fn due(&self, stored: &VersionVector, pending: u64) -> bool {
        self.stored.as_ref() != Some(stored)
            || self.blocked_at.is_some_and(|blocked| blocked != pending)
    }
}

/// What [`Session::apply_ready`] did.
struct Applied {
    /// Tables whose rows changed.
    tables: Vec<String>,
    /// Changes the registered hooks reported.
    notices: Vec<ChangeNotice>,
    /// Local captures left when it stopped on captures that sealing did not drain.
    blocked_at: Option<u64>,
}

/// The hub and managed asset files of one pass's asset steps.
struct AssetPeer {
    transport: PairedTransport,
    ids: PeerIds,
    assets: Arc<dyn AssetStore>,
}

impl ServiceState {
    /// Time left until the earliest delete hold expires. A hold that already expired was
    /// offered to the pass's seal, so it waits for the usual schedule instead of spinning.
    fn until_hold_expiry(&self) -> Option<Duration> {
        let expiry = self.hold_expiry?;
        let now = now_ms().ok()?;
        (expiry > now).then(|| Duration::from_millis(expiry - now))
    }

    /// Runs one pass once `gate` lets it, then publishes the status and wakes hub waits.
    async fn pass<H: SyncHost>(
        &mut self,
        host: &H,
        runtime: &RuntimeState,
        gate: &tokio::sync::Mutex<()>,
    ) -> Next {
        let _gate = gate.lock().await;
        let number = runtime.pass_started();
        let mut view = SyncStatusView {
            last_exchange_at_ms: self.view.last_exchange_at_ms,
            connection: self.view.connection.clone(),
            degraded: self.view.degraded.clone(),
            ..SyncStatusView::default()
        };
        let mut recovery = runtime.take_recovery_requests();
        let mut succeeded = false;
        let next = match self
            .pass_inner(host, runtime, &mut view, &mut recovery)
            .await
        {
            Ok(next) => {
                succeeded = true;
                next
            }
            Err(error) => {
                eprintln!("sync: {error}");
                view.state = SyncState::Error;
                view.last_error = Some(error);
                Next::Retry
            }
        };
        set_connectivity(host, &mut view).await;
        for request in recovery {
            let _ = request
                .reply
                .send(Err("sync is not ready on this device".to_string()));
        }
        self.publish(host, runtime, view);
        runtime
            .changes
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        runtime.pass_done(number);
        if succeeded {
            // Compaction ran in the pass, so retained versions no longer hold what it dropped.
            super::asset_collection::collect_if_due(host).await;
        }
        next
    }

    async fn pass_inner<H: SyncHost>(
        &mut self,
        host: &H,
        runtime: &RuntimeState,
        view: &mut SyncStatusView,
        recovery: &mut Vec<RecoveryRequest>,
    ) -> Result<Next, String> {
        if !host.sync_enabled()? {
            return Ok(Next::Idle);
        }
        let Some(pool) = host.connect_for_sync().await? else {
            return Ok(Next::Idle);
        };
        let vault_id = host.active_vault_id()?;
        if self.vault_id.as_deref() != Some(vault_id.as_str()) {
            *self = Self {
                vault_id: Some(vault_id.clone()),
                ..Self::default()
            };
            runtime.stability.clear();
        }
        let manager = host.pairing();
        let peer = crate::vault::handoff::transport::sync_peer(&manager)?
            .filter(|(linked_vault, _)| *linked_vault == vault_id);
        view.role = Some(if peer.is_some() {
            SyncRole::Client
        } else {
            SyncRole::Hub
        });
        let session =
            match Session::open(host, pool.clone(), &vault_id, KeyRelease::Request).await? {
                Some(session) => Some(session),
                None if host.create_identity(&pool).await? => {
                    Session::open(host, pool, &vault_id, KeyRelease::Request).await?
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
        self.hold_expiry = session.next_hold_expiry().await?;
        view.state = SyncState::Idle;
        if !session.ensure_writer(&mut self.writer).await? {
            view.state = SyncState::WaitingForIdentity;
        }
        self.apply_pause_request(runtime)?;
        view.paused = self.paused(runtime)?;
        let mut next = Next::Idle;
        let is_hub = peer.is_none();
        let mut asset_peer = None;
        match peer {
            None => {}
            Some(_) if view.paused => view.state = SyncState::Paused,
            Some((linked_vault, device_id)) => {
                self.publish(
                    host,
                    runtime,
                    SyncStatusView {
                        state: SyncState::Syncing,
                        ..view.clone()
                    },
                );
                let assets = AssetPeer {
                    transport: PairedTransport(manager.clone()),
                    ids: PeerIds {
                        vault_id: linked_vault,
                        device_id,
                    },
                    assets: Arc::new(ActiveVaultAssets::open(host, &vault_id)?),
                };
                let mut transfer = AssetTransfer::default();
                next = match self.upload_assets(&session, &assets, &mut transfer).await {
                    Err(error) => {
                        view.state = SyncState::Offline;
                        view.last_error = Some(error);
                        Next::Retry
                    }
                    Ok(()) => {
                        let wait = OnlineWait {
                            transport: assets.transport.clone(),
                            ids: assets.ids.clone(),
                            known: VersionVector::new(),
                        };
                        self.exchange(&session, wait, view).await?
                    }
                };
                asset_peer = Some((assets, transfer));
            }
        }
        self.apply_new(&session).await?;
        if let Some((assets, mut transfer)) = asset_peer {
            next = self
                .download_assets(&session, &assets, &mut transfer, next, view)
                .await;
        }
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
            host.emit_applied(restored.into_iter().collect());
        }
        let acknowledged = session.acknowledge(&mut self.writer).await?;
        if is_hub {
            session
                .revoke_unlinked(&manager, &vault_id, &mut self.writer)
                .await?;
            let stable = self
                .stability(host, &session, &manager, &vault_id, view)
                .await?;
            runtime.stability.publish(&vault_id, stable.clone());
            next = Self::compact(&session, &stable, next).await?;
        } else {
            runtime.stability.clear();
            if acknowledged.sealed_ops > 0 && matches!(next, Next::Online(_)) {
                // The hub learns of the acknowledgement on the next exchange, which runs now.
                next = Next::Continue;
            }
            let limit = self.client_compaction_limit(&session).await?;
            next = Self::compact(&session, &limit, next).await?;
        }
        let (status, clock_warnings) = {
            let mut conn = acquire(&session.pool).await?;
            let status = session
                .engine
                .status(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync status: {error}"))?;
            let clock_warnings = session
                .engine
                .far_ahead_writers(&mut conn, &session.ctx, now_ms()?)
                .await
                .map_err(|error| format!("read sync clocks: {error}"))?;
            (status, clock_warnings)
        };
        view.set_counts(&status);
        if clock_warnings.is_empty() {
            view.clock_ahead_devices.clear();
        } else {
            let names = super::devices::read_with(host.device_id()?, &manager);
            view.set_clock_ahead(&status, &clock_warnings, &names);
        }
        Ok(next)
    }

    /// On the coordinator, computes stability over the vault's linked devices and names the
    /// devices it does not count. A device whose link record has no valid time must be linked
    /// again before it counts.
    async fn stability<H: SyncHost>(
        &mut self,
        host: &H,
        session: &Session<'_, H>,
        manager: &PairingManager,
        vault_id: &str,
        view: &mut SyncStatusView,
    ) -> Result<VersionVector, String> {
        let now = now_ms()?;
        let devices: Vec<CountedDevice> = manager
            .linked_peers_for(vault_id)?
            .into_iter()
            .map(|peer| CountedDevice {
                device_id: peer.device_id,
                linked_at_ms: u64::try_from(peer.linked_at_ms)
                    .ok()
                    .filter(|linked_at| *linked_at > 0),
            })
            .collect();
        let mut conn = acquire(&session.pool).await?;
        let stability = session
            .engine
            .stability(&mut conn, &session.ctx, &devices, now)
            .await
            .map_err(|error| format!("compute sync stability: {error}"))?;
        if stability
            .devices
            .iter()
            .all(|device| device.standing == DeviceStanding::Counted)
        {
            view.uncounted_devices.clear();
        } else {
            let names = super::devices::read_with(host.device_id()?, manager);
            view.set_device_standings(&stability.devices, &names);
        }
        Ok(stability.stable)
    }

    /// Drops operation bodies below `limit` within one bounded run, and asks for another pass
    /// when bodies below it remain.
    async fn compact<H: SyncHost>(
        session: &Session<'_, H>,
        limit: &VersionVector,
        next: Next,
    ) -> Result<Next, String> {
        let mut conn = acquire(&session.pool).await?;
        let report = session
            .engine
            .compact(&mut conn, &session.ctx, limit)
            .await
            .map_err(|error| format!("compact sync log: {error}"))?;
        Ok(match next {
            Next::Idle if report.more => Next::Continue,
            next => next,
        })
    }

    /// On a linked device, the compaction limit: the stable vector the hub last reported, with
    /// this installation's writers capped at the head the hub last confirmed.
    async fn client_compaction_limit<H: SyncHost>(
        &self,
        session: &Session<'_, H>,
    ) -> Result<VersionVector, String> {
        let stable = {
            let mut conn = acquire(&session.pool).await?;
            session
                .engine
                .stable_vector(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync stability: {error}"))?
        };
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        Ok(compaction_limit(&stable, &own_writers, self.confirmed))
    }

    /// Exchanges with the hub, re-sealing after a fork of this installation's chain.
    async fn exchange<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
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
                    if done.pulled > 0 {
                        self.assets.hub_changed();
                    }
                    {
                        let mut conn = acquire(&session.pool).await?;
                        session
                            .engine
                            .record_stable(&mut conn, &session.ctx, &done.stable)
                            .await
                            .map_err(|error| format!("record sync stability: {error}"))?;
                    }
                    if done.confirmed.is_some() {
                        self.confirmed = done.confirmed;
                    }
                    if done.bootstrap_required {
                        // Waiting would return at once, since the hub holds what this device
                        // cannot pull; local commits and the fallback interval still push.
                        view.state = SyncState::Error;
                        view.last_error = done.problem.or_else(|| {
                            Some("the hub no longer holds operations this device lacks".into())
                        });
                        view.last_error_code = Some(SyncErrorCode::BootstrapRequired);
                        return Ok(Next::Idle);
                    }
                    match done.problem {
                        Some(problem) => {
                            view.state = SyncState::Error;
                            view.last_error = Some(problem);
                            view.last_error_code = (done.refusal
                                == Some(SyncRefusalCode::WriterLimit))
                            .then_some(SyncErrorCode::WriterLimit);
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
                    self.applied = ApplyMark::default();
                }
            }
        }
        Err("sync fork recovery did not settle".to_string())
    }

    /// Uploads the managed assets this device holds and the hub lacks, ahead of the operations
    /// that reference them. Only a transport failure is an error; a file that could not be read
    /// is a problem in `transfer`.
    async fn upload_assets<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        peer: &AssetPeer,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        asset_session(session, peer)
            .upload(&mut self.assets, transfer)
            .await
    }

    /// Downloads the referenced managed assets this device lacks once operations were applied,
    /// then folds the outcome of both directions into the status and the next wait. Downloads
    /// run only after an exchange that reached the hub.
    async fn download_assets<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        peer: &AssetPeer,
        transfer: &mut AssetTransfer,
        next: Next,
        view: &mut SyncStatusView,
    ) -> Next {
        if !matches!(next, Next::Online(_) | Next::Continue) {
            return next;
        }
        let downloaded = asset_session(session, peer)
            .download(&mut self.assets, transfer)
            .await;
        if let Err(error) = downloaded {
            view.state = SyncState::Offline;
            view.last_error = Some(error);
            return Next::Retry;
        }
        if let Some(problem) = transfer.problem.take() {
            view.state = SyncState::Error;
            view.last_error = Some(problem);
        }
        match next {
            Next::Online(_) if transfer.more => Next::Continue,
            next => next,
        }
    }

    /// Applies stored operations once the log grew since the last apply, or local captures
    /// changed since an apply stopped on them. Apply always commits, so applying on every pass
    /// would wake the service again.
    async fn apply_new<H: SyncHost>(&mut self, session: &Session<'_, H>) -> Result<(), String> {
        let stored = {
            let mut conn = acquire(&session.pool).await?;
            session
                .engine
                .stored_vector(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync log: {error}"))?
        };
        let pending = session.pending_captures().await?;
        if !self.applied.due(&stored, pending) {
            return Ok(());
        }
        let applied = session.apply_ready(&mut self.writer).await?;
        session.report(applied.tables, &applied.notices).await;
        self.applied = ApplyMark {
            stored: Some(stored),
            blocked_at: applied.blocked_at,
        };
        Ok(())
    }

    fn apply_pause_request(&mut self, runtime: &RuntimeState) -> Result<(), String> {
        let Some(active) = self.writer.as_mut() else {
            return Ok(());
        };
        let request = runtime
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
    fn paused(&self, runtime: &RuntimeState) -> Result<bool, String> {
        let pending = *runtime
            .pause_request
            .lock()
            .map_err(|_| "sync pause lock is unavailable".to_string())?;
        Ok(pending.unwrap_or_else(|| {
            self.writer
                .as_ref()
                .is_some_and(|writer| writer.record().paused)
        }))
    }

    fn publish<H: SyncHost>(&mut self, host: &H, runtime: &RuntimeState, view: SyncStatusView) {
        if self.view == view {
            return;
        }
        if let Ok(mut status) = runtime.status.lock() {
            status.clone_from(&view);
        }
        host.emit_status(&view);
        self.view = view;
    }
}

/// Sets how the device reached its coordinator and why connectivity is degraded, from the
/// latest coordinator dial and the host's platform probes. A device without a role syncs with
/// nobody, so it reports neither.
async fn set_connectivity<H: SyncHost>(host: &H, view: &mut SyncStatusView) {
    let Some(role) = view.role else {
        view.connection = None;
        view.degraded.clear();
        return;
    };
    let dial = super::connectivity::dial_report(role, &host.pairing());
    view.connection = super::connectivity::connection(dial.as_ref());
    let mut reasons = DegradedReasons::default();
    super::connectivity::dial_reasons(dial.as_ref(), &mut reasons);
    host.note_platform_degraded(role, dial.as_ref(), &mut reasons)
        .await;
    view.degraded = reasons.into_vec();
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
pub(crate) fn quiesced_sync<H: SyncHost>(host: &H) -> Option<QuiescedSync> {
    let config_dir = host.config_dir().ok()?;
    let status = crate::vault::ownership::active_status_in(&config_dir, host.ownership()).ok()?;
    if !host.ownership().is_linked(&status.vault_id) {
        return None;
    }
    let access = if status.can_write {
        DatabaseAccessMode::ReadWrite
    } else if status.role == "read-only" {
        DatabaseAccessMode::Guarded
    } else {
        return None;
    };
    let path = crate::vault::active_database_path_in(&config_dir).ok()?;
    Some(QuiescedSync {
        vault_id: status.vault_id,
        path,
        access,
    })
}

/// Seals pending captures of a quiesced vault through a private pool, because the shared
/// connection gate is held. It waits for any pass of this process to finish first. Failures
/// are logged: the captures stay and seal later.
pub(crate) async fn seal_quiesced<H: SyncHost>(host: &H, plan: Option<QuiescedSync>) {
    let Some(plan) = plan else {
        return;
    };
    let _gate = PASS_GATE.lock().await;
    let registry = DatabasePoolRegistry::default();
    let result = seal_quiesced_with(host, &registry, &plan).await;
    if let Err(error) = result.and(registry.close_all().await) {
        eprintln!("seal sync changes before vault transition: {error}");
    }
}

async fn seal_quiesced_with<H: SyncHost>(
    host: &H,
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
    let Some(session) = Session::open(host, pool, &plan.vault_id, KeyRelease::Skip).await? else {
        return Ok(());
    };
    let mut writer = None;
    session.start(&mut writer).await?;
    // Nothing can undo a delete once the vault leaves this device, so held deletes seal now.
    {
        let mut conn = acquire(&session.pool).await?;
        hold::expire_holds(&mut conn)
            .await
            .map_err(|error| format!("end sync delete holds: {error}"))?;
    }
    session.seal_pending(&mut writer, &mut None).await
}

/// Whether a missing person key may be requested from the coordinator.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyRelease {
    Request,
    /// The vault is quiesced, so the request could not read the vault database.
    Skip,
}

/// One pass over an open vault pool with the space it replicates.
struct Session<'a, H: SyncHost> {
    host: &'a H,
    pool: SqlitePool,
    engine: Engine,
    files: SyncFiles,
    keys: Arc<dyn WriterKeyStore>,
    ctx: SpaceContext,
    key_release: KeyRelease,
}

impl<'a, H: SyncHost> Session<'a, H> {
    /// Opens a session, or returns `None` while the vault has no person identity.
    async fn open(
        host: &'a H,
        pool: SqlitePool,
        vault_id: &str,
        key_release: KeyRelease,
    ) -> Result<Option<Self>, String> {
        let coordinator = coordinator_device_id(host, vault_id)?;
        let ctx = {
            let mut conn = acquire(&pool).await?;
            local::space_context(&mut conn, vault_id, &coordinator)
                .await
                .map_err(|error| format!("read sync space: {error}"))?
        };
        let Some(ctx) = ctx else {
            return Ok(None);
        };
        Ok(Some(Self {
            host,
            pool,
            engine: host.engine()?,
            files: host.sync_files(vault_id)?,
            keys: host.key_store()?,
            ctx,
            key_release,
        }))
    }

    /// Tells the host which tables changed and what the registered hooks reported.
    async fn report(&self, tables: Vec<String>, notices: &[ChangeNotice]) {
        self.host.report_changes(tables, notices).await;
    }

    /// Tells the host what one seal changed.
    async fn report_seal(&self, seal: &SealReport) {
        self.report(table_names(&seal.changed_tables), &seal.notices)
            .await;
    }

    /// Start order: refuse Projects placeholder keys, clear a stale apply flag, seed captures of tables that start to replicate,
    /// import a carry bundle a failed replacement left, check the writer, and apply what the
    /// import could not apply before local captures were sealed.
    async fn start(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        ganbaru_projects::conversion::assert_converted(&self.pool).await?;
        let imported = {
            let mut conn = acquire(&self.pool).await?;
            local::reset_applying(&mut conn)
                .await
                .map_err(|error| format!("reset sync apply state: {error}"))?;
            self.engine
                .init_space(&mut conn, &self.files.vault_id, now_ms()?)
                .await
                .map_err(|error| format!("initialize sync space: {error}"))?;
            self.engine
                .seed_captures(&mut conn, now_ms()?)
                .await
                .map_err(|error| format!("seed sync captures: {error}"))?;
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
            let applied = self.apply_ready(writer).await?;
            self.report(applied.tables, &applied.notices).await;
        }
        Ok(())
    }

    /// Seals pending captures in bounded transactions until none is ready. A seal that leaves
    /// captures in place is not repeated until the number of captures changes or a delete
    /// hold expires.
    async fn seal_pending(
        &self,
        writer: &mut Option<ActiveWriter>,
        stalled: &mut Option<u64>,
    ) -> Result<(), String> {
        let pending = self.pending_captures().await?;
        let now = now_ms()?;
        let hold_expired = self.next_hold_expiry().await?.is_some_and(|at| at <= now);
        if pending == 0 || (*stalled == Some(pending) && !hold_expired) {
            if pending == 0 {
                *stalled = None;
            }
            return Ok(());
        }
        if !self.ensure_writer(writer).await? {
            return Ok(());
        }
        self.seal_batches(writer).await?;
        let remaining = self.pending_captures().await?;
        *stalled = (remaining > 0).then_some(remaining);
        Ok(())
    }

    /// Seals pending captures in bounded transactions until no more is ready.
    async fn seal_batches(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        let mut invalid = 0;
        for _ in 0..MAX_SEAL_ROUNDS {
            let report = self.seal(writer).await?;
            invalid = report.invalid_rows.len();
            if !report.more {
                break;
            }
        }
        if invalid > 0 {
            eprintln!("sync: {invalid} local rows cannot be sealed");
        }
        Ok(())
    }

    /// When the earliest delete hold expires, so a pass can seal it then.
    async fn next_hold_expiry(&self) -> Result<Option<u64>, String> {
        let mut conn = acquire(&self.pool).await?;
        hold::next_hold_expiry(&mut conn)
            .await
            .map_err(|error| format!("read sync delete holds: {error}"))
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
        let Some(person) = self.host.person_key(&self.ctx.anchor).await? else {
            if self.key_release == KeyRelease::Request {
                self.host.request_person_key();
            }
            return Ok(None);
        };
        let device_id = self.host.device_id()?;
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
                    self.report_seal(&report).await;
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

    /// Seals the writer's genesis on its first pass, and an acknowledgement or keepalive when
    /// one is due, rotating once when the space no longer accepts the writer. Without a writer
    /// it seals nothing.
    async fn acknowledge(&self, writer: &mut Option<ActiveWriter>) -> Result<SealReport, String> {
        for attempt in 0..2 {
            let Some(active) = writer.as_mut() else {
                break;
            };
            let result = {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .acknowledge(&mut conn, &self.ctx, &mut active.local(), now_ms()?)
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
                Err(error) => return Err(format!("seal sync acknowledgement: {error}")),
            }
        }
        Ok(SealReport::default())
    }

    /// Applies ready operations, re-sealing and rotating the local writer when the space
    /// requires it. An apply that stops on local captures it cannot seal in one transaction is
    /// retried after sealing them in batches, while those seals drain captures.
    async fn apply_ready(&self, writer: &mut Option<ActiveWriter>) -> Result<Applied, String> {
        let mut tables = BTreeSet::new();
        let mut notices = BTreeSet::new();
        let done =
            |tables: BTreeSet<String>, notices: BTreeSet<ChangeNotice>, blocked_at| Applied {
                tables: tables.into_iter().collect(),
                notices: notices.into_iter().collect(),
                blocked_at,
            };
        for _ in 0..MAX_APPLY_ROUNDS {
            let report = {
                let mut conn = acquire(&self.pool).await?;
                let mut local = writer.as_mut().map(ActiveWriter::local);
                self.engine
                    .apply(&mut conn, &self.ctx, local.as_mut(), now_ms()?)
                    .await
                    .map_err(|error| format!("apply sync operations: {error}"))?
            };
            tables.extend(table_names(&report.changed_tables));
            notices.extend(report.notices.iter().cloned());
            if let Some(seal) = report.seal.as_ref() {
                tables.extend(table_names(&seal.changed_tables));
                notices.extend(seal.notices.iter().cloned());
            }
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
                    return Ok(done(tables, notices, None));
                }
                let Some(active) = writer.as_mut() else {
                    return Ok(done(tables, notices, None));
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
                tables.extend(table_names(&resealed.seal.changed_tables));
                notices.extend(resealed.seal.notices);
            } else if report.local_writer_frozen {
                if !self.rotate(writer).await? {
                    return Ok(done(tables, notices, None));
                }
            } else if report.pending_captures {
                let before = self.pending_captures().await?;
                self.seal_batches(writer).await?;
                let after = self.pending_captures().await?;
                if after >= before {
                    return Ok(done(tables, notices, Some(after)));
                }
            } else if !report.more {
                return Ok(done(tables, notices, None));
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
        self.report_seal(&resealed.seal).await;
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
            self.report_seal(&report).await;
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
            super::recovery::restorers(),
            &request.choice,
            now_ms()?,
        )
        .await
    }
}

/// Device id of the coordinator whose writers may revoke in `vault_id`: the pinned coordinator
/// when this device is linked to one for that vault, otherwise this device, which then hosts
/// the hub or does not sync with others.
pub(crate) fn coordinator_device_id<H: SyncHost>(
    host: &H,
    vault_id: &str,
) -> Result<String, String> {
    let pin = host.pairing().coordinator_pin()?;
    match pin.filter(|pin| pin.vault_id == vault_id) {
        Some(pin) => Ok(pin.device_id),
        None => host.device_id(),
    }
}

/// Device-local sync files of a vault.
pub(crate) fn sync_files<H: SyncHost>(host: &H, vault_id: &str) -> Result<SyncFiles, String> {
    host.sync_files(vault_id)
}

/// Owned names of engine-reported domain tables.
fn table_names(tables: &[&str]) -> Vec<String> {
    tables.iter().map(|table| (*table).to_string()).collect()
}

async fn acquire(pool: &SqlitePool) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
    pool.acquire()
        .await
        .map_err(|error| format!("acquire sync connection: {error}"))
}

/// Runs key store and record work off the async runtime, because key stores may block.
pub(super) async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| format!("sync key worker: {error}"))?
}

#[cfg(all(test, desktop))]
mod headless_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync_contracts::WriterId;

    fn vector(seq: u64) -> VersionVector {
        let mut vector = VersionVector::new();
        vector.set(WriterId::from_bytes([1; 16]), seq);
        vector
    }

    #[test]
    fn apply_runs_once_per_log_growth() {
        let mut mark = ApplyMark::default();
        assert!(mark.due(&vector(1), 0), "the first pass applies");
        mark.stored = Some(vector(1));
        assert!(!mark.due(&vector(1), 0));
        assert!(
            !mark.due(&vector(1), 7),
            "captures alone do not rerun an apply that finished"
        );
        assert!(mark.due(&vector(2), 0), "a grown log applies again");
    }

    #[test]
    fn a_blocked_apply_reruns_when_captures_change_and_waits_while_they_stay() {
        let mark = ApplyMark {
            stored: Some(vector(1)),
            blocked_at: Some(5),
        };
        assert!(
            !mark.due(&vector(1), 5),
            "the same captures would block again"
        );
        assert!(mark.due(&vector(1), 2), "sealed captures may unblock it");
        assert!(mark.due(&vector(1), 9), "new local edits may be sealable");
        assert!(mark.due(&vector(2), 5), "a grown log applies again");
    }
}

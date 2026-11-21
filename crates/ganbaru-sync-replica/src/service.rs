//! The sync service of the active vault: it imports carried operations, keeps the local writer
//! current, seals local captures after commits, and exchanges operations and managed asset
//! files with the hub. On the coordinator, the anchor of the vault's sync space, it founds the
//! space at first link, applies what linked devices push, and enrolls and removes devices as
//! they are linked and unlinked.
//!
//! A pass runs through a [`SyncHost`], so the app service and a headless pass share it. Passes
//! of one process run one at a time behind a gate, and a headless pass hands its work to the
//! app service when one is running.

#[cfg(desktop)]
mod anchor;
pub mod asset_collection;
pub mod connectivity;
pub mod devices;
pub mod dial_report;
pub mod discovery_keys;
mod exchange;
pub mod host;
mod hubs;
mod local_writer;
mod membership;
pub mod peers;
mod relay_pass;
mod server;
pub mod status;
pub mod transport;

pub use crate::anchor_setup::ServerEnrollment;
#[cfg(desktop)]
pub use anchor::{AuthorityAction, AuthorityAnswer, ForceRequest, LateDecision};
#[cfg(desktop)]
use anchor::{AuthorityRequest, ForwardRequest};
pub use host::{SYNC_DIRECTORY, SyncHost};
#[cfg(desktop)]
pub use hubs::{log_admission_source, quarantined_devices, server_hub, sync_hub};
pub use hubs::{member_admission_source, member_hub};
use status::{DegradedReasons, SyncPhase, SyncRole, SyncState, SyncStatusView};
use transport::PairedTransport;

use crate::assets::{AssetMemory, AssetStore, AssetTransfer};
#[cfg(desktop)]
use crate::authority::{self, ensure_admin_key as ensure_stored_admin_key, read_admin_key};
use crate::carry_forward;
use crate::client::{PeerIds, SyncTransport};
#[cfg(desktop)]
use crate::hub::HubStability;
use crate::now_ms;
use crate::outbox::Outbox;
use crate::quarantine::{PullSkips, Quarantine};
use crate::records::{FreeSpace, RecordExchange};
use crate::recovery::RecoveryChoice;
use crate::reset;
#[cfg(desktop)]
use crate::writer::{RestorePhase, SequencedCheck, restore_phase};
use crate::writer::{
    ActiveWriter, SyncFiles, SyncKeyStore, ensure_device_key as ensure_stored_device_key,
};
use ganbaru_db::DatabasePoolRegistry;
use ganbaru_handoff::pairing::PairingManager;
#[cfg(desktop)]
use ganbaru_handoff::pairing::PinnedCharter;
use ganbaru_sync::{ChangeNotice, Engine, SealReport, SpaceContext, compaction_limit, hold, local};
use ganbaru_sync_contracts::{DevicePublicKey, Seq, VersionVector, WriterId};
#[cfg(desktop)]
use ganbaru_sync_contracts::{Digest32, ProposalRejection, SignedProposal};
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::future::Future;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
use tokio::sync::{Notify, oneshot, watch};

/// A recovery action waiting for the service, which owns the local writer.
pub struct RecoveryRequest {
    pub choice: RecoveryChoice,
    /// Receives the id of the restored row, if any.
    pub reply: oneshot::Sender<Result<Option<String>, String>>,
}

/// The vault and device ids a sync exchange presents, or `None` on a device that is not linked
/// to a coordinator.
pub fn sync_peer(manager: &PairingManager) -> Result<Option<(String, String)>, String> {
    let Some(coordinator) = manager.coordinator_pin()? else {
        return Ok(None);
    };
    let device_id = manager.device_id()?;
    Ok(Some((coordinator.vault_id, device_id)))
}

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
/// How long a headless pass or a device enrollment waits for the running app service to finish
/// the pass it asked for.
const DELEGATED_PASS_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound on apply rounds in one pass; each round is one bounded transaction.
const MAX_APPLY_ROUNDS: usize = 10_000;
/// Bound on seal rounds in one pass; each round is one bounded transaction.
const MAX_SEAL_ROUNDS: usize = 10_000;
/// Fork recoveries one pass may run before it reports an error.
const MAX_FORK_RECOVERIES: usize = 3;
/// How long an anchor whose database was restored behind its own log waits for linked devices
/// to push back the entries it lost before it hands over to itself, which supersedes them.
#[cfg(desktop)]
const RESTORE_CATCH_UP_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Whether relay and server dials reach loopback: debug builds allow a server on this computer for
/// development, release builds refuse it.
pub const RELAY_LOOPBACK: ganbaru_handoff::dial::LoopbackPolicy = if cfg!(debug_assertions) {
    ganbaru_handoff::dial::LoopbackPolicy::Allow
} else {
    ganbaru_handoff::dial::LoopbackPolicy::Reject
};

/// Lets one sync pass of this process run at a time, whether the app service, a headless pass,
/// or the seal before a vault transition runs it.
static PASS_GATE: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The service state of the running app, so a headless pass can hand its work to it instead
/// of running a second pass. Cleared when the app exits.
static LIVE_RUNTIME: Mutex<Weak<RuntimeState>> = Mutex::new(Weak::new());

/// Managed state of the sync service.
#[derive(Clone, Default)]
pub struct SyncRuntime(Arc<RuntimeState>);

impl Deref for SyncRuntime {
    type Target = RuntimeState;

    fn deref(&self) -> &RuntimeState {
        &self.0
    }
}

/// What the sync service and its commands share.
pub struct RuntimeState {
    /// Signaled to run a pass now.
    now: Arc<Notify>,
    /// Bumped after each pass, so hub waits recheck the log.
    changes: watch::Sender<u64>,
    /// Record exchange windows, cursors, and backstop that this device's hubs and sessions
    /// share.
    records: RecordExchange,
    /// The root of the vault the service last ran for, whose volume the record backstop reads
    /// free space from.
    vault_root: Arc<Mutex<Option<PathBuf>>>,
    /// Stability the coordinator pass last computed, reported in every sync state.
    #[cfg(desktop)]
    stability: HubStability,
    /// The charter of the space this device anchors and its person key, as the last pass read
    /// them, for the pairing reply.
    #[cfg(desktop)]
    pairing_charter: Mutex<Option<PinnedCharter>>,
    /// Proposals linked devices sent, waiting for the anchor pass.
    #[cfg(desktop)]
    proposals: Mutex<Vec<ProposalRequest>>,
    /// Forward requests of a linked desktop that anchors, waiting for the next pass.
    #[cfg(desktop)]
    forwards: Mutex<Vec<ForwardRequest>>,
    /// Authority actions a person asked this desktop for, waiting for the next pass.
    #[cfg(desktop)]
    authority: Mutex<Vec<AuthorityRequest>>,
    /// A pause change waiting for the writer record.
    pause_request: Mutex<Option<bool>>,
    /// Whether the app is in the foreground, which a phone's member listener requires. False
    /// until the frontend reports it is visible.
    foreground: std::sync::atomic::AtomicBool,
    /// Recovery actions waiting for the next pass.
    recovery: Mutex<Vec<RecoveryRequest>>,
    /// Reset sync state requests waiting for the next pass, with where each answer goes.
    resets: Mutex<Vec<ResetReply>>,
    status: Mutex<SyncStatusView>,
    task: Mutex<Option<ServiceTask>>,
    /// Number of the latest pass that started.
    passes_started: AtomicU64,
    /// Number of the latest pass that finished.
    passes_done: watch::Sender<u64>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        let vault_root: Arc<Mutex<Option<PathBuf>>> = Arc::default();
        let root = Arc::clone(&vault_root);
        let free_space: FreeSpace = Arc::new(move || {
            let root = root.lock().ok()?.clone()?;
            volume_free_bytes(&root)
        });
        Self {
            now: Arc::default(),
            changes: watch::Sender::new(0),
            records: RecordExchange::default().with_free_space(free_space),
            vault_root,
            #[cfg(desktop)]
            stability: HubStability::default(),
            #[cfg(desktop)]
            pairing_charter: Mutex::default(),
            #[cfg(desktop)]
            proposals: Mutex::default(),
            #[cfg(desktop)]
            forwards: Mutex::default(),
            #[cfg(desktop)]
            authority: Mutex::default(),
            pause_request: Mutex::default(),
            foreground: std::sync::atomic::AtomicBool::new(false),
            recovery: Mutex::default(),
            resets: Mutex::default(),
            status: Mutex::default(),
            task: Mutex::default(),
            passes_started: AtomicU64::new(0),
            passes_done: watch::Sender::new(0),
        }
    }
}

/// Bytes an unprivileged process may still write on the volume holding `path`, or `None` when
/// the volume cannot be read. The record backstop then applies its byte and envelope bounds
/// alone.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn volume_free_bytes(path: &Path) -> Option<u64> {
    let stat = rustix::fs::statvfs(path).ok()?;
    Some(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// Free space is read on Linux and Android only; elsewhere the record backstop applies its
/// byte and envelope bounds alone.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn volume_free_bytes(_path: &Path) -> Option<u64> {
    None
}

impl RuntimeState {
    /// The latest status of the active vault.
    pub fn status(&self) -> Result<SyncStatusView, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|_| "sync status lock is unavailable".to_string())
    }

    /// Runs a pass now instead of waiting for a commit or the fallback interval.
    pub fn sync_now(&self) {
        self.now.notify_one();
    }

    /// The authorization record exchange every pass shares, with its storage backstop.
    pub fn records(&self) -> &RecordExchange {
        &self.records
    }

    /// Points the record backstop's free space reading at the volume of `root`.
    fn set_vault_root(&self, root: PathBuf) {
        if let Ok(mut current) = self.vault_root.lock() {
            *current = Some(root);
        }
    }

    /// Records whether the app is in the foreground. A pass serves the member listener only
    /// while it is.
    pub fn set_foreground(&self, visible: bool) {
        self.foreground.store(visible, Ordering::SeqCst);
    }

    /// Whether the app is in the foreground.
    pub fn in_foreground(&self) -> bool {
        self.foreground.load(Ordering::SeqCst)
    }

    /// Pauses or resumes exchanges on this device. The writer record keeps the choice.
    pub fn set_paused(&self, paused: bool) -> Result<(), String> {
        *self
            .pause_request
            .lock()
            .map_err(|_| "sync pause lock is unavailable".to_string())? = Some(paused);
        self.now.notify_one();
        Ok(())
    }

    /// Queues a recovery action for the service and wakes it. The receiver gets the id of a
    /// restored row.
    pub fn request_recovery(
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

    /// Records the charter the pairing reply carries, or none when this device anchors no space.
    #[cfg(desktop)]
    fn set_pairing_charter(&self, charter: Option<PinnedCharter>) -> Result<(), String> {
        *self
            .pairing_charter
            .lock()
            .map_err(|_| "sync pairing charter lock is unavailable".to_string())? = charter;
        Ok(())
    }

    /// The charter the pairing reply carries, as the last pass read it.
    #[cfg(desktop)]
    fn pairing_charter(&self) -> Result<Option<PinnedCharter>, String> {
        self.pairing_charter
            .lock()
            .map(|charter| charter.clone())
            .map_err(|_| "sync pairing charter lock is unavailable".to_string())
    }

    /// Queues a proposal a linked device sent for the anchor pass and wakes the service.
    #[cfg(desktop)]
    fn request_proposal(
        &self,
        proposal: SignedProposal,
    ) -> Result<oneshot::Receiver<ProposalOutcome>, String> {
        let (reply, receiver) = oneshot::channel();
        self.proposals
            .lock()
            .map_err(|_| "sync proposal lock is unavailable".to_string())?
            .push(ProposalRequest { proposal, reply });
        self.now.notify_one();
        Ok(receiver)
    }

    #[cfg(desktop)]
    fn take_proposal_requests(&self) -> Vec<ProposalRequest> {
        self.proposals
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    /// Queues a forward request of a linked device for the next pass and wakes the service.
    #[cfg(desktop)]
    fn request_forward(
        &self,
        device_id: String,
        settled: Vec<(Digest32, ProposalRejection)>,
    ) -> Result<oneshot::Receiver<Result<Vec<SignedProposal>, String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.forwards
            .lock()
            .map_err(|_| "sync forward lock is unavailable".to_string())?
            .push(ForwardRequest {
                device_id,
                settled,
                reply,
            });
        self.now.notify_one();
        Ok(receiver)
    }

    #[cfg(desktop)]
    fn take_forward_requests(&self) -> Vec<ForwardRequest> {
        self.forwards
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    /// Queues an authority action for the next pass and wakes the service. The receiver gets
    /// what the action produced.
    #[cfg(desktop)]
    pub fn request_authority(
        &self,
        action: AuthorityAction,
    ) -> Result<oneshot::Receiver<Result<AuthorityAnswer, String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.queue_authority(AuthorityRequest {
            action,
            reply: Some(reply),
        })?;
        Ok(receiver)
    }

    /// Queues an authority action nobody waits for, such as the removal proposal that
    /// follows a quarantine.
    #[cfg(desktop)]
    pub fn request_authority_detached(&self, action: AuthorityAction) -> Result<(), String> {
        self.queue_authority(AuthorityRequest {
            action,
            reply: None,
        })
    }

    #[cfg(desktop)]
    fn queue_authority(&self, request: AuthorityRequest) -> Result<(), String> {
        self.authority
            .lock()
            .map_err(|_| "sync authority lock is unavailable".to_string())?
            .push(request);
        self.now.notify_one();
        Ok(())
    }

    #[cfg(desktop)]
    fn take_authority_requests(&self) -> Vec<AuthorityRequest> {
        self.authority
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    /// Queues Reset sync state for the service and wakes it. The receiver gets the outcome.
    #[cfg(desktop)]
    pub fn request_reset(&self) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        let (reply, receiver) = oneshot::channel();
        self.resets
            .lock()
            .map_err(|_| "sync reset lock is unavailable".to_string())?
            .push(reply);
        self.now.notify_one();
        Ok(receiver)
    }

    fn take_reset_requests(&self) -> Vec<ResetReply> {
        self.resets
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    fn take_recovery_requests(&self) -> Vec<RecoveryRequest> {
        self.recovery
            .lock()
            .map(|mut requests| std::mem::take(&mut *requests))
            .unwrap_or_default()
    }

    /// Whether the service task is running.
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

/// The anchor's answer to a proposal: the revision that holds it or why it was rejected, or why
/// the pass could not answer.
#[cfg(desktop)]
type ProposalOutcome = Result<Result<u64, ProposalRejection>, String>;

/// A proposal a linked device sent through the hub, with where its answer goes.
#[cfg(desktop)]
struct ProposalRequest {
    proposal: SignedProposal,
    reply: oneshot::Sender<ProposalOutcome>,
}

/// Where the outcome of one Reset sync state request goes.
type ResetReply = oneshot::Sender<Result<(), String>>;

/// The running service task and how to stop it.
pub struct ServiceTask {
    stop: Arc<Notify>,
    handle: tokio::task::JoinHandle<()>,
}

impl ServiceTask {
    /// A task that stops when `stop` is notified.
    pub fn new(stop: Arc<Notify>, handle: tokio::task::JoinHandle<()>) -> Self {
        Self { stop, handle }
    }
}

/// Runtime internals that headless pass tests of a host drive.
#[cfg(feature = "test-support")]
pub mod test_support {
    use super::{RuntimeState, ServiceState, ServiceTask, SyncHost, SyncRuntime, SyncStatusView};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Weak};
    use tokio::sync::Notify;

    /// A live runtime whose task finishes each requested pass, or ignores requests when
    /// `answer` is false.
    pub fn live_runtime(answer: bool) -> SyncRuntime {
        let runtime = SyncRuntime::default();
        let worker = runtime.clone();
        let stop = Arc::new(Notify::new());
        let stopped = Arc::clone(&stop);
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = stopped.notified() => return,
                    () = worker.now.notified() => {}
                }
                if answer {
                    let number = worker.pass_started();
                    worker.pass_done(number);
                }
            }
        });
        *runtime.task.lock().expect("task lock") = Some(ServiceTask { stop, handle });
        runtime
    }

    /// The state of `runtime` as a headless pass finds it registered.
    pub fn registered(runtime: &SyncRuntime) -> Weak<RuntimeState> {
        Arc::downgrade(&runtime.0)
    }

    /// How many passes `runtime` started.
    pub fn passes_started(runtime: &SyncRuntime) -> u64 {
        runtime.passes_started.load(Ordering::SeqCst)
    }

    /// Runs one pass of `host` with the requests queued on `runtime`, and returns the status the
    /// pass left.
    pub async fn run_pass<H: SyncHost>(
        host: &H,
        runtime: &SyncRuntime,
    ) -> Result<SyncStatusView, String> {
        let gate = tokio::sync::Mutex::new(());
        ServiceState::for_runtime(runtime)
            .pass(host, runtime, &gate)
            .await;
        runtime.status()
    }
}

/// What starts the service: the notice the vault database signals after each commit, and the
/// runtime the service task is spawned on.
#[derive(Clone)]
pub struct ServiceStart {
    pub commits: Arc<Notify>,
    pub spawner: tokio::runtime::Handle,
}

/// Registers `runtime` as the runtime a headless pass hands its work to, and starts the
/// service. Runs once at startup, before the frontend opens the vault. The vault connection
/// hook is installed with the process pool registry.
pub fn setup<H: SyncHost>(host: &H, runtime: &SyncRuntime, start_with: &ServiceStart) {
    match LIVE_RUNTIME.lock() {
        Ok(mut live) => *live = Arc::downgrade(&runtime.0),
        Err(_) => eprintln!("register sync runtime: the runtime lock is unavailable"),
    }
    if let Err(error) = start(host, runtime, start_with) {
        eprintln!("start sync service: {error}");
    }
}

/// Forgets the running app service, so a later headless pass in this process runs its own
/// pass. Runs when the app exits.
pub fn release_runtime_handle() {
    match LIVE_RUNTIME.lock() {
        Ok(mut live) => *live = Weak::new(),
        Err(_) => eprintln!("release sync runtime: the runtime lock is unavailable"),
    }
}

/// Stops the service before a vault transition, letting a running pass finish first. A pass
/// of this process that does not finish in time fails the stop, which keeps the vault.
pub async fn stop_for_vault_handoff<H: SyncHost>(
    host: &H,
    runtime: &SyncRuntime,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
    let task = runtime
        .task
        .lock()
        .map_err(|_| "sync service lock is unavailable".to_string())?
        .take();
    host.stop_serving_members().await;
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
pub fn resume_after_vault_handoff<H: SyncHost>(
    host: &H,
    runtime: &SyncRuntime,
    start_with: &ServiceStart,
) -> Result<(), String> {
    start(host, runtime, start_with)
}

fn start<H: SyncHost>(
    host: &H,
    runtime: &SyncRuntime,
    start_with: &ServiceStart,
) -> Result<(), String> {
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
        commits: start_with.commits.clone(),
        now: runtime.now.clone(),
    };
    let handle = start_with
        .spawner
        .spawn(run(host.clone(), runtime.clone(), signals));
    *task = Some(ServiceTask { stop, handle });
    Ok(())
}

/// What a headless pass did.
pub struct HeadlessPass {
    /// The status after the pass.
    pub status: SyncStatusView,
    /// The running app service ran the pass.
    pub delegated: bool,
    /// An exchange stopped at its byte bound, so another pass would continue it.
    pub more: bool,
}

/// Runs one pass with the host `open` builds without a Tauri runtime: it seals, exchanges with
/// the hub, applies, and moves managed assets, then returns without waiting for the hub. When
/// the app service of this process is running, it runs the pass instead.
pub async fn headless_pass<H, F>(open: F) -> Result<HeadlessPass, String>
where
    H: SyncHost,
    F: Future<Output = Result<H, String>>,
{
    headless_pass_with(open, &LIVE_RUNTIME, &PASS_GATE, DELEGATED_PASS_TIMEOUT).await
}

/// Runs one headless pass with the host `open` builds, or asks the runtime registered in `live`
/// to run it and waits up to `wait` for it. The host is opened only when no runtime runs.
pub async fn headless_pass_with<H, F>(
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
    let next = ServiceState::for_runtime(&runtime)
        .pass(&host, &runtime, gate)
        .await;
    Ok(HeadlessPass {
        status: runtime.status()?,
        delegated: false,
        more: matches!(next, Next::Continue),
    })
}

/// Asks the running service for a pass and waits up to `wait` for one that started after the
/// request to finish. It never runs a pass itself, so a slow service fails the call.
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
    /// The coordinator's stored vector from the exchange, without writers of devices this
    /// installation quarantined.
    #[cfg(desktop)]
    hub: VersionVector,
    /// The anchor the long poll waits on instead of the coordinator, while the coordinator is
    /// away and an anchor that is not the coordinator answered a complete exchange. The status
    /// stays offline, and the coordinator is dialed again at the fallback interval.
    poll: Option<transport::MemberTransport>,
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
    /// request, or `hold` elapses. A wait the hub answers without news is repeated. A wait on
    /// the anchor while the coordinator is away also ends at the fallback interval, so the
    /// coordinator is dialed again.
    async fn online(&self, wait: &OnlineWait, hold: Option<Duration>) -> Wake {
        let hold = match wait.poll {
            Some(_) => Some(capped(FALLBACK_INTERVAL, hold)),
            None => hold,
        };
        let hold_expiry = hold.map(|delay| tokio::time::Instant::now() + delay);
        loop {
            let poll = async {
                match &wait.poll {
                    Some(anchor) => crate::client::wait(anchor, &wait.ids, &wait.known).await,
                    None => crate::client::wait(&wait.transport, &wait.ids, &wait.known).await,
                }
            };
            let result = tokio::select! {
                () = self.stop.notified() => return Wake::Stop,
                () = self.commits.notified() => return Wake::Commit,
                () = self.now.notified() => return Wake::Pass,
                () = sleep_until(hold_expiry) => return Wake::Pass,
                result = poll => result,
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
    let mut state = ServiceState::for_runtime(&runtime);
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

/// Whether a linked device reached the anchor in this pass: it anchors itself, the anchor is
/// the coordinator and the coordinator answered, or a direct exchange with the anchor
/// completed. A space without a log has no anchor to reach.
fn anchor_reachable(
    anchor: Option<&str>,
    own_device_id: &str,
    coordinator: Option<&str>,
    coordinator_answered: bool,
    anchor_answered: bool,
) -> bool {
    let Some(anchor) = anchor else {
        return true;
    };
    anchor == own_device_id
        || anchor_answered
        || (coordinator_answered && coordinator == Some(anchor))
}

/// `delay`, shortened to `limit` when one is given.
fn capped(delay: Duration, limit: Option<Duration>) -> Duration {
    limit.map_or(delay, |limit| delay.min(limit))
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
    /// Captures a seal left in place, which only a new local change or an expired delete hold
    /// can make sealable.
    stalled_captures: Option<u64>,
    /// When the earliest delete hold expires, in Unix milliseconds; a pass runs then.
    hold_expiry: Option<u64>,
    /// When the last apply ran, so the next one waits for something to change.
    applied: ApplyMark,
    /// Which managed assets the hub holds or lacks, so passes do not ask again.
    assets: AssetMemory,
    /// Which managed assets an anchor server holds or lacks.
    server_assets: AssetMemory,
    /// Whether the anchor server's protocol was checked since it last failed to answer.
    server_version_checked: bool,
    /// The head of the active writer the hub last confirmed holding with the same hash, which
    /// bounds compaction of this installation's own writers.
    confirmed: Option<(WriterId, Seq)>,
    /// Operations of quarantined devices pulls already turned away.
    pull_skips: PullSkips,
    /// A link to an anchor that is not the coordinator and answered the last exchange, which
    /// the pass sends waiting proposals to instead of the coordinator.
    anchor_link: Option<transport::MemberTransport>,
    /// Final rejections this desktop gave proposals an anchor server forwarded, which the next
    /// forward request reports to the server.
    #[cfg(desktop)]
    server_settled: Vec<(Digest32, ProposalRejection)>,
    /// Final rejections this desktop, anchoring as a linked device, gave forwarded proposals,
    /// which the next forward request reports to the coordinator.
    #[cfg(desktop)]
    settled: Vec<(Digest32, ProposalRejection)>,
    /// The stored vector the coordinator last reported, quarantined writers included. A
    /// linked anchor's removal cutoffs cover at least it, since the coordinator and members
    /// may hold operations of a device the anchor quarantined and never stored.
    #[cfg(desktop)]
    hub_reported: VersionVector,
    view: SyncStatusView,
    /// Record exchange state shared with this device's hubs.
    records: RecordExchange,
}

impl ServiceState {
    /// Fresh state that shares the record exchange state of `runtime`.
    fn for_runtime(runtime: &RuntimeState) -> Self {
        Self {
            records: runtime.records.clone(),
            ..Self::default()
        }
    }

    /// Starts over for the vault `vault_id` at `root`: per-vault state resets, the record
    /// exchange state stays shared with `runtime` and forgets the previous vault's cursors,
    /// windows, and backstop, and the backstop reads free space of the volume of `root`.
    fn enter_vault(&mut self, vault_id: String, runtime: &RuntimeState, root: PathBuf) {
        *self = Self {
            vault_id: Some(vault_id),
            ..Self::for_runtime(runtime)
        };
        runtime.records.reset_vault();
        runtime.set_vault_root(root);
        #[cfg(desktop)]
        runtime.stability.clear();
    }
}

/// When the service last applied stored operations.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ApplyMark {
    /// The stored vector at the last apply.
    stored: Option<VersionVector>,
    /// Envelopes in the authorization record store at the last apply.
    records: u64,
    /// Local captures left when the last apply stopped on captures that sealing did not drain.
    blocked_at: Option<u64>,
}

impl ApplyMark {
    /// Whether an apply is due: the log grew, the record store changed, which can move the
    /// selection and its holds with no new operation, or local captures changed since an apply
    /// stopped on them. A capture count that stays the same cannot unblock it.
    fn due(&self, stored: &VersionVector, records: u64, pending: u64) -> bool {
        self.stored.as_ref() != Some(stored)
            || self.records != records
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

    /// Whether this installation may sequence for the space, following [`RestorePhase`]. An
    /// anchor whose database holds less of the log than its writer record says it sealed was
    /// restored from a backup or copied: it moves to a successor writer at once, so nothing it
    /// seals reuses a sequence of the lost entries members may hold, and sequences nothing while
    /// members push back what it lost. Once the catch-up window, counted from the restore the
    /// writer record keeps, passes, or at once when `members_reachable` is false, it seals a
    /// handover to itself so the next epoch supersedes the entries it never got back. The
    /// coordinator passes whether it has linked devices; a linked desktop that anchors always
    /// has the coordinator. A device that is not the anchor is always ready, since it sequences
    /// nothing.
    #[cfg(desktop)]
    async fn anchor_ready(
        &mut self,
        session: &Session<'_, impl SyncHost>,
        manager: &PairingManager,
        members_reachable: bool,
    ) -> Result<bool, String> {
        if self.writer.is_none() || !session.anchors_here(manager).await? {
            return Ok(true);
        }
        let window = u64::try_from(RESTORE_CATCH_UP_WINDOW.as_millis()).unwrap_or(u64::MAX);
        let mut rotated = false;
        let (recorded, now) = loop {
            let Some(active) = self.writer.as_ref() else {
                return Ok(false);
            };
            let check = {
                let mut conn = acquire(&session.pool).await?;
                authority::restore_check(&session.engine, &mut conn, &session.ctx, active.record())
                    .await?
            };
            let now = now_ms()?;
            let sequenced = active.record().sequenced.as_ref();
            match restore_phase(check, sequenced, now, members_reachable, window) {
                RestorePhase::Ready => return Ok(true),
                // The writer check retires a writer whose database is behind its log; a restore
                // found while the writer was already active retires it here, once.
                RestorePhase::Rotate { .. } if !rotated => {
                    if !session.rotate_restored(&mut self.writer, now).await? {
                        return Ok(false);
                    }
                    rotated = true;
                }
                RestorePhase::Rotate { .. } => {
                    return Err("the restored anchor's successor lost its restore".to_string());
                }
                RestorePhase::CatchingUp { recorded, .. } => {
                    eprintln!(
                        "sync: the database was restored behind authorization revision \
                         {recorded}; waiting for linked devices to return the log"
                    );
                    return Ok(false);
                }
                RestorePhase::HandOver { recorded } => break (recorded, now),
            }
        };
        let Some(active) = self.writer.as_mut() else {
            return Ok(false);
        };
        let since = active
            .record()
            .sequenced
            .as_ref()
            .and_then(|entry| entry.restored_at_ms)
            .unwrap_or(now);
        let mut conn = acquire(&session.pool).await?;
        let head = session
            .engine
            .authorization(&mut conn, &session.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .map_or(0, |view| view.revision);
        if head == 0 {
            // A handover continues from an entry, so a log holding only its charter stays
            // unsequenced while content keeps syncing; linking the devices again founds anew.
            eprintln!(
                "sync: the database was restored to its bare charter behind authorization \
                 revision {recorded}; link the devices again to sequence authorization"
            );
            return Ok(false);
        }
        // Only the admin key: the recovery seed may be gone once the user saved the kit.
        let admin = session.admin_key().await?;
        let revision = authority::seal_handover_here(
            &session.engine,
            &mut conn,
            &session.ctx,
            &admin,
            &mut active.local(),
            None,
            now,
        )
        .await?
        .map_err(|rejection| format!("hand the restored anchor over to itself: {rejection}"))?;
        drop(conn);
        session.record_commit(active).await?;
        eprintln!(
            "sync: the database was restored behind authorization revision {recorded}; \
             the anchor continued the log in a new epoch at revision {revision}"
        );
        // A record of a lost epoch the handover only reached can still name a later revision;
        // the next pass then seals one more handover, whose epoch passes it. The handover's
        // entry replaced the restored one, so the writer is replaced again with the window it
        // already waited out.
        let mut conn = acquire(&session.pool).await?;
        let check =
            authority::restore_check(&session.engine, &mut conn, &session.ctx, active.record())
                .await?;
        drop(conn);
        if !matches!(check, SequencedCheck::Restored { .. }) {
            return Ok(true);
        }
        session.rotate_restored(&mut self.writer, since).await?;
        Ok(false)
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
        let mut view = SyncStatusView::default();
        view.last_exchange_at_ms = self.view.last_exchange_at_ms;
        view.anchor_unreachable_since_ms = self.view.anchor_unreachable_since_ms;
        view.connection = self.view.connection.clone();
        view.degraded = self.view.degraded.clone();
        let mut recovery = runtime.take_recovery_requests();
        let mut resets = runtime.take_reset_requests();
        let mut succeeded = false;
        let next = match self
            .pass_inner(host, runtime, &mut view, &mut recovery, &mut resets)
            .await
        {
            Ok(next) => {
                succeeded = true;
                next
            }
            Err(error) => {
                eprintln!("sync: {error}");
                host.recover_lost_keys().await;
                view.set_phase(SyncPhase::error(error, None));
                Next::Retry
            }
        };
        set_connectivity(host, &mut view).await;
        for request in recovery {
            let _ = request
                .reply
                .send(Err("sync is not ready on this device".to_string()));
        }
        for reply in resets {
            let _ = reply.send(Err("sync is not ready on this device".to_string()));
        }
        // A pass that ended before the anchor step leaves hub proposal, forward, and authority
        // requests queued; each is answered so no one waits, and a sender keeps its proposal in
        // its outbox.
        #[cfg(desktop)]
        {
            for request in runtime.take_proposal_requests() {
                let _ = request
                    .reply
                    .send(Err("sync is not ready on this device".to_string()));
            }
            for request in runtime.take_forward_requests() {
                let _ = request
                    .reply
                    .send(Err("sync is not ready on this device".to_string()));
            }
            for request in runtime.take_authority_requests() {
                request.answer(Err("sync is not ready on this device".to_string()));
            }
        }
        self.publish(host, runtime, view);
        runtime
            .changes
            .send_modify(|generation| *generation = generation.wrapping_add(1));
        runtime.pass_done(number);
        if succeeded {
            // Compaction ran in the pass, so retained versions no longer hold what it dropped.
            asset_collection::collect_if_due(host).await;
        }
        next
    }

    async fn pass_inner<H: SyncHost>(
        &mut self,
        host: &H,
        runtime: &RuntimeState,
        view: &mut SyncStatusView,
        recovery: &mut Vec<RecoveryRequest>,
        resets: &mut Vec<ResetReply>,
    ) -> Result<Next, String> {
        if !host.sync_enabled()? {
            host.stop_serving_members().await;
            return Ok(Next::Idle);
        }
        let pool = host.connect_for_sync().await?;
        let vault_id = host.active_vault_id()?;
        if self.vault_id.as_deref() != Some(vault_id.as_str()) {
            self.enter_vault(vault_id.clone(), runtime, host.active_vault_path()?);
        }
        let manager = host.pairing();
        let peer = sync_peer(&manager)?.filter(|(linked_vault, _)| *linked_vault == vault_id);
        view.role = Some(if peer.is_some() {
            SyncRole::Client
        } else {
            SyncRole::Hub
        });
        if !resets.is_empty() {
            let outcome = if peer.is_some() {
                Err("only the coordinating computer can reset sync state".to_string())
            } else {
                self.reset_sync_state(host, &pool, &manager, &vault_id)
                    .await
            };
            if outcome.is_ok() {
                #[cfg(desktop)]
                {
                    runtime.stability.clear();
                    runtime.set_pairing_charter(None)?;
                }
            }
            for reply in resets.drain(..) {
                let _ = reply.send(outcome.clone());
            }
        }
        let session =
            match Session::open(host, pool.clone(), &vault_id, KeyRelease::Request).await? {
                Some(session) => Some(session),
                None if host.create_identity(&pool).await? => {
                    Session::open(host, pool, &vault_id, KeyRelease::Request).await?
                }
                None => None,
            };
        let Some(session) = session else {
            view.set_phase(SyncPhase::WaitingForIdentity);
            return Ok(Next::Idle);
        };
        if peer.is_none() {
            let charter = session.found_space(&manager).await?;
            #[cfg(desktop)]
            runtime.set_pairing_charter(charter)?;
            #[cfg(mobile)]
            let _ = charter;
        } else {
            #[cfg(desktop)]
            runtime.set_pairing_charter(None)?;
            session.install_pinned_charter(&manager).await?;
        }
        session.follow_discovery_keys().await?;
        if !self.started {
            session.start(&mut self.writer).await?;
            self.started = true;
        }
        session
            .seal_pending(&mut self.writer, &mut self.stalled_captures)
            .await?;
        self.hold_expiry = session.next_hold_expiry().await?;
        view.set_phase(SyncPhase::Idle);
        if !session.ensure_writer(&mut self.writer).await? {
            view.set_phase(SyncPhase::WaitingForIdentity);
        }
        self.apply_pause_request(runtime)?;
        view.paused = self.paused(runtime)?;
        let mut next = Next::Idle;
        let is_hub = peer.is_none();
        let mut asset_peer = None;
        // The coordinator ids of this pass, also when only record exchange reaches it.
        let mut linked_ids = None;
        match peer {
            None => {}
            Some(_) if view.paused => view.set_phase(SyncPhase::Paused),
            Some(_) if session.coordinator_quarantined(&manager).await? => {
                view.set_phase(SyncPhase::offline(
                    "this device quarantined the coordinator",
                ));
            }
            Some((linked_vault, device_id)) if session.coordinator_removed(&manager).await? => {
                let ids = PeerIds {
                    vault_id: linked_vault,
                    device_id,
                };
                linked_ids = Some(ids.clone());
                next = self
                    .removed_coordinator_pass(&session, &manager, ids, view)
                    .await?;
            }
            Some((linked_vault, device_id)) => {
                let mut syncing = view.clone();
                syncing.set_phase(SyncPhase::Syncing);
                self.publish(host, runtime, syncing);
                let assets = AssetPeer {
                    transport: PairedTransport {
                        manager: manager.clone(),
                        dialer: host.dialer(),
                    },
                    ids: PeerIds {
                        vault_id: linked_vault,
                        device_id,
                    },
                    assets: host.asset_store(&vault_id)?,
                };
                let mut transfer = AssetTransfer::default();
                let wait = OnlineWait {
                    transport: assets.transport.clone(),
                    ids: assets.ids.clone(),
                    known: VersionVector::new(),
                    #[cfg(desktop)]
                    hub: VersionVector::new(),
                    poll: None,
                };
                next = match self.upload_assets(&session, &assets, &mut transfer).await {
                    // Operations never reach the coordinator ahead of the assets they
                    // reference, but members still exchange.
                    Err(error) => {
                        self.anchor_link = None;
                        let path = session.files.quarantine_path();
                        let quarantine = blocking(move || Quarantine::load(&path)).await?;
                        let listening = session.host.serve_members().await;
                        self.coordinator_away(
                            &session,
                            wait,
                            quarantine.devices(),
                            &listening,
                            view,
                            error,
                        )
                        .await
                    }
                    Ok(()) => self.exchange(&session, wait, view).await?,
                };
                linked_ids = Some(assets.ids.clone());
                asset_peer = Some((assets, transfer));
            }
        }
        let coordinator_answered = asset_peer.is_some() && view.state() != SyncState::Offline;
        if !view.paused
            && self.relay_pass(&session, view).await
            && matches!(next, Next::Idle | Next::Online(_))
        {
            next = Next::Continue;
        }
        let server_link = if view.paused {
            None
        } else {
            self.server_exchange(&session, &manager, &vault_id, view)
                .await
        };
        if let Some(link) = &server_link {
            // The anchor server answered, so the device is not offline; a removed coordinator
            // stays reported.
            if view.state() == SyncState::Offline {
                view.set_phase(SyncPhase::Idle);
            }
            if link.more && matches!(next, Next::Idle | Next::Online(_) | Next::Retry) {
                next = Next::Continue;
            }
        }
        // A failed apply fails the pass only once the steps that do not depend on it have run,
        // so operations that stay unapplied never block an authority action.
        let apply_failure = self.apply_new(&session).await.err();
        let anchor_link = self.anchor_link.take();
        if is_hub && !session.anchored_by_server().await? {
            view.anchor_unreachable_since_ms = None;
        } else if !view.paused {
            let reachable = anchor_reachable(
                session.anchor_device_id().await?.as_deref(),
                &manager.device_id()?,
                manager
                    .coordinator_pin()?
                    .map(|pin| pin.device_id)
                    .as_deref(),
                coordinator_answered,
                anchor_link.is_some() || server_link.as_ref().is_some_and(|link| link.anchors),
            );
            view.anchor_unreachable_since_ms = if reachable {
                None
            } else {
                Some(match view.anchor_unreachable_since_ms {
                    Some(since) => since,
                    None => now_ms()?,
                })
            };
        }
        // Whether this linked desktop anchors and may sequence, once the pass checked it.
        #[cfg(desktop)]
        let mut linked_anchor_ready: Option<bool> = None;
        #[cfg(desktop)]
        let mut take_anchor = {
            let (take_anchor, restored) = session
                .run_proposal_actions(
                    &manager,
                    &vault_id,
                    is_hub,
                    &mut self.writer,
                    runtime.take_authority_requests(),
                )
                .await;
            if !restored.is_empty() {
                host.emit_applied(restored.into_iter().collect());
            }
            take_anchor
        };
        // What an anchor server that answered this pass stores, which is what the handover away
        // from it must cover.
        #[cfg(desktop)]
        let anchor_server_stored = server_link
            .as_ref()
            .filter(|link| link.anchors)
            .map(|link| link.stored.clone());
        if let Some((assets, _)) = asset_peer.as_ref()
            && view.state() != SyncState::Offline
        {
            let coordinator = manager
                .coordinator_pin()?
                .map(|pin| pin.device_id)
                .unwrap_or_default();
            #[cfg(desktop)]
            {
                let old_anchor_stored = anchor_server_stored.clone().or_else(|| match &next {
                    Next::Online(wait) if view.state() == SyncState::Idle => Some(wait.hub.clone()),
                    _ => None,
                });
                for request in take_anchor.drain(..) {
                    let outcome = if let Some(stored) = old_anchor_stored.as_ref() {
                        session
                            .take_anchor(&manager, &mut self.writer, stored)
                            .await
                    } else {
                        Err(
                            "taking the anchor needs a current exchange with the coordinator or the anchor server"
                                .to_string(),
                        )
                    };
                    if outcome.is_ok() {
                        next = Next::Continue;
                    }
                    request.answer(outcome);
                }
                // A linked desktop that anchors checks for a restore as the coordinator does;
                // members return lost entries through the coordinator's exchange.
                // A restored anchor sequences nothing while it waits.
                let anchors_here = session.anchors_here(&manager).await?;
                linked_anchor_ready =
                    Some(anchors_here && self.anchor_ready(&session, &manager, true).await?);
                if anchors_here {
                    if linked_anchor_ready == Some(true) {
                        match session
                            .anchor_as_member(
                                &assets.transport,
                                &assets.ids,
                                &mut self.writer,
                                &mut self.settled,
                                &self.hub_reported,
                            )
                            .await
                        {
                            // The coordinator learns of the entries on the next exchange.
                            Ok(true) => next = Next::Continue,
                            Ok(false) => {}
                            Err(error) => {
                                eprintln!("sync: collect proposals for the anchor: {error}");
                            }
                        }
                    }
                } else if anchor_link.is_none() {
                    session
                        .replay_outbox(&assets.transport, &assets.ids, &coordinator)
                        .await;
                }
            }
            #[cfg(mobile)]
            if anchor_link.is_none() {
                session
                    .replay_outbox(&assets.transport, &assets.ids, &coordinator)
                    .await;
            }
        }
        // An anchor that is not the coordinator and answered this pass takes the proposals
        // directly, also while the coordinator is away.
        if let (Some(link), Some(ids)) = (anchor_link.as_ref(), linked_ids.as_ref()) {
            session.replay_outbox(link, ids, &link.device_id).await;
        }
        #[cfg(desktop)]
        for request in take_anchor {
            let outcome = match anchor_server_stored.as_ref() {
                Some(stored) => {
                    session
                        .take_anchor(&manager, &mut self.writer, stored)
                        .await
                }
                None if is_hub => {
                    Err("the coordinating computer cannot take the anchor back yet".to_string())
                }
                None => Err(
                    "taking the anchor needs a current exchange with the coordinator or the anchor server".to_string(),
                ),
            };
            if outcome.is_ok() {
                next = Next::Continue;
            }
            request.answer(outcome);
        }
        if let Some((assets, mut transfer)) = asset_peer {
            next = self
                .download_assets(&session, &assets, &mut transfer, next, view)
                .await;
        }
        if let Some(link) = server_link {
            match self.settle_server(&session, &manager, link).await {
                Ok(true) if matches!(next, Next::Idle | Next::Online(_)) => next = Next::Continue,
                Ok(_) => {}
                Err(error) => eprintln!("sync: finish the anchor server exchange: {error}"),
            }
        }
        #[cfg(desktop)]
        if host.wants_anchor()?
            && let Err(error) = session.take_wanted_anchor(&manager, &mut self.writer).await
        {
            eprintln!("sync: take the anchor: {error}");
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
            #[cfg(desktop)]
            session.unlink_removed_devices(&manager, &vault_id).await?;
            #[cfg(desktop)]
            let anchor_ready = {
                let members_reachable =
                    host.is_server() || !manager.linked_peers_for(&vault_id)?.is_empty();
                self.anchor_ready(&session, &manager, members_reachable)
                    .await?
            };
            #[cfg(mobile)]
            let anchor_ready = true;
            if anchor_ready {
                if let Err(error) = session.maintain_space_keys(&mut self.writer, true).await {
                    eprintln!("sync: maintain space keys: {error}");
                }
                session
                    .reconcile_devices(&manager, &vault_id, &mut self.writer)
                    .await?;
                #[cfg(desktop)]
                session
                    .sequence_proposals(
                        &manager,
                        &mut self.writer,
                        runtime.take_proposal_requests(),
                        &VersionVector::new(),
                    )
                    .await;
            } else {
                if let Err(error) = session.maintain_space_keys(&mut self.writer, false).await {
                    eprintln!("sync: maintain space keys: {error}");
                }
                #[cfg(desktop)]
                for request in runtime.take_proposal_requests() {
                    let _ = request.reply.send(Ok(Err(ProposalRejection::NotAnchor)));
                }
            }
            #[cfg(desktop)]
            session
                .answer_forwards(runtime.take_forward_requests())
                .await;
            let stable = self.stability(host, &session, &manager, view).await?;
            #[cfg(desktop)]
            runtime.stability.publish(&vault_id, stable.clone());
            next = Self::compact(&session, &stable, next).await?;
        } else {
            #[cfg(desktop)]
            {
                runtime.stability.clear();
                // A linked anchor sequences what members proposed to its listener directly,
                // also while the coordinator is away.
                let ready = match linked_anchor_ready {
                    Some(ready) => ready,
                    None => {
                        session.anchors_here(&manager).await?
                            && self.anchor_ready(&session, &manager, true).await?
                    }
                };
                let requests = runtime.take_proposal_requests();
                if !requests.is_empty() {
                    if ready {
                        session
                            .sequence_proposals(
                                &manager,
                                &mut self.writer,
                                requests,
                                &self.hub_reported,
                            )
                            .await;
                        if matches!(next, Next::Online(_)) {
                            // The coordinator learns of the entries on the next exchange.
                            next = Next::Continue;
                        }
                    } else {
                        for request in requests {
                            let _ = request.reply.send(Ok(Err(ProposalRejection::NotAnchor)));
                        }
                    }
                }
                // While the coordinator is away, a linked anchor still sequences its own
                // proposals, such as removing the anchor a forced handover replaced.
                if linked_anchor_ready.is_none()
                    && ready
                    && let Some(active) = self.writer.as_mut()
                {
                    match session
                        .sequence_own_outbox(active, &self.hub_reported)
                        .await
                    {
                        Ok(true) if matches!(next, Next::Online(_)) => next = Next::Continue,
                        Ok(_) => {}
                        Err(error) => eprintln!("sync: sequence own proposals: {error}"),
                    }
                }
                let may_sequence = linked_anchor_ready.is_none() && ready;
                match session
                    .maintain_space_keys(&mut self.writer, may_sequence)
                    .await
                {
                    Ok(true) if matches!(next, Next::Online(_)) => next = Next::Continue,
                    Ok(_) => {}
                    Err(error) => eprintln!("sync: maintain space keys: {error}"),
                }
            }
            #[cfg(mobile)]
            if let Err(error) = session.maintain_space_keys(&mut self.writer, false).await {
                eprintln!("sync: maintain space keys: {error}");
            }
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
            let names = devices::read_with(host.device_id()?, &manager);
            view.set_clock_ahead(&status, &clock_warnings, &names);
        }
        apply_failure.map_or(Ok(next), Err)
    }

    /// Reset sync state on the coordinator: unlinks every device linked for the vault, because
    /// each pinned the charter the reset discards, clears the engine tables, and removes this
    /// installation's writer state. The rest of the pass founds a new space with a new charter
    /// and republishes every current row, and unlinked devices pair again. A pending handoff
    /// refuses the reset before anything changes.
    async fn reset_sync_state<H: SyncHost>(
        &mut self,
        host: &H,
        pool: &SqlitePool,
        manager: &PairingManager,
        vault_id: &str,
    ) -> Result<(), String> {
        manager.ensure_can_unlink()?;
        for peer in manager.linked_peers_for(vault_id)? {
            manager.unlink_device(&peer.device_id)?;
        }
        *self = Self {
            vault_id: Some(vault_id.to_string()),
            view: std::mem::take(&mut self.view),
            ..Self::default()
        };
        let files = host.sync_files(vault_id)?;
        {
            let files = files.clone();
            blocking(move || reset::forget_high_water(&files)).await?;
        }
        {
            let mut conn = acquire(pool).await?;
            reset::reset_database(&mut conn).await?;
        }
        let keys = host.key_store()?;
        blocking(move || reset::remove_installation_state(&files, keys.as_ref())).await
    }

    /// On the coordinator, computes stability over the devices enrolled and not removed in the
    /// selected authorization log and names the devices it does not count.
    async fn stability<H: SyncHost>(
        &mut self,
        host: &H,
        session: &Session<'_, H>,
        manager: &PairingManager,
        view: &mut SyncStatusView,
    ) -> Result<VersionVector, String> {
        let now = now_ms()?;
        let local_device_id = host.device_id()?;
        let mut conn = acquire(&session.pool).await?;
        let devices = session
            .engine
            .counted_devices(&mut conn, &session.ctx, &local_device_id)
            .await
            .map_err(|error| format!("read the sync counted set: {error}"))?;
        let stability = session
            .engine
            .stability(&mut conn, &session.ctx, &devices, now)
            .await
            .map_err(|error| format!("compute sync stability: {error}"))?;
        let names = devices::read_with(local_device_id, manager);
        view.set_device_standings(&stability.devices, &names);
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

    /// Applies stored operations once the log grew or the record store changed since the last
    /// apply, or local captures changed since an apply stopped on them, then repairs the Notes
    /// structure that applies left pending. Apply always commits, so applying on every pass
    /// would wake the service again; the repair commits only when keys are pending.
    async fn apply_new<H: SyncHost>(&mut self, session: &Session<'_, H>) -> Result<(), String> {
        let applied = self.apply_if_due(session).await;
        session.repair_notes().await;
        applied
    }

    async fn apply_if_due<H: SyncHost>(&mut self, session: &Session<'_, H>) -> Result<(), String> {
        let (stored, records) = {
            let mut conn = acquire(&session.pool).await?;
            let stored = session
                .engine
                .stored_vector(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read sync log: {error}"))?;
            let records = session
                .engine
                .authorization_totals(&mut conn, &session.ctx)
                .await
                .map_err(|error| format!("read authorization storage: {error}"))?
                .envelopes;
            (stored, records)
        };
        let pending = session.pending_captures().await?;
        if !self.applied.due(&stored, records, pending) {
            return Ok(());
        }
        let applied = session.apply_ready(&mut self.writer).await?;
        session.report(applied.tables, &applied.notices).await;
        self.applied = ApplyMark {
            stored: Some(stored),
            records,
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
    let dial = connectivity::dial_report(role, &host.pairing());
    view.connection = connectivity::connection(dial.as_ref());
    let mut reasons = DegradedReasons::default();
    connectivity::dial_reasons(dial.as_ref(), &mut reasons);
    host.note_platform_degraded(role, dial.as_ref(), &mut reasons)
        .await;
    view.degraded = reasons.into_vec();
}

/// How a quiesced vault is opened to seal its captures.
pub struct QuiescedSync {
    vault_id: String,
    path: PathBuf,
}

/// Decides, before a vault transition fences writes, whether the vault must be sealed once its
/// pools close, which it must while this device is linked in it.
pub fn quiesced_sync<H: SyncHost>(host: &H) -> Option<QuiescedSync> {
    if !host.sync_enabled().ok()? {
        return None;
    }
    let vault_id = host.active_vault_id().ok()?;
    let path = host.active_database_path().ok()?;
    Some(QuiescedSync { vault_id, path })
}

/// Seals pending captures of a quiesced vault through a private pool, because the shared
/// connection gate is held. It waits for any pass of this process to finish first. Failures
/// are logged: the captures stay and seal later.
pub async fn seal_quiesced<H: SyncHost>(host: &H, plan: Option<QuiescedSync>) {
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
    let pool = registry.connect_path(&plan.path).await?;
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
    keys: Arc<dyn SyncKeyStore>,
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
            host,
            pool,
            engine: host.engine()?,
            files: host.sync_files(vault_id)?,
            keys: host.key_store()?,
            ctx,
            key_release,
        }))
    }

    /// Creates the vault's space row when it is missing, as on a fresh vault or after Reset sync
    /// state, so the charter can be read and installed before the session starts.
    async fn init_space(&self, conn: &mut sqlx::SqliteConnection) -> Result<(), String> {
        self.engine
            .init_space(conn, &self.files.vault_id, now_ms()?)
            .await
            .map(|_| ())
            .map_err(|error| format!("initialize sync space: {error}"))
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

    /// Start order: refuse Projects, Calendar, Notes, and Chat placeholder keys, clear a stale
    /// apply flag, refuse sync state from before the authorization log, seed captures of tables
    /// that start to replicate, import a carry bundle a failed replacement left, check the
    /// writer, and apply what the import could not apply before local captures were sealed.
    async fn start(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        self.host.assert_converted(&self.pool).await?;
        let imported = {
            let mut conn = acquire(&self.pool).await?;
            local::reset_applying(&mut conn)
                .await
                .map_err(|error| format!("reset sync apply state: {error}"))?;
            self.init_space(&mut conn).await?;
            self.engine
                .refuse_legacy_state(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("open sync space: {error}"))?;
            self.engine
                .seed_captures(&mut conn, now_ms()?)
                .await
                .map_err(|error| format!("seed sync captures: {error}"))?;
            carry_forward::import_pending(
                &self.engine,
                &mut conn,
                &self.ctx,
                &self.files,
                &self.host.device_id()?,
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
        self.repair_notes().await;
        Ok(())
    }

    /// Repairs the Notes structure around the Notes rows that applies changed. The changed keys
    /// wait in `notes_sync_repair_pending`, which the repair clears in its own transaction, so a
    /// failed repair is logged and retried on the next pass or start without stopping the
    /// exchange.
    async fn repair_notes(&self) {
        if let Err(error) = self.try_repair_notes().await {
            eprintln!("repair notes structure: {error}");
        }
    }

    async fn try_repair_notes(&self) -> Result<(), String> {
        let tables = self.host.repair_notes(&self.pool).await?;
        if !tables.is_empty() {
            self.report(tables, &[]).await;
        }
        Ok(())
    }

    /// On a client, drops outbox proposals the applied chain holds and sends the rest to the
    /// hub, which hands them to the anchor. A failure is reported and the proposals wait for the
    /// next pass.
    async fn replay_outbox<T: SyncTransport>(&self, transport: &T, ids: &PeerIds, answering: &str) {
        if let Err(error) = self.replay_outbox_inner(transport, ids, answering).await {
            eprintln!("sync: send waiting proposals: {error}");
        }
    }

    async fn replay_outbox_inner<T: SyncTransport>(
        &self,
        transport: &T,
        ids: &PeerIds,
        answering: &str,
    ) -> Result<(), String> {
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await?;
        crate::client::prune_and_replay_outbox(
            &self.engine,
            &self.pool,
            &self.ctx,
            transport,
            ids,
            &mut outbox,
            answering,
        )
        .await?;
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
        crate::recovery::close(
            &self.engine,
            &self.pool,
            &self.ctx,
            active,
            self.host.restorers(),
            &request.choice,
            now_ms()?,
        )
        .await
    }
}

/// What enrolling a linked device records in the space.
#[cfg(desktop)]
fn linked_enrollment(
    peer: &ganbaru_handoff::pairing::LinkedPeer,
) -> Result<authority::Enrollment, String> {
    let certificate = ganbaru_handoff::pairing::decode_certificate(&peer.certificate)?;
    Ok(authority::Enrollment {
        device_id: peer.device_id.clone(),
        device_key: ganbaru_handoff::protocol::decode_sync_device_key(&peer.sync_device_key)?,
        tls_fingerprint: tls_fingerprint(certificate.as_ref()),
        label: peer.device_label.clone(),
        device_kind: authority::device_kind(peer.device_kind),
    })
}

/// The SHA-256 fingerprint of a DER certificate, as the space records TLS identities.
#[cfg(desktop)]
fn tls_fingerprint(certificate: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(certificate).into()
}

/// The public half of this installation's device key for `vault_id`, created once. Pairing
/// sends it, so the coordinator can enroll the device in the vault's sync space.
pub async fn ensure_device_key<H: SyncHost>(
    host: &H,
    vault_id: &str,
) -> Result<DevicePublicKey, String> {
    let keys = host.key_store()?;
    let vault_id = vault_id.to_string();
    blocking(move || {
        ensure_stored_device_key(keys.as_ref(), &vault_id).map(|device| device.public_key())
    })
    .await
}

/// The public half of this desktop's admin key for `vault_id`, created once. Pairing sends it,
/// so an admin can later promote this desktop and let it take the anchor.
#[cfg(desktop)]
pub async fn ensure_admin_key<H: SyncHost>(
    host: &H,
    vault_id: &str,
) -> Result<ganbaru_sync_contracts::AdminPublicKey, String> {
    let keys = host.key_store()?;
    let vault_id = vault_id.to_string();
    blocking(move || {
        ensure_stored_admin_key(keys.as_ref(), &vault_id).map(|admin| admin.public_key())
    })
    .await
}

/// The public half of this desktop's admin key for `vault_id`, `None` when it has none. Unlike
/// [`ensure_admin_key`], it never creates a key.
#[cfg(desktop)]
pub async fn stored_admin_key<H: SyncHost>(
    host: &H,
    vault_id: &str,
) -> Result<Option<ganbaru_sync_contracts::AdminPublicKey>, String> {
    let keys = host.key_store()?;
    let vault_id = vault_id.to_string();
    blocking(move || {
        read_admin_key(keys.as_ref(), &vault_id).map(|admin| admin.map(|admin| admin.public_key()))
    })
    .await
}

/// Seals the enrollment of a device the coordinator just linked by running a pass of the
/// service and waiting for it, which reconciles linked devices with the space. Returns the
/// charter the pairing reply pins.
#[cfg(desktop)]
pub async fn seal_enrollment(runtime: &SyncRuntime) -> Result<Option<PinnedCharter>, String> {
    let status = delegate_pass(runtime, DELEGATED_PASS_TIMEOUT).await?;
    match status.phase() {
        SyncPhase::Error { error, .. } => Err(error.clone()),
        _ => runtime.pairing_charter(),
    }
}

/// Device-local sync files of a vault.
pub fn sync_files<H: SyncHost>(host: &H, vault_id: &str) -> Result<SyncFiles, String> {
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
pub async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("sync key worker: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync_contracts::WriterId;

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn the_record_backstop_reads_free_space_of_the_vault_volume() {
        let dir = std::env::temp_dir();
        assert!(volume_free_bytes(dir.as_path()).is_some_and(|free| free > 0));
        assert_eq!(
            volume_free_bytes(&dir.as_path().join("ganbaru-absent-volume-probe")),
            None
        );
        let runtime = RuntimeState::default();
        assert_eq!(runtime.records().free_space(), None);
        runtime.set_vault_root(dir.clone());
        assert!(runtime.records().free_space().is_some());
    }

    /// Each vault change keeps the record exchange state the service charges shared with the
    /// runtime, so its backstop reads free space of the new vault's volume; a state of its own
    /// would read none. The previous vault's exchange state does not carry over.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_vault_change_keeps_the_record_exchange_shared_with_the_runtime() {
        let runtime = RuntimeState::default();
        let mut state = ServiceState::for_runtime(&runtime);
        assert_eq!(state.records.free_space(), None);
        for vault in ["first-vault", "second-vault"] {
            state.records.disconnected("hub");
            assert!(runtime.records().holds_vault_state());
            state.enter_vault(vault.to_owned(), &runtime, std::env::temp_dir());
            assert_eq!(state.vault_id.as_deref(), Some(vault));
            assert!(state.records.free_space().is_some());
            assert!(!runtime.records().holds_vault_state());
        }
        assert_eq!(ServiceState::default().records.free_space(), None);
    }

    fn vector(seq: u64) -> VersionVector {
        let mut vector = VersionVector::new();
        vector.set(WriterId::from_bytes([1; 16]), seq);
        vector
    }

    #[test]
    fn the_anchor_counts_as_reached_only_through_a_link_that_answered() {
        assert!(anchor_reachable(None, "own", None, false, false));
        assert!(anchor_reachable(
            Some("own"),
            "own",
            Some("hub"),
            false,
            false
        ));
        assert!(anchor_reachable(
            Some("hub"),
            "own",
            Some("hub"),
            true,
            false
        ));
        assert!(!anchor_reachable(
            Some("hub"),
            "own",
            Some("hub"),
            false,
            false
        ));
        assert!(
            !anchor_reachable(Some("desk"), "own", Some("hub"), true, false),
            "the coordinator answering does not reach a member anchor"
        );
        assert!(anchor_reachable(
            Some("desk"),
            "own",
            Some("hub"),
            false,
            true
        ));
    }

    #[test]
    fn apply_runs_once_per_log_growth() {
        let mut mark = ApplyMark::default();
        assert!(mark.due(&vector(1), 0, 0), "the first pass applies");
        mark.stored = Some(vector(1));
        assert!(!mark.due(&vector(1), 0, 0));
        assert!(
            !mark.due(&vector(1), 0, 7),
            "captures alone do not rerun an apply that finished"
        );
        assert!(mark.due(&vector(2), 0, 0), "a grown log applies again");
    }

    #[test]
    fn a_record_admitted_with_no_new_operation_applies_once() {
        let mark = ApplyMark {
            stored: Some(vector(1)),
            records: 3,
            blocked_at: None,
        };
        assert!(!mark.due(&vector(1), 3, 0));
        assert!(
            mark.due(&vector(1), 4, 0),
            "a selection or hold read from the new record is not current until it applies"
        );
    }

    #[test]
    fn a_blocked_apply_reruns_when_captures_change_and_waits_while_they_stay() {
        let mark = ApplyMark {
            stored: Some(vector(1)),
            records: 0,
            blocked_at: Some(5),
        };
        assert!(
            !mark.due(&vector(1), 0, 5),
            "the same captures would block again"
        );
        assert!(mark.due(&vector(1), 0, 2), "sealed captures may unblock it");
        assert!(
            mark.due(&vector(1), 0, 9),
            "new local edits may be sealable"
        );
        assert!(mark.due(&vector(2), 0, 5), "a grown log applies again");
    }
}

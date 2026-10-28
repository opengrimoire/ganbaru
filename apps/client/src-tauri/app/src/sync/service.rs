//! The sync service of the active vault: it imports carried operations, keeps the local writer
//! current, seals local captures after commits, and exchanges operations and managed asset
//! files with the hub. On the coordinator, the anchor of the vault's sync space, it founds the
//! space at first link, applies what linked devices push, and enrolls and removes devices as
//! they are linked and unlinked.
//!
//! A pass runs through a [`SyncHost`], so the app service and a headless pass share it. Passes
//! of one process run one at a time behind a gate, and a headless pass hands its work to the
//! app service when one is running.

use super::assets::{AssetMemory, AssetSession, AssetStore, AssetTransfer, MAX_ASSET_PASS_BYTES};
use super::carry_forward;
use super::client::{ClientSession, Exchange, ForkPoint, PeerIds, SyncTransport};
#[cfg(mobile)]
use super::host::HeadlessHost;
use super::host::SyncHost;
#[cfg(desktop)]
use super::hub::{HubForwards, HubProposals, HubStability};
use super::hub::{HubVault, OpenHubVault, SyncHub};
use super::recovery::{RecoveryChoice, RecoveryRequest};
use super::status::{DegradedReasons, SyncErrorCode, SyncRole, SyncState, SyncStatusView};
use super::transport::PairedTransport;
use super::vault_assets::ActiveVaultAssets;
use super::writer::{
    ActiveWriter, SuccessorPlan, SyncFiles, SyncKeyStore, WriterCheck, check_writer, create_writer,
    ensure_device_key as ensure_stored_device_key, read_stored_writer, remove_retired_keys,
};
use crate::vault::handoff::pairing::{PairingManager, PinnedCharter};
#[cfg(desktop)]
pub(crate) use anchor::{AuthorityAction, AuthorityAnswer, LateDecision, PendingRecoveryKit};
#[cfg(desktop)]
use anchor::{AuthorityRequest, ForwardRequest};
use ganbaru_db::{DatabaseAccessMode, DatabasePoolRegistry};
use ganbaru_handoff::protocol::SyncRefusalCode;
use ganbaru_sync::stability::DeviceStanding;
use ganbaru_sync::{
    ChangeNotice, Engine, SealReport, SpaceContext, SyncError, compaction_limit, hold, local,
};
use ganbaru_sync_contracts::{DevicePublicKey, Seq, VersionVector, WriterId};
#[cfg(desktop)]
use ganbaru_sync_contracts::{Digest32, ProposalRejection, SignedCharter, SignedProposal};
#[cfg(desktop)]
use ganbaru_sync_replica::authority::{
    self, AnchorKeys, Founding, ensure_admin_key as ensure_stored_admin_key, ensure_anchor_keys,
    ensure_charter,
};
use ganbaru_sync_replica::now_ms;
use ganbaru_sync_replica::outbox::Outbox;
use ganbaru_sync_replica::peers::discovery_keys::DiscoveryKeys;
use ganbaru_sync_replica::quarantine::{PullSkips, Quarantine};
use ganbaru_sync_replica::reset;
#[cfg(desktop)]
use ganbaru_sync_replica::writer::SequencedCheck;
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
    #[cfg(any(mobile, test))]
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
        Self {
            now: Arc::default(),
            changes: watch::Sender::new(0),
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
            #[cfg(any(mobile, test))]
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

    /// Records whether the app is in the foreground. A pass serves the member listener only
    /// while it is.
    #[cfg(any(mobile, test))]
    pub(crate) fn set_foreground(&self, visible: bool) {
        self.foreground.store(visible, Ordering::SeqCst);
    }

    /// Whether the app is in the foreground.
    #[cfg(any(mobile, test))]
    #[cfg_attr(not(mobile), allow(dead_code))]
    pub(crate) fn in_foreground(&self) -> bool {
        self.foreground.load(Ordering::SeqCst)
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
    pub(crate) fn request_authority(
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
    pub(crate) fn request_authority_detached(&self, action: AuthorityAction) -> Result<(), String> {
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
    pub(crate) fn request_reset(&self) -> Result<oneshot::Receiver<Result<(), String>>, String> {
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
    let proposals = hub_proposals(runtime.inner());
    let queue = runtime.inner().clone();
    let forwards: HubForwards = Arc::new(move |device_id, settled| {
        let runtime = queue.clone();
        Box::pin(async move {
            let receiver = runtime.request_forward(device_id, settled)?;
            tokio::time::timeout(DELEGATED_PASS_TIMEOUT, receiver)
                .await
                .map_err(|_| "the coordinator did not collect proposals in time".to_string())?
                .map_err(|_| "the coordinator stopped before it collected proposals".to_string())?
        })
    });
    SyncHub::new(open, runtime.changes.clone())
        .with_stability(runtime.stability.clone())
        .with_proposals(proposals)
        .with_forwards(forwards)
}

/// Hands a proposal a listener received to the pass, which sequences it when this desktop
/// anchors and answers that it is not the anchor otherwise.
#[cfg(desktop)]
fn hub_proposals(runtime: &SyncRuntime) -> HubProposals {
    let queue = runtime.clone();
    Arc::new(move |proposal| {
        let runtime = queue.clone();
        Box::pin(async move {
            let receiver = runtime.request_proposal(proposal)?;
            tokio::time::timeout(DELEGATED_PASS_TIMEOUT, receiver)
                .await
                .map_err(|_| "the anchor did not sequence the proposal in time".to_string())?
                .map_err(|_| "the anchor stopped before it sequenced the proposal".to_string())?
        })
    })
}

/// The hub a member listener routes admitted sync requests to, serving the active vault. Its
/// admission lets proposals through only while the selected log names this desktop as the
/// anchor, so a member reaches the anchor directly after a handover; it forwards none, since
/// the coordinator holds proposals for an anchor members cannot reach. It relays geneses of
/// other members, which the engine admits only for enrolled devices. A phone never anchors, so
/// its hub takes no proposals and reports no stability.
pub(crate) fn member_hub<R: Runtime>(app: &AppHandle<R>) -> SyncHub {
    let opener = app.clone();
    let open: OpenHubVault = Arc::new(move || {
        let app = opener.clone();
        Box::pin(async move { open_hub_vault(&app).await })
    });
    let runtime = app.state::<SyncRuntime>();
    with_anchor_services(
        SyncHub::new(open, runtime.changes.clone()).with_relayed_geneses(),
        runtime.inner(),
    )
}

/// Adds the stability report and proposal sequencing a desktop member serves once it anchors.
#[cfg(desktop)]
fn with_anchor_services(hub: SyncHub, runtime: &SyncRuntime) -> SyncHub {
    hub.with_stability(runtime.stability.clone())
        .with_proposals(hub_proposals(runtime))
}

/// A phone never anchors, so its member hub serves no anchor services.
#[cfg(mobile)]
fn with_anchor_services(hub: SyncHub, _runtime: &SyncRuntime) -> SyncHub {
    hub
}

/// Admission of the active vault's member listener, read from the selected log and the
/// quarantine for every connection.
pub(crate) fn member_admission_source<R: Runtime>(
    app: &AppHandle<R>,
) -> crate::vault::handoff::transport::MemberAdmissionSource {
    let app = app.clone();
    Arc::new(move || {
        let app = app.clone();
        Box::pin(async move { super::peers::member_admission(&app).await })
    })
}

/// The devices this installation quarantined in the active vault, which the coordinator
/// listener refuses.
#[cfg(desktop)]
pub(crate) fn quarantined_devices<R: Runtime>(
    app: &AppHandle<R>,
) -> crate::vault::handoff::transport::QuarantinedDevices {
    let app = app.clone();
    // The last list read for each vault, which stands in when the file cannot be read.
    let last_known: Arc<Mutex<std::collections::BTreeMap<String, BTreeSet<String>>>> =
        Arc::default();
    Arc::new(move || {
        let app = app.clone();
        let last_known = last_known.clone();
        Box::pin(async move {
            let vault_id = app.active_vault_id()?;
            let path = app.sync_files(&vault_id)?.quarantine_path();
            let loaded = blocking(move || Quarantine::load(&path)).await;
            let mut last_known = last_known
                .lock()
                .map_err(|_| "sync quarantine cache is unavailable".to_string())?;
            match loaded {
                Ok(quarantine) => {
                    last_known.insert(vault_id, quarantine.devices().clone());
                    Ok(quarantine.devices().clone())
                }
                Err(error) => {
                    eprintln!("sync: read the quarantine list, keeping the last one read: {error}");
                    Ok(last_known.get(&vault_id).cloned().unwrap_or_default())
                }
            }
        })
    })
}

async fn open_hub_vault<R: Runtime>(app: &AppHandle<R>) -> Result<Option<HubVault>, String> {
    if !app.sync_enabled()? {
        return Ok(None);
    }
    let Some(pool) = app.connect_for_sync().await? else {
        return Ok(None);
    };
    let vault_id = app.active_vault_id()?;
    let assets: Arc<dyn AssetStore> = Arc::new(ActiveVaultAssets::open(app, &vault_id)?);
    let path = app.sync_files(&vault_id)?.quarantine_path();
    let quarantined = blocking(move || Quarantine::load(&path))
        .await?
        .devices()
        .clone();
    Ok(Some(HubVault {
        pool,
        vault_id,
        assets,
        quarantined,
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
    app.state::<crate::vault::handoff::MemberListenerLifecycle>()
        .stop()
        .await;
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
    poll: Option<super::transport::MemberTransport>,
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
                    Some(anchor) => super::client::wait(anchor, &wait.ids, &wait.known).await,
                    None => super::client::wait(&wait.transport, &wait.ids, &wait.known).await,
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
    /// Operations of quarantined devices pulls already turned away.
    pull_skips: PullSkips,
    /// A link to an anchor that is not the coordinator and answered the last exchange, which
    /// the pass sends waiting proposals to instead of the coordinator.
    anchor_link: Option<super::transport::MemberTransport>,
    /// When this anchor first found its database restored behind its own log, in Unix
    /// milliseconds; it sequences nothing until the log catches up or it hands over to itself.
    #[cfg(desktop)]
    restored_since: Option<u64>,
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

    /// Whether this installation may sequence for the space. An anchor whose database holds
    /// less of the log than its writer record says it sealed was restored from a backup or
    /// copied: it moves to a successor writer at once, so nothing it seals reuses a sequence of
    /// the lost entries members may hold, and sequences nothing while members push back what it
    /// lost. Once the catch-up window passes, or at once when `members_reachable` is false, it
    /// seals a handover to itself so the next epoch supersedes the entries it never got back.
    /// The coordinator passes whether it has linked devices; a linked desktop that anchors
    /// always has the coordinator. A device that is not the anchor is always ready, since it
    /// sequences nothing.
    #[cfg(desktop)]
    async fn anchor_ready(
        &mut self,
        session: &Session<'_, impl SyncHost>,
        manager: &PairingManager,
        members_reachable: bool,
    ) -> Result<bool, String> {
        let Some(active) = self.writer.as_ref() else {
            return Ok(true);
        };
        if !session.anchors_here(manager).await? {
            self.restored_since = None;
            return Ok(true);
        }
        let check = {
            let mut conn = acquire(&session.pool).await?;
            authority::restore_check(&session.engine, &mut conn, &session.ctx, active.record())
                .await?
        };
        let SequencedCheck::Restored { recorded } = check else {
            self.restored_since = None;
            return Ok(true);
        };
        // The writer check retires a writer whose database is behind its log; a restore found
        // while the writer was already active retires it here.
        if !active
            .record()
            .sequenced
            .as_ref()
            .is_some_and(|entry| entry.restored)
            && !session.rotate_restored(&mut self.writer).await?
        {
            return Ok(false);
        }
        let now = now_ms()?;
        let since = *self.restored_since.get_or_insert(now);
        let window = u64::try_from(RESTORE_CATCH_UP_WINDOW.as_millis()).unwrap_or(u64::MAX);
        if now.saturating_sub(since) < window && members_reachable {
            eprintln!(
                "sync: the database was restored behind authorization revision {recorded}; \
                 waiting for linked devices to return the log"
            );
            return Ok(false);
        }
        let Some(active) = self.writer.as_mut() else {
            return Ok(false);
        };
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
        // the next pass then seals one more handover, whose epoch passes it.
        let mut conn = acquire(&session.pool).await?;
        let check =
            authority::restore_check(&session.engine, &mut conn, &session.ctx, active.record())
                .await?;
        let ready = !matches!(check, SequencedCheck::Restored { .. });
        if ready {
            self.restored_since = None;
        }
        Ok(ready)
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
            anchor_unreachable_since_ms: self.view.anchor_unreachable_since_ms,
            connection: self.view.connection.clone(),
            degraded: self.view.degraded.clone(),
            ..SyncStatusView::default()
        };
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
                recover_if_key_protection_lost(host).await;
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
        resets: &mut Vec<ResetReply>,
    ) -> Result<Next, String> {
        if !host.sync_enabled()? {
            host.stop_serving_members().await;
            return Ok(Next::Idle);
        }
        let Some(pool) = host.connect_for_sync().await? else {
            host.stop_serving_members().await;
            return Ok(Next::Idle);
        };
        let vault_id = host.active_vault_id()?;
        if self.vault_id.as_deref() != Some(vault_id.as_str()) {
            *self = Self {
                vault_id: Some(vault_id.clone()),
                ..Self::default()
            };
            #[cfg(desktop)]
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
            view.state = SyncState::WaitingForIdentity;
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
            Some(_) if session.coordinator_quarantined(&manager).await? => {
                view.state = SyncState::Offline;
                view.last_error = Some("this device quarantined the coordinator".to_string());
            }
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
                asset_peer = Some((assets, transfer));
            }
        }
        let coordinator_answered = asset_peer.is_some() && view.state != SyncState::Offline;
        self.apply_new(&session).await?;
        let anchor_link = self.anchor_link.take();
        if is_hub {
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
                anchor_link.is_some(),
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
        if let Some((assets, _)) = asset_peer.as_ref()
            && view.state != SyncState::Offline
        {
            #[cfg(desktop)]
            {
                let hub_stored = match &next {
                    Next::Online(wait) if view.state == SyncState::Idle => Some(wait.hub.clone()),
                    _ => None,
                };
                for request in take_anchor.drain(..) {
                    let outcome = if let Some(hub_stored) = hub_stored.as_ref() {
                        session
                            .take_anchor(&manager, &mut self.writer, hub_stored)
                            .await
                    } else {
                        Err(
                            "taking the anchor needs a current exchange with the coordinator"
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
                    session.replay_outbox(&assets.transport, &assets.ids).await;
                }
            }
            #[cfg(mobile)]
            if anchor_link.is_none() {
                session.replay_outbox(&assets.transport, &assets.ids).await;
            }
        }
        // An anchor that is not the coordinator and answered this pass takes the proposals
        // directly, also while the coordinator is away.
        if let (Some(link), Some((assets, _))) = (anchor_link.as_ref(), asset_peer.as_ref()) {
            session.replay_outbox(link, &assets.ids).await;
        }
        #[cfg(desktop)]
        for request in take_anchor {
            request.answer(Err(if is_hub {
                "the coordinating computer cannot take the anchor back yet".to_string()
            } else {
                "taking the anchor needs a current exchange with the coordinator".to_string()
            }));
        }
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
            #[cfg(desktop)]
            let anchor_ready = {
                let members_reachable = !manager.linked_peers_for(&vault_id)?.is_empty();
                self.anchor_ready(&session, &manager, members_reachable)
                    .await?
            };
            #[cfg(mobile)]
            let anchor_ready = true;
            if anchor_ready {
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
            let names = super::devices::read_with(host.device_id()?, &manager);
            view.set_clock_ahead(&status, &clock_warnings, &names);
        }
        Ok(next)
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
        let path = session.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        let listening = session.host.serve_members().await;
        self.anchor_link = None;
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
                quarantined: quarantine.devices(),
                pull_skips: Some(&self.pull_skips),
                max_bytes: super::client::MAX_EXCHANGE_BYTES,
                listening: &listening,
            };
            match client.exchange().await {
                Err(error) => {
                    return Ok(self
                        .coordinator_away(
                            session,
                            wait,
                            quarantine.devices(),
                            &listening,
                            view,
                            error,
                        )
                        .await);
                }
                Ok(Exchange::Done(done)) => {
                    view.last_exchange_at_ms = Some(now_ms()?);
                    if let Err(error) = super::peers::record_reported_peers(
                        session.files.peers_path(),
                        &session.engine,
                        &session.pool,
                        &session.ctx,
                        quarantine.devices(),
                        done.members.clone(),
                    )
                    .await
                    {
                        eprintln!("sync: record reported members: {error}");
                    }
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
                        if let Some(pin) = wait.transport.0.coordinator_pin()? {
                            session
                                .engine
                                .record_anchor_marks(
                                    &mut conn,
                                    &session.ctx,
                                    &pin.device_id,
                                    &done.hub,
                                )
                                .await
                                .map_err(|error| format!("record anchor marks: {error}"))?;
                        }
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
                    // Operations of devices this client quarantined count as known, so the long
                    // poll does not return for what it turns away; the coordinator's stored
                    // vector without those writers is what taking the anchor merges.
                    let skipped = self.pull_skips.for_quarantine(quarantine.devices());
                    #[cfg(desktop)]
                    {
                        wait.hub = authority::without_writers(&done.hub, &skipped);
                        self.hub_reported.clone_from(&done.hub);
                    }
                    let members = self
                        .exchange_members(session, &wait, quarantine.devices(), &listening, true)
                        .await;
                    self.anchor_link = members.anchor;
                    if members.pulled > 0 {
                        // Apply what members brought, then exchange it with the coordinator.
                        return Ok(Next::Continue);
                    }
                    wait.known = done.known;
                    wait.known.merge(&skipped);
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

    /// The pass while the coordinator does not answer: members still exchange with each other,
    /// and an anchor that is not the coordinator and answered a complete exchange carries the
    /// long poll, so its changes arrive without waiting for the coordinator. The status stays
    /// offline with `error`.
    async fn coordinator_away<H: SyncHost>(
        &mut self,
        session: &Session<'_, H>,
        mut wait: OnlineWait,
        quarantined: &BTreeSet<String>,
        listening: &[String],
        view: &mut SyncStatusView,
        error: String,
    ) -> Next {
        let members = self
            .exchange_members(session, &wait, quarantined, listening, false)
            .await;
        view.state = SyncState::Offline;
        view.last_error = Some(error);
        self.anchor_link = members.anchor;
        let (Some(anchor), Some(known)) = (self.anchor_link.as_ref(), members.anchor_known) else {
            return Next::Retry;
        };
        wait.known = known;
        wait.known
            .merge(&self.pull_skips.for_quarantine(quarantined));
        wait.poll = Some(anchor.clone());
        Next::Online(Box::new(wait))
    }

    /// Exchanges with the other members the pass plan picks. A failure only delays what members
    /// would bring until a later pass, so it is logged.
    async fn exchange_members<H: SyncHost>(
        &self,
        session: &Session<'_, H>,
        wait: &OnlineWait,
        quarantined: &BTreeSet<String>,
        listening: &[String],
        coordinator_reachable: bool,
    ) -> super::peers::MemberPassOutcome {
        let own_writers = self
            .writer
            .as_ref()
            .map(ActiveWriter::own_writers)
            .unwrap_or_default();
        let pass = super::peers::MemberPass {
            manager: &wait.transport.0,
            ids: &wait.ids,
            engine: &session.engine,
            pool: &session.pool,
            ctx: &session.ctx,
            writer: self.writer.as_ref().map(ActiveWriter::id),
            own_writers: &own_writers,
            quarantined,
            pull_skips: &self.pull_skips,
            listening,
            files: &session.files,
        };
        match super::peers::exchange_with_members(pass, coordinator_reachable).await {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!("sync: member exchanges: {error}");
                super::peers::MemberPassOutcome::default()
            }
        }
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
        match &next {
            Next::Online(wait) if wait.poll.is_none() => {}
            Next::Continue => {}
            _ => return next,
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
    /// changed since an apply stopped on them, then repairs the Notes structure that applies
    /// left pending. Apply always commits, so applying on every pass would wake the service
    /// again; the repair commits only when keys are pending.
    async fn apply_new<H: SyncHost>(&mut self, session: &Session<'_, H>) -> Result<(), String> {
        let applied = self.apply_if_due(session).await;
        session.repair_notes().await;
        applied
    }

    async fn apply_if_due<H: SyncHost>(&mut self, session: &Session<'_, H>) -> Result<(), String> {
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
        ganbaru_projects::conversion::assert_converted(&self.pool).await?;
        ganbaru_calendar::conversion::assert_converted(&self.pool).await?;
        ganbaru_notes::sync_keys::assert_converted(&self.pool).await?;
        ganbaru_chat::conversion::assert_converted(&self.pool).await?;
        ganbaru_chat::attachment_files::assert_converted(&self.pool).await?;
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

    /// Repairs the Notes structure around the Notes rows that applies changed, on a device that
    /// may write Notes. The changed keys wait in `notes_sync_repair_pending`, which the repair
    /// clears in its own transaction, so a failed repair is logged and retried on the next pass
    /// or start without stopping the exchange. Keys stay pending on a device that may not write
    /// Notes until it may.
    async fn repair_notes(&self) {
        if let Err(error) = self.try_repair_notes().await {
            eprintln!("repair notes structure: {error}");
        }
    }

    async fn try_repair_notes(&self) -> Result<(), String> {
        if !ganbaru_notes::sync_repair::has_pending(&self.pool).await? {
            return Ok(());
        }
        let config_dir = self.host.config_dir()?;
        let status = crate::vault::ownership::active_status_in(&config_dir, self.host.ownership())?;
        if !status.can_write_domain(ganbaru_sync::manifest::vault::notes::DOMAIN_NAME) {
            return Ok(());
        }
        let repair = ganbaru_notes::sync_repair::repair_pending(&self.pool).await?;
        for failed in &repair.failed_keys {
            eprintln!("notes structure repair kept a failing key pending: {failed}");
        }
        if !repair.unplaced_blocks.is_empty() {
            eprintln!(
                "notes structure repair left {} block cycles without a page",
                repair.unplaced_blocks.len()
            );
        }
        if !repair.is_empty() {
            let tables = vec![
                ganbaru_sync::manifest::vault::notes::PAGES.name.to_string(),
                ganbaru_sync::manifest::vault::notes::BLOCKS
                    .name
                    .to_string(),
            ];
            self.report(tables, &[]).await;
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

    /// Makes sure a writer is ready, checking the recorded one against the database and the
    /// device key and creating a successor when needed. Returns whether a writer is ready.
    async fn ensure_writer(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        if writer.is_some() {
            return Ok(true);
        }
        let files = self.files.clone();
        let keys = self.keys.clone();
        let (device_key, stored) = blocking(move || {
            let device = ensure_stored_device_key(keys.as_ref(), &files.vault_id)?;
            let stored = read_stored_writer(&files, keys.as_ref())?;
            Ok((device.public_key(), stored))
        })
        .await?;
        let check = {
            let mut conn = acquire(&self.pool).await?;
            check_writer(
                &self.engine,
                &mut conn,
                &self.ctx,
                &self.files,
                &device_key,
                stored,
            )
            .await?
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
        if let Some(active) = writer.as_mut() {
            let mut conn = acquire(&self.pool).await?;
            let charter = self
                .engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
                .map(|view| view.charter);
            active.settle_sequenced(charter)?;
        }
        Ok(writer.is_some())
    }

    /// Creates a successor writer certified by this installation's device key. A device without
    /// the person key also asks the coordinator for it, which Contacts signing needs.
    async fn create_successor(&self, plan: SuccessorPlan) -> Result<Option<ActiveWriter>, String> {
        if self.key_release == KeyRelease::Request
            && self.host.person_key(&self.ctx.person_key).await?.is_none()
        {
            self.host.request_person_key();
        }
        let device_id = self.host.device_id()?;
        let files = self.files.clone();
        let keys = self.keys.clone();
        let now = now_ms()?;
        blocking(move || {
            let device = ensure_stored_device_key(keys.as_ref(), &files.vault_id)?;
            create_writer(&files, keys.as_ref(), plan, &device, &device_id, now).map(Some)
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

    /// Replaces the writer of an anchor whose database was restored behind its log, keeping the
    /// high-water marked as restored.
    #[cfg(desktop)]
    async fn rotate_restored(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let plan = writer
            .as_ref()
            .map(SuccessorPlan::after_restore)
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
                // Captures stay pending until the exchange brings this device's enrollment.
                Err(SyncError::NotEnrolled) => break,
                Err(error) => return Err(format!("seal sync changes: {error}")),
            }
        }
        Ok(SealReport::default())
    }

    /// Seals the writer's genesis on its first pass, and an acknowledgement or keepalive when
    /// one is due, rotating once when the space no longer accepts the writer. Without a writer,
    /// or before this device is enrolled, it seals nothing.
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
                Err(SyncError::NotEnrolled) => break,
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
            if !self.reseal_own_forks(writer).await? {
                return Ok(done(tables, notices, None));
            }
            if report.local_writer_frozen {
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

    /// Re-seals every chain of this device that stored fork evidence proves forked while this
    /// installation still holds its copy past the shared prefix. Evidence reaches a hub, the
    /// anchor included, without an exchange of its own, so each apply checks. Returns false
    /// while the person key is unavailable.
    async fn reseal_own_forks(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let own = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .own_forks(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read fork evidence: {error}"))?
        };
        for (forked, keep_through) in own {
            let fork = ForkPoint {
                writer: forked,
                keep_through,
            };
            if !self.recover_fork(writer, fork).await? {
                return Ok(false);
            }
        }
        Ok(true)
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

    /// On a desktop coordinator, founds the vault's sync space once the vault first links: it
    /// creates the anchor keys and installs a charter naming this installation as the anchor,
    /// signed by the person key. A space that has a charter is left as it is. Phones never found
    /// a space. Returns the charter with its person key for the pairing reply.
    async fn found_space(&self, manager: &PairingManager) -> Result<Option<PinnedCharter>, String> {
        #[cfg(desktop)]
        {
            let device_id = manager.device_id()?;
            let installed = {
                let mut conn = acquire(&self.pool).await?;
                self.init_space(&mut conn).await?;
                self.engine
                    .charter(&mut conn, &self.ctx)
                    .await
                    .map_err(|error| format!("read sync charter: {error}"))?
            };
            if let Some(charter) = installed {
                return Ok(Some(self.pinned_charter(&charter)));
            }
            let record_path = self.files.record_path();
            let record =
                blocking(move || ganbaru_sync_replica::writer::read_record(&record_path)).await?;
            reset::refuse_refounding(record.as_ref())?;
            let Some(person) = self.host.person_key(&self.ctx.person_key).await? else {
                return Err(
                    "the person key that founds the sync space is missing on this device"
                        .to_string(),
                );
            };
            let anchor = self.anchor_keys().await?;
            let tls_fingerprint = tls_fingerprint(manager.certificate()?.as_ref());
            let discovery_key = ganbaru_handoff::discovery::DiscoveryKey::generate()?;
            let founding = Founding {
                device_id: &device_id,
                tls_fingerprint,
                discovery_key: *discovery_key.as_bytes(),
                person: &person,
                now_ms: now_ms()?,
            };
            let mut conn = acquire(&self.pool).await?;
            // Writers without a charter come from a build before the authorization log, and
            // only Reset sync state may found a space over them.
            self.engine
                .refuse_legacy_state(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("open sync space: {error}"))?;
            let charter =
                ensure_charter(&self.engine, &mut conn, &self.ctx, &anchor, &founding).await?;
            Ok(Some(self.pinned_charter(&charter)))
        }
        #[cfg(mobile)]
        {
            let _ = manager;
            Ok(None)
        }
    }

    /// The pairing form of `charter`. The coordinator presents it whether or not it anchors the
    /// space: a linked device learns the current anchor from the log the charter starts.
    #[cfg(desktop)]
    fn pinned_charter(&self, charter: &SignedCharter) -> PinnedCharter {
        PinnedCharter {
            charter: ganbaru_handoff::protocol::encode_sync_charter(charter),
            person_key: self.ctx.person_key.to_text(),
        }
    }

    /// Whether this device quarantined the coordinator it pinned, which it then never dials.
    async fn coordinator_quarantined(&self, manager: &PairingManager) -> Result<bool, String> {
        let Some(pin) = manager.coordinator_pin()? else {
            return Ok(false);
        };
        let path = self.files.quarantine_path();
        let quarantine = blocking(move || Quarantine::load(&path)).await?;
        Ok(quarantine.contains(&pin.device_id))
    }

    /// On a client, installs the charter pinned at pairing when the space has none yet, and
    /// refuses a space whose charter differs from it, because the pin names the log this device
    /// agreed to follow.
    async fn install_pinned_charter(&self, manager: &PairingManager) -> Result<(), String> {
        let Some(pinned) = manager.coordinator_pin()?.and_then(|pin| pin.charter) else {
            return Ok(());
        };
        let (charter, _) =
            ganbaru_handoff::protocol::decode_sync_charter(&pinned.charter, &pinned.person_key)?;
        let mut conn = acquire(&self.pool).await?;
        self.init_space(&mut conn).await?;
        match self
            .engine
            .charter(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync charter: {error}"))?
        {
            Some(installed) if installed == charter => Ok(()),
            Some(_) => Err(
                "the sync space has a different charter than the one pinned at pairing".to_string(),
            ),
            None => self
                .engine
                .install_charter(&mut conn, &self.ctx, &charter)
                .await
                .map_err(|error| format!("install the pinned sync charter: {error}")),
        }
    }

    /// Follows the vault's discovery keys from the authorization log into the device-local file
    /// and publishes them for the advertiser and coordinator discovery of this process.
    async fn follow_discovery_keys(&self) -> Result<(), String> {
        let view = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
        };
        let path = self.files.discovery_keys_path();
        let vault_id = self.files.vault_id.clone();
        let now = now_ms()?;
        let keys =
            blocking(move || DiscoveryKeys::follow(&path, &vault_id, view.as_ref(), now)).await?;
        super::discovery_keys::publish(keys);
        Ok(())
    }

    /// The anchor keys of the vault, each created once.
    #[cfg(desktop)]
    async fn anchor_keys(&self) -> Result<AnchorKeys, String> {
        let keys = self.keys.clone();
        let vault_id = self.files.vault_id.clone();
        blocking(move || ensure_anchor_keys(keys.as_ref(), &vault_id)).await
    }

    /// On an admin coordinator, enrolls linked devices the space does not know yet and removes
    /// enrolled devices that are no longer linked, so a crash between linking and sealing heals
    /// on the next pass. Each change goes through the outbox as a proposal signed by this
    /// desktop's admin key; the coordinator sequences it while it anchors and holds it for the
    /// anchor's forward request after a handover. A device the space removed stays
    /// removed: linking it again under the same device id enrolls nothing. A device that cannot
    /// be enrolled is reported and skipped, so it never stops the pass.
    async fn reconcile_devices(
        &self,
        manager: &PairingManager,
        vault_id: &str,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<(), String> {
        #[cfg(desktop)]
        {
            let Some(active) = writer.as_mut() else {
                return Ok(());
            };
            let mut conn = acquire(&self.pool).await?;
            let Some(view) = self
                .engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
            else {
                return Ok(());
            };
            let own_device_id = manager.device_id()?;
            let admin = self.admin_key().await?;
            if !authority::is_admin(&view, &own_device_id, &admin.public_key()) {
                return Ok(());
            }
            let anchors = view.anchor_device_id == own_device_id;
            let peers = manager.linked_peers_for(vault_id)?;
            let enroll: Vec<_> = peers
                .iter()
                .filter(|peer| {
                    !view
                        .devices
                        .iter()
                        .any(|device| device.device_id == peer.device_id)
                })
                .collect();
            let remove = authority::unlinked_devices(&view, &own_device_id, |device_id| {
                peers.iter().any(|peer| peer.device_id == device_id)
            });
            let mut changes = authority::Reconciliation {
                remove,
                ..Default::default()
            };
            for peer in enroll {
                match linked_enrollment(peer) {
                    Ok(enrollment) => changes.enroll.push(enrollment),
                    Err(error) => {
                        eprintln!("sync: enroll linked device {}: {error}", peer.device_id);
                    }
                }
            }
            let path = self.files.outbox_path();
            let mut outbox = blocking(move || Outbox::load(&path)).await?;
            if !changes.enroll.is_empty() || !changes.remove.is_empty() {
                authority::queue_reconciliation(
                    &mut outbox,
                    self.ctx.space,
                    view.charter,
                    &admin,
                    changes,
                    now_ms()?,
                )?;
            }
            // A coordinator that handed the anchor over keeps the proposals for the anchor's
            // forward request.
            if !anchors || outbox.entries().iter().all(|entry| !entry.pending()) {
                return Ok(());
            }
            authority::sequence_outbox(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                &mut outbox,
                &VersionVector::new(),
                now_ms()?,
            )
            .await?;
            drop(conn);
            // The record must name what sequencing sealed, or a restored database behind those
            // entries would still match the writer.
            self.record_commit(active).await?;
            Ok(())
        }
        #[cfg(mobile)]
        {
            let _ = (manager, vault_id, writer);
            Ok(())
        }
    }

    /// On the anchor, sequences the proposals linked devices sent through the hub with the
    /// anchor writer and answers each. A coordinator that handed the anchor over holds each in
    /// its outbox for the anchor's forward request. Without a writer, each is answered as not
    /// reaching the anchor; the sender keeps it in its outbox either way. Removal cutoffs cover
    /// at least `covered`, the coordinator's last reported stored vector on a linked anchor.
    #[cfg(desktop)]
    async fn sequence_proposals(
        &self,
        manager: &PairingManager,
        writer: &mut Option<ActiveWriter>,
        requests: Vec<ProposalRequest>,
        covered: &VersionVector,
    ) {
        if requests.is_empty() {
            return;
        }
        let anchor = self.anchors_here(manager).await;
        let mut outbox = None;
        if matches!(anchor, Ok(false)) {
            let path = self.files.outbox_path();
            outbox = Some(blocking(move || Outbox::load(&path)).await);
        }
        for request in requests {
            let outcome = match (&anchor, writer.as_mut()) {
                (Err(error), _) => Err(error.clone()),
                (Ok(false), _) => match (outbox.as_mut(), acquire(&self.pool).await) {
                    (Some(Ok(outbox)), Ok(mut conn)) => {
                        authority::hold_for_anchor(
                            &self.engine,
                            &mut conn,
                            &self.ctx,
                            outbox,
                            request.proposal.clone(),
                        )
                        .await
                    }
                    (Some(Err(error)), _) => Err(error.clone()),
                    (_, Err(error)) => Err(error),
                    (None, _) => Ok(Err(ProposalRejection::NotAnchor)),
                },
                (Ok(true), Some(active)) => match acquire(&self.pool).await {
                    Ok(mut conn) => match now_ms() {
                        Ok(now) => {
                            authority::sequence_covering(
                                &self.engine,
                                &mut conn,
                                &self.ctx,
                                &mut active.local(),
                                &request.proposal,
                                covered,
                                now,
                            )
                            .await
                        }
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                },
                _ => Ok(Err(ProposalRejection::NotAnchor)),
            };
            let _ = request.reply.send(outcome);
        }
        if let (Ok(true), Some(active)) = (&anchor, writer.as_mut())
            && let Err(error) = self.record_commit(active).await
        {
            eprintln!("sync: record the sequenced entries: {error}");
        }
    }

    /// Whether the selected log names this installation's device as the anchor.
    #[cfg(desktop)]
    async fn anchors_here(&self, manager: &PairingManager) -> Result<bool, String> {
        let device_id = manager.device_id()?;
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .is_some_and(|view| view.anchor_device_id == device_id))
    }

    /// The device the selected log names as the anchor, if the space has a log.
    async fn anchor_device_id(&self) -> Result<Option<String>, String> {
        let mut conn = acquire(&self.pool).await?;
        Ok(self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
            .map(|view| view.anchor_device_id))
    }

    /// On a client, drops outbox proposals the applied chain holds and sends the rest to the
    /// hub, which hands them to the anchor. A failure is reported and the proposals wait for the
    /// next pass.
    async fn replay_outbox<T: SyncTransport>(&self, transport: &T, ids: &PeerIds) {
        if let Err(error) = self.replay_outbox_inner(transport, ids).await {
            eprintln!("sync: send waiting proposals: {error}");
        }
    }

    async fn replay_outbox_inner<T: SyncTransport>(
        &self,
        transport: &T,
        ids: &PeerIds,
    ) -> Result<(), String> {
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await?;
        super::client::prune_and_replay_outbox(
            &self.engine,
            &self.pool,
            &self.ctx,
            transport,
            ids,
            &mut outbox,
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

/// What enrolling a linked device records in the space.
#[cfg(desktop)]
fn linked_enrollment(
    peer: &crate::vault::handoff::pairing::LinkedPeer,
) -> Result<authority::Enrollment, String> {
    let certificate = crate::vault::handoff::pairing::decode_certificate(&peer.certificate)?;
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
pub(crate) async fn ensure_device_key<H: SyncHost>(
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
pub(crate) async fn ensure_admin_key<H: SyncHost>(
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

/// Seals the enrollment of a device the coordinator just linked by running a pass of the app
/// service and waiting for it, which reconciles linked devices with the space.
#[cfg(desktop)]
pub(crate) fn enrollment_sealer<R: Runtime>(
    app: &AppHandle<R>,
) -> crate::vault::handoff::transport::EnrollmentSealer {
    let runtime = app.state::<SyncRuntime>().inner().clone();
    Arc::new(move || {
        let runtime = runtime.clone();
        Box::pin(async move {
            let status = delegate_pass(&runtime, DELEGATED_PASS_TIMEOUT).await?;
            match status.state {
                SyncState::Error => Err(status
                    .last_error
                    .unwrap_or_else(|| "the sync pass failed".to_string())),
                _ => runtime.pairing_charter(),
            }
        })
    })
}

/// Device-local sync files of a vault.
pub(crate) fn sync_files<H: SyncHost>(host: &H, vault_id: &str) -> Result<SyncFiles, String> {
    host.sync_files(vault_id)
}

/// Recovers after a failed pass when opening a sealed key file reported its platform key lost,
/// so the device enrolls again with fresh keys instead of retrying a key it cannot recover; see
/// [`super::key_protection::recover_after_key_loss`]. Only the losses the key wrapper recorded
/// count, never the pass error, whose text can come from a remote hub. A host without file keys
/// is left as it is.
async fn recover_if_key_protection_lost<H: SyncHost>(host: &H) {
    let Some(protection) = host.key_protection() else {
        return;
    };
    let lost = protection.take_lost();
    if lost.is_empty() {
        return;
    }
    let pairing = host.pairing();
    let result = match host.config_dir() {
        Ok(config_dir) => {
            blocking(move || {
                super::key_protection::recover_after_key_loss(
                    &config_dir,
                    &pairing,
                    protection.as_ref(),
                    &lost,
                )
            })
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("sync: recover after losing protected keys: {error}");
    }
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

#[cfg(desktop)]
mod anchor;
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

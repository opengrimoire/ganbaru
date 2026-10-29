//! Record exchange: both sides of moving the authorization record store between paired devices
//! ahead of the operation lanes, with the device-local transport admission, storage backstop,
//! and growth alerts of the R7.3 A4 to A8 rules.
//!
//! Nothing here feeds an authority rule. Budgets only delay envelopes to a later window, and
//! the backstop only stops this device from storing more than it can hold until the user acts.

use super::client::{PeerIds, SyncTransport, answer};
use super::wire::{hash_from_wire, hash_to_wire, ops_from_wire, ops_to_wire, writer_from_wire};
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_RECORDS, PROTOCOL_VERSION, SYNC_PAGE_BYTES, SYNC_RECORD_CLASSES,
};
use ganbaru_sync::{
    AuthorizationStorage, Engine, RECORD_CLASSES, RecordAdmission, RecordClass, RecordCursor,
    RecordWants, SpaceContext, SyncError,
};
use ganbaru_sync_contracts::WriterId;
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const _: () = assert!(RECORD_CLASSES == SYNC_RECORD_CLASSES);

/// Length of one admission window.
pub const RECORD_WINDOW: Duration = Duration::from_secs(10);
/// What one peer is served, and what a normal peer may send, per window.
pub const NORMAL_BUDGET: RecordBudget = RecordBudget {
    bytes: 8 * 1024 * 1024,
    envelopes: 2_048,
};
/// What a low peer (removed, held, or not live at the local head) may send per window by
/// default.
pub const LOW_BUDGET: RecordBudget = RecordBudget {
    bytes: 64 * 1024,
    envelopes: 64,
};
/// The smallest low budget a device preference may set.
pub const LOW_BUDGET_MIN: RecordBudget = RecordBudget {
    bytes: 16 * 1024,
    envelopes: 16,
};
/// Stored envelope bytes past which the backstop starts.
pub const BACKSTOP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Stored envelopes past which the backstop starts.
pub const BACKSTOP_ENVELOPES: u64 = 2_097_152;
/// Free bytes on the vault volume under which the backstop starts.
pub const BACKSTOP_FREE_BYTES: u64 = 512 * 1024 * 1024;
/// Bytes the backstop still stores, for kit and contest records and their ancestors.
pub const BACKSTOP_RESERVE: u64 = 256 * 1024 * 1024;
/// The part of the reserve only kit records and their ancestors may spend.
pub const BACKSTOP_KIT_RESERVE: u64 = 64 * 1024 * 1024;
/// Free bytes on the vault volume under which record exchange stops fully.
pub const HARD_STOP_FREE_BYTES: u64 = 128 * 1024 * 1024;
/// Envelope bytes one device carries past which Settings > Sync alerts.
pub const ALERT_DEVICE_BYTES: u64 = 64 * 1024 * 1024;
/// Stored envelope bytes past which Settings > Sync alerts.
pub const ALERT_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// Rounds of wanted lookups one session makes, each following chains one step further.
const WANTED_ROUNDS: usize = 64;
/// Sessions a wanted entry every peer answered unknown waits at most before it is asked again,
/// doubling from one; with sessions every few seconds to minutes, about an hour.
const MAX_BACKOFF_SESSIONS: u32 = 256;

/// Bytes and envelopes of one admission window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordBudget {
    pub bytes: usize,
    pub envelopes: usize,
}

impl RecordBudget {
    /// The low budget a device preference asks for, kept between the minimum and default.
    pub fn low(preference: Option<Self>) -> Self {
        let wanted = preference.unwrap_or(LOW_BUDGET);
        Self {
            bytes: wanted.bytes.clamp(LOW_BUDGET_MIN.bytes, LOW_BUDGET.bytes),
            envelopes: wanted
                .envelopes
                .clamp(LOW_BUDGET_MIN.envelopes, LOW_BUDGET.envelopes),
        }
    }
}

/// What one peer moved in the current window.
#[derive(Clone, Copy, Debug)]
struct Window {
    started: Instant,
    bytes: usize,
    envelopes: usize,
}

impl Window {
    fn new(now: Instant) -> Self {
        Self {
            started: now,
            bytes: 0,
            envelopes: 0,
        }
    }

    /// The window at `now`, a fresh one once the current one ended.
    fn at(self, now: Instant) -> Self {
        if now.duration_since(self.started) >= RECORD_WINDOW {
            Self::new(now)
        } else {
            self
        }
    }

    /// What `budget` leaves in this window.
    fn left(&self, budget: RecordBudget) -> RecordBudget {
        RecordBudget {
            bytes: budget.bytes.saturating_sub(self.bytes),
            envelopes: budget.envelopes.saturating_sub(self.envelopes),
        }
    }

    fn spend(&mut self, bytes: usize, envelopes: usize) {
        self.bytes = self.bytes.saturating_add(bytes);
        self.envelopes = self.envelopes.saturating_add(envelopes);
    }
}

/// How the local selected head classes a peer for ingress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerClass {
    /// Enrolled, not removed, and not held.
    Normal,
    /// Removed, held, or not enrolled.
    Low,
}

/// Why this device stores no more record envelopes than kit and contest ones, see the R7.3 A7
/// rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackstopState {
    /// Kit and contest records and their ancestors still draw on the reserve.
    Reserve,
    /// The reserve is spent or the volume is nearly full: record exchange stores nothing.
    Stopped,
}

/// The storage backstop once it started: what it has spent of the reserve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Backstop {
    spent_kit: u64,
    spent_other: u64,
    stopped: bool,
}

impl Backstop {
    /// Charges `bytes` of `class` to the reserve. Returns whether the reserve covered them.
    fn charge(&mut self, class: RecordClass, bytes: u64) -> bool {
        let shared = BACKSTOP_RESERVE - BACKSTOP_KIT_RESERVE;
        let covered = match class {
            RecordClass::Kit => self.spent_kit + self.spent_other + bytes <= BACKSTOP_RESERVE,
            _ => self.spent_other + bytes <= shared,
        };
        if covered {
            match class {
                RecordClass::Kit => self.spent_kit += bytes,
                _ => self.spent_other += bytes,
            }
        } else {
            self.stopped = true;
        }
        covered
    }
}

/// A growth alert of Settings > Sync, see the R7.3 A8 rule. Display only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrowthAlert {
    /// Envelopes one device's writers carry exceed [`ALERT_DEVICE_BYTES`].
    Device { device_id: String, bytes: u64 },
    /// Stored envelopes exceed [`ALERT_TOTAL_BYTES`].
    Total { bytes: u64 },
    /// A writer holds two envelopes at one sequence or two certificates. `device_id` is the
    /// device a stored certificate of the writer names, if any.
    ForkedWriter {
        writer: WriterId,
        device_id: Option<String>,
    },
}

/// The growth alerts `storage` raises.
pub fn growth_alerts(storage: &AuthorizationStorage) -> Vec<GrowthAlert> {
    let mut alerts: Vec<GrowthAlert> = storage
        .by_device
        .iter()
        .filter(|(_, bytes)| *bytes > ALERT_DEVICE_BYTES)
        .map(|(device_id, bytes)| GrowthAlert::Device {
            device_id: device_id.clone(),
            bytes: *bytes,
        })
        .collect();
    if storage.bytes > ALERT_TOTAL_BYTES {
        alerts.push(GrowthAlert::Total {
            bytes: storage.bytes,
        });
    }
    alerts.extend(
        storage
            .forked_writers
            .iter()
            .map(|forked| GrowthAlert::ForkedWriter {
                writer: forked.writer,
                device_id: forked.device_id.clone(),
            }),
    );
    alerts
}

/// Free bytes on the vault volume, or `None` when the platform cannot tell.
pub type FreeSpace = Arc<dyn Fn() -> Option<u64> + Send + Sync>;

/// When wanted entry `key` may be asked again.
#[derive(Clone, Copy, Debug, Default)]
struct Backoff {
    /// Session count from which it is asked again.
    next: u64,
    /// Sessions it waits after the next unknown answer.
    wait: u32,
}

#[derive(Default)]
struct Inner {
    /// Where pulling from each peer continues.
    pulled: HashMap<String, RecordCursor>,
    /// Where pushing to each peer continues.
    pushed: HashMap<String, RecordCursor>,
    /// What each peer was served in the current window.
    served: HashMap<String, Window>,
    /// What each peer sent, by normal and by low budget, in the current window.
    received: HashMap<String, (Window, Window)>,
    /// Retry timing of wanted entries every peer answered unknown.
    backoff: BTreeMap<Vec<u8>, Backoff>,
    /// Sessions run, for backoff.
    sessions: u64,
    /// The backstop once it started.
    backstop: Option<Backstop>,
    /// The low budget a device preference sets.
    low: Option<RecordBudget>,
}

/// Device-local state of record exchange, shared by every session and listener of one
/// installation: cursors, admission windows, wanted retry timing, and the storage backstop.
#[derive(Clone, Default)]
pub struct RecordExchange {
    inner: Arc<Mutex<Inner>>,
    free_space: Option<FreeSpace>,
}

impl RecordExchange {
    /// Sets how the backstop reads free space on the vault volume.
    #[must_use]
    pub fn with_free_space(mut self, free_space: FreeSpace) -> Self {
        self.free_space = Some(free_space);
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the low ingress budget from a device preference, clamped to its bounds.
    pub fn set_low_budget(&self, preference: Option<RecordBudget>) {
        self.lock().low = preference.map(|budget| RecordBudget::low(Some(budget)));
    }

    /// Ends the backstop after the user freed space, raised the bound, or re-founded the
    /// space. It starts again at the next exchange if the store is still past a bound.
    pub fn resume(&self) {
        let mut inner = self.lock();
        inner.backstop = None;
        // Pages pulled while it ran left out what it did not store.
        inner.pulled.clear();
    }

    /// Forgets a peer's cursors, as when it connects anew, and lets every wanted entry be asked
    /// of it again.
    pub fn connected(&self, peer: &str) {
        let mut inner = self.lock();
        inner.pulled.remove(peer);
        inner.pushed.remove(peer);
        inner.backoff.clear();
    }

    /// Counts a session with `peer`. A peer this device has no cursor for is new, so every
    /// wanted entry may be asked of it, see the R7.3 A4 rule.
    fn start_session(&self, peer: &str) {
        let mut inner = self.lock();
        inner.sessions += 1;
        if !inner.pulled.contains_key(peer) {
            inner.backoff.clear();
        }
    }

    /// The backstop state as the last exchange left it, `None` while full sync runs. Unlike
    /// [`Self::backstop`], it reads no storage and never starts the backstop.
    pub fn backstop_state(&self) -> Option<BackstopState> {
        self.lock().backstop.map(|backstop| {
            if backstop.stopped {
                BackstopState::Stopped
            } else {
                BackstopState::Reserve
            }
        })
    }

    /// The backstop state, `None` while full sync runs. Starts the backstop when the store or
    /// the volume crossed a bound.
    pub async fn backstop(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> Result<Option<BackstopState>, SyncError> {
        let free = self.free_space.as_ref().and_then(|free| free());
        if self.lock().backstop.is_none() {
            let storage = engine.authorization_storage(conn, ctx).await?;
            let full = storage.bytes > BACKSTOP_BYTES
                || storage.envelopes > BACKSTOP_ENVELOPES
                || free.is_some_and(|free| free < BACKSTOP_FREE_BYTES);
            if !full {
                return Ok(None);
            }
            self.lock().backstop = Some(Backstop::default());
        }
        let mut inner = self.lock();
        let backstop = inner.backstop.get_or_insert_with(Backstop::default);
        if free.is_some_and(|free| free < HARD_STOP_FREE_BYTES) {
            backstop.stopped = true;
        }
        Ok(Some(if backstop.stopped {
            BackstopState::Stopped
        } else {
            BackstopState::Reserve
        }))
    }

    /// What `peer` may still be served in this window.
    fn serve_budget(&self, peer: &str) -> RecordBudget {
        let now = Instant::now();
        let mut inner = self.lock();
        let window = inner
            .served
            .entry(peer.to_string())
            .or_insert_with(|| Window::new(now));
        *window = window.at(now);
        window.left(NORMAL_BUDGET)
    }

    fn served(&self, peer: &str, bytes: usize, envelopes: usize) {
        let now = Instant::now();
        let mut inner = self.lock();
        let window = inner
            .served
            .entry(peer.to_string())
            .or_insert_with(|| Window::new(now));
        *window = window.at(now);
        window.spend(bytes, envelopes);
    }

    /// What `peer` of `class` may still send in this window. Kit envelopes count against the
    /// normal budget only.
    fn receive_budget(&self, peer: &str, class: PeerClass) -> (RecordBudget, RecordBudget) {
        let now = Instant::now();
        let mut inner = self.lock();
        let low = match class {
            PeerClass::Normal => NORMAL_BUDGET,
            PeerClass::Low => inner.low.unwrap_or(LOW_BUDGET),
        };
        let windows = inner
            .received
            .entry(peer.to_string())
            .or_insert_with(|| (Window::new(now), Window::new(now)));
        windows.0 = windows.0.at(now);
        windows.1 = windows.1.at(now);
        (windows.0.left(NORMAL_BUDGET), windows.1.left(low))
    }

    fn received(&self, peer: &str, all: (usize, usize), not_kit: (usize, usize)) {
        let now = Instant::now();
        let mut inner = self.lock();
        let windows = inner
            .received
            .entry(peer.to_string())
            .or_insert_with(|| (Window::new(now), Window::new(now)));
        windows.0 = windows.0.at(now);
        windows.1 = windows.1.at(now);
        windows.0.spend(all.0, all.1);
        windows.1.spend(not_kit.0, not_kit.1);
    }

    /// The envelopes of `envelopes` the backstop still stores, charged to its reserve: kit and
    /// contest envelopes and wanted entries. Every envelope while it is not running.
    fn backstop_filter(&self, envelopes: Vec<Vec<u8>>, classes: &[RecordClass]) -> Vec<Vec<u8>> {
        let mut inner = self.lock();
        let Some(backstop) = inner.backstop.as_mut() else {
            return envelopes;
        };
        envelopes
            .into_iter()
            .zip(classes)
            .filter(|(bytes, class)| {
                **class <= RecordClass::Wanted
                    && !backstop.stopped
                    && backstop.charge(**class, bytes.len() as u64)
            })
            .map(|(bytes, _)| bytes)
            .collect()
    }
}

/// The ingress class of `device_id` at the local selected head.
pub async fn peer_class(
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    device_id: &str,
) -> Result<PeerClass, SyncError> {
    let Some(view) = engine.authorization(conn, ctx).await? else {
        return Ok(PeerClass::Low);
    };
    let live = view
        .devices
        .iter()
        .any(|device| device.device_id == device_id && device.removed_revision.is_none());
    if !live || engine.device_held(conn, ctx, device_id).await? {
        return Ok(PeerClass::Low);
    }
    Ok(PeerClass::Normal)
}

/// Bytes and count of `envelopes`, in total and without kit envelopes.
fn sizes(envelopes: &[Vec<u8>], classes: &[RecordClass]) -> ((usize, usize), (usize, usize)) {
    let mut all = (0, 0);
    let mut not_kit = (0, 0);
    for (bytes, class) in envelopes.iter().zip(classes) {
        all = (all.0 + bytes.len(), all.1 + 1);
        if *class != RecordClass::Kit {
            not_kit = (not_kit.0 + bytes.len(), not_kit.1 + 1);
        }
    }
    (all, not_kit)
}

fn cursor_to_wire(cursor: &RecordCursor) -> Vec<u64> {
    cursor.0.to_vec()
}

fn cursor_from_wire(cursor: &[u64]) -> Result<RecordCursor, String> {
    <[u64; RECORD_CLASSES]>::try_from(cursor)
        .map(RecordCursor)
        .map_err(|_| "sync record cursor is invalid".to_string())
}

fn wants_to_wire(wants: &RecordWants) -> (Vec<String>, Vec<String>) {
    (
        wants
            .hashes
            .iter()
            .take(MAX_SYNC_RECORDS)
            .map(hash_to_wire)
            .collect(),
        wants
            .writers
            .iter()
            .take(MAX_SYNC_RECORDS)
            .map(WriterId::to_hex)
            .collect(),
    )
}

fn wants_from_wire(hashes: &[String], writers: &[String]) -> Result<RecordWants, String> {
    Ok(RecordWants {
        hashes: hashes
            .iter()
            .map(|hash| hash_from_wire(hash))
            .collect::<Result<_, _>>()?,
        writers: writers
            .iter()
            .map(|writer| writer_from_wire(writer))
            .collect::<Result<_, _>>()?,
    })
}

/// A record exchange request refused before it was answered.
#[derive(Debug, PartialEq, Eq)]
pub enum RecordRefusal {
    /// The request does not decode.
    Invalid(String),
    /// The peer's window is spent; the request may be repeated in a later one.
    Busy,
    /// The database failed.
    Failed(String),
}

/// Answers a record exchange request of `peer`, returning the answer and whether the record
/// store grew. Removed, held, and unenrolled peers are served too.
pub async fn respond(
    state: &RecordExchange,
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    peer: &str,
    request: ControlMessage,
) -> Result<(ControlMessage, bool), RecordRefusal> {
    let failed = |error: SyncError| RecordRefusal::Failed(error.to_string());
    match request {
        ControlMessage::SyncRecords {
            cursor, max_bytes, ..
        } => {
            let cursor = cursor_from_wire(&cursor).map_err(RecordRefusal::Invalid)?;
            let budget = state.serve_budget(peer);
            if budget.envelopes == 0 || budget.bytes == 0 {
                return Err(RecordRefusal::Busy);
            }
            let page = engine
                .record_page(
                    conn,
                    ctx,
                    &cursor,
                    budget.bytes.min(max_bytes as usize).min(SYNC_PAGE_BYTES),
                    budget.envelopes.min(MAX_SYNC_RECORDS),
                )
                .await
                .map_err(failed)?;
            let bytes: Vec<Vec<u8>> = page.envelopes.into_iter().map(|e| e.bytes).collect();
            state.served(peer, bytes.iter().map(Vec::len).sum(), bytes.len());
            Ok((
                ControlMessage::SyncRecordPage {
                    records: ops_to_wire(&bytes),
                    cursor: cursor_to_wire(&page.cursor),
                    more: page.more,
                },
                false,
            ))
        }
        ControlMessage::SyncRecordGet {
            hashes, writers, ..
        } => {
            let wants = wants_from_wire(&hashes, &writers).map_err(RecordRefusal::Invalid)?;
            let budget = state.serve_budget(peer);
            if budget.envelopes == 0 || budget.bytes == 0 {
                return Err(RecordRefusal::Busy);
            }
            let mut found = engine
                .records_by_hash(conn, ctx, &wants.hashes)
                .await
                .map_err(failed)?;
            found.extend(
                engine
                    .certificates_of(conn, ctx, &wants.writers)
                    .await
                    .map_err(failed)?,
            );
            let mut bytes = Vec::new();
            let mut size = 0;
            for envelope in found {
                let fits = bytes.len() < budget.envelopes.min(MAX_SYNC_RECORDS)
                    && size + envelope.bytes.len() <= budget.bytes.min(SYNC_PAGE_BYTES);
                if !fits && !bytes.is_empty() {
                    break;
                }
                size += envelope.bytes.len();
                bytes.push(envelope.bytes);
            }
            state.served(peer, size, bytes.len());
            Ok((
                ControlMessage::SyncRecordFound {
                    records: ops_to_wire(&bytes),
                },
                false,
            ))
        }
        ControlMessage::SyncRecordPush { records, .. } => {
            let records = ops_from_wire(&records).map_err(RecordRefusal::Invalid)?;
            let class = peer_class(engine, conn, ctx, peer).await.map_err(failed)?;
            let classes = engine
                .record_classes(conn, ctx, &records)
                .await
                .map_err(failed)?;
            let (normal, low) = state.receive_budget(peer, class);
            let (all, not_kit) = sizes(&records, &classes);
            // A page that starts in budget is taken whole; one that starts past it waits,
            // except for kit envelopes past only the low budget.
            let normal_spent = normal.bytes == 0 || normal.envelopes == 0;
            let low_spent = low.bytes == 0 || low.envelopes == 0;
            let (records, classes, busy) = if normal_spent {
                (Vec::new(), Vec::new(), true)
            } else if low_spent && not_kit.1 > 0 {
                let (kit, kit_classes): (Vec<_>, Vec<_>) = records
                    .into_iter()
                    .zip(classes)
                    .filter(|(_, class)| *class == RecordClass::Kit)
                    .unzip();
                (kit, kit_classes, true)
            } else {
                (records, classes, false)
            };
            if records.is_empty() && busy {
                return Err(RecordRefusal::Busy);
            }
            // The backstop keeps serving every peer and stores only what its reserve allows.
            if state.backstop(engine, conn, ctx).await.map_err(failed)?
                == Some(BackstopState::Stopped)
            {
                return Err(RecordRefusal::Busy);
            }
            let kept = state.backstop_filter(records, &classes);
            admit_pushed(state, engine, conn, ctx, peer, kept, all, not_kit, busy).await
        }
        _ => Err(RecordRefusal::Invalid(
            "not a record exchange request".to_string(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
async fn admit_pushed(
    state: &RecordExchange,
    engine: &Engine,
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    peer: &str,
    records: Vec<Vec<u8>>,
    all: (usize, usize),
    not_kit: (usize, usize),
    busy: bool,
) -> Result<(ControlMessage, bool), RecordRefusal> {
    let now = super::now_ms().map_err(RecordRefusal::Failed)?;
    state.received(peer, all, if busy { (0, 0) } else { not_kit });
    let admission = engine
        .admit_records(conn, ctx, &records, now)
        .await
        .map_err(|error| RecordRefusal::Failed(error.to_string()))?;
    if busy {
        return Err(RecordRefusal::Busy);
    }
    let (missing_hashes, missing_writers) = wants_to_wire(&admission.missing);
    Ok((
        ControlMessage::SyncRecordPushResult {
            admitted: u32::try_from(admission.admitted.min(MAX_SYNC_RECORDS)).unwrap_or(0),
            missing_hashes,
            missing_writers,
        },
        admission.admitted > 0,
    ))
}

/// What one record exchange moved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordsExchanged {
    /// Envelopes the local store did not hold before.
    pub stored: usize,
    /// Envelopes the peer did not hold before.
    pub sent: usize,
    /// The backstop state after the exchange.
    pub backstop: Option<BackstopState>,
    /// Some direction has envelopes left for a later window.
    pub more: bool,
}

/// The record exchange a sync session runs with one peer ahead of the operation lanes.
pub struct RecordLink<'a> {
    /// Device-local record exchange state.
    pub state: &'a RecordExchange,
    /// The peer's device id, or another stable name of it when it is not known.
    pub peer: &'a str,
}

/// One device's side of a record exchange with one peer.
pub struct RecordSession<'a, T: SyncTransport> {
    pub transport: &'a T,
    pub ids: &'a PeerIds,
    pub engine: &'a Engine,
    pub pool: &'a sqlx::SqlitePool,
    pub ctx: &'a SpaceContext,
    pub state: &'a RecordExchange,
    /// The peer's device id, or another stable name of it.
    pub peer: &'a str,
    /// The peer's ingress class, from the local selected head.
    pub peer_class: PeerClass,
}

impl<T: SyncTransport> RecordSession<'_, T> {
    async fn conn(&self) -> Result<sqlx::pool::PoolConnection<sqlx::Sqlite>, String> {
        self.pool
            .acquire()
            .await
            .map_err(|error| format!("acquire sync connection: {error}"))
    }

    /// Pulls the peer's record pages, fetches what the store wants, and pushes the local
    /// store, within the windows of both sides.
    pub async fn exchange(&self) -> Result<RecordsExchanged, String> {
        self.state.start_session(self.peer);
        let mut exchanged = RecordsExchanged {
            backstop: self.backstop().await?,
            ..RecordsExchanged::default()
        };
        if exchanged.backstop == Some(BackstopState::Stopped) {
            return Ok(exchanged);
        }
        self.pull(&mut exchanged).await?;
        self.fetch_wanted(&mut exchanged).await?;
        self.push(&mut exchanged).await?;
        exchanged.backstop = self.backstop().await?;
        Ok(exchanged)
    }

    async fn backstop(&self) -> Result<Option<BackstopState>, String> {
        let mut conn = self.conn().await?;
        self.state
            .backstop(self.engine, &mut conn, self.ctx)
            .await
            .map_err(|error| format!("read authorization storage: {error}"))
    }

    /// Classes and admits `envelopes` from the peer, charging the ingress windows and the
    /// backstop. Returns how many the store did not hold.
    async fn take(&self, envelopes: Vec<Vec<u8>>) -> Result<usize, String> {
        let mut conn = self.conn().await?;
        let classes = self
            .engine
            .record_classes(&mut conn, self.ctx, &envelopes)
            .await
            .map_err(|error| format!("class authorization records: {error}"))?;
        let (all, not_kit) = sizes(&envelopes, &classes);
        self.state.received(self.peer, all, not_kit);
        let kept = self.state.backstop_filter(envelopes, &classes);
        let now = super::now_ms()?;
        let RecordAdmission { admitted, missing } = self
            .engine
            .admit_records(&mut conn, self.ctx, &kept, now)
            .await
            .map_err(|error| format!("store authorization records: {error}"))?;
        // Wanted entries a newly stored envelope references are asked for again right away.
        let mut inner = self.state.lock();
        for hash in &missing.hashes {
            inner.backoff.remove(hash.as_slice());
        }
        for writer in &missing.writers {
            inner.backoff.remove(writer.as_bytes().as_slice());
        }
        Ok(admitted)
    }

    /// The page bytes the ingress windows still allow, `None` once one is spent. A pulled page
    /// cannot be limited to kit envelopes, so pulling from a low peer stops with its low
    /// window; the source serves kit envelopes first, so they lead every later window.
    fn ingress_page(&self) -> Option<u32> {
        let (normal, low) = self.state.receive_budget(self.peer, self.peer_class);
        let bytes = normal.bytes.min(low.bytes);
        if bytes == 0 || normal.envelopes.min(low.envelopes) == 0 {
            return None;
        }
        Some(u32::try_from(bytes.min(SYNC_PAGE_BYTES)).unwrap_or(u32::MAX))
    }

    async fn pull(&self, exchanged: &mut RecordsExchanged) -> Result<(), String> {
        let mut cursor = self
            .state
            .lock()
            .pulled
            .get(self.peer)
            .copied()
            .unwrap_or_default();
        loop {
            let Some(max_bytes) = self.ingress_page() else {
                exchanged.more = true;
                return Ok(());
            };
            let request = ControlMessage::SyncRecords {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                cursor: cursor_to_wire(&cursor),
                max_bytes,
            };
            let response = self.transport.request(request).await?;
            if is_busy(&response) {
                exchanged.more = true;
                return Ok(());
            }
            let (records, next, more) =
                answer(response, "SyncRecordPage", |response| match response {
                    ControlMessage::SyncRecordPage {
                        records,
                        cursor,
                        more,
                    } => Some((records, cursor, more)),
                    _ => None,
                })?;
            let records = ops_from_wire(&records)?;
            let empty = records.is_empty();
            exchanged.stored += self.take(records).await?;
            cursor = cursor_from_wire(&next)?;
            self.state
                .lock()
                .pulled
                .insert(self.peer.to_string(), cursor);
            if !more || empty {
                exchanged.more |= more;
                return Ok(());
            }
        }
    }

    /// Asks the peer for what the store wants, following each chain until the peer has
    /// nothing more of it.
    async fn fetch_wanted(&self, exchanged: &mut RecordsExchanged) -> Result<(), String> {
        for _ in 0..WANTED_ROUNDS {
            let wants = {
                let mut conn = self.conn().await?;
                self.engine
                    .record_wants(&mut conn, self.ctx, MAX_SYNC_RECORDS)
                    .await
                    .map_err(|error| format!("read wanted records: {error}"))?
            };
            let wants = self.due(wants);
            if wants.is_empty() {
                return Ok(());
            }
            let (hashes, writers) = wants_to_wire(&wants);
            let request = ControlMessage::SyncRecordGet {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                hashes,
                writers,
            };
            let response = self.transport.request(request).await?;
            if is_busy(&response) {
                exchanged.more = true;
                return Ok(());
            }
            let records = answer(response, "SyncRecordFound", |response| match response {
                ControlMessage::SyncRecordFound { records } => Some(records),
                _ => None,
            })?;
            let records = ops_from_wire(&records)?;
            let stored = self.take(records).await?;
            exchanged.stored += stored;
            if stored == 0 {
                self.back_off(&wants);
                return Ok(());
            }
        }
        Ok(())
    }

    /// The part of `wants` due to be asked in this session.
    fn due(&self, wants: RecordWants) -> RecordWants {
        let inner = self.state.lock();
        let due = |key: &[u8]| {
            inner
                .backoff
                .get(key)
                .is_none_or(|backoff| backoff.next <= inner.sessions)
        };
        RecordWants {
            hashes: wants
                .hashes
                .into_iter()
                .filter(|hash| due(hash.as_slice()))
                .collect(),
            writers: wants
                .writers
                .into_iter()
                .filter(|writer| due(writer.as_bytes().as_slice()))
                .collect(),
        }
    }

    /// Doubles the wait of entries the peer answered unknown.
    fn back_off(&self, wants: &RecordWants) {
        let mut inner = self.state.lock();
        let sessions = inner.sessions;
        let keys = wants.hashes.iter().map(|hash| hash.to_vec()).chain(
            wants
                .writers
                .iter()
                .map(|writer| writer.as_bytes().to_vec()),
        );
        for key in keys {
            let backoff = inner.backoff.entry(key).or_default();
            backoff.wait = (backoff.wait.max(1) * 2).min(MAX_BACKOFF_SESSIONS);
            backoff.next = sessions + u64::from(backoff.wait);
        }
    }

    /// Pushes the local store past the peer's push cursor, then what the peer reports it
    /// lacks of the pushed envelopes.
    async fn push(&self, exchanged: &mut RecordsExchanged) -> Result<(), String> {
        let mut cursor = self
            .state
            .lock()
            .pushed
            .get(self.peer)
            .copied()
            .unwrap_or_default();
        let budget = self.state.serve_budget(self.peer);
        let mut spent = 0usize;
        loop {
            let page = {
                let mut conn = self.conn().await?;
                self.engine
                    .record_page(
                        &mut conn,
                        self.ctx,
                        &cursor,
                        SYNC_PAGE_BYTES.min(budget.bytes.saturating_sub(spent).max(1)),
                        MAX_SYNC_RECORDS,
                    )
                    .await
                    .map_err(|error| format!("read authorization records: {error}"))?
            };
            if page.envelopes.is_empty() {
                return Ok(());
            }
            let bytes: Vec<Vec<u8>> = page.envelopes.into_iter().map(|e| e.bytes).collect();
            let Some(missing) = self.send(&bytes, exchanged).await? else {
                exchanged.more = true;
                return Ok(());
            };
            spent += bytes.iter().map(Vec::len).sum::<usize>();
            self.state
                .served(self.peer, bytes.iter().map(Vec::len).sum(), bytes.len());
            cursor = page.cursor;
            self.state
                .lock()
                .pushed
                .insert(self.peer.to_string(), cursor);
            self.send_missing(missing, exchanged).await?;
            if !page.more {
                return Ok(());
            }
            if spent >= budget.bytes {
                exchanged.more = true;
                return Ok(());
            }
        }
    }

    /// Sends what the peer reported missing, following its chains, from the local store.
    async fn send_missing(
        &self,
        mut missing: RecordWants,
        exchanged: &mut RecordsExchanged,
    ) -> Result<(), String> {
        for _ in 0..WANTED_ROUNDS {
            if missing.is_empty() {
                return Ok(());
            }
            let found = {
                let mut conn = self.conn().await?;
                let mut found = self
                    .engine
                    .records_by_hash(&mut conn, self.ctx, &missing.hashes)
                    .await
                    .map_err(|error| format!("read authorization records: {error}"))?;
                found.extend(
                    self.engine
                        .certificates_of(&mut conn, self.ctx, &missing.writers)
                        .await
                        .map_err(|error| format!("read writer certificates: {error}"))?,
                );
                found
            };
            let bytes: Vec<Vec<u8>> = found
                .into_iter()
                .take(MAX_SYNC_RECORDS)
                .map(|e| e.bytes)
                .collect();
            if bytes.is_empty() {
                return Ok(());
            }
            match self.send(&bytes, exchanged).await? {
                Some(next) => missing = next,
                None => {
                    exchanged.more = true;
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Pushes `records`, returning what the peer still lacks of them, or `None` while its
    /// window is spent.
    async fn send(
        &self,
        records: &[Vec<u8>],
        exchanged: &mut RecordsExchanged,
    ) -> Result<Option<RecordWants>, String> {
        let request = ControlMessage::SyncRecordPush {
            protocol_version: PROTOCOL_VERSION,
            vault_id: self.ids.vault_id.clone(),
            device_id: self.ids.device_id.clone(),
            records: ops_to_wire(records),
        };
        let response = self.transport.request(request).await?;
        if is_busy(&response) {
            return Ok(None);
        }
        let (admitted, hashes, writers) =
            answer(
                response,
                "SyncRecordPushResult",
                |response| match response {
                    ControlMessage::SyncRecordPushResult {
                        admitted,
                        missing_hashes,
                        missing_writers,
                    } => Some((admitted, missing_hashes, missing_writers)),
                    _ => None,
                },
            )?;
        exchanged.sent += admitted as usize;
        wants_from_wire(&hashes, &writers).map(Some)
    }
}

/// The error code of a record request answered while the peer's window is spent.
pub const RECORDS_BUSY: &str = "sync_records_busy";

fn is_busy(response: &ControlMessage) -> bool {
    matches!(response, ControlMessage::Error { code, .. } if code == RECORDS_BUSY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync::ForkedWriter;

    const MIB: u64 = 1024 * 1024;

    fn storage(
        bytes: u64,
        by_device: Vec<(&str, u64)>,
        forked: Vec<ForkedWriter>,
    ) -> AuthorizationStorage {
        AuthorizationStorage {
            bytes,
            envelopes: 1,
            by_device: by_device
                .into_iter()
                .map(|(device, bytes)| (device.to_string(), bytes))
                .collect(),
            forked_writers: forked,
        }
    }

    #[test]
    fn a_low_budget_preference_stays_between_the_minimum_and_the_default() {
        assert_eq!(RecordBudget::low(None), LOW_BUDGET);
        let tiny = RecordBudget {
            bytes: 1,
            envelopes: 1,
        };
        assert_eq!(RecordBudget::low(Some(tiny)), LOW_BUDGET_MIN);
        assert_eq!(RecordBudget::low(Some(NORMAL_BUDGET)), LOW_BUDGET);
        let between = RecordBudget {
            bytes: 32 * 1024,
            envelopes: 32,
        };
        assert_eq!(RecordBudget::low(Some(between)), between);
    }

    #[test]
    fn a_window_spends_until_its_budget_and_renews_after_its_length() {
        let start = Instant::now();
        let mut window = Window::new(start);
        window.spend(NORMAL_BUDGET.bytes - 10, 5);
        assert_eq!(
            window.left(NORMAL_BUDGET),
            RecordBudget {
                bytes: 10,
                envelopes: NORMAL_BUDGET.envelopes - 5,
            }
        );
        window.spend(100, NORMAL_BUDGET.envelopes);
        assert_eq!(window.left(NORMAL_BUDGET).bytes, 0);
        assert_eq!(window.left(NORMAL_BUDGET).envelopes, 0);
        let same = window.at(start + RECORD_WINDOW / 2);
        assert_eq!(same.left(NORMAL_BUDGET).bytes, 0);
        let next = window.at(start + RECORD_WINDOW);
        assert_eq!(next.left(NORMAL_BUDGET), NORMAL_BUDGET);
    }

    #[test]
    fn the_backstop_keeps_part_of_its_reserve_for_kit_records() {
        let shared = BACKSTOP_RESERVE - BACKSTOP_KIT_RESERVE;
        let mut backstop = Backstop::default();
        assert!(backstop.charge(RecordClass::Contest, shared - MIB));
        // Contest and wanted records cannot reach into the kit part.
        assert!(!backstop.charge(RecordClass::Wanted, 2 * MIB));
        assert!(backstop.stopped);

        let mut backstop = Backstop::default();
        assert!(backstop.charge(RecordClass::Contest, shared));
        assert!(backstop.charge(RecordClass::Kit, BACKSTOP_KIT_RESERVE));
        assert!(!backstop.charge(RecordClass::Kit, 1));
        assert!(backstop.stopped);

        // Kit records may spend the whole reserve.
        let mut backstop = Backstop::default();
        assert!(backstop.charge(RecordClass::Kit, BACKSTOP_RESERVE));
        assert!(!backstop.stopped);
    }

    #[test]
    fn the_backstop_filter_stores_only_kit_contest_and_wanted_records() {
        let state = RecordExchange::default();
        let envelopes = vec![vec![1; 8], vec![2; 8], vec![3; 8], vec![4; 8], vec![5; 8]];
        let classes = RECORD_CLASSES_IN_ORDER;
        assert_eq!(state.backstop_filter(envelopes.clone(), &classes).len(), 5);
        state.lock().backstop = Some(Backstop::default());
        assert_eq!(
            state.backstop_filter(envelopes.clone(), &classes),
            vec![vec![1; 8], vec![2; 8], vec![3; 8]]
        );
        state.lock().backstop = Some(Backstop {
            stopped: true,
            ..Backstop::default()
        });
        assert!(state.backstop_filter(envelopes, &classes).is_empty());
        state.resume();
        assert!(state.lock().backstop.is_none());
    }

    const RECORD_CLASSES_IN_ORDER: [RecordClass; 5] = [
        RecordClass::Kit,
        RecordClass::Contest,
        RecordClass::Wanted,
        RecordClass::Live,
        RecordClass::Rest,
    ];

    #[test]
    fn growth_alerts_name_heavy_devices_a_heavy_store_and_forked_writers() {
        assert!(growth_alerts(&storage(MIB, vec![("phone", MIB)], vec![])).is_empty());
        let writer = WriterId::from_bytes([7; 16]);
        let alerts = growth_alerts(&storage(
            ALERT_TOTAL_BYTES + 1,
            vec![
                ("flooder", ALERT_DEVICE_BYTES + 1),
                ("phone", ALERT_DEVICE_BYTES),
            ],
            vec![ForkedWriter {
                writer,
                device_id: Some("forker".to_string()),
            }],
        ));
        assert_eq!(
            alerts,
            vec![
                GrowthAlert::Device {
                    device_id: "flooder".to_string(),
                    bytes: ALERT_DEVICE_BYTES + 1,
                },
                GrowthAlert::Total {
                    bytes: ALERT_TOTAL_BYTES + 1,
                },
                GrowthAlert::ForkedWriter {
                    writer,
                    device_id: Some("forker".to_string()),
                },
            ]
        );
    }

    #[test]
    fn a_session_with_a_new_peer_lets_every_wanted_entry_be_asked_again() {
        let state = RecordExchange::default();
        state.lock().backoff.insert(
            vec![1],
            Backoff {
                next: 100,
                wait: 64,
            },
        );
        state
            .lock()
            .pulled
            .insert("hub".to_string(), RecordCursor::default());
        state.start_session("hub");
        assert_eq!(state.lock().backoff.len(), 1);
        state.start_session("laptop");
        assert!(state.lock().backoff.is_empty());
    }
}

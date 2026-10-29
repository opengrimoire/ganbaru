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
use ganbaru_sync_contracts::{Content, Envelope, OperationKind, WriterId};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

const _: () = assert!(RECORD_CLASSES == SYNC_RECORD_CLASSES);

/// Bytes of unpaired data envelopes kept per peer and [`Stream`]: two of the largest operation
/// and at least one page. The engine stores the halves of a fork pair next to each other, so a
/// page splits at most the one pair at its boundary, and its first half waits here for the page
/// that starts with the other even when the rest of that page is unpaired.
const UNPAIRED_BYTES: usize = 2 * ganbaru_sync_contracts::bounds::MAX_OPERATION_BYTES;
const _: () = assert!(SYNC_PAGE_BYTES <= UNPAIRED_BYTES);

/// The stream a batch from a peer arrives on, each with its own hold of unpaired data
/// envelopes, so a batch of one stream does not evict the half another stream waits to pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Stream {
    /// Pages this device pulls from the peer.
    Pull,
    /// Answers to what this device asks the peer for.
    Wanted,
    /// Pages the peer pushes.
    Push,
}

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

/// The device-local bounds of record exchange: windows, budgets, the storage backstop, and
/// growth alerts. [`Default`] holds the R7.3 A6 to A8 values; tests scale them down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordLimits {
    /// Length of one admission window.
    pub window: Duration,
    /// What one peer is served, and what a normal peer may send, per window.
    pub normal: RecordBudget,
    /// What a low peer may send per window by default.
    pub low: RecordBudget,
    /// The smallest low budget a device preference may set.
    pub low_min: RecordBudget,
    /// Stored envelope bytes past which the backstop starts.
    pub backstop_bytes: u64,
    /// Stored envelopes past which the backstop starts.
    pub backstop_envelopes: u64,
    /// Free bytes on the vault volume under which the backstop starts.
    pub backstop_free_bytes: u64,
    /// Bytes the backstop still stores, for kit and contest records and their ancestors.
    pub reserve: u64,
    /// The part of the reserve only kit records and their ancestors may spend.
    pub kit_reserve: u64,
    /// Free bytes on the vault volume under which record exchange stops fully.
    pub hard_stop_free_bytes: u64,
    /// Envelope bytes one device carries past which Settings > Sync alerts.
    pub alert_device_bytes: u64,
    /// Stored envelope bytes past which Settings > Sync alerts.
    pub alert_total_bytes: u64,
    /// Wanted requests one session sends a peer at most.
    pub wanted_rounds: usize,
    /// Wanted keys one request asks for at most.
    pub wants_per_request: usize,
}

impl Default for RecordLimits {
    fn default() -> Self {
        Self {
            window: RECORD_WINDOW,
            normal: NORMAL_BUDGET,
            low: LOW_BUDGET,
            low_min: LOW_BUDGET_MIN,
            backstop_bytes: BACKSTOP_BYTES,
            backstop_envelopes: BACKSTOP_ENVELOPES,
            backstop_free_bytes: BACKSTOP_FREE_BYTES,
            reserve: BACKSTOP_RESERVE,
            kit_reserve: BACKSTOP_KIT_RESERVE,
            hard_stop_free_bytes: HARD_STOP_FREE_BYTES,
            alert_device_bytes: ALERT_DEVICE_BYTES,
            alert_total_bytes: ALERT_TOTAL_BYTES,
            wanted_rounds: WANTED_ROUNDS,
            wants_per_request: MAX_SYNC_RECORDS,
        }
    }
}

impl RecordLimits {
    /// The low budget a device preference asks for, kept between the minimum and default.
    pub fn low_budget(&self, preference: Option<RecordBudget>) -> RecordBudget {
        let wanted = preference.unwrap_or(self.low);
        RecordBudget {
            bytes: wanted.bytes.clamp(self.low_min.bytes, self.low.bytes),
            envelopes: wanted
                .envelopes
                .clamp(self.low_min.envelopes, self.low.envelopes),
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

    /// The window at `now` for windows of `length` and `budget`. Once the current one ended,
    /// each window that passed repays one `budget` of what was spent, so a page taken whole past
    /// the budget is debt the next windows carry.
    fn at(self, now: Instant, length: Duration, budget: RecordBudget) -> Self {
        let elapsed = now.duration_since(self.started);
        if elapsed < length {
            return self;
        }
        if length.is_zero() {
            return Self::new(now);
        }
        let passed = elapsed.as_nanos() / length.as_nanos();
        let passed = usize::try_from(passed).unwrap_or(usize::MAX);
        let into = elapsed.as_nanos() % length.as_nanos();
        Self {
            started: now
                .checked_sub(Duration::from_nanos(u64::try_from(into).unwrap_or(0)))
                .unwrap_or(now),
            bytes: self
                .bytes
                .saturating_sub(budget.bytes.saturating_mul(passed)),
            envelopes: self
                .envelopes
                .saturating_sub(budget.envelopes.saturating_mul(passed)),
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
    /// Charges `bytes` of `class` to the reserve of `limits`. Returns whether the reserve
    /// covered them. Kit records and their ancestors may spend the whole reserve; every other
    /// class only the part outside the kit slice. A miss outside the kit slice leaves the kit
    /// slice open, so a flood of contest records or deep junk chains cannot stop a kit
    /// handover. The backstop stops fully only when a miss `stops`: a kit record whose carrier
    /// the binding fixes finds the reserve spent.
    fn charge(
        &mut self,
        limits: &RecordLimits,
        class: RecordClass,
        bytes: u64,
        stops: bool,
    ) -> bool {
        let shared = limits.reserve.saturating_sub(limits.kit_reserve);
        let covered = match class {
            RecordClass::Kit => self.spent_kit + self.spent_other + bytes <= limits.reserve,
            _ => self.spent_other + bytes <= shared,
        };
        match (covered, class) {
            (true, RecordClass::Kit) => self.spent_kit += bytes,
            (true, _) => self.spent_other += bytes,
            (false, RecordClass::Kit) if stops => self.stopped = true,
            (false, _) => {}
        }
        covered
    }
}

/// A growth alert of Settings > Sync, see the R7.3 A8 rule. Display only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrowthAlert {
    /// Envelopes one device's writers carry exceed [`RecordLimits::alert_device_bytes`].
    Device { device_id: String, bytes: u64 },
    /// Stored envelopes exceed [`RecordLimits::alert_total_bytes`].
    Total { bytes: u64 },
    /// A writer holds two envelopes at one sequence or two certificates. `device_id` is the
    /// device a stored certificate of the writer names, if any.
    ForkedWriter {
        writer: WriterId,
        device_id: Option<String>,
    },
}

/// The growth alerts `storage` raises under `limits`.
pub fn growth_alerts(storage: &AuthorizationStorage, limits: &RecordLimits) -> Vec<GrowthAlert> {
    let mut alerts: Vec<GrowthAlert> = storage
        .by_device
        .iter()
        .filter(|(_, bytes)| *bytes > limits.alert_device_bytes)
        .map(|(device_id, bytes)| GrowthAlert::Device {
            device_id: device_id.clone(),
            bytes: *bytes,
        })
        .collect();
    if storage.bytes > limits.alert_total_bytes {
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

/// The clock admission windows read.
pub type RecordClock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// Wanted-set keys of a decoded envelope: its envelope hash, its entry hash when it is a
/// record, and its writer when it is a genesis certificate.
fn envelope_keys(envelope: &Envelope<'_>) -> Vec<Vec<u8>> {
    let mut keys = vec![envelope.hash().to_vec()];
    match OperationKind::from_u8(envelope.kind) {
        Some(OperationKind::Genesis) => keys.push(envelope.writer.as_bytes().to_vec()),
        Some(OperationKind::Authorization) => {
            if let Some(record) = authorization_record(envelope) {
                keys.push(record.0);
            }
        }
        _ => {}
    }
    keys
}

/// Wanted-set keys an envelope references: its chain prefix, its writer's certificate, and
/// its parent entry when it is a record.
fn referenced_keys(envelope: &Envelope<'_>) -> Vec<Vec<u8>> {
    let mut keys = vec![
        envelope.previous_hash.to_vec(),
        envelope.writer.as_bytes().to_vec(),
    ];
    if let Some((_, parent)) = authorization_record(envelope) {
        keys.push(parent);
    }
    keys
}

/// The entry hash and parent entry hash of an authorization record envelope.
fn authorization_record(envelope: &Envelope<'_>) -> Option<(Vec<u8>, Vec<u8>)> {
    let operation = envelope.operation().ok()?;
    match &operation.content {
        Content::Authorization(record) => {
            Some((record.hash().to_vec(), record.previous_entry_hash.to_vec()))
        }
        _ => None,
    }
}

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
    /// Retry timing of wanted entries each peer answered unknown, by peer.
    backoff: HashMap<String, BTreeMap<Vec<u8>, Backoff>>,
    /// The wanted key each peer's next lookup continues after, so a session resumes the
    /// rotation through the wanted set where the last answered request left it.
    wanted_after: HashMap<String, Vec<u8>>,
    /// The latest data envelopes each peer sent on each stream that the store did not hold, at
    /// most [`UNPAIRED_BYTES`], sent again with that peer's next batch on the stream so the
    /// halves of a fork pair that a page boundary split still arrive together.
    unpaired: HashMap<(String, Stream), VecDeque<Vec<u8>>>,
    /// Sessions run, for backoff.
    sessions: u64,
    /// The backstop once it started.
    backstop: Option<Backstop>,
    /// The low budget a device preference sets.
    low: Option<RecordBudget>,
    /// Wanted-set keys that kit records stored past the backstop reference, directly or
    /// through ancestors stored the same way. Envelopes matching them are charged as kit
    /// records, so the ancestors a kit handover wants draw on the kit slice of the reserve.
    kit_lineage: BTreeSet<Vec<u8>>,
    /// Peers whose last session failed, so the next one is a new connection.
    away: BTreeSet<String>,
}

/// Device-local state of record exchange, shared by every session and listener of one
/// installation: cursors, admission windows, wanted retry timing, and the storage backstop.
#[derive(Clone)]
pub struct RecordExchange {
    inner: Arc<Mutex<Inner>>,
    free_space: Option<FreeSpace>,
    limits: RecordLimits,
    clock: RecordClock,
}

impl Default for RecordExchange {
    fn default() -> Self {
        Self {
            inner: Arc::default(),
            free_space: None,
            limits: RecordLimits::default(),
            clock: Arc::new(Instant::now),
        }
    }
}

impl RecordExchange {
    /// Sets how the backstop reads free space on the vault volume.
    #[must_use]
    pub fn with_free_space(mut self, free_space: FreeSpace) -> Self {
        self.free_space = Some(free_space);
        self
    }

    /// Sets the windows, budgets, backstop bounds, and alert thresholds.
    #[must_use]
    pub fn with_limits(mut self, limits: RecordLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Sets the clock admission windows read.
    #[must_use]
    pub fn with_clock(mut self, clock: RecordClock) -> Self {
        self.clock = clock;
        self
    }

    /// The bounds this exchange applies.
    pub fn limits(&self) -> &RecordLimits {
        &self.limits
    }

    /// Free bytes on the vault volume, `None` when no reading is set or the platform cannot
    /// tell; the backstop then applies its byte and envelope bounds alone.
    pub fn free_space(&self) -> Option<u64> {
        self.free_space.as_ref().and_then(|free| free())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the low ingress budget from a device preference, clamped to its bounds.
    pub fn set_low_budget(&self, preference: Option<RecordBudget>) {
        let low = preference.map(|budget| self.limits.low_budget(Some(budget)));
        self.lock().low = low;
    }

    /// Ends the backstop after the user freed space, raised the bound, or re-founded the
    /// space. It starts again at the next exchange if the store is still past a bound.
    pub fn resume(&self) {
        let mut inner = self.lock();
        inner.backstop = None;
        // Pages pulled while it ran left out what it did not store.
        inner.pulled.clear();
        inner.kit_lineage.clear();
    }

    /// Forgets the state of the vault this exchange served: cursors, windows, wanted retry
    /// timing, the backstop with its spend, and the kit lineage. The low budget preference is
    /// device-wide and stays. Called when the runtime enters another vault, since peer ids and
    /// stores differ between vaults.
    pub fn reset_vault(&self) {
        let mut inner = self.lock();
        let low = inner.low;
        *inner = Inner {
            low,
            ..Inner::default()
        };
    }

    /// Whether this exchange holds state of a vault: a cursor, a window, wanted retry timing or
    /// rotation, a session count, the backstop, the kit lineage, or a peer marked away.
    pub fn holds_vault_state(&self) -> bool {
        let inner = self.lock();
        !(inner.pulled.is_empty()
            && inner.pushed.is_empty()
            && inner.served.is_empty()
            && inner.received.is_empty()
            && inner.backoff.is_empty()
            && inner.wanted_after.is_empty()
            && inner.unpaired.is_empty()
            && inner.sessions == 0
            && inner.backstop.is_none()
            && inner.kit_lineage.is_empty()
            && inner.away.is_empty())
    }

    /// Forgets a peer's cursors, as when it connects anew, and lets every wanted entry be asked
    /// of it again. The rotation through the wanted set continues where it was, and the retry
    /// timing other peers set stays.
    pub fn connected(&self, peer: &str) {
        let mut inner = self.lock();
        Self::connect(&mut inner, peer);
    }

    fn connect(inner: &mut Inner, peer: &str) {
        inner.pulled.remove(peer);
        inner.pushed.remove(peer);
        inner.backoff.remove(peer);
        inner.unpaired.retain(|(holder, _), _| holder != peer);
        inner.away.remove(peer);
    }

    /// Takes the unpaired data envelopes `peer` sent before on `stream`, to send with its next
    /// batch there. None while the backstop runs, since it stores no fork evidence beyond its
    /// reserve.
    fn take_unpaired(&self, peer: &str, stream: Stream) -> Vec<Vec<u8>> {
        let mut inner = self.lock();
        let unpaired = inner
            .unpaired
            .remove(&(peer.to_string(), stream))
            .unwrap_or_default();
        if inner.backstop.is_some() {
            return Vec::new();
        }
        unpaired.into()
    }

    /// Keeps the latest of `envelopes`, in order, that fit in [`UNPAIRED_BYTES`] as the
    /// unpaired data envelopes of `peer` on `stream`.
    fn keep_unpaired(&self, peer: &str, stream: Stream, envelopes: Vec<Vec<u8>>) {
        let mut kept = VecDeque::new();
        let mut bytes = 0;
        for envelope in envelopes.into_iter().rev() {
            if kept.len() == MAX_SYNC_RECORDS || bytes + envelope.len() > UNPAIRED_BYTES {
                break;
            }
            bytes += envelope.len();
            kept.push_front(envelope);
        }
        let key = (peer.to_string(), stream);
        let mut inner = self.lock();
        if kept.is_empty() || inner.backstop.is_some() {
            inner.unpaired.remove(&key);
        } else {
            inner.unpaired.insert(key, kept);
        }
    }

    /// Admits `envelopes` from `peer` on `stream` after the unpaired data envelopes it sent
    /// before there, and keeps the data envelopes the store still does not hold for its next
    /// batch. The held envelopes are charged to the peer's ingress windows again, since each
    /// batch admits them again.
    #[allow(clippy::too_many_arguments)]
    async fn admit_from(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        (peer, class): (&str, PeerClass),
        stream: Stream,
        envelopes: Vec<Vec<u8>>,
        now_ms: u64,
    ) -> Result<RecordAdmission, SyncError> {
        let mut batch = self.take_unpaired(peer, stream);
        if !batch.is_empty() {
            let held = (batch.iter().map(Vec::len).sum(), batch.len());
            self.received(peer, class, held, held);
        }
        batch.extend(envelopes);
        let admission = engine.admit_records(conn, ctx, &batch, now_ms).await?;
        let unpaired = admission
            .unpaired
            .iter()
            .filter_map(|index| batch.get(*index).cloned())
            .collect();
        self.keep_unpaired(peer, stream, unpaired);
        Ok(admission)
    }

    /// Marks a peer whose session failed, so the next session with it starts as a new
    /// connection, see [`Self::connected`].
    pub fn disconnected(&self, peer: &str) {
        self.lock().away.insert(peer.to_string());
    }

    /// Counts a session with `peer`. A peer whose last session failed connects anew, see
    /// [`Self::connected`]; for a peer this device has no pull cursor for, every wanted entry may
    /// be asked of it again, see the R7.3 A4 rule.
    fn start_session(&self, peer: &str) {
        let mut inner = self.lock();
        inner.sessions += 1;
        if inner.away.contains(peer) {
            Self::connect(&mut inner, peer);
        } else if !inner.pulled.contains_key(peer) {
            inner.backoff.remove(peer);
        }
    }

    /// Wanted keys whose backoff with `peer` has not ended, which a lookup leaves out before
    /// its limit.
    pub(crate) fn backed_off(&self, peer: &str) -> BTreeSet<Vec<u8>> {
        let inner = self.lock();
        inner
            .backoff
            .get(peer)
            .into_iter()
            .flatten()
            .filter(|(_, backoff)| backoff.next > inner.sessions)
            .map(|(key, _)| key.clone())
            .collect()
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

    /// Bytes the backstop charged to the kit reserve so far, 0 while full sync runs.
    #[cfg(test)]
    pub(crate) fn kit_spent(&self) -> u64 {
        self.lock()
            .backstop
            .map_or(0, |backstop| backstop.spent_kit)
    }

    /// The backstop state, `None` while full sync runs. Starts the backstop when the store or
    /// the volume crossed a bound.
    pub async fn backstop(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> Result<Option<BackstopState>, SyncError> {
        let free = self.free_space();
        if self.lock().backstop.is_none() {
            let storage = engine.authorization_storage(conn, ctx).await?;
            let full = storage.bytes > self.limits.backstop_bytes
                || storage.envelopes > self.limits.backstop_envelopes
                || free.is_some_and(|free| free < self.limits.backstop_free_bytes);
            if !full {
                return Ok(None);
            }
            self.lock().backstop = Some(Backstop::default());
        }
        let mut inner = self.lock();
        let backstop = inner.backstop.get_or_insert_with(Backstop::default);
        if free.is_some_and(|free| free < self.limits.hard_stop_free_bytes) {
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
        let mut inner = self.lock();
        self.serve_window(&mut inner, peer).left(self.limits.normal)
    }

    fn served(&self, peer: &str, bytes: usize, envelopes: usize) {
        let mut inner = self.lock();
        self.serve_window(&mut inner, peer).spend(bytes, envelopes);
    }

    fn serve_window<'a>(&self, inner: &'a mut Inner, peer: &str) -> &'a mut Window {
        let now = (self.clock)();
        let window = inner
            .served
            .entry(peer.to_string())
            .or_insert_with(|| Window::new(now));
        *window = window.at(now, self.limits.window, self.limits.normal);
        window
    }

    /// What `peer` of `class` may still send in this window, by the normal and the low budget.
    /// Kit envelopes count against the normal budget only.
    fn receive_budget(&self, peer: &str, class: PeerClass) -> (RecordBudget, RecordBudget) {
        let mut inner = self.lock();
        let low = self.low_of(&inner, class);
        let windows = self.receive_windows(&mut inner, peer, class);
        (windows.0.left(self.limits.normal), windows.1.left(low))
    }

    fn received(&self, peer: &str, class: PeerClass, all: (usize, usize), not_kit: (usize, usize)) {
        let mut inner = self.lock();
        let windows = self.receive_windows(&mut inner, peer, class);
        windows.0.spend(all.0, all.1);
        windows.1.spend(not_kit.0, not_kit.1);
    }

    fn low_of(&self, inner: &Inner, class: PeerClass) -> RecordBudget {
        match class {
            PeerClass::Normal => self.limits.normal,
            PeerClass::Low => inner.low.unwrap_or(self.limits.low),
        }
    }

    fn receive_windows<'a>(
        &self,
        inner: &'a mut Inner,
        peer: &str,
        class: PeerClass,
    ) -> &'a mut (Window, Window) {
        let now = (self.clock)();
        let low = self.low_of(inner, class);
        let windows = inner
            .received
            .entry(peer.to_string())
            .or_insert_with(|| (Window::new(now), Window::new(now)));
        windows.0 = windows.0.at(now, self.limits.window, self.limits.normal);
        windows.1 = windows.1.at(now, self.limits.window, low);
        windows
    }

    /// The envelopes of `envelopes` the backstop still stores, charged to its reserve: kit and
    /// contest envelopes and wanted entries. Every envelope while it is not running.
    ///
    /// An envelope a stored class 1 envelope reaches through its writer chain, its writer's
    /// genesis certificate, or its parent entry counts as kit, in this batch and in later ones,
    /// so the ancestors a kit handover wants are charged to the kit slice. Only envelopes the
    /// engine classes 1 seed that lineage, each lineage key is spent by the first envelope that
    /// matches it, and a lineage envelope that misses the reserve never stops the backstop.
    fn backstop_filter(&self, envelopes: Vec<Vec<u8>>, classes: &[RecordClass]) -> Vec<Vec<u8>> {
        let limits = self.limits;
        let mut guard = self.lock();
        let inner = &mut *guard;
        let Some(backstop) = inner.backstop.as_mut() else {
            return envelopes;
        };
        let decoded: Vec<Option<Envelope<'_>>> = envelopes
            .iter()
            .map(|bytes| Envelope::decode(bytes).ok())
            .collect();
        let mut kit: Vec<bool> = classes
            .iter()
            .map(|class| *class == RecordClass::Kit)
            .collect();
        let mut lineage = inner.kit_lineage.clone();
        let mut matched = BTreeSet::new();
        loop {
            let mut grew = false;
            for (index, envelope) in decoded.iter().enumerate() {
                let Some(envelope) = envelope else { continue };
                if classes[index] > RecordClass::Wanted {
                    continue;
                }
                if !kit[index] {
                    let keys: Vec<Vec<u8>> = envelope_keys(envelope)
                        .into_iter()
                        .filter(|key| lineage.contains(key) && !matched.contains(key))
                        .collect();
                    if !keys.is_empty() {
                        kit[index] = true;
                        matched.extend(keys);
                    }
                }
                if kit[index] {
                    for key in referenced_keys(envelope) {
                        grew |= lineage.insert(key);
                    }
                }
            }
            if !grew {
                break;
            }
        }
        for key in &matched {
            inner.kit_lineage.remove(key);
        }
        let mut stored = Vec::new();
        for (index, bytes) in envelopes.iter().enumerate() {
            let bound = classes[index] == RecordClass::Kit;
            let class = if kit[index] {
                RecordClass::Kit
            } else {
                classes[index]
            };
            if class > RecordClass::Wanted
                || backstop.stopped
                || !backstop.charge(&limits, class, bytes.len() as u64, bound)
            {
                continue;
            }
            if class == RecordClass::Kit
                && let Some(envelope) = &decoded[index]
            {
                inner.kit_lineage.extend(
                    referenced_keys(envelope)
                        .into_iter()
                        .filter(|key| !matched.contains(key)),
                );
            }
            stored.push(index);
        }
        drop(decoded);
        let keep: BTreeSet<usize> = stored.into_iter().collect();
        envelopes
            .into_iter()
            .enumerate()
            .filter(|(index, _)| keep.contains(index))
            .map(|(_, bytes)| bytes)
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
            let max_bytes = budget.bytes.min(SYNC_PAGE_BYTES);
            let max_envelopes = budget.envelopes.min(MAX_SYNC_RECORDS);
            let mut found = engine
                .records_by_hash(conn, ctx, &wants.hashes, max_bytes, max_envelopes)
                .await
                .map_err(failed)?;
            found.extend(
                engine
                    .certificates_of(conn, ctx, &wants.writers, max_bytes, max_envelopes)
                    .await
                    .map_err(failed)?,
            );
            let mut bytes = Vec::new();
            let mut size = 0;
            for envelope in found {
                let fits = bytes.len() < max_envelopes && size + envelope.bytes.len() <= max_bytes;
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
            // An empty push carries nothing to class or admit, and must not admit the
            // peer's held envelopes again outside its windows.
            if records.is_empty() {
                return Ok((
                    ControlMessage::SyncRecordPushResult {
                        admitted: 0,
                        missing_hashes: Vec::new(),
                        missing_writers: Vec::new(),
                    },
                    false,
                ));
            }
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
            admit_pushed(
                state,
                engine,
                conn,
                ctx,
                (peer, class),
                kept,
                all,
                not_kit,
                busy,
            )
            .await
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
    (peer, class): (&str, PeerClass),
    records: Vec<Vec<u8>>,
    all: (usize, usize),
    not_kit: (usize, usize),
    busy: bool,
) -> Result<(ControlMessage, bool), RecordRefusal> {
    let now = super::now_ms().map_err(RecordRefusal::Failed)?;
    state.received(peer, class, all, if busy { (0, 0) } else { not_kit });
    let admission = state
        .admit_from(engine, conn, ctx, (peer, class), Stream::Push, records, now)
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

    /// Classes and admits `envelopes` the peer sent on `stream`, charging the ingress windows and
    /// the backstop. Returns how many the store did not hold.
    async fn take(&self, envelopes: Vec<Vec<u8>>, stream: Stream) -> Result<usize, String> {
        if envelopes.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn().await?;
        let classes = self
            .engine
            .record_classes(&mut conn, self.ctx, &envelopes)
            .await
            .map_err(|error| format!("class authorization records: {error}"))?;
        let (all, not_kit) = sizes(&envelopes, &classes);
        self.state
            .received(self.peer, self.peer_class, all, not_kit);
        let kept = self.state.backstop_filter(envelopes, &classes);
        let now = super::now_ms()?;
        let RecordAdmission {
            admitted, missing, ..
        } = self
            .state
            .admit_from(
                self.engine,
                &mut conn,
                self.ctx,
                (self.peer, self.peer_class),
                stream,
                kept,
                now,
            )
            .await
            .map_err(|error| format!("store authorization records: {error}"))?;
        // Wanted entries a newly stored envelope references are asked for again right away.
        let mut inner = self.state.lock();
        for backoff in inner.backoff.values_mut() {
            for hash in &missing.hashes {
                backoff.remove(hash.as_slice());
            }
            for writer in &missing.writers {
                backoff.remove(writer.as_bytes().as_slice());
            }
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
            exchanged.stored += self.take(records, Stream::Pull).await?;
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
    /// nothing more of it. Keys the peer answered unknown back off with this peer and leave
    /// room for the next ones in the same session. Each answered request moves this peer's
    /// rotation through the wanted set on, and the next session resumes it, so every wanted key
    /// is asked within a number of answered requests bounded by the wanted set's size.
    /// Ancestors of kit records and admin yields lead the first request and each one after a
    /// request that stored records, so the record store is walked for them only when it changed.
    async fn fetch_wanted(&self, exchanged: &mut RecordsExchanged) -> Result<(), String> {
        let mut lead = true;
        let limits = self.state.limits();
        let per_request = limits.wants_per_request.clamp(1, MAX_SYNC_RECORDS);
        for _ in 0..limits.wanted_rounds {
            // An answer cannot be limited, so asking stops once an ingress window is spent; what an
            // answer takes past it is debt the later windows carry.
            if self.ingress_page().is_none() {
                exchanged.more = true;
                return Ok(());
            }
            let skip = self.state.backed_off(self.peer);
            let after = self
                .state
                .lock()
                .wanted_after
                .get(self.peer)
                .cloned()
                .unwrap_or_default();
            let (wants, next) = {
                let mut conn = self.conn().await?;
                self.engine
                    .record_wants_after(&mut conn, self.ctx, per_request, &skip, &after, lead)
                    .await
                    .map_err(|error| format!("read wanted records: {error}"))?
            };
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
            self.state
                .lock()
                .wanted_after
                .insert(self.peer.to_string(), next);
            let records = ops_from_wire(&records)?;
            let stored = self.take(records, Stream::Wanted).await?;
            exchanged.stored += stored;
            lead = stored > 0;
            if stored == 0 {
                self.back_off(&wants);
            }
        }
        Ok(())
    }

    /// Doubles the wait with this peer of entries it answered unknown.
    fn back_off(&self, wants: &RecordWants) {
        let mut inner = self.state.lock();
        let sessions = inner.sessions;
        let keys = wants.hashes.iter().map(|hash| hash.to_vec()).chain(
            wants
                .writers
                .iter()
                .map(|writer| writer.as_bytes().to_vec()),
        );
        let peer = inner.backoff.entry(self.peer.to_string()).or_default();
        for key in keys {
            let backoff = peer.entry(key).or_default();
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
                    .records_by_hash(
                        &mut conn,
                        self.ctx,
                        &missing.hashes,
                        SYNC_PAGE_BYTES,
                        MAX_SYNC_RECORDS,
                    )
                    .await
                    .map_err(|error| format!("read authorization records: {error}"))?;
                found.extend(
                    self.engine
                        .certificates_of(
                            &mut conn,
                            self.ctx,
                            &missing.writers,
                            SYNC_PAGE_BYTES,
                            MAX_SYNC_RECORDS,
                        )
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
        let limits = RecordLimits::default();
        assert_eq!(limits.low_budget(None), LOW_BUDGET);
        let tiny = RecordBudget {
            bytes: 1,
            envelopes: 1,
        };
        assert_eq!(limits.low_budget(Some(tiny)), LOW_BUDGET_MIN);
        assert_eq!(limits.low_budget(Some(NORMAL_BUDGET)), LOW_BUDGET);
        let between = RecordBudget {
            bytes: 32 * 1024,
            envelopes: 32,
        };
        assert_eq!(limits.low_budget(Some(between)), between);
    }

    #[test]
    fn a_window_spends_until_its_budget_and_carries_overspend_into_later_windows() {
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
        let same = window.at(start + RECORD_WINDOW / 2, RECORD_WINDOW, NORMAL_BUDGET);
        assert_eq!(same.left(NORMAL_BUDGET).bytes, 0);
        // The page taken past the budget is debt the next window starts with.
        let next = window.at(start + RECORD_WINDOW, RECORD_WINDOW, NORMAL_BUDGET);
        assert_eq!(
            next.left(NORMAL_BUDGET),
            RecordBudget {
                bytes: NORMAL_BUDGET.bytes - 90,
                envelopes: NORMAL_BUDGET.envelopes - 5,
            }
        );
        let later = window.at(start + RECORD_WINDOW * 2, RECORD_WINDOW, NORMAL_BUDGET);
        assert_eq!(later.left(NORMAL_BUDGET), NORMAL_BUDGET);
    }

    #[test]
    fn the_backstop_keeps_part_of_its_reserve_for_kit_records() {
        let limits = RecordLimits::default();
        let shared = BACKSTOP_RESERVE - BACKSTOP_KIT_RESERVE;
        let mut backstop = Backstop::default();
        assert!(backstop.charge(&limits, RecordClass::Contest, shared - MIB, true));
        // Contest and wanted records cannot reach into the kit part, and their miss leaves it
        // open.
        assert!(!backstop.charge(&limits, RecordClass::Wanted, 2 * MIB, true));
        assert!(!backstop.stopped);
        assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_KIT_RESERVE + MIB, true));
        // A kit-charged envelope the binding does not fix, such as an ancestor, misses without
        // stopping.
        assert!(!backstop.charge(&limits, RecordClass::Kit, 1, false));
        assert!(!backstop.stopped);
        assert!(!backstop.charge(&limits, RecordClass::Kit, 1, true));
        assert!(backstop.stopped);

        // A full shared part still admits kit records up to the whole reserve.
        let mut backstop = Backstop::default();
        assert!(backstop.charge(&limits, RecordClass::Contest, shared, true));
        assert!(!backstop.charge(&limits, RecordClass::Contest, 1, true));
        assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_KIT_RESERVE, true));
        assert!(!backstop.stopped);

        // Kit records may spend the whole reserve.
        let mut backstop = Backstop::default();
        assert!(backstop.charge(&limits, RecordClass::Kit, BACKSTOP_RESERVE, true));
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
        assert!(
            growth_alerts(
                &storage(MIB, vec![("phone", MIB)], vec![]),
                &RecordLimits::default()
            )
            .is_empty()
        );
        let writer = WriterId::from_bytes([7; 16]);
        let alerts = growth_alerts(
            &storage(
                ALERT_TOTAL_BYTES + 1,
                vec![
                    ("flooder", ALERT_DEVICE_BYTES + 1),
                    ("phone", ALERT_DEVICE_BYTES),
                ],
                vec![ForkedWriter {
                    writer,
                    device_id: Some("forker".to_string()),
                }],
            ),
            &RecordLimits::default(),
        );
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
    fn a_session_with_a_new_peer_lets_every_wanted_entry_be_asked_of_it_again() {
        let state = RecordExchange::default();
        let waiting = Backoff {
            next: 100,
            wait: 64,
        };
        for peer in ["hub", "laptop"] {
            state
                .lock()
                .backoff
                .insert(peer.to_string(), BTreeMap::from([(vec![1], waiting)]));
        }
        state
            .lock()
            .pulled
            .insert("hub".to_string(), RecordCursor::default());
        state.start_session("hub");
        assert_eq!(state.backed_off("hub"), BTreeSet::from([vec![1]]));
        state.start_session("laptop");
        assert!(state.backed_off("laptop").is_empty());
        assert_eq!(state.backed_off("hub"), BTreeSet::from([vec![1]]));
        state.disconnected("hub");
        state.start_session("hub");
        assert!(state.backed_off("hub").is_empty());
    }
}

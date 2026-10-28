//! The authorization log: the space charter, kind 4 entries in the anchor's writer chain, chain
//! selection, and the device and writer state derived from the selected chain.
//!
//! Every kind 4 operation an anchor writer seals is kept in `sync_auth_entries`. Selection walks
//! the entries from the charter through the ones whose rules hold at their parent, and keeps the
//! chain whose head has the highest epoch; between two handovers into the same epoch the lower
//! entry hash wins, then the longer chain, then the lower head hash. `sync_devices` and the
//! writer states of removed devices are recomputed from the selected chain whenever an entry
//! arrives, so a reselection reverts what a superseded chain set.
//!
//! A fork of a writer of an epoch's anchor freezes the epoch at the authorization revision its
//! forked operations were sealed against, the last one both copies of the anchor share: entries
//! of the epoch past it are rejected, so the chain through them is superseded, and only a
//! handover continues the log.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ganbaru_sync_contracts::bounds::DISCOVERY_KEY_BYTES;
use ganbaru_sync_contracts::{
    AdminPublicKey, AuthorizationRecord, Content, DeviceKind, DevicePublicKey, DeviceRole,
    Digest32, Entry, Envelope, ProposalRejection, ResolveLate, Seq, Sequencing, SetRole,
    SignedCharter, SignedProposal, SpaceId, VersionVector, WriterId,
};
use sqlx::{Connection, Row, SqliteConnection};

use crate::apply::{self, Constrained};
use crate::error::{SyncError, SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::Batch;
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, exclusion, fork, sql};

/// An enrolled device as the selected chain leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolledDevice {
    /// Handoff device id.
    pub device_id: String,
    /// Key that certifies the device's writers.
    pub device_key: DevicePublicKey,
    /// SHA-256 fingerprint of its TLS certificate.
    pub tls_fingerprint: [u8; 32],
    /// Label the enrollment recorded, if any.
    pub label: Option<String>,
    /// Form factor.
    pub kind: DeviceKind,
    /// Role, with the admin key of an admin.
    pub role: DeviceRole,
    /// Revision that enrolled it; 0 for the charter's anchor.
    pub enrolled_revision: u64,
    /// Revision that removed it.
    pub removed_revision: Option<u64>,
}

/// The selected authorization state of a space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationView {
    /// Epoch of the selected head.
    pub epoch: u32,
    /// Revision of the selected head; 0 when only the charter is known.
    pub revision: u64,
    /// Entry hash of the selected head; the charter hash when only the charter is known.
    pub entry_hash: Digest32,
    /// Device id of the current anchor.
    pub anchor_device_id: String,
    /// Revision past which the epoch is frozen because a writer of its anchor forked; only a
    /// handover continues the log then.
    pub frozen_at: Option<u64>,
    /// The old anchor's stored vector the handover that opened this epoch carried. The new
    /// anchor must know every writer it names before it sequences a removal.
    pub catch_up: Option<VersionVector>,
    /// Hash of the charter the selected chain starts from, which every proposal names.
    pub charter: Digest32,
    /// The recovery admin key the selected chain names, which the current recovery kit derives.
    pub recovery_admin_key: AdminPublicKey,
    /// Every device the selected chain enrolled, removed ones included, by device id.
    pub devices: Vec<EnrolledDevice>,
    /// The discovery key devices advertise under now: the charter's, replaced by each removal.
    pub discovery_key: [u8; DISCOVERY_KEY_BYTES],
    /// The key the latest removal replaced, which devices that have not applied it yet still
    /// advertise under.
    pub previous_discovery_key: Option<[u8; DISCOVERY_KEY_BYTES]>,
}

/// A device in derived state.
#[derive(Debug, Clone)]
struct DeviceState {
    device_key: DevicePublicKey,
    tls_fingerprint: [u8; 32],
    label: Option<String>,
    kind: DeviceKind,
    role: DeviceRole,
    enrolled: u64,
    removed: Option<u64>,
    /// Cutoffs of the removal, by writer.
    cutoffs: BTreeMap<WriterId, Seq>,
}

/// Authorization state after some prefix of the log.
#[derive(Debug, Clone)]
pub(crate) struct AuthState {
    space: SpaceId,
    epoch: u32,
    revision: u64,
    entry_hash: Digest32,
    /// Entry hash that opened the current epoch: the charter hash for epoch 0.
    epoch_root: Digest32,
    /// Hash of the charter, which every proposal of the space names.
    charter: Digest32,
    anchor_device_id: String,
    recovery_admin_key: AdminPublicKey,
    devices: BTreeMap<String, DeviceState>,
    /// Revisions at which forks freeze epochs, by anchor device.
    freezes: Arc<BTreeMap<String, Vec<u64>>>,
    /// Revision past which the current epoch is frozen.
    frozen_at: Option<u64>,
    /// The old anchor's stored vector from the handover that opened the current epoch.
    catch_up: Option<Arc<VersionVector>>,
    /// The current discovery key and the one the latest removal replaced.
    discovery_key: [u8; DISCOVERY_KEY_BYTES],
    previous_discovery_key: Option<[u8; DISCOVERY_KEY_BYTES]>,
}

/// Revision past which an epoch that `anchor` opened at revision `start` is frozen: the lowest
/// fork revision of the anchor's writers at or past the start. A fork sealed before the epoch
/// opened belongs to an earlier one.
fn frozen_at(freezes: &BTreeMap<String, Vec<u64>>, anchor: &str, start: u64) -> Option<u64> {
    freezes
        .get(anchor)?
        .iter()
        .copied()
        .find(|revision| *revision >= start)
}

impl AuthState {
    /// State at revision 0: the charter enrolls its anchor as an admin desktop. `freezes` lists
    /// the revisions at which forks freeze epochs, by anchor device.
    fn from_charter(charter: &SignedCharter, freezes: Arc<BTreeMap<String, Vec<u64>>>) -> Self {
        let body = &charter.charter;
        let hash = charter.hash();
        let anchor = DeviceState {
            device_key: body.anchor_device_key,
            tls_fingerprint: body.anchor_tls_fingerprint,
            label: None,
            kind: DeviceKind::Desktop,
            role: DeviceRole::Admin(body.anchor_admin_key),
            enrolled: 0,
            removed: None,
            cutoffs: BTreeMap::new(),
        };
        Self {
            space: body.space,
            epoch: 0,
            revision: 0,
            entry_hash: hash,
            epoch_root: hash,
            charter: hash,
            anchor_device_id: body.anchor_device_id.clone(),
            recovery_admin_key: body.recovery_admin_key,
            devices: BTreeMap::from([(body.anchor_device_id.clone(), anchor)]),
            frozen_at: frozen_at(&freezes, &body.anchor_device_id, 0),
            freezes,
            catch_up: None,
            discovery_key: body.discovery_key,
            previous_discovery_key: None,
        }
    }

    /// The recovery key and every enrolled admin key whose device is not removed.
    fn is_admin(&self, key: &AdminPublicKey) -> bool {
        *key == self.recovery_admin_key
            || self
                .devices
                .values()
                .any(|device| device.removed.is_none() && device.role == DeviceRole::Admin(*key))
    }

    fn live_device(&self, device_id: &str) -> Result<&DeviceState, ProposalRejection> {
        let device = self
            .devices
            .get(device_id)
            .ok_or(ProposalRejection::UnknownDevice)?;
        if device.removed.is_some() {
            return Err(ProposalRejection::DeviceRemoved);
        }
        Ok(device)
    }

    /// Whether `record` may follow this state.
    pub(crate) fn check(&self, record: &AuthorizationRecord) -> Result<(), ProposalRejection> {
        let proposal = &record.proposal.proposal;
        if proposal.space != self.space || proposal.charter != self.charter {
            return Err(ProposalRejection::WrongSpace);
        }
        if record.revision != self.revision + 1 || record.previous_entry_hash != self.entry_hash {
            return Err(ProposalRejection::Malformed);
        }
        if !self.is_admin(&proposal.author) {
            return Err(ProposalRejection::NotAdmin);
        }
        if let Entry::Handover(handover) = &proposal.entry {
            if record.epoch != self.epoch.wrapping_add(1) || self.epoch == u32::MAX {
                return Err(ProposalRejection::HandoverConflict);
            }
            let device = self.live_device(&handover.new_anchor_device_id)?;
            // The recovery key forces a handover to any live desktop, because the anchor that
            // would sequence its promotion to admin may be lost. That desktop proposes its own
            // promotion as the first entry of the new epoch.
            let forced_by_recovery = proposal.author == self.recovery_admin_key;
            if !forced_by_recovery && !matches!(device.role, DeviceRole::Admin(_)) {
                return Err(ProposalRejection::NotAdmin);
            }
            if device.kind != DeviceKind::Desktop {
                return Err(ProposalRejection::AdminNotDesktop);
            }
            return Ok(());
        }
        if record.epoch != self.epoch || record.anchor_device_id != self.anchor_device_id {
            return Err(ProposalRejection::NotAnchor);
        }
        if self
            .frozen_at
            .is_some_and(|frozen| record.revision > frozen)
        {
            return Err(ProposalRejection::EpochFrozen);
        }
        match &proposal.entry {
            Entry::Enroll(enroll) => {
                if self.devices.contains_key(&enroll.device_id) {
                    return Err(ProposalRejection::AlreadyEnrolled);
                }
                if matches!(enroll.role, DeviceRole::Admin(_))
                    && enroll.device_kind != DeviceKind::Desktop
                {
                    return Err(ProposalRejection::AdminNotDesktop);
                }
            }
            Entry::Remove(remove) => {
                self.live_device(&remove.device_id)?;
                if remove.device_id == self.anchor_device_id {
                    return Err(ProposalRejection::NamesAnchor);
                }
            }
            Entry::SetRole(SetRole::Device { device_id, role }) => {
                let device = self.live_device(device_id)?;
                if *device_id == self.anchor_device_id && *role == DeviceRole::Member {
                    return Err(ProposalRejection::NamesAnchor);
                }
                if matches!(role, DeviceRole::Admin(_)) && device.kind != DeviceKind::Desktop {
                    return Err(ProposalRejection::AdminNotDesktop);
                }
            }
            Entry::SetRole(SetRole::Recovery { .. }) | Entry::Unknown { .. } => {}
            Entry::ResolveLate(resolve) => {
                let known = self
                    .devices
                    .values()
                    .any(|device| device.removed == Some(resolve.removal_revision));
                if !known {
                    return Err(ProposalRejection::UnknownRemoval);
                }
            }
            Entry::Handover(_) => return Err(ProposalRejection::HandoverConflict),
        }
        Ok(())
    }

    /// Applies a record that passed [`Self::check`].
    fn advance(&mut self, record: &AuthorizationRecord, hash: Digest32) {
        self.revision = record.revision;
        self.entry_hash = hash;
        match &record.proposal.proposal.entry {
            Entry::Handover(handover) => {
                self.epoch = record.epoch;
                self.epoch_root = hash;
                self.anchor_device_id = handover.new_anchor_device_id.clone();
                self.frozen_at = frozen_at(&self.freezes, &self.anchor_device_id, record.revision);
                self.catch_up = match &record.sequencing {
                    Sequencing::Handover {
                        old_anchor_vector: Some(vector),
                    } => Some(Arc::new(vector.clone())),
                    _ => None,
                };
            }
            Entry::Enroll(enroll) => {
                self.devices.insert(
                    enroll.device_id.clone(),
                    DeviceState {
                        device_key: enroll.device_key,
                        tls_fingerprint: enroll.tls_fingerprint,
                        label: Some(enroll.label.clone()),
                        kind: enroll.device_kind,
                        role: enroll.role,
                        enrolled: record.revision,
                        removed: None,
                        cutoffs: BTreeMap::new(),
                    },
                );
            }
            Entry::Remove(remove) => {
                if let Some(device) = self.devices.get_mut(&remove.device_id) {
                    device.removed = Some(record.revision);
                    if let Sequencing::Removal { cutoffs, .. } = &record.sequencing {
                        device.cutoffs = cutoffs.iter().copied().collect();
                    }
                }
                if let Sequencing::Removal { discovery_key, .. } = &record.sequencing
                    && *discovery_key != self.discovery_key
                {
                    self.previous_discovery_key = Some(self.discovery_key);
                    self.discovery_key = *discovery_key;
                }
            }
            Entry::SetRole(SetRole::Device { device_id, role }) => {
                if let Some(device) = self.devices.get_mut(device_id) {
                    device.role = *role;
                }
            }
            Entry::SetRole(SetRole::Recovery { recovery_admin_key }) => {
                self.recovery_admin_key = *recovery_admin_key;
            }
            Entry::ResolveLate(_) | Entry::Unknown { .. } => {}
        }
    }

    /// Selection order of a chain head: higher epoch, then the lower hash of the handover that
    /// opened it, then the higher revision, then the lower head hash.
    fn rank(
        &self,
    ) -> (
        u32,
        std::cmp::Reverse<Digest32>,
        u64,
        std::cmp::Reverse<Digest32>,
    ) {
        (
            self.epoch,
            std::cmp::Reverse(self.epoch_root),
            self.revision,
            std::cmp::Reverse(self.entry_hash),
        )
    }

    /// Device id of the anchor of the current epoch.
    pub(crate) fn anchor_device_id(&self) -> &str {
        &self.anchor_device_id
    }

    pub(crate) fn view(&self) -> AuthorizationView {
        AuthorizationView {
            epoch: self.epoch,
            revision: self.revision,
            entry_hash: self.entry_hash,
            anchor_device_id: self.anchor_device_id.clone(),
            frozen_at: self.frozen_at,
            catch_up: self.catch_up.as_deref().cloned(),
            charter: self.charter,
            recovery_admin_key: self.recovery_admin_key,
            devices: self
                .devices
                .iter()
                .map(|(device_id, device)| EnrolledDevice {
                    device_id: device_id.clone(),
                    device_key: device.device_key,
                    tls_fingerprint: device.tls_fingerprint,
                    label: device.label.clone(),
                    kind: device.kind,
                    role: device.role,
                    enrolled_revision: device.enrolled,
                    removed_revision: device.removed,
                })
                .collect(),
            discovery_key: self.discovery_key,
            previous_discovery_key: self.previous_discovery_key,
        }
    }
}

/// A stored entry: the kind 4 operation that carries it and its record.
struct Node {
    writer: WriterId,
    seq: Seq,
    hash: Digest32,
    record: AuthorizationRecord,
}

/// The selected chain: its head state and the operations of its entries in revision order.
#[derive(Clone)]
pub(crate) struct Selection {
    pub(crate) state: AuthState,
    path: Vec<(WriterId, Seq)>,
    /// Late change decisions of the chain with their revisions, in revision order.
    pub(crate) resolutions: Vec<(u64, ResolveLate)>,
    /// Highest revision any stored chain reaches from the charter, selected or not.
    pub(crate) reachable: u64,
}

impl Selection {
    /// This chain extended by `node`, which its head state admits.
    fn extended(mut self, node: &Node) -> Self {
        self.state.advance(&node.record, node.hash);
        self.path.push((node.writer, node.seq));
        if let Entry::ResolveLate(resolve) = &node.record.proposal.proposal.entry {
            self.resolutions
                .push((node.record.revision, resolve.clone()));
        }
        self.reachable = 0;
        self
    }

    /// Device id of the selected anchor.
    pub(crate) fn anchor(&self) -> &str {
        &self.state.anchor_device_id
    }
}

fn kind_text(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Desktop => "desktop",
        DeviceKind::Mobile => "mobile",
    }
}

/// The record of a kind 4 envelope.
pub(crate) fn record_of(bytes: &[u8]) -> Option<AuthorizationRecord> {
    let operation = Envelope::decode(bytes).ok()?.operation().ok()?;
    match operation.content {
        Content::Authorization(record) => Some(*record),
        _ => None,
    }
}

/// The stored charter of a space.
pub(crate) async fn load_charter(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<SignedCharter>> {
    let bytes: Option<Option<Vec<u8>>> =
        sqlx::query_scalar("SELECT charter FROM sync_spaces WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_optional(&mut *conn)
            .await?;
    bytes
        .flatten()
        .map(|bytes| {
            SignedCharter::decode(&bytes).map_err(|_| corrupt("stored charter does not decode"))
        })
        .transpose()
}

/// The revision at which the selected chain holds a proposal.
async fn selected_proposal_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
    proposal_hash: &Digest32,
) -> SyncResult<Option<u64>> {
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM sync_auth_entries
         WHERE space_id = ? AND proposal_hash = ? AND selected = 1",
    )
    .bind(space.as_bytes().as_slice())
    .bind(proposal_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    revision
        .map(|revision| stored_u64(revision, "revision"))
        .transpose()
}

async fn selected_proposal_position(
    conn: &mut SqliteConnection,
    space: SpaceId,
    proposal_hash: &Digest32,
) -> SyncResult<Option<(u32, u64)>> {
    let row: Option<(i64, i64)> = sqlx::query_as(
        "SELECT epoch, revision FROM sync_auth_entries
         WHERE space_id = ? AND proposal_hash = ? AND selected = 1",
    )
    .bind(space.as_bytes().as_slice())
    .bind(proposal_hash.as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    row.map(|(epoch, revision)| {
        let epoch =
            u32::try_from(epoch).map_err(|_| corrupt("authorization epoch is out of range"))?;
        Ok((epoch, stored_u64(revision, "revision")?))
    })
    .transpose()
}

/// Every stored entry, deduplicated by entry hash.
async fn load_nodes(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Vec<Node>> {
    let rows = sqlx::query(
        "SELECT e.writer_id, e.seq, e.entry_hash, o.envelope
         FROM sync_auth_entries e JOIN sync_ops o
            ON o.space_id = e.space_id AND o.writer_id = e.writer_id AND o.seq = e.seq
         WHERE e.space_id = ? ORDER BY e.writer_id, e.seq",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    let mut seen = BTreeSet::new();
    let mut nodes = Vec::with_capacity(rows.len());
    for row in &rows {
        let hash = log::digest(&row.try_get::<Vec<u8>, _>(2)?)?;
        if !seen.insert(hash) {
            continue;
        }
        let envelope: Vec<u8> = row.try_get(3)?;
        let record = record_of(&envelope)
            .ok_or_else(|| corrupt("a stored authorization entry does not decode"))?;
        nodes.push(Node {
            writer: log::writer_id(&row.try_get::<Vec<u8>, _>(0)?)?,
            seq: stored_u64(row.try_get(1)?, "sequence")?,
            hash,
            record,
        });
    }
    Ok(nodes)
}

/// Readiness revision of a space whose stored chains reach a freeze: every revision, kept within
/// the stored integer range.
const UNBOUNDED_REVISION: u64 = i64::MAX.unsigned_abs();

/// Selects the chain from the charter through the stored entries, or `None` without a charter.
pub(crate) async fn select(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<Selection>> {
    let Some(charter) = load_charter(conn, space).await? else {
        return Ok(None);
    };
    let nodes = load_nodes(conn, space).await?;
    let freezes = Arc::new(fork::freezes(conn, space).await?);
    let mut children: BTreeMap<Digest32, Vec<usize>> = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        children
            .entry(node.record.previous_entry_hash)
            .or_default()
            .push(index);
    }
    // Entries name their parent by hash and duplicates are dropped, so the walk visits each
    // entry at most once.
    let mut best: Option<Selection> = None;
    let mut reachable = 0;
    let mut stack = vec![Selection {
        state: AuthState::from_charter(&charter, freezes),
        path: Vec::new(),
        resolutions: Vec::new(),
        reachable: 0,
    }];
    while let Some(selection) = stack.pop() {
        reachable = reachable.max(selection.state.revision);
        // Copies of a forked anchor writer may carry entries past the revision its epoch froze
        // at, which no replica keeps once the fork is excluded. An author that applied one
        // before the fork was found must not wait for it, so a chain that reaches a freeze
        // reaches every revision. Each operation's device is still checked against the selected
        // chain when it applies.
        if selection
            .state
            .frozen_at
            .is_some_and(|frozen| selection.state.revision >= frozen)
        {
            reachable = UNBOUNDED_REVISION;
        }
        let valid: Vec<&Node> = children
            .get(&selection.state.entry_hash)
            .into_iter()
            .flatten()
            .map(|index| &nodes[*index])
            .filter(|node| selection.state.check(&node.record).is_ok())
            .collect();
        let Some((last, forks)) = valid.split_last() else {
            if best
                .as_ref()
                .is_none_or(|best| selection.state.rank() > best.state.rank())
            {
                best = Some(selection);
            }
            continue;
        };
        // Only forks copy the state, so a chain without forks is walked in linear time.
        for node in forks {
            stack.push(selection.clone().extended(node));
        }
        stack.push(selection.extended(last));
    }
    Ok(best.map(|best| Selection { reachable, ..best }))
}

/// Revision an operation's authorization revision must not exceed to be applied: the highest
/// revision any stored chain reaches, selected or not.
///
/// This guarantees that the receiver stored every entry the author had applied when it sealed
/// the operation, not that the selected chain covers them: a superseded chain can satisfy it.
/// Requiring the selected chain instead would leave an operation sealed against a superseded
/// entry waiting forever. Once a stored chain reaches the revision a fork froze its epoch at,
/// every revision is reachable, since the forked copies' later entries are kept nowhere.
pub(crate) async fn readiness_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<u64> {
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT reachable_revision FROM sync_spaces WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_optional(&mut *conn)
            .await?;
    revision.map_or(Ok(0), |revision| stored_u64(revision, "revision"))
}

/// Highest revision of the selected chain, 0 with only the charter or none.
pub(crate) async fn selected_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<u64> {
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT max(revision) FROM sync_auth_entries WHERE space_id = ? AND selected = 1",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_one(&mut *conn)
    .await?;
    revision.map_or(Ok(0), |revision| stored_u64(revision, "revision"))
}

/// Records a kind 4 operation of `writer` at `seq` as a log entry.
/// Whether a writer of `carrier_device` may carry `record`: only writers of the device the record
/// names as anchor carry entries, and only well-formed ones.
pub(crate) fn carries(carrier_device: &str, record: &AuthorizationRecord) -> bool {
    carrier_device == record.anchor_device_id && record.validate().is_ok()
}

pub(crate) async fn insert_entry(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
    seq: Seq,
    record: &AuthorizationRecord,
) -> SyncResult<()> {
    sqlx::query(
        "INSERT INTO sync_auth_entries
            (space_id, writer_id, seq, epoch, revision, previous_hash, entry_hash, entry_kind,
             proposal_hash, selected)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
         ON CONFLICT DO NOTHING",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .bind(seq_i64(seq)?)
    .bind(i64::from(record.epoch))
    .bind(seq_i64(record.revision)?)
    .bind(record.previous_entry_hash.as_slice())
    .bind(record.hash().as_slice())
    .bind(i64::from(record.proposal.proposal.entry.kind()))
    .bind(record.proposal.hash().as_slice())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Selects the chain and writes what it derives: the selected flags, `sync_devices`, the
/// reachable revision, the anchor, and the writer state of removed devices. Writers of a removed
/// device become revoked at the cutoff the removal lists, or 0 when it lists none, and every
/// writer of a removed device, retired and forked ones included, gets that cutoff as its removal
/// cutoff; revoked writers of a device that is no longer removed become active again. Retired
/// and forked writers keep their state, and writers of devices the chain does not enroll are left
/// as they are. Anchor marks are cleared when the anchor changes, since they confirm what the
/// previous anchor stored.
pub(crate) async fn rederive(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<Selection>> {
    let Some(selection) = select(conn, space).await? else {
        return Ok(None);
    };
    let previous_anchor: Option<String> =
        sqlx::query_scalar("SELECT anchor_device_id FROM sync_spaces WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_one(&mut *conn)
            .await?;
    if previous_anchor.as_deref() != Some(selection.anchor()) {
        sqlx::query("DELETE FROM sync_anchor_marks WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .execute(&mut *conn)
            .await?;
    }
    sqlx::query(
        "UPDATE sync_spaces SET reachable_revision = ?, anchor_device_id = ? WHERE space_id = ?",
    )
    .bind(seq_i64(selection.reachable)?)
    .bind(selection.anchor())
    .bind(space.as_bytes().as_slice())
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE sync_auth_entries SET selected = 0 WHERE space_id = ? AND selected = 1")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    for (writer, seq) in &selection.path {
        sqlx::query(
            "UPDATE sync_auth_entries SET selected = 1
             WHERE space_id = ? AND writer_id = ? AND seq = ?",
        )
        .bind(space.as_bytes().as_slice())
        .bind(writer.as_bytes().as_slice())
        .bind(seq_i64(*seq)?)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("DELETE FROM sync_devices WHERE space_id = ?")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    for (device_id, device) in &selection.state.devices {
        let (role, admin_key) = match device.role {
            DeviceRole::Member => ("member", None),
            DeviceRole::Admin(key) => ("admin", Some(key.as_bytes().to_vec())),
        };
        sqlx::query(
            "INSERT INTO sync_devices
                (space_id, device_id, device_key, tls_fingerprint, label, kind, role, admin_key,
                 enrolled_revision, removed_revision)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(space.as_bytes().as_slice())
        .bind(device_id.as_str())
        .bind(device.device_key.as_bytes().as_slice())
        .bind(device.tls_fingerprint.as_slice())
        .bind(device.label.as_deref())
        .bind(kind_text(device.kind))
        .bind(role)
        .bind(admin_key)
        .bind(seq_i64(device.enrolled)?)
        .bind(device.removed.map(seq_i64).transpose()?)
        .execute(&mut *conn)
        .await?;
    }
    for writer in log::writers(conn, space).await? {
        let removal_cutoff = selection
            .state
            .devices
            .get(&writer.device_id)
            .filter(|device| device.removed.is_some())
            .map(|device| device.cutoffs.get(&writer.id).copied().unwrap_or(0));
        if removal_cutoff != writer.removal_cutoff {
            sqlx::query(
                "UPDATE sync_writers SET removal_cutoff_seq = ?
                 WHERE space_id = ? AND writer_id = ?",
            )
            .bind(removal_cutoff.map(seq_i64).transpose()?)
            .bind(space.as_bytes().as_slice())
            .bind(writer.id.as_bytes().as_slice())
            .execute(&mut *conn)
            .await?;
        }
        let Some(device) = selection.state.devices.get(&writer.device_id) else {
            continue;
        };
        let (state, cutoff) = match (device.removed, writer.state) {
            (Some(_), log::WriterState::Active | log::WriterState::Revoked) => (
                "revoked",
                Some(seq_i64(
                    device.cutoffs.get(&writer.id).copied().unwrap_or(0),
                )?),
            ),
            (None, log::WriterState::Revoked) => ("active", None),
            _ => continue,
        };
        sqlx::query(
            "UPDATE sync_writers SET state = ?, cutoff_seq = ? WHERE space_id = ? AND writer_id = ?",
        )
        .bind(state)
        .bind(cutoff)
        .bind(space.as_bytes().as_slice())
        .bind(writer.id.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    }
    Ok(Some(selection))
}

/// How the selected chain knows the device a genesis names.
pub(crate) enum Enrollment {
    /// The device is enrolled with this key and not removed.
    Enrolled,
    /// The device is enrolled with another key.
    WrongKey,
    /// The device was removed; the key is the one it was enrolled with.
    Removed,
    /// The space has no charter, or the device is not enrolled.
    Absent,
}

/// Whether `device_id` is enrolled with `device_key` and not removed.
pub(crate) async fn enrollment(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
    device_key: &DevicePublicKey,
) -> SyncResult<Enrollment> {
    let row: Option<(Vec<u8>, Option<i64>)> = sqlx::query_as(
        "SELECT device_key, removed_revision FROM sync_devices
         WHERE space_id = ? AND device_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(match row {
        None => Enrollment::Absent,
        Some((key, Some(_))) if key == device_key.as_bytes() => Enrollment::Removed,
        Some((key, None)) if key == device_key.as_bytes() => Enrollment::Enrolled,
        Some(_) => Enrollment::WrongKey,
    })
}

/// Fails with [`SyncError::ResetRequired`] when the space holds writers but no charter, or a
/// writer certificate this build cannot read: state from before the authorization log.
pub(crate) async fn refuse_legacy(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<()> {
    let (charter, writers): (Option<i64>, i64) = sqlx::query_as(
        "SELECT (SELECT 1 FROM sync_spaces WHERE space_id = ?1 AND charter IS NOT NULL),
            (SELECT COUNT(*) FROM sync_writers WHERE space_id = ?1)",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_one(&mut *conn)
    .await?;
    if charter.is_none() && writers > 0 {
        return Err(SyncError::ResetRequired(
            "the space has writers but no charter",
        ));
    }
    let certificates: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT certificate FROM sync_writers WHERE space_id = ?")
            .bind(space.as_bytes().as_slice())
            .fetch_all(&mut *conn)
            .await?;
    if certificates
        .iter()
        .any(|bytes| ganbaru_sync_contracts::SignedCertificate::decode(bytes).is_err())
    {
        return Err(SyncError::ResetRequired(
            "a writer certificate uses a retired format",
        ));
    }
    Ok(())
}

impl Engine {
    /// Installs the space charter, revision 0 of the authorization log, after verifying it with
    /// the context's person key. Installing the same charter again does nothing; a different one
    /// is refused.
    pub async fn install_charter(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        charter: &SignedCharter,
    ) -> SyncResult<()> {
        if charter.charter.space != ctx.space || charter.verify(&ctx.person_key).is_err() {
            return Err(SyncError::InvalidCharter);
        }
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        match load_charter(&mut tx, ctx.space).await? {
            Some(existing) if existing == *charter => return Ok(()),
            Some(_) => return Err(SyncError::InvalidCharter),
            None => {}
        }
        sqlx::query("UPDATE sync_spaces SET charter = ? WHERE space_id = ?")
            .bind(charter.encode())
            .bind(ctx.space.as_bytes().as_slice())
            .execute(&mut *tx)
            .await?;
        refuse_legacy(&mut tx, ctx.space).await?;
        rederive(&mut tx, ctx.space).await?;
        tx.commit().await?;
        Ok(())
    }

    /// The installed charter of the space.
    pub async fn charter(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Option<SignedCharter>> {
        log::ensure_space(conn, ctx.space).await?;
        load_charter(conn, ctx.space).await
    }

    /// The selected authorization state, or `None` without a charter.
    pub async fn authorization(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Option<AuthorizationView>> {
        log::ensure_space(conn, ctx.space).await?;
        Ok(select(conn, ctx.space)
            .await?
            .map(|selection| selection.state.view()))
    }

    /// The revision at which the selected chain holds the proposal with `proposal_hash`, if it
    /// does. An outbox drops a proposal once its own replica holds it.
    pub async fn proposal_revision(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal_hash: &Digest32,
    ) -> SyncResult<Option<u64>> {
        log::ensure_space(conn, ctx.space).await?;
        selected_proposal_revision(conn, ctx.space, proposal_hash).await
    }

    /// The epoch and revision at which the selected chain holds the proposal with
    /// `proposal_hash`, if it does. An outbox keeps a held proposal until a later epoch kept it,
    /// because a handover from an earlier revision would supersede it.
    pub async fn proposal_position(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal_hash: &Digest32,
    ) -> SyncResult<Option<(u32, u64)>> {
        log::ensure_space(conn, ctx.space).await?;
        selected_proposal_position(conn, ctx.space, proposal_hash).await
    }

    /// Fails with [`SyncError::ResetRequired`] when the space holds sync state from before the
    /// authorization log, which this build does not read.
    pub async fn refuse_legacy_state(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<()> {
        log::ensure_space(conn, ctx.space).await?;
        refuse_legacy(conn, ctx.space).await
    }

    /// Orders an admin-signed proposal as the next revision and seals it in the local writer's
    /// chain. The local device must be the anchor of the selected chain. Returns the revision, or
    /// why the proposal was rejected; a proposal already in the selected chain returns its
    /// revision without sealing again. A removal takes `discovery_key` as the rotated key.
    ///
    /// This sequences `enroll`, `remove`, `set_role`, and `resolve_late`; handovers go through
    /// [`Self::seal_handover`] and unknown kinds are rejected as unsupported.
    pub async fn sequence_proposal(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence_proposal_covering(
            conn,
            ctx,
            proposal,
            discovery_key,
            &VersionVector::new(),
            writer,
            now_ms,
        )
        .await
    }

    /// [`Self::sequence_proposal`] on an anchor that may store less than other replicas, such
    /// as a linked anchor that quarantined a device: a removal's cutoff of each writer is at
    /// least what `covered` holds, the stored vector the coordinator last reported, so
    /// operations the coordinator and members already applied do not become late changes.
    #[allow(clippy::too_many_arguments)]
    pub async fn sequence_proposal_covering(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        covered: &VersionVector,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence(
            conn,
            ctx,
            proposal,
            Ordering::Anchor {
                discovery_key,
                covered,
            },
            writer,
            now_ms,
        )
        .await
    }

    /// Seals an admin-signed handover as the first entry of the epoch it opens, in the local
    /// writer's chain. The local device must be the handover's new anchor and the selected head
    /// the revision the handover continues from; `old_anchor_vector` is the old anchor's stored
    /// vector when known. A frozen epoch does not stop a handover. Returns the revision, or why
    /// the handover was rejected; one already in the selected chain returns its revision without
    /// sealing again.
    pub async fn seal_handover(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        old_anchor_vector: Option<VersionVector>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        self.sequence(
            conn,
            ctx,
            proposal,
            Ordering::Handover { old_anchor_vector },
            writer,
            now_ms,
        )
        .await
    }

    async fn sequence(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        proposal: &SignedProposal,
        ordering: Ordering<'_>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<u64, ProposalRejection>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        if let Some(revision) =
            selected_proposal_revision(&mut tx, ctx.space, &proposal.hash()).await?
        {
            return Ok(Ok(revision));
        }
        let Some(selection) = select(&mut tx, ctx.space).await? else {
            return Err(SyncError::ResetRequired("the space has no charter"));
        };
        let state = selection.state;
        let local_device = &writer.certificate.certificate.device_id;
        let record = match (ordering, &proposal.proposal.entry) {
            (Ordering::Anchor { .. }, _) if *local_device != state.anchor_device_id => {
                return Ok(Err(ProposalRejection::NotAnchor));
            }
            (
                Ordering::Anchor {
                    discovery_key,
                    covered,
                },
                entry,
            ) => {
                let sequencing = match entry {
                    Entry::Enroll(_) | Entry::SetRole(_) | Entry::ResolveLate(_) => {
                        Sequencing::Empty
                    }
                    Entry::Remove(remove) => {
                        let Some(cutoffs) = removal_cutoffs(
                            &mut tx,
                            ctx.space,
                            &remove.device_id,
                            state.catch_up.as_deref(),
                            covered,
                        )
                        .await?
                        else {
                            return Ok(Err(ProposalRejection::CatchingUp));
                        };
                        Sequencing::Removal {
                            cutoffs,
                            discovery_key,
                        }
                    }
                    Entry::Handover(_) | Entry::Unknown { .. } => {
                        return Ok(Err(ProposalRejection::UnsupportedEntry));
                    }
                };
                AuthorizationRecord {
                    epoch: state.epoch,
                    revision: state.revision + 1,
                    previous_entry_hash: state.entry_hash,
                    anchor_device_id: state.anchor_device_id.clone(),
                    proposal: proposal.clone(),
                    sequencing,
                }
            }
            (Ordering::Handover { old_anchor_vector }, Entry::Handover(handover)) => {
                if handover.new_anchor_device_id != *local_device {
                    return Ok(Err(ProposalRejection::NotAnchor));
                }
                if handover.continues_from_revision != state.revision
                    || handover.continues_from_hash != state.entry_hash
                {
                    return Ok(Err(ProposalRejection::HandoverConflict));
                }
                AuthorizationRecord {
                    epoch: handover.epoch,
                    revision: state.revision + 1,
                    previous_entry_hash: state.entry_hash,
                    anchor_device_id: handover.new_anchor_device_id.clone(),
                    proposal: proposal.clone(),
                    sequencing: Sequencing::Handover { old_anchor_vector },
                }
            }
            (Ordering::Handover { .. }, _) => {
                return Ok(Err(ProposalRejection::UnsupportedEntry));
            }
        };
        if record.validate().is_err() {
            return Ok(Err(ProposalRejection::Malformed));
        }
        if let Err(rejection) = state.check(&record) {
            return Ok(Err(rejection));
        }
        sql::begin_engine_write(&mut tx).await?;
        let mut batch = Batch::load(&mut tx, ctx.space).await?;
        exclusion::note_local_device(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &writer.certificate.certificate.device_id,
        )
        .await?;
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        let (sealer, writer) = slot
            .open(self, &mut tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        let revision = record.revision;
        writer
            .reservation
            .reserve_entry(state.charter, record.epoch, revision, record.hash())
            .map_err(SyncError::Reservation)?;
        let seq = sealer
            .seal_authorization(
                self,
                &mut tx,
                ctx,
                &mut batch,
                writer,
                record.clone(),
                now_ms,
                &mut report,
            )
            .await?;
        insert_entry(&mut tx, ctx.space, writer.id(), seq, &record).await?;
        if let Some(selection) = rederive(&mut tx, ctx.space).await? {
            exclusion::settle(self, &mut tx, ctx, &mut batch, &selection).await?;
        }
        apply::finish_sealed(
            self,
            &mut tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut report,
        )
        .await?;
        failpoint!("sequence.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(revision))
    }
}

/// How a proposal is ordered into the log.
enum Ordering<'a> {
    /// By the selected anchor, in its epoch; a removal takes the rotated discovery key, and its
    /// cutoffs cover at least `covered`.
    Anchor {
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
        covered: &'a VersionVector,
    },
    /// By the new anchor a handover names, as the first entry of the next epoch.
    Handover {
        old_anchor_vector: Option<VersionVector>,
    },
}

/// Cutoffs of a removal of `device_id`: the stored sequence of each of its writers. In an epoch
/// a handover opened with the old anchor's stored vector, a writer's cutoff is at least what the
/// old anchor stored, because those operations are final, and it is at least what `covered`
/// holds, which other replicas already store. `None` while a writer of the old anchor vector is
/// unknown here: it may belong to the removed device, whose cutoff could not be set. A writer
/// `covered` names but this replica never stored has no row and gets no cutoff.
async fn removal_cutoffs(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
    catch_up: Option<&VersionVector>,
    covered: &VersionVector,
) -> SyncResult<Option<Vec<(WriterId, Seq)>>> {
    if let Some(old_anchor) = catch_up {
        let rows: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT writer_id FROM sync_writers WHERE space_id = ?")
                .bind(space.as_bytes().as_slice())
                .fetch_all(&mut *conn)
                .await?;
        let mut writers = std::collections::BTreeSet::new();
        for writer in &rows {
            writers.insert(log::writer_id(writer)?);
        }
        if old_anchor
            .iter()
            .any(|(writer, seq)| seq > 0 && !writers.contains(&writer))
        {
            return Ok(None);
        }
    }
    let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT writer_id, stored_seq FROM sync_writers
         WHERE space_id = ? AND device_id = ? ORDER BY writer_id",
    )
    .bind(space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|(writer, seq)| {
            let writer = log::writer_id(writer)?;
            let stored = stored_u64(*seq, "stored sequence")?;
            let finalized = catch_up.map_or(0, |old_anchor| old_anchor.get(&writer));
            Ok((writer, stored.max(finalized).max(covered.get(&writer))))
        })
        .collect::<SyncResult<_>>()
        .map(Some)
}

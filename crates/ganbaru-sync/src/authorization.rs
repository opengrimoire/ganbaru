//! The authorization log: the space charter, kind 4 entries in the anchor's writer chain, chain
//! selection, and the device and writer state derived from the selected chain.
//!
//! Every kind 4 operation an anchor writer seals is kept in `sync_auth_entries`. Selection walks
//! the tree of entries whose rules hold at their parent and keeps the effective entry of highest
//! rank; `sync_devices` and the writer states of removed devices are recomputed from the selected
//! chain whenever an entry arrives, so a reselection reverts what a superseded chain set.
//!
//! A handover is taken: the target device's admin key signs it, or the kit key, or an installed
//! recovery key. Each epoch instance resolves its handovers before the head is chosen. Two
//! handovers out of one instance, or an entry of it that revokes a handover's signer or target,
//! contest it; a kit handover's ancestry pins the winner, a lone candidate wins, a yield that
//! every other contestant signed accepts one, and otherwise the instance freezes. A kit or
//! recovery handover demotes every other admin and resets the recovery key to the kit key. A fork
//! of a writer of an epoch's anchor freezes the instance at the revision its forked operations
//! were sealed against. While the selected instance is frozen, devices that an ineffective entry
//! past the freeze removes are held: their operations are stored but not applied.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

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
    /// Revision past which the epoch is frozen because a writer of its anchor forked or
    /// handovers out of it are contested; only a handover continues the log then.
    pub frozen_at: Option<u64>,
    /// The old anchor's stored vector the handover that opened this epoch carried. The new
    /// anchor must know every writer it names before it sequences a removal.
    pub catch_up: Option<VersionVector>,
    /// Hash of the charter the selected chain starts from, which every proposal names.
    pub charter: Digest32,
    /// The recovery admin key the selected chain names: the kit key, or a recovery key the
    /// anchor installed.
    pub recovery_admin_key: AdminPublicKey,
    /// The admin key the printed recovery kit derives, which only a rotation the kit key signs
    /// replaces.
    pub kit_admin_key: AdminPublicKey,
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

/// Authorization state after some prefix of the log. It depends only on the entries from the
/// charter through that prefix.
#[derive(Debug, Clone)]
pub(crate) struct AuthState {
    space: SpaceId,
    epoch: u32,
    revision: u64,
    entry_hash: Digest32,
    /// Entry hash that opened the current epoch instance: the charter hash for epoch 0.
    epoch_root: Digest32,
    /// Hash of the charter, which every proposal of the space names.
    charter: Digest32,
    anchor_device_id: String,
    /// The kit-lineage recovery key: the charter's, or the one a rotation it signed installed.
    /// Only a `SetRole::Recovery` this key signs replaces it.
    kit_key: AdminPublicKey,
    /// The recovery key the current recovery kit derives: the kit key, or a key the anchor
    /// installed from Settings, see [`Self::recovery_admits`].
    recovery_admin_key: AdminPublicKey,
    /// Device that installed `recovery_admin_key` when it is not the kit key.
    installer: Option<String>,
    /// Root of the epoch instance a handover the installed recovery key signed opened.
    recovery_opened: Option<Digest32>,
    /// Handovers on the chain that the kit key signed and no rotation vetoed.
    kit_handovers: u32,
    devices: BTreeMap<String, DeviceState>,
    /// Revision past which the selected epoch is frozen. Only a selected head carries it, so
    /// the walk never reads it.
    frozen_at: Option<u64>,
    /// Kit rotations and kit handovers the stored tree vetoes, see [`Vetoes`].
    vetoes: Arc<Vetoes>,
    /// The old anchor's stored vector from the handover that opened the current epoch.
    catch_up: Option<Arc<VersionVector>>,
    /// The current discovery key and the one the latest removal replaced.
    discovery_key: [u8; DISCOVERY_KEY_BYTES],
    previous_discovery_key: Option<[u8; DISCOVERY_KEY_BYTES]>,
    /// Hashes of the proposals the chain holds, so a chain never orders one proposal twice.
    proposals: Arc<BTreeSet<Digest32>>,
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

/// Selection order of a chain head, see [`AuthState::rank`].
type Rank = (
    u32,
    u32,
    std::cmp::Reverse<Digest32>,
    u64,
    std::cmp::Reverse<Digest32>,
);

/// A revocation an entry carries. An entry of an epoch instance that revokes the signer key or
/// the target of a handover out of it objects to that handover, and kit rotations veto the kit
/// handovers and rotations of the key they replace, see [`Walk`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Revocation {
    /// A device removed, or demoted from admin to member.
    Device(String),
    /// An admin key revoked or replaced, recovery keys included.
    Key(AdminPublicKey),
    /// A rotation away from a kit key that the key itself signed.
    KitRotation(AdminPublicKey),
}

/// Kit rotations and kit handovers the stored tree vetoes, computed from every stored rotation
/// before the walk that selects, see [`Walk`].
#[derive(Debug, Default, PartialEq, Eq)]
struct Vetoes {
    /// Kit keys with two stored rotations away from them, neither an ancestor of the other. Their
    /// rotations are refused.
    conflicted: BTreeSet<AdminPublicKey>,
    /// Hashes of kit handovers that stay valid but neither pin nor count in rank.
    kit_handovers: BTreeSet<Digest32>,
}

/// Who signed a handover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signer {
    /// The admin key its target holds.
    Take,
    /// The kit key.
    Kit,
    /// A recovery key the anchor installed.
    Recovery,
}

impl AuthState {
    /// State at revision 0: the charter enrolls its anchor as an admin desktop.
    fn from_charter(charter: &SignedCharter, vetoes: Arc<Vetoes>) -> Self {
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
            kit_key: body.recovery_admin_key,
            recovery_admin_key: body.recovery_admin_key,
            installer: None,
            recovery_opened: None,
            kit_handovers: 0,
            devices: BTreeMap::from([(body.anchor_device_id.clone(), anchor)]),
            frozen_at: None,
            vetoes,
            catch_up: None,
            discovery_key: body.discovery_key,
            previous_discovery_key: None,
            proposals: Arc::default(),
        }
    }

    /// The kit key, an installed recovery key where [`Self::recovery_admits`] it, and every
    /// enrolled admin key whose device is not removed.
    fn is_admin(&self, key: &AdminPublicKey) -> bool {
        *key == self.kit_key
            || self.recovery_admits(key)
            || self
                .devices
                .values()
                .any(|device| device.removed.is_none() && device.role == DeviceRole::Admin(*key))
    }

    /// Whether `key` is a recovery key the anchor installed and an admin here: in a state its
    /// installer anchors, or in the epoch instance its own handover opened.
    fn recovery_admits(&self, key: &AdminPublicKey) -> bool {
        *key == self.recovery_admin_key
            && self.installer.as_ref().is_some_and(|installer| {
                *installer == self.anchor_device_id || self.recovery_opened == Some(self.epoch_root)
            })
    }

    /// Admin key of the live anchor device.
    fn anchor_admin_key(&self) -> Option<AdminPublicKey> {
        self.devices
            .get(&self.anchor_device_id)
            .filter(|device| device.removed.is_none())
            .and_then(|device| match device.role {
                DeviceRole::Admin(key) => Some(key),
                DeviceRole::Member => None,
            })
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

    /// Who signed `record` when it is a handover this state admits.
    fn signer(&self, record: &AuthorizationRecord) -> Option<Signer> {
        let proposal = &record.proposal.proposal;
        let Entry::Handover(handover) = &proposal.entry else {
            return None;
        };
        let author = proposal.author;
        if author == self.kit_key {
            return Some(Signer::Kit);
        }
        let target = self.devices.get(&handover.new_anchor_device_id)?;
        if target.role == DeviceRole::Admin(author) {
            return Some(Signer::Take);
        }
        self.recovery_admits(&author).then_some(Signer::Recovery)
    }

    /// Whether `record` may follow this state, whatever other entries the tree stores.
    pub(crate) fn check(&self, record: &AuthorizationRecord) -> Result<(), ProposalRejection> {
        let proposal = &record.proposal.proposal;
        if proposal.space != self.space || proposal.charter != self.charter {
            return Err(ProposalRejection::WrongSpace);
        }
        if record.revision != self.revision + 1 || record.previous_entry_hash != self.entry_hash {
            return Err(ProposalRejection::Malformed);
        }
        // The anchor orders each proposal once, so a replayed one, such as an old recovery key
        // or admin role after a later entry replaced it, is refused.
        if self.proposals.contains(&record.proposal.hash()) {
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
            // A handover is a take, signed by the admin key its target holds, unless the kit key
            // or an admitted installed recovery key signed it: those name any live desktop,
            // because the anchor that would sequence its promotion to admin may be lost.
            if self.signer(record).is_none() {
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
                // A certificate names one device for the life of the log, removed or not, so
                // enrolling and removing a decoy cannot refuse the device that holds it.
                if self
                    .devices
                    .values()
                    .any(|device| device.tls_fingerprint == enroll.tls_fingerprint)
                {
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
            // Only the kit key rotates the kit, and a rotation the stored tree vetoes is
            // refused. Otherwise only the anchor installs a recovery key.
            Entry::SetRole(SetRole::Recovery { recovery_admin_key }) => {
                if proposal.author == self.kit_key {
                    if *recovery_admin_key != self.kit_key
                        && self.vetoes.conflicted.contains(&self.kit_key)
                    {
                        return Err(ProposalRejection::HandoverConflict);
                    }
                } else if self.anchor_admin_key() != Some(proposal.author) {
                    return Err(ProposalRejection::NotAdmin);
                }
            }
            Entry::Yield(_) | Entry::Unknown { .. } => {}
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
        let author = record.proposal.proposal.author;
        let signer = self.signer(record);
        self.revision = record.revision;
        self.entry_hash = hash;
        Arc::make_mut(&mut self.proposals).insert(record.proposal.hash());
        match &record.proposal.proposal.entry {
            Entry::Handover(handover) => {
                if signer == Some(Signer::Kit) && !self.vetoes.kit_handovers.contains(&hash) {
                    self.kit_handovers = self.kit_handovers.saturating_add(1);
                }
                self.epoch = record.epoch;
                self.epoch_root = hash;
                self.anchor_device_id = handover.new_anchor_device_id.clone();
                self.catch_up = match &record.sequencing {
                    Sequencing::Handover {
                        old_anchor_vector: Some(vector),
                    } => Some(Arc::new(vector.clone())),
                    _ => None,
                };
                // A kit or recovery handover leaves no admin but its target, so no other
                // device, planted or former anchor, can take the new instance from it.
                if matches!(signer, Some(Signer::Kit | Signer::Recovery)) {
                    for (device_id, device) in &mut self.devices {
                        if *device_id != handover.new_anchor_device_id && device.removed.is_none() {
                            device.role = DeviceRole::Member;
                        }
                    }
                }
                match signer {
                    Some(Signer::Kit) => self.reset_recovery(),
                    Some(Signer::Recovery) => self.recovery_opened = Some(hash),
                    _ => {}
                }
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
                if author == self.kit_key {
                    self.kit_key = *recovery_admin_key;
                    self.reset_recovery();
                } else {
                    self.recovery_admin_key = *recovery_admin_key;
                    self.installer = (*recovery_admin_key != self.kit_key)
                        .then(|| self.anchor_device_id.clone());
                    self.recovery_opened = None;
                }
            }
            Entry::ResolveLate(_) | Entry::Yield(_) | Entry::Unknown { .. } => {}
        }
    }

    /// Makes the kit key the recovery key again, dropping an installed one.
    fn reset_recovery(&mut self) {
        self.recovery_admin_key = self.kit_key;
        self.installer = None;
        self.recovery_opened = None;
    }

    /// The revocations `record`, which this state admits, carries: removed and demoted devices,
    /// revoked or replaced admin and recovery keys, and rotations away from the kit key that it
    /// signed.
    fn revocations(&self, record: &AuthorizationRecord) -> Vec<Revocation> {
        let proposal = &record.proposal.proposal;
        let role = |device_id: &str| self.devices.get(device_id).map(|device| device.role);
        match &proposal.entry {
            Entry::Remove(remove) => {
                let mut revoked = vec![Revocation::Device(remove.device_id.clone())];
                if let Some(DeviceRole::Admin(key)) = role(&remove.device_id) {
                    revoked.push(Revocation::Key(key));
                }
                revoked
            }
            Entry::SetRole(SetRole::Device {
                device_id,
                role: next,
            }) => match role(device_id) {
                Some(DeviceRole::Admin(key)) if *next == DeviceRole::Member => {
                    vec![Revocation::Device(device_id.clone()), Revocation::Key(key)]
                }
                Some(DeviceRole::Admin(key)) if *next != DeviceRole::Admin(key) => {
                    vec![Revocation::Key(key)]
                }
                _ => Vec::new(),
            },
            Entry::SetRole(SetRole::Recovery { recovery_admin_key }) => {
                let mut revoked = Vec::new();
                if self.recovery_admin_key != self.kit_key
                    && *recovery_admin_key != self.recovery_admin_key
                {
                    revoked.push(Revocation::Key(self.recovery_admin_key));
                }
                if proposal.author == self.kit_key && *recovery_admin_key != self.kit_key {
                    revoked.push(Revocation::Key(self.kit_key));
                    revoked.push(Revocation::KitRotation(self.kit_key));
                }
                revoked
            }
            _ => Vec::new(),
        }
    }

    /// Selection order of a chain head: more unvetoed kit handovers, then the higher epoch, then
    /// the lower hash of the handover that opened it, then the higher revision, then the lower
    /// head hash.
    fn rank(&self) -> Rank {
        (
            self.kit_handovers,
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
            kit_admin_key: self.kit_key,
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
    /// Devices held while the head's epoch instance is frozen: removed on a stored branch of the
    /// contest that is not effective. Their operations wait and they are served authorization
    /// records only.
    pub(crate) held: BTreeSet<String>,
    /// Whether the stored tree holds a handover out of the head's epoch instance, so its anchor
    /// stops sequencing there.
    pub(crate) handed_over: bool,
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

/// The charter, the freezes, and every stored entry of a space.
struct Stored {
    charter: SignedCharter,
    freezes: Arc<BTreeMap<String, Vec<u64>>>,
    nodes: Vec<Node>,
}

impl Stored {
    /// What `space` stores, or `None` without a charter.
    async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Option<Self>> {
        let Some(charter) = load_charter(conn, space).await? else {
            return Ok(None);
        };
        let nodes = load_nodes(conn, space).await?;
        let freezes = Arc::new(fork::freezes(conn, space).await?);
        Ok(Some(Self {
            charter,
            freezes,
            nodes,
        }))
    }
}

/// One check-valid entry of the tree, or the charter at index 0.
struct Visit {
    /// The stored entry, `None` for the charter.
    node: Option<usize>,
    /// Index of the parent visit, `None` for the charter.
    parent: Option<usize>,
    /// Index of the visit that opened the entry's epoch instance: the charter, or the entry
    /// itself when it is a handover.
    instance: usize,
    /// Entry hash, the charter hash for the charter.
    hash: Digest32,
    /// Rank of the state after the entry.
    rank: Rank,
    /// Revision of the entry, 0 for the charter.
    revision: u64,
    /// Anchor of the state after the entry.
    anchor: String,
    /// Key that signed the entry's proposal, `None` for the charter.
    author: Option<AdminPublicKey>,
    /// Signer kind and target device when the entry is a handover.
    handover: Option<(Signer, String)>,
    /// Admin key of the anchor at the parent, if the anchor is a live admin there.
    parent_anchor_key: Option<AdminPublicKey>,
    /// Revocations the entry carries.
    revocations: Vec<Revocation>,
    /// Hash of the handover a yield names.
    yields_to: Option<Digest32>,
    /// Device a removal removes.
    removes: Option<String>,
    /// Kit key of the state after the entry.
    kit_key: AdminPublicKey,
}

/// The tree of check-valid stored entries from the charter, see [`Resolution`].
struct Walk<'a> {
    stored: &'a Stored,
    vetoes: Arc<Vetoes>,
    visits: Vec<Visit>,
    /// Child visits of each visit.
    children: Vec<Vec<usize>>,
    /// Device ids and keys any check-valid chain enrolls.
    enrolled: EnrolledSet,
}

impl<'a> Walk<'a> {
    fn new(stored: &'a Stored, vetoes: Arc<Vetoes>) -> Self {
        let mut by_parent: BTreeMap<Digest32, Vec<usize>> = BTreeMap::new();
        for (index, node) in stored.nodes.iter().enumerate() {
            by_parent
                .entry(node.record.previous_entry_hash)
                .or_default()
                .push(index);
        }
        let root = AuthState::from_charter(&stored.charter, Arc::clone(&vetoes));
        let mut walk = Self {
            stored,
            vetoes,
            visits: vec![Visit {
                node: None,
                parent: None,
                instance: 0,
                hash: root.entry_hash,
                rank: root.rank(),
                revision: 0,
                anchor: root.anchor_device_id.clone(),
                author: None,
                handover: None,
                parent_anchor_key: None,
                revocations: Vec::new(),
                yields_to: None,
                removes: None,
                kit_key: root.kit_key,
            }],
            children: vec![Vec::new()],
            enrolled: BTreeSet::new(),
        };
        // Entries name their parent by hash and duplicates are dropped, so the walk visits each
        // entry at most once, and a visit always comes after its parent. Only forks copy the
        // state, so a chain without forks is walked in linear time.
        let mut stack = vec![(root, 0)];
        while let Some((state, visit)) = stack.pop() {
            let valid: Vec<usize> = by_parent
                .get(&state.entry_hash)
                .into_iter()
                .flatten()
                .copied()
                .filter(|index| state.check(&stored.nodes[*index].record).is_ok())
                .collect();
            let Some((last, forks)) = valid.split_last() else {
                continue;
            };
            for index in forks {
                stack.push(walk.visit(visit, *index, state.clone()));
            }
            stack.push(walk.visit(visit, *last, state));
        }
        walk
    }

    /// Records entry `index`, which `state` admits, as a child of `parent`, and returns the
    /// state after it with its visit.
    fn visit(&mut self, parent: usize, index: usize, mut state: AuthState) -> (AuthState, usize) {
        let stored = self.stored;
        let node = &stored.nodes[index];
        let record = &node.record;
        let entry = &record.proposal.proposal.entry;
        if let Entry::Enroll(enroll) = entry {
            self.enrolled
                .insert((enroll.device_id.clone(), enroll.device_key));
        }
        let handover = match entry {
            Entry::Handover(handover) => state
                .signer(record)
                .map(|signer| (signer, handover.new_anchor_device_id.clone())),
            _ => None,
        };
        let yields_to = match entry {
            Entry::Yield(give_way) => Some(give_way.handover_hash),
            _ => None,
        };
        let removes = match entry {
            Entry::Remove(remove) => Some(remove.device_id.clone()),
            _ => None,
        };
        let parent_anchor_key = state.anchor_admin_key();
        let revocations = state.revocations(record);
        state.advance(record, node.hash);
        let visit = self.visits.len();
        let instance = if handover.is_some() {
            visit
        } else {
            self.visits[parent].instance
        };
        self.visits.push(Visit {
            node: Some(index),
            parent: Some(parent),
            instance,
            hash: node.hash,
            rank: state.rank(),
            revision: state.revision,
            anchor: state.anchor_device_id.clone(),
            author: Some(record.proposal.proposal.author),
            handover,
            parent_anchor_key,
            revocations,
            yields_to,
            removes,
            kit_key: state.kit_key,
        });
        self.children.push(Vec::new());
        self.children[parent].push(visit);
        (state, visit)
    }

    fn parent(&self, visit: usize) -> usize {
        self.visits[visit].parent.unwrap_or(0)
    }

    fn signer(&self, visit: usize) -> Option<Signer> {
        self.visits[visit]
            .handover
            .as_ref()
            .map(|(signer, _)| *signer)
    }

    /// Whether `ancestor` is `visit` or one of its ancestors.
    fn is_ancestor(&self, ancestor: usize, mut visit: usize) -> bool {
        let floor = self.visits[ancestor].revision;
        loop {
            if visit == ancestor {
                return true;
            }
            match self.visits[visit].parent {
                Some(parent) if self.visits[visit].revision > floor => visit = parent,
                _ => return false,
            }
        }
    }

    /// The deepest common ancestor-or-self of `a` and `b`.
    fn common_ancestor(&self, mut a: usize, mut b: usize) -> usize {
        while a != b {
            let (here, there) = (self.visits[a].revision, self.visits[b].revision);
            if here >= there {
                a = self.parent(a);
            }
            if there >= here {
                b = self.parent(b);
            }
        }
        a
    }

    /// The rotation vetoes this walk's rotations cast, see [`Resolution`].
    fn vetoes(&self) -> Vetoes {
        let mut rotations: BTreeMap<AdminPublicKey, Vec<usize>> = BTreeMap::new();
        for (index, visit) in self.visits.iter().enumerate() {
            for revocation in &visit.revocations {
                if let Revocation::KitRotation(key) = revocation {
                    rotations.entry(*key).or_default().push(index);
                }
            }
        }
        let conflicted: BTreeSet<AdminPublicKey> = rotations
            .iter()
            .filter(|(_, members)| {
                members.iter().any(|a| {
                    members
                        .iter()
                        .any(|b| !self.is_ancestor(*a, *b) && !self.is_ancestor(*b, *a))
                })
            })
            .map(|(key, _)| *key)
            .collect();
        let kit_handovers = (0..self.visits.len())
            .filter(|index| self.signer(*index) == Some(Signer::Kit))
            .filter(|index| {
                self.visits[*index].author.is_some_and(|key| {
                    conflicted.contains(&key)
                        || rotations.get(&key).is_some_and(|members| {
                            members
                                .iter()
                                .any(|rotation| !self.is_ancestor(*index, *rotation))
                        })
                })
            })
            .map(|index| self.visits[index].hash)
            .collect();
        Vetoes {
            conflicted,
            kit_handovers,
        }
    }

    /// The winner of each instance a pin source passes through: the handover out of it on the
    /// ancestry of the highest ranked kit handover no rotation vetoed.
    fn pinned(&self) -> BTreeMap<usize, usize> {
        let mut sources: Vec<usize> = (0..self.visits.len())
            .filter(|index| {
                self.signer(*index) == Some(Signer::Kit)
                    && !self
                        .vetoes
                        .kit_handovers
                        .contains(&self.visits[*index].hash)
            })
            .collect();
        sources.sort_by(|a, b| self.visits[*b].rank.cmp(&self.visits[*a].rank));
        let mut winners = BTreeMap::new();
        for source in sources {
            let mut at = source;
            while let Some(parent) = self.visits[at].parent {
                if self.visits[at].handover.is_some() {
                    winners.entry(self.visits[parent].instance).or_insert(at);
                }
                at = parent;
            }
        }
        winners
    }

    /// Entries of the parent's instance that revoke the signer key or the target of
    /// `handover`, after its parent and not through it.
    fn objections(&self, handover: usize) -> Vec<usize> {
        let visit = &self.visits[handover];
        let Some((_, target)) = &visit.handover else {
            return Vec::new();
        };
        let revokes = |revocation: &Revocation| match revocation {
            Revocation::Device(device) => device == target,
            Revocation::Key(key) => Some(*key) == visit.author,
            Revocation::KitRotation(_) => false,
        };
        let mut found = Vec::new();
        let mut stack: Vec<usize> = self.children[self.parent(handover)]
            .iter()
            .copied()
            .filter(|child| *child != handover)
            .collect();
        while let Some(at) = stack.pop() {
            // A handover opens another instance, whose entries object to nothing here.
            if self.visits[at].handover.is_some() {
                continue;
            }
            if self.visits[at].revocations.iter().any(revokes) {
                found.push(at);
            }
            stack.extend(self.children[at].iter().copied());
        }
        found
    }

    /// The author key and named handover hash of every stored yield.
    fn yields(&self) -> Vec<(AdminPublicKey, Digest32)> {
        self.visits
            .iter()
            .filter_map(|visit| visit.author.zip(visit.yields_to))
            .collect()
    }

    /// Revision past which a writer fork of its anchor freezes the instance opened at `root`.
    fn fork(&self, root: usize) -> Option<u64> {
        let visit = &self.visits[root];
        frozen_at(&self.stored.freezes, &visit.anchor, visit.revision)
    }

    /// The chain from the charter through `visit`.
    fn selection(&self, visit: usize) -> Selection {
        let mut path = Vec::new();
        let mut at = Some(visit);
        while let Some(index) = at {
            path.push(index);
            at = self.visits[index].parent;
        }
        path.reverse();
        let mut selection = Selection {
            state: AuthState::from_charter(&self.stored.charter, Arc::clone(&self.vetoes)),
            path: Vec::new(),
            resolutions: Vec::new(),
            reachable: 0,
            held: BTreeSet::new(),
            handed_over: false,
        };
        for node in path.iter().filter_map(|index| self.visits[*index].node) {
            selection = selection.extended(&self.stored.nodes[node]);
        }
        selection
    }
}

/// How an epoch instance resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// No handover leaves the instance and no writer fork freezes it.
    Open,
    /// The handover out of the instance that wins it.
    Won(usize),
    /// Contested without a winner: only the visit and its ancestors are effective.
    FrozenAt(usize),
    /// A writer fork of its anchor with no handover out of it: entries past the revision are
    /// not effective.
    FrozenPast(u64),
}

/// The authority the stored tree derives, all of it before the head is selected.
///
/// Two walks build the tree of check-valid entries from the charter. The first collects every
/// rotation away from a kit key that the key signed; two rotations away from one key, neither an
/// ancestor of the other, refuse each other, and a kit handover that is not an ancestor of every
/// rotation away from its key, or whose key is in such a conflict, keeps its validity and rule K
/// but neither pins nor counts in rank. The second walk applies those vetoes.
///
/// Each epoch instance, identified by the entry that opened it, has the handovers out of it as
/// candidates. An entry of the instance after a candidate's parent, not through the candidate,
/// that revokes its signer key or its target objects to it. The instance is contested when it
/// has two candidates or an objected one, and resolves in this order: the candidate on the
/// ancestry of the highest ranked unvetoed kit handover wins; a lone unobjected candidate wins;
/// a candidate every other contestant and, for objections, the instance's anchor yielded to wins,
/// the lowest hash among several; otherwise a contested instance freezes at the common ancestor
/// of the candidates' parents, lowered to the revision a writer fork of its anchor froze it at, and an
/// uncontested one with a writer fork freezes past that revision.
///
/// An entry is effective when its parent is, it is not a handover that lost or left a frozen
/// instance, it is not off the path to a contested freeze point, it is not past a writer-fork
/// freeze, and in an instance whose winner has a writer fork, it is not past the fork beside the
/// winner's ancestry. The effective entry of highest rank is the head. Nothing reads the selected
/// chain afterwards to change any of this: the head reports the state its ancestry derives, the
/// freeze point, and the devices held for data. The result depends only on the stored entries
/// and the writer forks, not on arrival order or the writers that carried them.
struct Resolution<'a> {
    walk: Walk<'a>,
    /// Handovers out of each instance, by instance root.
    candidates: BTreeMap<usize, Vec<usize>>,
    /// Outcome of each instance, by instance root.
    outcomes: BTreeMap<usize, Outcome>,
    /// Whether each visit is effective.
    effective: Vec<bool>,
    /// The effective visit of highest rank.
    best: usize,
    /// Highest revision any check-valid chain reaches.
    reachable: u64,
}

impl<'a> Resolution<'a> {
    fn new(stored: &'a Stored) -> Self {
        let first = Walk::new(stored, Arc::default());
        let vetoes = first.vetoes();
        let walk = if vetoes == Vetoes::default() {
            first
        } else {
            Walk::new(stored, Arc::new(vetoes))
        };
        let mut candidates: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for index in 1..walk.visits.len() {
            if walk.visits[index].handover.is_some() {
                let instance = walk.visits[walk.parent(index)].instance;
                candidates.entry(instance).or_default().push(index);
            }
        }
        let pinned = walk.pinned();
        let yields = walk.yields();
        let mut outcomes = BTreeMap::new();
        let mut forks = BTreeMap::new();
        for root in 0..walk.visits.len() {
            if walk.visits[root].instance != root {
                continue;
            }
            let fork = walk.fork(root);
            let own = candidates.get(&root).map_or(&[][..], Vec::as_slice);
            outcomes.insert(
                root,
                Self::outcome(&walk, root, own, fork, &pinned, &yields),
            );
            forks.insert(root, fork);
        }
        let mut reachable = 0;
        let mut effective = vec![true; walk.visits.len()];
        for index in 0..walk.visits.len() {
            let visit = &walk.visits[index];
            reachable = reachable.max(visit.revision);
            // Copies of a forked anchor writer may carry entries past the revision its instance
            // froze at, which no replica keeps once the fork is excluded. An author that applied
            // one before the fork was found must not wait for it, so a chain that reaches a
            // writer-fork freeze reaches every revision. Each operation's device is still
            // checked against the selected chain when it applies.
            if forks
                .get(&visit.instance)
                .copied()
                .flatten()
                .is_some_and(|fork| visit.revision >= fork)
            {
                reachable = UNBOUNDED_REVISION;
            }
            if index > 0 {
                effective[index] = effective[walk.parent(index)]
                    && Self::admitted(&walk, index, &outcomes, &forks);
            }
        }
        let best = (0..walk.visits.len())
            .filter(|index| effective[*index])
            .max_by(|a, b| walk.visits[*a].rank.cmp(&walk.visits[*b].rank))
            .unwrap_or(0);
        Self {
            walk,
            candidates,
            outcomes,
            effective,
            best,
            reachable,
        }
    }

    /// How the instance opened at `root` resolves, see [`Resolution`].
    fn outcome(
        walk: &Walk<'_>,
        root: usize,
        candidates: &[usize],
        fork: Option<u64>,
        pinned: &BTreeMap<usize, usize>,
        yields: &[(AdminPublicKey, Digest32)],
    ) -> Outcome {
        if let Some(winner) = pinned.get(&root) {
            return Outcome::Won(*winner);
        }
        let Some(first) = candidates.first() else {
            return fork.map_or(Outcome::Open, Outcome::FrozenPast);
        };
        let objections: Vec<Vec<usize>> = candidates
            .iter()
            .map(|candidate| walk.objections(*candidate))
            .collect();
        if candidates.len() == 1 && objections.iter().all(Vec::is_empty) {
            return Outcome::Won(*first);
        }
        let accepted = candidates
            .iter()
            .zip(&objections)
            .filter(|(candidate, objected)| {
                let hash = walk.visits[**candidate].hash;
                let gave_way = |key: Option<AdminPublicKey>| {
                    key.is_some_and(|key| {
                        yields
                            .iter()
                            .any(|(author, named)| *author == key && *named == hash)
                    })
                };
                candidates
                    .iter()
                    .all(|other| other == *candidate || gave_way(walk.visits[*other].author))
                    && (objected.is_empty() || gave_way(walk.visits[**candidate].parent_anchor_key))
            })
            .map(|(candidate, _)| *candidate)
            .min_by_key(|candidate| walk.visits[*candidate].hash);
        if let Some(winner) = accepted {
            return Outcome::Won(winner);
        }
        let mut point = candidates
            .iter()
            .map(|candidate| walk.parent(*candidate))
            .reduce(|a, b| walk.common_ancestor(a, b))
            .unwrap_or(root);
        if let Some(fork) = fork {
            while point != root && walk.visits[point].revision > fork {
                point = walk.parent(point);
            }
        }
        Outcome::FrozenAt(point)
    }

    /// Whether visit `index`, whose parent is effective, is effective itself.
    fn admitted(
        walk: &Walk<'_>,
        index: usize,
        outcomes: &BTreeMap<usize, Outcome>,
        forks: &BTreeMap<usize, Option<u64>>,
    ) -> bool {
        let visit = &walk.visits[index];
        if visit.handover.is_some() {
            let out_of = walk.visits[walk.parent(index)].instance;
            if outcomes.get(&out_of) != Some(&Outcome::Won(index)) {
                return false;
            }
        }
        match outcomes.get(&visit.instance) {
            Some(Outcome::FrozenAt(point)) => walk.is_ancestor(index, *point),
            Some(Outcome::FrozenPast(fork)) => visit.revision <= *fork,
            Some(Outcome::Won(winner)) => match forks.get(&visit.instance).copied().flatten() {
                Some(fork) => visit.revision <= fork || walk.is_ancestor(index, *winner),
                None => true,
            },
            Some(Outcome::Open) | None => true,
        }
    }

    /// Revision past which the head's instance is frozen, if it is.
    fn frozen_at(&self) -> Option<u64> {
        match self.outcomes.get(&self.walk.visits[self.best].instance) {
            Some(Outcome::FrozenAt(point)) => Some(self.walk.visits[*point].revision),
            Some(Outcome::FrozenPast(fork)) => Some(*fork),
            _ => None,
        }
    }

    /// Devices a stored entry that is not effective removes after the freeze point of the
    /// head's instance, while it is frozen.
    fn held(&self) -> BTreeSet<String> {
        let instance = self.walk.visits[self.best].instance;
        let start = match self.outcomes.get(&instance) {
            Some(Outcome::FrozenAt(point)) => *point,
            Some(Outcome::FrozenPast(_)) => instance,
            _ => return BTreeSet::new(),
        };
        let mut held = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            if !self.effective[at]
                && let Some(device) = &self.walk.visits[at].removes
            {
                held.insert(device.clone());
            }
            stack.extend(self.walk.children[at].iter().copied());
        }
        held
    }

    /// The selected chain, with the freeze, the hold, and the handovers of the head's instance.
    fn selected(&self) -> Selection {
        let mut selection = self.walk.selection(self.best);
        selection.state.frozen_at = self.frozen_at();
        selection.held = self.held();
        selection.handed_over = self
            .candidates
            .get(&self.walk.visits[self.best].instance)
            .is_some_and(|candidates| !candidates.is_empty());
        selection.reachable = self.reachable;
        selection
    }

    /// The highest ranked visit in the subtree of `start` where `signer` is the kit key and
    /// `target` a live desktop, staying in `start`'s instance when `within` is set.
    fn kit_point_under(
        &self,
        start: usize,
        within: bool,
        signer: &AdminPublicKey,
        target: &str,
    ) -> Option<usize> {
        let instance = self.walk.visits[start].instance;
        let mut subtree = Vec::new();
        let mut stack = vec![start];
        while let Some(at) = stack.pop() {
            if within && self.walk.visits[at].instance != instance {
                continue;
            }
            if self.walk.visits[at].kit_key == *signer {
                subtree.push(at);
            }
            stack.extend(self.walk.children[at].iter().copied());
        }
        subtree.sort_by(|a, b| self.walk.visits[*b].rank.cmp(&self.walk.visits[*a].rank));
        subtree
            .into_iter()
            .find(|visit| self.admits_kit_target(*visit, target))
    }

    fn admits_kit_target(&self, visit: usize, target: &str) -> bool {
        let state = self.walk.selection(visit).state;
        state.epoch < u32::MAX
            && state
                .live_device(target)
                .is_ok_and(|device| device.kind == DeviceKind::Desktop)
    }

    /// Visits a kit handover `signer` signs for the desktop `target` may continue from, the
    /// selected head first: when the head's instance is frozen, also the best entry under each
    /// handover out of it and on each branch of its own continuation past the freeze point.
    fn kit_points(&self, signer: &AdminPublicKey, target: &str) -> Vec<usize> {
        let mut points = Vec::new();
        if self.walk.visits[self.best].kit_key == *signer
            && self.admits_kit_target(self.best, target)
        {
            points.push(self.best);
        }
        let instance = self.walk.visits[self.best].instance;
        let continuations: Vec<usize> = match self.outcomes.get(&instance) {
            Some(Outcome::FrozenAt(point)) => self.walk.children[*point].clone(),
            Some(Outcome::FrozenPast(fork)) => (0..self.walk.visits.len())
                .filter(|index| {
                    let visit = &self.walk.visits[*index];
                    visit.instance == instance && visit.revision == fork.saturating_add(1)
                })
                .collect(),
            _ => return points,
        };
        let branches = self
            .candidates
            .get(&instance)
            .into_iter()
            .flatten()
            .map(|candidate| (*candidate, false))
            .chain(
                continuations
                    .into_iter()
                    .filter(|visit| self.walk.visits[*visit].handover.is_none())
                    .map(|visit| (visit, true)),
            );
        for (start, within) in branches {
            if let Some(point) = self.kit_point_under(start, within, signer, target)
                && !points.contains(&point)
            {
                points.push(point);
            }
        }
        points
    }
}

/// Selects the chain from the charter through the stored entries, or `None` without a charter.
/// See [`Resolution`] for the rule.
pub(crate) async fn select(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Option<Selection>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    Ok(Some(Resolution::new(&stored).selected()))
}

/// Whether any check-valid stored chain enrolls `device_id` with `device_key`, selected or not.
/// A replica that selected one branch still stores the writers of another, whose entries may
/// contest its selection. This admits storage only, never authority: selection and enrollment
/// read the selected chain. The walk is cached per stored entry set and freezes, so a batch of
/// geneses walks the tree once.
pub(crate) async fn enrolled_on_any_chain(
    conn: &mut SqliteConnection,
    cache: &EnrolledCache,
    space: SpaceId,
    device_id: &str,
    device_key: &DevicePublicKey,
) -> SyncResult<bool> {
    let key = EnrolledKey::load(conn, space).await?;
    let wanted = (device_id.to_string(), *device_key);
    if let Some(hit) = cache.lookup(&key) {
        return Ok(hit.contains(&wanted));
    }
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(false);
    };
    let enrolled = Arc::new(Walk::new(&stored, Arc::default()).enrolled);
    cache.store(key, Arc::clone(&enrolled));
    Ok(enrolled.contains(&wanted))
}

/// What the enrollments of every stored chain depend on: the space, its charter, the hashes of
/// its stored authorization entries, and its freezes. The charter keeps a reset space that a
/// new charter founds, or another vault with the same space, from reading a stale answer.
#[derive(Debug, PartialEq, Eq)]
struct EnrolledKey {
    space: SpaceId,
    charter: Option<Digest32>,
    entries: Vec<Digest32>,
    freezes: BTreeMap<String, Vec<u64>>,
}

impl EnrolledKey {
    async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        let rows: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT entry_hash FROM sync_auth_entries WHERE space_id = ? ORDER BY entry_hash",
        )
        .bind(space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
        let entries = rows
            .iter()
            .map(|hash| log::digest(hash))
            .collect::<SyncResult<Vec<_>>>()?;
        Ok(Self {
            space,
            charter: load_charter(conn, space)
                .await?
                .map(|charter| charter.hash()),
            entries,
            freezes: fork::freezes(conn, space).await?,
        })
    }
}

/// The device ids and keys any check-valid chain enrolls, kept for the last stored entry set an
/// engine walked. The key is the stored content, so a rolled back transaction cannot leave a
/// stale answer behind.
#[derive(Debug, Default)]
pub(crate) struct EnrolledCache(Mutex<Option<(EnrolledKey, Arc<EnrolledSet>)>>);

type EnrolledSet = BTreeSet<(String, DevicePublicKey)>;

impl EnrolledCache {
    fn lookup(&self, key: &EnrolledKey) -> Option<Arc<EnrolledSet>> {
        let slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_ref()
            .filter(|(cached, _)| cached == key)
            .map(|(_, enrolled)| Arc::clone(enrolled))
    }

    fn store(&self, key: EnrolledKey, enrolled: Arc<EnrolledSet>) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some((key, enrolled));
    }
}

/// The state after the stored entry at `revision` with `entry_hash` when it is one of the
/// points a kit handover `author` signs for `target` may continue from, see
/// [`Resolution::kit_points`].
async fn kit_point(
    conn: &mut SqliteConnection,
    space: SpaceId,
    author: &AdminPublicKey,
    target: &str,
    revision: u64,
    entry_hash: &Digest32,
) -> SyncResult<Option<AuthState>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    let resolution = Resolution::new(&stored);
    Ok(resolution
        .kit_points(author, target)
        .into_iter()
        .find(|visit| {
            let visit = &resolution.walk.visits[*visit];
            visit.hash == *entry_hash && visit.revision == revision
        })
        .map(|visit| resolution.walk.selection(visit).state))
}

/// Where a handover that `signer` signs for the desktop `target` continues from: the first kit
/// point, see [`Resolution::kit_points`], or else the selected head. `None` without a charter.
async fn handover_point(
    conn: &mut SqliteConnection,
    space: SpaceId,
    signer: &AdminPublicKey,
    target: &str,
) -> SyncResult<Option<HandoverPoint>> {
    let Some(stored) = Stored::load(conn, space).await? else {
        return Ok(None);
    };
    let resolution = Resolution::new(&stored);
    if let Some(point) = resolution.kit_points(signer, target).first() {
        let state = resolution.walk.selection(*point).state;
        return Ok(Some(HandoverPoint::of(&state, true)));
    }
    let state = resolution.selected().state;
    let by_kit = state.kit_key == *signer;
    Ok(Some(HandoverPoint::of(&state, by_kit)))
}

/// Where a handover that `signer` signs for the desktop `target` continues from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoverPoint {
    /// Epoch of the entry, which the handover follows with the next one.
    pub epoch: u32,
    /// Revision of the entry.
    pub revision: u64,
    /// Hash of the entry.
    pub entry_hash: Digest32,
    /// Anchor of the entry's epoch.
    pub anchor_device_id: String,
    /// Whether `signer` is the kit key there.
    pub by_kit: bool,
}

impl HandoverPoint {
    fn of(state: &AuthState, by_kit: bool) -> Self {
        Self {
            epoch: state.epoch,
            revision: state.revision,
            entry_hash: state.entry_hash,
            anchor_device_id: state.anchor_device_id.clone(),
            by_kit,
        }
    }
}

/// What the selection rule derives from stored entries, for its tests.
#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectedHead {
    /// Hash of the selected head.
    pub(crate) head: Digest32,
    /// Highest revision any stored chain reaches.
    pub(crate) reachable: u64,
    /// Revision past which the selected epoch is frozen.
    pub(crate) frozen_at: Option<u64>,
    /// Anchor of the selected epoch.
    pub(crate) anchor: String,
    /// Revision that removed each enrolled device, by device id, `None` for a live one.
    pub(crate) devices: BTreeMap<String, Option<u64>>,
    /// Live devices with an admin role.
    pub(crate) admins: BTreeSet<String>,
    /// Devices held for data while the head's instance is frozen.
    pub(crate) held: BTreeSet<String>,
    /// Unvetoed kit handovers on the selected chain.
    pub(crate) kit_handovers: u32,
    /// The recovery key the head names.
    pub(crate) recovery_admin_key: AdminPublicKey,
}

/// What the stored `records` derive from `charter`, each carried by its writer, for tests of
/// the selection rule.
#[cfg(test)]
pub(crate) fn select_records(
    charter: &SignedCharter,
    freezes: BTreeMap<String, Vec<u64>>,
    records: &[(WriterId, AuthorizationRecord)],
) -> SelectedHead {
    let mut seen = BTreeSet::new();
    let nodes = records
        .iter()
        .filter(|(_, record)| seen.insert(record.hash()))
        .enumerate()
        .map(|(index, (writer, record))| Node {
            writer: *writer,
            seq: u64::try_from(index).unwrap_or(u64::MAX),
            hash: record.hash(),
            record: record.clone(),
        })
        .collect();
    let stored = Stored {
        charter: charter.clone(),
        freezes: Arc::new(freezes),
        nodes,
    };
    let selection = Resolution::new(&stored).selected();
    let state = &selection.state;
    SelectedHead {
        head: state.entry_hash,
        reachable: selection.reachable,
        frozen_at: state.frozen_at,
        anchor: state.anchor_device_id.clone(),
        devices: state
            .devices
            .iter()
            .map(|(id, device)| (id.clone(), device.removed))
            .collect(),
        admins: state
            .devices
            .iter()
            .filter(|(_, device)| {
                device.removed.is_none() && matches!(device.role, DeviceRole::Admin(_))
            })
            .map(|(id, _)| id.clone())
            .collect(),
        held: selection.held.clone(),
        kit_handovers: state.kit_handovers,
        recovery_admin_key: state.recovery_admin_key,
    }
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

/// The revision `writer`'s next content operation is sealed against: the selected revision, but
/// never below what the writer's own stored operations were sealed against. A freeze or a
/// reselection can lower the selected revision under an anchor that already sequenced past it,
/// and fork evidence is provable only when no forked operation claims a revision below the
/// shared operation before it, so a writer's revisions never decrease.
pub(crate) async fn sealing_revision(
    conn: &mut SqliteConnection,
    space: SpaceId,
    writer: WriterId,
) -> SyncResult<u64> {
    let selected = selected_revision(conn, space).await?;
    let own: Option<i64> = sqlx::query_scalar(
        "SELECT max(authorization_revision) FROM sync_ops WHERE space_id = ? AND writer_id = ?",
    )
    .bind(space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_one(&mut *conn)
    .await?;
    let own = own.map_or(Ok(0), |revision| {
        stored_u64(revision, "authorization revision")
    })?;
    Ok(selected.max(own))
}

/// Whether a writer of `carrier_device` may carry `record`: only writers of the device the record
/// names as anchor carry entries, and only well-formed ones.
pub(crate) fn carries(carrier_device: &str, record: &AuthorizationRecord) -> bool {
    carrier_device == record.anchor_device_id && record.validate().is_ok()
}

/// Records a kind 4 operation of `writer` at `seq` as a log entry.
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

/// Selects the chain and writes what it derives: the selected flags, `sync_devices` with the
/// devices held for data, the reachable revision, the anchor, and the writer state of removed devices. Writers of a removed
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
                 enrolled_revision, removed_revision, held)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(i64::from(selection.held.contains(device_id)))
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

/// The cutoff the selected removal of `device_id` lists for `writer`, 0 when it names none.
pub(crate) async fn removal_cutoff(
    conn: &mut SqliteConnection,
    space: SpaceId,
    device_id: &str,
    writer: WriterId,
) -> SyncResult<Seq> {
    let envelope: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT o.envelope
         FROM sync_devices d
         JOIN sync_auth_entries e
            ON e.space_id = d.space_id AND e.revision = d.removed_revision AND e.selected = 1
         JOIN sync_ops o ON o.space_id = e.space_id AND o.writer_id = e.writer_id AND o.seq = e.seq
         WHERE d.space_id = ? AND d.device_id = ?
         LIMIT 1",
    )
    .bind(space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    let cutoff = match envelope
        .as_deref()
        .and_then(record_of)
        .map(|record| record.sequencing)
    {
        Some(Sequencing::Removal { cutoffs, .. }) => cutoffs
            .iter()
            .find(|(named, _)| *named == writer)
            .map_or(0, |(_, seq)| *seq),
        _ => 0,
    };
    Ok(cutoff)
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

    /// Where a handover that `signer` signs for the desktop `target` continues from: the
    /// selected head when `signer` is its kit key and `target` a live desktop there, or, while
    /// the head's instance is frozen, the best such entry on a branch past the freeze, so a kit
    /// can recover from a stolen anchor that removed its target; else the selected head. `None`
    /// without a charter.
    pub async fn handover_point(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        signer: &AdminPublicKey,
        target: &str,
    ) -> SyncResult<Option<HandoverPoint>> {
        log::ensure_space(conn, ctx.space).await?;
        handover_point(conn, ctx.space, signer, target).await
    }

    /// Seals an admin-signed handover as the first entry of the epoch it opens, in the local
    /// writer's chain. The local device must be the handover's new anchor and the selected head
    /// the revision the handover continues from, or, for the kit key, one of its kit points,
    /// see [`Self::handover_point`]; `old_anchor_vector` is the old anchor's stored vector when
    /// known. A frozen epoch does not stop a handover, but a target refuses a take out of an
    /// instance that another handover already leaves. Returns the revision, or why the handover
    /// was rejected; one already in the selected chain returns its revision without sealing
    /// again.
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
        let handed_over = selection.handed_over;
        let mut state = selection.state;
        let local_device = &writer.certificate.certificate.device_id;
        let record = match (ordering, &proposal.proposal.entry) {
            (Ordering::Anchor { .. }, _) if *local_device != state.anchor_device_id => {
                return Ok(Err(ProposalRejection::NotAnchor));
            }
            // An anchor that stores a handover out of its instance stops sequencing there.
            (Ordering::Anchor { .. }, _) if handed_over => {
                return Ok(Err(ProposalRejection::HandoverConflict));
            }
            (
                Ordering::Anchor {
                    discovery_key,
                    covered,
                },
                entry,
            ) => {
                let sequencing = match entry {
                    Entry::Enroll(_)
                    | Entry::SetRole(_)
                    | Entry::ResolveLate(_)
                    | Entry::Yield(_) => Sequencing::Empty,
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
                    // The kit key may continue from a branch past a freeze, such as one before
                    // a stolen anchor removed its target.
                    let point = kit_point(
                        &mut tx,
                        ctx.space,
                        &proposal.proposal.author,
                        &handover.new_anchor_device_id,
                        handover.continues_from_revision,
                        &handover.continues_from_hash,
                    )
                    .await?;
                    let Some(point) = point else {
                        return Ok(Err(ProposalRejection::HandoverConflict));
                    };
                    state = point;
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
        // A take out of an instance that another handover already leaves would only contest it;
        // the kit alone continues there.
        if handed_over
            && state
                .signer(&record)
                .is_some_and(|signer| signer != Signer::Kit)
        {
            return Ok(Err(ProposalRejection::HandoverConflict));
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

/// How far past what the anchor stored a removal cutoff may follow the covered vector the
/// coordinator reports: operations other replicas stored that have not reached the anchor yet.
pub(crate) const MAX_COVERED_AHEAD: Seq = 1024;

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
/// `covered` names but this replica never stored has no row and gets no cutoff. `covered` comes
/// from the coordinator, so it raises a cutoff at most [`MAX_COVERED_AHEAD`] past what this
/// replica stored or the old anchor finalized: a coordinator cannot keep a removal from
/// excluding anything by reporting an unbounded vector.
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
            let known = stored.max(finalized);
            let reported = covered
                .get(&writer)
                .min(known.saturating_add(MAX_COVERED_AHEAD));
            Ok((writer, known.max(reported)))
        })
        .collect::<SyncResult<_>>()
        .map(Some)
}

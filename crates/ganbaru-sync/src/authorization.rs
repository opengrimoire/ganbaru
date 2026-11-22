//! The authorization log: the space charter, kind 4 entries in the anchor's writer chain, chain
//! selection, and the device and writer state derived from the selected chain.
//!
//! Every authorization record envelope, writer certificate, and fork evidence envelope that
//! passes its intrinsic checks is kept in the record store (`records`). Selection walks the tree
//! of stored records whose rules hold at their parent, whose anchor sequence lies below its
//! writer's horizon, and whose anchor is bound by a stored certificate of a carrier, and keeps
//! the effective entry of highest rank; `sync_auth_selected`, `sync_devices`, and the writer
//! states of removed devices are recomputed from the selected chain whenever the store grows, so
//! a reselection reverts what a superseded chain set.
//!
//! A handover is taken: the target device's admin key signs it, or the kit key, or an installed
//! recovery key. Each epoch instance resolves its handovers before the head is chosen. Two
//! handovers out of one instance, or an entry of it that revokes a handover's signer or target,
//! contest it; a kit handover's ancestry pins the winner, a lone candidate wins, a yield that
//! every other contestant signed accepts one, and otherwise the instance freezes. A kit or
//! recovery handover demotes every other admin and resets the recovery key to the kit key. A
//! provable fork of a writer that carries an instance's records freezes the instance at the last
//! record that writer carries below the fork. While the selected instance is frozen, devices
//! that an ineffective entry past the freeze removes are held: their operations are stored but
//! not applied.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ganbaru_sync_contracts::bounds::DISCOVERY_KEY_BYTES;
use ganbaru_sync_contracts::{
    AdminPublicKey, AuthorizationRecord, ChainLink, Content, DeviceKind, DevicePublicKey,
    DeviceRole, Digest32, EncryptionPublicKey, Entry, Envelope, KeyEnvelope, KeyEpoch, KeyId,
    Proposal, ProposalRejection, Seq, Sequencing, Server, SetRole, SignedCharter, SpaceId,
    VersionVector, WriterId,
};
use sqlx::SqliteConnection;

use crate::error::{SyncError, SyncResult, corrupt};
use crate::exclusion;
use crate::log::{self, seq_i64, stored_u64};
use crate::records;

mod contest;
mod contest_view;
mod engine;
mod exchange;
mod key_epoch;
mod selection;

pub use contest::ContestRejection;
pub use contest_view::{
    CandidateKind, ContestAction, ContestCandidate, ContestCause, ContestEntry, ContestInstance,
    ContestObjection, ContestRole, ContestView, ContestYield, ContestsView, KitPoint,
    KitPointPlace, LosingRemoval,
};
#[cfg(test)]
pub(crate) use engine::MAX_COVERED_AHEAD;
use engine::{follow_with_rotation, removal_cutoffs};
pub use exchange::{
    AuthorizationStorage, AuthorizationTotals, ForkedWriter, RECORD_CLASSES, RecordAdmission,
    RecordClass, RecordCursor, RecordEnvelope, RecordPage, RecordRole, RecordWants,
};
pub use key_epoch::KeyEpochStanding;
pub(crate) use key_epoch::key_epoch_standing;
pub use selection::HandoverPoint;
pub(crate) use selection::{EnrolledCache, Selection, enrolled_on_any_chain, select};
use selection::{Outcome, Resolution, Stored, Walk};
#[cfg(test)]
pub(crate) use selection::{
    ReversalCheck, SelectedHead, reversal_in_records, select_records, tour_spans,
};

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
    /// Whether a frozen contest holds the device: a stored branch that is not effective removes
    /// it, so it stays enrolled and is served authorization records only.
    pub held: bool,
    /// The encryption key its latest device key entry published.
    pub encryption_key: Option<EncryptionPublicKey>,
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
    /// Admin keys the selected chain took away without removing a device: a device's admin key
    /// a role change replaced, and recovery and kit keys a rotation or kit handover replaced. A
    /// key stays here when a later entry names it again, so readers subtract the live set.
    pub retired_admin_keys: BTreeSet<AdminPublicKey>,
    /// The discovery key devices advertise under now: the charter's, replaced by each removal.
    pub discovery_key: [u8; DISCOVERY_KEY_BYTES],
    /// The key the latest removal replaced, which devices that have not applied it yet still
    /// advertise under.
    pub previous_discovery_key: Option<[u8; DISCOVERY_KEY_BYTES]>,
    /// The space key epoch of the selected head; 0 before the first recovery key.
    pub key_epoch: u32,
    /// The encryption key of the recovery kit the latest recovery key entry published.
    pub recovery_encryption_key: Option<EncryptionPublicKey>,
    /// Device a removal at the head names that takes effect once the anchor's key epoch
    /// follows it.
    pub pending_removal: Option<String>,
    /// The sync server the latest server configuration names.
    pub server: Option<Server>,
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
    /// The encryption key its latest device key entry published.
    encryption_key: Option<EncryptionPublicKey>,
}

/// A space key rotation the anchor seals as the entry right after the one that needs it: a
/// removal, or a recovery change, in a space with a key epoch, and the first recovery key.
#[derive(Debug, Clone)]
struct PendingRotation {
    /// The removal that takes effect with the key epoch, if a removal needs it.
    removal: Option<PendingRemoval>,
}

/// A removal waiting for its key epoch.
#[derive(Debug, Clone)]
struct PendingRemoval {
    device_id: String,
    revision: u64,
    cutoffs: BTreeMap<WriterId, Seq>,
    discovery_key: Option<[u8; DISCOVERY_KEY_BYTES]>,
}

impl PendingRemoval {
    fn of(device_id: &str, record: &AuthorizationRecord) -> Self {
        let (cutoffs, discovery_key) = match &record.sequencing {
            Sequencing::Removal {
                cutoffs,
                discovery_key,
            } => (cutoffs.iter().copied().collect(), Some(*discovery_key)),
            _ => (BTreeMap::new(), None),
        };
        Self {
            device_id: device_id.to_string(),
            revision: record.revision,
            cutoffs,
            discovery_key,
        }
    }
}

/// The key material one key epoch of the selected chain publishes: its chain link, and the
/// envelopes of its key epoch entry followed by those device key entries of the epoch carry.
#[derive(Debug, Clone)]
pub(crate) struct EpochKeys {
    epoch: u32,
    revision: u64,
    wrapped_previous: Option<ChainLink>,
    envelopes: Vec<KeyEnvelope>,
}

/// One key epoch of the selected chain as `sync_space_keys` stores it for the local device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSpaceKey {
    /// The key epoch.
    pub epoch: u32,
    /// The previous epoch's key sealed under this epoch's key; `None` for epoch 1.
    pub wrapped_previous: Option<ChainLink>,
    /// The envelope of this epoch's key to the local device's published encryption key.
    pub own_envelope: Option<KeyEnvelope>,
    /// Revision of the key epoch entry.
    pub sequenced_revision: u64,
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
    /// Admin keys the chain took away without removing a device, see
    /// [`AuthorizationView::retired_admin_keys`].
    retired_admin_keys: Arc<BTreeSet<AdminPublicKey>>,
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
    /// The space key epoch: 0 until the first recovery key, then the latest key epoch entry's.
    key_epoch: u32,
    /// The recovery kit's encryption key, which every key epoch seals an envelope to.
    recovery_encryption_key: Option<EncryptionPublicKey>,
    /// The rotation the head waits for: only the anchor's key epoch entry may follow it.
    pending: Option<PendingRotation>,
    /// The sync server the latest server configuration names.
    server: Option<Server>,
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
            encryption_key: None,
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
            retired_admin_keys: Arc::default(),
            frozen_at: None,
            vetoes,
            catch_up: None,
            discovery_key: body.discovery_key,
            previous_discovery_key: None,
            proposals: Arc::default(),
            key_epoch: 0,
            recovery_encryption_key: None,
            pending: None,
            server: None,
        }
    }

    /// The encryption keys a key epoch that completes `pending` seals envelopes to, by key id:
    /// every live device's published key except the one the pending removal names, and the
    /// recovery kit's key.
    fn rotation_recipients(
        &self,
        pending: &PendingRotation,
    ) -> BTreeMap<KeyId, EncryptionPublicKey> {
        let removed = pending
            .removal
            .as_ref()
            .map(|removal| removal.device_id.as_str());
        self.devices
            .iter()
            .filter(|(device_id, device)| {
                device.removed.is_none() && Some(device_id.as_str()) != removed
            })
            .filter_map(|(_, device)| device.encryption_key)
            .chain(self.recovery_encryption_key)
            .map(|key| (key.key_id(), key))
            .collect()
    }

    /// Marks the device of `removal` removed and rotates the discovery key it names.
    fn apply_removal(&mut self, removal: PendingRemoval) {
        if let Some(device) = self.devices.get_mut(&removal.device_id) {
            device.removed = Some(removal.revision);
            device.cutoffs = removal.cutoffs;
        }
        if let Some(discovery_key) = removal.discovery_key
            && discovery_key != self.discovery_key
        {
            self.previous_discovery_key = Some(self.discovery_key);
            self.discovery_key = discovery_key;
        }
    }

    /// Records a rotation the anchor must seal next; in a space without a key epoch only the
    /// first recovery key starts one, and a removal applies at once.
    fn rotate(&mut self, removal: Option<PendingRemoval>, first: bool) {
        if self.key_epoch == 0 && !first {
            if let Some(removal) = removal {
                self.apply_removal(removal);
            }
            return;
        }
        self.pending = Some(PendingRotation { removal });
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

    /// Whether `entry` may be sequenced while a rotation is pending: the key epoch that completes
    /// it, and the anchor's own device key. An anchor that a handover or the kit made before it
    /// published an encryption key can seal no key epoch, so it publishes first, sealing the
    /// current key to itself from an envelope it opens, such as the recovery envelope.
    fn admits_while_pending(&self, entry: &Entry) -> bool {
        match entry {
            Entry::KeyEpoch(_) => true,
            Entry::DeviceKey(device_key) => device_key.device_id == self.anchor_device_id,
            _ => false,
        }
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
        // or admin role after a later entry replaced it, is refused. This covers the walked
        // chain; the anchor refuses an entry a stored abandoned branch reversed when it sequences.
        if self.proposals.contains(&record.proposal.hash()) {
            return Err(ProposalRejection::Malformed);
        }
        // A device key or key epoch entry is signed by a device key, which its arm checks.
        if proposal.device_author().is_none() && !self.is_admin(&proposal.author) {
            return Err(ProposalRejection::NotAdmin);
        }
        if let Entry::Handover(handover) = &proposal.entry {
            if record.epoch != self.epoch.wrapping_add(1) || self.epoch == u32::MAX {
                return Err(ProposalRejection::HandoverConflict);
            }
            let device = self.live_device(&handover.new_anchor_device_id)?;
            // A handover continues past a pending rotation, whose key epoch the new anchor
            // seals, but never to the device the pending removal names.
            let pending_removal = self
                .pending
                .as_ref()
                .and_then(|pending| pending.removal.as_ref());
            if pending_removal
                .is_some_and(|removal| removal.device_id == handover.new_anchor_device_id)
            {
                return Err(ProposalRejection::DeviceRemoved);
            }
            // A handover is a take, signed by the admin key its target holds, unless the kit key
            // or an admitted installed recovery key signed it: those name any live desktop or
            // server, because the anchor that would sequence its promotion to admin may be lost.
            if self.signer(record).is_none() {
                return Err(ProposalRejection::NotAdmin);
            }
            if !device.kind.may_be_admin() {
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
        if self.pending.is_some() && !self.admits_while_pending(&proposal.entry) {
            return Err(ProposalRejection::RotationPending);
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
                if matches!(enroll.role, DeviceRole::Admin(_)) && !enroll.device_kind.may_be_admin()
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
                // The anchor is never demoted, and only its own key, the kit, or an admitted
                // recovery key names its admin key: another admin naming a key it holds would
                // take the anchor's objection, recovery key installation, and removal. The kit
                // and recovery cases promote the device a forced handover made anchor.
                if *device_id == self.anchor_device_id {
                    let author = proposal.author;
                    let keeps_authority = self.anchor_admin_key() == Some(author)
                        || author == self.kit_key
                        || self.recovery_admits(&author);
                    if *role == DeviceRole::Member || !keeps_authority {
                        return Err(ProposalRejection::NamesAnchor);
                    }
                }
                if matches!(role, DeviceRole::Admin(_)) && !device.kind.may_be_admin() {
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
            Entry::DeviceKey(device_key) => {
                let device = self.live_device(&device_key.device_id)?;
                if proposal.device_author() != Some(device.device_key) {
                    return Err(ProposalRejection::NotAdmin);
                }
                // In a space with a key epoch the anchor seals the current key to the new
                // encryption key; before the first one there is nothing to seal.
                let sealed = matches!(
                    record.sequencing,
                    Sequencing::DeviceKey { envelope: Some(_) }
                );
                if sealed != (self.key_epoch > 0) {
                    return Err(ProposalRejection::Malformed);
                }
            }
            Entry::KeyEpoch(key_epoch) => self.check_key_epoch(proposal, key_epoch)?,
            Entry::RecoveryKey(_) => {
                if proposal.author != self.kit_key
                    && self.anchor_admin_key() != Some(proposal.author)
                {
                    return Err(ProposalRejection::NotAdmin);
                }
            }
            Entry::ServerConfig(_) => {}
        }
        Ok(())
    }

    /// Whether `key_epoch`, which `proposal` carries, completes the pending rotation: the next
    /// epoch, signed by the live anchor's device key, with one envelope to each key of
    /// [`Self::rotation_recipients`] and no other.
    fn check_key_epoch(
        &self,
        proposal: &Proposal,
        key_epoch: &KeyEpoch,
    ) -> Result<(), ProposalRejection> {
        let Some(pending) = &self.pending else {
            return Err(ProposalRejection::Malformed);
        };
        if Some(key_epoch.key_epoch) != self.key_epoch.checked_add(1) {
            return Err(ProposalRejection::Malformed);
        }
        let anchor = self.live_device(&self.anchor_device_id)?;
        if proposal.device_author() != Some(anchor.device_key) {
            return Err(ProposalRejection::NotAnchor);
        }
        let expected: BTreeSet<KeyId> = self.rotation_recipients(pending).into_keys().collect();
        let sealed: BTreeSet<KeyId> = key_epoch
            .envelopes
            .iter()
            .map(|envelope| envelope.recipient)
            .collect();
        if sealed != expected || sealed.len() != key_epoch.envelopes.len() {
            return Err(ProposalRejection::Malformed);
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
                    let mut demoted = Vec::new();
                    for (device_id, device) in &mut self.devices {
                        if *device_id != handover.new_anchor_device_id && device.removed.is_none() {
                            if let DeviceRole::Admin(key) = device.role {
                                demoted.push(key);
                            }
                            device.role = DeviceRole::Member;
                        }
                    }
                    self.retire(demoted);
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
                        encryption_key: None,
                    },
                );
            }
            // In a space with a key epoch a removal takes effect only with the key epoch the
            // anchor seals right after it, so the removed device gets no envelope to it.
            Entry::Remove(remove) => {
                self.rotate(Some(PendingRemoval::of(&remove.device_id, record)), false);
            }
            Entry::SetRole(SetRole::Device { device_id, role }) => {
                if let Some(device) = self.devices.get_mut(device_id) {
                    let replaced = match device.role {
                        DeviceRole::Admin(key) if device.role != *role => Some(key),
                        _ => None,
                    };
                    device.role = *role;
                    self.retire(replaced);
                }
            }
            Entry::SetRole(SetRole::Recovery { recovery_admin_key }) => {
                if author == self.kit_key {
                    let replaced = self.kit_key;
                    self.kit_key = *recovery_admin_key;
                    self.reset_recovery();
                    self.retire((replaced != self.kit_key).then_some(replaced));
                } else {
                    let replaced = self.recovery_admin_key;
                    self.retire((replaced != *recovery_admin_key).then_some(replaced));
                    self.recovery_admin_key = *recovery_admin_key;
                    self.installer = (*recovery_admin_key != self.kit_key)
                        .then(|| self.anchor_device_id.clone());
                    self.recovery_opened = None;
                }
                self.rotate(None, false);
            }
            Entry::RecoveryKey(recovery) => {
                self.recovery_encryption_key = Some(recovery.encryption_key);
                self.rotate(None, true);
            }
            Entry::KeyEpoch(key_epoch) => {
                if let Some(removal) = self.pending.take().and_then(|pending| pending.removal) {
                    self.apply_removal(removal);
                }
                self.key_epoch = key_epoch.key_epoch;
            }
            Entry::DeviceKey(device_key) => {
                if let Some(device) = self.devices.get_mut(&device_key.device_id) {
                    device.encryption_key = Some(device_key.encryption_key);
                }
            }
            Entry::ServerConfig(config) => self.server = config.server.clone(),
            Entry::ResolveLate(_) | Entry::Yield(_) | Entry::Unknown { .. } => {}
        }
    }

    /// Makes the kit key the recovery key again, dropping an installed one.
    fn reset_recovery(&mut self) {
        let replaced = self.recovery_admin_key;
        self.recovery_admin_key = self.kit_key;
        self.installer = None;
        self.recovery_opened = None;
        self.retire((replaced != self.kit_key).then_some(replaced));
    }

    /// Records admin keys the chain took away without removing a device.
    fn retire(&mut self, keys: impl IntoIterator<Item = AdminPublicKey>) {
        let mut keys = keys.into_iter().peekable();
        if keys.peek().is_some() {
            Arc::make_mut(&mut self.retired_admin_keys).extend(keys);
        }
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

    /// The view of this state. A state alone does not know the holds of a contest, so every
    /// device is unheld here; [`Selection::view`] marks them.
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
            retired_admin_keys: (*self.retired_admin_keys).clone(),
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
                    held: false,
                    encryption_key: device.encryption_key,
                })
                .collect(),
            discovery_key: self.discovery_key,
            previous_discovery_key: self.previous_discovery_key,
            key_epoch: self.key_epoch,
            recovery_encryption_key: self.recovery_encryption_key,
            pending_removal: self
                .pending
                .as_ref()
                .and_then(|pending| pending.removal.as_ref())
                .map(|removal| removal.device_id.clone()),
            server: self.server.clone(),
        }
    }
}

/// Adds the key material `record`, the next entry of a chain, publishes to the chain's key
/// epochs: a key epoch entry opens one, and a device key entry's envelope joins the latest.
fn push_epoch_keys(key_epochs: &mut Vec<EpochKeys>, record: &AuthorizationRecord) {
    match (&record.proposal.proposal.entry, &record.sequencing) {
        (Entry::KeyEpoch(key_epoch), _) => key_epochs.push(EpochKeys {
            epoch: key_epoch.key_epoch,
            revision: record.revision,
            wrapped_previous: key_epoch.wrapped_previous.clone(),
            envelopes: key_epoch.envelopes.clone(),
        }),
        (
            Entry::DeviceKey(_),
            Sequencing::DeviceKey {
                envelope: Some(envelope),
            },
        ) => {
            if let Some(keys) = key_epochs.last_mut() {
                keys.envelopes.push(envelope.clone());
            }
        }
        _ => {}
    }
}

fn kind_text(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Desktop => "desktop",
        DeviceKind::Mobile => "mobile",
        DeviceKind::Server => "server",
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
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT max(revision) FROM sync_auth_selected WHERE space_id = ?")
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
    sqlx::query("DELETE FROM sync_auth_selected WHERE space_id = ?")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    for entry in &selection.path {
        sqlx::query(
            "INSERT INTO sync_auth_selected
                 (space_id, revision, epoch, entry_hash, proposal_hash, envelope_hash)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(space.as_bytes().as_slice())
        .bind(seq_i64(entry.revision)?)
        .bind(i64::from(entry.epoch))
        .bind(entry.entry_hash.as_slice())
        .bind(entry.proposal_hash.as_slice())
        .bind(entry.envelope_hash.as_slice())
        .execute(&mut *conn)
        .await?;
    }
    if let Some(charter) = load_charter(conn, space).await? {
        records::walked(conn, space, &charter.hash()).await?;
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
    write_space_keys(conn, space, &selection).await?;
    for writer in log::writers(conn, space).await? {
        let removal_cutoff = selection
            .state
            .devices
            .get(&writer.device_id)
            .filter(|device| device.removed.is_some())
            .map(|device| device.cutoffs.get(&writer.id).copied().unwrap_or(0));
        // A device the selected chain does not name has no removal cutoff, so its writer is
        // active as a replica storing it fresh holds it, and the unnamed-writer bound applies.
        log::settle_state(conn, space, &writer, removal_cutoff).await?;
    }
    Ok(Some(selection))
}

/// Rewrites `sync_space_keys` from the key epochs of `selection`: each epoch's chain link and
/// the latest envelope to the encryption key the local device published, so a reselection drops
/// what a superseded chain sealed.
pub(crate) async fn write_space_keys(
    conn: &mut SqliteConnection,
    space: SpaceId,
    selection: &Selection,
) -> SyncResult<()> {
    sqlx::query("DELETE FROM sync_space_keys WHERE space_id = ?")
        .bind(space.as_bytes().as_slice())
        .execute(&mut *conn)
        .await?;
    let own_key = exclusion::local_device(conn, space)
        .await?
        .and_then(|device_id| selection.state.devices.get(&device_id)?.encryption_key)
        .map(|key| key.key_id());
    for keys in &selection.key_epochs {
        let own_envelope = own_key.and_then(|id| {
            keys.envelopes
                .iter()
                .rev()
                .find(|envelope| envelope.recipient == id)
        });
        sqlx::query(
            "INSERT INTO sync_space_keys
                 (space_id, epoch, wrapped_previous, own_envelope, sequenced_revision)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(space.as_bytes().as_slice())
        .bind(i64::from(keys.epoch))
        .bind(
            keys.wrapped_previous
                .as_ref()
                .map(|link| link.to_bytes().to_vec()),
        )
        .bind(own_envelope.map(|envelope| envelope.to_bytes().to_vec()))
        .bind(seq_i64(keys.revision)?)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// The key epochs `sync_space_keys` holds for the local device, in epoch order.
async fn stored_space_keys(
    conn: &mut SqliteConnection,
    space: SpaceId,
) -> SyncResult<Vec<StoredSpaceKey>> {
    type Row = (i64, Option<Vec<u8>>, Option<Vec<u8>>, i64);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT epoch, wrapped_previous, own_envelope, sequenced_revision
         FROM sync_space_keys WHERE space_id = ? ORDER BY epoch",
    )
    .bind(space.as_bytes().as_slice())
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|(epoch, wrapped_previous, own_envelope, revision)| {
            Ok(StoredSpaceKey {
                epoch: u32::try_from(epoch).map_err(|_| corrupt("stored key epoch is invalid"))?,
                wrapped_previous: wrapped_previous
                    .map(|bytes| {
                        ChainLink::from_slice(&bytes)
                            .ok_or_else(|| corrupt("stored key chain link does not decode"))
                    })
                    .transpose()?,
                own_envelope: own_envelope
                    .map(|bytes| {
                        KeyEnvelope::from_slice(&bytes)
                            .ok_or_else(|| corrupt("stored key envelope does not decode"))
                    })
                    .transpose()?,
                sequenced_revision: stored_u64(revision, "key epoch revision")?,
            })
        })
        .collect()
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
        "SELECT r.envelope
         FROM sync_devices d
         JOIN sync_auth_selected s
            ON s.space_id = d.space_id AND s.revision = d.removed_revision
         JOIN sync_auth_envelopes r
            ON r.space_id = s.space_id AND r.role = 'record' AND r.envelope_hash = s.envelope_hash
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

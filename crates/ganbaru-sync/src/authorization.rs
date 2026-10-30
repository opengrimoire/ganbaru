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
use std::sync::{Arc, Mutex, PoisonError};

use ganbaru_sync_contracts::bounds::DISCOVERY_KEY_BYTES;
use ganbaru_sync_contracts::{
    AdminPublicKey, AuthorizationRecord, ChainLink, Content, DeviceKind, DevicePublicKey,
    DeviceRole, Digest32, EncryptionPublicKey, Entry, Envelope, KeyEnvelope, KeyEpoch, KeyId,
    Proposal, ProposalRejection, ResolveLate, Seq, Sequencing, Server, SetRole, SignedCharter,
    SignedProposal, SpaceId, SpaceKey, VersionVector, WriterId, sign_device_proposal,
};
use sqlx::{Connection, SqliteConnection};

use crate::apply::{self, Constrained};
use crate::error::{SyncError, SyncResult, corrupt, failpoint};
use crate::log::{self, seq_i64, stored_u64};
use crate::merge::Batch;
use crate::records::{self, Node};
use crate::seal::{SealReport, SealSlot};
use crate::{Engine, LocalWriter, SpaceContext, SpaceKeyHolder, exclusion, sql};

mod contest;
mod contest_view;
mod exchange;

pub use contest::ContestRejection;
pub use contest_view::{
    CandidateKind, ContestAction, ContestCandidate, ContestCause, ContestEntry, ContestInstance,
    ContestObjection, ContestRole, ContestView, ContestYield, ContestsView, KitPoint,
    KitPointPlace, LosingRemoval,
};
pub use exchange::{
    AuthorizationStorage, ForkedWriter, RECORD_CLASSES, RecordAdmission, RecordClass, RecordCursor,
    RecordEnvelope, RecordPage, RecordRole, RecordWants,
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
        // or admin role after a later entry replaced it, is refused.
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

/// One entry of the selected chain, with the stored record envelope of lowest hash that carries
/// it.
#[derive(Debug, Clone)]
struct SelectedEntry {
    revision: u64,
    epoch: u32,
    entry_hash: Digest32,
    proposal_hash: Digest32,
    envelope_hash: Digest32,
}

/// The selected chain: its head state and its entries in revision order.
#[derive(Clone)]
pub(crate) struct Selection {
    pub(crate) state: AuthState,
    path: Vec<SelectedEntry>,
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
    /// The key epochs of the chain in epoch order.
    pub(crate) key_epochs: Vec<EpochKeys>,
}

impl Selection {
    /// This chain extended by `node`, which its head state admits.
    fn extended(mut self, node: &Node) -> Self {
        self.state.advance(&node.record, node.hash);
        push_epoch_keys(&mut self.key_epochs, &node.record);
        if let Some(carrier) = node.carriers.first() {
            self.path.push(SelectedEntry {
                revision: node.record.revision,
                epoch: node.record.epoch,
                entry_hash: node.hash,
                proposal_hash: node.record.proposal.hash(),
                envelope_hash: carrier.envelope_hash,
            });
        }
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

/// Opens the key of `state`'s key epoch with `holder` from any envelope of that epoch in
/// `key_epochs`, the chain `state` heads: `None` before the first key epoch, and
/// [`ProposalRejection::KeyUnavailable`] when the holder opens none, as for an anchor that a
/// handover or the kit made before its own envelope reached it and that holds no recovery key.
fn open_current(
    state: &AuthState,
    key_epochs: &[EpochKeys],
    holder: Option<&dyn SpaceKeyHolder>,
) -> Result<Option<SpaceKey>, ProposalRejection> {
    if state.key_epoch == 0 {
        return Ok(None);
    }
    let holder = holder.ok_or(ProposalRejection::KeyUnavailable)?;
    key_epochs
        .iter()
        .rev()
        .find(|keys| keys.epoch == state.key_epoch)
        .and_then(|keys| {
            keys.envelopes
                .iter()
                .find_map(|envelope| holder.open(&state.space, state.key_epoch, envelope))
        })
        .map(Some)
        .ok_or(ProposalRejection::KeyUnavailable)
}

/// The key epoch entry that completes the rotation `state` waits for, sealed by the anchor
/// with a fresh space key: one envelope to each of [`AuthState::rotation_recipients`], the
/// previous key `current` wrapped under the new one, and the new relay access key. Refuses with
/// [`ProposalRejection::KeyUnavailable`] unless the holder is the anchor's device and holds the
/// encryption key the anchor published, so the anchor can always open what it seals, and
/// unless it opened the current key, so the new epoch never breaks the chain.
fn rotation_record(
    state: &AuthState,
    current: Option<&SpaceKey>,
    holder: Option<&dyn SpaceKeyHolder>,
    now_ms: u64,
) -> SyncResult<Result<(AuthorizationRecord, SpaceKey), ProposalRejection>> {
    let Some(pending) = &state.pending else {
        return Err(corrupt("a key epoch was sealed without a pending rotation"));
    };
    let Some(holder) = holder else {
        return Ok(Err(ProposalRejection::KeyUnavailable));
    };
    let device_key = holder.device_key().public_key();
    let held = state
        .devices
        .get(&state.anchor_device_id)
        .is_some_and(|anchor| {
            anchor.device_key == device_key
                && anchor.encryption_key == Some(holder.encryption_key())
        });
    if !held || (state.key_epoch > 0) != current.is_some() {
        return Ok(Err(ProposalRejection::KeyUnavailable));
    }
    let Some(epoch) = state.key_epoch.checked_add(1) else {
        return Ok(Err(ProposalRejection::Malformed));
    };
    let next = SpaceKey::generate(epoch).map_err(SyncError::Crypto)?;
    let wrapped_previous = current
        .map(|previous| ChainLink::seal(&state.space, &next, previous))
        .transpose()
        .map_err(SyncError::Crypto)?;
    let envelopes = state
        .rotation_recipients(pending)
        .values()
        .map(|recipient| KeyEnvelope::seal(&state.space, &next, recipient))
        .collect::<Result<Vec<_>, _>>()
        .map_err(SyncError::Crypto)?;
    let relay_access_key = next
        .relay_access_public_key(&state.space)
        .map_err(SyncError::Crypto)?;
    let proposal = Proposal {
        space: state.space,
        charter: state.charter,
        author: AdminPublicKey::from_bytes(*device_key.as_bytes()),
        created_at_ms: i64::try_from(now_ms).unwrap_or(i64::MAX),
        entry: Entry::KeyEpoch(KeyEpoch {
            key_epoch: epoch,
            wrapped_previous,
            envelopes,
            relay_access_key,
        }),
    };
    let signed = sign_device_proposal(&proposal, holder.device_key())
        .map_err(|error| corrupt(format!("key epoch does not sign: {error}")))?;
    let record = AuthorizationRecord {
        epoch: state.epoch,
        revision: state.revision + 1,
        previous_entry_hash: state.entry_hash,
        anchor_device_id: state.anchor_device_id.clone(),
        proposal: signed,
        sequencing: Sequencing::Empty,
    };
    Ok(Ok((record, next)))
}

/// Appends the key epoch that the last of `records`, which `state` admits as its parent, makes
/// pending, so the removal or recovery change and its key epoch seal as consecutive revisions
/// in one transaction. `current` is a key epoch already opened or sealed for `state`.
fn follow_with_rotation(
    state: &AuthState,
    records: &mut Vec<AuthorizationRecord>,
    key_epochs: &[EpochKeys],
    current: Option<SpaceKey>,
    holder: Option<&dyn SpaceKeyHolder>,
    now_ms: u64,
) -> SyncResult<Result<(), ProposalRejection>> {
    let Some(last) = records.last() else {
        return Ok(Ok(()));
    };
    let mut next = state.clone();
    next.advance(last, last.hash());
    if next.pending.is_none() {
        return Ok(Ok(()));
    }
    let current = match current {
        Some(key) => Some(key),
        None => match open_current(&next, key_epochs, holder) {
            Ok(key) => key,
            Err(rejection) => return Ok(Err(rejection)),
        },
    };
    let (rotation, _) = match rotation_record(&next, current.as_ref(), holder, now_ms)? {
        Ok(sealed) => sealed,
        Err(rejection) => return Ok(Err(rejection)),
    };
    if let Err(rejection) = next.check(&rotation) {
        return Ok(Err(rejection));
    }
    records.push(rotation);
    Ok(Ok(()))
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
        "SELECT revision FROM sync_auth_selected WHERE space_id = ? AND proposal_hash = ?",
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
        "SELECT epoch, revision FROM sync_auth_selected WHERE space_id = ? AND proposal_hash = ?",
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

/// Readiness revision of a space whose stored chains reach a writer-fork freeze: every
/// revision, kept within the stored integer range.
const UNBOUNDED_REVISION: u64 = i64::MAX.unsigned_abs();

/// The charter and what the record store derives for the walk.
struct Stored {
    charter: SignedCharter,
    /// Every node, see [`records::load`].
    nodes: Vec<Node>,
    /// The device id and key each verified certificate of a writer names.
    certificates: BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>,
    /// Lowest sequence of each writer with provable fork evidence.
    provable: BTreeMap<WriterId, Seq>,
    /// Highest revision of any verified stored record.
    max_revision: u64,
    /// Verified record bodies that are not nodes, see [`records::Loaded::others`]. The walk
    /// never reads them; only the record exchange does.
    others: Vec<Node>,
    /// Whether a node needs a certificate of a carrier that names its anchor device and key.
    /// Only the selection rule's tests, which have no certificates, leave it unset.
    check_binding: bool,
}

impl Stored {
    /// The key epochs of the stored chain from the charter to the node `head`, in epoch order.
    fn key_epochs(&self, head: &Digest32) -> Vec<EpochKeys> {
        let by_hash: BTreeMap<&Digest32, &Node> =
            self.nodes.iter().map(|node| (&node.hash, node)).collect();
        let mut chain = Vec::new();
        let mut at = by_hash.get(head).copied();
        while let Some(node) = at {
            chain.push(&node.record);
            at = by_hash.get(&node.record.previous_entry_hash).copied();
        }
        let mut key_epochs = Vec::new();
        for record in chain.into_iter().rev() {
            push_epoch_keys(&mut key_epochs, record);
        }
        key_epochs
    }

    /// What `space` stores, or `None` without a charter.
    async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Option<Self>> {
        let Some(charter) = load_charter(conn, space).await? else {
            return Ok(None);
        };
        let loaded = records::load(conn, space, &charter.hash()).await?;
        Ok(Some(Self {
            charter,
            nodes: loaded.nodes,
            certificates: loaded.certificates,
            provable: loaded.provable,
            max_revision: loaded.max_revision,
            others: loaded.others,
            check_binding: true,
        }))
    }

    /// The carriers of `node` bound at `state`, its parent: those with a certificate that names
    /// the node's anchor device and the key that device holds at `state`.
    fn bound(&self, node: &Node, state: &AuthState) -> Vec<(WriterId, Seq)> {
        let anchor = &node.record.anchor_device_id;
        let key = state.devices.get(anchor).map(|device| device.device_key);
        node.carriers
            .iter()
            .filter(|carrier| {
                !self.check_binding
                    || self.certificates.get(&carrier.writer).is_some_and(|names| {
                        names
                            .iter()
                            .any(|(device, named)| device == anchor && Some(*named) == key)
                    })
            })
            .map(|carrier| (carrier.writer, carrier.seq))
            .collect()
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
    /// Writers and sequences of the envelopes that carry the entry bound to its anchor device
    /// at the parent, empty for the charter.
    bound: Vec<(WriterId, Seq)>,
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
                author: None,
                handover: None,
                parent_anchor_key: None,
                revocations: Vec::new(),
                yields_to: None,
                removes: None,
                kit_key: root.kit_key,
                bound: Vec::new(),
            }],
            children: vec![Vec::new()],
            enrolled: BTreeSet::new(),
        };
        // Entries name their parent by hash and duplicates are dropped, so the walk visits each
        // entry at most once, and a visit always comes after its parent. Only forks copy the
        // state, so a chain without forks is walked in linear time.
        let mut stack = vec![(root, 0)];
        while let Some((state, visit)) = stack.pop() {
            // A child is valid when its rules hold at the parent and a certificate of one of its
            // carriers binds it to the key its anchor device holds there.
            let mut valid: Vec<(usize, Vec<(WriterId, Seq)>)> = by_parent
                .get(&state.entry_hash)
                .into_iter()
                .flatten()
                .copied()
                .filter(|index| state.check(&stored.nodes[*index].record).is_ok())
                .map(|index| (index, stored.bound(&stored.nodes[index], &state)))
                .filter(|(_, bound)| !bound.is_empty())
                .collect();
            let Some((last, bound)) = valid.pop() else {
                continue;
            };
            for (index, bound) in valid {
                stack.push(walk.visit(visit, index, bound, state.clone()));
            }
            stack.push(walk.visit(visit, last, bound, state));
        }
        walk
    }

    /// Records entry `index`, which `state` admits, as a child of `parent`, and returns the
    /// state after it with its visit.
    fn visit(
        &mut self,
        parent: usize,
        index: usize,
        bound: Vec<(WriterId, Seq)>,
        mut state: AuthState,
    ) -> (AuthState, usize) {
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
            author: Some(record.proposal.proposal.author),
            handover,
            parent_anchor_key,
            revocations,
            yields_to,
            removes,
            kit_key: state.kit_key,
            bound,
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

    /// The freeze node of each writer fork of every instance, by instance root.
    ///
    /// A writer with provable fork evidence that carries a bound entry of an instance below its
    /// first provable sequence forks the instance: its freeze node is the visit of the instance
    /// it carries at the highest such sequence, the common ancestor of several at one sequence.
    /// Every bound entry of an instance is carried by a writer of the instance's anchor, since
    /// its certificate names the anchor device. A writer without such an entry forks nothing.
    fn freeze_nodes(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut highest: BTreeMap<(usize, WriterId), (Seq, usize)> = BTreeMap::new();
        for (index, visit) in self.visits.iter().enumerate() {
            for (writer, seq) in &visit.bound {
                let Some(first) = self.stored.provable.get(writer) else {
                    continue;
                };
                if seq >= first {
                    continue;
                }
                highest
                    .entry((visit.instance, *writer))
                    .and_modify(|(at, node)| {
                        if seq > at {
                            *at = *seq;
                            *node = index;
                        } else if seq == at {
                            *node = self.common_ancestor(*node, index);
                        }
                    })
                    .or_insert((*seq, index));
            }
        }
        let mut nodes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for ((instance, _), (_, node)) in highest {
            nodes.entry(instance).or_default().push(node);
        }
        nodes
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
            key_epochs: Vec::new(),
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
    /// Contested without a winner, or forked by a writer without a candidate: only the visit
    /// and its ancestors are effective.
    FrozenAt(usize),
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
/// of the candidates' parents, lowered to the common ancestor with the freeze node of each writer
/// fork of the instance, and an uncontested one with a writer fork freezes at the common ancestor
/// of its freeze nodes.
///
/// An entry is effective when its parent is, it is not a handover that lost or left a frozen
/// instance, it is not off the path to a freeze point, and in an instance whose winner has a
/// writer fork, it is an ancestor of the winner or of every freeze node. The effective entry of
/// highest rank is the head. Nothing reads the selected chain afterwards to change any of this:
/// the head reports the state its ancestry derives, the freeze point, and the devices held for
/// data. The result depends only on the stored envelope set, not on arrival order, the peer that
/// delivered an envelope, or the operation slots a replica kept.
struct Resolution<'a> {
    walk: Walk<'a>,
    /// Handovers out of each instance, by instance root.
    candidates: BTreeMap<usize, Vec<usize>>,
    /// The winner a pin source names for each instance it passes through, by instance root.
    pinned: BTreeMap<usize, usize>,
    /// Outcome of each instance, by instance root.
    outcomes: BTreeMap<usize, Outcome>,
    /// Whether each visit is effective.
    effective: Vec<bool>,
    /// The effective visit of highest rank.
    best: usize,
    /// Highest revision of any verified stored record, or every revision once a writer fork
    /// freezes an instance.
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
        let forks = walk.freeze_nodes();
        let mut outcomes = BTreeMap::new();
        for root in 0..walk.visits.len() {
            if walk.visits[root].instance != root {
                continue;
            }
            let own = candidates.get(&root).map_or(&[][..], Vec::as_slice);
            let freezes = forks.get(&root).map_or(&[][..], Vec::as_slice);
            outcomes.insert(
                root,
                Self::outcome(&walk, root, own, freezes, &pinned, &yields),
            );
        }
        // Copies of a forked anchor writer may carry entries past its freeze node that no
        // replica walks. An author that applied one before the fork was found must not wait for
        // it, so a writer fork makes every revision reachable. Each operation's device is still
        // checked against the selected chain when it applies.
        let reachable = if forks.is_empty() {
            stored.max_revision
        } else {
            UNBOUNDED_REVISION
        };
        let mut effective = vec![true; walk.visits.len()];
        for index in 1..walk.visits.len() {
            effective[index] =
                effective[walk.parent(index)] && Self::admitted(&walk, index, &outcomes, &forks);
        }
        let best = (0..walk.visits.len())
            .filter(|index| effective[*index])
            .max_by(|a, b| walk.visits[*a].rank.cmp(&walk.visits[*b].rank))
            .unwrap_or(0);
        Self {
            walk,
            candidates,
            pinned,
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
        freezes: &[usize],
        pinned: &BTreeMap<usize, usize>,
        yields: &[(AdminPublicKey, Digest32)],
    ) -> Outcome {
        if let Some(winner) = pinned.get(&root) {
            return Outcome::Won(*winner);
        }
        let Some(first) = candidates.first() else {
            return freezes
                .iter()
                .copied()
                .reduce(|a, b| walk.common_ancestor(a, b))
                .map_or(Outcome::Open, Outcome::FrozenAt);
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
        let point = candidates
            .iter()
            .map(|candidate| walk.parent(*candidate))
            .chain(freezes.iter().copied())
            .reduce(|a, b| walk.common_ancestor(a, b))
            .unwrap_or(root);
        Outcome::FrozenAt(point)
    }

    /// Whether visit `index`, whose parent is effective, is effective itself.
    fn admitted(
        walk: &Walk<'_>,
        index: usize,
        outcomes: &BTreeMap<usize, Outcome>,
        forks: &BTreeMap<usize, Vec<usize>>,
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
            Some(Outcome::Won(winner)) => {
                walk.is_ancestor(index, *winner)
                    || forks.get(&visit.instance).is_none_or(|freezes| {
                        freezes
                            .iter()
                            .all(|freeze| walk.is_ancestor(index, *freeze))
                    })
            }
            Some(Outcome::Open) | None => true,
        }
    }

    /// Revision past which the head's instance is frozen, if it is.
    fn frozen_at(&self) -> Option<u64> {
        match self.outcomes.get(&self.walk.visits[self.best].instance) {
            Some(Outcome::FrozenAt(point)) => Some(self.walk.visits[*point].revision),
            _ => None,
        }
    }

    /// Devices a stored entry that is not effective removes after the freeze point of the
    /// head's instance, while it is frozen.
    fn held(&self) -> BTreeSet<String> {
        let instance = self.walk.visits[self.best].instance;
        match self.outcomes.get(&instance) {
            Some(Outcome::FrozenAt(point)) => self.held_under(*point),
            _ => BTreeSet::new(),
        }
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

/// What the enrollments of every stored chain depend on: the space, its charter, and the
/// envelopes of its record store with whether each is verified. The charter keeps a reset space
/// that a new charter founds, or another vault with the same space, from reading a stale answer.
#[derive(Debug, PartialEq, Eq)]
struct EnrolledKey {
    space: SpaceId,
    charter: Option<Digest32>,
    envelopes: Vec<(Vec<u8>, i64)>,
}

impl EnrolledKey {
    async fn load(conn: &mut SqliteConnection, space: SpaceId) -> SyncResult<Self> {
        let envelopes = sqlx::query_as(
            "SELECT envelope_hash, verified FROM sync_auth_envelopes
             WHERE space_id = ? ORDER BY envelope_hash, role",
        )
        .bind(space.as_bytes().as_slice())
        .fetch_all(&mut *conn)
        .await?;
        Ok(Self {
            space,
            charter: load_charter(conn, space)
                .await?
                .map(|charter| charter.hash()),
            envelopes,
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
    /// The authorization state after the entry, derived from verified stored records only, so a
    /// handover can check what it continues from, such as its key epoch and the encryption keys
    /// published there, rather than the selected head.
    pub view: AuthorizationView,
}

impl HandoverPoint {
    fn of(state: &AuthState, by_kit: bool) -> Self {
        Self {
            epoch: state.epoch,
            revision: state.revision,
            entry_hash: state.entry_hash,
            anchor_device_id: state.anchor_device_id.clone(),
            by_kit,
            view: state.view(),
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

/// What the stored `records` derive from `charter`, each carried by its writer at its sequence,
/// for tests of the selection rule. `forks` names the first provable fork sequence of forked
/// writers. With `certificates`, the device id and key each certificate of a writer names, a
/// carrier is bound as in the record store; without, every carrier counts as bound. A record
/// of a forked writer at or past its fork sequence is a node only as an ancestor of another,
/// as in a record store whose horizon equals the provable sequence.
#[cfg(test)]
pub(crate) fn select_records(
    charter: &SignedCharter,
    forks: BTreeMap<WriterId, Seq>,
    certificates: Option<BTreeMap<WriterId, Vec<(String, DevicePublicKey)>>>,
    records: &[(WriterId, Seq, AuthorizationRecord)],
) -> SelectedHead {
    let mut bodies: BTreeMap<Digest32, Node> = BTreeMap::new();
    for (writer, seq, record) in records {
        let hash = record.hash();
        bodies
            .entry(hash)
            .or_insert_with(|| Node {
                hash,
                record: record.clone(),
                carriers: Vec::new(),
            })
            .carriers
            .push(records::Carrier {
                writer: *writer,
                seq: *seq,
                envelope_hash: ganbaru_sync_contracts::signing::domain_digest(
                    b"test carrier",
                    &[writer.as_bytes(), &seq.to_be_bytes()],
                ),
            });
    }
    let charter_hash = charter.hash();
    let anchors: Vec<Digest32> = bodies
        .values()
        .filter(|node| {
            node.carriers.iter().any(|carrier| {
                forks
                    .get(&carrier.writer)
                    .is_none_or(|fork| carrier.seq < *fork)
            })
        })
        .map(|node| node.hash)
        .collect();
    let mut included = BTreeSet::new();
    for hash in anchors {
        let mut path = Vec::new();
        let mut at = hash;
        while at != charter_hash && !included.contains(&at) {
            let Some(node) = bodies.get(&at) else {
                path.clear();
                break;
            };
            path.push(at);
            at = node.record.previous_entry_hash;
        }
        included.extend(path);
    }
    let max_revision = bodies
        .values()
        .map(|node| node.record.revision)
        .max()
        .unwrap_or(0);
    let mut nodes: Vec<Node> = bodies
        .into_values()
        .filter(|node| included.contains(&node.hash))
        .collect();
    nodes.sort_by_key(|node| (node.record.revision, node.hash));
    let stored = Stored {
        charter: charter.clone(),
        nodes,
        check_binding: certificates.is_some(),
        certificates: certificates.unwrap_or_default(),
        provable: forks,
        max_revision,
        others: Vec::new(),
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

    /// The key epochs of the selected chain for the local device, in epoch order, after
    /// rebuilding `sync_space_keys` from the stored record set, so a snapshot copy, which
    /// carries none, gets them back at service start. The service opens the newest envelope it
    /// holds and walks the chain links back from it.
    pub async fn space_keys(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> SyncResult<Vec<StoredSpaceKey>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        if let Some(selection) = select(&mut tx, ctx.space).await? {
            write_space_keys(&mut tx, ctx.space, &selection).await?;
        }
        let keys = stored_space_keys(&mut tx, ctx.space).await?;
        tx.commit().await?;
        Ok(keys)
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

    /// Seals the key epoch the selected head waits for when the local writer is its anchor, as
    /// after a handover that continued past a removal or recovery change. Sequencing seals it
    /// too before the next proposal; this seals it without one. Returns its revision, `None`
    /// when no rotation is pending, or why the anchor cannot seal it, such as
    /// [`ProposalRejection::KeyUnavailable`] while it opens no envelope of the current epoch.
    pub async fn complete_rotation(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<Result<Option<u64>, ProposalRejection>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let Some(selection) = select(&mut tx, ctx.space).await? else {
            return Err(SyncError::ResetRequired("the space has no charter"));
        };
        let state = &selection.state;
        if state.pending.is_none() {
            return Ok(Ok(None));
        }
        if writer.certificate.certificate.device_id != state.anchor_device_id {
            return Ok(Err(ProposalRejection::NotAnchor));
        }
        if selection.handed_over {
            return Ok(Err(ProposalRejection::HandoverConflict));
        }
        let current = match open_current(state, &selection.key_epochs, writer.keys) {
            Ok(key) => key,
            Err(rejection) => return Ok(Err(rejection)),
        };
        let (record, _) = match rotation_record(state, current.as_ref(), writer.keys, now_ms)? {
            Ok(sealed) => sealed,
            Err(rejection) => return Ok(Err(rejection)),
        };
        if let Err(rejection) = state.check(&record) {
            return Ok(Err(rejection));
        }
        let revision = record.revision;
        self.seal_records(&mut tx, ctx, state.charter, vec![record], writer, now_ms)
            .await?;
        failpoint!("complete_rotation.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(Some(revision)))
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

    /// The kit point at `entry` for a handover the kit key `signer` signs for the desktop
    /// `target`, when `entry` is one of the kit points, see [`ContestAction::UseKit`]. `None`
    /// without a charter or when `entry` is not a kit point.
    pub async fn kit_handover_point(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        signer: &AdminPublicKey,
        target: &str,
        entry: &ContestEntry,
    ) -> SyncResult<Option<HandoverPoint>> {
        log::ensure_space(conn, ctx.space).await?;
        Ok(
            kit_point(conn, ctx.space, signer, target, entry.revision, &entry.hash)
                .await?
                .map(|state| HandoverPoint::of(&state, true)),
        )
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
        let key_epochs = selection.key_epochs;
        let mut state = selection.state;
        let holder = writer.keys;
        let local_device = &writer.certificate.certificate.device_id;
        let anchor_ordering = matches!(ordering, Ordering::Anchor { .. });
        // Records sealed before the proposal's: the key epoch a handover left pending.
        let mut records = Vec::new();
        // The key of the current key epoch, once opened or sealed here.
        let mut current: Option<SpaceKey> = None;
        if anchor_ordering && *local_device == state.anchor_device_id && !handed_over {
            if matches!(proposal.proposal.entry, Entry::KeyEpoch(_)) {
                return Ok(Err(ProposalRejection::AnchorOnly));
            }
            // The anchor's own device key goes first, and the key epoch follows it.
            if state.pending.is_some() && !state.admits_while_pending(&proposal.proposal.entry) {
                let opened = match open_current(&state, &key_epochs, holder) {
                    Ok(key) => key,
                    Err(rejection) => return Ok(Err(rejection)),
                };
                let (rotation, next) =
                    match rotation_record(&state, opened.as_ref(), holder, now_ms)? {
                        Ok(sealed) => sealed,
                        Err(rejection) => return Ok(Err(rejection)),
                    };
                if let Err(rejection) = state.check(&rotation) {
                    return Ok(Err(rejection));
                }
                state.advance(&rotation, rotation.hash());
                records.push(rotation);
                current = Some(next);
            }
        }
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
                    Entry::RecoveryKey(_) | Entry::ServerConfig(_) => Sequencing::Empty,
                    // The anchor seals the current key to a device's new encryption key.
                    Entry::DeviceKey(device_key) => {
                        if current.is_none() {
                            current = match open_current(&state, &key_epochs, holder) {
                                Ok(key) => key,
                                Err(rejection) => return Ok(Err(rejection)),
                            };
                        }
                        let envelope = current
                            .as_ref()
                            .map(|key| {
                                KeyEnvelope::seal(&state.space, key, &device_key.encryption_key)
                            })
                            .transpose()
                            .map_err(SyncError::Crypto)?;
                        Sequencing::DeviceKey { envelope }
                    }
                    Entry::KeyEpoch(_) => return Ok(Err(ProposalRejection::AnchorOnly)),
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
        let revision = record.revision;
        records.push(record);
        if anchor_ordering
            && let Err(rejection) =
                follow_with_rotation(&state, &mut records, &key_epochs, current, holder, now_ms)?
        {
            return Ok(Err(rejection));
        }
        self.seal_records(&mut tx, ctx, state.charter, records, writer, now_ms)
            .await?;
        failpoint!("sequence.before_commit");
        sql::set_applying(&mut tx, false).await?;
        tx.commit().await?;
        Ok(Ok(revision))
    }

    /// Seals `records`, consecutive revisions that the rules at each one's parent admit, as the
    /// next operations of the local writer, rederives the selection, and applies what the sealed
    /// operations make ready. The reservation records the first, so a rolled back seal of the
    /// pair stays ahead by one. The caller commits.
    pub(super) async fn seal_records(
        &self,
        tx: &mut SqliteConnection,
        ctx: &SpaceContext,
        charter: Digest32,
        records: Vec<AuthorizationRecord>,
        writer: &mut LocalWriter<'_>,
        now_ms: u64,
    ) -> SyncResult<()> {
        let Some(first) = records.first() else {
            return Ok(());
        };
        let reserved = (first.epoch, first.revision, first.hash());
        sql::begin_engine_write(&mut *tx).await?;
        let mut batch = Batch::load(&mut *tx, ctx.space).await?;
        exclusion::note_local_device(
            self,
            &mut *tx,
            ctx,
            &mut batch,
            &writer.certificate.certificate.device_id,
        )
        .await?;
        let mut slot = SealSlot::new(Some(writer));
        let mut report = SealReport::default();
        let (sealer, writer) = slot
            .open(self, &mut *tx, ctx, &mut batch, now_ms)
            .await?
            .ok_or_else(|| corrupt("sealer slot has no writer"))?;
        writer
            .reservation
            .reserve_entry(charter, reserved.0, reserved.1, reserved.2)
            .map_err(SyncError::Reservation)?;
        for record in records {
            sealer
                .seal_authorization(
                    self,
                    &mut *tx,
                    ctx,
                    &mut batch,
                    writer,
                    record,
                    now_ms,
                    &mut report,
                )
                .await?;
            failpoint!("seal_records.between");
        }
        if let Some(selection) = rederive(&mut *tx, ctx.space).await? {
            exclusion::settle(self, &mut *tx, ctx, &mut batch, &selection).await?;
        }
        apply::finish_sealed(
            self,
            &mut *tx,
            ctx,
            &mut batch,
            &mut slot,
            Constrained::Seal,
            now_ms,
            &mut report,
        )
        .await?;
        Ok(())
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

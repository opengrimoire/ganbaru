//! Authorization log entries, admin-signed proposals, and anchor rejection reasons.
//!
//! The authorization log of a space starts at the charter (revision 0, epoch 0). Each later
//! revision is one kind 4 operation sealed by the anchor's writer; its body is an authorization
//! record:
//!
//! epoch (4) | revision (8) | previous entry hash (32) | anchor device id length (1) and device
//! id | signed proposal | sequencing length (4) and sequencing.
//!
//! The previous entry hash of revision 1 is the charter hash, and of every later revision the
//! entry hash of the revision before it. The entry hash is the authorization entry digest of the
//! record encoding, so records chain independently of the writer that sealed them, which lets the
//! chain continue across an anchor handover.
//!
//! A signed proposal is: version (1) | space id (16) | charter hash (32) | author admin public key
//! (32) | created at milliseconds (8) | entry kind (1) | entry length (4) and entry | admin
//! signature (64) over the proposal domain and everything before it. Its hash, the proposal hash digest of the unsigned
//! encoding, makes resubmission idempotent. Decoding verifies the signature against the embedded
//! author key; whether that key is an admin of the space at the record's revision is the log's
//! question.
//!
//! Entries are extensible: an entry kind this build does not know decodes as
//! [`Entry::Unknown`] with its bytes kept, so it can be stored, forwarded, and chained. Known
//! entries, big endian:
//!
//! - Enroll (0): device id length (1) and id | device public key (32) | TLS fingerprint (32) |
//!   label length (1) and label | device kind (1) | role (1) and, for an admin, admin public key
//!   (32).
//! - Remove (1): device id length (1) and id.
//! - Set role (2): target (1), then `0` device id length (1) and id | role (1) and, for an admin,
//!   admin public key (32); or `1` replacement recovery admin public key (32).
//! - Resolve late (3): removal revision (8) | row count (2) and per row table id (2) | row key
//!   length (1) and key | resolution (1).
//! - Handover (4): new anchor device id length (1) and id | new epoch (4) | continues from
//!   revision (8) | continues from entry hash (32).
//! - Yield (5): hash of the handover the author gives way to (32).
//!
//! Sequencing is what only the anchor knows when it orders a proposal, outside the admin
//! signature: for a removal, the stored sequence cutoff of each writer of the removed device and
//! a fresh discovery key; for a handover, sealed by the new anchor as the first entry of its
//! epoch, the old anchor's stored vector when known. Other kinds have empty sequencing, and an
//! unknown kind keeps its sequencing bytes.

use crate::bounds::{
    DISCOVERY_KEY_BYTES, MAX_DEVICE_LABEL_BYTES, MAX_LATE_RESOLUTIONS, MAX_OPERATION_BYTES,
    MAX_REMOVAL_CUTOFFS, MAX_ROW_KEY_BYTES, MAX_WRITERS_PER_SPACE, TLS_FINGERPRINT_BYTES,
};
use crate::certificate::{CertificateError, read_device_id, validate_device_id};
use crate::codec::{CodecError, Reader, Writer};
use crate::ids::{RowKey, Seq, SpaceId, TableId, WriterId};
use crate::keys::{
    AdminKeyPair, AdminPublicKey, DevicePublicKey, PUBLIC_KEY_BYTES, SIGNATURE_BYTES,
};
use crate::signing::{
    AUTHORIZATION_ENTRY_DOMAIN, DIGEST_BYTES, Digest32, PROPOSAL_DOMAIN, PROPOSAL_HASH_DOMAIN,
    domain_digest,
};
use crate::vector::VersionVector;
use ganbaru_contacts::validate_display_text;
use std::fmt;

const PROPOSAL_VERSION: u8 = 2;

const ENTRY_ENROLL: u8 = 0;
const ENTRY_REMOVE: u8 = 1;
const ENTRY_SET_ROLE: u8 = 2;
const ENTRY_RESOLVE_LATE: u8 = 3;
const ENTRY_HANDOVER: u8 = 4;
const ENTRY_YIELD: u8 = 5;

const ROLE_MEMBER: u8 = 0;
const ROLE_ADMIN: u8 = 1;
const SET_ROLE_DEVICE: u8 = 0;
const SET_ROLE_RECOVERY: u8 = 1;
const VECTOR_ABSENT: u8 = 0;
const VECTOR_PRESENT: u8 = 1;

/// An authorization record or proposal that cannot be signed, decoded, or validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationError {
    /// The encoding is malformed or exceeds a bound.
    Codec(CodecError),
    /// The proposal version is unknown.
    Version,
    /// A device id is not a valid identifier.
    DeviceId,
    /// A device label is not valid display text.
    Label,
    /// The signing key is not the proposal author.
    WrongAuthor,
    /// The admin signature does not verify.
    Signature,
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "authorization entry: {error}"),
            Self::Version => formatter.write_str("authorization proposal version is unsupported"),
            Self::DeviceId => formatter.write_str("authorization entry device id is invalid"),
            Self::Label => formatter.write_str("authorization entry device label is invalid"),
            Self::WrongAuthor => formatter.write_str("authorization proposal author mismatch"),
            Self::Signature => {
                formatter.write_str("authorization proposal signature does not verify")
            }
        }
    }
}

impl std::error::Error for AuthorizationError {}

impl From<CodecError> for AuthorizationError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<CertificateError> for AuthorizationError {
    fn from(error: CertificateError) -> Self {
        match error {
            CertificateError::Codec(error) => Self::Codec(error),
            _ => Self::DeviceId,
        }
    }
}

/// The form factor an enrollment records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// A desktop installation.
    Desktop,
    /// A mobile installation.
    Mobile,
}

impl DeviceKind {
    /// Wire value.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Desktop => 0,
            Self::Mobile => 1,
        }
    }

    /// Kind for a wire value.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Desktop),
            1 => Some(Self::Mobile),
            _ => None,
        }
    }
}

/// A device's role in a space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceRole {
    /// Writes data; cannot propose authorization changes.
    Member,
    /// Writes data and proposes authorization changes with the admin key it holds.
    Admin(AdminPublicKey),
}

impl DeviceRole {
    fn encode_into(&self, writer: &mut Writer) {
        match self {
            Self::Member => writer.u8(ROLE_MEMBER),
            Self::Admin(key) => {
                writer.u8(ROLE_ADMIN);
                writer.raw(key.as_bytes());
            }
        }
    }

    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, CodecError> {
        match reader.u8()? {
            ROLE_MEMBER => Ok(Self::Member),
            ROLE_ADMIN => Ok(Self::Admin(AdminPublicKey::from_bytes(
                reader.array::<PUBLIC_KEY_BYTES>()?,
            ))),
            _ => Err(CodecError::Malformed("device role")),
        }
    }
}

/// Admits a device to the space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enroll {
    /// Handoff device id.
    pub device_id: String,
    /// Device key that signs the device's writer certificates.
    pub device_key: DevicePublicKey,
    /// TLS certificate fingerprint the device presents.
    pub tls_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
    /// Label shown to other devices.
    pub label: String,
    /// Form factor.
    pub device_kind: DeviceKind,
    /// Role in the space.
    pub role: DeviceRole,
}

/// Removes a device from the space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remove {
    /// Handoff device id of the removed device.
    pub device_id: String,
}

/// Changes a device role or replaces the recovery admin key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetRole {
    /// Gives an enrolled device a new role.
    Device {
        /// Handoff device id.
        device_id: String,
        /// New role.
        role: DeviceRole,
    },
    /// Replaces the recovery admin key, for example after a new recovery kit.
    Recovery {
        /// The replacement recovery admin key.
        recovery_admin_key: AdminPublicKey,
    },
}

/// What happens to a row a removed device changed after its cutoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LateResolution {
    /// The late change is applied.
    Restore,
    /// The late change is dropped.
    Discard,
}

impl LateResolution {
    /// Wire value.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Restore => 0,
            Self::Discard => 1,
        }
    }

    /// Resolution for a wire value.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Restore),
            1 => Some(Self::Discard),
            _ => None,
        }
    }
}

/// The decision for one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateRow {
    /// Replicated table.
    pub table: TableId,
    /// Row primary key.
    pub row: RowKey,
    /// Decision.
    pub resolution: LateResolution,
}

/// Decides the late changes of a removal, per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveLate {
    /// Revision of the removal whose late changes are decided.
    pub removal_revision: u64,
    /// Decisions, ascending by table and row, one per row.
    pub rows: Vec<LateRow>,
}

/// Moves the anchor role to another admin desktop and starts a new epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    /// Device id of the new anchor.
    pub new_anchor_device_id: String,
    /// The new epoch: the previous epoch plus one.
    pub epoch: u32,
    /// Last revision of the previous epoch the new anchor continues from.
    pub continues_from_revision: u64,
    /// Entry hash of that revision.
    pub continues_from_hash: Digest32,
}

/// Withdraws the author's other handovers out of one epoch instance, and the anchor's objections
/// to the named handover, in favor of the named handover.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Yield {
    /// Entry hash of the handover the author gives way to.
    pub handover_hash: Digest32,
}

/// One authorization change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Admits a device.
    Enroll(Enroll),
    /// Removes a device.
    Remove(Remove),
    /// Changes a role.
    SetRole(SetRole),
    /// Decides late changes of a removal.
    ResolveLate(ResolveLate),
    /// Moves the anchor role.
    Handover(Handover),
    /// Gives way to a handover that contests the author's own handover or objection.
    Yield(Yield),
    /// An entry kind this build does not know, kept byte for byte.
    Unknown {
        /// Wire kind.
        kind: u8,
        /// Entry encoding.
        bytes: Vec<u8>,
    },
}

impl Entry {
    /// Wire kind.
    pub fn kind(&self) -> u8 {
        match self {
            Self::Enroll(_) => ENTRY_ENROLL,
            Self::Remove(_) => ENTRY_REMOVE,
            Self::SetRole(_) => ENTRY_SET_ROLE,
            Self::ResolveLate(_) => ENTRY_RESOLVE_LATE,
            Self::Handover(_) => ENTRY_HANDOVER,
            Self::Yield(_) => ENTRY_YIELD,
            Self::Unknown { kind, .. } => *kind,
        }
    }

    fn validate(&self) -> Result<(), AuthorizationError> {
        match self {
            Self::Enroll(enroll) => {
                validate_device_id(&enroll.device_id)?;
                validate_display_text(&enroll.label, MAX_DEVICE_LABEL_BYTES)
                    .map_err(|_| AuthorizationError::Label)?;
            }
            Self::Remove(remove) => validate_device_id(&remove.device_id)?,
            Self::SetRole(SetRole::Device { device_id, .. }) => validate_device_id(device_id)?,
            Self::SetRole(SetRole::Recovery { .. }) => {}
            Self::ResolveLate(resolve) => {
                if resolve.removal_revision == 0
                    || resolve.rows.is_empty()
                    || resolve.rows.len() > MAX_LATE_RESOLUTIONS
                {
                    return Err(CodecError::Bounds("late resolutions").into());
                }
                let ascending = resolve.rows.windows(2).all(|pair| {
                    (pair[0].table, pair[0].row.as_str()) < (pair[1].table, pair[1].row.as_str())
                });
                if !ascending {
                    return Err(CodecError::Malformed("late resolution order").into());
                }
            }
            Self::Handover(handover) => {
                validate_device_id(&handover.new_anchor_device_id)?;
                if handover.epoch == 0 || handover.continues_from_revision == 0 {
                    return Err(CodecError::Malformed("handover").into());
                }
            }
            Self::Yield(_) => {}
            Self::Unknown { kind, .. } => {
                if *kind <= ENTRY_YIELD {
                    return Err(CodecError::Malformed("entry kind").into());
                }
            }
        }
        Ok(())
    }

    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        match self {
            Self::Enroll(enroll) => {
                writer.bytes_u8(enroll.device_id.as_bytes());
                writer.raw(enroll.device_key.as_bytes());
                writer.raw(&enroll.tls_fingerprint);
                writer.bytes_u8(enroll.label.as_bytes());
                writer.u8(enroll.device_kind.as_u8());
                enroll.role.encode_into(&mut writer);
            }
            Self::Remove(remove) => writer.bytes_u8(remove.device_id.as_bytes()),
            Self::SetRole(SetRole::Device { device_id, role }) => {
                writer.u8(SET_ROLE_DEVICE);
                writer.bytes_u8(device_id.as_bytes());
                role.encode_into(&mut writer);
            }
            Self::SetRole(SetRole::Recovery { recovery_admin_key }) => {
                writer.u8(SET_ROLE_RECOVERY);
                writer.raw(recovery_admin_key.as_bytes());
            }
            Self::ResolveLate(resolve) => {
                writer.u64(resolve.removal_revision);
                writer.u16(resolve.rows.len() as u16);
                for row in &resolve.rows {
                    writer.u16(row.table.0);
                    writer.bytes_u8(row.row.as_str().as_bytes());
                    writer.u8(row.resolution.as_u8());
                }
            }
            Self::Handover(handover) => {
                writer.bytes_u8(handover.new_anchor_device_id.as_bytes());
                writer.u32(handover.epoch);
                writer.u64(handover.continues_from_revision);
                writer.raw(&handover.continues_from_hash);
            }
            Self::Yield(give_way) => writer.raw(&give_way.handover_hash),
            Self::Unknown { bytes, .. } => writer.raw(bytes),
        }
        writer.into_bytes()
    }

    fn decode(kind: u8, bytes: &[u8]) -> Result<Self, AuthorizationError> {
        let mut reader = Reader::new(bytes);
        let entry = match kind {
            ENTRY_ENROLL => {
                let device_id = read_device_id(&mut reader)?;
                let device_key = DevicePublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
                let tls_fingerprint = reader.array()?;
                let label = reader.bytes_u8(MAX_DEVICE_LABEL_BYTES, "device label")?;
                let label = std::str::from_utf8(label)
                    .map_err(|_| AuthorizationError::Label)?
                    .to_string();
                let device_kind = DeviceKind::from_u8(reader.u8()?)
                    .ok_or(CodecError::Malformed("device kind"))?;
                let role = DeviceRole::decode_from(&mut reader)?;
                Self::Enroll(Enroll {
                    device_id,
                    device_key,
                    tls_fingerprint,
                    label,
                    device_kind,
                    role,
                })
            }
            ENTRY_REMOVE => Self::Remove(Remove {
                device_id: read_device_id(&mut reader)?,
            }),
            ENTRY_SET_ROLE => match reader.u8()? {
                SET_ROLE_DEVICE => {
                    let device_id = read_device_id(&mut reader)?;
                    let role = DeviceRole::decode_from(&mut reader)?;
                    Self::SetRole(SetRole::Device { device_id, role })
                }
                SET_ROLE_RECOVERY => Self::SetRole(SetRole::Recovery {
                    recovery_admin_key: AdminPublicKey::from_bytes(
                        reader.array::<PUBLIC_KEY_BYTES>()?,
                    ),
                }),
                _ => return Err(CodecError::Malformed("set role target").into()),
            },
            ENTRY_RESOLVE_LATE => {
                let removal_revision = reader.u64()?;
                let count = usize::from(reader.u16()?);
                if count > MAX_LATE_RESOLUTIONS {
                    return Err(CodecError::Bounds("late resolutions").into());
                }
                let mut rows = Vec::with_capacity(count);
                for _ in 0..count {
                    let table = TableId(reader.u16()?);
                    let key = reader.bytes_u8(MAX_ROW_KEY_BYTES, "row key")?;
                    let key =
                        std::str::from_utf8(key).map_err(|_| CodecError::Malformed("row key"))?;
                    let row = RowKey::new(key).map_err(|_| CodecError::Malformed("row key"))?;
                    let resolution = LateResolution::from_u8(reader.u8()?)
                        .ok_or(CodecError::Malformed("late resolution"))?;
                    rows.push(LateRow {
                        table,
                        row,
                        resolution,
                    });
                }
                Self::ResolveLate(ResolveLate {
                    removal_revision,
                    rows,
                })
            }
            ENTRY_HANDOVER => Self::Handover(Handover {
                new_anchor_device_id: read_device_id(&mut reader)?,
                epoch: reader.u32()?,
                continues_from_revision: reader.u64()?,
                continues_from_hash: reader.array::<DIGEST_BYTES>()?,
            }),
            ENTRY_YIELD => Self::Yield(Yield {
                handover_hash: reader.array::<DIGEST_BYTES>()?,
            }),
            _ => {
                let bytes = reader.take(reader.remaining())?.to_vec();
                Self::Unknown { kind, bytes }
            }
        };
        reader.finish()?;
        entry.validate()?;
        Ok(entry)
    }
}

/// An authorization change an admin asks the anchor to order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Space the change applies to.
    pub space: SpaceId,
    /// Hash of the charter the author's chain starts from. An anchor refuses a proposal made
    /// for another charter, so a proposal signed before Reset sync state cannot be replayed into
    /// the space founded after it.
    pub charter: Digest32,
    /// Admin key of the author.
    pub author: AdminPublicKey,
    /// Creation time in Unix milliseconds; distinguishes deliberate re-proposals.
    pub created_at_ms: i64,
    /// The change.
    pub entry: Entry,
}

/// A proposal with its admin signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedProposal {
    /// Proposal contents.
    pub proposal: Proposal,
    /// Admin signature over the proposal domain and the unsigned encoding.
    pub signature: [u8; SIGNATURE_BYTES],
}

fn encode_unsigned_proposal(proposal: &Proposal, writer: &mut Writer) {
    writer.u8(PROPOSAL_VERSION);
    writer.raw(proposal.space.as_bytes());
    writer.raw(&proposal.charter);
    writer.raw(proposal.author.as_bytes());
    writer.i64(proposal.created_at_ms);
    writer.u8(proposal.entry.kind());
    writer.bytes_u32(&proposal.entry.encode());
}

/// Signs a proposal with the author's admin key.
pub fn sign_proposal(
    proposal: &Proposal,
    admin: &AdminKeyPair,
) -> Result<SignedProposal, AuthorizationError> {
    proposal.entry.validate()?;
    if admin.public_key() != proposal.author {
        return Err(AuthorizationError::WrongAuthor);
    }
    let mut writer = Writer::default();
    encode_unsigned_proposal(proposal, &mut writer);
    let signature = admin.sign(PROPOSAL_DOMAIN, writer.as_bytes());
    Ok(SignedProposal {
        proposal: proposal.clone(),
        signature,
    })
}

impl SignedProposal {
    /// Appends the canonical encoding.
    pub fn encode_into(&self, writer: &mut Writer) {
        encode_unsigned_proposal(&self.proposal, writer);
        writer.raw(&self.signature);
    }

    /// Canonical encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        self.encode_into(&mut writer);
        writer.into_bytes()
    }

    /// Reads one proposal and verifies its signature against the embedded author key.
    pub fn decode_from(reader: &mut Reader<'_>) -> Result<Self, AuthorizationError> {
        if reader.u8()? != PROPOSAL_VERSION {
            return Err(AuthorizationError::Version);
        }
        let space = SpaceId::from_bytes(reader.array()?);
        let charter = reader.array::<DIGEST_BYTES>()?;
        let author = AdminPublicKey::from_bytes(reader.array::<PUBLIC_KEY_BYTES>()?);
        let created_at_ms = reader.i64()?;
        let kind = reader.u8()?;
        let entry = Entry::decode(kind, reader.bytes_u32(MAX_OPERATION_BYTES, "entry")?)?;
        let signature = reader.array::<SIGNATURE_BYTES>()?;
        let proposal = Proposal {
            space,
            charter,
            author,
            created_at_ms,
            entry,
        };
        let mut unsigned = Writer::default();
        encode_unsigned_proposal(&proposal, &mut unsigned);
        if !proposal
            .author
            .verify(PROPOSAL_DOMAIN, unsigned.as_bytes(), &signature)
        {
            return Err(AuthorizationError::Signature);
        }
        Ok(Self {
            proposal,
            signature,
        })
    }

    /// Decodes a complete proposal encoding and verifies its signature.
    pub fn decode(bytes: &[u8]) -> Result<Self, AuthorizationError> {
        let mut reader = Reader::new(bytes);
        let proposal = Self::decode_from(&mut reader)?;
        reader.finish()?;
        Ok(proposal)
    }

    /// Proposal hash: the idempotence key under which the anchor orders a proposal once.
    pub fn hash(&self) -> Digest32 {
        let mut writer = Writer::default();
        encode_unsigned_proposal(&self.proposal, &mut writer);
        domain_digest(PROPOSAL_HASH_DOMAIN, &[writer.as_bytes()])
    }
}

/// What the anchor adds to a proposal when it orders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sequencing {
    /// Nothing: enrollments, role changes, and late resolutions.
    Empty,
    /// The removal's per-writer cutoffs and the rotated discovery key.
    Removal {
        /// Stored sequence of each writer of the removed device, ascending by writer, nonzero.
        /// A writer the anchor has never stored is absent and keeps nothing.
        cutoffs: Vec<(WriterId, Seq)>,
        /// Discovery key that replaces the current one.
        discovery_key: [u8; DISCOVERY_KEY_BYTES],
    },
    /// The old anchor's stored vector, when the new anchor knows it.
    Handover {
        /// Stored vector of the old anchor.
        old_anchor_vector: Option<VersionVector>,
    },
    /// Sequencing of an unknown entry kind, kept byte for byte.
    Opaque(Vec<u8>),
}

impl Sequencing {
    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        match self {
            Self::Empty => {}
            Self::Removal {
                cutoffs,
                discovery_key,
            } => {
                writer.u16(cutoffs.len() as u16);
                for (writer_id, seq) in cutoffs {
                    writer.raw(writer_id.as_bytes());
                    writer.u64(*seq);
                }
                writer.raw(discovery_key);
            }
            Self::Handover { old_anchor_vector } => match old_anchor_vector {
                Some(vector) => {
                    writer.u8(VECTOR_PRESENT);
                    writer.raw(&vector.encode());
                }
                None => writer.u8(VECTOR_ABSENT),
            },
            Self::Opaque(bytes) => writer.raw(bytes),
        }
        writer.into_bytes()
    }

    fn decode(entry: &Entry, bytes: &[u8]) -> Result<Self, CodecError> {
        let mut reader = Reader::new(bytes);
        let sequencing = match entry {
            Entry::Enroll(_) | Entry::SetRole(_) | Entry::ResolveLate(_) | Entry::Yield(_) => {
                Self::Empty
            }
            Entry::Remove(_) => {
                let count = usize::from(reader.u16()?);
                if count > MAX_REMOVAL_CUTOFFS {
                    return Err(CodecError::Bounds("removal cutoffs"));
                }
                let mut cutoffs = Vec::with_capacity(count);
                for _ in 0..count {
                    cutoffs.push((WriterId::from_bytes(reader.array()?), reader.u64()?));
                }
                Self::Removal {
                    cutoffs,
                    discovery_key: reader.array()?,
                }
            }
            Entry::Handover(_) => match reader.u8()? {
                VECTOR_ABSENT => Self::Handover {
                    old_anchor_vector: None,
                },
                VECTOR_PRESENT => Self::Handover {
                    old_anchor_vector: Some(VersionVector::decode(
                        reader.take(reader.remaining())?,
                    )?),
                },
                _ => return Err(CodecError::Malformed("handover vector")),
            },
            Entry::Unknown { .. } => Self::Opaque(reader.take(reader.remaining())?.to_vec()),
        };
        reader.finish()?;
        Ok(sequencing)
    }

    fn validate(&self, entry: &Entry) -> Result<(), CodecError> {
        match (entry, self) {
            (
                Entry::Enroll(_) | Entry::SetRole(_) | Entry::ResolveLate(_) | Entry::Yield(_),
                Self::Empty,
            )
            | (Entry::Unknown { .. }, Self::Opaque(_)) => Ok(()),
            (Entry::Remove(_), Self::Removal { cutoffs, .. }) => {
                if cutoffs.len() > MAX_REMOVAL_CUTOFFS {
                    return Err(CodecError::Bounds("removal cutoffs"));
                }
                let ascending = cutoffs.windows(2).all(|pair| pair[0].0 < pair[1].0);
                if !ascending || cutoffs.iter().any(|(_, seq)| *seq == 0) {
                    return Err(CodecError::Malformed("removal cutoffs"));
                }
                Ok(())
            }
            (Entry::Handover(_), Self::Handover { old_anchor_vector }) => {
                if old_anchor_vector
                    .as_ref()
                    .is_some_and(|vector| vector.len() > MAX_WRITERS_PER_SPACE)
                {
                    return Err(CodecError::Bounds("handover vector"));
                }
                Ok(())
            }
            _ => Err(CodecError::Malformed("sequencing")),
        }
    }
}

/// One revision of the authorization log, as a kind 4 operation body carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRecord {
    /// Anchor epoch; a handover opens the next one.
    pub epoch: u32,
    /// Revision this record creates, from 1.
    pub revision: u64,
    /// Entry hash of the previous revision, or the charter hash for revision 1.
    pub previous_entry_hash: Digest32,
    /// Device id of the anchor that ordered the record.
    pub anchor_device_id: String,
    /// The admin-signed change.
    pub proposal: SignedProposal,
    /// What the anchor added when ordering it.
    pub sequencing: Sequencing,
}

impl AuthorizationRecord {
    /// Checks the record's internal rules: the revision, the anchor device id, the sequencing
    /// shape, and for a handover its continuity with the record.
    pub fn validate(&self) -> Result<(), AuthorizationError> {
        if self.revision == 0 {
            return Err(CodecError::Malformed("authorization revision").into());
        }
        validate_device_id(&self.anchor_device_id)?;
        let entry = &self.proposal.proposal.entry;
        entry.validate()?;
        self.sequencing.validate(entry)?;
        if let Entry::Handover(handover) = entry {
            let continuous = handover.epoch == self.epoch
                && handover.new_anchor_device_id == self.anchor_device_id
                && handover.continues_from_revision.checked_add(1) == Some(self.revision)
                && handover.continues_from_hash == self.previous_entry_hash;
            if !continuous {
                return Err(CodecError::Malformed("handover continuity").into());
            }
        }
        Ok(())
    }

    /// Canonical encoding, the kind 4 operation body.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        writer.u32(self.epoch);
        writer.u64(self.revision);
        writer.raw(&self.previous_entry_hash);
        writer.bytes_u8(self.anchor_device_id.as_bytes());
        self.proposal.encode_into(&mut writer);
        writer.bytes_u32(&self.sequencing.encode());
        writer.into_bytes()
    }

    /// Decodes and validates a record, verifying the proposal signature.
    pub fn decode(bytes: &[u8]) -> Result<Self, AuthorizationError> {
        let mut reader = Reader::new(bytes);
        let epoch = reader.u32()?;
        let revision = reader.u64()?;
        let previous_entry_hash = reader.array::<DIGEST_BYTES>()?;
        let anchor_device_id = read_device_id(&mut reader)?;
        let proposal = SignedProposal::decode_from(&mut reader)?;
        let sequencing = Sequencing::decode(
            &proposal.proposal.entry,
            reader.bytes_u32(MAX_OPERATION_BYTES, "sequencing")?,
        )?;
        reader.finish()?;
        let record = Self {
            epoch,
            revision,
            previous_entry_hash,
            anchor_device_id,
            proposal,
            sequencing,
        };
        record.validate()?;
        Ok(record)
    }

    /// Entry hash: the previous entry hash of the next revision.
    pub fn hash(&self) -> Digest32 {
        entry_hash(&self.encode())
    }
}

/// Entry hash of an encoded authorization record.
pub fn entry_hash(record_bytes: &[u8]) -> Digest32 {
    domain_digest(AUTHORIZATION_ENTRY_DOMAIN, &[record_bytes])
}

/// Why the anchor refused to order a proposal. The set is closed: a reason this build does not
/// know is a protocol error, not a new outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProposalRejection {
    /// The receiver is not the anchor of the current epoch.
    NotAnchor,
    /// The proposal names another space, or a charter other than the one the space starts from.
    WrongSpace,
    /// The author key is not an admin key at the current revision.
    NotAdmin,
    /// The proposal or its entry does not decode or verify.
    Malformed,
    /// The entry kind is unknown to the anchor.
    UnsupportedEntry,
    /// The enrolled device id is already enrolled.
    AlreadyEnrolled,
    /// The named device is not enrolled.
    UnknownDevice,
    /// The named device was removed.
    DeviceRemoved,
    /// The entry would remove or demote the anchor itself.
    NamesAnchor,
    /// The entry would give the admin role to a mobile device.
    AdminNotDesktop,
    /// The entry would leave the space without an admin key besides the recovery key.
    LastAdmin,
    /// The late resolution names no removal, or a row with no late change.
    UnknownRemoval,
    /// The handover does not continue the current log, or another handover won.
    HandoverConflict,
    /// A writer of the anchor forked in the current epoch, or the epoch's handovers are
    /// contested, so only a yield or a recovery kit handover continues the log.
    EpochFrozen,
    /// The anchor a handover installed does not yet know a writer the old anchor had stored, so
    /// it cannot set a removal's cutoffs yet.
    CatchingUp,
}

impl ProposalRejection {
    /// Every reason, in wire order.
    pub const ALL: [Self; 15] = [
        Self::NotAnchor,
        Self::WrongSpace,
        Self::NotAdmin,
        Self::Malformed,
        Self::UnsupportedEntry,
        Self::AlreadyEnrolled,
        Self::UnknownDevice,
        Self::DeviceRemoved,
        Self::NamesAnchor,
        Self::AdminNotDesktop,
        Self::LastAdmin,
        Self::UnknownRemoval,
        Self::HandoverConflict,
        Self::EpochFrozen,
        Self::CatchingUp,
    ];

    /// Wire value.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::NotAnchor => 0,
            Self::WrongSpace => 1,
            Self::NotAdmin => 2,
            Self::Malformed => 3,
            Self::UnsupportedEntry => 4,
            Self::AlreadyEnrolled => 5,
            Self::UnknownDevice => 6,
            Self::DeviceRemoved => 7,
            Self::NamesAnchor => 8,
            Self::AdminNotDesktop => 9,
            Self::LastAdmin => 10,
            Self::UnknownRemoval => 11,
            Self::HandoverConflict => 12,
            Self::EpochFrozen => 13,
            Self::CatchingUp => 14,
        }
    }

    /// Reason for a wire value.
    pub fn from_u8(value: u8) -> Option<Self> {
        Self::ALL.get(usize::from(value)).copied()
    }

    /// Stable name for logs and diagnostics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotAnchor => "not_anchor",
            Self::WrongSpace => "wrong_space",
            Self::NotAdmin => "not_admin",
            Self::Malformed => "malformed",
            Self::UnsupportedEntry => "unsupported_entry",
            Self::AlreadyEnrolled => "already_enrolled",
            Self::UnknownDevice => "unknown_device",
            Self::DeviceRemoved => "device_removed",
            Self::NamesAnchor => "names_anchor",
            Self::AdminNotDesktop => "admin_not_desktop",
            Self::LastAdmin => "last_admin",
            Self::UnknownRemoval => "unknown_removal",
            Self::HandoverConflict => "handover_conflict",
            Self::EpochFrozen => "epoch_frozen",
            Self::CatchingUp => "catching_up",
        }
    }
}

impl fmt::Display for ProposalRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

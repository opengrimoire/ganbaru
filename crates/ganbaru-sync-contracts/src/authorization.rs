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
//! A signed proposal is: version (1) | space id (16) | charter hash (32) | author public key (32)
//! | created at milliseconds (8) | entry kind (1) | entry length (4) and entry | signature (64)
//! over the proposal domain and everything before it. Its hash, the proposal hash digest of the
//! unsigned encoding, makes resubmission idempotent. Decoding verifies the signature against the
//! embedded author key; whether that key may author the entry at the record's revision is the
//! log's question. Most entries are signed by an admin key under the proposal domain. Device key
//! and key epoch entries are signed by a device key under the device proposal domain: a device
//! key entry by the device it names, a key epoch entry by the anchor device.
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
//! - Device key (6): device id length (1) and id | device encryption public key (32).
//! - Key epoch (7): key epoch (4) | chain link flag (1), `1` followed by the link sealing the
//!   previous epoch's key (72) and present exactly when the key epoch is above 1 | envelope count
//!   (2) and envelopes (96 each), strictly ascending by recipient key id | relay access public key
//!   (32).
//! - Recovery key (8): recovery encryption public key (32).
//! - Server config (9): empty to remove the configuration, or mode (1) | endpoint count (1) and
//!   per endpoint length (1) and `host:port` | certificate fingerprint (32).
//!
//! Sequencing is what only the anchor knows when it orders a proposal, outside the signature:
//! for a removal, the stored sequence cutoff of each writer of the removed device and a fresh
//! discovery key; for a handover, sealed by the new anchor as the first entry of its epoch, the
//! old anchor's stored vector when known; for a device key, flag (1) and, when `1`, the envelope
//! of the current epoch's key sealed to the new device key (96). Other kinds have empty
//! sequencing, and an unknown kind keeps its sequencing bytes.

use crate::bounds::{
    DISCOVERY_KEY_BYTES, MAX_DEVICE_LABEL_BYTES, MAX_KEY_RECIPIENTS, MAX_LATE_RESOLUTIONS,
    MAX_OPERATION_BYTES, MAX_REMOVAL_CUTOFFS, MAX_ROW_KEY_BYTES, MAX_SERVER_ENDPOINT_BYTES,
    MAX_SERVER_ENDPOINTS, MAX_WRITERS_PER_SPACE, TLS_FINGERPRINT_BYTES,
};
use crate::certificate::{CertificateError, read_device_id, validate_device_id};
use crate::codec::{CodecError, Reader, Writer};
use crate::crypto::{
    CHAIN_LINK_BYTES, ChainLink, ENCRYPTION_PUBLIC_KEY_BYTES, EncryptionPublicKey,
    KEY_ENVELOPE_BYTES, KeyEnvelope,
};
use crate::ids::{RowKey, Seq, SpaceId, TableId, WriterId};
use crate::keys::{
    AdminKeyPair, AdminPublicKey, DeviceKeyPair, DevicePublicKey, PUBLIC_KEY_BYTES,
    RelayAccessPublicKey, SIGNATURE_BYTES,
};
use crate::signing::{
    AUTHORIZATION_ENTRY_DOMAIN, DEVICE_PROPOSAL_DOMAIN, DIGEST_BYTES, Digest32, PROPOSAL_DOMAIN,
    PROPOSAL_HASH_DOMAIN, domain_digest,
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
const ENTRY_DEVICE_KEY: u8 = 6;
const ENTRY_KEY_EPOCH: u8 = 7;
const ENTRY_RECOVERY_KEY: u8 = 8;
const ENTRY_SERVER_CONFIG: u8 = 9;
/// The highest entry kind this build knows.
const ENTRY_LAST_KNOWN: u8 = ENTRY_SERVER_CONFIG;

const ROLE_MEMBER: u8 = 0;
const ROLE_ADMIN: u8 = 1;
const SET_ROLE_DEVICE: u8 = 0;
const SET_ROLE_RECOVERY: u8 = 1;
const VECTOR_ABSENT: u8 = 0;
const VECTOR_PRESENT: u8 = 1;
const LINK_ABSENT: u8 = 0;
const LINK_PRESENT: u8 = 1;
const ENVELOPE_ABSENT: u8 = 0;
const ENVELOPE_PRESENT: u8 = 1;
const SERVER_RELAY: u8 = 0;
const SERVER_ANCHOR: u8 = 1;

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
    /// The signature does not verify.
    Signature,
    /// The entry kind is signed by the other kind of key: a device key for device key and key
    /// epoch entries, an admin key for every other entry.
    Signer,
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
            Self::Signer => {
                formatter.write_str("authorization entry kind is signed by another key kind")
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

/// Publishes the device encryption key of an enrolled device, signed by that device's key. A
/// later device key entry for the same device replaces the earlier one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceKey {
    /// Handoff device id of the device the key belongs to.
    pub device_id: String,
    /// The device's static X25519 encryption key for this vault.
    pub encryption_key: EncryptionPublicKey,
}

/// Opens a key epoch, signed by the anchor device: the new epoch's key sealed to every live
/// device key and the recovery key, and the previous epoch's key sealed under the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEpoch {
    /// The new key epoch, from 1.
    pub key_epoch: u32,
    /// The previous epoch's key sealed under the new key, present exactly when the key epoch is
    /// above 1.
    pub wrapped_previous: Option<ChainLink>,
    /// One envelope per recipient, strictly ascending by recipient key id.
    pub envelopes: Vec<KeyEnvelope>,
    /// Public half of the relay access key derived from the new epoch's key.
    pub relay_access_key: RelayAccessPublicKey,
}

impl KeyEpoch {
    /// The envelope addressed to `recipient`, if any.
    pub fn envelope_for(&self, recipient: &EncryptionPublicKey) -> Option<&KeyEnvelope> {
        let id = recipient.key_id();
        self.envelopes
            .binary_search_by(|envelope| envelope.recipient.cmp(&id))
            .ok()
            .map(|index| &self.envelopes[index])
    }
}

/// Sets the recovery encryption key, the recipient of every epoch key that a recovery kit opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryKey {
    /// Recovery X25519 public key.
    pub encryption_key: EncryptionPublicKey,
}

/// What a configured server does for the space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerMode {
    /// Stores and forwards sealed records.
    Relay,
    /// Runs as the space's anchor.
    Anchor,
}

impl ServerMode {
    /// Wire value.
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Relay => SERVER_RELAY,
            Self::Anchor => SERVER_ANCHOR,
        }
    }

    /// Mode for a wire value.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            SERVER_RELAY => Some(Self::Relay),
            SERVER_ANCHOR => Some(Self::Anchor),
            _ => None,
        }
    }
}

/// A self-hosted server the space uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// What the server does.
    pub mode: ServerMode,
    /// Addresses as `host:port`, from 1 to [`MAX_SERVER_ENDPOINTS`].
    pub endpoints: Vec<String>,
    /// SHA-256 of the server's DER certificate.
    pub certificate_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
}

/// Sets or removes the space's server configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConfig {
    /// The configuration, or `None` to remove it.
    pub server: Option<Server>,
}

/// Whether `endpoint` is `host:port` with a nonempty host of printable ASCII without spaces or
/// slashes (an IPv6 host in brackets) and a decimal port from 1 to 65535 without leading zeros.
pub fn validate_server_endpoint(endpoint: &str) -> bool {
    if endpoint.is_empty() || endpoint.len() > MAX_SERVER_ENDPOINT_BYTES {
        return false;
    }
    let Some((host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };
    let port_valid = !port.is_empty()
        && !port.starts_with('0')
        && port.bytes().all(|byte| byte.is_ascii_digit())
        && port.parse::<u16>().is_ok();
    let host = match host.strip_prefix('[') {
        Some(inner) => match inner.strip_suffix(']') {
            Some(address) => {
                if address.parse::<std::net::Ipv6Addr>().is_err() {
                    return false;
                }
                address
            }
            None => return false,
        },
        None => {
            if host.contains(':') {
                return false;
            }
            host
        }
    };
    let host_valid = !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'/' | b'[' | b']' | b'@'));
    port_valid && host_valid
}

fn read_encryption_key(reader: &mut Reader<'_>) -> Result<EncryptionPublicKey, CodecError> {
    EncryptionPublicKey::from_bytes(reader.array::<ENCRYPTION_PUBLIC_KEY_BYTES>()?)
        .map_err(|_| CodecError::Malformed("encryption key"))
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
    /// Publishes a device encryption key.
    DeviceKey(DeviceKey),
    /// Opens a key epoch.
    KeyEpoch(KeyEpoch),
    /// Sets the recovery encryption key.
    RecoveryKey(RecoveryKey),
    /// Sets or removes the server configuration.
    ServerConfig(ServerConfig),
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
            Self::DeviceKey(_) => ENTRY_DEVICE_KEY,
            Self::KeyEpoch(_) => ENTRY_KEY_EPOCH,
            Self::RecoveryKey(_) => ENTRY_RECOVERY_KEY,
            Self::ServerConfig(_) => ENTRY_SERVER_CONFIG,
            Self::Unknown { kind, .. } => *kind,
        }
    }

    /// Whether a device key, rather than an admin key, signs this entry. An unknown kind is
    /// admin-signed.
    pub fn device_signed(&self) -> bool {
        matches!(self, Self::DeviceKey(_) | Self::KeyEpoch(_))
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
            Self::Yield(_) | Self::RecoveryKey(_) => {}
            Self::DeviceKey(device_key) => validate_device_id(&device_key.device_id)?,
            Self::KeyEpoch(key_epoch) => {
                if key_epoch.key_epoch == 0
                    || key_epoch.wrapped_previous.is_some() != (key_epoch.key_epoch > 1)
                {
                    return Err(CodecError::Malformed("key epoch").into());
                }
                if key_epoch.envelopes.is_empty() || key_epoch.envelopes.len() > MAX_KEY_RECIPIENTS
                {
                    return Err(CodecError::Bounds("key envelopes").into());
                }
                let ascending = key_epoch
                    .envelopes
                    .windows(2)
                    .all(|pair| pair[0].recipient < pair[1].recipient);
                if !ascending {
                    return Err(CodecError::Malformed("key envelope order").into());
                }
            }
            Self::ServerConfig(config) => {
                if let Some(server) = &config.server {
                    if server.endpoints.is_empty() || server.endpoints.len() > MAX_SERVER_ENDPOINTS
                    {
                        return Err(CodecError::Bounds("server endpoints").into());
                    }
                    if !server
                        .endpoints
                        .iter()
                        .all(|endpoint| validate_server_endpoint(endpoint))
                    {
                        return Err(CodecError::Malformed("server endpoint").into());
                    }
                }
            }
            Self::Unknown { kind, .. } => {
                if *kind <= ENTRY_LAST_KNOWN {
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
            Self::DeviceKey(device_key) => {
                writer.bytes_u8(device_key.device_id.as_bytes());
                writer.raw(device_key.encryption_key.as_bytes());
            }
            Self::KeyEpoch(key_epoch) => {
                writer.u32(key_epoch.key_epoch);
                match &key_epoch.wrapped_previous {
                    Some(link) => {
                        writer.u8(LINK_PRESENT);
                        writer.raw(&link.to_bytes());
                    }
                    None => writer.u8(LINK_ABSENT),
                }
                writer.u16(key_epoch.envelopes.len() as u16);
                for envelope in &key_epoch.envelopes {
                    writer.raw(&envelope.to_bytes());
                }
                writer.raw(key_epoch.relay_access_key.as_bytes());
            }
            Self::RecoveryKey(recovery) => writer.raw(recovery.encryption_key.as_bytes()),
            Self::ServerConfig(config) => {
                if let Some(server) = &config.server {
                    writer.u8(server.mode.as_u8());
                    writer.u8(server.endpoints.len() as u8);
                    for endpoint in &server.endpoints {
                        writer.bytes_u8(endpoint.as_bytes());
                    }
                    writer.raw(&server.certificate_fingerprint);
                }
            }
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
            ENTRY_DEVICE_KEY => Self::DeviceKey(DeviceKey {
                device_id: read_device_id(&mut reader)?,
                encryption_key: read_encryption_key(&mut reader)?,
            }),
            ENTRY_KEY_EPOCH => {
                let key_epoch = reader.u32()?;
                let wrapped_previous = match reader.u8()? {
                    LINK_ABSENT => None,
                    LINK_PRESENT => Some(
                        ChainLink::from_slice(reader.take(CHAIN_LINK_BYTES)?)
                            .ok_or(CodecError::Malformed("key chain link"))?,
                    ),
                    _ => return Err(CodecError::Malformed("key chain link").into()),
                };
                let count = usize::from(reader.u16()?);
                if count > MAX_KEY_RECIPIENTS {
                    return Err(CodecError::Bounds("key envelopes").into());
                }
                let mut envelopes = Vec::with_capacity(count);
                for _ in 0..count {
                    envelopes.push(
                        KeyEnvelope::from_slice(reader.take(KEY_ENVELOPE_BYTES)?)
                            .ok_or(CodecError::Malformed("key envelope"))?,
                    );
                }
                let relay_access_key =
                    RelayAccessPublicKey::from_slice(&reader.array::<PUBLIC_KEY_BYTES>()?)
                        .map_err(|_| CodecError::Malformed("relay access key"))?;
                Self::KeyEpoch(KeyEpoch {
                    key_epoch,
                    wrapped_previous,
                    envelopes,
                    relay_access_key,
                })
            }
            ENTRY_RECOVERY_KEY => Self::RecoveryKey(RecoveryKey {
                encryption_key: read_encryption_key(&mut reader)?,
            }),
            ENTRY_SERVER_CONFIG => {
                let server = if reader.remaining() == 0 {
                    None
                } else {
                    let mode = ServerMode::from_u8(reader.u8()?)
                        .ok_or(CodecError::Malformed("server mode"))?;
                    let count = usize::from(reader.u8()?);
                    if count > MAX_SERVER_ENDPOINTS {
                        return Err(CodecError::Bounds("server endpoints").into());
                    }
                    let mut endpoints = Vec::with_capacity(count);
                    for _ in 0..count {
                        let endpoint =
                            reader.bytes_u8(MAX_SERVER_ENDPOINT_BYTES, "server endpoint")?;
                        endpoints.push(
                            std::str::from_utf8(endpoint)
                                .map_err(|_| CodecError::Malformed("server endpoint"))?
                                .to_string(),
                        );
                    }
                    Some(Server {
                        mode,
                        endpoints,
                        certificate_fingerprint: reader.array()?,
                    })
                };
                Self::ServerConfig(ServerConfig { server })
            }
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
    /// Key of the author: an admin key, or for an entry that [`Entry::device_signed`] the raw
    /// bytes of a device key, which [`Proposal::device_author`] reads.
    pub author: AdminPublicKey,
    /// Creation time in Unix milliseconds; distinguishes deliberate re-proposals.
    pub created_at_ms: i64,
    /// The change.
    pub entry: Entry,
}

impl Proposal {
    /// The device key that signs a device-signed entry; `None` for an admin-signed one.
    pub fn device_author(&self) -> Option<DevicePublicKey> {
        self.entry
            .device_signed()
            .then(|| DevicePublicKey::from_bytes(*self.author.as_bytes()))
    }

    /// The admin key that signs an admin-signed entry; `None` for a device-signed one.
    pub fn admin_author(&self) -> Option<AdminPublicKey> {
        (!self.entry.device_signed()).then_some(self.author)
    }

    fn verify(&self, unsigned: &[u8], signature: &[u8]) -> bool {
        match self.device_author() {
            Some(device) => device.verify(DEVICE_PROPOSAL_DOMAIN, unsigned, signature),
            None => self.author.verify(PROPOSAL_DOMAIN, unsigned, signature),
        }
    }
}

/// A proposal with its signature.
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

/// Signs an admin-signed proposal with the author's admin key.
pub fn sign_proposal(
    proposal: &Proposal,
    admin: &AdminKeyPair,
) -> Result<SignedProposal, AuthorizationError> {
    proposal.entry.validate()?;
    if proposal.entry.device_signed() {
        return Err(AuthorizationError::Signer);
    }
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

/// Signs a device-signed proposal, a device key or key epoch entry, with the author's device key.
pub fn sign_device_proposal(
    proposal: &Proposal,
    device: &DeviceKeyPair,
) -> Result<SignedProposal, AuthorizationError> {
    proposal.entry.validate()?;
    if !proposal.entry.device_signed() {
        return Err(AuthorizationError::Signer);
    }
    if Some(device.public_key()) != proposal.device_author() {
        return Err(AuthorizationError::WrongAuthor);
    }
    let mut writer = Writer::default();
    encode_unsigned_proposal(proposal, &mut writer);
    let signature = device.sign(DEVICE_PROPOSAL_DOMAIN, writer.as_bytes());
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

    /// Reads one proposal and verifies its signature against the embedded author key, under the
    /// domain its entry kind is signed with.
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
        if !proposal.verify(unsigned.as_bytes(), &signature) {
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
    /// Nothing: enrollments, role changes, late resolutions, yields, key epochs, recovery keys,
    /// and server configurations.
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
    /// The current epoch's key sealed to the new device key, absent while the space has no key
    /// epoch.
    DeviceKey {
        /// Envelope of the current epoch's key, sealed by the anchor.
        envelope: Option<KeyEnvelope>,
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
            Self::DeviceKey { envelope } => match envelope {
                Some(envelope) => {
                    writer.u8(ENVELOPE_PRESENT);
                    writer.raw(&envelope.to_bytes());
                }
                None => writer.u8(ENVELOPE_ABSENT),
            },
            Self::Opaque(bytes) => writer.raw(bytes),
        }
        writer.into_bytes()
    }

    fn decode(entry: &Entry, bytes: &[u8]) -> Result<Self, CodecError> {
        let mut reader = Reader::new(bytes);
        let sequencing = match entry {
            Entry::Enroll(_)
            | Entry::SetRole(_)
            | Entry::ResolveLate(_)
            | Entry::Yield(_)
            | Entry::KeyEpoch(_)
            | Entry::RecoveryKey(_)
            | Entry::ServerConfig(_) => Self::Empty,
            Entry::DeviceKey(_) => match reader.u8()? {
                ENVELOPE_ABSENT => Self::DeviceKey { envelope: None },
                ENVELOPE_PRESENT => Self::DeviceKey {
                    envelope: Some(
                        KeyEnvelope::from_slice(reader.take(KEY_ENVELOPE_BYTES)?)
                            .ok_or(CodecError::Malformed("key envelope"))?,
                    ),
                },
                _ => return Err(CodecError::Malformed("device key envelope")),
            },
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
                Entry::Enroll(_)
                | Entry::SetRole(_)
                | Entry::ResolveLate(_)
                | Entry::Yield(_)
                | Entry::KeyEpoch(_)
                | Entry::RecoveryKey(_)
                | Entry::ServerConfig(_),
                Self::Empty,
            )
            | (Entry::Unknown { .. }, Self::Opaque(_)) => Ok(()),
            (Entry::DeviceKey(device_key), Self::DeviceKey { envelope }) => match envelope {
                Some(envelope) if envelope.recipient != device_key.encryption_key.key_id() => {
                    Err(CodecError::Malformed("device key envelope"))
                }
                _ => Ok(()),
            },
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
    /// The device key entry names a device this anchor holds in quarantine.
    DeviceQuarantined,
    /// The entry opens a key epoch, but the anchor cannot open the current epoch's key or holds
    /// no device encryption key of its own.
    KeyUnavailable,
    /// The entry kind is only sealed by the anchor itself.
    AnchorOnly,
    /// A removal or recovery change waits for the key epoch that the anchor seals right after it,
    /// so no other entry may follow it yet.
    RotationPending,
}

impl ProposalRejection {
    /// Every reason, in wire order.
    pub const ALL: [Self; 19] = [
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
        Self::DeviceQuarantined,
        Self::KeyUnavailable,
        Self::AnchorOnly,
        Self::RotationPending,
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
            Self::DeviceQuarantined => 15,
            Self::KeyUnavailable => 16,
            Self::AnchorOnly => 17,
            Self::RotationPending => 18,
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
            Self::DeviceQuarantined => "device_quarantined",
            Self::KeyUnavailable => "key_unavailable",
            Self::AnchorOnly => "anchor_only",
            Self::RotationPending => "rotation_pending",
        }
    }
}

impl fmt::Display for ProposalRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

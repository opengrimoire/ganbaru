//! Wire contract between a linked device and a self-hosted relay server.
//!
//! The relay stores sealed relay records and asset chunks it cannot open. A device proves that it
//! holds a space epoch key by signing with that epoch's relay access key, bound to the TLS session
//! through an exporter, so the relay learns which epoch a connection holds and nothing about the
//! device. Every byte field travels as base64url and every list is bounded, so a frame is checked
//! completely before any of its fields is decoded.

use super::{Frame, SyncSeq, is_base64url};
use base64::Engine;
use ganbaru_sync_contracts::bounds::{
    MAX_OPERATION_BYTES, MAX_SERVER_ENDPOINTS, MAX_WRITERS_PER_SPACE, TLS_FINGERPRINT_BYTES,
};
use ganbaru_sync_contracts::keys::{PUBLIC_KEY_BYTES, SIGNATURE_BYTES};
use ganbaru_sync_contracts::relay::{
    MAX_RELAY_ASSET_CHUNK_BYTES, MAX_RELAY_RECORD_BYTES, RELAY_ASSET_ID_BYTES,
};
use ganbaru_sync_contracts::{AdminPublicKey, RelayAccessPublicKey, SpaceId, WriterId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Version of the relay exchange; a relay refuses any other version.
pub const RELAY_PROTOCOL_VERSION: u16 = 1;
/// Default TCP port of a relay server.
pub const RELAY_PORT: u16 = 43_823;
/// Signature domain of a relay access credential.
pub const RELAY_AUTH_DOMAIN: &[u8] = b"ganbaru-sync/relay-auth/v1";
/// Signature domain of an epoch registration, signed with the next epoch's access key.
pub const RELAY_EPOCH_DOMAIN: &[u8] = b"ganbaru-sync/relay-epoch/v1";
/// Signature domain of a relay admin command, signed with an admin key of the space.
pub const RELAY_ADMIN_DOMAIN: &[u8] = b"ganbaru-sync/relay-admin/v1";
/// TLS exporter label that binds relay signatures to one connection.
pub const RELAY_EXPORTER_LABEL: &[u8] = b"ganbaru relay auth";
/// Bytes taken from the TLS exporter.
pub const RELAY_EXPORTER_BYTES: usize = 32;
/// Raw record bytes in one push or pull page. Its base64url form plus JSON framing stays below
/// the control frame limit, and a record of the largest size always fits on its own.
pub const RELAY_PAGE_BYTES: usize = 600 * 1024;
/// Records in one page; a record is never smaller than its header, nonce, and tag.
pub const MAX_RELAY_PAGE_RECORDS: usize = 8_192;
/// Admin keys a space keeps on a relay.
pub const MAX_RELAY_ADMIN_KEYS: usize = 64;
/// Assets in one presence query.
pub const MAX_RELAY_ASSET_QUERY: usize = 1_024;
/// Largest key epoch authorization entry a relay keeps for an epoch.
pub const MAX_RELAY_EPOCH_ENTRY_BYTES: usize = MAX_OPERATION_BYTES;
/// Longest a relay holds a wait request open before answering that nothing arrived.
pub const RELAY_WAIT_TIMEOUT: Duration = Duration::from_secs(12);
/// Bytes of a one-time setup token.
pub const RELAY_SETUP_TOKEN_BYTES: usize = 32;
/// How long a setup token can claim a space.
pub const RELAY_SETUP_TOKEN_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// Text prefix of a relay setup code.
pub const RELAY_SETUP_TEXT_PREFIX: &str = "ganbaru-relay:";
/// Longest refusal message on the wire.
pub const MAX_RELAY_REFUSAL_MESSAGE_BYTES: usize = 256;

pub(super) const RELAY_SETUP_MAGIC: &[u8; 4] = b"GBRL";
const RELAY_SETUP_VERSION: u8 = 1;
const ADMIN_COMMAND_CLAIM: u8 = 1;
const ADMIN_COMMAND_ADD_KEY: u8 = 2;
const ADMIN_COMMAND_REMOVE_KEY: u8 = 3;
const ADMIN_COMMAND_WIPE: u8 = 4;
const ADMIN_COMMAND_REMOVE_SPACE: u8 = 5;
const ADMIN_COMMAND_EPOCH_REPAIR: u8 = 6;
const SPACE_TEXT_LENGTH: usize = 32;
const WRITER_TEXT_LENGTH: usize = 32;
const ASSET_TEXT_LENGTH: usize = RELAY_ASSET_ID_BYTES * 2;

const fn base64_length(bytes: usize) -> usize {
    (bytes * 4).div_ceil(3)
}

const KEY_TEXT_LENGTH: usize = base64_length(PUBLIC_KEY_BYTES);
const SIGNATURE_TEXT_LENGTH: usize = base64_length(SIGNATURE_BYTES);
const MAX_RECORD_TEXT_BYTES: usize = base64_length(MAX_RELAY_RECORD_BYTES);
const MAX_CHUNK_TEXT_BYTES: usize = base64_length(MAX_RELAY_ASSET_CHUNK_BYTES);
const MAX_ENTRY_TEXT_BYTES: usize = base64_length(MAX_RELAY_EPOCH_ENTRY_BYTES);
/// JSON quotes and separator around one base64url string in a list.
const LIST_ITEM_OVERHEAD: usize = 3;
/// Room for the message kind, flags, and the fixed fields around a page or chunk.
const FRAME_OVERHEAD: usize = 64 * 1024;

const _: () = assert!(
    base64_length(RELAY_PAGE_BYTES) + MAX_RELAY_PAGE_RECORDS * LIST_ITEM_OVERHEAD + FRAME_OVERHEAD
        <= super::MAX_CONTROL_BYTES
);
const _: () = assert!(MAX_RELAY_RECORD_BYTES <= RELAY_PAGE_BYTES);
const _: () = assert!(MAX_CHUNK_TEXT_BYTES + FRAME_OVERHEAD <= super::MAX_CONTROL_BYTES);
const _: () = assert!(MAX_ENTRY_TEXT_BYTES + FRAME_OVERHEAD <= super::MAX_CONTROL_BYTES);

/// Encodes bytes as unpadded base64url for a relay frame.
pub fn encode_relay_bytes(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// Decodes a base64url field of at most `max_bytes` decoded bytes, checking the text length
/// before decoding.
pub fn decode_relay_bytes(label: &str, text: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if text.is_empty() || text.len() > base64_length(max_bytes) || !is_base64url(text) {
        return Err(format!("{label} is invalid"));
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| format!("{label} is invalid"))?;
    if bytes.len() > max_bytes {
        return Err(format!("{label} is invalid"));
    }
    Ok(bytes)
}

/// Decodes a base64url field of exactly `N` bytes.
pub fn decode_relay_fixed<const N: usize>(label: &str, text: &str) -> Result<[u8; N], String> {
    if text.len() != base64_length(N) {
        return Err(format!("{label} is invalid"));
    }
    let bytes = decode_relay_bytes(label, text, N)?;
    <[u8; N]>::try_from(bytes.as_slice()).map_err(|_| format!("{label} is invalid"))
}

/// Parses a relay access key field.
pub fn decode_relay_access_key(text: &str) -> Result<RelayAccessPublicKey, String> {
    let bytes = decode_relay_fixed::<PUBLIC_KEY_BYTES>("relay access key", text)?;
    Ok(RelayAccessPublicKey::from_bytes(bytes))
}

/// Parses an admin key field.
pub fn decode_relay_admin_key(text: &str) -> Result<AdminPublicKey, String> {
    let bytes = decode_relay_fixed::<PUBLIC_KEY_BYTES>("relay admin key", text)?;
    Ok(AdminPublicKey::from_bytes(bytes))
}

/// Parses a signature field.
pub fn decode_relay_signature(text: &str) -> Result<[u8; SIGNATURE_BYTES], String> {
    decode_relay_fixed::<SIGNATURE_BYTES>("relay signature", text)
}

/// Parses a space id field.
pub fn decode_relay_space(text: &str) -> Result<SpaceId, String> {
    if text.len() != SPACE_TEXT_LENGTH {
        return Err("relay space is invalid".to_string());
    }
    SpaceId::from_hex(text).ok_or_else(|| "relay space is invalid".to_string())
}

/// Parses a writer id field.
pub fn decode_relay_writer(text: &str) -> Result<WriterId, String> {
    if text.len() != WRITER_TEXT_LENGTH {
        return Err("relay writer is invalid".to_string());
    }
    WriterId::from_hex(text).ok_or_else(|| "relay writer is invalid".to_string())
}

/// Parses an asset id field.
pub fn decode_relay_asset(text: &str) -> Result<[u8; RELAY_ASSET_ID_BYTES], String> {
    if text.len() != ASSET_TEXT_LENGTH {
        return Err("relay asset is invalid".to_string());
    }
    ganbaru_sync_contracts::ids::parse_hex(text).ok_or_else(|| "relay asset is invalid".to_string())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

/// Hexadecimal form of an asset id on the wire.
pub fn encode_relay_asset(asset: &[u8; RELAY_ASSET_ID_BYTES]) -> String {
    hex(asset)
}

fn is_lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn check_hex(label: &str, text: &str, length: usize) -> Result<(), String> {
    if is_lower_hex(text, length) {
        Ok(())
    } else {
        Err(format!("{label} is invalid"))
    }
}

fn check_fixed_text(label: &str, text: &str, length: usize) -> Result<(), String> {
    if text.len() == length && is_base64url(text) {
        Ok(())
    } else {
        Err(format!("{label} is invalid"))
    }
}

fn check_bounded_text(label: &str, text: &str, max_length: usize) -> Result<(), String> {
    if !text.is_empty() && text.len() <= max_length && is_base64url(text) {
        Ok(())
    } else {
        Err(format!("{label} is invalid"))
    }
}

fn check_vector(have: &[SyncSeq]) -> Result<(), String> {
    if have.len() > MAX_WRITERS_PER_SPACE {
        return Err("relay vector has too many writers".to_string());
    }
    for entry in have {
        check_hex("relay vector writer", &entry.writer, WRITER_TEXT_LENGTH)?;
    }
    Ok(())
}

/// The context every relay signature is bound to: the connection's TLS exporter and the space.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RelayBinding {
    /// Exporter bytes of the TLS session, see [`RELAY_EXPORTER_LABEL`].
    pub exporter: [u8; RELAY_EXPORTER_BYTES],
    /// Space the connection works on.
    pub space: SpaceId,
}

impl fmt::Debug for RelayBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayBinding")
            .field("space", &self.space)
            .finish_non_exhaustive()
    }
}

impl RelayBinding {
    fn payload(&self, extra: usize) -> Vec<u8> {
        let mut payload = Vec::with_capacity(RELAY_EXPORTER_BYTES + 16 + extra);
        payload.extend_from_slice(&self.exporter);
        payload.extend_from_slice(self.space.as_bytes());
        payload
    }

    /// Payload a device signs with an epoch's relay access key to authenticate.
    pub fn auth_payload(&self, epoch: u32) -> Vec<u8> {
        let mut payload = self.payload(4);
        payload.extend_from_slice(&epoch.to_be_bytes());
        payload
    }

    /// Payload a device signs with the next epoch's relay access key to register that epoch.
    pub fn epoch_payload(&self, next_epoch: u32) -> Vec<u8> {
        self.auth_payload(next_epoch)
    }

    /// Payload an admin signs to run `command` with `counter`.
    pub fn admin_payload(&self, counter: u64, command: &RelayAdminCommand<'_>) -> Vec<u8> {
        let mut payload = self.payload(8 + 1 + 4 + PUBLIC_KEY_BYTES * (MAX_RELAY_ADMIN_KEYS + 1));
        payload.extend_from_slice(&counter.to_be_bytes());
        match command {
            RelayAdminCommand::Claim {
                epoch,
                access_key,
                admins,
            } => {
                payload.push(ADMIN_COMMAND_CLAIM);
                payload.extend_from_slice(&epoch.to_be_bytes());
                payload.extend_from_slice(access_key.as_bytes());
                // The admin count is bounded by MAX_RELAY_ADMIN_KEYS, far below u8::MAX.
                payload.push(u8::try_from(admins.len()).unwrap_or(u8::MAX));
                for admin in *admins {
                    payload.extend_from_slice(admin.as_bytes());
                }
            }
            RelayAdminCommand::AddKey(key) => {
                payload.push(ADMIN_COMMAND_ADD_KEY);
                payload.extend_from_slice(key.as_bytes());
            }
            RelayAdminCommand::RemoveKey(key) => {
                payload.push(ADMIN_COMMAND_REMOVE_KEY);
                payload.extend_from_slice(key.as_bytes());
            }
            RelayAdminCommand::Wipe => payload.push(ADMIN_COMMAND_WIPE),
            RelayAdminCommand::RemoveSpace => payload.push(ADMIN_COMMAND_REMOVE_SPACE),
            RelayAdminCommand::RelayEpochRepair { epoch, access_key } => {
                payload.push(ADMIN_COMMAND_EPOCH_REPAIR);
                payload.extend_from_slice(&epoch.to_be_bytes());
                payload.extend_from_slice(access_key.as_bytes());
            }
        }
        payload
    }
}

/// An admin command in the form its signature covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayAdminCommand<'a> {
    /// Creates a space with its first epoch access key and admin set.
    Claim {
        epoch: u32,
        access_key: &'a RelayAccessPublicKey,
        admins: &'a [AdminPublicKey],
    },
    /// Adds an admin key to the space's admin set.
    AddKey(&'a AdminPublicKey),
    /// Removes an admin key; the last key cannot be removed.
    RemoveKey(&'a AdminPublicKey),
    /// Deletes every stored record and asset of the space and keeps its epochs and admins.
    Wipe,
    /// Deletes the space and everything stored for it.
    RemoveSpace,
    /// Sets the space's current epoch and its access key, also below the current epoch, and
    /// deletes every epoch, record, and asset chunk stored above it. The relay cannot check that a
    /// registered epoch key is the real one, so this is how an admin undoes a wrong registration;
    /// the admin counter, not the epoch, keeps it from being replayed.
    RelayEpochRepair {
        epoch: u32,
        access_key: &'a RelayAccessPublicKey,
    },
}

/// An admin action on the wire.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "action")]
pub enum RelayAdminAction {
    /// See [`RelayAdminCommand::AddKey`].
    AddKey { key: String },
    /// See [`RelayAdminCommand::RemoveKey`].
    RemoveKey { key: String },
    /// See [`RelayAdminCommand::Wipe`].
    Wipe,
    /// See [`RelayAdminCommand::RemoveSpace`].
    RemoveSpace,
    /// See [`RelayAdminCommand::RelayEpochRepair`].
    RelayEpochRepair { epoch: u32, access_key: String },
}

/// A request a device sends to a relay.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RelayRequest {
    /// Opens the exchange for one space.
    Hello {
        protocol_version: u16,
        space: String,
    },
    /// Authenticates with the relay access key of `epoch`.
    Auth { epoch: u32, signature: String },
    /// Creates the space with a one-time setup token.
    Claim {
        token: RelaySetupToken,
        epoch: u32,
        access_key: String,
        admins: Vec<String>,
        counter: u64,
        signer: String,
        signature: String,
    },
    /// Runs an admin command signed by an admin key of the space.
    Admin {
        #[serde(flatten)]
        action: RelayAdminAction,
        counter: u64,
        signer: String,
        signature: String,
    },
    /// Asks for the space's writer ranges, epoch, usage, and admin set.
    State,
    /// Stores records sealed under the current epoch.
    Push { records: Vec<String> },
    /// Fetches records after a version vector.
    Pull { have: Vec<SyncSeq> },
    /// Waits until the relay holds records after a version vector.
    Wait { have: Vec<SyncSeq> },
    /// Fetches the key epoch entries registered after `after`.
    Epochs { after: u32 },
    /// Registers `epoch`, the current epoch plus one, with its access key and key epoch entry.
    RegisterEpoch {
        epoch: u32,
        access_key: String,
        entry: String,
        signature: String,
    },
    /// Asks which assets the relay holds completely.
    AssetQuery { assets: Vec<String> },
    /// Stores one sealed asset chunk.
    AssetPut { chunk: String },
    /// Fetches one sealed asset chunk.
    AssetGet { asset: String, index: u32 },
}

impl RelayRequest {
    /// Whether the relay accepts this request before the connection authenticates.
    pub fn allowed_before_auth(&self) -> bool {
        matches!(
            self,
            Self::Hello { .. } | Self::Auth { .. } | Self::Claim { .. } | Self::Admin { .. }
        )
    }
}

impl Frame for RelayRequest {
    const LABEL: &'static str = "relay request";

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Hello { space, .. } => check_hex("relay space", space, SPACE_TEXT_LENGTH),
            Self::Auth { signature, .. } => {
                check_fixed_text("relay signature", signature, SIGNATURE_TEXT_LENGTH)
            }
            Self::Claim {
                access_key,
                admins,
                signer,
                signature,
                ..
            } => {
                check_fixed_text("relay access key", access_key, KEY_TEXT_LENGTH)?;
                if admins.is_empty() || admins.len() > MAX_RELAY_ADMIN_KEYS {
                    return Err("relay admin set is invalid".to_string());
                }
                for admin in admins {
                    check_fixed_text("relay admin key", admin, KEY_TEXT_LENGTH)?;
                }
                check_fixed_text("relay admin key", signer, KEY_TEXT_LENGTH)?;
                check_fixed_text("relay signature", signature, SIGNATURE_TEXT_LENGTH)
            }
            Self::Admin {
                action,
                signer,
                signature,
                ..
            } => {
                if let RelayAdminAction::AddKey { key } | RelayAdminAction::RemoveKey { key } =
                    action
                {
                    check_fixed_text("relay admin key", key, KEY_TEXT_LENGTH)?;
                }
                if let RelayAdminAction::RelayEpochRepair { access_key, .. } = action {
                    check_fixed_text("relay access key", access_key, KEY_TEXT_LENGTH)?;
                }
                check_fixed_text("relay admin key", signer, KEY_TEXT_LENGTH)?;
                check_fixed_text("relay signature", signature, SIGNATURE_TEXT_LENGTH)
            }
            Self::State | Self::Epochs { .. } => Ok(()),
            Self::Push { records } => {
                if records.is_empty() || records.len() > MAX_RELAY_PAGE_RECORDS {
                    return Err("relay page has an invalid record count".to_string());
                }
                let mut total = 0usize;
                for record in records {
                    check_bounded_text("relay record", record, MAX_RECORD_TEXT_BYTES)?;
                    total = total.saturating_add(record.len());
                }
                if total > base64_length(RELAY_PAGE_BYTES) {
                    return Err("relay page is too large".to_string());
                }
                Ok(())
            }
            Self::Pull { have } | Self::Wait { have } => check_vector(have),
            Self::RegisterEpoch {
                access_key,
                entry,
                signature,
                ..
            } => {
                check_fixed_text("relay access key", access_key, KEY_TEXT_LENGTH)?;
                check_bounded_text("relay epoch entry", entry, MAX_ENTRY_TEXT_BYTES)?;
                check_fixed_text("relay signature", signature, SIGNATURE_TEXT_LENGTH)
            }
            Self::AssetQuery { assets } => {
                if assets.is_empty() || assets.len() > MAX_RELAY_ASSET_QUERY {
                    return Err("relay asset query has an invalid size".to_string());
                }
                for asset in assets {
                    check_hex("relay asset", asset, ASSET_TEXT_LENGTH)?;
                }
                Ok(())
            }
            Self::AssetPut { chunk } => {
                check_bounded_text("relay asset chunk", chunk, MAX_CHUNK_TEXT_BYTES)
            }
            Self::AssetGet { asset, .. } => check_hex("relay asset", asset, ASSET_TEXT_LENGTH),
        }
    }
}

/// Why a relay refused a request.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayRefusalCode {
    /// The protocol version differs from [`RELAY_PROTOCOL_VERSION`].
    Version,
    /// The request is malformed or not allowed in the connection's state.
    BadRequest,
    /// The relay holds no space with this id.
    UnknownSpace,
    /// The space was already claimed.
    SpaceExists,
    /// The request needs an authenticated connection.
    Unauthenticated,
    /// The credential's epoch is older than the space's current epoch, or no longer known.
    StaleEpoch,
    /// The credential's epoch is newer than any registered epoch, or not the next one.
    UnknownEpoch,
    /// The access signature does not verify.
    Authentication,
    /// The setup token is unknown, expired, or already used.
    Token,
    /// The admin signature does not verify or the signer is not an admin.
    AdminSignature,
    /// The admin counter is not above the last accepted counter.
    AdminCounter,
    /// The admin set change is not allowed.
    AdminSet,
    /// A record or chunk does not parse or names another space or epoch.
    BadRecord,
    /// The space already holds the most writers it may.
    WriterLimit,
    /// The requested item is not stored.
    NotFound,
    /// An asset chunk disagrees with the stored chunks of the asset.
    AssetMismatch,
    /// The relay is at its connection budget.
    Busy,
}

/// What happened to one pushed record.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayPushOutcome {
    /// The record was stored.
    Stored,
    /// The same record was already stored.
    Duplicate,
    /// A different record already holds the writer's sequence.
    SlotTaken,
    /// Earlier records of the writer are missing.
    Gap,
    /// The sequence is below the writer's floor; retention already trimmed it.
    Trimmed,
}

/// What happened to one stored asset chunk.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayAssetOutcome {
    /// The chunk was stored.
    Stored,
    /// The same chunk was already stored.
    Duplicate,
    /// A different chunk already holds the index.
    SlotTaken,
}

/// The stored sequence range of one writer: records `floor` up to `next - 1`.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RelayWriterRange {
    /// Writer id in hexadecimal.
    pub writer: String,
    /// Lowest stored sequence; earlier records were trimmed.
    pub floor: u64,
    /// Sequence the writer's next record takes.
    pub next: u64,
}

/// A registered key epoch entry.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RelayEpochEntry {
    /// Epoch the entry introduced.
    pub epoch: u32,
    /// The key epoch authorization entry as the registering device sent it.
    pub entry: String,
}

/// A response a relay sends to a device.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum RelayResponse {
    /// Accepts the hello.
    Welcome { protocol_version: u16 },
    /// Accepts the credential. A stale connection may read but not write.
    Authenticated {
        epoch: u32,
        current_epoch: u32,
        stale: bool,
    },
    /// The space was created.
    Claimed { epoch: u32 },
    /// The admin command ran.
    AdminApplied { counter: u64 },
    /// State of the space.
    State {
        current_epoch: u32,
        writers: Vec<RelayWriterRange>,
        stored_bytes: u64,
        quota_bytes: u64,
        admins: Vec<String>,
        admin_counter: u64,
    },
    /// One outcome per pushed record, in order.
    Pushed { outcomes: Vec<RelayPushOutcome> },
    /// Records after the requested vector.
    Records {
        records: Vec<String>,
        /// Writers whose next wanted record was trimmed, with their current floor as `seq`.
        behind_floor: Vec<SyncSeq>,
        more: bool,
    },
    /// Whether records arrived before the wait timed out.
    Waited { ready: bool },
    /// Key epoch entries after the requested epoch.
    Epochs {
        current_epoch: u32,
        entries: Vec<RelayEpochEntry>,
        more: bool,
    },
    /// The epoch was registered and is now current.
    EpochRegistered { epoch: u32 },
    /// Assets the relay holds completely.
    AssetPresence { assets: Vec<String> },
    /// Outcome of an asset chunk put.
    AssetStored { outcome: RelayAssetOutcome },
    /// One sealed asset chunk.
    AssetChunk { chunk: String },
    /// The request was refused.
    Refused {
        code: RelayRefusalCode,
        message: String,
        /// For [`RelayRefusalCode::AdminCounter`], the lowest counter the relay accepts next, so
        /// an admin that lost track can sign again. It is replay state, not a secret.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_counter: Option<u64>,
    },
}

impl RelayResponse {
    /// A refusal with a message cut to its bound.
    pub fn refused(code: RelayRefusalCode, message: &str) -> Self {
        let mut end = message.len().min(MAX_RELAY_REFUSAL_MESSAGE_BYTES);
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        Self::Refused {
            code,
            message: message[..end].to_string(),
            next_counter: None,
        }
    }

    /// A refusal of an admin counter that is not above `last_counter`, naming the next counter
    /// the relay accepts.
    pub fn stale_admin_counter(last_counter: u64) -> Self {
        Self::Refused {
            code: RelayRefusalCode::AdminCounter,
            message: "admin counter was already used".to_string(),
            next_counter: Some(last_counter.saturating_add(1)),
        }
    }
}

impl Frame for RelayResponse {
    const LABEL: &'static str = "relay response";

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Welcome { .. }
            | Self::Authenticated { .. }
            | Self::Claimed { .. }
            | Self::AdminApplied { .. }
            | Self::Waited { .. }
            | Self::EpochRegistered { .. }
            | Self::AssetStored { .. } => Ok(()),
            Self::State {
                writers, admins, ..
            } => {
                if writers.len() > MAX_WRITERS_PER_SPACE {
                    return Err("relay state has too many writers".to_string());
                }
                for range in writers {
                    check_hex("relay writer", &range.writer, WRITER_TEXT_LENGTH)?;
                }
                if admins.len() > MAX_RELAY_ADMIN_KEYS {
                    return Err("relay state has too many admins".to_string());
                }
                for admin in admins {
                    check_fixed_text("relay admin key", admin, KEY_TEXT_LENGTH)?;
                }
                Ok(())
            }
            Self::Pushed { outcomes } => {
                if outcomes.len() > MAX_RELAY_PAGE_RECORDS {
                    return Err("relay push outcome has too many records".to_string());
                }
                Ok(())
            }
            Self::Records {
                records,
                behind_floor,
                ..
            } => {
                if records.len() > MAX_RELAY_PAGE_RECORDS {
                    return Err("relay page has too many records".to_string());
                }
                let mut total = 0usize;
                for record in records {
                    check_bounded_text("relay record", record, MAX_RECORD_TEXT_BYTES)?;
                    total = total.saturating_add(record.len());
                }
                if total > base64_length(RELAY_PAGE_BYTES) {
                    return Err("relay page is too large".to_string());
                }
                check_vector(behind_floor)
            }
            Self::Epochs { entries, .. } => {
                let mut total = 0usize;
                for entry in entries {
                    check_bounded_text("relay epoch entry", &entry.entry, MAX_ENTRY_TEXT_BYTES)?;
                    total = total.saturating_add(entry.entry.len());
                }
                if total > MAX_ENTRY_TEXT_BYTES.max(base64_length(RELAY_PAGE_BYTES)) {
                    return Err("relay epoch page is too large".to_string());
                }
                Ok(())
            }
            Self::AssetPresence { assets } => {
                if assets.len() > MAX_RELAY_ASSET_QUERY {
                    return Err("relay asset presence is too large".to_string());
                }
                for asset in assets {
                    check_hex("relay asset", asset, ASSET_TEXT_LENGTH)?;
                }
                Ok(())
            }
            Self::AssetChunk { chunk } => {
                check_bounded_text("relay asset chunk", chunk, MAX_CHUNK_TEXT_BYTES)
            }
            Self::Refused { message, .. } => {
                if message.len() > MAX_RELAY_REFUSAL_MESSAGE_BYTES {
                    return Err("relay refusal message is too long".to_string());
                }
                Ok(())
            }
        }
    }
}

/// A one-time secret that lets an admin claim a space on a relay. Its `Debug` output is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct RelaySetupToken([u8; RELAY_SETUP_TOKEN_BYTES]);

impl RelaySetupToken {
    /// Draws a fresh token from the system random source.
    pub fn generate() -> Result<Self, String> {
        let mut bytes = [0u8; RELAY_SETUP_TOKEN_BYTES];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes)
            .map_err(|_| "generate relay setup token".to_string())?;
        Ok(Self(bytes))
    }

    /// Wraps raw token bytes.
    pub const fn from_bytes(bytes: [u8; RELAY_SETUP_TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    /// Parses the wire form.
    pub fn from_text(text: &str) -> Result<Self, String> {
        decode_relay_fixed::<RELAY_SETUP_TOKEN_BYTES>("relay setup token", text).map(Self)
    }

    /// Raw token bytes.
    pub const fn as_bytes(&self) -> &[u8; RELAY_SETUP_TOKEN_BYTES] {
        &self.0
    }

    /// Wire form for a claim request.
    pub fn to_text(&self) -> String {
        encode_relay_bytes(&self.0)
    }
}

impl fmt::Debug for RelaySetupToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RelaySetupToken(<redacted>)")
    }
}

impl Serialize for RelaySetupToken {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for RelaySetupToken {
    /// Reads the wire text, refusing any length other than the token's before decoding.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_text(&text).map_err(serde::de::Error::custom)
    }
}

/// What a relay operator hands to an admin device: where the relay listens, how to pin its TLS
/// certificate, and the one-time token that claims a space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelaySetupCode {
    /// Addresses as `host:port`, from 1 to [`MAX_SERVER_ENDPOINTS`].
    pub endpoints: Vec<String>,
    /// SHA-256 of the relay's DER certificate.
    pub certificate_fingerprint: [u8; TLS_FINGERPRINT_BYTES],
    /// One-time claim token.
    pub token: RelaySetupToken,
}

impl RelaySetupCode {
    fn validate(&self) -> Result<(), String> {
        if self.endpoints.is_empty() || self.endpoints.len() > MAX_SERVER_ENDPOINTS {
            return Err("relay setup code has an invalid endpoint count".to_string());
        }
        for endpoint in &self.endpoints {
            if !ganbaru_sync_contracts::authorization::validate_server_endpoint(endpoint) {
                return Err("relay setup code has an invalid endpoint".to_string());
            }
        }
        Ok(())
    }

    /// Binary form carried by the QR code.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(RELAY_SETUP_MAGIC);
        bytes.push(RELAY_SETUP_VERSION);
        bytes.extend_from_slice(&self.certificate_fingerprint);
        bytes.extend_from_slice(self.token.as_bytes());
        bytes.push(u8::try_from(self.endpoints.len()).map_err(|_| "too many endpoints")?);
        for endpoint in &self.endpoints {
            bytes.push(u8::try_from(endpoint.len()).map_err(|_| "endpoint is too long")?);
            bytes.extend_from_slice(endpoint.as_bytes());
        }
        Ok(bytes)
    }

    /// Parses the binary form.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let invalid = || "relay setup code is invalid".to_string();
        let rest = bytes.strip_prefix(RELAY_SETUP_MAGIC).ok_or_else(invalid)?;
        let (&version, rest) = rest.split_first().ok_or_else(invalid)?;
        if version != RELAY_SETUP_VERSION {
            return Err("relay setup code uses an unsupported version".to_string());
        }
        let (fingerprint, rest) = rest
            .split_first_chunk::<TLS_FINGERPRINT_BYTES>()
            .ok_or_else(invalid)?;
        let (token, rest) = rest
            .split_first_chunk::<RELAY_SETUP_TOKEN_BYTES>()
            .ok_or_else(invalid)?;
        let (&count, mut rest) = rest.split_first().ok_or_else(invalid)?;
        if count == 0 || usize::from(count) > MAX_SERVER_ENDPOINTS {
            return Err(invalid());
        }
        let mut endpoints = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let (&length, tail) = rest.split_first().ok_or_else(invalid)?;
            let length = usize::from(length);
            if tail.len() < length {
                return Err(invalid());
            }
            let (endpoint, tail) = tail.split_at(length);
            let endpoint = std::str::from_utf8(endpoint).map_err(|_| invalid())?;
            endpoints.push(endpoint.to_string());
            rest = tail;
        }
        if !rest.is_empty() {
            return Err(invalid());
        }
        let code = Self {
            endpoints,
            certificate_fingerprint: *fingerprint,
            token: RelaySetupToken(*token),
        };
        code.validate()?;
        Ok(code)
    }

    /// Text form an admin can paste.
    pub fn to_text(&self) -> Result<String, String> {
        Ok(format!(
            "{RELAY_SETUP_TEXT_PREFIX}{}",
            encode_relay_bytes(&self.to_bytes()?)
        ))
    }

    /// Parses the text form.
    pub fn from_text(text: &str) -> Result<Self, String> {
        let encoded = text
            .trim()
            .strip_prefix(RELAY_SETUP_TEXT_PREFIX)
            .ok_or_else(|| "relay setup code is invalid".to_string())?;
        let max_bytes = 4
            + 1
            + TLS_FINGERPRINT_BYTES
            + RELAY_SETUP_TOKEN_BYTES
            + 1
            + MAX_SERVER_ENDPOINTS * (1 + usize::from(u8::MAX));
        let bytes = decode_relay_bytes("relay setup code", encoded, max_bytes)?;
        Self::from_bytes(&bytes)
    }
}

/// Compares two byte strings in time that depends only on their lengths.
pub fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right));
    std::hint::black_box(difference) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{read_frame, write_frame};
    use ganbaru_sync_contracts::{AdminKeyPair, SpaceKey};
    use tokio::io::AsyncWriteExt;

    fn binding() -> RelayBinding {
        RelayBinding {
            exporter: [7; RELAY_EXPORTER_BYTES],
            space: SpaceId::from_bytes([3; 16]),
        }
    }

    #[tokio::test]
    async fn relay_frames_round_trip_through_the_generic_codec() {
        let request = RelayRequest::Push {
            records: vec![encode_relay_bytes(&[1, 2, 3]), encode_relay_bytes(&[4; 90])],
        };
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        write_frame(&mut client, &request).await.unwrap();
        let received: RelayRequest = read_frame(&mut server, super::super::MAX_CONTROL_BYTES)
            .await
            .unwrap();
        assert_eq!(received, request);
    }

    #[tokio::test]
    async fn oversized_and_empty_frames_are_refused_before_reading_the_body() {
        let (mut client, mut server) = tokio::io::duplex(1024);
        let limit = u32::try_from(super::super::MAX_UNAUTHENTICATED_CONTROL_BYTES).unwrap();
        client.write_u32(limit + 1).await.unwrap();
        let error = read_frame::<_, RelayRequest>(
            &mut server,
            super::super::MAX_UNAUTHENTICATED_CONTROL_BYTES,
        )
        .await
        .unwrap_err();
        assert!(error.contains("invalid size"), "{error}");

        let (mut client, mut server) = tokio::io::duplex(1024);
        client.write_u32(0).await.unwrap();
        let error = read_frame::<_, RelayRequest>(&mut server, 1024)
            .await
            .unwrap_err();
        assert!(error.contains("invalid size"), "{error}");
    }

    #[tokio::test]
    async fn invalid_fields_are_refused_on_write_and_read() {
        let bad = RelayRequest::Hello {
            protocol_version: RELAY_PROTOCOL_VERSION,
            space: "ZZ".to_string(),
        };
        let (mut client, mut server) = tokio::io::duplex(1024);
        assert!(write_frame(&mut client, &bad).await.is_err());

        let json = serde_json::to_vec(&bad).unwrap();
        client
            .write_u32(u32::try_from(json.len()).unwrap())
            .await
            .unwrap();
        client.write_all(&json).await.unwrap();
        let error = read_frame::<_, RelayRequest>(&mut server, 1024)
            .await
            .unwrap_err();
        assert!(error.contains("relay space is invalid"), "{error}");
    }

    #[test]
    fn push_pages_are_bounded_by_count_and_size() {
        let empty = RelayRequest::Push { records: vec![] };
        assert!(empty.validate().is_err());
        let too_many = RelayRequest::Push {
            records: vec!["AA".to_string(); MAX_RELAY_PAGE_RECORDS + 1],
        };
        assert!(too_many.validate().is_err());
        let large = "A".repeat(MAX_RECORD_TEXT_BYTES);
        let too_large = RelayRequest::Push {
            records: vec![large.clone(), large],
        };
        assert!(too_large.validate().is_err());
        let not_base64 = RelayRequest::Push {
            records: vec!["a+b".to_string()],
        };
        assert!(not_base64.validate().is_err());
    }

    #[test]
    fn signatures_bind_the_exporter_space_and_command() {
        let key = SpaceKey::generate(4).unwrap();
        let space = binding().space;
        let access = key.relay_access_key(&space).unwrap();
        let signature = access.sign(RELAY_AUTH_DOMAIN, &binding().auth_payload(4));
        let public = access.public_key();
        assert!(public.verify(RELAY_AUTH_DOMAIN, &binding().auth_payload(4), &signature));
        assert!(!public.verify(RELAY_AUTH_DOMAIN, &binding().auth_payload(5), &signature));
        let other = RelayBinding {
            exporter: [8; RELAY_EXPORTER_BYTES],
            ..binding()
        };
        assert!(!public.verify(RELAY_AUTH_DOMAIN, &other.auth_payload(4), &signature));
        assert!(!public.verify(RELAY_EPOCH_DOMAIN, &binding().auth_payload(4), &signature));

        let (_, admin) = AdminKeyPair::generate().unwrap();
        let added = admin.public_key();
        let add = binding().admin_payload(2, &RelayAdminCommand::AddKey(&added));
        let remove = binding().admin_payload(2, &RelayAdminCommand::RemoveKey(&added));
        let later = binding().admin_payload(3, &RelayAdminCommand::AddKey(&added));
        assert_ne!(add, remove);
        assert_ne!(add, later);

        let next = SpaceKey::generate(5)
            .unwrap()
            .relay_access_key(&space)
            .unwrap()
            .public_key();
        let repair = |counter: u64, epoch: u32, access_key| {
            binding().admin_payload(
                counter,
                &RelayAdminCommand::RelayEpochRepair { epoch, access_key },
            )
        };
        let repaired = repair(2, 5, &next);
        assert_ne!(repaired, repair(2, 4, &next));
        assert_ne!(repaired, repair(2, 5, &public));
        assert_ne!(repaired, repair(3, 5, &next));
        assert_ne!(
            repaired,
            binding().admin_payload(2, &RelayAdminCommand::Wipe)
        );
        assert_ne!(
            repaired,
            other.admin_payload(
                2,
                &RelayAdminCommand::RelayEpochRepair {
                    epoch: 5,
                    access_key: &next,
                }
            )
        );
    }

    #[test]
    fn epoch_repairs_validate_their_access_key_and_stale_counters_name_the_next_one() {
        let repair = |access_key: String| RelayRequest::Admin {
            action: RelayAdminAction::RelayEpochRepair {
                epoch: 3,
                access_key,
            },
            counter: 7,
            signer: encode_relay_bytes(&[1; PUBLIC_KEY_BYTES]),
            signature: encode_relay_bytes(&[2; SIGNATURE_BYTES]),
        };
        assert!(
            repair(encode_relay_bytes(&[3; PUBLIC_KEY_BYTES]))
                .validate()
                .is_ok()
        );
        assert!(repair(encode_relay_bytes(&[3; 31])).validate().is_err());
        assert!(repair("!".repeat(KEY_TEXT_LENGTH)).validate().is_err());

        let refusal = RelayResponse::stale_admin_counter(9);
        let RelayResponse::Refused {
            code, next_counter, ..
        } = &refusal
        else {
            panic!("expected a refusal");
        };
        assert_eq!(*code, RelayRefusalCode::AdminCounter);
        assert_eq!(*next_counter, Some(10));
        let json = serde_json::to_string(&refusal).unwrap();
        assert_eq!(
            serde_json::from_str::<RelayResponse>(&json).unwrap(),
            refusal
        );
        let plain =
            serde_json::to_string(&RelayResponse::refused(RelayRefusalCode::Busy, "busy")).unwrap();
        assert!(!plain.contains("counter"), "{plain}");
    }

    #[test]
    fn setup_codes_round_trip_and_reject_damage() {
        let code = RelaySetupCode {
            endpoints: vec![
                "relay.example.org:43823".to_string(),
                "192.0.2.4:443".to_string(),
            ],
            certificate_fingerprint: [9; TLS_FINGERPRINT_BYTES],
            token: RelaySetupToken::generate().unwrap(),
        };
        let text = code.to_text().unwrap();
        assert!(text.starts_with(RELAY_SETUP_TEXT_PREFIX));
        assert_eq!(RelaySetupCode::from_text(&text).unwrap(), code);
        let bytes = code.to_bytes().unwrap();
        assert!(RelaySetupCode::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(RelaySetupCode::from_bytes(&trailing).is_err());
        let mut version = bytes;
        version[4] = 2;
        assert!(RelaySetupCode::from_bytes(&version).is_err());
        assert!(RelaySetupCode::from_text("ganbaru-relay:").is_err());
        assert!(!format!("{code:?}").contains(&code.token.to_text()));
        let claim = RelayRequest::Claim {
            token: code.token.clone(),
            epoch: 1,
            access_key: encode_relay_bytes(&[1; PUBLIC_KEY_BYTES]),
            admins: vec![encode_relay_bytes(&[2; PUBLIC_KEY_BYTES])],
            counter: 1,
            signer: encode_relay_bytes(&[2; PUBLIC_KEY_BYTES]),
            signature: encode_relay_bytes(&[3; SIGNATURE_BYTES]),
        };
        assert!(!format!("{claim:?}").contains(&code.token.to_text()));
        let wire = serde_json::to_vec(&claim).unwrap();
        assert_eq!(
            serde_json::from_slice::<RelayRequest>(&wire).unwrap(),
            claim
        );
        let short = String::from_utf8(wire)
            .unwrap()
            .replace(&code.token.to_text(), "AAAA");
        assert!(serde_json::from_str::<RelayRequest>(&short).is_err());
    }

    #[test]
    fn refusal_messages_are_cut_at_a_character_boundary() {
        let message = "é".repeat(MAX_RELAY_REFUSAL_MESSAGE_BYTES);
        let RelayResponse::Refused { message, .. } =
            RelayResponse::refused(RelayRefusalCode::BadRequest, &message)
        else {
            panic!("expected a refusal");
        };
        assert!(message.len() <= MAX_RELAY_REFUSAL_MESSAGE_BYTES);
    }

    #[test]
    fn constant_time_equal_compares_contents_and_lengths() {
        assert!(constant_time_equal(b"abc", b"abc"));
        assert!(!constant_time_equal(b"abc", b"abd"));
        assert!(!constant_time_equal(b"abc", b"ab"));
    }
}

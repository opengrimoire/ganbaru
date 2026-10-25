//! Bounded wire contracts for the local vault handoff service.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod sync;

pub use sync::{
    DISCOVERY_KEY_BYTES, MAX_COORDINATOR_ENDPOINTS, MAX_SYNC_ASSET_BYTES, MAX_SYNC_ASSETS,
    MAX_SYNC_EVIDENCE_OPS, MAX_SYNC_HASHES, SYNC_ASSET_CHUNK_BYTES, SYNC_PAGE_BYTES, SyncAsset,
    SyncAssetKey, SyncProbe, SyncRefusal, SyncRefusalCode, SyncSeq, decode_sync_charter,
    decode_sync_device_key, decode_sync_proposal, encode_sync_charter, encode_sync_device_key,
    encode_sync_proposal, validate_coordinator_endpoint, validate_coordinator_endpoints,
    validate_discovery_key,
};

pub const PROTOCOL_VERSION: u16 = 13;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
/// Frame bound before the peer presents a linked certificate. Enrollment (with its 8 KiB
/// certificate) and contact requests fit well inside it.
pub const MAX_UNAUTHENTICATED_CONTROL_BYTES: usize = 16 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 160;
pub const MAX_DEVICE_LABEL_BYTES: usize = 128;
const MAX_APP_VERSION_BYTES: usize = 64;
pub const TRANSFER_CHUNK_BYTES: usize = 64 * 1024;
/// LAN port the coordinator of a production build listens on.
pub const COORDINATOR_PORT: u16 = 43_821;
/// LAN port the coordinator of a development build listens on.
pub const DEVELOPMENT_COORDINATOR_PORT: u16 = 43_822;
const PAIRING_QR_MAGIC: &[u8; 4] = b"GBQ\x01";
/// Base64url text of a 64-byte Ed25519 signature.
const SIGNATURE_TEXT_LENGTH: usize = 86;
/// Base64url text of a 16-byte card nonce.
const CARD_NONCE_TEXT_LENGTH: usize = 22;
/// Upper bound on the pasteable text form of a contact card.
const MAX_CARD_TEXT_BYTES: usize =
    ganbaru_contacts::CARD_TEXT_PREFIX.len() + ganbaru_contacts::MAX_CARD_BYTES.div_ceil(3) * 4;
/// Upper bound on a base64url PKCS#8 person key.
const MAX_PERSON_KEY_TEXT_BYTES: usize = 256;
/// Tolerated clock skew for signed status requests.
const STATUS_REPLAY_WINDOW_MS: i64 = 5 * 60 * 1000;

/// Outcome of a contact request as reported by its recipient.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ContactRequestOutcome {
    Pending,
    Accepted,
    Declined,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Computer,
    Phone,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffCompatibility {
    pub app_version: String,
    pub database_schema_sha256: String,
}

impl HandoffCompatibility {
    pub fn validate(&self) -> Result<(), String> {
        if self.app_version.trim().is_empty()
            || self.app_version.len() > MAX_APP_VERSION_BYTES
            || self.app_version.chars().any(char::is_control)
        {
            return Err("application version is invalid".to_string());
        }
        validate_sha256(&self.database_schema_sha256)
            .map_err(|_| "database compatibility fingerprint is invalid".to_string())
    }
}

pub fn ensure_compatible(
    local: &HandoffCompatibility,
    remote: &HandoffCompatibility,
) -> Result<(), String> {
    local.validate()?;
    remote.validate()?;
    if local.database_schema_sha256 != remote.database_schema_sha256 {
        return Err(format!(
            "handoff compatibility mismatch between Ganbaru {} and {}; update Ganbaru on both devices",
            local.app_version, remote.app_version
        ));
    }
    Ok(())
}

/// A pairing invitation secret. It serializes as the plain string, and `Debug` never prints it.
#[derive(Clone, Deserialize, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct InvitationSecret(String);

impl From<String> for InvitationSecret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::ops::Deref for InvitationSecret {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for InvitationSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("InvitationSecret([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInvitation {
    pub protocol_version: u16,
    pub compatibility: HandoffCompatibility,
    pub invitation_id: String,
    pub secret: InvitationSecret,
    pub endpoint: String,
    pub coordinator_fingerprint: String,
    pub coordinator_device_id: String,
    pub vault_id: String,
    pub generation: u64,
    pub expires_at_ms: i64,
}

impl PairingInvitation {
    pub fn validate(&self, now_ms: i64) -> Result<(), String> {
        validate_protocol(self.protocol_version)?;
        self.compatibility.validate()?;
        validate_identifier("invitation id", &self.invitation_id)?;
        validate_identifier("invitation secret", &self.secret)?;
        validate_certificate_fingerprint("coordinator fingerprint", &self.coordinator_fingerprint)?;
        validate_identifier("coordinator device id", &self.coordinator_device_id)?;
        validate_identifier("vault id", &self.vault_id)?;
        let endpoint = self
            .endpoint
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "pairing invitation endpoint is invalid".to_string())?;
        crate::dial::validate_dial_endpoint(endpoint)
            .map_err(|error| format!("pairing invitation endpoint is refused: {error}"))?;
        if self.expires_at_ms <= now_ms {
            return Err("pairing invitation has expired".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleMetadata {
    pub protocol_version: u16,
    pub compatibility: HandoffCompatibility,
    pub vault_id: String,
    pub device_id: String,
    pub transfer_id: String,
    pub generation: u64,
    pub archive_bytes: u64,
    pub archive_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BundlePurpose {
    Ownership,
    Refresh,
}

impl BundleMetadata {
    pub fn validate(&self) -> Result<(), String> {
        validate_protocol(self.protocol_version)?;
        self.compatibility.validate()?;
        validate_identifier("vault id", &self.vault_id)?;
        validate_identifier("device id", &self.device_id)?;
        validate_identifier("transfer id", &self.transfer_id)?;
        validate_sha256(&self.archive_sha256)?;
        if self.archive_bytes == 0 || self.archive_bytes > MAX_ARCHIVE_BYTES {
            return Err(format!(
                "bundle size must be between 1 and {MAX_ARCHIVE_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ControlMessage {
    Enroll {
        protocol_version: u16,
        invitation_id: String,
        secret: InvitationSecret,
        vault_id: String,
        device_id: String,
        device_label: String,
        device_kind: DeviceKind,
        device_certificate: String,
        /// Device key that certifies the device's sync writers, which the anchor enrolls.
        sync_device_key: String,
    },
    /// The coordinator linked the device. `charter` is the vault's space charter and
    /// `person_key` the person key that signed it; both are present once the coordinator founded
    /// the space, and both are absent when founding failed and the device learns the charter from
    /// the vault bundle instead.
    Enrolled {
        protocol_version: u16,
        coordinator_device_id: String,
        #[serde(default)]
        coordinator_device_label: Option<String>,
        #[serde(default)]
        charter: Option<String>,
        #[serde(default)]
        person_key: Option<String>,
    },
    RequestBundle {
        protocol_version: u16,
        compatibility: HandoffCompatibility,
        vault_id: String,
        device_id: String,
        generation: u64,
        purpose: BundlePurpose,
    },
    BundlePrepared {
        metadata: BundleMetadata,
        purpose: BundlePurpose,
    },
    BundlePending {
        protocol_version: u16,
        owner_device_id: String,
        generation: u64,
    },
    DownloadBundle {
        metadata: BundleMetadata,
    },
    UploadBundle {
        metadata: BundleMetadata,
        source_device_id: String,
        purpose: BundlePurpose,
    },
    BundleMetadata {
        metadata: BundleMetadata,
    },
    ResumeAt {
        offset: u64,
    },
    BundleComplete {
        transfer_id: String,
    },
    BundleStaged {
        transfer_id: String,
    },
    CommitStagedOwnership {
        metadata: BundleMetadata,
    },
    OwnershipGrant {
        protocol_version: u16,
        vault_id: String,
        transfer_id: String,
        owner_device_id: String,
        generation: u64,
    },
    CommitUploadedOwnership {
        metadata: BundleMetadata,
        source_device_id: String,
    },
    ActivationComplete {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        transfer_id: String,
        generation: u64,
        purpose: BundlePurpose,
    },
    ActivationAcknowledged {
        transfer_id: String,
    },
    CancelTransfer {
        transfer_id: String,
    },
    TransferCancelled {
        transfer_id: String,
    },
    RefreshRequest {
        protocol_version: u16,
        compatibility: HandoffCompatibility,
        vault_id: String,
        device_id: String,
        generation: u64,
    },
    RefreshStatus {
        available: bool,
        generation: u64,
        transfer_id: Option<String>,
        requested_upload: Option<BundlePurpose>,
    },
    /// Unauthenticated: a person asks the recipient (whose card they hold) to become a contact.
    ContactRequest {
        protocol_version: u16,
        recipient_card_nonce: String,
        requester_card: String,
        request_id: String,
        signature: String,
    },
    ContactRequestReceived {
        request_id: String,
    },
    /// Unauthenticated: the requester asks whether a request was accepted.
    ContactRequestStatus {
        protocol_version: u16,
        request_id: String,
        requester_public_key: String,
        issued_at_ms: i64,
        signature: String,
    },
    ContactRequestState {
        state: ContactRequestOutcome,
        recipient_card: Option<String>,
    },
    /// Authenticated: a linked device asks the coordinator for the person's signing key.
    PersonKeyRequest {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
    },
    PersonKeyRelease {
        public_key: String,
        private_key_pkcs8: String,
    },
    /// Authenticated: a linked device compares its stored operations with the hub's.
    SyncHello {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        manifest_version: u16,
        stored: Vec<SyncSeq>,
        probe: Option<SyncProbe>,
    },
    /// The hub's stored operations and its hash at the probed sequence, when it holds it, plus
    /// the listener endpoints the coordinator reports now and the vault's discovery key, so a
    /// linked device follows address changes without relinking. `stable` lists the operations
    /// every counted device holds, and `bootstrap_required` tells the device that the hub no
    /// longer holds operations it lacks. `forked` lists the writers the hub holds fork evidence
    /// for, each with the sequence where its copies differ.
    SyncState {
        manifest_version: u16,
        stored: Vec<SyncSeq>,
        probe_hash: Option<String>,
        coordinator_endpoints: Vec<String>,
        discovery_key: String,
        stable: Vec<SyncSeq>,
        bootstrap_required: bool,
        forked: Vec<SyncSeq>,
    },
    /// Authenticated: operations the hub lacks, in an order that keeps chains contiguous.
    SyncPush {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        ops: Vec<String>,
    },
    SyncPushResult {
        stored: Vec<SyncSeq>,
        refusal: Option<SyncRefusal>,
    },
    /// Authenticated: a page of operations past `known`.
    SyncPull {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        known: Vec<SyncSeq>,
        max_bytes: u32,
    },
    SyncOps {
        ops: Vec<String>,
        more: bool,
    },
    /// Authenticated: answered with `SyncState` once the hub stores operations past `known`,
    /// or after a bounded wait.
    SyncWait {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        known: Vec<SyncSeq>,
    },
    /// Authenticated: hashes of a writer's chain from `from_seq`, to locate a fork.
    SyncHashes {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        writer: String,
        from_seq: u64,
        limit: u32,
    },
    /// Consecutive hashes from the requested sequence; fewer when the chain ends.
    SyncHashList {
        hashes: Vec<String>,
    },
    /// Authenticated: the fork evidence of `writer`. `evidence` is empty to ask for the hub's
    /// pair, one operation of `writer` that the hub may hold differently at its sequence, or a
    /// pair the hub may lack.
    SyncForkEvidence {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        writer: String,
        evidence: Vec<String>,
    },
    /// The two operations proving that the requested writer forked, or none when the hub holds
    /// no evidence for it.
    SyncForkPair {
        evidence: Vec<String>,
    },
    /// Authenticated: which of these assets the hub holds.
    SyncAssetQuery {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        assets: Vec<SyncAsset>,
    },
    /// The queried assets the hub holds.
    SyncAssetPresence {
        held: Vec<SyncAssetKey>,
    },
    /// Authenticated: a chunk of an asset the hub holds, from `offset`.
    SyncAssetGet {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        asset: SyncAsset,
        offset: u32,
    },
    SyncAssetChunk {
        data: String,
    },
    /// Authenticated: a chunk of an asset the hub lacks, at `offset`.
    SyncAssetPut {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        asset: SyncAsset,
        offset: u32,
        data: String,
    },
    /// The bytes of the asset the hub received so far, where the next chunk starts; the asset
    /// size once the hub verified and stored it.
    SyncAssetReceived {
        received: u32,
    },
    /// Authenticated: an admin-signed proposal for the anchor to sequence. Only the anchor
    /// sequences; any other hub answers with the `not_anchor` rejection.
    SyncPropose {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        proposal: String,
    },
    /// The revision that holds the proposal, or the wire value of the rejection.
    SyncProposed {
        revision: Option<u64>,
        rejection: Option<u8>,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}

impl ControlMessage {
    /// The vault and device a sync request names, or `None` for any other message.
    #[cfg(desktop)]
    pub fn sync_peer(&self) -> Option<(&str, &str)> {
        match self {
            Self::SyncHello {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncPush {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncPull {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncWait {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncHashes {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncForkEvidence {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncAssetQuery {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncAssetGet {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncAssetPut {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncPropose {
                vault_id,
                device_id,
                ..
            } => Some((vault_id, device_id)),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Enroll {
                protocol_version,
                invitation_id,
                secret,
                vault_id,
                device_id,
                device_label,
                device_kind: _,
                device_certificate,
                sync_device_key,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("invitation id", invitation_id)?;
                validate_identifier("invitation secret", secret)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                validate_device_label("device label", device_label)?;
                if device_certificate.is_empty() || device_certificate.len() > 8 * 1024 {
                    return Err("device certificate is invalid".to_string());
                }
                decode_sync_device_key(sync_device_key)?;
            }
            Self::Enrolled {
                protocol_version,
                coordinator_device_id,
                coordinator_device_label,
                charter,
                person_key,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("coordinator device id", coordinator_device_id)?;
                if let Some(label) = coordinator_device_label {
                    validate_device_label("coordinator device label", label)?;
                }
                match (charter, person_key) {
                    (Some(charter), Some(person_key)) => {
                        let (charter, _) = decode_sync_charter(charter, person_key)?;
                        if charter.charter.anchor_device_id != *coordinator_device_id {
                            return Err("the sync charter names a different anchor".to_string());
                        }
                    }
                    (None, None) => {}
                    _ => {
                        return Err(
                            "the sync charter and its person key travel together".to_string()
                        );
                    }
                }
            }
            Self::RequestBundle {
                protocol_version,
                compatibility,
                vault_id,
                device_id,
                ..
            } => {
                validate_protocol(*protocol_version)?;
                compatibility.validate()?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
            }
            Self::BundlePrepared { metadata, .. } | Self::CommitStagedOwnership { metadata } => {
                metadata.validate()?
            }
            Self::BundlePending {
                protocol_version,
                owner_device_id,
                ..
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("owner device id", owner_device_id)?;
            }
            Self::DownloadBundle { metadata } | Self::BundleMetadata { metadata } => {
                metadata.validate()?
            }
            Self::UploadBundle {
                metadata,
                source_device_id,
                ..
            }
            | Self::CommitUploadedOwnership {
                metadata,
                source_device_id,
            } => {
                metadata.validate()?;
                validate_identifier("source device id", source_device_id)?;
                if source_device_id == &metadata.device_id {
                    return Err("bundle source and receiver must be different devices".to_string());
                }
            }
            Self::ResumeAt { offset } => {
                if *offset > MAX_ARCHIVE_BYTES {
                    return Err("resume offset exceeds the archive limit".to_string());
                }
            }
            Self::BundleComplete { transfer_id }
            | Self::BundleStaged { transfer_id }
            | Self::ActivationAcknowledged { transfer_id }
            | Self::CancelTransfer { transfer_id }
            | Self::TransferCancelled { transfer_id } => {
                validate_identifier("transfer id", transfer_id)?;
            }
            Self::OwnershipGrant {
                protocol_version,
                vault_id,
                transfer_id,
                owner_device_id,
                generation,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("transfer id", transfer_id)?;
                validate_identifier("owner device id", owner_device_id)?;
                if *generation == 0 {
                    return Err("ownership grant generation must be positive".to_string());
                }
            }
            Self::ActivationComplete {
                protocol_version,
                vault_id,
                device_id,
                transfer_id,
                generation,
                purpose,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                validate_identifier("transfer id", transfer_id)?;
                if *purpose == BundlePurpose::Ownership && *generation == 0 {
                    return Err("activation generation must be positive".to_string());
                }
            }
            Self::RefreshRequest {
                protocol_version,
                compatibility,
                vault_id,
                device_id,
                ..
            } => {
                validate_protocol(*protocol_version)?;
                compatibility.validate()?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
            }
            Self::RefreshStatus { transfer_id, .. } => {
                if let Some(transfer_id) = transfer_id {
                    validate_identifier("transfer id", transfer_id)?;
                }
            }
            Self::ContactRequest {
                protocol_version,
                recipient_card_nonce,
                requester_card,
                request_id,
                signature,
            } => {
                validate_protocol(*protocol_version)?;
                validate_base64url_text(
                    "recipient card nonce",
                    recipient_card_nonce,
                    CARD_NONCE_TEXT_LENGTH,
                )?;
                validate_card_text("requester card", requester_card)?;
                validate_identifier("contact request id", request_id)?;
                validate_base64url_text(
                    "contact request signature",
                    signature,
                    SIGNATURE_TEXT_LENGTH,
                )?;
            }
            Self::ContactRequestReceived { request_id } => {
                validate_identifier("contact request id", request_id)?;
            }
            Self::ContactRequestStatus {
                protocol_version,
                request_id,
                requester_public_key,
                issued_at_ms,
                signature,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("contact request id", request_id)?;
                validate_base64url_text(
                    "requester public key",
                    requester_public_key,
                    ganbaru_contacts::PUBLIC_KEY_TEXT_LENGTH,
                )?;
                if unix_time_ms().abs_diff(*issued_at_ms) > STATUS_REPLAY_WINDOW_MS.unsigned_abs() {
                    return Err("contact request status is outside the replay window".to_string());
                }
                validate_base64url_text(
                    "contact status signature",
                    signature,
                    SIGNATURE_TEXT_LENGTH,
                )?;
            }
            Self::ContactRequestState {
                state,
                recipient_card,
            } => match (state, recipient_card) {
                (ContactRequestOutcome::Accepted, Some(card)) => {
                    validate_card_text("recipient card", card)?;
                }
                (ContactRequestOutcome::Accepted, None) => {
                    return Err("accepted contact request is missing the card".to_string());
                }
                (_, Some(_)) => {
                    return Err("contact request state carries an unexpected card".to_string());
                }
                (_, None) => {}
            },
            Self::PersonKeyRequest {
                protocol_version,
                vault_id,
                device_id,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
            }
            Self::PersonKeyRelease {
                public_key,
                private_key_pkcs8,
            } => {
                validate_base64url_text(
                    "person public key",
                    public_key,
                    ganbaru_contacts::PUBLIC_KEY_TEXT_LENGTH,
                )?;
                if private_key_pkcs8.is_empty()
                    || private_key_pkcs8.len() > MAX_PERSON_KEY_TEXT_BYTES
                    || !is_base64url(private_key_pkcs8)
                {
                    return Err("person private key is invalid".to_string());
                }
            }
            Self::SyncHello {
                protocol_version,
                vault_id,
                device_id,
                manifest_version: _,
                stored,
                probe,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_vector(stored)?;
                if let Some(probe) = probe {
                    sync::validate_probe(probe)?;
                }
            }
            Self::SyncState {
                manifest_version: _,
                stored,
                probe_hash,
                coordinator_endpoints,
                discovery_key,
                stable,
                bootstrap_required: _,
                forked,
            } => {
                sync::validate_vector(stored)?;
                sync::validate_vector(stable)?;
                sync::validate_vector(forked)?;
                if let Some(hash) = probe_hash {
                    sync::validate_hashes(std::slice::from_ref(hash))?;
                }
                sync::validate_coordinator_endpoints(coordinator_endpoints)?;
                sync::validate_discovery_key(discovery_key)?;
            }
            Self::SyncPush {
                protocol_version,
                vault_id,
                device_id,
                ops,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_ops(ops)?;
            }
            Self::SyncPushResult { stored, refusal } => {
                sync::validate_vector(stored)?;
                if let Some(refusal) = refusal {
                    sync::validate_refusal(refusal)?;
                }
            }
            Self::SyncPull {
                protocol_version,
                vault_id,
                device_id,
                known,
                max_bytes,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_vector(known)?;
                sync::validate_page_bytes(*max_bytes)?;
            }
            Self::SyncOps { ops, more: _ } => sync::validate_ops(ops)?,
            Self::SyncWait {
                protocol_version,
                vault_id,
                device_id,
                known,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_vector(known)?;
            }
            Self::SyncHashes {
                protocol_version,
                vault_id,
                device_id,
                writer,
                from_seq,
                limit,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_writer(writer)?;
                sync::validate_hash_range(*from_seq, *limit)?;
            }
            Self::SyncHashList { hashes } => sync::validate_hashes(hashes)?,
            Self::SyncForkEvidence {
                protocol_version,
                vault_id,
                device_id,
                writer,
                evidence,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_writer(writer)?;
                sync::validate_evidence(evidence, false)?;
            }
            Self::SyncForkPair { evidence } => sync::validate_evidence(evidence, true)?,
            Self::SyncAssetQuery {
                protocol_version,
                vault_id,
                device_id,
                assets,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_assets(assets)?;
            }
            Self::SyncAssetPresence { held } => sync::validate_asset_keys(held)?,
            Self::SyncAssetGet {
                protocol_version,
                vault_id,
                device_id,
                asset,
                offset,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_asset(asset)?;
                sync::validate_asset_offset(asset, *offset)?;
            }
            Self::SyncAssetChunk { data } => sync::validate_asset_data(data)?,
            Self::SyncAssetPut {
                protocol_version,
                vault_id,
                device_id,
                asset,
                offset,
                data,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_asset_chunk(asset, *offset, data)?;
            }
            Self::SyncAssetReceived { received } => sync::validate_asset_received(*received)?,
            Self::SyncPropose {
                protocol_version,
                vault_id,
                device_id,
                proposal,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                decode_sync_proposal(proposal)?;
            }
            Self::SyncProposed {
                revision,
                rejection,
            } => sync::validate_proposed(*revision, *rejection)?,
            Self::Error { code, message, .. } => {
                validate_identifier("error code", code)?;
                if message.is_empty() || message.len() > 1024 {
                    return Err("protocol error message is invalid".to_string());
                }
            }
        }
        Ok(())
    }
}

fn is_base64url(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate_base64url_text(label: &str, value: &str, length: usize) -> Result<(), String> {
    if value.len() != length || !is_base64url(value) {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn validate_card_text(label: &str, value: &str) -> Result<(), String> {
    let Some(encoded) = value.strip_prefix(ganbaru_contacts::CARD_TEXT_PREFIX) else {
        return Err(format!("{label} is invalid"));
    };
    if encoded.is_empty() || value.len() > MAX_CARD_TEXT_BYTES || !is_base64url(encoded) {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

pub async fn write_control<W>(writer: &mut W, message: &ControlMessage) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    message.validate()?;
    let encoded = serde_json::to_vec(message)
        .map_err(|error| format!("encode handoff control message: {error}"))?;
    if encoded.len() > MAX_CONTROL_BYTES {
        return Err("handoff control message exceeds the size limit".to_string());
    }
    writer
        .write_u32(encoded.len() as u32)
        .await
        .map_err(|error| format!("write handoff message length: {error}"))?;
    writer
        .write_all(&encoded)
        .await
        .map_err(|error| format!("write handoff message: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("flush handoff message: {error}"))
}

pub async fn read_control<R>(reader: &mut R) -> Result<ControlMessage, String>
where
    R: AsyncRead + Unpin,
{
    read_control_limited(reader, MAX_CONTROL_BYTES).await
}

/// Reads one control frame no larger than `max_bytes`, which never exceeds
/// [`MAX_CONTROL_BYTES`].
pub async fn read_control_limited<R>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<ControlMessage, String>
where
    R: AsyncRead + Unpin,
{
    let length = reader
        .read_u32()
        .await
        .map_err(|error| format!("read handoff message length: {error}"))?
        as usize;
    if length == 0 || length > max_bytes.min(MAX_CONTROL_BYTES) {
        return Err("handoff control message has an invalid size".to_string());
    }
    let mut encoded = vec![0_u8; length];
    reader
        .read_exact(&mut encoded)
        .await
        .map_err(|error| format!("read handoff message: {error}"))?;
    let message: ControlMessage = decode_bounded_json(&encoded, "handoff control message")?;
    message.validate()?;
    Ok(message)
}

fn decode_bounded_json<T: DeserializeOwned>(encoded: &[u8], label: &str) -> Result<T, String> {
    serde_json::from_slice(encoded).map_err(|error| format!("decode {label}: {error}"))
}

pub fn encode_invitation(invitation: &PairingInvitation) -> Result<String, String> {
    invitation.validate(unix_time_ms().saturating_sub(1))?;
    let json = serde_json::to_vec(invitation)
        .map_err(|error| format!("encode pairing invitation: {error}"))?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        json,
    ))
}

pub fn decode_invitation(encoded: &str, now_ms: i64) -> Result<PairingInvitation, String> {
    if encoded.is_empty() || encoded.len() > 8 * 1024 {
        return Err("pairing invitation has an invalid size".to_string());
    }
    let json = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
        .map_err(|error| format!("decode pairing invitation: {error}"))?;
    let invitation: PairingInvitation = decode_bounded_json(&json, "pairing invitation")?;
    invitation.validate(now_ms)?;
    Ok(invitation)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QrMatrix {
    pub width: usize,
    pub modules: Vec<bool>,
}

#[cfg(desktop)]
pub fn invitation_qr_matrix(invitation: &PairingInvitation) -> Result<QrMatrix, String> {
    let payload = encode_pairing_qr_payload(invitation)?;
    qr_matrix_for_bytes(&payload)
}

/// Renders an opaque binary payload as a QR module matrix.
pub fn qr_matrix_for_bytes(payload: &[u8]) -> Result<QrMatrix, String> {
    let code = qrcode::QrCode::with_error_correction_level(payload, qrcode::EcLevel::M)
        .map_err(|error| format!("create QR code: {error}"))?;
    let width = code.width();
    let modules = (0..width)
        .flat_map(|y| {
            let code = &code;
            (0..width).map(move |x| code[(x, y)] == qrcode::Color::Dark)
        })
        .collect();
    Ok(QrMatrix { width, modules })
}

/// Finds the first QR code in a grayscale frame whose payload starts with `expected_magic`.
pub fn decode_qr_bytes_luma(
    width: usize,
    height: usize,
    luma: &[u8],
    expected_magic: &[u8],
) -> Result<Vec<u8>, String> {
    const MAX_QR_FRAME_PIXELS: usize = 1920 * 1080;
    let pixels = width
        .checked_mul(height)
        .ok_or_else(|| "QR frame dimensions overflow".to_string())?;
    if width == 0 || height == 0 || pixels > MAX_QR_FRAME_PIXELS || luma.len() != pixels {
        return Err("QR frame dimensions are invalid".to_string());
    }
    let mut scanner = quircs::Quirc::default();
    for identified in scanner.identify(width, height, luma) {
        let identified = identified.map_err(|error| format!("identify QR code: {error}"))?;
        if let Ok(decoded) = identified.decode()
            && decoded.payload.starts_with(expected_magic)
        {
            return Ok(decoded.payload);
        }
    }
    Err("no readable QR code was found".to_string())
}

pub fn decode_pairing_qr_luma(
    width: usize,
    height: usize,
    luma: &[u8],
    now_ms: i64,
) -> Result<PairingInvitation, String> {
    let payload = decode_qr_bytes_luma(width, height, luma, PAIRING_QR_MAGIC)?;
    decode_pairing_qr_payload(&payload, now_ms)
}

#[cfg(desktop)]
fn encode_pairing_qr_payload(invitation: &PairingInvitation) -> Result<Vec<u8>, String> {
    invitation.validate(unix_time_ms().saturating_sub(1))?;
    let mut payload = Vec::with_capacity(256);
    payload.extend_from_slice(PAIRING_QR_MAGIC);
    payload.extend_from_slice(&invitation.protocol_version.to_be_bytes());
    push_qr_string(&mut payload, &invitation.compatibility.app_version)?;
    payload.extend_from_slice(&decode_qr_digest(
        &invitation.compatibility.database_schema_sha256,
        "database compatibility fingerprint",
    )?);
    push_qr_string(&mut payload, &invitation.invitation_id)?;
    push_qr_string(&mut payload, &invitation.secret)?;
    push_qr_string(&mut payload, &invitation.endpoint)?;
    payload.extend_from_slice(&decode_qr_digest(
        &invitation.coordinator_fingerprint,
        "coordinator fingerprint",
    )?);
    push_qr_string(&mut payload, &invitation.coordinator_device_id)?;
    push_qr_string(&mut payload, &invitation.vault_id)?;
    payload.extend_from_slice(&invitation.generation.to_be_bytes());
    payload.extend_from_slice(&invitation.expires_at_ms.to_be_bytes());
    Ok(payload)
}

fn decode_pairing_qr_payload(payload: &[u8], now_ms: i64) -> Result<PairingInvitation, String> {
    if !payload.starts_with(PAIRING_QR_MAGIC) {
        return Err("pairing QR code has an unsupported format".to_string());
    }

    let mut position = PAIRING_QR_MAGIC.len();
    let protocol_version = u16::from_be_bytes(take_qr_array(payload, &mut position)?);
    let app_version = take_qr_string(payload, &mut position)?;
    let database_schema_sha256 = encode_qr_digest(take_qr_array(payload, &mut position)?);
    let invitation_id = take_qr_string(payload, &mut position)?;
    let secret = take_qr_string(payload, &mut position)?;
    let endpoint = take_qr_string(payload, &mut position)?;
    let coordinator_fingerprint = encode_qr_digest(take_qr_array(payload, &mut position)?);
    let coordinator_device_id = take_qr_string(payload, &mut position)?;
    let vault_id = take_qr_string(payload, &mut position)?;
    let generation = u64::from_be_bytes(take_qr_array(payload, &mut position)?);
    let expires_at_ms = i64::from_be_bytes(take_qr_array(payload, &mut position)?);
    if position != payload.len() {
        return Err("pairing QR code contains unexpected data".to_string());
    }

    let invitation = PairingInvitation {
        protocol_version,
        compatibility: HandoffCompatibility {
            app_version,
            database_schema_sha256,
        },
        invitation_id,
        secret: secret.into(),
        endpoint,
        coordinator_fingerprint,
        coordinator_device_id,
        vault_id,
        generation,
        expires_at_ms,
    };
    invitation.validate(now_ms)?;
    Ok(invitation)
}

#[cfg(desktop)]
fn push_qr_string(payload: &mut Vec<u8>, value: &str) -> Result<(), String> {
    let length =
        u8::try_from(value.len()).map_err(|_| "pairing QR field is too long".to_string())?;
    payload.push(length);
    payload.extend_from_slice(value.as_bytes());
    Ok(())
}

fn take_qr_string(payload: &[u8], position: &mut usize) -> Result<String, String> {
    let length = usize::from(take_qr_array::<1>(payload, position)?[0]);
    let value = take_qr_bytes(payload, position, length)?;
    std::str::from_utf8(value)
        .map(str::to_owned)
        .map_err(|_| "pairing QR field is not UTF-8".to_string())
}

fn take_qr_array<const LENGTH: usize>(
    payload: &[u8],
    position: &mut usize,
) -> Result<[u8; LENGTH], String> {
    take_qr_bytes(payload, position, LENGTH)?
        .try_into()
        .map_err(|_| "pairing QR code is truncated".to_string())
}

fn take_qr_bytes<'a>(
    payload: &'a [u8],
    position: &mut usize,
    length: usize,
) -> Result<&'a [u8], String> {
    let end = position
        .checked_add(length)
        .ok_or_else(|| "pairing QR field length overflow".to_string())?;
    let value = payload
        .get(*position..end)
        .ok_or_else(|| "pairing QR code is truncated".to_string())?;
    *position = end;
    Ok(value)
}

fn decode_qr_digest(value: &str, label: &str) -> Result<[u8; 32], String> {
    validate_sha256(value).map_err(|_| format!("{label} is invalid"))?;
    let mut digest = [0_u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| format!("{label} is invalid"))?;
    }
    Ok(digest)
}

fn encode_qr_digest(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn validate_staging_file(path: &Path, metadata: &BundleMetadata) -> Result<(), String> {
    metadata.validate()?;
    let actual_bytes = path
        .metadata()
        .map_err(|error| format!("inspect staged bundle: {error}"))?
        .len();
    if actual_bytes != metadata.archive_bytes {
        return Err("staged bundle size does not match metadata".to_string());
    }
    let digest = sha256_file(path)?;
    if digest != metadata.archive_sha256 {
        return Err("staged bundle digest does not match metadata".to_string());
    }
    Ok(())
}

/// Lowercase hex SHA-256 digest of a file, read in transfer-sized chunks.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("open file for SHA-256 digest: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; TRANSFER_CHUNK_BYTES];
    loop {
        let bytes = file
            .read(&mut buffer)
            .map_err(|error| format!("read file for SHA-256 digest: {error}"))?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Validates a device label another device will display, with the shared display text rules.
///
/// # Errors
///
/// Returns a message naming `field` when the label is blank, too long, or holds a control,
/// bidirectional override, or invisible character.
pub fn validate_device_label(field: &str, label: &str) -> Result<(), String> {
    ganbaru_contacts::validate_display_text(label, MAX_DEVICE_LABEL_BYTES)
        .map_err(|error| format!("{field} is invalid: {error}"))
}

/// Accepts only the form certificate fingerprints are produced in: the SHA-256 of the DER
/// certificate as 64 lowercase hex characters. Every stored, scanned, or received fingerprint
/// passes this check so pins can be compared exactly.
pub fn validate_certificate_fingerprint(label: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

/// The 32-byte digest of a fingerprint that passes [`validate_certificate_fingerprint`].
pub fn decode_certificate_fingerprint(value: &str) -> Option<[u8; 32]> {
    validate_certificate_fingerprint("certificate fingerprint", value).ok()?;
    decode_qr_digest(value, "certificate fingerprint").ok()
}

pub fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn validate_protocol(version: u16) -> Result<(), String> {
    if version != PROTOCOL_VERSION {
        return Err("handoff protocol version is unsupported".to_string());
    }
    Ok(())
}

fn validate_sync_peer(version: u16, vault_id: &str, device_id: &str) -> Result<(), String> {
    validate_protocol(version)?;
    validate_identifier("vault id", vault_id)?;
    validate_identifier("device id", device_id)
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("archive SHA-256 digest is invalid".to_string());
    }
    Ok(())
}

pub fn unix_time_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Fixed compatibility values for tests that exercise handoff peers.
#[cfg(any(test, feature = "test-support"))]
pub fn test_compatibility() -> HandoffCompatibility {
    HandoffCompatibility {
        app_version: "test".to_string(),
        database_schema_sha256: "a".repeat(64),
    }
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;

    fn test_device_key() -> ganbaru_sync_contracts::DevicePublicKey {
        ganbaru_sync_contracts::DevicePublicKey::from_bytes([3; 32])
    }

    fn qr_invitation() -> PairingInvitation {
        PairingInvitation {
            protocol_version: PROTOCOL_VERSION,
            compatibility: test_compatibility(),
            invitation_id: "invite-abcdefghijklmnopqrstuv".to_string(),
            secret: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG"
                .to_string()
                .into(),
            endpoint: "192.168.1.20:43821".to_string(),
            coordinator_fingerprint: "b".repeat(64),
            coordinator_device_id: "device-abcdefghijklmnopqrstuv".to_string(),
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            generation: 7,
            expires_at_ms: i64::MAX,
        }
    }

    #[test]
    fn invitation_secrets_serialize_plainly_and_stay_out_of_debug_output() {
        let invitation = qr_invitation();
        let secret = invitation.secret.to_string();
        let enroll = ControlMessage::Enroll {
            protocol_version: PROTOCOL_VERSION,
            invitation_id: invitation.invitation_id.clone(),
            secret: invitation.secret.clone(),
            vault_id: invitation.vault_id.clone(),
            device_id: "device-phone".to_string(),
            device_label: "Phone".to_string(),
            device_kind: DeviceKind::Phone,
            device_certificate: "certificate".to_string(),
            sync_device_key: encode_sync_device_key(&test_device_key()),
        };
        for output in [format!("{invitation:?}"), format!("{enroll:?}")] {
            assert!(!output.contains(&secret), "{output}");
            assert!(output.contains("[REDACTED]"), "{output}");
        }
        let serialized = serde_json::to_value(&enroll).expect("serialize enrollment");
        assert_eq!(serialized["secret"], secret.as_str());
        let decoded: ControlMessage =
            serde_json::from_value(serialized).expect("decode enrollment");
        assert_eq!(decoded, enroll);
    }

    #[test]
    fn usage_exchange_messages_are_not_part_of_the_protocol() {
        // Usage reaches other devices as replicated daily totals, so the control channel
        // carries no usage samples in either direction.
        for kind in ["distractionsExchange", "distractionsAcknowledged"] {
            let message = serde_json::json!({
                "kind": kind,
                "protocolVersion": PROTOCOL_VERSION,
                "vaultId": "vault-abcdefghijklmnopqrstuv",
                "deviceId": "device-phone",
                "samples": [],
                "acknowledgedPeerSampleIds": [],
                "ownerSnapshot": [],
                "acknowledgedSampleIds": [],
                "peerSamples": [],
                "combinedSamples": [],
            });
            assert!(
                serde_json::from_value::<ControlMessage>(message).is_err(),
                "{kind} must not decode"
            );
        }
    }

    #[test]
    fn enrollment_messages_reject_control_and_bidi_labels() {
        let enroll = |label: &str| ControlMessage::Enroll {
            protocol_version: PROTOCOL_VERSION,
            invitation_id: "invite-abcdefghijklmnopqrstuv".to_string(),
            secret: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG"
                .to_string()
                .into(),
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            device_id: "device-phone".to_string(),
            device_label: label.to_string(),
            device_kind: DeviceKind::Phone,
            device_certificate: "certificate".to_string(),
            sync_device_key: encode_sync_device_key(&test_device_key()),
        };
        let enrolled = |label: &str| ControlMessage::Enrolled {
            protocol_version: PROTOCOL_VERSION,
            coordinator_device_id: "device-desktop".to_string(),
            coordinator_device_label: Some(label.to_string()),
            charter: None,
            person_key: None,
        };
        for label in ["phone\u{0007}", "a\u{202E}b", "a\u{200F}b", ""] {
            assert!(enroll(label).validate().is_err(), "{label:?}");
            assert!(enrolled(label).validate().is_err(), "{label:?}");
        }
        for label in [
            "Phone",
            "\u{1F469}\u{200D}\u{1F4BB}",
            "\u{0645}\u{200C}\u{0627}",
        ] {
            assert_eq!(enroll(label).validate(), Ok(()), "{label:?}");
            assert_eq!(enrolled(label).validate(), Ok(()), "{label:?}");
        }
        let with_key = |key: String| match enroll("Phone") {
            ControlMessage::Enroll {
                protocol_version,
                invitation_id,
                secret,
                vault_id,
                device_id,
                device_label,
                device_kind,
                device_certificate,
                ..
            } => ControlMessage::Enroll {
                protocol_version,
                invitation_id,
                secret,
                vault_id,
                device_id,
                device_label,
                device_kind,
                device_certificate,
                sync_device_key: key,
            },
            _ => unreachable!(),
        };
        let full = encode_sync_device_key(&test_device_key());
        for invalid in [
            String::new(),
            full[1..].to_string(),
            format!("{full}AA"),
            "+".repeat(43),
        ] {
            assert!(with_key(invalid.clone()).validate().is_err(), "{invalid:?}");
        }
        assert_eq!(
            decode_sync_device_key(&full),
            Ok(test_device_key()),
            "the device key round-trips"
        );
    }

    #[test]
    fn invitation_qr_round_trips_through_production_decoder() {
        let invitation = qr_invitation();
        let matrix = invitation_qr_matrix(&invitation).expect("QR should encode");
        let scale = 8;
        let quiet = 4;
        let image_width = (matrix.width + quiet * 2) * scale;
        let mut image = vec![255_u8; image_width * image_width];
        for y in 0..matrix.width {
            for x in 0..matrix.width {
                if matrix.modules[y * matrix.width + x] {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = (x + quiet) * scale + dx;
                            let py = (y + quiet) * scale + dy;
                            image[py * image_width + px] = 0;
                        }
                    }
                }
            }
        }
        assert_eq!(
            decode_pairing_qr_luma(image_width, image_width, &image, 1).expect("QR should decode"),
            invitation
        );
    }

    #[test]
    fn certificate_fingerprints_require_exactly_64_lowercase_hex_characters() {
        let lowercase = "0123456789abcdef".repeat(4);
        assert!(validate_certificate_fingerprint("fingerprint", &lowercase).is_ok());
        let digest = decode_certificate_fingerprint(&lowercase).expect("lowercase digest");
        assert_eq!(encode_qr_digest(digest), lowercase);
        for invalid in [
            lowercase.to_ascii_uppercase(),
            format!("+{}", &lowercase[1..]),
            format!("{} ", &lowercase[1..]),
            lowercase[1..].to_string(),
            format!("{lowercase}0"),
            "g".repeat(64),
        ] {
            assert!(
                validate_certificate_fingerprint("fingerprint", &invalid).is_err(),
                "{invalid}"
            );
            assert!(
                decode_certificate_fingerprint(&invalid).is_none(),
                "{invalid}"
            );
            let invitation = PairingInvitation {
                coordinator_fingerprint: invalid.clone(),
                ..qr_invitation()
            };
            assert!(invitation.validate(0).is_err(), "{invalid}");
        }
    }
    #[test]
    fn invitations_with_non_lan_endpoints_fail_validation() {
        for endpoint in [
            "203.0.113.5:443",
            "8.8.8.8:43821",
            "224.0.0.251:43821",
            "192.168.1.20:0",
        ] {
            let invitation = PairingInvitation {
                endpoint: endpoint.to_string(),
                ..qr_invitation()
            };
            assert!(invitation.validate(0).is_err(), "{endpoint}");
            assert!(encode_invitation(&invitation).is_err(), "{endpoint}");
            let crafted = base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                serde_json::to_vec(&invitation).expect("invitation json"),
            );
            assert!(decode_invitation(&crafted, 0).is_err(), "{endpoint}");
        }
        for endpoint in [
            "100.64.3.4:43821",
            "[2a01:4f8:10:20::5]:43821",
            "[fe80::1]:43821",
        ] {
            let invitation = PairingInvitation {
                endpoint: endpoint.to_string(),
                ..qr_invitation()
            };
            assert_eq!(invitation.validate(0), Ok(()), "{endpoint}");
        }
    }

    #[test]
    fn compact_pairing_qr_is_materially_smaller_than_the_manual_code() {
        let invitation = qr_invitation();
        let payload = encode_pairing_qr_payload(&invitation).expect("QR payload");
        let manual = encode_invitation(&invitation).expect("manual invitation");
        let matrix = invitation_qr_matrix(&invitation).expect("QR should encode");

        assert!(payload.len() * 4 < manual.len() * 3);
        assert!(matrix.width <= 65, "QR width was {} modules", matrix.width);
    }

    #[test]
    fn compact_pairing_qr_rejects_truncation_and_trailing_data() {
        let invitation = qr_invitation();
        let payload = encode_pairing_qr_payload(&invitation).expect("QR payload");

        assert!(
            decode_pairing_qr_payload(&payload[..payload.len() - 1], 1)
                .unwrap_err()
                .contains("truncated")
        );
        let mut trailing = payload;
        trailing.push(0);
        assert!(
            decode_pairing_qr_payload(&trailing, 1)
                .unwrap_err()
                .contains("unexpected data")
        );
    }

    #[test]
    fn pairing_qr_decoder_rejects_text_invitations() {
        let invitation = qr_invitation();
        let encoded = encode_invitation(&invitation).expect("manual invitation");

        assert!(decode_pairing_qr_payload(encoded.as_bytes(), 1).is_err());
        assert_eq!(decode_invitation(&encoded, 1).unwrap(), invitation);
    }

    #[test]
    fn malformed_and_oversized_metadata_is_rejected() {
        let mut metadata = BundleMetadata {
            protocol_version: PROTOCOL_VERSION,
            compatibility: test_compatibility(),
            vault_id: "vault-1".to_string(),
            device_id: "device-1".to_string(),
            transfer_id: "transfer-1".to_string(),
            generation: 1,
            archive_bytes: 10,
            archive_sha256: "a".repeat(64),
        };
        metadata.archive_sha256 = "not-a-digest".to_string();
        assert!(metadata.validate().unwrap_err().contains("SHA-256"));
        metadata.archive_sha256 = "a".repeat(64);
        metadata.archive_bytes = MAX_ARCHIVE_BYTES + 1;
        assert!(metadata.validate().unwrap_err().contains("bundle size"));
    }

    fn asset(byte_size: u32) -> SyncAsset {
        SyncAsset {
            class: "quick-notes/images".to_string(),
            sha256: "a".repeat(64),
            content_type: "image/png".to_string(),
            byte_size,
        }
    }

    fn put(asset: SyncAsset, offset: u32, data: String) -> ControlMessage {
        ControlMessage::SyncAssetPut {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-1".to_string(),
            asset,
            offset,
            data,
        }
    }

    #[test]
    fn asset_references_must_be_well_formed() {
        let query = |assets: Vec<SyncAsset>| ControlMessage::SyncAssetQuery {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-1".to_string(),
            assets,
        };
        query(vec![asset(10)]).validate().unwrap();
        let mut upper = asset(10);
        upper.sha256 = "A".repeat(64);
        let mut short = asset(10);
        short.sha256 = "a".repeat(63);
        for content_type in [
            "image",
            "image/",
            "/png",
            "image/png/x",
            "Image/PNG",
            "image/p ng",
        ] {
            let mut typed = asset(10);
            typed.content_type = content_type.to_string();
            assert!(query(vec![typed]).validate().is_err(), "{content_type}");
        }
        for invalid in [upper, short, asset(0), asset(MAX_SYNC_ASSET_BYTES + 1)] {
            assert!(query(vec![invalid]).validate().is_err());
        }
        assert!(
            query(vec![asset(10); MAX_SYNC_ASSETS + 1])
                .validate()
                .unwrap_err()
                .contains("too large")
        );
        let presence = |held: Vec<SyncAssetKey>| ControlMessage::SyncAssetPresence { held };
        presence(vec![asset(10).key()]).validate().unwrap();
        let mut escaping = asset(10).key();
        escaping.sha256 = "../a".to_string();
        assert!(presence(vec![escaping]).validate().is_err());
        assert!(
            presence(vec![asset(10).key(); MAX_SYNC_ASSETS + 1])
                .validate()
                .unwrap_err()
                .contains("too large")
        );
    }

    #[test]
    fn sync_asset_classes_must_be_relative_lowercase_paths() {
        let query = |class: &str| {
            let mut classed = asset(10);
            classed.class = class.to_string();
            ControlMessage::SyncAssetQuery {
                protocol_version: PROTOCOL_VERSION,
                vault_id: "vault-1".to_string(),
                device_id: "device-1".to_string(),
                assets: vec![classed],
            }
        };
        for valid in ["quick-notes/images", "notes", "a/b/c-2"] {
            query(valid).validate().unwrap();
        }
        for invalid in [
            "",
            "/quick-notes",
            "quick-notes/",
            "quick-notes//images",
            "../images",
            "quick-notes/..",
            ".",
            "Quick-notes/images",
            "quick notes",
            "quick-notes\\images",
            &"a".repeat(65),
        ] {
            assert_eq!(
                query(invalid).validate(),
                Err("sync asset class is invalid".to_string()),
                "{invalid}"
            );
            let key = SyncAssetKey {
                class: invalid.to_string(),
                sha256: "a".repeat(64),
            };
            assert!(
                ControlMessage::SyncAssetPresence { held: vec![key] }
                    .validate()
                    .is_err(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn sync_assets_up_to_64_mib_stream_through_the_exchange() {
        assert_eq!(MAX_SYNC_ASSET_BYTES, 64 * 1024 * 1024);
        let last = MAX_SYNC_ASSET_BYTES - 3;
        put(asset(MAX_SYNC_ASSET_BYTES), last, "AAAA".to_string())
            .validate()
            .unwrap();
    }

    #[test]
    fn asset_chunks_must_lie_inside_their_asset() {
        put(asset(6), 0, "AAAAAAAA".to_string()).validate().unwrap();
        put(asset(6), 3, "AAAA".to_string()).validate().unwrap();
        assert!(
            put(asset(6), 3, "AAAAAAAA".to_string())
                .validate()
                .unwrap_err()
                .contains("past the asset")
        );
        assert!(put(asset(6), 6, "AAAA".to_string()).validate().is_err());
        assert!(put(asset(6), 0, String::new()).validate().is_err());
        assert!(put(asset(6), 0, "AA+/".to_string()).validate().is_err());
        let oversized = "A".repeat((SYNC_ASSET_CHUNK_BYTES * 4).div_ceil(3) + 4);
        assert!(
            put(asset(MAX_SYNC_ASSET_BYTES), 0, oversized)
                .validate()
                .is_err()
        );

        let get = |offset| ControlMessage::SyncAssetGet {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-1".to_string(),
            asset: asset(6),
            offset,
        };
        get(5).validate().unwrap();
        assert!(get(6).validate().is_err());
        let received = |received| ControlMessage::SyncAssetReceived { received };
        received(MAX_SYNC_ASSET_BYTES).validate().unwrap();
        assert!(received(MAX_SYNC_ASSET_BYTES + 1).validate().is_err());
    }

    #[test]
    fn compatibility_requires_the_same_database_migration_set() {
        let local = test_compatibility();
        let mut remote = local.clone();
        ensure_compatible(&local, &remote).expect("matching schemas should interoperate");

        remote.app_version = "newer".to_string();
        remote.database_schema_sha256 = "b".repeat(64);
        let error = ensure_compatible(&local, &remote).unwrap_err();
        assert!(error.contains("compatibility mismatch"));
        assert!(error.contains("test"));
        assert!(error.contains("newer"));
    }

    #[test]
    fn contact_status_replay_window_holds_at_every_timestamp_extreme() {
        let status = |issued_at_ms: i64| ControlMessage::ContactRequestStatus {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-abcdefghijklmnopqrstuv".to_string(),
            requester_public_key: "a".repeat(ganbaru_contacts::PUBLIC_KEY_TEXT_LENGTH),
            issued_at_ms,
            signature: "s".repeat(SIGNATURE_TEXT_LENGTH),
        };
        let now = unix_time_ms();

        status(now).validate().expect("current status poll");
        status(now - STATUS_REPLAY_WINDOW_MS / 2)
            .validate()
            .expect("status poll inside the window");
        for issued_at_ms in [
            i64::MIN,
            i64::MIN + 1,
            i64::MAX,
            0,
            now - 2 * STATUS_REPLAY_WINDOW_MS,
            now + 2 * STATUS_REPLAY_WINDOW_MS,
        ] {
            let error = status(issued_at_ms).validate().unwrap_err();
            assert!(error.contains("replay window"), "{issued_at_ms}: {error}");
        }
    }

    fn discovery_key_of(bytes: usize) -> String {
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            vec![7_u8; bytes],
        )
    }

    fn sync_state(endpoints: &[&str], discovery_key: String) -> ControlMessage {
        ControlMessage::SyncState {
            manifest_version: 1,
            stored: vec![SyncSeq {
                writer: "a".repeat(32),
                seq: 3,
            }],
            probe_hash: Some("c".repeat(64)),
            coordinator_endpoints: endpoints.iter().map(ToString::to_string).collect(),
            discovery_key,
            stable: vec![SyncSeq {
                writer: "a".repeat(32),
                seq: 2,
            }],
            bootstrap_required: false,
            forked: vec![SyncSeq {
                writer: "b".repeat(32),
                seq: 3,
            }],
        }
    }

    fn frame_round_trip(message: &ControlMessage) -> Result<ControlMessage, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let mut frame = Vec::new();
            write_control(&mut frame, message).await?;
            read_control(&mut frame.as_slice()).await
        })
    }

    #[test]
    fn sync_state_round_trips_reported_endpoints_and_discovery_key() {
        let state = sync_state(
            &[
                "192.168.1.20:43821",
                "[fd00::5]:43821",
                "[2a01:4f8:10:20::5]:43821",
                "100.64.3.4:43821",
            ],
            discovery_key_of(DISCOVERY_KEY_BYTES),
        );
        assert_eq!(frame_round_trip(&state), Ok(state));
        let empty = sync_state(&[], discovery_key_of(DISCOVERY_KEY_BYTES));
        assert_eq!(frame_round_trip(&empty), Ok(empty));
        let full = vec!["192.168.1.20:43821"; MAX_COORDINATOR_ENDPOINTS];
        assert_eq!(
            sync_state(&full, discovery_key_of(DISCOVERY_KEY_BYTES)).validate(),
            Ok(())
        );
    }

    #[test]
    fn sync_state_refuses_unreportable_endpoints_and_malformed_keys() {
        let key = discovery_key_of(DISCOVERY_KEY_BYTES);
        let nine = vec!["192.168.1.20:43821"; MAX_COORDINATOR_ENDPOINTS + 1];
        assert!(sync_state(&nine, key.clone()).validate().is_err());
        for endpoint in [
            "203.0.113.5:43821",
            "127.0.0.1:43821",
            "[::1]:43821",
            "[fe80::1]:43821",
            "192.168.1.20:0",
            "192.168.1.20",
            "",
        ] {
            let state = sync_state(&["10.0.0.4:43821", endpoint], key.clone());
            assert!(state.validate().is_err(), "{endpoint:?}");
            assert!(frame_round_trip(&state).is_err(), "{endpoint:?}");
        }
        for malformed in [
            discovery_key_of(DISCOVERY_KEY_BYTES - 1),
            discovery_key_of(DISCOVERY_KEY_BYTES + 1),
            format!("{key}="),
            "!".repeat(key.len()),
            String::new(),
        ] {
            assert!(
                sync_state(&[], malformed.clone()).validate().is_err(),
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn sync_state_round_trips_stability_and_refuses_malformed_stable_vectors() {
        let key = discovery_key_of(DISCOVERY_KEY_BYTES);
        let mut state = sync_state(&[], key.clone());
        if let ControlMessage::SyncState {
            bootstrap_required, ..
        } = &mut state
        {
            *bootstrap_required = true;
        }
        assert_eq!(frame_round_trip(&state), Ok(state));
        for stable in [
            vec![SyncSeq {
                writer: "a".repeat(31),
                seq: 1,
            }],
            vec![SyncSeq {
                writer: "a".repeat(32),
                seq: 0,
            }],
        ] {
            let mut state = sync_state(&[], key.clone());
            if let ControlMessage::SyncState { stable: field, .. } = &mut state {
                *field = stable.clone();
            }
            assert!(state.validate().is_err(), "{stable:?}");
        }
        let refusal = SyncRefusal {
            code: SyncRefusalCode::BootstrapRequired,
            writer: None,
            seq: None,
        };
        let encoded = serde_json::to_string(&refusal).expect("refusal encodes");
        assert!(encoded.contains("\"bootstrap_required\""), "{encoded}");
    }

    #[test]
    fn sync_state_bounds_its_forked_writers() {
        let key = discovery_key_of(DISCOVERY_KEY_BYTES);
        let with_forked = |forked: Vec<SyncSeq>| {
            let mut state = sync_state(&[], key.clone());
            if let ControlMessage::SyncState { forked: field, .. } = &mut state {
                *field = forked;
            }
            state
        };
        let writers = ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_SPACE;
        let entry = |index: usize| SyncSeq {
            writer: format!("{index:032x}"),
            seq: 2,
        };
        let full = with_forked((0..writers).map(entry).collect());
        assert_eq!(frame_round_trip(&full), Ok(full));
        assert!(
            with_forked((0..=writers).map(entry).collect())
                .validate()
                .is_err()
        );
        for malformed in [
            SyncSeq {
                writer: "B".repeat(32),
                seq: 2,
            },
            SyncSeq {
                writer: "b".repeat(32),
                seq: 0,
            },
        ] {
            assert!(with_forked(vec![malformed.clone()]).validate().is_err());
        }
    }

    #[test]
    fn fork_evidence_messages_round_trip_and_bound_their_operations() {
        let op = |byte: u8| {
            base64::Engine::encode(
                &base64::engine::general_purpose::URL_SAFE_NO_PAD,
                vec![byte; 90],
            )
        };
        let request = |evidence: Vec<String>| ControlMessage::SyncForkEvidence {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            writer: "a".repeat(32),
            evidence,
        };
        for evidence in [Vec::new(), vec![op(1)], vec![op(1), op(2)]] {
            let message = request(evidence);
            assert_eq!(frame_round_trip(&message), Ok(message.clone()));
            assert_eq!(message.sync_peer(), Some(("vault-1", "device-phone")));
        }
        assert!(request(vec![op(1), op(2), op(3)]).validate().is_err());
        assert!(request(vec!["not base64!".to_string()]).validate().is_err());
        let mut foreign = request(Vec::new());
        if let ControlMessage::SyncForkEvidence { writer, .. } = &mut foreign {
            *writer = "a".repeat(31);
        }
        assert!(foreign.validate().is_err());
        let pair = |evidence| ControlMessage::SyncForkPair { evidence };
        for evidence in [Vec::new(), vec![op(1), op(2)]] {
            let message = pair(evidence);
            assert_eq!(frame_round_trip(&message), Ok(message));
        }
        assert!(pair(vec![op(1)]).validate().is_err());
        assert!(pair(vec![op(1), op(2), op(3)]).validate().is_err());
    }

    fn signed_charter(
        person: &ganbaru_contacts::PersonKeyPair,
        anchor_device_id: &str,
    ) -> ganbaru_sync_contracts::SignedCharter {
        let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[5; 32]).expect("admin");
        ganbaru_sync_contracts::sign_charter(
            &ganbaru_sync_contracts::Charter {
                space: ganbaru_sync_contracts::SpaceId::personal("vault-1"),
                anchor_device_id: anchor_device_id.to_string(),
                anchor_device_key: test_device_key(),
                anchor_tls_fingerprint: [6; 32],
                anchor_admin_key: admin.public_key(),
                recovery_admin_key: admin.public_key(),
                discovery_key: [7; 32],
                created_at_ms: 1,
            },
            person,
        )
        .expect("sign charter")
    }

    #[test]
    fn pairing_response_carries_a_charter_its_person_key_verifies() {
        let person = ganbaru_contacts::PersonKeyPair::from_seed(&[8; 32]).expect("person");
        let other = ganbaru_contacts::PersonKeyPair::from_seed(&[9; 32]).expect("other");
        let enrolled =
            |charter: Option<String>, person_key: Option<String>| ControlMessage::Enrolled {
                protocol_version: PROTOCOL_VERSION,
                coordinator_device_id: "device-desktop".to_string(),
                coordinator_device_label: None,
                charter,
                person_key,
            };
        let charter = encode_sync_charter(&signed_charter(&person, "device-desktop"));
        let person_key = person.public_key().to_text();
        let valid = enrolled(Some(charter.clone()), Some(person_key.clone()));
        assert_eq!(frame_round_trip(&valid), Ok(valid.clone()));
        assert_eq!(enrolled(None, None).validate(), Ok(()));

        let foreign_anchor = encode_sync_charter(&signed_charter(&person, "device-other"));
        let mut tampered =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &charter)
                .expect("charter bytes");
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        let tampered =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, tampered);
        for invalid in [
            enrolled(Some(charter.clone()), None),
            enrolled(None, Some(person_key.clone())),
            enrolled(Some(charter.clone()), Some(other.public_key().to_text())),
            enrolled(Some(foreign_anchor), Some(person_key.clone())),
            enrolled(Some(tampered), Some(person_key.clone())),
            enrolled(Some("A".repeat(4096)), Some(person_key.clone())),
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn proposals_round_trip_verified_and_answers_name_one_outcome() {
        let admin = ganbaru_sync_contracts::AdminKeyPair::from_seed(&[5; 32]).expect("admin");
        let proposal = ganbaru_sync_contracts::sign_proposal(
            &ganbaru_sync_contracts::Proposal {
                space: ganbaru_sync_contracts::SpaceId::personal("vault-1"),
                author: admin.public_key(),
                created_at_ms: 1,
                entry: ganbaru_sync_contracts::Entry::Remove(ganbaru_sync_contracts::Remove {
                    device_id: "device-phone".to_string(),
                }),
            },
            &admin,
        )
        .expect("sign proposal");
        let propose = |proposal: String| ControlMessage::SyncPropose {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-desktop".to_string(),
            proposal,
        };
        let text = encode_sync_proposal(&proposal);
        let message = propose(text.clone());
        assert_eq!(frame_round_trip(&message), Ok(message.clone()));
        assert_eq!(message.sync_peer(), Some(("vault-1", "device-desktop")));
        assert_eq!(decode_sync_proposal(&text), Ok(proposal));
        let mut forged =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &text)
                .expect("proposal bytes");
        let last = forged.len() - 1;
        forged[last] ^= 1;
        let forged =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, forged);
        for invalid in [String::new(), forged, "not base64!".to_string()] {
            assert!(propose(invalid.clone()).validate().is_err(), "{invalid:?}");
        }

        let answer = |revision, rejection| ControlMessage::SyncProposed {
            revision,
            rejection,
        };
        let not_anchor = ganbaru_sync_contracts::ProposalRejection::NotAnchor.as_u8();
        for valid in [answer(Some(3), None), answer(None, Some(not_anchor))] {
            assert_eq!(frame_round_trip(&valid), Ok(valid.clone()));
        }
        for invalid in [
            answer(None, None),
            answer(Some(3), Some(not_anchor)),
            answer(Some(0), None),
            answer(Some(u64::MAX), None),
            answer(None, Some(u8::MAX)),
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn sync_requests_of_the_previous_protocol_ask_for_an_update() {
        let hello = |protocol_version| ControlMessage::SyncHello {
            protocol_version,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            manifest_version: 1,
            stored: Vec::new(),
            probe: None,
        };
        assert_eq!(hello(PROTOCOL_VERSION).validate(), Ok(()));
        assert_eq!(
            hello(PROTOCOL_VERSION - 1).validate(),
            Err("handoff protocol version is unsupported".to_string())
        );
    }
}

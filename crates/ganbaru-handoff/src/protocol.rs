//! Bounded wire contracts for the local vault handoff service.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

mod sync;

pub use sync::{
    MAX_SYNC_HASHES, SYNC_PAGE_BYTES, SyncProbe, SyncRefusal, SyncRefusalCode, SyncSeq,
};

pub const PROTOCOL_VERSION: u16 = 5;
pub const MAX_CONTROL_BYTES: usize = 1024 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 160;
pub const MAX_DEVICE_LABEL_BYTES: usize = 128;
const MAX_APP_VERSION_BYTES: usize = 64;
pub const MAX_DISTRACTIONS_SAMPLES: usize = 1_024;
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
            "handoff compatibility mismatch between Ganbaru AI {} and {}; update Ganbaru AI on both devices",
            local.app_version, remote.app_version
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInvitation {
    pub protocol_version: u16,
    pub compatibility: HandoffCompatibility,
    pub invitation_id: String,
    pub secret: String,
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
        validate_identifier("coordinator fingerprint", &self.coordinator_fingerprint)?;
        validate_identifier("coordinator device id", &self.coordinator_device_id)?;
        validate_identifier("vault id", &self.vault_id)?;
        self.endpoint
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "pairing invitation endpoint is invalid".to_string())?;
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
#[serde(rename_all = "camelCase")]
pub struct DistractionsSampleMessage {
    pub sample_id: String,
    pub device_id: String,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ControlMessage {
    Enroll {
        protocol_version: u16,
        invitation_id: String,
        secret: String,
        vault_id: String,
        device_id: String,
        device_label: String,
        device_kind: DeviceKind,
        device_certificate: String,
    },
    Enrolled {
        protocol_version: u16,
        coordinator_device_id: String,
        #[serde(default)]
        coordinator_device_label: Option<String>,
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
    DistractionsExchange {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        samples: Vec<DistractionsSampleMessage>,
        acknowledged_peer_sample_ids: Vec<String>,
        owner_snapshot: Vec<DistractionsSampleMessage>,
    },
    DistractionsAcknowledged {
        acknowledged_sample_ids: Vec<String>,
        peer_samples: Vec<DistractionsSampleMessage>,
        combined_samples: Vec<DistractionsSampleMessage>,
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
    /// The hub's stored operations and its hash at the probed sequence, when it holds it.
    SyncState {
        manifest_version: u16,
        stored: Vec<SyncSeq>,
        probe_hash: Option<String>,
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
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("invitation id", invitation_id)?;
                validate_identifier("invitation secret", secret)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                if device_label.trim().is_empty() || device_label.len() > MAX_DEVICE_LABEL_BYTES {
                    return Err("device label is invalid".to_string());
                }
                if device_certificate.is_empty() || device_certificate.len() > 8 * 1024 {
                    return Err("device certificate is invalid".to_string());
                }
            }
            Self::Enrolled {
                protocol_version,
                coordinator_device_id,
                coordinator_device_label,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("coordinator device id", coordinator_device_id)?;
                if coordinator_device_label.as_ref().is_some_and(|label| {
                    label.trim().is_empty()
                        || label.len() > MAX_DEVICE_LABEL_BYTES
                        || label.chars().any(char::is_control)
                }) {
                    return Err("coordinator device label is invalid".to_string());
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
            Self::DistractionsExchange {
                protocol_version,
                vault_id,
                device_id,
                samples,
                acknowledged_peer_sample_ids,
                owner_snapshot,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                if samples.len() > MAX_DISTRACTIONS_SAMPLES {
                    return Err("too many Distractions samples".to_string());
                }
                for sample in samples {
                    validate_distractions_sample(sample)?;
                    if sample.device_id != *device_id {
                        return Err("Distractions sample metadata is invalid".to_string());
                    }
                }
                validate_distractions_sample_ids(acknowledged_peer_sample_ids)?;
                validate_distractions_samples(owner_snapshot)?;
                if samples.len() + owner_snapshot.len() > MAX_DISTRACTIONS_SAMPLES {
                    return Err("Distractions exchange exceeds the sample limit".to_string());
                }
            }
            Self::DistractionsAcknowledged {
                acknowledged_sample_ids,
                peer_samples,
                combined_samples,
            } => {
                validate_distractions_sample_ids(acknowledged_sample_ids)?;
                validate_distractions_samples(peer_samples)?;
                validate_distractions_samples(combined_samples)?;
                if peer_samples.len() + combined_samples.len() > MAX_DISTRACTIONS_SAMPLES {
                    return Err("Distractions response exceeds the sample limit".to_string());
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
                if (unix_time_ms() - *issued_at_ms).abs() > STATUS_REPLAY_WINDOW_MS {
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
            } => {
                sync::validate_vector(stored)?;
                if let Some(hash) = probe_hash {
                    sync::validate_hashes(std::slice::from_ref(hash))?;
                }
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

fn validate_distractions_sample_ids(sample_ids: &[String]) -> Result<(), String> {
    if sample_ids.len() > MAX_DISTRACTIONS_SAMPLES {
        return Err("too many acknowledged Distractions samples".to_string());
    }
    for sample_id in sample_ids {
        validate_identifier("sample id", sample_id)?;
    }
    Ok(())
}

fn validate_distractions_samples(samples: &[DistractionsSampleMessage]) -> Result<(), String> {
    if samples.len() > MAX_DISTRACTIONS_SAMPLES {
        return Err("too many Distractions samples".to_string());
    }
    for sample in samples {
        validate_distractions_sample(sample)?;
    }
    Ok(())
}

fn validate_distractions_sample(sample: &DistractionsSampleMessage) -> Result<(), String> {
    validate_identifier("sample id", &sample.sample_id)?;
    validate_identifier("sample device id", &sample.device_id)?;
    if !matches!(
        sample.source_type.as_str(),
        "website" | "desktop-app" | "mobile-app"
    ) || sample.source_key.trim().is_empty()
        || sample.source_key.len() > 255
        || sample
            .display_name
            .as_ref()
            .is_some_and(|name| name.len() > 120)
        || sample.started_at_ms < 0
        || !(1..=86_400).contains(&sample.elapsed_seconds)
        || !is_valid_local_date(&sample.local_date)
        || sample.created_at_ms < 0
    {
        return Err("Distractions sample metadata is invalid".to_string());
    }
    Ok(())
}

fn is_valid_local_date(value: &str) -> bool {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
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
    let length = reader
        .read_u32()
        .await
        .map_err(|error| format!("read handoff message length: {error}"))?
        as usize;
    if length == 0 || length > MAX_CONTROL_BYTES {
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
        secret,
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

#[cfg(desktop)]
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

    fn qr_invitation() -> PairingInvitation {
        PairingInvitation {
            protocol_version: PROTOCOL_VERSION,
            compatibility: test_compatibility(),
            invitation_id: "invite-abcdefghijklmnopqrstuv".to_string(),
            secret: "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG".to_string(),
            endpoint: "192.168.1.20:43821".to_string(),
            coordinator_fingerprint: "b".repeat(64),
            coordinator_device_id: "device-abcdefghijklmnopqrstuv".to_string(),
            vault_id: "vault-abcdefghijklmnopqrstuv".to_string(),
            generation: 7,
            expires_at_ms: i64::MAX,
        }
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
}

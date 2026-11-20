//! Bounded wire contracts for the local vault handoff service.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub mod anchor;
pub mod relay;
mod sync;

pub use sync::{
    DISCOVERY_KEY_BYTES, MAX_COORDINATOR_ENDPOINTS, MAX_SYNC_ASSET_BYTES, MAX_SYNC_ASSETS,
    MAX_SYNC_EVIDENCE_OPS, MAX_SYNC_FORWARDED_PROPOSALS, MAX_SYNC_HASHES, SYNC_ASSET_CHUNK_BYTES,
    SYNC_PAGE_BYTES, SyncAsset, SyncAssetKey, SyncProbe, SyncRefusal, SyncRefusalCode, SyncSeq,
    SyncSettledProposal, decode_sync_admin_key, decode_sync_charter, decode_sync_device_key,
    decode_sync_proposal, encode_sync_admin_key, encode_sync_charter, encode_sync_device_key,
    encode_sync_proposal, validate_coordinator_endpoint, validate_coordinator_endpoints,
    validate_member_endpoints,
};
pub use sync::{MAX_MEMBER_ENDPOINTS, MAX_SYNC_RECORDS, MemberEndpoint, SYNC_RECORD_CLASSES};

pub const PROTOCOL_VERSION: u16 = 19;
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
const PAIRING_QR_MAGIC: &[u8; 4] = b"GBQ\x02";
/// Base64url text of a 64-byte Ed25519 signature.
const SIGNATURE_TEXT_LENGTH: usize = 86;
/// Base64url text of a 16-byte card nonce.
const CARD_NONCE_TEXT_LENGTH: usize = 22;
/// Upper bound on the pasteable text form of a contact card.
const MAX_CARD_TEXT_BYTES: usize =
    ganbaru_contacts::CARD_TEXT_PREFIX.len() + ganbaru_contacts::MAX_CARD_BYTES.div_ceil(3) * 4;
/// Upper bound on a base64url PKCS#8 person key.
const MAX_PERSON_KEY_TEXT_BYTES: usize = 256;
/// Upper bound on the JSON-escaped payload of a [`ControlMessage::ChatLive`] answer, measured
/// by [`chat_live_payload_bytes`], so the whole frame stays well inside [`MAX_CONTROL_BYTES`].
pub const MAX_CHAT_LIVE_PAYLOAD_BYTES: usize = 640 * 1024;
/// Upper bound on a Chat identifier a [`ControlMessage::ChatLiveRead`] names.
const MAX_CHAT_IDENTIFIER_BYTES: usize = 160;
/// Error code a desktop answers a [`ControlMessage::ChatLiveRead`] with when it does not execute
/// the assignment or does not know it.
pub const CHAT_LIVE_NOT_HERE_CODE: &str = "chat_live_not_here";
/// Error code a desktop answers a [`ControlMessage::ChatLiveRead`] with when it could not read
/// the execution now.
pub const CHAT_LIVE_UNAVAILABLE_CODE: &str = "chat_live_unavailable";
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
    pub archive_bytes: u64,
    pub archive_sha256: String,
    /// Whether the sender registers no post-apply hooks, so the receiver runs every hook in
    /// full rebuild mode before the vault opens.
    pub derived_state_stale: bool,
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
        /// Admin key a desktop proposes authorization changes with once an admin promotes it;
        /// phones send none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sync_admin_key: Option<String>,
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
    },
    BundlePrepared {
        metadata: BundleMetadata,
    },
    DownloadBundle {
        metadata: BundleMetadata,
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
    ActivationComplete {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        transfer_id: String,
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
    /// Authenticated: a linked device reads the live execution of a Chat work assignment from
    /// the desktop that runs it. `after_change` is the change cursor of the previous answer, or
    /// `None` for the first read; `wait` asks the desktop to hold the answer until something
    /// changes or a short bound passes.
    ChatLiveRead {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        assignment_id: String,
        after_change: Option<u64>,
        wait: bool,
    },
    /// The answer to [`ControlMessage::ChatLiveRead`]: one bounded page of the execution as
    /// JSON, which the reader validates as the Chat live execution contract.
    ChatLive {
        payload: String,
        change_cursor: u64,
        has_more: bool,
        reset_required: bool,
        settled: bool,
    },
    /// Authenticated: a linked device compares its stored operations with the hub's, and
    /// reports the endpoints its own sync listener serves on, empty when it does not listen.
    SyncHello {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        manifest_version: u16,
        stored: Vec<SyncSeq>,
        probe: Option<SyncProbe>,
        listening: Vec<String>,
    },
    /// The hub's stored operations and its hash at the probed sequence, when it holds it, plus
    /// the member listener endpoints the hub knows, its own first, so a linked device follows
    /// address changes without relinking. `stable` lists the
    /// operations every counted device holds, and `bootstrap_required` tells the device that
    /// the hub no longer holds operations it lacks. `forked` lists the writers the hub holds
    /// fork evidence for, each with the sequence where its copies differ.
    SyncState {
        manifest_version: u16,
        stored: Vec<SyncSeq>,
        probe_hash: Option<String>,
        members: Vec<MemberEndpoint>,
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
    /// Authenticated: the operation of `writer` at the hub's stored head, which the device
    /// that sealed `writer` checks before it takes a claimed fork as proven.
    SyncWriterHead {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        writer: String,
    },
    /// The signed operation at the hub's stored head of the requested writer, or none when the
    /// hub holds none it may show.
    SyncWriterHeadOp {
        op: Option<String>,
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
    /// Authenticated: the anchor, while another device coordinates, asks the hub for the
    /// proposals the hub holds for it and reports the final rejections of proposals it took
    /// earlier. The hub answers only the anchor of its selected chain.
    SyncForward {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        settled: Vec<SyncSettledProposal>,
    },
    /// The proposals the hub holds for the anchor, oldest first.
    SyncForwarded {
        proposals: Vec<String>,
    },
    /// Authenticated: a page of the hub's authorization record store past `cursor`, the
    /// stored order the requester reached in each envelope class. Served to removed and held
    /// devices too.
    SyncRecords {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        cursor: Vec<u64>,
        max_bytes: u32,
    },
    SyncRecordPage {
        records: Vec<String>,
        cursor: Vec<u64>,
        more: bool,
    },
    /// Authenticated: stored envelopes whose envelope or entry hash is among `hashes`, and the
    /// certificates of `writers`.
    SyncRecordGet {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        hashes: Vec<String>,
        writers: Vec<String>,
    },
    SyncRecordFound {
        records: Vec<String>,
    },
    /// Authenticated: authorization records, certificates, and fork evidence the hub may lack.
    SyncRecordPush {
        protocol_version: u16,
        vault_id: String,
        device_id: String,
        records: Vec<String>,
    },
    /// How many pushed envelopes the hub did not hold, and what it still lacks that they
    /// reference.
    SyncRecordPushResult {
        admitted: u32,
        missing_hashes: Vec<String>,
        missing_writers: Vec<String>,
    },
    /// Opens the setup of an always-on anchor, or asks its version. A device may send it
    /// without a client certificate. Its shape and the challenge's stay stable across protocol
    /// versions, so each side can say which one needs an update.
    AnchorHello {
        protocol_version: u16,
        compatibility: HandoffCompatibility,
        client_nonce: String,
    },
    /// The server's versions and its nonce for the setup proof.
    AnchorChallenge {
        protocol_version: u16,
        compatibility: HandoffCompatibility,
        server_nonce: String,
    },
    /// Proves the setup secret, see [`anchor::anchor_setup_proof`].
    AnchorProof {
        proof: String,
        /// The vault the device sets up, or `None` when it recovers the vault the server runs.
        vault_id: Option<String>,
    },
    /// What the server tells a device that proved the secret: the keys it enrolls with, and
    /// the space it already serves, if any.
    AnchorIdentity {
        device_id: String,
        device_label: String,
        device_key: String,
        admin_key: String,
        space: Option<anchor::AnchorSpace>,
    },
    /// Uploads a snapshot of the vault over the setup connection. A [`Self::ResumeAt`] answer
    /// asks for the archive bytes, then a [`Self::BundleComplete`] closes them.
    AnchorUpload {
        metadata: BundleMetadata,
        charter: String,
        person_key: String,
        take_anchor: bool,
    },
    /// The server opened the uploaded snapshot and runs it.
    AnchorActivated {
        vault_id: String,
    },
    /// Asks the server's sequencer to take a proposal the recovery key signed, for a device
    /// that lost every other device of the space. [`Self::SyncProposed`] answers it.
    AnchorRecover {
        proposal: String,
    },
    /// Asks the server to enroll this desktop as an admin of the vault it runs, signed with the
    /// server's own admin key. Proving a fresh code already takes a shell on the server, which
    /// holds an admin key and the plain vault, so the code grants nothing the server lacks.
    /// [`Self::SyncProposed`] answers it, with the current revision when the log already enrolls
    /// these keys.
    AnchorJoin {
        device_id: String,
        device_label: String,
        device_key: String,
        admin_key: String,
        /// SHA-256 of the certificate this desktop dials members with, as lowercase hex.
        tls_fingerprint: String,
    },
    Error {
        code: String,
        message: String,
        retryable: bool,
    },
}

impl ControlMessage {
    /// Whether the request hands out vault content or the person key, which a device held by
    /// an anchor handover contest does not receive until the contest resolves.
    pub fn is_vault_material(&self) -> bool {
        matches!(
            self,
            Self::RequestBundle { .. }
                | Self::DownloadBundle { .. }
                | Self::PersonKeyRequest { .. }
                | Self::ChatLiveRead { .. }
        )
    }

    /// Whether this is a record exchange request, which every paired peer is served whatever
    /// the authorization log says of its device.
    pub fn is_record_exchange(&self) -> bool {
        matches!(
            self,
            Self::SyncRecords { .. } | Self::SyncRecordGet { .. } | Self::SyncRecordPush { .. }
        )
    }

    /// The vault and device a sync request names, or `None` for any other message.
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
            | Self::SyncWriterHead {
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
            }
            | Self::SyncForward {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncRecords {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncRecordGet {
                vault_id,
                device_id,
                ..
            }
            | Self::SyncRecordPush {
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
                sync_admin_key,
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
                if let Some(key) = sync_admin_key {
                    decode_sync_admin_key(key)?;
                }
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
                        decode_sync_charter(charter, person_key)?;
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
            } => {
                validate_protocol(*protocol_version)?;
                compatibility.validate()?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
            }
            Self::BundlePrepared { metadata }
            | Self::DownloadBundle { metadata }
            | Self::BundleMetadata { metadata } => metadata.validate()?,
            Self::ResumeAt { offset } => {
                if *offset > MAX_ARCHIVE_BYTES {
                    return Err("resume offset exceeds the archive limit".to_string());
                }
            }
            Self::BundleComplete { transfer_id }
            | Self::ActivationAcknowledged { transfer_id }
            | Self::CancelTransfer { transfer_id }
            | Self::TransferCancelled { transfer_id } => {
                validate_identifier("transfer id", transfer_id)?;
            }
            Self::ActivationComplete {
                protocol_version,
                vault_id,
                device_id,
                transfer_id,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                validate_identifier("transfer id", transfer_id)?;
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
            Self::ChatLiveRead {
                protocol_version,
                vault_id,
                device_id,
                assignment_id,
                after_change,
                wait: _,
            } => {
                validate_protocol(*protocol_version)?;
                validate_identifier("vault id", vault_id)?;
                validate_identifier("device id", device_id)?;
                validate_chat_identifier("assignment id", assignment_id)?;
                if after_change.is_some_and(|cursor| cursor > i64::MAX as u64) {
                    return Err("chat live change cursor is invalid".to_string());
                }
            }
            Self::ChatLive {
                payload,
                change_cursor,
                ..
            } => {
                if payload.is_empty()
                    || chat_live_payload_bytes(payload) > MAX_CHAT_LIVE_PAYLOAD_BYTES
                {
                    return Err("chat live payload is empty or too large".to_string());
                }
                if *change_cursor > i64::MAX as u64 {
                    return Err("chat live change cursor is invalid".to_string());
                }
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
                listening,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_vector(stored)?;
                if let Some(probe) = probe {
                    sync::validate_probe(probe)?;
                }
                sync::validate_coordinator_endpoints(listening)?;
            }
            Self::SyncState {
                manifest_version: _,
                stored,
                probe_hash,
                members,
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
                sync::validate_member_endpoints(members)?;
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
            Self::SyncWriterHead {
                protocol_version,
                vault_id,
                device_id,
                writer,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_writer(writer)?;
            }
            Self::SyncWriterHeadOp { op } => {
                if let Some(op) = op {
                    sync::validate_ops(std::slice::from_ref(op))?;
                }
            }
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
            Self::SyncForward {
                protocol_version,
                vault_id,
                device_id,
                settled,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_settled(settled)?;
            }
            Self::SyncForwarded { proposals } => sync::validate_forwarded(proposals)?,
            Self::SyncRecords {
                protocol_version,
                vault_id,
                device_id,
                cursor,
                max_bytes,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_record_cursor(cursor)?;
                sync::validate_page_bytes(*max_bytes)?;
            }
            Self::SyncRecordPage {
                records,
                cursor,
                more: _,
            } => {
                sync::validate_records(records)?;
                sync::validate_record_cursor(cursor)?;
            }
            Self::SyncRecordGet {
                protocol_version,
                vault_id,
                device_id,
                hashes,
                writers,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_record_wants(hashes, writers)?;
            }
            Self::SyncRecordFound { records } => sync::validate_records(records)?,
            Self::SyncRecordPush {
                protocol_version,
                vault_id,
                device_id,
                records,
            } => {
                validate_sync_peer(*protocol_version, vault_id, device_id)?;
                sync::validate_records(records)?;
            }
            Self::SyncRecordPushResult {
                admitted,
                missing_hashes,
                missing_writers,
            } => {
                sync::validate_record_wants(missing_hashes, missing_writers)?;
                if *admitted as usize > sync::MAX_SYNC_RECORDS {
                    return Err("sync record admission count is invalid".to_string());
                }
            }
            Self::AnchorHello {
                compatibility,
                client_nonce: nonce,
                ..
            }
            | Self::AnchorChallenge {
                compatibility,
                server_nonce: nonce,
                ..
            } => {
                compatibility.validate()?;
                anchor::decode_anchor_nonce(nonce)?;
            }
            Self::AnchorProof { proof, vault_id } => {
                anchor::decode_anchor_proof(proof)?;
                if let Some(vault_id) = vault_id {
                    validate_identifier("vault id", vault_id)?;
                }
            }
            Self::AnchorIdentity {
                device_id,
                device_label,
                device_key,
                admin_key,
                space,
            } => {
                validate_identifier("server device id", device_id)?;
                validate_device_label("server device label", device_label)?;
                decode_sync_device_key(device_key)?;
                decode_sync_admin_key(admin_key)?;
                if let Some(space) = space {
                    space.validate()?;
                }
            }
            Self::AnchorUpload {
                metadata,
                charter,
                person_key,
                ..
            } => {
                metadata.validate()?;
                decode_sync_charter(charter, person_key)?;
            }
            Self::AnchorActivated { vault_id } => validate_identifier("vault id", vault_id)?,
            Self::AnchorRecover { proposal } => decode_sync_proposal(proposal).map(drop)?,
            Self::AnchorJoin {
                device_id,
                device_label,
                device_key,
                admin_key,
                tls_fingerprint,
            } => {
                validate_identifier("device id", device_id)?;
                validate_device_label("device label", device_label)?;
                decode_sync_device_key(device_key)?;
                decode_sync_admin_key(admin_key)?;
                validate_certificate_fingerprint("device fingerprint", tls_fingerprint)?;
            }
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

/// A message carried in one length-prefixed JSON frame. Every frame is validated before it is
/// encoded and after it is decoded, so a peer never sends or acts on a message outside its bounds.
pub trait Frame: Serialize + DeserializeOwned {
    /// What the frame is called in errors.
    const LABEL: &'static str;

    /// Checks every bound and text form of the message.
    fn validate(&self) -> Result<(), String>;
}

impl Frame for ControlMessage {
    const LABEL: &'static str = "handoff control message";

    fn validate(&self) -> Result<(), String> {
        ControlMessage::validate(self)
    }
}

/// Writes one frame: a big-endian u32 length, then the JSON encoding of a valid `message` no
/// larger than [`MAX_CONTROL_BYTES`].
pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
    T: Frame,
{
    message.validate()?;
    let label = T::LABEL;
    let encoded =
        serde_json::to_vec(message).map_err(|error| format!("encode {label}: {error}"))?;
    if encoded.len() > MAX_CONTROL_BYTES {
        return Err(format!("{label} exceeds the size limit"));
    }
    writer
        .write_u32(encoded.len() as u32)
        .await
        .map_err(|error| format!("write {label} length: {error}"))?;
    writer
        .write_all(&encoded)
        .await
        .map_err(|error| format!("write {label}: {error}"))?;
    writer
        .flush()
        .await
        .map_err(|error| format!("flush {label}: {error}"))
}

/// Reads one frame no larger than `max_bytes`, which never exceeds [`MAX_CONTROL_BYTES`]. The
/// length is checked before the frame is allocated, and the decoded message is validated.
pub async fn read_frame<R, T>(reader: &mut R, max_bytes: usize) -> Result<T, String>
where
    R: AsyncRead + Unpin,
    T: Frame,
{
    let length = read_frame_length::<R, T>(reader, max_bytes).await?;
    read_frame_body(reader, length).await
}

/// Reads the length prefix of one frame and checks it against `max_bytes`, which never exceeds
/// [`MAX_CONTROL_BYTES`]. A caller that budgets memory reserves the returned length before it
/// reads the body with [`read_frame_body`].
pub async fn read_frame_length<R, T>(reader: &mut R, max_bytes: usize) -> Result<usize, String>
where
    R: AsyncRead + Unpin,
    T: Frame,
{
    let label = T::LABEL;
    let length = reader
        .read_u32()
        .await
        .map_err(|error| format!("read {label} length: {error}"))? as usize;
    if length == 0 || length > max_bytes.min(MAX_CONTROL_BYTES) {
        return Err(format!("{label} has an invalid size"));
    }
    Ok(length)
}

/// Reads and validates the body of a frame whose length [`read_frame_length`] returned.
pub async fn read_frame_body<R, T>(reader: &mut R, length: usize) -> Result<T, String>
where
    R: AsyncRead + Unpin,
    T: Frame,
{
    let label = T::LABEL;
    if length == 0 || length > MAX_CONTROL_BYTES {
        return Err(format!("{label} has an invalid size"));
    }
    let mut encoded = vec![0_u8; length];
    reader
        .read_exact(&mut encoded)
        .await
        .map_err(|error| format!("read {label}: {error}"))?;
    let message: T = decode_bounded_json(&encoded, label)?;
    message.validate()?;
    Ok(message)
}

pub async fn write_control<W>(writer: &mut W, message: &ControlMessage) -> Result<(), String>
where
    W: AsyncWrite + Unpin,
{
    write_frame(writer, message).await
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
    read_frame(reader, max_bytes).await
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

/// Checks a Chat identifier such as a work assignment id: generated Chat ids join a prefix and
/// hex digits with `:`, so they fall outside [`validate_identifier`].
fn validate_chat_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_CHAT_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

/// How many bytes `payload` takes once encoded as a JSON string inside a frame, quotes
/// included: quotes and backslashes escape to two bytes and other control characters to six.
pub fn chat_live_payload_bytes(payload: &str) -> usize {
    2 + payload
        .bytes()
        .map(|byte| match byte {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            0x00..=0x1f => 6,
            _ => 1,
        })
        .sum::<usize>()
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
mod tests;

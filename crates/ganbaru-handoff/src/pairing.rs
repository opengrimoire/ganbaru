//! Platform-private pairing identity and membership.
//!
//! Membership and transfer operations share one mutex and one durable schema.
//! Child modules organize implementation without introducing separate state owners.

mod storage;
#[cfg(all(test, desktop))]
mod tests;
mod transfers;

pub use storage::write_private_file_atomically;
use storage::{
    persist_initialized_state, persist_state, quarantine_invalid_labels, read_state,
    recover_private_state, validate_state,
};

#[cfg(desktop)]
use super::protocol::HandoffCompatibility;
#[cfg(desktop)]
use super::protocol::PROTOCOL_VERSION;
use super::protocol::{
    BundleMetadata, BundlePurpose, DeviceKind, PairingInvitation, validate_device_label,
    validate_identifier,
};
use base64::Engine;
use rcgen::{CertificateParams, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const PAIRING_STATE_FILE: &str = "vault-handoff.json";
const PAIRING_STATE_SCHEMA_VERSION: u32 = 2;
const STAGING_DIRECTORY: &str = "vault-handoff-staging";
#[cfg(desktop)]
const INVITATION_LIFETIME_MS: i64 = 3 * 60 * 1_000;
const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;
const MAX_LINKED_PEERS: usize = 32;
const MAX_REVOKED_PEERS: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredIdentity {
    device_id: String,
    certificate: String,
    private_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedPeer {
    pub device_id: String,
    pub device_label: String,
    #[serde(default)]
    pub device_kind: DeviceKind,
    pub vault_id: String,
    pub certificate: String,
    pub certificate_fingerprint: String,
    /// Enrollment time; at the stored limit, the oldest memberships of other vaults are revoked first.
    #[serde(default)]
    pub linked_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorPin {
    pub device_id: String,
    #[serde(default)]
    pub device_label: Option<String>,
    pub endpoint: String,
    pub vault_id: String,
    #[serde(default)]
    pub generation: u64,
    pub certificate_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RevokedPeer {
    device_id: String,
    vault_id: String,
    certificate: String,
    certificate_fingerprint: String,
    revoked_at_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingStateFile {
    schema_version: u32,
    identity: StoredIdentity,
    #[serde(default)]
    linked_peers: BTreeMap<String, LinkedPeer>,
    #[serde(default)]
    revoked_peers: BTreeMap<String, RevokedPeer>,
    coordinator: Option<CoordinatorPin>,
    #[serde(default)]
    revoked_by_coordinator: bool,
    #[serde(default)]
    replica_ready: bool,
    #[serde(default)]
    pending_acknowledgement: Option<PendingAcknowledgement>,
    #[serde(default)]
    outgoing_transfer: Option<StoredOutgoingTransfer>,
    #[serde(default)]
    incoming_transfer: Option<StoredIncomingTransfer>,
    #[serde(default)]
    completed_activation: Option<PendingAcknowledgement>,
    #[serde(default)]
    requested_upload: Option<BundlePurpose>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAcknowledgement {
    pub vault_id: String,
    pub device_id: String,
    pub transfer_id: String,
    pub generation: u64,
    pub purpose: BundlePurpose,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredOutgoingTransfer {
    pub metadata: BundleMetadata,
    pub purpose: BundlePurpose,
    pub committed: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredIncomingTransfer {
    pub metadata: BundleMetadata,
    pub source_device_id: String,
    pub purpose: BundlePurpose,
}

pub struct TlsIdentity {
    pub certificate: CertificateDer<'static>,
    pub private_key: PrivateKeyDer<'static>,
}

impl Clone for TlsIdentity {
    fn clone(&self) -> Self {
        Self {
            certificate: self.certificate.clone(),
            private_key: self.private_key.clone_key(),
        }
    }
}

impl std::fmt::Debug for TlsIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TlsIdentity")
            .field("certificate_bytes", &self.certificate.as_ref().len())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
#[cfg(desktop)]
struct PendingInvitation {
    invitation: PairingInvitation,
}

#[derive(Clone, Debug)]
#[cfg(desktop)]
pub struct RegisteredBundle {
    pub metadata: BundleMetadata,
    pub path: PathBuf,
}

#[cfg(desktop)]
pub struct Enrollment<'a> {
    pub invitation_id: &'a str,
    pub secret: &'a str,
    pub vault_id: &'a str,
    pub device_id: &'a str,
    pub device_label: &'a str,
    pub device_kind: DeviceKind,
    pub certificate_b64: &'a str,
}

#[derive(Default)]
struct PairingManagerInner {
    state_path: Option<PathBuf>,
    staging_root: Option<PathBuf>,
    state: Option<PairingStateFile>,
    /// Why startup disabled linking; pairing operations report it instead of running.
    disabled_reason: Option<String>,
    #[cfg(desktop)]
    invitations: BTreeMap<String, PendingInvitation>,
    #[cfg(desktop)]
    outgoing_bundles: BTreeMap<String, RegisteredBundle>,
}

/// Durable membership and transfer progress with process-local invitations and bundles.
#[derive(Clone, Default)]
pub struct PairingManager {
    inner: std::sync::Arc<Mutex<PairingManagerInner>>,
}

impl PairingManager {
    pub fn initialize(&self, app_config_dir: PathBuf, device_id: String) -> Result<(), String> {
        validate_identifier("device id", &device_id)?;
        fs::create_dir_all(&app_config_dir)
            .map_err(|error| format!("create app config directory: {error}"))?;
        let state_path = app_config_dir.join(PAIRING_STATE_FILE);
        recover_private_state(&state_path)?;
        let staging_root = app_config_dir.join(STAGING_DIRECTORY);
        fs::create_dir_all(&staging_root)
            .map_err(|error| format!("create handoff staging directory: {error}"))?;

        let state = if state_path.exists() {
            let mut state = read_state(&state_path)?;
            if quarantine_invalid_labels(&mut state) {
                eprintln!("pairing state held invalid device labels; they were replaced");
                persist_state(&state_path, &state)?;
            }
            state
        } else {
            let state = PairingStateFile {
                schema_version: PAIRING_STATE_SCHEMA_VERSION,
                identity: create_identity(device_id.clone())?,
                linked_peers: BTreeMap::new(),
                revoked_peers: BTreeMap::new(),
                coordinator: None,
                revoked_by_coordinator: false,
                replica_ready: false,
                pending_acknowledgement: None,
                outgoing_transfer: None,
                incoming_transfer: None,
                completed_activation: None,
                requested_upload: None,
            };
            persist_state(&state_path, &state)?;
            state
        };
        validate_state(&state, &device_id)?;

        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "pairing state lock is unavailable".to_string())?;
        inner.state_path = Some(state_path);
        inner.staging_root = Some(staging_root);
        inner.state = Some(state);
        inner.disabled_reason = None;
        #[cfg(desktop)]
        {
            inner.invitations.clear();
            inner.outgoing_bundles.clear();
        }
        Ok(())
    }

    /// Turns linking off for this run after startup could not load or reconcile pairing state.
    /// The state file is left untouched for diagnosis, and every pairing operation then fails
    /// with [`LINKING_DISABLED_ERROR`], so the rest of the app still starts.
    pub fn disable(&self, reason: String) -> Result<(), String> {
        let mut inner = self.lock()?;
        inner.state = None;
        inner.state_path = None;
        inner.staging_root = None;
        inner.disabled_reason = Some(reason);
        #[cfg(desktop)]
        {
            inner.invitations.clear();
            inner.outgoing_bundles.clear();
        }
        Ok(())
    }

    pub fn identity(&self) -> Result<(String, TlsIdentity), String> {
        let inner = self.lock()?;
        let state = initialized_state(&inner)?;
        Ok((
            state.identity.device_id.clone(),
            decode_identity(&state.identity)?,
        ))
    }

    pub fn linked_peers(&self) -> Result<Vec<LinkedPeer>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .linked_peers
            .values()
            .cloned()
            .collect())
    }

    /// Devices linked to `vault_id`. Memberships recorded for other vaults stay stored but inactive.
    pub fn linked_peers_for(&self, vault_id: &str) -> Result<Vec<LinkedPeer>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .linked_peers
            .values()
            .filter(|peer| peer.vault_id == vault_id)
            .cloned()
            .collect())
    }

    pub fn linked_peer(&self, device_id: &str) -> Result<Option<LinkedPeer>, String> {
        validate_identifier("peer device id", device_id)?;
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .linked_peers
            .get(device_id)
            .cloned())
    }

    pub fn coordinator_pin(&self) -> Result<Option<CoordinatorPin>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.coordinator.clone())
    }

    /// Whether this device shares `vault_id` with a linked peer or a pinned coordinator.
    pub fn is_linked_for(&self, vault_id: &str) -> Result<bool, String> {
        let inner = self.lock()?;
        let state = initialized_state(&inner)?;
        if state.revoked_by_coordinator {
            return Ok(false);
        }
        Ok(state
            .coordinator
            .as_ref()
            .is_some_and(|coordinator| coordinator.vault_id == vault_id)
            || state
                .linked_peers
                .values()
                .any(|peer| peer.vault_id == vault_id))
    }

    pub fn revoked_by_coordinator(&self) -> Result<bool, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.revoked_by_coordinator)
    }

    /// Devices this coordinator unlinked from `vault_id`, whose sync writers it revokes.
    pub fn revoked_device_ids(
        &self,
        vault_id: &str,
    ) -> Result<std::collections::BTreeSet<String>, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .revoked_peers
            .values()
            .filter(|peer| peer.vault_id == vault_id)
            .map(|peer| peer.device_id.clone())
            .collect())
    }

    #[cfg(desktop)]
    pub fn client_auth_certificates(&self) -> Result<Vec<CertificateDer<'static>>, String> {
        let inner = self.lock()?;
        let state = initialized_state(&inner)?;
        state
            .linked_peers
            .values()
            .map(|peer| decode_certificate(&peer.certificate))
            .chain(
                state
                    .revoked_peers
                    .values()
                    .map(|peer| decode_certificate(&peer.certificate)),
            )
            .collect()
    }

    #[cfg(desktop)]
    pub fn is_revoked_certificate(
        &self,
        certificate: Option<&CertificateDer<'_>>,
    ) -> Result<bool, String> {
        let Some(certificate) = certificate else {
            return Ok(false);
        };
        let fingerprint = certificate_fingerprint(certificate.as_ref());
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?
            .revoked_peers
            .values()
            .any(|peer| peer.certificate_fingerprint == fingerprint))
    }

    pub fn ensure_enrollment_target(&self, invitation: &PairingInvitation) -> Result<(), String> {
        let inner = self.lock()?;
        ensure_enrollment_target(initialized_state(&inner)?, invitation)
    }

    pub fn ensure_can_unlink(&self) -> Result<(), String> {
        if self.has_pending_transfer()? {
            return Err("finish or retry the active handoff before unlinking".to_string());
        }
        Ok(())
    }

    pub fn unlink(&self) -> Result<(), String> {
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        if previous.pending_acknowledgement.is_some()
            || previous.outgoing_transfer.is_some()
            || previous.incoming_transfer.is_some()
            || previous.requested_upload.is_some()
        {
            return Err("finish or retry the active handoff before unlinking".to_string());
        }
        let state = initialized_state_mut(&mut inner)?;
        let removed_peers = std::mem::take(&mut state.linked_peers);
        for peer in removed_peers.into_values() {
            remember_revoked_peer(state, peer, super::protocol::unix_time_ms());
        }
        state.coordinator = None;
        state.revoked_by_coordinator = false;
        state.replica_ready = false;
        state.completed_activation = None;
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        #[cfg(desktop)]
        {
            inner.invitations.clear();
            inner.outgoing_bundles.clear();
        }
        Ok(())
    }

    pub fn unlink_device(&self, device_id: &str) -> Result<(), String> {
        validate_identifier("peer device id", device_id)?;
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        if previous.pending_acknowledgement.is_some()
            || previous.outgoing_transfer.is_some()
            || previous.incoming_transfer.is_some()
            || previous.requested_upload.is_some()
        {
            return Err("finish or retry the active handoff before unlinking".to_string());
        }
        let state = initialized_state_mut(&mut inner)?;
        let removed_peer = state.linked_peers.remove(device_id);
        let removed_peer_exists = removed_peer.is_some();
        if let Some(peer) = removed_peer {
            remember_revoked_peer(state, peer, super::protocol::unix_time_ms());
        }
        let removed_coordinator = state
            .coordinator
            .as_ref()
            .is_some_and(|coordinator| coordinator.device_id == device_id);
        if removed_coordinator {
            state.coordinator = None;
            state.revoked_by_coordinator = false;
            state.replica_ready = false;
            state.completed_activation = None;
        }
        if !removed_peer_exists && !removed_coordinator {
            return Err("linked device was not found".to_string());
        }
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        #[cfg(desktop)]
        if removed_peer_exists {
            inner.invitations.clear();
        }
        Ok(())
    }

    pub fn accept_coordinator_revocation(&self, expected: &CoordinatorPin) -> Result<(), String> {
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        let state = initialized_state_mut(&mut inner)?;
        let matches = state.coordinator.as_ref().is_some_and(|coordinator| {
            coordinator.device_id == expected.device_id
                && coordinator.vault_id == expected.vault_id
                && coordinator.certificate_fingerprint == expected.certificate_fingerprint
        });
        if !matches {
            return Err("coordinator changed before revocation could be applied".to_string());
        }
        state.coordinator = None;
        state.revoked_by_coordinator = true;
        state.pending_acknowledgement = None;
        state.outgoing_transfer = None;
        state.incoming_transfer = None;
        state.completed_activation = None;
        state.requested_upload = None;
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        Ok(())
    }

    #[cfg(desktop)]
    pub fn create_invitation(
        &self,
        endpoint: std::net::SocketAddr,
        vault_id: String,
        generation: u64,
        compatibility: HandoffCompatibility,
        now_ms: i64,
    ) -> Result<PairingInvitation, String> {
        validate_identifier("vault id", &vault_id)?;
        compatibility.validate()?;
        let mut inner = self.lock()?;
        let state = initialized_state(&inner)?;
        let invitation = PairingInvitation {
            protocol_version: PROTOCOL_VERSION,
            compatibility,
            invitation_id: random_token("invite")?,
            secret: random_secret()?,
            endpoint: endpoint.to_string(),
            coordinator_fingerprint: certificate_fingerprint(
                decode_certificate(&state.identity.certificate)?.as_ref(),
            ),
            coordinator_device_id: state.identity.device_id.clone(),
            vault_id,
            generation,
            expires_at_ms: now_ms.saturating_add(INVITATION_LIFETIME_MS),
        };
        inner.invitations.clear();
        inner.invitations.insert(
            invitation.invitation_id.clone(),
            PendingInvitation {
                invitation: invitation.clone(),
            },
        );
        Ok(invitation)
    }

    #[cfg(desktop)]
    pub fn enroll_peer(
        &self,
        enrollment: Enrollment<'_>,
        now_ms: i64,
    ) -> Result<LinkedPeer, String> {
        validate_identifier("device id", enrollment.device_id)?;
        validate_device_label("device label", enrollment.device_label)?;
        let certificate = decode_certificate(enrollment.certificate_b64)?;
        let mut inner = self.lock()?;
        let pending = inner
            .invitations
            .get(enrollment.invitation_id)
            .ok_or_else(|| "pairing invitation is unknown or already used".to_string())?;
        pending.invitation.validate(now_ms)?;
        if pending.invitation.vault_id != enrollment.vault_id
            || !constant_time_equal(
                pending.invitation.secret.as_bytes(),
                enrollment.secret.as_bytes(),
            )
        {
            return Err("pairing invitation credentials are invalid".to_string());
        }
        let peer = LinkedPeer {
            device_id: enrollment.device_id.to_string(),
            device_label: enrollment.device_label.to_string(),
            device_kind: enrollment.device_kind,
            vault_id: enrollment.vault_id.to_string(),
            certificate: enrollment.certificate_b64.to_string(),
            certificate_fingerprint: certificate_fingerprint(certificate.as_ref()),
            linked_at_ms: now_ms,
        };
        let previous = initialized_state(&inner)?.clone();
        let state = initialized_state_mut(&mut inner)?;
        if peer.device_id == state.identity.device_id {
            return Err("a device cannot enroll its coordinator identity".to_string());
        }
        // A device follows one coordinator vault at a time, so enrolling it under another vault
        // replaces its earlier membership.
        let evicted_device_id = if let Some(existing) = state.linked_peers.get(&peer.device_id) {
            if existing.certificate_fingerprint != peer.certificate_fingerprint {
                return Err(
                    "this device identity conflicts with an existing linked device".to_string(),
                );
            }
            None
        } else {
            if state
                .linked_peers
                .values()
                .any(|existing| existing.certificate_fingerprint == peer.certificate_fingerprint)
            {
                return Err("this certificate already belongs to another linked device".to_string());
            }
            if state.linked_peers.len() >= MAX_LINKED_PEERS {
                Some(
                    oldest_membership_outside(state, &peer.vault_id).ok_or_else(|| {
                        format!("at most {MAX_LINKED_PEERS} devices can be linked to one vault")
                    })?,
                )
            } else {
                None
            }
        };
        if state.revoked_peers.values().any(|revoked| {
            revoked.device_id != peer.device_id
                && revoked.certificate_fingerprint == peer.certificate_fingerprint
        }) {
            return Err("this certificate belonged to another revoked device".to_string());
        }
        if let Some(evicted) = evicted_device_id.and_then(|id| state.linked_peers.remove(&id)) {
            remember_revoked_peer(state, evicted, now_ms);
        }
        state
            .linked_peers
            .insert(peer.device_id.clone(), peer.clone());
        state.revoked_peers.remove(&peer.device_id);
        state
            .revoked_peers
            .retain(|_, revoked| revoked.certificate_fingerprint != peer.certificate_fingerprint);
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        inner.invitations.remove(enrollment.invitation_id);
        Ok(peer)
    }

    pub fn record_coordinator(
        &self,
        invitation: &PairingInvitation,
        coordinator_certificate: &[u8],
        coordinator_device_label: Option<String>,
    ) -> Result<(), String> {
        let fingerprint = certificate_fingerprint(coordinator_certificate);
        if fingerprint != invitation.coordinator_fingerprint {
            return Err("coordinator certificate does not match the QR invitation".to_string());
        }
        if let Some(label) = coordinator_device_label.as_deref() {
            validate_device_label("coordinator device label", label)?;
        }
        let mut inner = self.lock()?;
        let state = initialized_state_mut(&mut inner)?;
        ensure_enrollment_target(state, invitation)?;
        state.coordinator = Some(CoordinatorPin {
            device_id: invitation.coordinator_device_id.clone(),
            device_label: coordinator_device_label,
            endpoint: invitation.endpoint.clone(),
            vault_id: invitation.vault_id.clone(),
            generation: invitation.generation,
            certificate_fingerprint: fingerprint,
        });
        state.revoked_by_coordinator = false;
        persist_initialized_state(&inner)
    }

    #[cfg(desktop)]
    pub fn verify_authenticated_peer(
        &self,
        device_id: &str,
        certificate: Option<&CertificateDer<'_>>,
        vault_id: &str,
    ) -> Result<(), String> {
        let certificate = certificate
            .ok_or_else(|| "authenticated client certificate is required".to_string())?;
        let inner = self.lock()?;
        let peer = initialized_state(&inner)?
            .linked_peers
            .get(device_id)
            .ok_or_else(|| "this device is not linked".to_string())?;
        if peer.device_id != device_id
            || peer.vault_id != vault_id
            || peer.certificate_fingerprint != certificate_fingerprint(certificate.as_ref())
        {
            return Err("authenticated device identity does not match the request".to_string());
        }
        Ok(())
    }

    #[cfg(desktop)]
    pub fn authenticated_peer(
        &self,
        certificate: Option<&CertificateDer<'_>>,
    ) -> Result<LinkedPeer, String> {
        let certificate = certificate
            .ok_or_else(|| "authenticated client certificate is required".to_string())?;
        let fingerprint = certificate_fingerprint(certificate.as_ref());
        let inner = self.lock()?;
        initialized_state(&inner)?
            .linked_peers
            .values()
            .find(|peer| peer.certificate_fingerprint == fingerprint)
            .cloned()
            .ok_or_else(|| "authenticated device is not linked".to_string())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, PairingManagerInner>, String> {
        self.inner
            .lock()
            .map_err(|_| "pairing state lock is unavailable".to_string())
    }
}

/// Prefix of the error every pairing operation returns after [`PairingManager::disable`].
pub const LINKING_DISABLED_ERROR: &str =
    "vault linking is disabled because its saved state could not be loaded";

fn uninitialized_error(inner: &PairingManagerInner) -> String {
    match inner.disabled_reason.as_deref() {
        Some(reason) => format!("{LINKING_DISABLED_ERROR}: {reason}"),
        None => "pairing state is not initialized".to_string(),
    }
}

fn initialized_state(inner: &PairingManagerInner) -> Result<&PairingStateFile, String> {
    inner
        .state
        .as_ref()
        .ok_or_else(|| uninitialized_error(inner))
}

fn initialized_state_mut(inner: &mut PairingManagerInner) -> Result<&mut PairingStateFile, String> {
    if inner.state.is_none() {
        return Err(uninitialized_error(inner));
    }
    inner
        .state
        .as_mut()
        .ok_or_else(|| "pairing state is not initialized".to_string())
}

fn ensure_enrollment_target(
    state: &PairingStateFile,
    invitation: &PairingInvitation,
) -> Result<(), String> {
    match &state.coordinator {
        Some(coordinator)
            if coordinator.device_id == invitation.coordinator_device_id
                && coordinator.vault_id == invitation.vault_id
                && coordinator
                    .certificate_fingerprint
                    .eq_ignore_ascii_case(&invitation.coordinator_fingerprint) =>
        {
            Ok(())
        }
        Some(_) => Err(
            "this device is already linked to another coordinator; unlink it before linking to a different coordinator"
                .to_string(),
        ),
        None if !state.linked_peers.is_empty() => Err(
            "unlink coordinated devices before linking this device to another coordinator"
                .to_string(),
        ),
        None => Ok(()),
    }
}

fn create_identity(device_id: String) -> Result<StoredIdentity, String> {
    let signing_key =
        KeyPair::generate().map_err(|error| format!("generate device key: {error}"))?;
    let certificate = CertificateParams::new(vec!["ganbaru-device.local".to_string()])
        .map_err(|error| format!("configure device certificate: {error}"))?
        .self_signed(&signing_key)
        .map_err(|error| format!("generate device certificate: {error}"))?;
    Ok(StoredIdentity {
        device_id,
        certificate: base64::engine::general_purpose::STANDARD.encode(certificate.der().as_ref()),
        private_key: base64::engine::general_purpose::STANDARD.encode(signing_key.serialize_der()),
    })
}

fn decode_identity(identity: &StoredIdentity) -> Result<TlsIdentity, String> {
    let certificate = decode_certificate(&identity.certificate)?;
    let private_key = base64::engine::general_purpose::STANDARD
        .decode(&identity.private_key)
        .map_err(|error| format!("decode device private key: {error}"))?;
    if private_key.is_empty() || private_key.len() > MAX_CERTIFICATE_BYTES {
        return Err("device private key has an invalid size".to_string());
    }
    Ok(TlsIdentity {
        certificate,
        private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key)),
    })
}

pub fn decode_certificate(encoded: &str) -> Result<CertificateDer<'static>, String> {
    let certificate = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("decode device certificate: {error}"))?;
    if certificate.is_empty() || certificate.len() > MAX_CERTIFICATE_BYTES {
        return Err("device certificate has an invalid size".to_string());
    }
    Ok(CertificateDer::from(certificate))
}

pub fn encode_certificate(certificate: &CertificateDer<'_>) -> String {
    base64::engine::general_purpose::STANDARD.encode(certificate.as_ref())
}

pub fn certificate_fingerprint(certificate: &[u8]) -> String {
    let digest = Sha256::digest(certificate);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(desktop)]
fn random_secret() -> Result<String, String> {
    let mut random = [0_u8; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut random)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random))
}

pub fn random_token(prefix: &str) -> Result<String, String> {
    let mut random = [0_u8; 16];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut random)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    Ok(format!(
        "{prefix}-{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random)
    ))
}

#[cfg(desktop)]
fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

/// The least recently enrolled device linked to a vault other than `vault_id`.
#[cfg(desktop)]
fn oldest_membership_outside(state: &PairingStateFile, vault_id: &str) -> Option<String> {
    state
        .linked_peers
        .values()
        .filter(|peer| peer.vault_id != vault_id)
        .min_by_key(|peer| (peer.linked_at_ms, peer.device_id.as_str()))
        .map(|peer| peer.device_id.clone())
}

fn remember_revoked_peer(state: &mut PairingStateFile, peer: LinkedPeer, revoked_at_ms: i64) {
    state.revoked_peers.insert(
        peer.device_id.clone(),
        RevokedPeer {
            device_id: peer.device_id,
            vault_id: peer.vault_id,
            certificate: peer.certificate,
            certificate_fingerprint: peer.certificate_fingerprint,
            revoked_at_ms,
        },
    );
    while state.revoked_peers.len() > MAX_REVOKED_PEERS {
        let Some(oldest_device_id) = state
            .revoked_peers
            .values()
            .min_by_key(|peer| (peer.revoked_at_ms, peer.device_id.as_str()))
            .map(|peer| peer.device_id.clone())
        else {
            break;
        };
        state.revoked_peers.remove(&oldest_device_id);
    }
}

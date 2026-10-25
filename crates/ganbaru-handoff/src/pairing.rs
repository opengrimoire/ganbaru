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
use super::protocol::{
    BundleMetadata, BundlePurpose, DeviceKind, PairingInvitation, decode_sync_charter,
    validate_coordinator_endpoints, validate_device_label, validate_discovery_key,
    validate_identifier,
};
#[cfg(desktop)]
use super::protocol::{DISCOVERY_KEY_BYTES, PROTOCOL_VERSION, decode_sync_device_key};
use base64::Engine;
use rcgen::{CertificateParams, DnType, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const PAIRING_STATE_FILE: &str = "vault-handoff.json";
const PAIRING_STATE_SCHEMA_VERSION: u32 = 2;
const STAGING_DIRECTORY: &str = "vault-handoff-staging";
#[cfg(desktop)]
const INVITATION_LIFETIME_MS: i64 = 3 * 60 * 1_000;
const MAX_CERTIFICATE_BYTES: usize = 16 * 1024;
const MAX_LINKED_PEERS: usize = 32;
const MAX_REVOKED_PEERS: usize = 64;
/// Most device certificates the coordinator pins at once, linked and revoked together. Every
/// TLS server configuration and handshake check scales with this set.
const MAX_PINNED_DEVICE_CERTIFICATES: usize = 128;
const _: () = assert!(MAX_LINKED_PEERS + MAX_REVOKED_PEERS <= MAX_PINNED_DEVICE_CERTIFICATES);
const ENROLLMENT_CODE_DOMAIN: &[u8] = b"ganbaru-enrollment-code-v1";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredIdentity {
    device_id: String,
    certificate: String,
    /// The PKCS#8 key while it stays in this private file; `None` once it lives in the
    /// [`IdentityKeyStore`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    private_key: Option<String>,
}

impl std::fmt::Debug for StoredIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredIdentity")
            .field("device_id", &self.device_id)
            .field("certificate", &self.certificate)
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Device-local secret storage for the TLS private key, such as the operating-system keyring.
/// Calls may block on platform services.
pub trait IdentityKeyStore: Send + Sync {
    /// The stored PKCS#8 key, or `None` when this device has no entry.
    fn read(&self, device_id: &str) -> Result<Option<Vec<u8>>, String>;
    fn write(&self, device_id: &str, pkcs8: &[u8]) -> Result<(), String>;
    fn remove(&self, device_id: &str) -> Result<(), String>;
}

/// Prefix of the error returned while the key store holding the TLS key cannot be read.
pub const DEVICE_KEY_UNAVAILABLE_ERROR: &str =
    "this device's linking key could not be read from the system keyring";
/// Error returned when a linked device's TLS key is gone from its key store. Unlinking lets the
/// next use create a fresh identity, because no peer pins it anymore.
pub const DEVICE_KEY_MISSING_ERROR: &str = "this device's linking key is missing from the system keyring; unlink this device and link it again";

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
    /// Public half of the device key the device enrolls in the vault's sync space, base64url.
    pub sync_device_key: String,
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
    /// The invitation endpoint, kept as the last dial candidate.
    pub endpoint: String,
    pub vault_id: String,
    #[serde(default)]
    pub generation: u64,
    pub certificate_fingerprint: String,
    /// The endpoint of the last authenticated connection to the coordinator.
    #[serde(default)]
    pub last_endpoint: Option<String>,
    /// The listener endpoints the coordinator reported in its latest sync state.
    #[serde(default)]
    pub reported_endpoints: Vec<String>,
    /// The vault's discovery key from the coordinator's latest sync state.
    #[serde(default)]
    pub discovery_key: Option<String>,
    /// The vault's space charter and the person key that signed it, as the coordinator presented
    /// them when this device linked.
    #[serde(default)]
    pub charter: Option<PinnedCharter>,
}

/// A space charter a coordinator presented at linking, verified against the person key it
/// carried and the vault.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PinnedCharter {
    /// Base64url charter encoding.
    pub charter: String,
    /// Base64url person public key.
    pub person_key: String,
}

impl PinnedCharter {
    /// Decodes the charter and verifies it against its person key and the vault's personal
    /// space. The charter names the founding anchor, which need not be the coordinator that
    /// presents it: the log the charter starts says which device anchors the space now.
    pub fn verify(&self, vault_id: &str) -> Result<ganbaru_sync_contracts::SignedCharter, String> {
        let (charter, _) = decode_sync_charter(&self.charter, &self.person_key)?;
        if charter.charter.space != ganbaru_sync_contracts::SpaceId::personal(vault_id) {
            return Err("the coordinator presented a charter for another vault".to_string());
        }
        Ok(charter)
    }
}

impl CoordinatorPin {
    /// The endpoint that most likely reaches the coordinator now: the last authenticated one,
    /// else the invitation endpoint.
    pub fn current_endpoint(&self) -> &str {
        self.last_endpoint.as_deref().unwrap_or(&self.endpoint)
    }
}

/// Coordinator addresses learned from one authenticated exchange. `None` leaves a field as
/// stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoordinatorContact {
    /// The endpoint an authenticated connection just reached.
    pub last_endpoint: Option<String>,
    /// The listener endpoints the coordinator reports now.
    pub reported_endpoints: Option<Vec<String>>,
    /// The vault's current discovery key.
    pub discovery_key: Option<String>,
}

impl CoordinatorContact {
    fn validate(&self) -> Result<(), String> {
        if let Some(endpoint) = self.last_endpoint.as_deref() {
            validate_pinned_endpoint("last coordinator endpoint", endpoint)?;
        }
        if let Some(endpoints) = self.reported_endpoints.as_deref() {
            validate_coordinator_endpoints(endpoints)?;
        }
        if let Some(key) = self.discovery_key.as_deref() {
            validate_discovery_key(key)?;
        }
        Ok(())
    }

    /// Applies the update and returns whether the pin changed.
    fn apply_to(self, pin: &mut CoordinatorPin) -> bool {
        let mut changed = false;
        if let Some(endpoint) = self.last_endpoint
            && pin.last_endpoint.as_deref() != Some(endpoint.as_str())
        {
            pin.last_endpoint = Some(endpoint);
            changed = true;
        }
        if let Some(endpoints) = self.reported_endpoints
            && pin.reported_endpoints != endpoints
        {
            pin.reported_endpoints = endpoints;
            changed = true;
        }
        if let Some(key) = self.discovery_key
            && pin.discovery_key.as_deref() != Some(key.as_str())
        {
            pin.discovery_key = Some(key);
            changed = true;
        }
        changed
    }
}

/// Checks a stored or learned coordinator endpoint: a socket address of a dialable class.
fn validate_pinned_endpoint(label: &str, endpoint: &str) -> Result<(), String> {
    let address = endpoint
        .parse::<std::net::SocketAddr>()
        .map_err(|_| format!("{label} is invalid"))?;
    super::dial::validate_dial_endpoint(address)
        .map(|_| ())
        .map_err(|error| format!("{label} is refused: {error}"))
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
    /// Discovery keys of the vaults this coordinator has linked devices for, by vault ID.
    #[serde(default)]
    discovery_keys: BTreeMap<String, String>,
}

impl PairingStateFile {
    /// Drops the discovery keys of vaults that no linked device uses anymore.
    fn prune_discovery_keys(&mut self) {
        let linked_peers = &self.linked_peers;
        self.discovery_keys
            .retain(|vault_id, _| linked_peers.values().any(|peer| &peer.vault_id == vault_id));
    }
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
    /// Public half of the device's sync device key, base64url.
    pub sync_device_key: &'a str,
}

#[derive(Default)]
struct PairingManagerInner {
    state_path: Option<PathBuf>,
    staging_root: Option<PathBuf>,
    state: Option<PairingStateFile>,
    /// Why startup disabled linking; pairing operations report it instead of running.
    disabled_reason: Option<String>,
    /// Where the TLS key moves out of the state file; without one it stays in the file.
    key_store: Option<Arc<dyn IdentityKeyStore>>,
    /// The PKCS#8 key once loaded, so later handshakes do not reach the key store again.
    private_key: Option<Vec<u8>>,
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
        // Staging holds whole-vault archives, so an existing directory with wider permissions
        // is tightened too. A file system that refuses the change still allows handoff.
        ganbaru_atomic_files::create_private_directories(&staging_root)
            .map_err(|error| format!("create handoff staging directory: {error}"))?;
        if let Err(error) = ganbaru_atomic_files::restrict_to_owner(&staging_root) {
            eprintln!("could not make the handoff staging directory private: {error}");
        }

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
                discovery_keys: BTreeMap::new(),
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
        inner.private_key = None;
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
        inner.private_key = None;
        #[cfg(desktop)]
        {
            inner.invitations.clear();
            inner.outgoing_bundles.clear();
        }
        Ok(())
    }

    /// Whether [`Self::initialize`] loaded state or [`Self::disable`] turned linking off. A
    /// loaded manager is the authority for its state file, so it is not loaded again.
    pub fn is_loaded(&self) -> Result<bool, String> {
        let inner = self.lock()?;
        Ok(inner.state.is_some() || inner.disabled_reason.is_some())
    }

    /// Selects where the TLS key is kept. The key moves there on its next load and stays in the
    /// private state file whenever the store refuses it.
    pub fn set_identity_key_store(&self, store: Arc<dyn IdentityKeyStore>) -> Result<(), String> {
        let mut inner = self.lock()?;
        inner.key_store = Some(store);
        inner.private_key = None;
        Ok(())
    }

    /// This device's pairing ID, read without loading the TLS key.
    pub fn device_id(&self) -> Result<String, String> {
        let inner = self.lock()?;
        Ok(initialized_state(&inner)?.identity.device_id.clone())
    }

    /// This device's TLS certificate, read without loading the private key.
    pub fn certificate(&self) -> Result<CertificateDer<'static>, String> {
        let inner = self.lock()?;
        decode_certificate(&initialized_state(&inner)?.identity.certificate)
    }

    /// The device ID and TLS identity. The first call may block on the key store, so async
    /// callers run it on a blocking thread.
    pub fn identity(&self) -> Result<(String, TlsIdentity), String> {
        let (device_id, certificate, stored_key, store) = {
            let inner = self.lock()?;
            let identity = &initialized_state(&inner)?.identity;
            let certificate = decode_certificate(&identity.certificate)?;
            if let Some(key) = inner.private_key.as_ref() {
                return Ok((
                    identity.device_id.clone(),
                    tls_identity(certificate, key.clone()),
                ));
            }
            (
                identity.device_id.clone(),
                certificate,
                identity.private_key.clone(),
                inner.key_store.clone(),
            )
        };
        let key = match (stored_key, store) {
            (Some(encoded), store) => {
                let key = decode_private_key(&encoded)?;
                if let Some(store) = store {
                    self.move_key_to_store(store.as_ref(), &device_id, &encoded, &key);
                }
                key
            }
            (None, Some(store)) => match store.read(&device_id) {
                Ok(Some(key)) => {
                    validate_private_key(&key)?;
                    key
                }
                Ok(None) => return self.replace_lost_identity(&device_id),
                Err(error) => return Err(format!("{DEVICE_KEY_UNAVAILABLE_ERROR}: {error}")),
            },
            (None, None) => {
                return Err(format!(
                    "{DEVICE_KEY_UNAVAILABLE_ERROR}: no key store is configured"
                ));
            }
        };
        let mut inner = self.lock()?;
        if initialized_state(&inner)?.identity.device_id == device_id {
            inner.private_key = Some(key.clone());
        }
        Ok((device_id, tls_identity(certificate, key)))
    }

    /// Moves the TLS key from the state file into `store`. The file keeps the key unless the
    /// store returns the same bytes and the file is rewritten without it.
    fn move_key_to_store(
        &self,
        store: &dyn IdentityKeyStore,
        device_id: &str,
        encoded: &str,
        key: &[u8],
    ) {
        let stored = store
            .write(device_id, key)
            .and_then(|()| store.read(device_id))
            .and_then(|read| match read {
                Some(read) if read == key => Ok(()),
                _ => Err("the key store returned different bytes".to_string()),
            });
        if let Err(error) = stored {
            eprintln!("device linking key stays in the private pairing file: {error}");
            return;
        }
        let persisted = (|| {
            let mut inner = self.lock()?;
            let state = initialized_state(&inner)?;
            if state.identity.device_id != device_id
                || state.identity.private_key.as_deref() != Some(encoded)
            {
                return Ok(());
            }
            let mut next = state.clone();
            next.identity.private_key = None;
            let path = inner
                .state_path
                .clone()
                .ok_or_else(|| "pairing state path is not initialized".to_string())?;
            persist_state(&path, &next)?;
            inner.state = Some(next);
            Ok::<(), String>(())
        })();
        if let Err(error) = persisted {
            eprintln!("device linking key stays in the private pairing file: {error}");
            if let Err(error) = store.remove(device_id) {
                eprintln!("remove unused device linking key: {error}");
            }
        }
    }

    /// Creates a fresh identity after the stored key vanished, but only while no peer or
    /// coordinator pins the old certificate; otherwise the user must unlink first.
    fn replace_lost_identity(&self, device_id: &str) -> Result<(String, TlsIdentity), String> {
        {
            let mut inner = self.lock()?;
            let state = initialized_state(&inner)?;
            if state.identity.device_id != device_id || state.identity.private_key.is_some() {
                drop(inner);
                return self.identity();
            }
            if !state.linked_peers.is_empty() || state.coordinator.is_some() {
                return Err(DEVICE_KEY_MISSING_ERROR.to_string());
            }
            let mut next = state.clone();
            next.identity = create_identity(device_id.to_string())?;
            let path = inner
                .state_path
                .clone()
                .ok_or_else(|| "pairing state path is not initialized".to_string())?;
            persist_state(&path, &next)?;
            inner.state = Some(next);
            #[cfg(desktop)]
            inner.invitations.clear();
        }
        self.identity()
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
        state.prune_discovery_keys();
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
        state.prune_discovery_keys();
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
            secret: random_secret()?.into(),
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

    /// Checks an enrollment against the current invitation and memberships without persisting
    /// anything, and returns the code the enrolling device shows, so the coordinator only asks
    /// for approval of requests that would link.
    #[cfg(desktop)]
    pub fn verify_enrollment(
        &self,
        enrollment: &Enrollment<'_>,
        now_ms: i64,
    ) -> Result<String, String> {
        let inner = self.lock()?;
        let (peer, _) = admit_enrollment(&inner, enrollment, now_ms)?;
        let invitation = &inner
            .invitations
            .get(enrollment.invitation_id)
            .ok_or_else(|| "pairing invitation is unknown or already used".to_string())?
            .invitation;
        Ok(enrollment_code_for_fingerprint(
            invitation,
            &peer.certificate_fingerprint,
        ))
    }

    /// Discards an invitation, for example after the coordinator user rejected a device that
    /// presented it.
    #[cfg(desktop)]
    pub fn discard_invitation(&self, invitation_id: &str) -> Result<(), String> {
        self.lock()?.invitations.remove(invitation_id);
        Ok(())
    }

    /// Links an approved device. Call [`Self::verify_enrollment`] and obtain approval first.
    #[cfg(desktop)]
    pub fn enroll_peer(
        &self,
        enrollment: Enrollment<'_>,
        now_ms: i64,
    ) -> Result<LinkedPeer, String> {
        let mut inner = self.lock()?;
        let (peer, evicted_device_id) = admit_enrollment(&inner, &enrollment, now_ms)?;
        let previous = initialized_state(&inner)?.clone();
        let state = initialized_state_mut(&mut inner)?;
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
        state.prune_discovery_keys();
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
        self.record_enrolled(
            invitation,
            coordinator_certificate,
            coordinator_device_label,
            None,
        )
    }

    /// Pins the coordinator that just linked this device, with the space charter it presented,
    /// which must belong to the invitation's vault.
    pub fn record_enrolled(
        &self,
        invitation: &PairingInvitation,
        coordinator_certificate: &[u8],
        coordinator_device_label: Option<String>,
        charter: Option<PinnedCharter>,
    ) -> Result<(), String> {
        let fingerprint = certificate_fingerprint(coordinator_certificate);
        if fingerprint != invitation.coordinator_fingerprint {
            return Err("coordinator certificate does not match the QR invitation".to_string());
        }
        if let Some(label) = coordinator_device_label.as_deref() {
            validate_device_label("coordinator device label", label)?;
        }
        if let Some(charter) = charter.as_ref() {
            charter.verify(&invitation.vault_id)?;
        }
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        let state = initialized_state_mut(&mut inner)?;
        ensure_enrollment_target(state, invitation)?;
        // Relinking to the same coordinator and vault keeps what its exchanges reported.
        let (reported_endpoints, discovery_key) = match state.coordinator.take() {
            Some(pin)
                if pin.certificate_fingerprint == fingerprint
                    && pin.vault_id == invitation.vault_id =>
            {
                (pin.reported_endpoints, pin.discovery_key)
            }
            _ => (Vec::new(), None),
        };
        state.coordinator = Some(CoordinatorPin {
            device_id: invitation.coordinator_device_id.clone(),
            device_label: coordinator_device_label,
            endpoint: invitation.endpoint.clone(),
            vault_id: invitation.vault_id.clone(),
            generation: invitation.generation,
            certificate_fingerprint: fingerprint,
            last_endpoint: Some(invitation.endpoint.clone()),
            reported_endpoints,
            discovery_key,
            charter,
        });
        state.revoked_by_coordinator = false;
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        Ok(())
    }

    /// Records coordinator addresses learned from an authenticated exchange with the coordinator
    /// that `certificate_fingerprint` identifies for `vault_id`.
    ///
    /// Writes only when a field changed, and only while the pin still names that vault and
    /// fingerprint, so an unlink or relink racing the exchange never resurrects or alters
    /// another pin. Returns whether the pin changed.
    pub fn record_coordinator_contact(
        &self,
        vault_id: &str,
        certificate_fingerprint: &str,
        contact: CoordinatorContact,
    ) -> Result<bool, String> {
        contact.validate()?;
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        let state = initialized_state_mut(&mut inner)?;
        let Some(pin) = state.coordinator.as_mut().filter(|pin| {
            pin.vault_id == vault_id && pin.certificate_fingerprint == certificate_fingerprint
        }) else {
            return Ok(false);
        };
        if !contact.apply_to(pin) {
            return Ok(false);
        }
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        Ok(true)
    }

    /// Returns the discovery key of `vault_id`, creating and persisting it the first time a
    /// vault with linked devices needs one.
    #[cfg(desktop)]
    pub fn discovery_key(&self, vault_id: &str) -> Result<String, String> {
        validate_identifier("vault id", vault_id)?;
        let mut inner = self.lock()?;
        let previous = initialized_state(&inner)?.clone();
        if let Some(key) = previous.discovery_keys.get(vault_id) {
            return Ok(key.clone());
        }
        if !previous
            .linked_peers
            .values()
            .any(|peer| peer.vault_id == vault_id)
        {
            return Err("no linked device uses this vault".to_string());
        }
        let key = random_discovery_key()?;
        initialized_state_mut(&mut inner)?
            .discovery_keys
            .insert(vault_id.to_string(), key.clone());
        if let Err(error) = persist_initialized_state(&inner) {
            inner.state = Some(previous);
            return Err(error);
        }
        Ok(key)
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
                && coordinator.certificate_fingerprint == invitation.coordinator_fingerprint =>
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
    let mut params = CertificateParams::new(vec!["ganbaru-device.local".to_string()])
        .map_err(|error| format!("configure device certificate: {error}"))?;
    params
        .distinguished_name
        .push(DnType::CommonName, device_id.as_str());
    let certificate = params
        .self_signed(&signing_key)
        .map_err(|error| format!("generate device certificate: {error}"))?;
    Ok(StoredIdentity {
        device_id,
        certificate: base64::engine::general_purpose::STANDARD.encode(certificate.der().as_ref()),
        private_key: Some(
            base64::engine::general_purpose::STANDARD.encode(signing_key.serialize_der()),
        ),
    })
}

fn decode_private_key(encoded: &str) -> Result<Vec<u8>, String> {
    let private_key = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("decode device private key: {error}"))?;
    validate_private_key(&private_key)?;
    Ok(private_key)
}

fn validate_private_key(private_key: &[u8]) -> Result<(), String> {
    if private_key.is_empty() || private_key.len() > MAX_CERTIFICATE_BYTES {
        return Err("device private key has an invalid size".to_string());
    }
    Ok(())
}

fn tls_identity(certificate: CertificateDer<'static>, private_key: Vec<u8>) -> TlsIdentity {
    TlsIdentity {
        certificate,
        private_key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(private_key)),
    }
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

/// Code an enrolling device and its coordinator both show before the coordinator user approves
/// the link. It covers the invitation and both certificates, so a device that presents a leaked
/// invitation with its own certificate shows a different code than the device in hand.
pub fn enrollment_code(invitation: &PairingInvitation, device_certificate: &[u8]) -> String {
    enrollment_code_for_fingerprint(invitation, &certificate_fingerprint(device_certificate))
}

fn enrollment_code_for_fingerprint(
    invitation: &PairingInvitation,
    device_fingerprint: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ENROLLMENT_CODE_DOMAIN);
    for part in [
        invitation.invitation_id.as_str(),
        invitation.coordinator_fingerprint.as_str(),
        device_fingerprint,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    ganbaru_contacts::verification_code(&hasher.finalize().into())
}

/// The peer an enrollment would link and the membership it would evict, after checking the
/// invitation credentials and every membership conflict.
#[cfg(desktop)]
fn admit_enrollment(
    inner: &PairingManagerInner,
    enrollment: &Enrollment<'_>,
    now_ms: i64,
) -> Result<(LinkedPeer, Option<String>), String> {
    validate_identifier("device id", enrollment.device_id)?;
    validate_device_label("device label", enrollment.device_label)?;
    let certificate = decode_certificate(enrollment.certificate_b64)?;
    decode_sync_device_key(enrollment.sync_device_key)?;
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
        sync_device_key: enrollment.sync_device_key.to_string(),
        linked_at_ms: now_ms,
    };
    let state = initialized_state(inner)?;
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
    Ok((peer, evicted_device_id))
}

/// A fresh vault discovery key: [`DISCOVERY_KEY_BYTES`] random bytes as unpadded base64url.
#[cfg(desktop)]
fn random_discovery_key() -> Result<String, String> {
    let mut random = [0_u8; DISCOVERY_KEY_BYTES];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut random)
        .map_err(|_| "secure random generator is unavailable".to_string())?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random))
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

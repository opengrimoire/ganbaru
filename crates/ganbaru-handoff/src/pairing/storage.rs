//! Pairing-state validation and crash-recoverable private-file persistence.

use super::role::{LinkRole, MemberCopy};
use super::{
    CoordinatorPin, LinkedPeer, MAX_LINKED_PEERS, MAX_REVOKED_PEERS, PAIRING_STATE_SCHEMA_VERSION,
    PairingManagerInner, PairingState, PendingAcknowledgement, PendingActivation, RevokedPeer,
    StoredIdentity, StoredOutgoingTransfer, UnlinkReason, certificate_fingerprint,
    decode_certificate, decode_private_key, initialized_state, random_token,
    validate_pinned_endpoint,
};
use crate::protocol::{self, validate_device_label, validate_identifier};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

pub(super) fn persist_initialized_state(inner: &PairingManagerInner) -> Result<(), String> {
    let path = inner
        .state_path
        .as_ref()
        .ok_or_else(|| "pairing state path is not initialized".to_string())?;
    persist_state(path, initialized_state(inner)?)
}

pub(super) fn validate_stored_outgoing(transfer: &StoredOutgoingTransfer) -> Result<(), String> {
    transfer.metadata.validate()?;
    if transfer.expires_at_ms <= 0 {
        return Err("outgoing transfer expiry is invalid".to_string());
    }
    Ok(())
}

fn validate_pending_acknowledgement(
    pending: &PendingAcknowledgement,
    expected_device_id: &str,
) -> Result<(), String> {
    validate_activation_record(pending)?;
    if pending.device_id != expected_device_id {
        return Err("pending activation acknowledgement is inconsistent".to_string());
    }
    Ok(())
}

/// Refuses an activation record whose folders are not two distinct absolute paths, so recovery
/// never judges a folder it would have to resolve against the working directory.
fn validate_pending_activation(
    activation: &PendingActivation,
    expected_device_id: &str,
) -> Result<(), String> {
    validate_pending_acknowledgement(&activation.acknowledgement, expected_device_id)?;
    let folder = |path: &Path| path.is_absolute() && path.parent().is_some();
    if !folder(&activation.target)
        || !folder(&activation.staging)
        || activation.target == activation.staging
    {
        return Err("pending activation folders are inconsistent".to_string());
    }
    Ok(())
}

pub(super) fn validate_activation_record(record: &PendingAcknowledgement) -> Result<(), String> {
    validate_identifier("activation vault id", &record.vault_id)?;
    validate_identifier("activation device id", &record.device_id)?;
    validate_identifier("activation transfer id", &record.transfer_id)?;
    Ok(())
}

/// Replaces stored device labels that fail the display text rules, so one bad label cannot block
/// startup or drop a working link. A linked peer keeps its label with the offending characters
/// removed, or its device ID when nothing displayable remains; an invalid coordinator label is
/// forgotten. Labels are local display data, never signed, so rewriting them is safe.
///
/// Returns whether anything changed.
pub(super) fn quarantine_invalid_labels(state: &mut PairingState) -> bool {
    let mut changed = false;
    for peer in state.linked_peers.values_mut() {
        if validate_device_label("linked device label", &peer.device_label).is_ok() {
            continue;
        }
        let cleaned = displayable_label(&peer.device_label);
        peer.device_label = if validate_device_label("linked device label", &cleaned).is_ok() {
            cleaned
        } else {
            displayable_label(&peer.device_id)
        };
        changed = true;
    }
    if let Some(coordinator) = state.role.coordinator_mut()
        && coordinator
            .device_label
            .as_deref()
            .is_some_and(|label| validate_device_label("coordinator device label", label).is_err())
    {
        coordinator.device_label = None;
        changed = true;
    }
    changed
}

/// Removes disallowed characters and keeps whole characters within the label byte bound.
fn displayable_label(label: &str) -> String {
    let mut cleaned = String::new();
    for character in label
        .trim()
        .chars()
        .filter(|character| !ganbaru_contacts::is_disallowed_display_character(*character))
    {
        if cleaned.len() + character.len_utf8() > protocol::MAX_DEVICE_LABEL_BYTES {
            break;
        }
        cleaned.push(character);
    }
    cleaned
}

pub(super) fn validate_state(state: &PairingState, expected_device_id: &str) -> Result<(), String> {
    if state.schema_version != PAIRING_STATE_SCHEMA_VERSION {
        return Err("pairing state schema version is unsupported".to_string());
    }
    if state.identity.device_id != expected_device_id {
        return Err("pairing identity belongs to a different device".to_string());
    }
    validate_identifier("device id", &state.identity.device_id)?;
    decode_certificate(&state.identity.certificate)?;
    if let Some(private_key) = state.identity.private_key.as_deref() {
        decode_private_key(private_key)?;
    }
    if state.linked_peers.len() > MAX_LINKED_PEERS {
        return Err("too many linked devices are stored".to_string());
    }
    if state.revoked_peers.len() > MAX_REVOKED_PEERS {
        return Err("too many revoked devices are stored".to_string());
    }
    if matches!(state.role, LinkRole::Coordinator { .. }) == state.linked_peers.is_empty() {
        return Err("linked devices are stored only by a coordinator".to_string());
    }
    let mut certificate_fingerprints = std::collections::BTreeSet::new();
    for (device_id, peer) in &state.linked_peers {
        if device_id != &peer.device_id {
            return Err("linked device index is inconsistent".to_string());
        }
        if peer.device_id == state.identity.device_id {
            return Err("linked device identity duplicates the local device".to_string());
        }
        validate_identifier("peer device id", &peer.device_id)?;
        validate_identifier("peer vault id", &peer.vault_id)?;
        validate_device_label("linked device label", &peer.device_label)?;
        let certificate = decode_certificate(&peer.certificate)?;
        if certificate_fingerprint(certificate.as_ref()) != peer.certificate_fingerprint {
            return Err("linked peer certificate fingerprint is inconsistent".to_string());
        }
        if !certificate_fingerprints.insert(&peer.certificate_fingerprint) {
            return Err("linked devices reuse a certificate identity".to_string());
        }
    }
    for (device_id, peer) in &state.revoked_peers {
        if device_id != &peer.device_id {
            return Err("revoked device index is inconsistent".to_string());
        }
        if state.linked_peers.contains_key(device_id) {
            return Err("a device cannot be linked and revoked".to_string());
        }
        validate_identifier("revoked device id", &peer.device_id)?;
        validate_identifier("revoked vault id", &peer.vault_id)?;
        let certificate = decode_certificate(&peer.certificate)?;
        if certificate_fingerprint(certificate.as_ref()) != peer.certificate_fingerprint {
            return Err("revoked peer certificate fingerprint is inconsistent".to_string());
        }
        if !certificate_fingerprints.insert(&peer.certificate_fingerprint) {
            return Err("linked and revoked devices reuse a certificate identity".to_string());
        }
    }
    if let Some(coordinator) = state.role.coordinator() {
        validate_identifier("coordinator device id", &coordinator.device_id)?;
        validate_identifier("coordinator vault id", &coordinator.vault_id)?;
        coordinator
            .endpoint
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "coordinator endpoint is invalid".to_string())?;
        protocol::validate_certificate_fingerprint(
            "coordinator certificate fingerprint",
            &coordinator.certificate_fingerprint,
        )?;
        if let Some(label) = coordinator.device_label.as_deref() {
            validate_device_label("coordinator device label", label)?;
        }
        if let Some(endpoint) = coordinator.last_endpoint.as_deref() {
            validate_pinned_endpoint("last coordinator endpoint", endpoint)?;
        }
        protocol::validate_coordinator_endpoints(&coordinator.reported_endpoints)?;
    }
    if let Some(pending) = state.role.pending_acknowledgement() {
        validate_pending_acknowledgement(pending, expected_device_id)?;
    }
    if let Some((activation, _)) = state.role.activation() {
        validate_pending_activation(activation, expected_device_id)?;
    }
    if let Some(transfer) = state.role.outgoing() {
        validate_stored_outgoing(transfer)?;
    }
    if let Some(completed) = state.role.completed() {
        validate_activation_record(completed)?;
    }
    Ok(())
}

/// The pairing state as `vault-handoff.json` stores it. The flat fields are decoded into one
/// [`LinkRole`] and any combination no role can hold is refused.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoredPairingState {
    schema_version: u32,
    identity: StoredIdentity,
    #[serde(default)]
    linked_peers: BTreeMap<String, LinkedPeer>,
    #[serde(default)]
    revoked_peers: BTreeMap<String, RevokedPeer>,
    coordinator: Option<CoordinatorPin>,
    #[serde(default)]
    revoked_by_coordinator: bool,
    /// Set when this device unlinked itself; cleared by the next link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unlink_reason: Option<UnlinkReason>,
    #[serde(default)]
    replica_ready: bool,
    #[serde(default)]
    pending_acknowledgement: Option<PendingAcknowledgement>,
    #[serde(default)]
    outgoing_transfer: Option<StoredOutgoingTransfer>,
    #[serde(default)]
    completed_activation: Option<PendingAcknowledgement>,
    /// The copy a member is activating, with the readiness it returns to when abandoned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    activation: Option<StoredActivation>,
}

/// A member activation as `vault-handoff.json` stores it.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredActivation {
    activation: PendingActivation,
    was_ready: bool,
}

impl TryFrom<StoredPairingState> for PairingState {
    type Error = String;

    fn try_from(stored: StoredPairingState) -> Result<Self, String> {
        let role = decode_role(
            !stored.linked_peers.is_empty(),
            stored.coordinator,
            stored.revoked_by_coordinator,
            stored.unlink_reason,
            stored.replica_ready,
            stored.pending_acknowledgement,
            stored.outgoing_transfer,
            stored.completed_activation,
            stored.activation,
        )?;
        Ok(Self {
            schema_version: stored.schema_version,
            identity: stored.identity,
            linked_peers: stored.linked_peers,
            revoked_peers: stored.revoked_peers,
            role,
        })
    }
}

impl From<&PairingState> for StoredPairingState {
    fn from(state: &PairingState) -> Self {
        let role = &state.role;
        Self {
            schema_version: state.schema_version,
            identity: state.identity.clone(),
            linked_peers: state.linked_peers.clone(),
            revoked_peers: state.revoked_peers.clone(),
            coordinator: role.coordinator().cloned(),
            revoked_by_coordinator: matches!(role, LinkRole::Revoked),
            unlink_reason: match role {
                LinkRole::Unlinked { reason } => *reason,
                _ => None,
            },
            replica_ready: role.copy_ready(),
            pending_acknowledgement: role.pending_acknowledgement().cloned(),
            outgoing_transfer: role.outgoing().cloned(),
            completed_activation: role.completed().cloned(),
            activation: role
                .activation()
                .map(|(activation, was_ready)| StoredActivation {
                    activation: activation.clone(),
                    was_ready,
                }),
        }
    }
}

fn inconsistent(detail: &str) -> String {
    format!("pairing state is inconsistent: {detail}")
}

/// Decodes the stored role fields, refusing every combination that no [`LinkRole`] holds.
#[allow(clippy::too_many_arguments)]
fn decode_role(
    has_peers: bool,
    coordinator: Option<CoordinatorPin>,
    revoked: bool,
    reason: Option<UnlinkReason>,
    copy_ready: bool,
    pending: Option<PendingAcknowledgement>,
    outgoing: Option<StoredOutgoingTransfer>,
    completed: Option<PendingAcknowledgement>,
    activation: Option<StoredActivation>,
) -> Result<LinkRole, String> {
    let serves_copies = outgoing.is_some() || completed.is_some();
    let Some(coordinator) = coordinator else {
        if copy_ready || pending.is_some() || activation.is_some() {
            return Err(inconsistent(
                "a vault copy is recorded without a coordinator",
            ));
        }
        return match (has_peers, revoked, reason) {
            (true, true, _) => Err(inconsistent("a revoked device stores linked devices")),
            (true, false, Some(_)) | (false, true, Some(_)) => Err(inconsistent(
                "an unlink reason is recorded for a linked device",
            )),
            (true, false, None) => Ok(LinkRole::Coordinator {
                outgoing,
                completed,
            }),
            (false, _, _) if serves_copies => {
                Err(inconsistent("a copy is served without linked devices"))
            }
            (false, true, None) => Ok(LinkRole::Revoked),
            (false, false, reason) => Ok(LinkRole::Unlinked { reason }),
        };
    };
    if has_peers {
        return Err(inconsistent("a member stores linked devices of its own"));
    }
    if revoked {
        return Err(inconsistent(
            "a pinned coordinator is also recorded as revoked",
        ));
    }
    if reason.is_some() {
        return Err(inconsistent(
            "an unlink reason is recorded for a linked device",
        ));
    }
    if serves_copies {
        return Err(inconsistent("a member serves a coordinator copy"));
    }
    let copy = match (copy_ready, pending, activation) {
        (true, _, Some(_)) => {
            return Err(inconsistent(
                "a vault copy is recorded as ready while it is being activated",
            ));
        }
        (_, Some(_), Some(_)) => {
            return Err(inconsistent(
                "an activation awaits acknowledgement while another copy is being activated",
            ));
        }
        (
            false,
            None,
            Some(StoredActivation {
                activation,
                was_ready,
            }),
        ) => MemberCopy::Activating {
            activation,
            was_ready,
        },
        (false, Some(_), None) => {
            return Err(inconsistent(
                "an activation awaits acknowledgement without a vault copy",
            ));
        }
        (false, None, None) => MemberCopy::Required,
        (true, pending_acknowledgement, None) => MemberCopy::Ready {
            pending_acknowledgement,
        },
    };
    Ok(LinkRole::Member { coordinator, copy })
}

pub(super) fn read_state(path: &Path) -> Result<PairingState, String> {
    let bytes = fs::read(path).map_err(|error| format!("read pairing state: {error}"))?;
    let stored: StoredPairingState =
        serde_json::from_slice(&bytes).map_err(|error| format!("decode pairing state: {error}"))?;
    PairingState::try_from(stored)
}

pub(super) fn persist_state(path: &Path, state: &PairingState) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(&StoredPairingState::from(state))
        .map_err(|error| format!("encode pairing state: {error}"))?;
    write_private_file_atomically(path, &bytes)
}

pub fn write_private_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "private state path has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("create private state directory: {error}"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "private state filename is invalid".to_string())?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", random_token("write")?));
    let rollback = path.with_extension("rollback");
    if rollback.exists() {
        fs::remove_file(&rollback)
            .map_err(|error| format!("remove stale private state rollback: {error}"))?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| format!("open temporary private state: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("write private state: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync private state: {error}"))?;
    let had_current = path.exists();
    if had_current {
        fs::rename(path, &rollback)
            .map_err(|error| format!("prepare private state replacement: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_current {
            let _ = fs::rename(&rollback, path);
        }
        return Err(format!("replace private state: {error}"));
    }
    if had_current {
        let _ = fs::remove_file(&rollback);
    }
    sync_parent_directory(path)?;
    Ok(())
}

pub(super) fn recover_private_state(path: &Path) -> Result<(), String> {
    let rollback = path.with_extension("rollback");
    if path.exists() {
        if rollback.exists() {
            fs::remove_file(&rollback)
                .map_err(|error| format!("remove private state rollback: {error}"))?;
        }
        return Ok(());
    }
    if rollback.exists() {
        fs::rename(&rollback, path)
            .map_err(|error| format!("recover private pairing state: {error}"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "private state path has no parent".to_string())?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("sync private state directory: {error}"))
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}

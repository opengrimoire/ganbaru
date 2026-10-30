//! Pairing-state validation and crash-recoverable private-file persistence.

use super::{
    MAX_LINKED_PEERS, MAX_REVOKED_PEERS, PAIRING_STATE_SCHEMA_VERSION, PairingManagerInner,
    PairingStateFile, PendingAcknowledgement, StoredOutgoingTransfer,
    certificate_fingerprint, decode_certificate, decode_private_key, initialized_state,
    random_token, validate_pinned_endpoint,
};
use crate::protocol::{self, validate_device_label, validate_identifier};
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
pub(super) fn quarantine_invalid_labels(state: &mut PairingStateFile) -> bool {
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
    if let Some(coordinator) = state.coordinator.as_mut()
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

pub(super) fn validate_state(
    state: &PairingStateFile,
    expected_device_id: &str,
) -> Result<(), String> {
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
    if state.coordinator.is_some() && !state.linked_peers.is_empty() {
        return Err("a coordinator client cannot store its own linked devices".to_string());
    }
    if state.coordinator.is_some() && state.revoked_by_coordinator {
        return Err("a linked coordinator cannot also be recorded as revoked".to_string());
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
    if let Some(coordinator) = state.coordinator.as_ref() {
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
    if let Some(pending) = state.pending_acknowledgement.as_ref() {
        validate_pending_acknowledgement(pending, expected_device_id)?;
        if !state.replica_ready {
            return Err("pending activation acknowledgement is inconsistent".to_string());
        }
    }
    if let Some(transfer) = state.outgoing_transfer.as_ref() {
        validate_stored_outgoing(transfer)?;
    }
    if let Some(completed) = state.completed_activation.as_ref() {
        validate_activation_record(completed)?;
    }
    Ok(())
}

pub(super) fn read_state(path: &Path) -> Result<PairingStateFile, String> {
    let bytes = fs::read(path).map_err(|error| format!("read pairing state: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("decode pairing state: {error}"))
}

pub(super) fn persist_state(path: &Path, state: &PairingStateFile) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state)
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

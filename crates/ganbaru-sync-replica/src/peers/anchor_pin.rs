//! The device-local anchor pin: which device anchors the vault's authorization log and the
//! certificate its enrollment recorded.
//!
//! The pin is separate from the handoff coordinator pin, whose generation counts whole-vault
//! ownership transfers and never moves on a handover. A member follows a handover by rewriting
//! this pin from the selected log, and dials the anchor at the endpoints the peer book holds for
//! it, so no device links again. The selected log always wins: the pin only stands in while the
//! log cannot be read, and a pin the log contradicts is replaced.

use std::path::Path;

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_private_directories, read_regular_file_capped,
    write_file_atomically,
};
use ganbaru_handoff::protocol::validate_identifier;
use ganbaru_sync::AuthorizationView;
use serde::{Deserialize, Serialize};

use super::admission::fingerprint_hex;

const ANCHOR_PIN_SCHEMA_VERSION: u32 = 1;

/// Largest anchor pin file this installation writes or reads.
pub const MAX_ANCHOR_PIN_BYTES: usize = 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnchorPinFile {
    schema_version: u32,
    vault_id: String,
    device_id: String,
    certificate_fingerprint: String,
    epoch: u32,
}

/// The anchor of one vault as the selected log last named it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorPin {
    /// Vault whose log names the anchor.
    pub vault_id: String,
    /// Handoff device id of the anchor.
    pub device_id: String,
    /// Lowercase hex SHA-256 fingerprint of the TLS certificate the anchor's enrollment
    /// recorded, which every dial to it pins.
    pub certificate_fingerprint: String,
    /// Epoch of the selected head the pin was taken from.
    pub epoch: u32,
}

impl AnchorPin {
    /// The anchor `view` names for `vault_id`, or `None` when the log does not enroll it, which
    /// leaves nothing to pin a dial to.
    pub fn from_view(vault_id: &str, view: &AuthorizationView) -> Option<Self> {
        let device = view
            .devices
            .iter()
            .find(|device| device.device_id == view.anchor_device_id)?;
        Some(Self {
            vault_id: vault_id.to_string(),
            device_id: device.device_id.clone(),
            certificate_fingerprint: fingerprint_hex(&device.tls_fingerprint),
            epoch: view.epoch,
        })
    }

    /// Reads the pin of `vault_id` at `path`, `None` when the file does not exist. Refuses a
    /// file that is too large, does not parse, has another schema version, names another vault,
    /// or holds a malformed device id or fingerprint.
    pub fn load(path: &Path, vault_id: &str) -> Result<Option<Self>, String> {
        let bytes = match read_regular_file_capped(
            path,
            u64::try_from(MAX_ANCHOR_PIN_BYTES).unwrap_or(u64::MAX),
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read sync anchor pin: {error}")),
        };
        let file: AnchorPinFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse sync anchor pin: {error}"))?;
        if file.schema_version != ANCHOR_PIN_SCHEMA_VERSION {
            return Err("sync anchor pin has an unsupported schema version".to_string());
        }
        if file.vault_id != vault_id {
            return Err("sync anchor pin names another vault".to_string());
        }
        validate_identifier("anchor device id", &file.device_id)
            .map_err(|error| format!("sync anchor pin is malformed: {error}"))?;
        if !is_fingerprint_hex(&file.certificate_fingerprint) {
            return Err("sync anchor pin holds a malformed certificate fingerprint".to_string());
        }
        Ok(Some(Self {
            vault_id: file.vault_id,
            device_id: file.device_id,
            certificate_fingerprint: file.certificate_fingerprint,
            epoch: file.epoch,
        }))
    }

    /// Writes the pin to `path` atomically, owner-only.
    pub fn store(&self, path: &Path) -> Result<(), String> {
        let file = AnchorPinFile {
            schema_version: ANCHOR_PIN_SCHEMA_VERSION,
            vault_id: self.vault_id.clone(),
            device_id: self.device_id.clone(),
            certificate_fingerprint: self.certificate_fingerprint.clone(),
            epoch: self.epoch,
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| format!("encode sync anchor pin: {error}"))?;
        if let Some(parent) = path.parent() {
            create_private_directories(parent)
                .map_err(|error| format!("create sync anchor pin directory: {error}"))?;
        }
        write_file_atomically(path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write sync anchor pin: {error}"))
    }

    /// Follows the selected log: returns the anchor `view` names and rewrites the pin at `path`
    /// when it differs. Without a view, returns the stored pin; a damaged pin is removed then,
    /// since the next pass that reads the log writes it again.
    pub fn follow(
        path: &Path,
        vault_id: &str,
        view: Option<&AuthorizationView>,
    ) -> Result<Option<Self>, String> {
        let stored = match Self::load(path, vault_id) {
            Ok(stored) => stored,
            Err(error) => {
                remove_damaged(path, &error)?;
                None
            }
        };
        let Some(view) = view else {
            return Ok(stored);
        };
        let current = Self::from_view(vault_id, view);
        match (&current, &stored) {
            (Some(current), stored) if stored.as_ref() != Some(current) => current.store(path)?,
            (None, Some(_)) => remove_damaged(path, "the selected log enrolls no anchor")?,
            _ => {}
        }
        Ok(current)
    }
}

/// Removes a pin that cannot be used, reporting why.
fn remove_damaged(path: &Path, reason: &str) -> Result<(), String> {
    eprintln!("sync: dropping the anchor pin: {reason}");
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove the sync anchor pin: {error}")),
    }
}

/// Whether `value` is a lowercase hex SHA-256 fingerprint.
fn is_fingerprint_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

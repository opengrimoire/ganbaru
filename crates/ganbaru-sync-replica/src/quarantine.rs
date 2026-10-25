//! Device-local quarantine of other devices.
//!
//! Any installation can quarantine another device of the vault at once, without the anchor. A
//! quarantined device is refused at TLS, never dialed, and the operations of every writer its
//! device key certified are refused at store, so a forwarding peer skips them and keeps the rest
//! of its page. Quarantine excludes nothing: lifting it lets the operations arrive again.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_private_directories, read_regular_file_capped,
    write_file_atomically,
};
use serde::{Deserialize, Serialize};

const QUARANTINE_SCHEMA_VERSION: u32 = 1;

/// Most devices one installation can quarantine, the most a space enrolls besides itself.
pub const MAX_QUARANTINED_DEVICES: usize = 256;

/// Largest quarantine file this installation writes or reads.
pub const MAX_QUARANTINE_BYTES: usize = 64 * 1024;

/// Longest device id a quarantine file holds.
const MAX_DEVICE_ID_BYTES: usize = 128;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QuarantineFile {
    schema_version: u32,
    devices: Vec<String>,
}

/// The quarantine file of one vault with the device ids it holds.
#[derive(Debug, Clone)]
pub struct Quarantine {
    path: PathBuf,
    devices: BTreeSet<String>,
}

impl Quarantine {
    /// Reads the quarantine at `path`, empty when the file does not exist. Refuses a file that
    /// is too large, does not parse, or holds too many or malformed device ids.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match read_regular_file_capped(
            path,
            u64::try_from(MAX_QUARANTINE_BYTES).unwrap_or(u64::MAX),
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_path_buf(),
                    devices: BTreeSet::new(),
                });
            }
            Err(error) => return Err(format!("read sync quarantine: {error}")),
        };
        let file: QuarantineFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse sync quarantine: {error}"))?;
        if file.schema_version != QUARANTINE_SCHEMA_VERSION {
            return Err("sync quarantine has an unsupported schema version".to_string());
        }
        if file.devices.len() > MAX_QUARANTINED_DEVICES {
            return Err("sync quarantine holds too many devices".to_string());
        }
        for device_id in &file.devices {
            validate_device_id(device_id)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            devices: file.devices.into_iter().collect(),
        })
    }

    /// The quarantined device ids.
    pub fn devices(&self) -> &BTreeSet<String> {
        &self.devices
    }

    /// Whether `device_id` is quarantined.
    pub fn contains(&self, device_id: &str) -> bool {
        self.devices.contains(device_id)
    }

    /// Quarantines a device and writes the file. Returns whether it was not quarantined yet.
    pub fn add(&mut self, device_id: &str) -> Result<bool, String> {
        validate_device_id(device_id)?;
        if self.devices.contains(device_id) {
            return Ok(false);
        }
        if self.devices.len() >= MAX_QUARANTINED_DEVICES {
            return Err("sync quarantine is full".to_string());
        }
        let mut next = self.devices.clone();
        next.insert(device_id.to_string());
        self.replace(next)?;
        Ok(true)
    }

    /// Lifts the quarantine of a device and writes the file. Returns whether it was quarantined.
    pub fn lift(&mut self, device_id: &str) -> Result<bool, String> {
        if !self.devices.contains(device_id) {
            return Ok(false);
        }
        let mut next = self.devices.clone();
        next.remove(device_id);
        self.replace(next)?;
        Ok(true)
    }

    fn replace(&mut self, next: BTreeSet<String>) -> Result<(), String> {
        let file = QuarantineFile {
            schema_version: QUARANTINE_SCHEMA_VERSION,
            devices: next.iter().cloned().collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| format!("encode sync quarantine: {error}"))?;
        if let Some(parent) = self.path.parent() {
            create_private_directories(parent)
                .map_err(|error| format!("create sync quarantine directory: {error}"))?;
        }
        write_file_atomically(&self.path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write sync quarantine: {error}"))?;
        self.devices = next;
        Ok(())
    }
}

fn validate_device_id(device_id: &str) -> Result<(), String> {
    if device_id.is_empty()
        || device_id.len() > MAX_DEVICE_ID_BYTES
        || device_id.chars().any(char::is_control)
    {
        return Err("sync quarantine names a malformed device id".to_string());
    }
    Ok(())
}

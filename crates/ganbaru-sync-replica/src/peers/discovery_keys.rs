//! The device-local view of the vault's discovery keys: the current key the selected log names,
//! which this device advertises under, and the key it replaced, which this device still
//! browses for a while.
//!
//! The log holds the keys: the charter names the first one and every removal rotates it. The
//! log carries no sequencing time, so this file records when this device first saw the current
//! key, which bounds how long the previous key is browsed. Devices that have not applied the
//! removal yet still advertise under the previous key; browsing it for [`PREVIOUS_KEY_BROWSE_MS`]
//! keeps them reachable over DNS-SD while they catch up, and nothing is ever advertised under
//! it.

use std::path::Path;

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_private_directories, read_regular_file_capped,
    write_file_atomically,
};
use ganbaru_handoff::discovery::DiscoveryKey;
use ganbaru_handoff::protocol::DISCOVERY_KEY_BYTES;
use ganbaru_sync::AuthorizationView;
use serde::{Deserialize, Serialize};

use crate::wire::bytes_to_wire;

const DISCOVERY_KEYS_SCHEMA_VERSION: u32 = 1;

/// Largest discovery keys file this installation writes or reads.
pub const MAX_DISCOVERY_KEYS_BYTES: usize = 1024;

/// How long after first seeing a rotated key this device still browses the previous one.
pub const PREVIOUS_KEY_BROWSE_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DiscoveryKeysFile {
    schema_version: u32,
    vault_id: String,
    current: String,
    previous: Option<String>,
    seen_at_ms: u64,
}

/// The discovery keys of one vault as the selected log last named them.
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveryKeys {
    /// Vault whose log names the keys.
    pub vault_id: String,
    /// The key the selected log names now, the only one this device advertises under.
    pub current: [u8; DISCOVERY_KEY_BYTES],
    /// The key the latest rotation replaced, if the log rotated one.
    pub previous: Option<[u8; DISCOVERY_KEY_BYTES]>,
    /// When this device first saw `current`, in Unix milliseconds.
    pub seen_at_ms: u64,
}

impl std::fmt::Debug for DiscoveryKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DiscoveryKeys")
            .field("vault_id", &self.vault_id)
            .field("previous", &self.previous.is_some())
            .field("seen_at_ms", &self.seen_at_ms)
            .finish_non_exhaustive()
    }
}

impl DiscoveryKeys {
    /// The key to advertise under.
    pub fn advertised(&self) -> DiscoveryKey {
        DiscoveryKey::from_bytes(self.current)
    }

    /// The keys to browse at `now_ms`: the current one, then the previous one until
    /// [`PREVIOUS_KEY_BROWSE_MS`] after this device first saw the current one.
    pub fn browsed(&self, now_ms: u64) -> Vec<DiscoveryKey> {
        let mut keys = vec![DiscoveryKey::from_bytes(self.current)];
        if let Some(previous) = self.previous
            && now_ms.saturating_sub(self.seen_at_ms) < PREVIOUS_KEY_BROWSE_MS
        {
            keys.push(DiscoveryKey::from_bytes(previous));
        }
        keys
    }

    /// Reads the keys of `vault_id` at `path`, `None` when the file does not exist. Refuses a
    /// file that is too large, does not parse, has another schema version, names another vault,
    /// or holds a malformed key.
    pub fn load(path: &Path, vault_id: &str) -> Result<Option<Self>, String> {
        let bytes = match read_regular_file_capped(
            path,
            u64::try_from(MAX_DISCOVERY_KEYS_BYTES).unwrap_or(u64::MAX),
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read sync discovery keys: {error}")),
        };
        let file: DiscoveryKeysFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse sync discovery keys: {error}"))?;
        if file.schema_version != DISCOVERY_KEYS_SCHEMA_VERSION {
            return Err("sync discovery keys have an unsupported schema version".to_string());
        }
        if file.vault_id != vault_id {
            return Err("sync discovery keys name another vault".to_string());
        }
        let current = decode_key(&file.current)?;
        let previous = file.previous.as_deref().map(decode_key).transpose()?;
        Ok(Some(Self {
            vault_id: file.vault_id,
            current,
            previous,
            seen_at_ms: file.seen_at_ms,
        }))
    }

    /// Writes the keys to `path` atomically, owner-only.
    pub fn store(&self, path: &Path) -> Result<(), String> {
        let file = DiscoveryKeysFile {
            schema_version: DISCOVERY_KEYS_SCHEMA_VERSION,
            vault_id: self.vault_id.clone(),
            current: bytes_to_wire(&self.current),
            previous: self.previous.as_ref().map(|key| bytes_to_wire(key)),
            seen_at_ms: self.seen_at_ms,
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| format!("encode sync discovery keys: {error}"))?;
        if let Some(parent) = path.parent() {
            create_private_directories(parent)
                .map_err(|error| format!("create sync discovery keys directory: {error}"))?;
        }
        write_file_atomically(path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write sync discovery keys: {error}"))
    }

    /// Removes the file at `path`; a missing file is already removed.
    pub fn remove(path: &Path) -> Result<(), String> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("remove the sync discovery keys: {error}")),
        }
    }

    /// Follows the selected log at `now_ms`: returns the keys `view` names and rewrites the
    /// file at `path` when the current key changed, recording `now_ms` as when this device
    /// first saw it. Without a view, returns the stored keys; a damaged file is removed then,
    /// since the next pass that reads the log writes it again.
    pub fn follow(
        path: &Path,
        vault_id: &str,
        view: Option<&AuthorizationView>,
        now_ms: u64,
    ) -> Result<Option<Self>, String> {
        let stored = match Self::load(path, vault_id) {
            Ok(stored) => stored,
            Err(error) => {
                eprintln!("sync: dropping the discovery keys: {error}");
                Self::remove(path)?;
                None
            }
        };
        let Some(view) = view else {
            return Ok(stored);
        };
        let current = Self {
            vault_id: vault_id.to_string(),
            current: view.discovery_key,
            previous: view.previous_discovery_key,
            seen_at_ms: now_ms,
        };
        match stored {
            Some(stored)
                if stored.current == current.current && stored.previous == current.previous =>
            {
                Ok(Some(stored))
            }
            _ => {
                current.store(path)?;
                Ok(Some(current))
            }
        }
    }
}

fn decode_key(encoded: &str) -> Result<[u8; DISCOVERY_KEY_BYTES], String> {
    DiscoveryKey::decode(encoded)
        .map(|key| *key.as_bytes())
        .map_err(|error| format!("sync discovery keys are malformed: {error}"))
}

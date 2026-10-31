//! What an anchor keeps in its data directory: the layout, the activation record that commits a
//! vault, the one-time setup secret, the published certificate fingerprint, and the key files.

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, read_regular_file_capped, write_file_atomically,
};
use ganbaru_handoff::protocol::relay::RelaySetupToken;
use ganbaru_sync_replica::writer::{SyncKey, SyncKeyStore, key_reference};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Largest small state file an anchor reads.
const MAX_STATE_FILE_BYTES: u64 = 16 * 1024;
const FINGERPRINT_BYTES: usize = 32;

/// Files of an anchor data directory.
#[derive(Clone, Debug)]
pub struct DataPaths {
    root: PathBuf,
}

impl DataPaths {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Device-local state: identity, pairing, and sync files.
    pub fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    /// The installation's signing and encryption keys.
    pub fn keys(&self) -> PathBuf {
        self.config().join("keys")
    }

    pub fn device_id(&self) -> PathBuf {
        self.config().join("device-id")
    }

    /// The replica's vault folder, in the layout a desktop vault has.
    pub fn vault(&self) -> PathBuf {
        self.root.join("vault")
    }

    pub fn database(&self) -> PathBuf {
        self.vault()
            .join(ganbaru_sync_replica::backup::DATABASE_FILE)
    }

    /// The record that commits an activation.
    pub fn activation(&self) -> PathBuf {
        self.root.join("anchor.json")
    }

    pub fn setup_secret(&self) -> PathBuf {
        self.root.join("setup-secret")
    }

    /// The certificate fingerprint, readable while a server holds the data directory lock.
    pub fn fingerprint(&self) -> PathBuf {
        self.root.join("certificate.sha256")
    }

    /// Uploads in progress, cleared at startup.
    pub fn incoming(&self) -> PathBuf {
        self.root.join("incoming")
    }

    pub fn upload_archive(&self) -> PathBuf {
        self.incoming().join("upload.zip")
    }

    pub fn upload_staging(&self) -> PathBuf {
        self.incoming().join("vault")
    }

    /// Private staging of a backup file.
    pub fn backup_staging(&self) -> PathBuf {
        self.root.join("backup-staging")
    }
}

fn read_small(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match read_regular_file_capped(path, MAX_STATE_FILE_BYTES, FinalSymlink::Refuse) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_file_atomically(path, bytes, FileAccess::OwnerOnly)
        .map_err(|error| format!("write {}: {error}", path.display()))
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove {}: {error}", path.display())),
    }
}

/// The vault an anchor runs. The file appears only after the vault passed every activation check,
/// so a crash during activation leaves an anchor that runs nothing.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Activation {
    pub vault_id: String,
    /// The anchor takes the anchor role once it can, until it holds it for the first time.
    pub take_anchor: bool,
}

impl Activation {
    pub fn load(paths: &DataPaths) -> Result<Option<Self>, String> {
        read_small(&paths.activation())?
            .map(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|error| format!("read the activation record: {error}"))
            })
            .transpose()
    }

    pub fn save(&self, paths: &DataPaths) -> Result<(), String> {
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("encode the activation record: {error}"))?;
        write_private(&paths.activation(), &bytes)
    }
}

/// The stored form of a setup secret.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredSecret {
    token: String,
    expires_at_ms: i64,
}

/// Replaces the setup secret with `token`, so only the newest printed code works.
pub fn store_setup_secret(
    paths: &DataPaths,
    token: &RelaySetupToken,
    expires_at_ms: i64,
) -> Result<(), String> {
    let stored = StoredSecret {
        token: token.to_text(),
        expires_at_ms,
    };
    let bytes =
        serde_json::to_vec(&stored).map_err(|_| "encode the setup secret failed".to_string())?;
    write_private(&paths.setup_secret(), &bytes)
}

/// The setup secret when one is stored and it has not expired.
pub fn live_setup_secret(
    paths: &DataPaths,
    now_ms: i64,
) -> Result<Option<RelaySetupToken>, String> {
    let Some(bytes) = read_small(&paths.setup_secret())? else {
        return Ok(None);
    };
    let stored: StoredSecret =
        serde_json::from_slice(&bytes).map_err(|_| "the stored setup secret is damaged")?;
    if stored.expires_at_ms <= now_ms {
        return Ok(None);
    }
    RelaySetupToken::from_text(&stored.token)
        .map(Some)
        .map_err(|_| "the stored setup secret is damaged".to_string())
}

/// Removes the setup secret, which makes its code useless.
pub fn consume_setup_secret(paths: &DataPaths) -> Result<(), String> {
    remove_if_present(&paths.setup_secret())
}

/// Publishes the certificate fingerprint for `anchor setup`.
pub fn store_fingerprint(paths: &DataPaths, fingerprint: &str) -> Result<(), String> {
    write_private(&paths.fingerprint(), fingerprint.as_bytes())
}

/// The published certificate fingerprint, `None` before the anchor created its identity.
pub fn load_fingerprint(paths: &DataPaths) -> Result<Option<[u8; FINGERPRINT_BYTES]>, String> {
    read_small(&paths.fingerprint())?
        .map(|bytes| parse_fingerprint(&bytes))
        .transpose()
}

/// Decodes a fingerprint in lowercase hex.
pub fn parse_fingerprint(text: &[u8]) -> Result<[u8; FINGERPRINT_BYTES], String> {
    let damaged = || "the stored certificate fingerprint is damaged".to_string();
    if text.len() != FINGERPRINT_BYTES * 2 {
        return Err(damaged());
    }
    let mut bytes = [0_u8; FINGERPRINT_BYTES];
    let (pairs, _) = text.as_chunks::<2>();
    for (byte, pair) in bytes.iter_mut().zip(pairs) {
        let pair = std::str::from_utf8(pair).map_err(|_| damaged())?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| damaged())?;
    }
    Ok(bytes)
}

/// Sync secrets in private files. The data directory is the trust boundary of a headless server,
/// which has no operating system keyring. A file is named by the digest of its key reference.
pub struct KeyFiles {
    directory: PathBuf,
}

impl KeyFiles {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self, vault_id: &str, key: SyncKey<'_>) -> PathBuf {
        let digest = Sha256::digest(key_reference(vault_id, key).as_bytes());
        let mut name = String::with_capacity(digest.len() * 2);
        for byte in digest {
            let _ = write!(name, "{byte:02x}");
        }
        self.directory.join(name)
    }
}

impl SyncKeyStore for KeyFiles {
    fn read(&self, vault_id: &str, key: SyncKey<'_>) -> Result<Option<Vec<u8>>, String> {
        read_small(&self.path(vault_id, key))
    }

    fn write(&self, vault_id: &str, key: SyncKey<'_>, secret: &[u8]) -> Result<(), String> {
        ganbaru_atomic_files::create_private_directories(&self.directory)
            .map_err(|error| format!("create key directory: {error}"))?;
        write_private(&self.path(vault_id, key), secret)
    }

    fn remove(&self, vault_id: &str, key: SyncKey<'_>) -> Result<(), String> {
        remove_if_present(&self.path(vault_id, key))
    }
}

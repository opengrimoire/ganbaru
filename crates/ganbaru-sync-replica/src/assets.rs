//! Managed asset files between a replica and the hub. Operations carry only asset references,
//! so each pass uploads the referenced assets the hub lacks before pushing operations, and
//! downloads the referenced assets this device lacks after applying them.
//!
//! Transfers stream: a receiver writes each chunk to a temporary file beside the final path
//! while it hashes the bytes, so it holds one chunk in memory. It moves the file into place only
//! when the size and digest match the reference and the asset's class accepts the bytes, and an
//! aborted or refused transfer removes the temporary file. Paths come from the registered
//! [`AssetClass`] of a reference, never from wire text.

use super::client::{PeerIds, SyncTransport, answer};
use super::wire::{asset_key, asset_to_wire, bytes_from_wire, bytes_to_wire};
use ganbaru_atomic_files::{FileAccess, StagedFile, create_directories_within};
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_ASSETS, PROTOCOL_VERSION, SYNC_ASSET_CHUNK_BYTES, SyncAssetKey,
};
use ganbaru_sync::Engine;
use ganbaru_sync::assets::{AssetClass, AssetRef};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Error code of a request for an asset the hub does not hold.
pub const ASSET_UNAVAILABLE: &str = "asset_unavailable";
/// Error code of uploaded bytes that do not match their reference.
pub const INVALID_ASSET: &str = "invalid_sync_asset";
/// Bound on asset bytes moved in one direction of one pass; the service runs the next pass
/// right away to continue.
pub const MAX_ASSET_PASS_BYTES: usize = 64 * 1024 * 1024;
/// How long an asset the hub confirmed or refused is not offered again. The hub may lose an
/// unreferenced upload to collection, so the confirmation expires.
const SETTLED_UPLOAD_TTL: Duration = Duration::from_secs(60 * 60);
/// How long an asset the hub lacked is not requested again, unless the hub log grows first.
const UNAVAILABLE_TTL: Duration = Duration::from_secs(2 * 60);
/// How long an asset the hub sent damaged is not requested again. New operations do not
/// repair a damaged hub file, so the hub log growing does not end the wait.
const REFUSED_DOWNLOAD_TTL: Duration = Duration::from_secs(60 * 60);
/// Times an upload may restart after the hub lost part of it.
const MAX_UPLOAD_RESTARTS: usize = 2;

/// A failed asset store call.
#[derive(Debug, PartialEq, Eq)]
pub enum AssetStoreError {
    /// The store does not hold the asset at its exact size.
    Unavailable,
    /// Received bytes do not match their reference, or their class refuses them.
    Mismatch(String),
    /// A local fault, such as a failed write.
    Failed(String),
}

impl AssetStoreError {
    /// Diagnostic text of the outcome.
    pub fn describe(&self) -> String {
        match self {
            Self::Unavailable => "the asset is not held at its size".to_string(),
            Self::Mismatch(message) | Self::Failed(message) => message.clone(),
        }
    }
}

fn io_failure(context: &str, error: std::io::Error) -> AssetStoreError {
    AssetStoreError::Failed(format!("{context}: {error}"))
}

/// A class-specific check of a fully received asset, given the temporary file that holds its
/// verified bytes. An error refuses the asset with its message.
pub type AssetCheck = fn(&AssetRef, &Path) -> Result<(), String>;

/// The managed asset files of the vault a sync side serves. Calls block on file IO.
pub trait AssetStore: Send + Sync {
    /// Whether the vault holds a regular file for the asset at its exact size. The digest is
    /// checked when the bytes arrive, not here, so the check stays cheap for every reference.
    fn holds(&self, asset: &AssetRef) -> Result<bool, AssetStoreError>;
    /// At most `max_len` bytes of a held asset from `offset`. The receiver verifies the
    /// assembled bytes.
    fn read_chunk(
        &self,
        asset: &AssetRef,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, AssetStoreError>;
    /// Starts receiving an asset into a temporary file beside its final path.
    fn stage(&self, asset: &AssetRef) -> Result<StagedAsset, AssetStoreError>;
    /// Verifies a completely received asset and moves it into place. An asset the store
    /// already holds is kept as it is.
    fn commit(&self, staged: StagedAsset) -> Result<(), AssetStoreError>;
}

/// An asset being received: the temporary file and the running digest of its bytes. Dropping
/// it before [`AssetStore::commit`] removes the temporary file.
#[derive(Debug)]
pub struct StagedAsset {
    asset: AssetRef,
    file: StagedFile,
    hasher: Sha256,
}

impl StagedAsset {
    /// The reference the bytes must match.
    pub fn asset(&self) -> &AssetRef {
        &self.asset
    }

    /// Bytes received so far, where the next chunk starts.
    pub fn received(&self) -> u32 {
        // Writes never pass the reference size, which is a u32.
        u32::try_from(self.file.written()).unwrap_or(u32::MAX)
    }

    /// Whether every byte of the asset arrived.
    pub fn is_complete(&self) -> bool {
        self.received() == self.asset.byte_size()
    }

    /// Appends the next chunk. A chunk that is empty or ends past the declared size refuses
    /// the transfer, and the caller drops the staged asset.
    pub fn write(&mut self, bytes: &[u8]) -> Result<u32, AssetStoreError> {
        let end = u64::from(self.received()) + bytes.len() as u64;
        if bytes.is_empty() || end > u64::from(self.asset.byte_size()) {
            return Err(AssetStoreError::Mismatch(
                "a chunk ends past the asset".to_string(),
            ));
        }
        self.file
            .write_all(bytes)
            .map_err(|error| io_failure("write asset", error))?;
        self.hasher.update(bytes);
        Ok(self.received())
    }

    /// Checks the size and digest of the received bytes and returns the file to commit.
    fn verified(self) -> Result<(AssetRef, StagedFile), AssetStoreError> {
        if !self.is_complete() {
            return Err(AssetStoreError::Mismatch(
                "the asset bytes do not match their reference".to_string(),
            ));
        }
        let digest = hex(&self.hasher.finalize());
        if digest != self.asset.sha256() {
            return Err(AssetStoreError::Mismatch(
                "the asset bytes do not match their reference".to_string(),
            ));
        }
        Ok((self.asset, self.file))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The asset files under a fixed vault root, each at the path its class derives, with the
/// checks registered for some classes.
#[derive(Clone, Debug)]
pub struct ContentAddressedStore {
    root: PathBuf,
    checks: Vec<(&'static AssetClass, AssetCheck)>,
}

impl ContentAddressedStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            checks: Vec::new(),
        }
    }

    /// Adds a check that every received asset of `class` must pass before it is stored.
    pub fn with_check(mut self, class: &'static AssetClass, check: AssetCheck) -> Self {
        self.checks.push((class, check));
        self
    }

    /// The vault root the files live under.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl AssetStore for ContentAddressedStore {
    fn holds(&self, asset: &AssetRef) -> Result<bool, AssetStoreError> {
        match fs::symlink_metadata(asset.path(&self.root)) {
            Ok(metadata) => {
                Ok(metadata.is_file() && metadata.len() == u64::from(asset.byte_size()))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_failure("inspect asset", error)),
        }
    }

    fn read_chunk(
        &self,
        asset: &AssetRef,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, AssetStoreError> {
        if offset >= asset.byte_size() || !self.holds(asset)? {
            return Err(AssetStoreError::Unavailable);
        }
        let mut file = fs::File::open(asset.path(&self.root))
            .map_err(|error| io_failure("open asset", error))?;
        file.seek(SeekFrom::Start(u64::from(offset)))
            .map_err(|error| io_failure("read asset", error))?;
        let len = max_len.min((asset.byte_size() - offset) as usize);
        let mut bytes = Vec::with_capacity(len);
        file.take(len as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| io_failure("read asset", error))?;
        if bytes.len() != len {
            return Err(AssetStoreError::Unavailable);
        }
        Ok(bytes)
    }

    fn stage(&self, asset: &AssetRef) -> Result<StagedAsset, AssetStoreError> {
        create_directories_within(&self.root, &asset.class().directory(&self.root))
            .map_err(|error| io_failure("create asset directory", error))?;
        let file = StagedFile::create(&asset.path(&self.root), FileAccess::Default)
            .map_err(|error| io_failure("stage asset", error))?;
        Ok(StagedAsset {
            asset: asset.clone(),
            file,
            hasher: Sha256::new(),
        })
    }

    fn commit(&self, staged: StagedAsset) -> Result<(), AssetStoreError> {
        let (asset, file) = staged.verified()?;
        for (class, check) in &self.checks {
            if *class == asset.class() {
                check(&asset, file.staged_path()).map_err(AssetStoreError::Mismatch)?;
            }
        }
        if self.holds(&asset)? {
            return Ok(());
        }
        file.commit()
            .map_err(|error| io_failure("store asset", error))
    }
}

/// Runs store work off the async runtime.
pub(crate) async fn blocking<T, F>(
    store: &Arc<dyn AssetStore>,
    work: F,
) -> Result<T, AssetStoreError>
where
    T: Send + 'static,
    F: FnOnce(&dyn AssetStore) -> Result<T, AssetStoreError> + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || work(store.as_ref()))
        .await
        .map_err(|error| AssetStoreError::Failed(format!("asset worker: {error}")))?
}

/// Appends a chunk to a staged asset off the async runtime. On an error the staged asset is
/// dropped there, which removes its temporary file.
pub(crate) async fn write_staged(
    mut staged: StagedAsset,
    chunk: Vec<u8>,
) -> Result<StagedAsset, AssetStoreError> {
    tokio::task::spawn_blocking(move || staged.write(&chunk).map(|_| staged))
        .await
        .map_err(|error| AssetStoreError::Failed(format!("asset worker: {error}")))?
}

/// Splits assets into those the store holds and those it lacks, keeping their order.
pub(crate) async fn partition_held(
    store: &Arc<dyn AssetStore>,
    references: Vec<AssetRef>,
) -> Result<(Vec<AssetRef>, Vec<AssetRef>), AssetStoreError> {
    blocking(store, move |store| {
        let mut held = Vec::new();
        let mut missing = Vec::new();
        for asset in references {
            if store.holds(&asset)? {
                held.push(asset);
            } else {
                missing.push(asset);
            }
        }
        Ok((held, missing))
    })
    .await
}

/// The memory key of an asset: its class name and digest.
type MemoryKey = (&'static str, String);

fn memory_key(asset: &AssetRef) -> MemoryKey {
    (asset.class().name, asset.sha256().to_string())
}

/// Assets remembered until a deadline.
#[derive(Debug, Default)]
struct AssetDeadlines {
    until: HashMap<MemoryKey, Instant>,
}

impl AssetDeadlines {
    fn remembers(&mut self, asset: &AssetRef, now: Instant) -> bool {
        let key = memory_key(asset);
        match self.until.get(&key) {
            Some(until) if *until > now => true,
            Some(_) => {
                self.until.remove(&key);
                false
            }
            None => false,
        }
    }

    fn remember(&mut self, asset: &AssetRef, ttl: Duration, now: Instant) {
        self.until.insert(memory_key(asset), now + ttl);
    }
}

/// What the asset steps remember between passes over one vault, so a pass does not query or
/// request the same files again.
#[derive(Debug, Default)]
pub struct AssetMemory {
    /// Assets the hub holds or refused.
    settled_uploads: AssetDeadlines,
    /// Assets the hub lacked.
    unavailable: AssetDeadlines,
    /// Assets the hub sent damaged.
    refused_downloads: AssetDeadlines,
}

impl AssetMemory {
    /// Forgets which assets the hub lacked, after its log grew and another device may have
    /// uploaded them.
    pub fn hub_changed(&mut self) {
        self.unavailable.until.clear();
    }
}

/// What one pass of the asset steps moved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AssetTransfer {
    pub uploaded: usize,
    pub downloaded: usize,
    /// A direction stopped at its byte bound with files left to move.
    pub more: bool,
    /// An asset the hub or this device refused.
    pub problem: Option<String>,
}

enum Upload {
    Stored,
    /// The local file changed or vanished since the presence check.
    Missing,
    Refused(String),
}

enum Download {
    Stored,
    Unavailable,
    Refused(String),
}

/// One replica's view for the asset steps of a pass.
pub struct AssetSession<'a, T: SyncTransport> {
    pub transport: &'a T,
    pub ids: &'a PeerIds,
    pub engine: &'a Engine,
    pub pool: &'a SqlitePool,
    pub store: &'a Arc<dyn AssetStore>,
    /// Bytes after which a direction stops, normally [`MAX_ASSET_PASS_BYTES`].
    pub max_bytes: usize,
}

impl<T: SyncTransport> AssetSession<'_, T> {
    /// Uploads the referenced assets this device holds and the hub lacks. Runs before the
    /// operations that reference them are pushed, so other devices find them on the hub.
    pub async fn upload(
        &self,
        memory: &mut AssetMemory,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        let now = Instant::now();
        let references = self
            .referenced()
            .await?
            .into_iter()
            .filter(|asset| !memory.settled_uploads.remembers(asset, now))
            .collect();
        let (held, _) = partition_held(self.store, references)
            .await
            .map_err(|error| error.describe())?;
        let mut lacking = Vec::new();
        for batch in held.chunks(MAX_SYNC_ASSETS) {
            let on_hub = self.query(batch).await?;
            for asset in batch {
                if on_hub.contains(&asset_key(asset)) {
                    memory
                        .settled_uploads
                        .remember(asset, SETTLED_UPLOAD_TTL, now);
                } else {
                    lacking.push(asset);
                }
            }
        }
        let mut moved = 0usize;
        for asset in lacking {
            if moved >= self.max_bytes {
                transfer.more = true;
                break;
            }
            match self.upload_one(asset, &mut moved).await? {
                Upload::Stored => transfer.uploaded += 1,
                Upload::Missing => continue,
                Upload::Refused(message) => {
                    transfer.problem = Some(format!(
                        "the hub refused {} asset {}: {message}",
                        asset.class().name,
                        asset.sha256()
                    ));
                }
            }
            memory
                .settled_uploads
                .remember(asset, SETTLED_UPLOAD_TTL, now);
        }
        Ok(())
    }

    /// Downloads the referenced assets this device lacks from the hub.
    pub async fn download(
        &self,
        memory: &mut AssetMemory,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        let now = Instant::now();
        let references = self
            .referenced()
            .await?
            .into_iter()
            .filter(|asset| {
                !memory.unavailable.remembers(asset, now)
                    && !memory.refused_downloads.remembers(asset, now)
            })
            .collect();
        let (_, missing) = partition_held(self.store, references)
            .await
            .map_err(|error| error.describe())?;
        let mut moved = 0usize;
        for asset in &missing {
            if moved >= self.max_bytes {
                transfer.more = true;
                break;
            }
            match self.download_one(asset, &mut moved).await? {
                Download::Stored => {
                    transfer.downloaded += 1;
                    continue;
                }
                Download::Unavailable => {
                    memory.unavailable.remember(asset, UNAVAILABLE_TTL, now);
                }
                Download::Refused(message) => {
                    transfer.problem = Some(format!(
                        "this device refused {} asset {} from the hub: {message}",
                        asset.class().name,
                        asset.sha256()
                    ));
                    memory
                        .refused_downloads
                        .remember(asset, REFUSED_DOWNLOAD_TTL, now);
                }
            }
        }
        Ok(())
    }

    /// Every asset the vault references.
    async fn referenced(&self) -> Result<Vec<AssetRef>, String> {
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|error| format!("open asset references: {error}"))?;
        self.engine
            .referenced_assets(&mut conn)
            .await
            .map_err(|error| format!("load asset references: {error}"))
    }

    /// The assets of the batch that the hub holds.
    async fn query(&self, batch: &[AssetRef]) -> Result<HashSet<SyncAssetKey>, String> {
        let request = ControlMessage::SyncAssetQuery {
            protocol_version: PROTOCOL_VERSION,
            vault_id: self.ids.vault_id.clone(),
            device_id: self.ids.device_id.clone(),
            assets: batch.iter().map(asset_to_wire).collect(),
        };
        let held = answer(
            self.transport.request(request).await?,
            "SyncAssetPresence",
            |response| match response {
                ControlMessage::SyncAssetPresence { held } => Some(held),
                _ => None,
            },
        )?;
        Ok(held.into_iter().collect())
    }

    async fn upload_one(&self, asset: &AssetRef, moved: &mut usize) -> Result<Upload, String> {
        let size = asset.byte_size();
        let mut offset = 0u32;
        let mut restarts = 0usize;
        while offset < size {
            let chunk = {
                let asset = asset.clone();
                blocking(self.store, move |store| {
                    store.read_chunk(&asset, offset, SYNC_ASSET_CHUNK_BYTES)
                })
                .await
            };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(AssetStoreError::Unavailable) => return Ok(Upload::Missing),
                Err(error) => return Err(error.describe()),
            };
            let request = ControlMessage::SyncAssetPut {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                asset: asset_to_wire(asset),
                offset,
                data: bytes_to_wire(&chunk),
            };
            let response = self.transport.request(request).await?;
            if let ControlMessage::Error { code, message, .. } = &response
                && code == INVALID_ASSET
            {
                return Ok(Upload::Refused(message.clone()));
            }
            let received = answer(response, "SyncAssetReceived", |response| match response {
                ControlMessage::SyncAssetReceived { received } => Some(received),
                _ => None,
            })?;
            *moved += chunk.len();
            let expected = offset + chunk.len() as u32;
            if received > size {
                return Err("the hub reported more asset bytes than were sent".to_string());
            }
            if received != expected && received != size {
                restarts += 1;
                if restarts > MAX_UPLOAD_RESTARTS {
                    return Err(format!(
                        "the hub did not keep the upload of {} asset {}",
                        asset.class().name,
                        asset.sha256()
                    ));
                }
            }
            offset = received;
        }
        Ok(Upload::Stored)
    }

    async fn download_one(&self, asset: &AssetRef, moved: &mut usize) -> Result<Download, String> {
        let size = asset.byte_size();
        let mut staged = {
            let asset = asset.clone();
            blocking(self.store, move |store| store.stage(&asset))
                .await
                .map_err(|error| error.describe())?
        };
        while !staged.is_complete() {
            let request = ControlMessage::SyncAssetGet {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                asset: asset_to_wire(asset),
                offset: staged.received(),
            };
            let response = self.transport.request(request).await?;
            if let ControlMessage::Error { code, .. } = &response
                && code == ASSET_UNAVAILABLE
            {
                return Ok(Download::Unavailable);
            }
            let data = answer(response, "SyncAssetChunk", |response| match response {
                ControlMessage::SyncAssetChunk { data } => Some(data),
                _ => None,
            })?;
            let chunk = bytes_from_wire(&data)?;
            if chunk.is_empty()
                || u64::from(staged.received()) + chunk.len() as u64 > u64::from(size)
            {
                return Ok(Download::Refused(
                    "a chunk does not fit the asset".to_string(),
                ));
            }
            *moved += chunk.len();
            staged = match write_staged(staged, chunk).await {
                Ok(staged) => staged,
                Err(AssetStoreError::Failed(message)) => return Err(message),
                Err(error) => return Ok(Download::Refused(error.describe())),
            };
        }
        match blocking(self.store, move |store| store.commit(staged)).await {
            Ok(()) => Ok(Download::Stored),
            Err(AssetStoreError::Failed(message)) => Err(message),
            Err(error) => Ok(Download::Refused(error.describe())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync::assets::MAX_ASSET_BYTES;

    static CLASS: AssetClass = AssetClass {
        name: "test/files",
        max_bytes: 100,
    };

    fn asset(sha256: char) -> AssetRef {
        AssetRef::new(
            &CLASS,
            sha256.to_string().repeat(64),
            "image/png".into(),
            10,
        )
        .expect("asset reference")
    }

    #[test]
    fn remembered_assets_expire() {
        let start = Instant::now();
        let mut memory = AssetDeadlines::default();
        memory.remember(&asset('a'), Duration::from_secs(10), start);
        assert!(memory.remembers(&asset('a'), start + Duration::from_secs(9)));
        assert!(!memory.remembers(&asset('a'), start + Duration::from_secs(10)));
        assert!(!memory.remembers(&asset('a'), start));
        assert!(!memory.remembers(&asset('b'), start));
    }

    #[test]
    fn remembered_assets_are_told_apart_by_class() {
        static OTHER: AssetClass = AssetClass {
            name: "test/other",
            max_bytes: 100,
        };
        let now = Instant::now();
        let mut memory = AssetDeadlines::default();
        memory.remember(&asset('a'), Duration::from_secs(10), now);
        let other =
            AssetRef::new(&OTHER, "a".repeat(64), "image/png".into(), 10).expect("asset reference");
        assert!(!memory.remembers(&other, now));
        assert!(memory.remembers(&asset('a'), now));
    }

    #[test]
    fn hub_changes_forget_only_unavailable_assets() {
        let now = Instant::now();
        let mut memory = AssetMemory::default();
        memory
            .settled_uploads
            .remember(&asset('a'), SETTLED_UPLOAD_TTL, now);
        memory
            .unavailable
            .remember(&asset('b'), UNAVAILABLE_TTL, now);
        memory
            .refused_downloads
            .remember(&asset('c'), REFUSED_DOWNLOAD_TTL, now);
        memory.hub_changed();
        assert!(memory.settled_uploads.remembers(&asset('a'), now));
        assert!(!memory.unavailable.remembers(&asset('b'), now));
        assert!(memory.refused_downloads.remembers(&asset('c'), now));
    }

    #[test]
    fn assets_bound_matches_the_sync_wire_bound() {
        assert_eq!(
            MAX_ASSET_BYTES,
            ganbaru_handoff::protocol::MAX_SYNC_ASSET_BYTES
        );
    }
}

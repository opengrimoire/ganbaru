//! Quick note image files between a replica and the hub. Operations carry only image
//! references, so each pass uploads the referenced images the hub lacks before pushing
//! operations, and downloads the referenced images this device lacks after applying them. Every
//! receiver verifies the bytes against the reference before it stores them.

use super::client::{PeerIds, SyncTransport, answer};
use super::wire::{asset_to_wire, bytes_from_wire, bytes_to_wire};
use ganbaru_handoff::protocol::{
    ControlMessage, MAX_SYNC_ASSETS, PROTOCOL_VERSION, SYNC_ASSET_CHUNK_BYTES,
};
use ganbaru_quick_notes::images::{self, QuickNoteImageError, ReferencedImage};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Error code of a request for an asset the hub does not hold.
pub const ASSET_UNAVAILABLE: &str = "asset_unavailable";
/// Error code of uploaded bytes that do not match their reference.
pub const INVALID_ASSET: &str = "invalid_sync_asset";
/// Bound on asset bytes moved in one direction of one pass; the service runs the next pass
/// right away to continue.
pub const MAX_ASSET_PASS_BYTES: usize = 64 * 1024 * 1024;
/// How long an image the hub confirmed or refused is not offered again. The hub may lose an
/// unreferenced upload to collection, so the confirmation expires.
const SETTLED_UPLOAD_TTL: Duration = Duration::from_secs(60 * 60);
/// How long an image the hub lacked or sent damaged is not requested again, unless the hub
/// log grows first.
const UNAVAILABLE_TTL: Duration = Duration::from_secs(2 * 60);
/// Times an upload may restart after the hub lost part of it.
const MAX_UPLOAD_RESTARTS: usize = 2;

/// The image files of the vault a sync side serves. Calls block on file IO.
pub trait ImageStore: Send + Sync {
    /// Whether the vault holds the image at its exact size.
    fn holds(&self, image: &ReferencedImage) -> Result<bool, QuickNoteImageError>;
    /// At most `max_len` bytes of a held image from `offset`.
    fn read_chunk(
        &self,
        image: &ReferencedImage,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, QuickNoteImageError>;
    /// Verifies the bytes against the reference and stores them.
    fn store(&self, image: &ReferencedImage, bytes: &[u8]) -> Result<(), QuickNoteImageError>;
}

/// The image files under a fixed vault root.
pub struct VaultImages {
    pub root: PathBuf,
}

impl ImageStore for VaultImages {
    fn holds(&self, image: &ReferencedImage) -> Result<bool, QuickNoteImageError> {
        images::holds_image(&self.root, image)
    }

    fn read_chunk(
        &self,
        image: &ReferencedImage,
        offset: u32,
        max_len: usize,
    ) -> Result<Vec<u8>, QuickNoteImageError> {
        images::read_image_chunk(&self.root, image, offset, max_len)
    }

    fn store(&self, image: &ReferencedImage, bytes: &[u8]) -> Result<(), QuickNoteImageError> {
        images::store_image(&self.root, image, bytes)
    }
}

/// Runs store work off the async runtime.
pub(crate) async fn blocking<T, F>(
    store: &Arc<dyn ImageStore>,
    work: F,
) -> Result<T, QuickNoteImageError>
where
    T: Send + 'static,
    F: FnOnce(&dyn ImageStore) -> Result<T, QuickNoteImageError> + Send + 'static,
{
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || work(store.as_ref()))
        .await
        .map_err(|error| QuickNoteImageError::Failed(format!("quick note image worker: {error}")))?
}

/// Splits images into those the store holds and those it lacks, keeping their order.
pub(crate) async fn partition_held(
    store: &Arc<dyn ImageStore>,
    references: Vec<ReferencedImage>,
) -> Result<(Vec<ReferencedImage>, Vec<ReferencedImage>), QuickNoteImageError> {
    blocking(store, move |store| {
        let mut held = Vec::new();
        let mut missing = Vec::new();
        for image in references {
            if store.holds(&image)? {
                held.push(image);
            } else {
                missing.push(image);
            }
        }
        Ok((held, missing))
    })
    .await
}

/// Digests remembered until a deadline.
#[derive(Debug, Default)]
struct DigestMemory {
    until: HashMap<String, Instant>,
}

impl DigestMemory {
    fn remembers(&mut self, digest: &str, now: Instant) -> bool {
        match self.until.get(digest) {
            Some(until) if *until > now => true,
            Some(_) => {
                self.until.remove(digest);
                false
            }
            None => false,
        }
    }

    fn remember(&mut self, digest: &str, ttl: Duration, now: Instant) {
        self.until.insert(digest.to_string(), now + ttl);
    }
}

/// What the asset steps remember between passes over one vault, so a pass does not query or
/// request the same files again.
#[derive(Debug, Default)]
pub struct AssetMemory {
    /// Images the hub holds or refused.
    settled_uploads: DigestMemory,
    /// Images the hub lacked or sent damaged.
    unavailable: DigestMemory,
}

impl AssetMemory {
    /// Forgets which images the hub lacked, after its log grew and another device may have
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
    /// An image the hub or this device refused.
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
    pub pool: &'a SqlitePool,
    pub store: &'a Arc<dyn ImageStore>,
    /// Bytes after which a direction stops, normally [`MAX_ASSET_PASS_BYTES`].
    pub max_bytes: usize,
}

impl<T: SyncTransport> AssetSession<'_, T> {
    /// Uploads the referenced images this device holds and the hub lacks. Runs before the
    /// operations that reference them are pushed, so other devices find them on the hub.
    pub async fn upload(
        &self,
        memory: &mut AssetMemory,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        let now = Instant::now();
        let references = images::referenced_images(self.pool)
            .await?
            .into_iter()
            .filter(|image| !memory.settled_uploads.remembers(image.file().sha256(), now))
            .collect();
        let (held, _) = partition_held(self.store, references)
            .await
            .map_err(describe_store_error)?;
        let mut lacking = Vec::new();
        for batch in held.chunks(MAX_SYNC_ASSETS) {
            let on_hub = self.query(batch).await?;
            for image in batch {
                if on_hub.contains(image.file().sha256()) {
                    memory
                        .settled_uploads
                        .remember(image.file().sha256(), SETTLED_UPLOAD_TTL, now);
                } else {
                    lacking.push(image);
                }
            }
        }
        let mut moved = 0usize;
        for image in lacking {
            if moved >= self.max_bytes {
                transfer.more = true;
                break;
            }
            match self.upload_one(image, &mut moved).await? {
                Upload::Stored => transfer.uploaded += 1,
                Upload::Missing => continue,
                Upload::Refused(message) => {
                    transfer.problem = Some(format!(
                        "the hub refused image {}: {message}",
                        image.file().sha256()
                    ));
                }
            }
            memory
                .settled_uploads
                .remember(image.file().sha256(), SETTLED_UPLOAD_TTL, now);
        }
        Ok(())
    }

    /// Downloads the referenced images this device lacks from the hub.
    pub async fn download(
        &self,
        memory: &mut AssetMemory,
        transfer: &mut AssetTransfer,
    ) -> Result<(), String> {
        let now = Instant::now();
        let references = images::referenced_images(self.pool)
            .await?
            .into_iter()
            .filter(|image| !memory.unavailable.remembers(image.file().sha256(), now))
            .collect();
        let (_, missing) = partition_held(self.store, references)
            .await
            .map_err(describe_store_error)?;
        let mut moved = 0usize;
        for image in &missing {
            if moved >= self.max_bytes {
                transfer.more = true;
                break;
            }
            match self.download_one(image, &mut moved).await? {
                Download::Stored => {
                    transfer.downloaded += 1;
                    continue;
                }
                Download::Unavailable => {}
                Download::Refused(message) => {
                    transfer.problem = Some(format!(
                        "this device refused image {} from the hub: {message}",
                        image.file().sha256()
                    ));
                }
            }
            memory
                .unavailable
                .remember(image.file().sha256(), UNAVAILABLE_TTL, now);
        }
        Ok(())
    }

    /// Digests of the batch that the hub holds.
    async fn query(&self, batch: &[ReferencedImage]) -> Result<HashSet<String>, String> {
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

    async fn upload_one(
        &self,
        image: &ReferencedImage,
        moved: &mut usize,
    ) -> Result<Upload, String> {
        let size = image.byte_size();
        let mut offset = 0u32;
        let mut restarts = 0usize;
        while offset < size {
            let chunk = {
                let image = image.clone();
                blocking(self.store, move |store| {
                    store.read_chunk(&image, offset, SYNC_ASSET_CHUNK_BYTES)
                })
                .await
            };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(QuickNoteImageError::Unavailable) => return Ok(Upload::Missing),
                Err(error) => return Err(describe_store_error(error)),
            };
            let request = ControlMessage::SyncAssetPut {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                asset: asset_to_wire(image),
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
                return Err("the hub reported more image bytes than were sent".to_string());
            }
            if received != expected && received != size {
                restarts += 1;
                if restarts > MAX_UPLOAD_RESTARTS {
                    return Err(format!(
                        "the hub did not keep the upload of image {}",
                        image.file().sha256()
                    ));
                }
            }
            offset = received;
        }
        Ok(Upload::Stored)
    }

    async fn download_one(
        &self,
        image: &ReferencedImage,
        moved: &mut usize,
    ) -> Result<Download, String> {
        let size = image.byte_size() as usize;
        let mut bytes = Vec::with_capacity(size);
        while bytes.len() < size {
            let request = ControlMessage::SyncAssetGet {
                protocol_version: PROTOCOL_VERSION,
                vault_id: self.ids.vault_id.clone(),
                device_id: self.ids.device_id.clone(),
                asset: asset_to_wire(image),
                offset: bytes.len() as u32,
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
            if chunk.is_empty() || bytes.len() + chunk.len() > size {
                return Ok(Download::Refused(
                    "a chunk does not fit the image".to_string(),
                ));
            }
            *moved += chunk.len();
            bytes.extend_from_slice(&chunk);
        }
        let stored = {
            let image = image.clone();
            blocking(self.store, move |store| store.store(&image, &bytes)).await
        };
        match stored {
            Ok(()) => Ok(Download::Stored),
            Err(QuickNoteImageError::Failed(message)) => Err(message),
            Err(error) => Ok(Download::Refused(describe_store_error(error))),
        }
    }
}

/// Diagnostic text of a store outcome.
pub(crate) fn describe_store_error(error: QuickNoteImageError) -> String {
    match error {
        QuickNoteImageError::Unsupported => "the bytes are not a supported image".to_string(),
        QuickNoteImageError::TooLarge => "the image exceeds the limits".to_string(),
        QuickNoteImageError::Unavailable => {
            "the image bytes do not match their reference".to_string()
        }
        QuickNoteImageError::Failed(message) => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_digests_expire() {
        let start = Instant::now();
        let mut memory = DigestMemory::default();
        memory.remember("a", Duration::from_secs(10), start);
        assert!(memory.remembers("a", start + Duration::from_secs(9)));
        assert!(!memory.remembers("a", start + Duration::from_secs(10)));
        assert!(!memory.remembers("a", start));
        assert!(!memory.remembers("b", start));
    }

    #[test]
    fn hub_changes_forget_only_unavailable_images() {
        let now = Instant::now();
        let mut memory = AssetMemory::default();
        memory
            .settled_uploads
            .remember("a", SETTLED_UPLOAD_TTL, now);
        memory.unavailable.remember("b", UNAVAILABLE_TTL, now);
        memory.hub_changed();
        assert!(memory.settled_uploads.remembers("a", now));
        assert!(!memory.unavailable.remembers("b", now));
    }
}

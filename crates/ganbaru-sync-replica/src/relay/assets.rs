//! Managed asset transfer of a relay pass: which referenced assets the relay already holds,
//! uploading the ones it lacks in chunks, and downloading and verifying the ones this device
//! lacks.

use super::{
    Connection, RelayError, RelayPass, RelayProblem, RelaySession, acquire, relay_asset_id,
    sha256_of, unexpected,
};
use crate::assets::{AssetStore, AssetStoreError, blocking, partition_held, write_staged};
use ganbaru_handoff::protocol::relay::{
    MAX_RELAY_ASSET_QUERY, RelayAssetOutcome, RelayRefusalCode, RelayRequest, RelayResponse,
    decode_relay_asset, decode_relay_bytes, encode_relay_asset, encode_relay_bytes,
};
use ganbaru_sync::RelayKeys;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync_contracts::bounds::RELAY_ASSET_CHUNK_BYTES;
use ganbaru_sync_contracts::relay::MAX_RELAY_ASSET_CHUNK_BYTES;
use ganbaru_sync_contracts::{RelayAssetId, relay_asset_chunk_count};
use std::collections::BTreeSet;
use std::sync::Arc;
use zeroize::Zeroizing;

impl RelaySession<'_> {
    async fn referenced(&self) -> Result<Vec<AssetRef>, RelayError> {
        let mut conn = acquire(self.pool).await?;
        self.engine
            .referenced_assets(&mut conn)
            .await
            .map_err(|error| format!("load asset references: {error}").into())
    }

    /// The relay ids of `assets` the relay holds completely.
    async fn present(
        connection: &mut Connection,
        keys: &RelayKeys,
        assets: &[AssetRef],
    ) -> Result<BTreeSet<RelayAssetId>, RelayError> {
        let mut present = BTreeSet::new();
        for batch in assets.chunks(MAX_RELAY_ASSET_QUERY) {
            let ids = batch
                .iter()
                .map(|asset| relay_asset_id(keys, asset))
                .collect::<Result<Vec<_>, _>>()?;
            let request = RelayRequest::AssetQuery {
                assets: ids
                    .iter()
                    .map(|id| encode_relay_asset(id.as_bytes()))
                    .collect(),
            };
            match connection.request(&request).await? {
                RelayResponse::AssetPresence { assets } => {
                    for text in assets {
                        let bytes = decode_relay_asset(&text)?;
                        present.insert(RelayAssetId::from_bytes(bytes));
                    }
                }
                other => return Err(unexpected("AssetPresence", &other)),
            }
        }
        Ok(present)
    }

    /// Uploads the referenced files this replica holds and the relay lacks, before the
    /// operations that reference them.
    pub(super) async fn upload_assets(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        epoch: u32,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        if keys.newest_epoch() != Some(epoch) {
            return Ok(());
        }
        let (held, _) = partition_held(store, self.referenced().await?)
            .await
            .map_err(|error| error.describe())?;
        let (held, deferred): (Vec<_>, Vec<_>) = held
            .into_iter()
            .partition(|asset| asset.byte_size() <= self.limits.max_asset_bytes);
        pass.deferred_assets += deferred.len();
        let present = Self::present(connection, keys, &held).await?;
        let mut moved = 0usize;
        for asset in &held {
            let id = relay_asset_id(keys, asset)?;
            if present.contains(&id) {
                continue;
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                break;
            }
            match self
                .upload_asset(connection, keys, store, asset, id, &mut moved)
                .await?
            {
                Ok(true) => pass.uploaded += 1,
                Ok(false) => {}
                Err(message) => pass.note(
                    RelayProblem::AssetRefused,
                    format!(
                        "the relay refused {} asset {}: {message}",
                        asset.class().name,
                        asset.sha256()
                    ),
                ),
            }
        }
        Ok(())
    }

    /// Seals and puts every chunk of one file. `Ok(false)` when the local file changed or
    /// vanished; the inner error is the relay's refusal.
    async fn upload_asset(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        asset: &AssetRef,
        id: RelayAssetId,
        moved: &mut usize,
    ) -> Result<Result<bool, String>, RelayError> {
        let size = asset.byte_size() as usize;
        let Some(count) = relay_asset_chunk_count(size) else {
            return Ok(Err(
                "the file is empty or too large for the relay".to_string()
            ));
        };
        for index in 0..count {
            let offset = index as usize * RELAY_ASSET_CHUNK_BYTES;
            let Ok(offset) = u32::try_from(offset) else {
                return Ok(Err("the file is too large for the relay".to_string()));
            };
            let plaintext = {
                let asset = asset.clone();
                blocking(store, move |store| {
                    store.read_chunk(&asset, offset, RELAY_ASSET_CHUNK_BYTES)
                })
                .await
            };
            let plaintext = match plaintext {
                Ok(bytes) if !bytes.is_empty() => Zeroizing::new(bytes),
                Ok(_) | Err(AssetStoreError::Unavailable) => return Ok(Ok(false)),
                Err(error) => return Err(error.describe().into()),
            };
            let chunk = keys
                .seal_asset_chunk(id, index, count, &plaintext)
                .map_err(|error| format!("seal relay asset chunk: {error}"))?;
            *moved += plaintext.len();
            let request = RelayRequest::AssetPut {
                chunk: encode_relay_bytes(&chunk.encode()),
            };
            match connection.request(&request).await? {
                RelayResponse::AssetStored {
                    outcome:
                        RelayAssetOutcome::Stored
                        | RelayAssetOutcome::Duplicate
                        | RelayAssetOutcome::SlotTaken,
                } => {}
                RelayResponse::Refused { code, message, .. } => {
                    return Ok(Err(format!("{code:?}: {message}")));
                }
                other => return Err(unexpected("AssetStored", &other)),
            }
        }
        Ok(Ok(true))
    }

    /// Downloads the referenced files this replica lacks and the relay holds.
    pub(super) async fn download_assets(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        pass: &mut RelayPass,
    ) -> Result<(), RelayError> {
        let (_, missing) = partition_held(store, self.referenced().await?)
            .await
            .map_err(|error| error.describe())?;
        let present = Self::present(connection, keys, &missing).await?;
        let mut moved = 0usize;
        for asset in &missing {
            let id = relay_asset_id(keys, asset)?;
            if !present.contains(&id) {
                continue;
            }
            if asset.byte_size() > self.limits.max_asset_bytes {
                pass.deferred_assets += 1;
                continue;
            }
            if moved >= self.limits.max_bytes {
                pass.more = true;
                break;
            }
            match self
                .download_asset(connection, keys, store, asset, id, &mut moved)
                .await?
            {
                Ok(true) => pass.downloaded += 1,
                Ok(false) => {}
                Err(message) => pass.note(
                    RelayProblem::AssetRefused,
                    format!(
                        "this device refused {} asset {} from the relay: {message}",
                        asset.class().name,
                        asset.sha256()
                    ),
                ),
            }
        }
        Ok(())
    }

    /// Fetches, opens, and stages every chunk of one file, then commits it once the opener
    /// confirms the whole digest. `Ok(false)` when the relay no longer holds it; the inner error
    /// refuses the file, whose staged bytes are dropped.
    async fn download_asset(
        &self,
        connection: &mut Connection,
        keys: &RelayKeys,
        store: &Arc<dyn AssetStore>,
        asset: &AssetRef,
        id: RelayAssetId,
        moved: &mut usize,
    ) -> Result<Result<bool, String>, RelayError> {
        let Some(expected) = relay_asset_chunk_count(asset.byte_size() as usize) else {
            return Ok(Err(
                "the file is empty or too large for the relay".to_string()
            ));
        };
        let sha256 = sha256_of(asset)?;
        let mut opener = keys
            .asset_opener(sha256)
            .map_err(|error| format!("open relay asset: {error}"))?;
        let mut staged = {
            let asset = asset.clone();
            blocking(store, move |store| store.stage(&asset))
                .await
                .map_err(|error| error.describe())?
        };
        for index in 0..expected {
            let request = RelayRequest::AssetGet {
                asset: encode_relay_asset(id.as_bytes()),
                index,
            };
            let text = match connection.request(&request).await? {
                RelayResponse::AssetChunk { chunk } => chunk,
                RelayResponse::Refused {
                    code: RelayRefusalCode::NotFound,
                    ..
                } => return Ok(Ok(false)),
                other => return Err(unexpected("AssetChunk", &other)),
            };
            let bytes =
                decode_relay_bytes("relay asset chunk", &text, MAX_RELAY_ASSET_CHUNK_BYTES)?;
            *moved += bytes.len();
            let (plaintext, count) = match keys.open_asset_chunk(&mut opener, &bytes) {
                Ok(opened) => opened,
                Err(error) => return Ok(Err(error.to_string())),
            };
            if count != expected {
                return Ok(Err(
                    "the chunk count disagrees with the file size".to_string()
                ));
            }
            staged = match write_staged(staged, plaintext).await {
                Ok(staged) => staged,
                Err(error) => return Ok(Err(error.describe())),
            };
        }
        if let Err(error) = opener.finish() {
            return Ok(Err(error.to_string()));
        }
        match blocking(store, move |store| store.commit(staged)).await {
            Ok(()) => Ok(Ok(true)),
            Err(AssetStoreError::Mismatch(message)) => Ok(Err(message)),
            Err(error) => Err(error.describe().into()),
        }
    }
}

//! The key epochs a replica holds for relay records.
//!
//! A replica holds the key of an epoch only through its own envelope: the newest stored epoch
//! whose envelope the replica opens, and the epochs before it through the chain of wrapped
//! previous keys. A device removed before a rotation has no envelope in the rotated epoch, so it
//! opens nothing sealed after its removal.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    OpenedRecord, RelayAssetChunk, RelayAssetId, RelayAssetOpener, RelayError, RelayRecord,
    SignedCertificate, SpaceId, SpaceKey,
};

use crate::{SpaceKeyHolder, StoredSpaceKey};

/// The keys `stored` opens for `holder`, by epoch: the newest stored epoch whose own envelope
/// `holder` opens, then every earlier epoch its chain links reach without a gap.
pub fn open_space_keys(
    space: &SpaceId,
    stored: &[StoredSpaceKey],
    holder: &dyn SpaceKeyHolder,
) -> BTreeMap<u32, SpaceKey> {
    let mut opened = BTreeMap::new();
    let newest = stored.iter().enumerate().rev().find_map(|(index, keys)| {
        let envelope = keys.own_envelope.as_ref()?;
        Some((index, holder.open(space, keys.epoch, envelope)?))
    });
    let Some((start, mut key)) = newest else {
        return opened;
    };
    for index in (0..=start).rev() {
        let previous = stored[index]
            .wrapped_previous
            .as_ref()
            .and_then(|link| link.open(space, &key).ok());
        opened.insert(key.epoch(), key);
        match previous {
            Some(next) if index > 0 && stored[index - 1].epoch == next.epoch() => key = next,
            _ => break,
        }
    }
    opened
}

/// The epoch keys of one space a replica holds, which seal and open its relay records.
pub struct RelayKeys {
    space: SpaceId,
    keys: BTreeMap<u32, SpaceKey>,
}

impl RelayKeys {
    /// The keys `stored` opens for `holder`, see [`open_space_keys`].
    pub fn open(space: SpaceId, stored: &[StoredSpaceKey], holder: &dyn SpaceKeyHolder) -> Self {
        Self {
            space,
            keys: open_space_keys(&space, stored, holder),
        }
    }

    /// Adds a key the replica opened from its own envelope outside the stored chain, such as one
    /// of a key epoch entry it fetched before storing it.
    pub fn insert(&mut self, key: SpaceKey) {
        self.keys.insert(key.epoch(), key);
    }

    /// The space these keys belong to.
    pub fn space(&self) -> SpaceId {
        self.space
    }

    /// The newest epoch held, or `None` before the space has a key.
    pub fn newest_epoch(&self) -> Option<u32> {
        self.keys.keys().next_back().copied()
    }

    /// The key of `epoch`, when held.
    pub fn key(&self, epoch: u32) -> Option<&SpaceKey> {
        self.keys.get(&epoch)
    }

    /// Seals a stored operation and its writer's certificate under the newest epoch held.
    pub fn seal_record(
        &self,
        operation: &[u8],
        certificate: &SignedCertificate,
    ) -> Result<RelayRecord, RelayError> {
        RelayRecord::seal(self.newest_key()?, operation, certificate)
    }

    /// Seals a stored operation and its writer's certificate under the key of `epoch`, for
    /// records a relay at an earlier epoch still has to receive before the next one registers.
    pub fn seal_record_at(
        &self,
        epoch: u32,
        operation: &[u8],
        certificate: &SignedCertificate,
    ) -> Result<RelayRecord, RelayError> {
        let key = self
            .keys
            .get(&epoch)
            .ok_or(RelayError::UnknownEpoch(epoch))?;
        RelayRecord::seal(key, operation, certificate)
    }

    fn newest_key(&self) -> Result<&SpaceKey, RelayError> {
        self.keys.values().next_back().ok_or(RelayError::NoKey)
    }

    fn first_key(&self) -> Result<&SpaceKey, RelayError> {
        self.keys.get(&1).ok_or(RelayError::UnknownEpoch(1))
    }

    /// The relay id of the asset whose plaintext has `sha256`, derived from the epoch 1 key.
    pub fn asset_id(&self, sha256: &[u8; 32]) -> Result<RelayAssetId, RelayError> {
        RelayAssetId::derive(&self.space, self.first_key()?, sha256)
    }

    /// Seals chunk `index` of `count` of an asset under the newest epoch held, so a caller
    /// streams a file without holding it whole.
    pub fn seal_asset_chunk(
        &self,
        asset: RelayAssetId,
        index: u32,
        count: u32,
        plaintext: &[u8],
    ) -> Result<RelayAssetChunk, RelayError> {
        RelayAssetChunk::seal(
            self.newest_key()?,
            &self.space,
            asset,
            index,
            count,
            plaintext,
        )
    }

    /// Starts opening the chunks of the asset whose plaintext has `sha256`.
    pub fn asset_opener(&self, sha256: [u8; 32]) -> Result<RelayAssetOpener, RelayError> {
        RelayAssetOpener::new(&self.space, self.first_key()?, sha256)
    }

    /// Opens the next chunk of an asset from relay bytes with the key of its stated epoch, and
    /// returns its plaintext with the chunk count the authenticated header states. On any error
    /// the caller abandons the asset.
    pub fn open_asset_chunk(
        &self,
        opener: &mut RelayAssetOpener,
        bytes: &[u8],
    ) -> Result<(Vec<u8>, u32), RelayError> {
        let chunk = RelayAssetChunk::decode(bytes)?;
        let header = chunk.header();
        if header.space != self.space {
            return Err(RelayError::Mismatch("space"));
        }
        let key = self
            .keys
            .get(&header.epoch)
            .ok_or(RelayError::UnknownEpoch(header.epoch))?;
        let count = header.count;
        let plaintext = opener.open_chunk(&chunk, key)?;
        Ok((plaintext, count))
    }

    /// Opens relay record bytes of this space with the key of their stated epoch. A record of an
    /// epoch the replica holds no key for is refused, so nothing it carries reaches the store.
    pub fn open_record(&self, bytes: &[u8]) -> Result<OpenedRecord, RelayError> {
        let record = RelayRecord::decode(bytes)?;
        let header = record.header();
        if header.space != self.space {
            return Err(RelayError::Mismatch("space"));
        }
        let key = self
            .keys
            .get(&header.epoch)
            .ok_or(RelayError::UnknownEpoch(header.epoch))?;
        record.open(key)
    }
}

impl std::fmt::Debug for RelayKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RelayKeys")
            .field("space", &self.space)
            .field("epochs", &self.keys.keys().collect::<Vec<_>>())
            .finish()
    }
}

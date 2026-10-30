//! The key epochs a replica holds for relay records.
//!
//! A replica holds the key of an epoch only through its own envelope: the newest stored epoch
//! whose envelope the replica opens, and the epochs before it through the chain of wrapped
//! previous keys. A device removed before a rotation has no envelope in the rotated epoch, so it
//! opens nothing sealed after its removal.

use std::collections::BTreeMap;

use ganbaru_sync_contracts::{
    OpenedRecord, RelayError, RelayRecord, SignedCertificate, SpaceId, SpaceKey,
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
        let key = self
            .keys
            .values()
            .next_back()
            .ok_or(RelayError::UnknownEpoch(0))?;
        RelayRecord::seal(key, operation, certificate)
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

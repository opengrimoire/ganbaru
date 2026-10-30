//! The installation's space keys: its device encryption key and the plaintext epoch keys it
//! opened, which live in memory only, and the recovery key a kit flow lends them while it runs.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::writer::{SyncKey, SyncKeyStore, ensure_device_key};
use ganbaru_sync::{Engine, SpaceContext, SpaceKeyHolder, StoredSpaceKey};
use ganbaru_sync_contracts::crypto::SPACE_KEY_BYTES;
use ganbaru_sync_contracts::{
    DeviceKeyPair, EncryptionKeyPair, EncryptionPublicKey, KeyEnvelope, SpaceId, SpaceKey,
};
use sqlx::SqliteConnection;
use zeroize::Zeroizing;

/// The installation's device encryption key for a vault, created once and kept, because the
/// space's log publishes its public half and every later envelope names it. Key stores may
/// block, so callers run this off the async runtime.
pub fn ensure_encryption_key(
    keys: &dyn SyncKeyStore,
    vault_id: &str,
) -> Result<EncryptionKeyPair, String> {
    if let Some(secret) = keys.read(vault_id, SyncKey::DeviceEncryption)? {
        let secret: Zeroizing<Vec<u8>> = Zeroizing::new(secret);
        let mut bytes = Zeroizing::new([0u8; SPACE_KEY_BYTES]);
        if secret.len() != SPACE_KEY_BYTES {
            return Err("stored sync encryption key is malformed".to_string());
        }
        bytes.copy_from_slice(&secret);
        return EncryptionKeyPair::from_secret(bytes)
            .map_err(|error| format!("decode sync encryption key: {error}"));
    }
    let pair = EncryptionKeyPair::generate()
        .map_err(|error| format!("create sync encryption key: {error}"))?;
    keys.write(vault_id, SyncKey::DeviceEncryption, pair.secret_bytes())?;
    Ok(pair)
}

/// The keys the installation opens and seals space keys with, and the epoch keys it opened.
pub struct SpaceKeys {
    device: DeviceKeyPair,
    encryption: EncryptionKeyPair,
    opened: Mutex<BTreeMap<u32, SpaceKey>>,
}

impl SpaceKeys {
    /// Keys of an installation with its device signing key and device encryption key.
    pub fn new(device: DeviceKeyPair, encryption: EncryptionKeyPair) -> Self {
        Self {
            device,
            encryption,
            opened: Mutex::new(BTreeMap::new()),
        }
    }

    /// Reads the installation's device key and device encryption key of a vault, creating
    /// each missing one once. Key stores may block, so callers run this off the async runtime.
    pub fn load(keys: &dyn SyncKeyStore, vault_id: &str) -> Result<Self, String> {
        let device = ensure_device_key(keys, vault_id)?;
        let encryption = ensure_encryption_key(keys, vault_id)?;
        Ok(Self::new(device, encryption))
    }

    /// The public half of the device encryption key.
    pub fn public_key(&self) -> EncryptionPublicKey {
        self.encryption.public_key()
    }

    /// The key of `epoch`, if this installation opened it.
    pub fn epoch_key(&self, epoch: u32) -> Option<SpaceKey> {
        self.opened.lock().ok()?.get(&epoch).cloned()
    }

    /// The newest epoch whose key this installation opened.
    pub fn newest_epoch(&self) -> Option<u32> {
        self.opened.lock().ok()?.keys().next_back().copied()
    }

    /// Opens the epoch keys of the selected chain from the envelopes and chain links the
    /// engine holds for this installation, replacing those opened before: the newest epoch
    /// with an envelope this installation opens, then each older one through the chain links.
    /// Epochs newer than that stay closed until an envelope arrives. Returns the newest epoch
    /// opened, `None` when none opened.
    pub async fn open_epochs(
        &self,
        engine: &Engine,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
    ) -> Result<Option<u32>, String> {
        let stored = engine
            .space_keys(conn, ctx)
            .await
            .map_err(|error| format!("read sync space keys: {error}"))?;
        let opened = self.open_stored(&ctx.space, &stored);
        let newest = opened.keys().next_back().copied();
        *self
            .opened
            .lock()
            .map_err(|_| "sync space keys are poisoned".to_string())? = opened;
        Ok(newest)
    }

    /// The keys `stored` opens, by epoch.
    fn open_stored(&self, space: &SpaceId, stored: &[StoredSpaceKey]) -> BTreeMap<u32, SpaceKey> {
        let mut opened = BTreeMap::new();
        let Some(start) = stored.iter().rposition(|keys| {
            keys.own_envelope
                .as_ref()
                .is_some_and(|envelope| self.open(space, keys.epoch, envelope).is_some())
        }) else {
            return opened;
        };
        let Some(mut key) = stored[start]
            .own_envelope
            .as_ref()
            .and_then(|envelope| self.open(space, stored[start].epoch, envelope))
        else {
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
}

impl SpaceKeyHolder for SpaceKeys {
    fn device_key(&self) -> &DeviceKeyPair {
        &self.device
    }

    fn encryption_key(&self) -> EncryptionPublicKey {
        self.encryption.public_key()
    }

    fn open(&self, space: &SpaceId, epoch: u32, envelope: &KeyEnvelope) -> Option<SpaceKey> {
        open_with(&self.encryption, space, epoch, envelope)
    }
}

impl std::fmt::Debug for SpaceKeys {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpaceKeys")
            .field("encryption", &self.encryption.public_key())
            .finish_non_exhaustive()
    }
}

/// The installation's keys with the recovery encryption key a recovery kit derives, which
/// opens the recovery envelope of every key epoch. An anchor the kit recovered needs it to
/// sign its own device key and seal the rotations after the handover before an envelope to its
/// own key exists. The kit flow builds this for one call and drops it, so the recovery key
/// never outlives the flow.
pub struct KitKeys<'a> {
    base: &'a dyn SpaceKeyHolder,
    recovery: &'a EncryptionKeyPair,
}

impl<'a> KitKeys<'a> {
    /// `base` with the kit's recovery encryption key.
    pub fn new(base: &'a dyn SpaceKeyHolder, recovery: &'a EncryptionKeyPair) -> Self {
        Self { base, recovery }
    }
}

impl SpaceKeyHolder for KitKeys<'_> {
    fn device_key(&self) -> &DeviceKeyPair {
        self.base.device_key()
    }

    fn encryption_key(&self) -> EncryptionPublicKey {
        self.base.encryption_key()
    }

    fn open(&self, space: &SpaceId, epoch: u32, envelope: &KeyEnvelope) -> Option<SpaceKey> {
        self.base
            .open(space, epoch, envelope)
            .or_else(|| open_with(self.recovery, space, epoch, envelope))
    }
}

impl std::fmt::Debug for KitKeys<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KitKeys")
            .field("encryption", &self.base.encryption_key())
            .field("recovery", &self.recovery.public_key())
            .finish_non_exhaustive()
    }
}

/// Opens `envelope` with `pair` when the envelope names it.
fn open_with(
    pair: &EncryptionKeyPair,
    space: &SpaceId,
    epoch: u32,
    envelope: &KeyEnvelope,
) -> Option<SpaceKey> {
    (pair.public_key().key_id() == envelope.recipient)
        .then(|| envelope.open(space, epoch, pair).ok())
        .flatten()
}

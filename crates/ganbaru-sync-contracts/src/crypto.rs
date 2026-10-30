//! Space keys: device encryption keys, epoch keys, their subkeys, key envelopes, and the key
//! chain.
//!
//! Each installation holds one static X25519 device encryption key pair per vault, kept as
//! Ganbaru's own 32 secret bytes. Each key epoch of a space has a random 32-byte space key
//! (epochs count from 1; 0 means no key). Every use of a space key takes its own subkey through
//! HKDF-SHA256 with info `ganbaru-sync/<purpose>/v1`, a zero byte, and the space id.
//!
//! A key envelope seals a space key to one device encryption key or the recovery key with RFC
//! 9180 HPKE in base mode (DHKEM X25519 with HKDF-SHA256, HKDF-SHA256, ChaCha20-Poly1305), info
//! [`SPACE_KEY_INFO`], a zero byte, the space id, the epoch, and the recipient key id, and empty
//! AAD. A chain link seals the previous epoch's key under the next with XChaCha20-Poly1305 and
//! AAD [`KEY_CHAIN_DOMAIN`], a zero byte, the space id, and the previous epoch, so a holder of
//! the current key walks back to any older epoch and never forward.
//!
//! Randomness comes from `ring`'s system generator for space keys, device secrets, and chain
//! nonces. HPKE encapsulation draws from the operating system generator of the `hpke` crate.
//! Plaintext keys live only in [`Zeroizing`] buffers.

use crate::ids::{SpaceId, hex};
use crate::keys::{RelayAccessKeyPair, RelayAccessPublicKey};
use crate::signing::domain_digest;
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use ring::hkdf::{HKDF_SHA256, KeyType, Salt};
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use zeroize::Zeroizing;

/// Device encryption key id: the first [`KEY_ID_BYTES`] of a digest of the public key.
pub const DEVICE_KEY_ID_DOMAIN: &[u8] = b"ganbaru-sync/device-key/v1";
/// HPKE info prefix of a key envelope.
pub const SPACE_KEY_INFO: &[u8] = b"ganbaru-sync/space-key/v1";
/// AAD prefix of a key chain link.
pub const KEY_CHAIN_DOMAIN: &[u8] = b"ganbaru-sync/key-chain/v1";

/// Length of a space key, a device encryption secret, and a subkey.
pub const SPACE_KEY_BYTES: usize = 32;
/// Length of an X25519 public key.
pub const ENCRYPTION_PUBLIC_KEY_BYTES: usize = 32;
/// Length of a key id.
pub const KEY_ID_BYTES: usize = 16;
/// Length of an HPKE encapsulated key for DHKEM X25519.
pub const ENCAPSULATED_KEY_BYTES: usize = 32;
/// Length of an AEAD tag.
pub const TAG_BYTES: usize = 16;
/// Length of a sealed space key: the key and its tag.
pub const SEALED_KEY_BYTES: usize = SPACE_KEY_BYTES + TAG_BYTES;
/// Length of an encoded key envelope.
pub const KEY_ENVELOPE_BYTES: usize = KEY_ID_BYTES + ENCAPSULATED_KEY_BYTES + SEALED_KEY_BYTES;
/// Length of an XChaCha20-Poly1305 nonce.
pub const CHAIN_NONCE_BYTES: usize = 24;
/// Length of an encoded chain link.
pub const CHAIN_LINK_BYTES: usize = CHAIN_NONCE_BYTES + SEALED_KEY_BYTES;

/// Key material that cannot be generated, sealed, or opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    /// The random source failed.
    Random,
    /// A public key is not a usable X25519 key.
    InvalidKey,
    /// An envelope or chain link does not open with the given key, or was sealed for another
    /// space, epoch, or recipient.
    Open,
    /// Sealing failed.
    Seal,
    /// An epoch is outside the range the operation allows.
    Epoch,
}

impl fmt::Display for CryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random => formatter.write_str("secure random source failed"),
            Self::InvalidKey => formatter.write_str("encryption key is invalid"),
            Self::Open => formatter.write_str("sealed key does not open"),
            Self::Seal => formatter.write_str("sealing a key failed"),
            Self::Epoch => formatter.write_str("key epoch is out of range"),
        }
    }
}

impl std::error::Error for CryptoError {}

fn random_secret() -> Result<Zeroizing<[u8; SPACE_KEY_BYTES]>, CryptoError> {
    let mut secret = Zeroizing::new([0u8; SPACE_KEY_BYTES]);
    SystemRandom::new()
        .fill(secret.as_mut())
        .map_err(|_| CryptoError::Random)?;
    Ok(secret)
}

/// The id of a device encryption key or a recovery encryption key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId([u8; KEY_ID_BYTES]);

impl KeyId {
    /// Wraps raw id bytes.
    pub const fn from_bytes(bytes: [u8; KEY_ID_BYTES]) -> Self {
        Self(bytes)
    }

    /// Raw id bytes.
    pub const fn as_bytes(&self) -> &[u8; KEY_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("KeyId").field(&hex(&self.0)).finish()
    }
}

/// A static X25519 public key: a device encryption key or the recovery encryption key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EncryptionPublicKey([u8; ENCRYPTION_PUBLIC_KEY_BYTES]);

impl EncryptionPublicKey {
    /// Parses a raw key, refusing a key of small order, whose shared secrets carry no
    /// contribution from the other side.
    pub fn from_bytes(bytes: [u8; ENCRYPTION_PUBLIC_KEY_BYTES]) -> Result<Self, CryptoError> {
        // A clamped scalar clears the cofactor, so the product is the identity exactly when the
        // point has small order. The scalar is public; only the point is checked.
        let probe = x25519_dalek::x25519([1u8; 32], bytes);
        if probe == [0u8; 32] {
            return Err(CryptoError::InvalidKey);
        }
        Ok(Self(bytes))
    }

    /// Parses a raw key from a slice, see [`Self::from_bytes`].
    pub fn from_slice(bytes: &[u8]) -> Result<Self, CryptoError> {
        let bytes = <[u8; ENCRYPTION_PUBLIC_KEY_BYTES]>::try_from(bytes)
            .map_err(|_| CryptoError::InvalidKey)?;
        Self::from_bytes(bytes)
    }

    /// Raw key bytes.
    pub fn as_bytes(&self) -> &[u8; ENCRYPTION_PUBLIC_KEY_BYTES] {
        &self.0
    }

    /// The key id: the first bytes of the device key digest of the public key.
    pub fn key_id(&self) -> KeyId {
        let digest = domain_digest(DEVICE_KEY_ID_DOMAIN, &[&self.0]);
        let mut id = [0u8; KEY_ID_BYTES];
        id.copy_from_slice(&digest[..KEY_ID_BYTES]);
        KeyId(id)
    }

    fn hpke_key(&self) -> Result<<X25519HkdfSha256 as Kem>::PublicKey, CryptoError> {
        <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(&self.0)
            .map_err(|_| CryptoError::InvalidKey)
    }
}

impl fmt::Debug for EncryptionPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EncryptionPublicKey")
            .field(&hex(&self.0))
            .finish()
    }
}

/// A static X25519 key pair held as its 32 secret bytes.
pub struct EncryptionKeyPair {
    secret: Zeroizing<[u8; SPACE_KEY_BYTES]>,
    public: EncryptionPublicKey,
}

impl EncryptionKeyPair {
    /// Generates a fresh key pair from the secure random source.
    pub fn generate() -> Result<Self, CryptoError> {
        Self::from_secret(random_secret()?)
    }

    /// Restores a key pair from its stored secret bytes. The secret must come from
    /// [`Self::generate`] or a key derivation over a secure secret, except in test vectors.
    pub fn from_secret(secret: Zeroizing<[u8; SPACE_KEY_BYTES]>) -> Result<Self, CryptoError> {
        let scalar = x25519_dalek::StaticSecret::from(*secret);
        let public =
            EncryptionPublicKey::from_bytes(x25519_dalek::PublicKey::from(&scalar).to_bytes())?;
        Ok(Self { secret, public })
    }

    /// The secret bytes, for storage in a key store only.
    pub fn secret_bytes(&self) -> &[u8; SPACE_KEY_BYTES] {
        &self.secret
    }

    /// Public half of the key pair.
    pub fn public_key(&self) -> EncryptionPublicKey {
        self.public
    }

    pub(crate) fn hpke_key(&self) -> Result<<X25519HkdfSha256 as Kem>::PrivateKey, CryptoError> {
        <X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(self.secret.as_ref())
            .map_err(|_| CryptoError::InvalidKey)
    }
}

impl fmt::Debug for EncryptionKeyPair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("EncryptionKeyPair")
            .field(&self.public)
            .finish()
    }
}

/// The plaintext key of one key epoch of a space.
pub struct SpaceKey {
    epoch: u32,
    key: Zeroizing<[u8; SPACE_KEY_BYTES]>,
}

impl SpaceKey {
    /// Generates a fresh key for `epoch`, which must be at least 1.
    pub fn generate(epoch: u32) -> Result<Self, CryptoError> {
        Self::from_bytes(epoch, random_secret()?)
    }

    /// Wraps key bytes for `epoch`, which must be at least 1.
    pub fn from_bytes(
        epoch: u32,
        key: Zeroizing<[u8; SPACE_KEY_BYTES]>,
    ) -> Result<Self, CryptoError> {
        if epoch == 0 {
            return Err(CryptoError::Epoch);
        }
        Ok(Self { epoch, key })
    }

    /// The key epoch.
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// The key bytes.
    pub fn bytes(&self) -> &[u8; SPACE_KEY_BYTES] {
        &self.key
    }

    /// The subkey of this epoch for `purpose` in `space`.
    pub fn subkey(&self, purpose: Subkey, space: &SpaceId) -> Zeroizing<[u8; SPACE_KEY_BYTES]> {
        derive_subkey(&self.key, purpose, space)
    }

    /// The relay access key pair of this epoch, whose seed is the `relay-access` subkey.
    pub fn relay_access_key(&self, space: &SpaceId) -> Result<RelayAccessKeyPair, CryptoError> {
        RelayAccessKeyPair::from_seed(&self.subkey(Subkey::RelayAccess, space))
            .map_err(|_| CryptoError::InvalidKey)
    }

    /// The relay access public key of this epoch, as a `KeyEpoch` entry publishes it.
    pub fn relay_access_public_key(
        &self,
        space: &SpaceId,
    ) -> Result<RelayAccessPublicKey, CryptoError> {
        Ok(self.relay_access_key(space)?.public_key())
    }
}

impl Clone for SpaceKey {
    fn clone(&self) -> Self {
        Self {
            epoch: self.epoch,
            key: Zeroizing::new(*self.key),
        }
    }
}

impl fmt::Debug for SpaceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaceKey")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

/// The purposes a space key derives a subkey for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subkey {
    /// Relay record encryption.
    RelayRecord,
    /// Relay asset encryption.
    RelayAsset,
    /// Relay access signing, see [`SpaceKey::relay_access_key`].
    RelayAccess,
    /// Backup key wrapping.
    BackupWrap,
    /// Relay asset ids, taken from epoch 1 only so ids stay stable across rotations.
    RelayAssetId,
}

impl Subkey {
    /// Every purpose.
    pub const ALL: [Self; 5] = [
        Self::RelayRecord,
        Self::RelayAsset,
        Self::RelayAccess,
        Self::BackupWrap,
        Self::RelayAssetId,
    ];

    /// The HKDF info label of the purpose.
    pub fn info(self) -> &'static [u8] {
        match self {
            Self::RelayRecord => b"ganbaru-sync/relay-record/v1",
            Self::RelayAsset => b"ganbaru-sync/relay-asset/v1",
            Self::RelayAccess => b"ganbaru-sync/relay-access/v1",
            Self::BackupWrap => b"ganbaru-sync/backup-wrap/v1",
            Self::RelayAssetId => b"ganbaru-sync/relay-asset-id/v1",
        }
    }
}

struct SubkeyLength;

impl KeyType for SubkeyLength {
    fn len(&self) -> usize {
        SPACE_KEY_BYTES
    }
}

fn derive_subkey(
    key: &[u8; SPACE_KEY_BYTES],
    purpose: Subkey,
    space: &SpaceId,
) -> Zeroizing<[u8; SPACE_KEY_BYTES]> {
    let mut output = Zeroizing::new([0u8; SPACE_KEY_BYTES]);
    let info: [&[u8]; 3] = [purpose.info(), &[0u8], space.as_bytes()];
    // HKDF-SHA256 can expand up to 8,160 bytes, so a 32-byte output never fails.
    Salt::new(HKDF_SHA256, &[])
        .extract(key)
        .expand(&info, SubkeyLength)
        .and_then(|okm| okm.fill(output.as_mut()))
        .unwrap_or_else(|_| unreachable!("HKDF-SHA256 expands 32 bytes"));
    output
}

/// The HPKE info of the envelope sealing epoch `epoch` of `space` to `recipient`.
pub fn envelope_info(space: &SpaceId, epoch: u32, recipient: &KeyId) -> Vec<u8> {
    let mut info =
        Vec::with_capacity(SPACE_KEY_INFO.len() + 1 + space.as_bytes().len() + 4 + KEY_ID_BYTES);
    info.extend_from_slice(SPACE_KEY_INFO);
    info.push(0);
    info.extend_from_slice(space.as_bytes());
    info.extend_from_slice(&epoch.to_be_bytes());
    info.extend_from_slice(recipient.as_bytes());
    info
}

/// The AAD of the chain link that seals epoch `previous` of `space` under the next epoch.
pub fn chain_aad(space: &SpaceId, previous: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(KEY_CHAIN_DOMAIN.len() + 1 + space.as_bytes().len() + 4);
    aad.extend_from_slice(KEY_CHAIN_DOMAIN);
    aad.push(0);
    aad.extend_from_slice(space.as_bytes());
    aad.extend_from_slice(&previous.to_be_bytes());
    aad
}

/// One space key sealed to one recipient: the recipient key id, the HPKE encapsulated key, and
/// the sealed key with its tag.
#[derive(Clone, PartialEq, Eq)]
pub struct KeyEnvelope {
    /// Id of the recipient's encryption key.
    pub recipient: KeyId,
    /// HPKE encapsulated key.
    pub encapsulated: [u8; ENCAPSULATED_KEY_BYTES],
    /// The space key sealed with its tag.
    pub sealed: [u8; SEALED_KEY_BYTES],
}

impl KeyEnvelope {
    /// Seals `key` to `recipient` for `space`.
    pub fn seal(
        space: &SpaceId,
        key: &SpaceKey,
        recipient: &EncryptionPublicKey,
    ) -> Result<Self, CryptoError> {
        let info = envelope_info(space, key.epoch, &recipient.key_id());
        Self::seal_secret(&info, key.bytes(), recipient)
    }

    /// Seals 32 secret bytes to `recipient` with the HPKE info `info` and empty AAD.
    pub(crate) fn seal_secret(
        info: &[u8],
        secret: &[u8; SPACE_KEY_BYTES],
        recipient: &EncryptionPublicKey,
    ) -> Result<Self, CryptoError> {
        let (encapsulated, sealed) = hpke::single_shot_seal::<
            ChaCha20Poly1305,
            HkdfSha256,
            X25519HkdfSha256,
        >(
            &OpModeS::Base, &recipient.hpke_key()?, info, secret, &[]
        )
        .map_err(|_| CryptoError::Seal)?;
        Self::from_parts(recipient.key_id(), &encapsulated.to_bytes(), &sealed)
    }

    /// Opens 32 secret bytes sealed to `pair` with the HPKE info `info`. It fails when the
    /// envelope names another recipient or was sealed with another info.
    pub(crate) fn open_secret(
        &self,
        info: &[u8],
        pair: &EncryptionKeyPair,
    ) -> Result<Zeroizing<[u8; SPACE_KEY_BYTES]>, CryptoError> {
        if pair.public.key_id() != self.recipient {
            return Err(CryptoError::Open);
        }
        let encapsulated = <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(&self.encapsulated)
            .map_err(|_| CryptoError::Open)?;
        let opened = Zeroizing::new(
            hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
                &OpModeR::Base,
                &pair.hpke_key()?,
                &encapsulated,
                info,
                &self.sealed,
                &[],
            )
            .map_err(|_| CryptoError::Open)?,
        );
        let mut secret = Zeroizing::new([0u8; SPACE_KEY_BYTES]);
        if opened.len() != SPACE_KEY_BYTES {
            return Err(CryptoError::Open);
        }
        secret.copy_from_slice(&opened);
        Ok(secret)
    }

    pub(crate) fn from_parts(
        recipient: KeyId,
        encapsulated: &[u8],
        sealed: &[u8],
    ) -> Result<Self, CryptoError> {
        Ok(Self {
            recipient,
            encapsulated: encapsulated.try_into().map_err(|_| CryptoError::Seal)?,
            sealed: sealed.try_into().map_err(|_| CryptoError::Seal)?,
        })
    }

    /// Opens the envelope as the key of epoch `epoch` of `space` with `pair`. It fails when
    /// the envelope names another recipient or was sealed for another space or epoch.
    pub fn open(
        &self,
        space: &SpaceId,
        epoch: u32,
        pair: &EncryptionKeyPair,
    ) -> Result<SpaceKey, CryptoError> {
        let info = envelope_info(space, epoch, &self.recipient);
        SpaceKey::from_bytes(epoch, self.open_secret(&info, pair)?)
    }

    /// The encoding: key id, encapsulated key, and sealed key.
    pub fn to_bytes(&self) -> [u8; KEY_ENVELOPE_BYTES] {
        let mut bytes = [0u8; KEY_ENVELOPE_BYTES];
        bytes[..KEY_ID_BYTES].copy_from_slice(self.recipient.as_bytes());
        bytes[KEY_ID_BYTES..KEY_ID_BYTES + ENCAPSULATED_KEY_BYTES]
            .copy_from_slice(&self.encapsulated);
        bytes[KEY_ID_BYTES + ENCAPSULATED_KEY_BYTES..].copy_from_slice(&self.sealed);
        bytes
    }

    /// Parses an encoding of [`KEY_ENVELOPE_BYTES`].
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != KEY_ENVELOPE_BYTES {
            return None;
        }
        let (id, rest) = bytes.split_at(KEY_ID_BYTES);
        let (encapsulated, sealed) = rest.split_at(ENCAPSULATED_KEY_BYTES);
        Some(Self {
            recipient: KeyId(id.try_into().ok()?),
            encapsulated: encapsulated.try_into().ok()?,
            sealed: sealed.try_into().ok()?,
        })
    }
}

impl fmt::Debug for KeyEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeyEnvelope")
            .field("recipient", &self.recipient)
            .finish_non_exhaustive()
    }
}

/// The previous epoch's key sealed under the next: a random nonce and the sealed key.
#[derive(Clone, PartialEq, Eq)]
pub struct ChainLink {
    /// XChaCha20-Poly1305 nonce.
    pub nonce: [u8; CHAIN_NONCE_BYTES],
    /// The previous key sealed with its tag.
    pub sealed: [u8; SEALED_KEY_BYTES],
}

impl ChainLink {
    /// Seals `previous` under `next`, whose epoch must follow it.
    pub fn seal(
        space: &SpaceId,
        next: &SpaceKey,
        previous: &SpaceKey,
    ) -> Result<Self, CryptoError> {
        let mut nonce = [0u8; CHAIN_NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| CryptoError::Random)?;
        Self::seal_with_nonce(space, next, previous, nonce)
    }

    pub(crate) fn seal_with_nonce(
        space: &SpaceId,
        next: &SpaceKey,
        previous: &SpaceKey,
        nonce: [u8; CHAIN_NONCE_BYTES],
    ) -> Result<Self, CryptoError> {
        if previous.epoch.checked_add(1) != Some(next.epoch) {
            return Err(CryptoError::Epoch);
        }
        let cipher =
            XChaCha20Poly1305::new_from_slice(next.bytes()).map_err(|_| CryptoError::Seal)?;
        let mut buffer = Zeroizing::new(*previous.bytes());
        let tag = cipher
            .encrypt_inout_detached(
                &XNonce::from(nonce),
                &chain_aad(space, previous.epoch),
                buffer.as_mut_slice().into(),
            )
            .map_err(|_| CryptoError::Seal)?;
        let mut sealed = [0u8; SEALED_KEY_BYTES];
        sealed[..SPACE_KEY_BYTES].copy_from_slice(buffer.as_ref());
        sealed[SPACE_KEY_BYTES..].copy_from_slice(&tag);
        Ok(Self { nonce, sealed })
    }

    /// Opens the key of the epoch before `next`.
    pub fn open(&self, space: &SpaceId, next: &SpaceKey) -> Result<SpaceKey, CryptoError> {
        let previous = next.epoch.checked_sub(1).filter(|epoch| *epoch > 0);
        let previous = previous.ok_or(CryptoError::Epoch)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(next.bytes()).map_err(|_| CryptoError::Open)?;
        let mut buffer = Zeroizing::new([0u8; SPACE_KEY_BYTES]);
        buffer.copy_from_slice(&self.sealed[..SPACE_KEY_BYTES]);
        let tag = Tag::try_from(&self.sealed[SPACE_KEY_BYTES..]).map_err(|_| CryptoError::Open)?;
        cipher
            .decrypt_inout_detached(
                &XNonce::from(self.nonce),
                &chain_aad(space, previous),
                buffer.as_mut_slice().into(),
                &tag,
            )
            .map_err(|_| CryptoError::Open)?;
        SpaceKey::from_bytes(previous, buffer)
    }

    /// The encoding: nonce and sealed key.
    pub fn to_bytes(&self) -> [u8; CHAIN_LINK_BYTES] {
        let mut bytes = [0u8; CHAIN_LINK_BYTES];
        bytes[..CHAIN_NONCE_BYTES].copy_from_slice(&self.nonce);
        bytes[CHAIN_NONCE_BYTES..].copy_from_slice(&self.sealed);
        bytes
    }

    /// Parses an encoding of [`CHAIN_LINK_BYTES`].
    pub fn from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != CHAIN_LINK_BYTES {
            return None;
        }
        let (nonce, sealed) = bytes.split_at(CHAIN_NONCE_BYTES);
        Some(Self {
            nonce: nonce.try_into().ok()?,
            sealed: sealed.try_into().ok()?,
        })
    }
}

impl fmt::Debug for ChainLink {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ChainLink").finish_non_exhaustive()
    }
}

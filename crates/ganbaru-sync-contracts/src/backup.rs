//! The `GBBK` encrypted backup format, version 1: its header, file key, and chunk sealing.
//!
//! A backup file is a clear header followed by the backup archive cut into chunks. The header
//! holds the magic, the version, the space id, the key epoch, the random 32-byte file key
//! wrapped under the epoch's [`Subkey::BackupWrap`] subkey when the epoch is at least 1, and an
//! HPKE envelope of the file key to the space's recovery encryption key with info
//! [`BACKUP_INFO`], a zero byte, and the space id. Any member holding the epoch key opens the
//! backup through the wrap; the recovery kit opens every backup, including epoch 0 backups of a
//! vault that was never linked.
//!
//! The body is a sequence of chunk frames: a u32 ciphertext length, a random 24-byte nonce, and
//! the XChaCha20-Poly1305 ciphertext of at most [`BACKUP_CHUNK_BYTES`] plaintext under the file
//! key. Every chunk but the last holds exactly [`BACKUP_CHUNK_BYTES`]; an empty archive is one
//! empty last chunk. The AAD of a chunk is the SHA-256 of the encoded header, the chunk index
//! as a u64, and a last flag byte, so a reader that marks the frame before the end of the file
//! as last refuses reordered, truncated, extended, and header-swapped files.
//!
//! Nonces and file keys come from `ring`'s system generator; plaintext keys live only in
//! [`Zeroizing`] buffers. Nothing here formats a key or plaintext.

use crate::crypto::{
    CHAIN_NONCE_BYTES, CryptoError, EncryptionKeyPair, EncryptionPublicKey, KEY_ENVELOPE_BYTES,
    KeyEnvelope, SPACE_KEY_BYTES, SpaceKey, Subkey, TAG_BYTES,
};
use crate::ids::{ID_BYTES, SpaceId};
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ring::rand::{SecureRandom, SystemRandom};
use sha2::{Digest, Sha256};
use std::fmt;
use zeroize::Zeroizing;

/// Magic bytes that open every backup file.
pub const BACKUP_MAGIC: [u8; 4] = *b"GBBK";
/// The format version this build writes and reads.
pub const BACKUP_VERSION: u8 = 1;
/// HPKE info prefix of the recovery envelope.
pub const BACKUP_INFO: &[u8] = b"ganbaru-sync/backup/v1";
/// AAD prefix of the epoch wrap.
pub const BACKUP_WRAP_DOMAIN: &[u8] = b"ganbaru-sync/backup-wrap/v1";
/// Plaintext bytes of every chunk but the last.
pub const BACKUP_CHUNK_BYTES: usize = 1024 * 1024;
/// Largest archive a backup holds, 100 GiB.
pub const MAX_BACKUP_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
/// Most chunks a backup holds: the full chunks of the largest archive and a last chunk.
pub const MAX_BACKUP_CHUNKS: u64 = MAX_BACKUP_ARCHIVE_BYTES / BACKUP_CHUNK_BYTES as u64 + 1;
/// Length of a chunk nonce.
pub const BACKUP_NONCE_BYTES: usize = CHAIN_NONCE_BYTES;
/// Length of the file key.
pub const FILE_KEY_BYTES: usize = SPACE_KEY_BYTES;
/// Length of an encoded epoch wrap: its nonce and the sealed file key.
pub const EPOCH_WRAP_BYTES: usize = BACKUP_NONCE_BYTES + FILE_KEY_BYTES + TAG_BYTES;
/// Length of the header prefix that fixes the header length: magic, version, space, epoch.
pub const HEADER_PREFIX_BYTES: usize = BACKUP_MAGIC.len() + 1 + ID_BYTES + 4;
/// Length of the frame fields before a chunk's ciphertext: its length and nonce.
pub const CHUNK_FRAME_PREFIX_BYTES: usize = 4 + BACKUP_NONCE_BYTES;
/// Largest chunk ciphertext: a full chunk and its tag.
pub const MAX_CHUNK_CIPHERTEXT_BYTES: usize = BACKUP_CHUNK_BYTES + TAG_BYTES;
/// Largest backup file: the longest header, then every chunk of the largest archive with its
/// frame fields and tag. A copy of a picked file stops reading beyond it.
pub const MAX_BACKUP_FILE_BYTES: u64 = (HEADER_PREFIX_BYTES + EPOCH_WRAP_BYTES + KEY_ENVELOPE_BYTES)
    as u64
    + MAX_BACKUP_CHUNKS * (CHUNK_FRAME_PREFIX_BYTES + TAG_BYTES) as u64
    + MAX_BACKUP_ARCHIVE_BYTES;

/// A backup that cannot be created or opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupError {
    /// The bytes do not start with [`BACKUP_MAGIC`].
    NotBackup,
    /// The backup was written in a format version this build does not read.
    Version(u8),
    /// The file ends inside the header or before its last chunk.
    Truncated,
    /// A field holds a value the format forbids: a chunk length out of bounds, a short chunk
    /// before the last, an epoch wrap at epoch 0, or bytes after the last chunk.
    Malformed,
    /// The backup holds more chunks than [`MAX_BACKUP_CHUNKS`].
    TooLarge,
    /// The key offered belongs to another epoch, or the backup has no epoch wrap.
    Epoch,
    /// The epoch key or recovery key offered does not open the file key.
    Key,
    /// A chunk fails authentication: it was changed, moved, or comes from another file.
    Authentication,
    /// A chunk was sealed or opened after the last chunk.
    Finished,
    /// The random source failed.
    Random,
    /// Sealing failed.
    Seal,
}

impl fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBackup => formatter.write_str("file is not a Ganbaru backup"),
            Self::Version(version) => {
                write!(formatter, "backup format version {version} is not supported")
            }
            Self::Truncated => formatter.write_str("backup file is incomplete"),
            Self::Malformed => formatter.write_str("backup file is malformed"),
            Self::TooLarge => formatter.write_str("backup file is too large"),
            Self::Epoch => formatter.write_str("backup was made with another key epoch"),
            Self::Key => formatter.write_str("backup does not open with this key"),
            Self::Authentication => formatter.write_str("backup file was changed or damaged"),
            Self::Finished => formatter.write_str("backup stream already ended"),
            Self::Random => formatter.write_str("secure random source failed"),
            Self::Seal => formatter.write_str("sealing the backup failed"),
        }
    }
}

impl std::error::Error for BackupError {}

/// The random key every chunk of one backup is sealed under.
pub struct FileKey(Zeroizing<[u8; FILE_KEY_BYTES]>);

impl FileKey {
    fn generate() -> Result<Self, BackupError> {
        let mut key = Zeroizing::new([0u8; FILE_KEY_BYTES]);
        SystemRandom::new()
            .fill(key.as_mut())
            .map_err(|_| BackupError::Random)?;
        Ok(Self(key))
    }

    #[cfg(test)]
    pub(crate) fn from_bytes(bytes: [u8; FILE_KEY_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> &[u8; FILE_KEY_BYTES] {
        &self.0
    }

    fn cipher(&self) -> Result<XChaCha20Poly1305, BackupError> {
        XChaCha20Poly1305::new_from_slice(self.0.as_ref()).map_err(|_| BackupError::Seal)
    }
}

impl fmt::Debug for FileKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("FileKey").finish_non_exhaustive()
    }
}

/// The file key sealed under an epoch's backup wrap subkey.
#[derive(Clone, PartialEq, Eq)]
pub struct EpochWrap {
    /// XChaCha20-Poly1305 nonce.
    pub nonce: [u8; BACKUP_NONCE_BYTES],
    /// The file key sealed with its tag.
    pub sealed: [u8; FILE_KEY_BYTES + TAG_BYTES],
}

impl fmt::Debug for EpochWrap {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EpochWrap").finish_non_exhaustive()
    }
}

/// The AAD of the epoch wrap of `epoch` in `space`.
pub fn wrap_aad(space: &SpaceId, epoch: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(BACKUP_WRAP_DOMAIN.len() + 1 + space.as_bytes().len() + 4);
    aad.extend_from_slice(BACKUP_WRAP_DOMAIN);
    aad.push(0);
    aad.extend_from_slice(space.as_bytes());
    aad.extend_from_slice(&epoch.to_be_bytes());
    aad
}

/// The HPKE info of the recovery envelope of a backup of `space`.
pub fn recovery_info(space: &SpaceId) -> Vec<u8> {
    let mut info = Vec::with_capacity(BACKUP_INFO.len() + 1 + space.as_bytes().len());
    info.extend_from_slice(BACKUP_INFO);
    info.push(0);
    info.extend_from_slice(space.as_bytes());
    info
}

/// The clear header of a backup.
#[derive(Clone, PartialEq, Eq)]
pub struct BackupHeader {
    /// Space the backed up vault belongs to.
    pub space: SpaceId,
    /// Key epoch of the epoch wrap, 0 when the backup has none.
    pub epoch: u32,
    /// The file key under the epoch's backup wrap subkey, present exactly when `epoch` is at
    /// least 1.
    pub epoch_wrap: Option<EpochWrap>,
    /// The file key sealed to the space's recovery encryption key.
    pub recovery: KeyEnvelope,
}

impl fmt::Debug for BackupHeader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BackupHeader")
            .field("space", &self.space)
            .field("epoch", &self.epoch)
            .field("recovery", &self.recovery.recipient)
            .finish_non_exhaustive()
    }
}

impl BackupHeader {
    /// Seals a fresh file key for a backup of `space`: wrapped under `epoch_key` when the
    /// installation holds one, and always sealed to `recovery`.
    pub fn seal(
        space: &SpaceId,
        epoch_key: Option<&SpaceKey>,
        recovery: &EncryptionPublicKey,
    ) -> Result<(Self, FileKey), BackupError> {
        let file_key = FileKey::generate()?;
        let epoch_wrap = match epoch_key {
            Some(key) => {
                let mut nonce = [0u8; BACKUP_NONCE_BYTES];
                SystemRandom::new()
                    .fill(&mut nonce)
                    .map_err(|_| BackupError::Random)?;
                Some(seal_wrap(space, key, &file_key, nonce)?)
            }
            None => None,
        };
        let recovery = KeyEnvelope::seal_secret(&recovery_info(space), &file_key.0, recovery)
            .map_err(seal_error)?;
        let header = Self {
            space: *space,
            epoch: epoch_key.map_or(0, SpaceKey::epoch),
            epoch_wrap,
            recovery,
        };
        Ok((header, file_key))
    }

    /// The header length the prefix announces, after checking its magic and version.
    pub fn length(prefix: &[u8; HEADER_PREFIX_BYTES]) -> Result<usize, BackupError> {
        if prefix[..BACKUP_MAGIC.len()] != BACKUP_MAGIC {
            return Err(BackupError::NotBackup);
        }
        let version = prefix[BACKUP_MAGIC.len()];
        if version != BACKUP_VERSION {
            return Err(BackupError::Version(version));
        }
        let epoch = u32::from_be_bytes(
            prefix[HEADER_PREFIX_BYTES - 4..]
                .try_into()
                .map_err(|_| BackupError::Malformed)?,
        );
        let wrap = if epoch == 0 { 0 } else { EPOCH_WRAP_BYTES };
        Ok(HEADER_PREFIX_BYTES + wrap + KEY_ENVELOPE_BYTES)
    }

    /// Decodes a whole header, refusing trailing bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, BackupError> {
        let prefix: &[u8; HEADER_PREFIX_BYTES] = bytes
            .get(..HEADER_PREFIX_BYTES)
            .and_then(|prefix| prefix.try_into().ok())
            .ok_or(BackupError::Truncated)?;
        let length = Self::length(prefix)?;
        if bytes.len() < length {
            return Err(BackupError::Truncated);
        }
        if bytes.len() > length {
            return Err(BackupError::Malformed);
        }
        let space_start = BACKUP_MAGIC.len() + 1;
        let space: [u8; ID_BYTES] = bytes[space_start..space_start + ID_BYTES]
            .try_into()
            .map_err(|_| BackupError::Malformed)?;
        let epoch = u32::from_be_bytes(
            bytes[space_start + ID_BYTES..HEADER_PREFIX_BYTES]
                .try_into()
                .map_err(|_| BackupError::Malformed)?,
        );
        let mut rest = &bytes[HEADER_PREFIX_BYTES..];
        let epoch_wrap = if epoch == 0 {
            None
        } else {
            let (wrap, tail) = rest.split_at(EPOCH_WRAP_BYTES);
            rest = tail;
            let (nonce, sealed) = wrap.split_at(BACKUP_NONCE_BYTES);
            Some(EpochWrap {
                nonce: nonce.try_into().map_err(|_| BackupError::Malformed)?,
                sealed: sealed.try_into().map_err(|_| BackupError::Malformed)?,
            })
        };
        let recovery = KeyEnvelope::from_slice(rest).ok_or(BackupError::Malformed)?;
        Ok(Self {
            space: SpaceId::from_bytes(space),
            epoch,
            epoch_wrap,
            recovery,
        })
    }

    /// The encoding: magic, version, space, epoch, the epoch wrap when present, and the
    /// recovery envelope.
    pub fn to_bytes(&self) -> Vec<u8> {
        let wrap = self.epoch_wrap.as_ref().map_or(0, |_| EPOCH_WRAP_BYTES);
        let mut bytes = Vec::with_capacity(HEADER_PREFIX_BYTES + wrap + KEY_ENVELOPE_BYTES);
        bytes.extend_from_slice(&BACKUP_MAGIC);
        bytes.push(BACKUP_VERSION);
        bytes.extend_from_slice(self.space.as_bytes());
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        if let Some(wrap) = &self.epoch_wrap {
            bytes.extend_from_slice(&wrap.nonce);
            bytes.extend_from_slice(&wrap.sealed);
        }
        bytes.extend_from_slice(&self.recovery.to_bytes());
        bytes
    }

    /// SHA-256 of the encoding, which every chunk's AAD binds.
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.to_bytes()).into()
    }

    /// Opens the file key with the key of the header's epoch.
    pub fn open_with_epoch_key(&self, key: &SpaceKey) -> Result<FileKey, BackupError> {
        let wrap = self.epoch_wrap.as_ref().ok_or(BackupError::Epoch)?;
        if key.epoch() != self.epoch {
            return Err(BackupError::Epoch);
        }
        let subkey = key.subkey(Subkey::BackupWrap, &self.space);
        let cipher =
            XChaCha20Poly1305::new_from_slice(subkey.as_ref()).map_err(|_| BackupError::Key)?;
        let mut buffer = Zeroizing::new([0u8; FILE_KEY_BYTES]);
        buffer.copy_from_slice(&wrap.sealed[..FILE_KEY_BYTES]);
        let tag = Tag::try_from(&wrap.sealed[FILE_KEY_BYTES..]).map_err(|_| BackupError::Key)?;
        cipher
            .decrypt_inout_detached(
                &XNonce::from(wrap.nonce),
                &wrap_aad(&self.space, self.epoch),
                buffer.as_mut_slice().into(),
                &tag,
            )
            .map_err(|_| BackupError::Key)?;
        Ok(FileKey(buffer))
    }

    /// Opens the file key with the recovery encryption key a recovery kit derives.
    pub fn open_with_recovery(&self, recovery: &EncryptionKeyPair) -> Result<FileKey, BackupError> {
        self.recovery
            .open_secret(&recovery_info(&self.space), recovery)
            .map(FileKey)
            .map_err(|_| BackupError::Key)
    }
}

fn seal_error(error: CryptoError) -> BackupError {
    match error {
        CryptoError::Random => BackupError::Random,
        _ => BackupError::Seal,
    }
}

pub(crate) fn seal_wrap(
    space: &SpaceId,
    key: &SpaceKey,
    file_key: &FileKey,
    nonce: [u8; BACKUP_NONCE_BYTES],
) -> Result<EpochWrap, BackupError> {
    let subkey = key.subkey(Subkey::BackupWrap, space);
    let cipher =
        XChaCha20Poly1305::new_from_slice(subkey.as_ref()).map_err(|_| BackupError::Seal)?;
    let mut buffer = Zeroizing::new(*file_key.0);
    let tag = cipher
        .encrypt_inout_detached(
            &XNonce::from(nonce),
            &wrap_aad(space, key.epoch()),
            buffer.as_mut_slice().into(),
        )
        .map_err(|_| BackupError::Seal)?;
    let mut sealed = [0u8; FILE_KEY_BYTES + TAG_BYTES];
    sealed[..FILE_KEY_BYTES].copy_from_slice(buffer.as_ref());
    sealed[FILE_KEY_BYTES..].copy_from_slice(&tag);
    Ok(EpochWrap { nonce, sealed })
}

/// The AAD of chunk `index` of the backup whose header digest is `digest`.
pub fn chunk_aad(digest: &[u8; 32], index: u64, last: bool) -> [u8; 32 + 8 + 1] {
    let mut aad = [0u8; 32 + 8 + 1];
    aad[..32].copy_from_slice(digest);
    aad[32..40].copy_from_slice(&index.to_be_bytes());
    aad[40] = u8::from(last);
    aad
}

/// Seals the chunks of one backup in order.
pub struct ChunkSealer {
    key: FileKey,
    digest: [u8; 32],
    index: u64,
    finished: bool,
}

impl fmt::Debug for ChunkSealer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChunkSealer")
            .field("index", &self.index)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl ChunkSealer {
    /// A sealer for the backup with `header`, whose file key is `key`.
    pub fn new(header: &BackupHeader, key: FileKey) -> Self {
        Self {
            key,
            digest: header.digest(),
            index: 0,
            finished: false,
        }
    }

    /// Encrypts the plaintext chunk in `buffer` in place and appends its tag, returning the
    /// chunk's nonce. A chunk that is not `last` must hold exactly [`BACKUP_CHUNK_BYTES`].
    pub fn seal_in_place(
        &mut self,
        buffer: &mut Vec<u8>,
        last: bool,
    ) -> Result<[u8; BACKUP_NONCE_BYTES], BackupError> {
        let mut nonce = [0u8; BACKUP_NONCE_BYTES];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| BackupError::Random)?;
        self.seal_in_place_with_nonce(buffer, last, nonce)?;
        Ok(nonce)
    }

    pub(crate) fn seal_in_place_with_nonce(
        &mut self,
        buffer: &mut Vec<u8>,
        last: bool,
        nonce: [u8; BACKUP_NONCE_BYTES],
    ) -> Result<(), BackupError> {
        if self.finished {
            return Err(BackupError::Finished);
        }
        let full = buffer.len() == BACKUP_CHUNK_BYTES;
        if buffer.len() > BACKUP_CHUNK_BYTES || (!last && !full) {
            return Err(BackupError::Malformed);
        }
        if self.index >= MAX_BACKUP_CHUNKS {
            return Err(BackupError::TooLarge);
        }
        let tag = self
            .key
            .cipher()?
            .encrypt_inout_detached(
                &XNonce::from(nonce),
                &chunk_aad(&self.digest, self.index, last),
                buffer.as_mut_slice().into(),
            )
            .map_err(|_| BackupError::Seal)?;
        buffer.extend_from_slice(&tag);
        self.index += 1;
        self.finished = last;
        Ok(())
    }

    /// Whether the last chunk was sealed.
    pub fn finished(&self) -> bool {
        self.finished
    }
}

/// Opens the chunks of one backup in order.
pub struct ChunkOpener {
    key: FileKey,
    digest: [u8; 32],
    index: u64,
    finished: bool,
}

impl fmt::Debug for ChunkOpener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChunkOpener")
            .field("index", &self.index)
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl ChunkOpener {
    /// An opener for the backup with `header`, whose file key is `key`.
    pub fn new(header: &BackupHeader, key: FileKey) -> Self {
        Self {
            key,
            digest: header.digest(),
            index: 0,
            finished: false,
        }
    }

    /// Checks the ciphertext length a frame announces before the reader allocates for it.
    pub fn check_length(&self, length: u32) -> Result<usize, BackupError> {
        if self.finished {
            return Err(BackupError::Malformed);
        }
        if self.index >= MAX_BACKUP_CHUNKS {
            return Err(BackupError::TooLarge);
        }
        let length = usize::try_from(length).map_err(|_| BackupError::Malformed)?;
        if !(TAG_BYTES..=MAX_CHUNK_CIPHERTEXT_BYTES).contains(&length) {
            return Err(BackupError::Malformed);
        }
        Ok(length)
    }

    /// Decrypts the chunk ciphertext in `buffer` in place, leaving its plaintext. `last` is
    /// whether the frame ends the file. Nothing is left in `buffer` on failure.
    pub fn open_in_place(
        &mut self,
        nonce: &[u8; BACKUP_NONCE_BYTES],
        buffer: &mut Vec<u8>,
        last: bool,
    ) -> Result<(), BackupError> {
        let result = self.open_buffer(nonce, buffer, last);
        if result.is_err() {
            buffer.clear();
        }
        result
    }

    fn open_buffer(
        &mut self,
        nonce: &[u8; BACKUP_NONCE_BYTES],
        buffer: &mut Vec<u8>,
        last: bool,
    ) -> Result<(), BackupError> {
        let length = u32::try_from(buffer.len()).map_err(|_| BackupError::Malformed)?;
        self.check_length(length)?;
        let plaintext = buffer.len() - TAG_BYTES;
        if !last && plaintext != BACKUP_CHUNK_BYTES {
            return Err(BackupError::Malformed);
        }
        let tag = Tag::try_from(&buffer[plaintext..]).map_err(|_| BackupError::Malformed)?;
        buffer.truncate(plaintext);
        self.key.cipher().and_then(|cipher| {
            cipher
                .decrypt_inout_detached(
                    &XNonce::from(*nonce),
                    &chunk_aad(&self.digest, self.index, last),
                    buffer.as_mut_slice().into(),
                    &tag,
                )
                .map_err(|_| BackupError::Authentication)
        })?;
        self.index += 1;
        self.finished = last;
        Ok(())
    }

    /// Whether the last chunk was opened.
    pub fn finished(&self) -> bool {
        self.finished
    }
}

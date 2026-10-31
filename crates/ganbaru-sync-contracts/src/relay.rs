//! Relay records and relay asset chunks: the sealed forms a relay stores and serves.
//!
//! A relay record carries one stored operation and its writer's signed certificate, sealed with
//! XChaCha20-Poly1305 under the `relay-record` subkey of one key epoch. A relay asset chunk
//! carries one chunk of a managed asset under the `relay-asset` subkey. The clear headers name
//! only the slot a relay needs (space, writer, sequence, asset id, epoch, chunk position), and the
//! AAD covers every header byte, so a relay cannot move a ciphertext to another slot. Opening a
//! record checks the decrypted operation against the clear slot, and reassembling an asset checks
//! chunk order, completeness, and the plaintext digest.

use crate::bounds::{
    MAX_CERTIFICATE_BYTES, MAX_OPERATION_BYTES, MAX_RELAY_ASSET_BYTES, MAX_RELAY_ASSET_CHUNKS,
    MAX_RELAY_RECORD_PLAINTEXT_BYTES, RELAY_ASSET_CHUNK_BYTES,
};
use crate::certificate::{CertificateError, SignedCertificate};
use crate::codec::{CodecError, Reader, Writer};
use crate::crypto::{CryptoError, SpaceKey, Subkey, TAG_BYTES};
use crate::ids::{Seq, SpaceId, WriterId, hex};
use crate::op::Envelope;
use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use ring::digest::{Context, SHA256};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;

/// Magic bytes that open every relay record.
pub const RELAY_RECORD_MAGIC: &[u8; 4] = b"GBSR";
/// Relay record format version.
pub const RELAY_RECORD_VERSION: u8 = 1;
/// AAD domain of relay records, followed by a zero byte and the clear header.
pub const RELAY_RECORD_DOMAIN: &[u8] = b"ganbaru-sync/relay-record/v1";
/// Clear relay record header: magic, version, space, writer, sequence, and epoch.
pub const RELAY_RECORD_HEADER_BYTES: usize = 4 + 1 + 16 + 16 + 8 + 4;
/// Magic bytes that open every relay asset chunk.
pub const RELAY_ASSET_MAGIC: &[u8; 4] = b"GBSA";
/// Relay asset chunk format version.
pub const RELAY_ASSET_VERSION: u8 = 1;
/// Clear relay asset chunk header: magic, version, space, asset id, epoch, chunk index, and
/// chunk count.
pub const RELAY_ASSET_HEADER_BYTES: usize = 4 + 1 + 16 + RELAY_ASSET_ID_BYTES + 4 + 4 + 4;
/// Length of a relay asset id.
pub const RELAY_ASSET_ID_BYTES: usize = 32;
/// Length of a relay nonce.
pub const RELAY_NONCE_BYTES: usize = 24;
/// Largest relay record encoding.
pub const MAX_RELAY_RECORD_BYTES: usize =
    RELAY_RECORD_HEADER_BYTES + RELAY_NONCE_BYTES + MAX_RELAY_RECORD_PLAINTEXT_BYTES + TAG_BYTES;
/// Largest relay asset chunk encoding.
pub const MAX_RELAY_ASSET_CHUNK_BYTES: usize =
    RELAY_ASSET_HEADER_BYTES + RELAY_NONCE_BYTES + RELAY_ASSET_CHUNK_BYTES + TAG_BYTES;

/// A relay record or asset chunk that cannot be sealed, parsed, or opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelayError {
    /// The encoding is malformed or exceeds a bound.
    Codec(CodecError),
    /// The format version is newer than this build reads.
    Version(u8),
    /// The replica holds no key of the stated epoch.
    UnknownEpoch(u32),
    /// The replica holds no epoch key at all, so it cannot seal.
    NoKey,
    /// The key offered is of another epoch than the one stated.
    WrongEpoch {
        /// The epoch the header states.
        stated: u32,
        /// The epoch of the key offered.
        key: u32,
    },
    /// The ciphertext does not authenticate under the key, space, and header.
    Authentication,
    /// A decrypted field disagrees with the clear header, or a chunk with its asset.
    Mismatch(&'static str),
    /// The certificate inside a record does not verify.
    Certificate(CertificateError),
    /// A chunk arrived out of order or twice.
    Order,
    /// The chunk set ended before its last chunk.
    Incomplete,
    /// The reassembled plaintext does not have the referenced digest.
    Digest,
    /// Sealing or key derivation failed.
    Crypto(CryptoError),
}

impl fmt::Display for RelayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(formatter, "relay encoding: {error}"),
            Self::Version(version) => write!(formatter, "relay format version {version} is newer"),
            Self::UnknownEpoch(epoch) => write!(formatter, "key epoch {epoch} is not held"),
            Self::NoKey => formatter.write_str("no key epoch is held"),
            Self::WrongEpoch { stated, key } => {
                write!(
                    formatter,
                    "relay epoch {stated} offered a key of epoch {key}"
                )
            }
            Self::Authentication => formatter.write_str("relay ciphertext does not authenticate"),
            Self::Mismatch(field) => write!(formatter, "relay {field} disagrees with its header"),
            Self::Certificate(error) => write!(formatter, "relay certificate: {error}"),
            Self::Order => formatter.write_str("relay asset chunk is out of order"),
            Self::Incomplete => formatter.write_str("relay asset chunk set is incomplete"),
            Self::Digest => formatter.write_str("relay asset digest does not match"),
            Self::Crypto(error) => write!(formatter, "relay key: {error}"),
        }
    }
}

impl std::error::Error for RelayError {}

impl From<CodecError> for RelayError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

fn random_nonce() -> Result<[u8; RELAY_NONCE_BYTES], RelayError> {
    let mut nonce = [0u8; RELAY_NONCE_BYTES];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| RelayError::Crypto(CryptoError::Random))?;
    Ok(nonce)
}

fn seal_payload(
    key: &SpaceKey,
    purpose: Subkey,
    space: &SpaceId,
    aad: &[u8],
    plaintext: &[u8],
    nonce: [u8; RELAY_NONCE_BYTES],
) -> Result<Vec<u8>, RelayError> {
    let subkey = key.subkey(purpose, space);
    let cipher = XChaCha20Poly1305::new_from_slice(subkey.as_ref())
        .map_err(|_| RelayError::Crypto(CryptoError::Seal))?;
    let mut buffer = Vec::with_capacity(plaintext.len() + TAG_BYTES);
    buffer.extend_from_slice(plaintext);
    let tag = cipher
        .encrypt_inout_detached(&XNonce::from(nonce), aad, buffer.as_mut_slice().into())
        .map_err(|_| RelayError::Crypto(CryptoError::Seal))?;
    buffer.extend_from_slice(&tag);
    Ok(buffer)
}

fn open_payload(
    key: &SpaceKey,
    purpose: Subkey,
    space: &SpaceId,
    aad: &[u8],
    nonce: &[u8; RELAY_NONCE_BYTES],
    ciphertext: &[u8],
) -> Result<Vec<u8>, RelayError> {
    let split = ciphertext
        .len()
        .checked_sub(TAG_BYTES)
        .ok_or(RelayError::Codec(CodecError::Truncated))?;
    let (sealed, tag) = ciphertext.split_at(split);
    let tag = Tag::try_from(tag).map_err(|_| RelayError::Authentication)?;
    let subkey = key.subkey(purpose, space);
    let cipher = XChaCha20Poly1305::new_from_slice(subkey.as_ref())
        .map_err(|_| RelayError::Authentication)?;
    let mut buffer = sealed.to_vec();
    cipher
        .decrypt_inout_detached(
            &XNonce::from(*nonce),
            aad,
            buffer.as_mut_slice().into(),
            &tag,
        )
        .map_err(|_| RelayError::Authentication)?;
    Ok(buffer)
}

fn check_epoch(stated: u32, key: &SpaceKey) -> Result<(), RelayError> {
    if key.epoch() == stated {
        Ok(())
    } else {
        Err(RelayError::WrongEpoch {
            stated,
            key: key.epoch(),
        })
    }
}

fn read_version(reader: &mut Reader<'_>, current: u8) -> Result<(), RelayError> {
    match reader.u8()? {
        version if version == current => Ok(()),
        0 => Err(CodecError::Malformed("format version").into()),
        version => Err(RelayError::Version(version)),
    }
}

/// The clear header of a relay record: the slot a relay stores it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayRecordHeader {
    /// Space id.
    pub space: SpaceId,
    /// Writer id.
    pub writer: WriterId,
    /// Sequence of the operation, from 1.
    pub seq: Seq,
    /// Key epoch the record is sealed under, from 1.
    pub epoch: u32,
}

impl RelayRecordHeader {
    /// The header encoding, every byte before the nonce.
    pub fn encode(&self) -> [u8; RELAY_RECORD_HEADER_BYTES] {
        let mut writer = Writer::with_capacity(RELAY_RECORD_HEADER_BYTES);
        writer.raw(RELAY_RECORD_MAGIC);
        writer.u8(RELAY_RECORD_VERSION);
        writer.raw(self.space.as_bytes());
        writer.raw(self.writer.as_bytes());
        writer.u64(self.seq);
        writer.u32(self.epoch);
        let mut bytes = [0u8; RELAY_RECORD_HEADER_BYTES];
        bytes.copy_from_slice(writer.as_bytes());
        bytes
    }

    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, RelayError> {
        if reader.take(RELAY_RECORD_MAGIC.len())? != RELAY_RECORD_MAGIC {
            return Err(CodecError::Malformed("magic").into());
        }
        read_version(reader, RELAY_RECORD_VERSION)?;
        let space = SpaceId::from_bytes(reader.array()?);
        let writer = WriterId::from_bytes(reader.array()?);
        let seq = reader.u64()?;
        if seq == 0 {
            return Err(CodecError::Malformed("sequence").into());
        }
        let epoch = reader.u32()?;
        if epoch == 0 {
            return Err(CodecError::Malformed("epoch").into());
        }
        Ok(Self {
            space,
            writer,
            seq,
            epoch,
        })
    }
}

/// The AAD of a relay record: the record domain, a zero byte, and the clear header.
pub fn relay_record_aad(header: &RelayRecordHeader) -> Vec<u8> {
    let mut aad = Vec::with_capacity(RELAY_RECORD_DOMAIN.len() + 1 + RELAY_RECORD_HEADER_BYTES);
    aad.extend_from_slice(RELAY_RECORD_DOMAIN);
    aad.push(0);
    aad.extend_from_slice(&header.encode());
    aad
}

/// The plaintext of an opened relay record.
pub struct OpenedRecord {
    /// The stored operation envelope, checked against the clear header but not verified.
    pub operation: Vec<u8>,
    /// The writer's signed certificate, whose device signature verified.
    pub certificate: SignedCertificate,
}

impl fmt::Debug for OpenedRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenedRecord")
            .field("operation_bytes", &self.operation.len())
            .finish_non_exhaustive()
    }
}

/// One sealed operation as a relay stores it.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayRecord {
    header: RelayRecordHeader,
    nonce: [u8; RELAY_NONCE_BYTES],
    ciphertext: Vec<u8>,
}

impl RelayRecord {
    /// Seals a stored operation and its writer's certificate under `key`, with a fresh nonce.
    pub fn seal(
        key: &SpaceKey,
        operation: &[u8],
        certificate: &SignedCertificate,
    ) -> Result<Self, RelayError> {
        Self::seal_with_nonce(key, operation, certificate, random_nonce()?)
    }

    pub(crate) fn seal_with_nonce(
        key: &SpaceKey,
        operation: &[u8],
        certificate: &SignedCertificate,
        nonce: [u8; RELAY_NONCE_BYTES],
    ) -> Result<Self, RelayError> {
        let envelope = Envelope::decode(operation)?;
        if certificate.certificate.writer_id() != envelope.writer {
            return Err(RelayError::Mismatch("certificate writer"));
        }
        let header = RelayRecordHeader {
            space: envelope.space,
            writer: envelope.writer,
            seq: envelope.seq,
            epoch: key.epoch(),
        };
        let certificate = certificate.encode();
        let mut plaintext = Writer::with_capacity(8 + operation.len() + certificate.len());
        plaintext.bytes_u32(operation);
        plaintext.bytes_u32(&certificate);
        Self::seal_plaintext(key, header, plaintext.as_bytes(), nonce)
    }

    /// Seals `plaintext` under `header` without checking it against the header, so tests can
    /// build records whose contents disagree with their slot.
    pub(crate) fn seal_plaintext(
        key: &SpaceKey,
        header: RelayRecordHeader,
        plaintext: &[u8],
        nonce: [u8; RELAY_NONCE_BYTES],
    ) -> Result<Self, RelayError> {
        check_epoch(header.epoch, key)?;
        if plaintext.len() > MAX_RELAY_RECORD_PLAINTEXT_BYTES {
            return Err(CodecError::Bounds("relay record").into());
        }
        let ciphertext = seal_payload(
            key,
            Subkey::RelayRecord,
            &header.space,
            &relay_record_aad(&header),
            plaintext,
            nonce,
        )?;
        Ok(Self {
            header,
            nonce,
            ciphertext,
        })
    }

    /// The clear header.
    pub fn header(&self) -> &RelayRecordHeader {
        &self.header
    }

    /// The encoding: header, nonce, then ciphertext and tag.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            RELAY_RECORD_HEADER_BYTES + RELAY_NONCE_BYTES + self.ciphertext.len(),
        );
        bytes.extend_from_slice(&self.header.encode());
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Parses an encoding without a key, so a relay can store it under its slot.
    pub fn decode(bytes: &[u8]) -> Result<Self, RelayError> {
        if bytes.len() > MAX_RELAY_RECORD_BYTES {
            return Err(CodecError::Bounds("relay record").into());
        }
        let mut reader = Reader::new(bytes);
        let header = RelayRecordHeader::decode_from(&mut reader)?;
        let nonce = reader.array()?;
        // The smallest plaintext is two empty length prefixes.
        if reader.remaining() < 8 + TAG_BYTES {
            return Err(CodecError::Truncated.into());
        }
        let ciphertext = reader.take(reader.remaining())?.to_vec();
        reader.finish()?;
        Ok(Self {
            header,
            nonce,
            ciphertext,
        })
    }

    /// Opens the record with the key of its stated epoch, then checks that the operation and
    /// certificate inside belong to the clear slot.
    pub fn open(&self, key: &SpaceKey) -> Result<OpenedRecord, RelayError> {
        check_epoch(self.header.epoch, key)?;
        let plaintext = open_payload(
            key,
            Subkey::RelayRecord,
            &self.header.space,
            &relay_record_aad(&self.header),
            &self.nonce,
            &self.ciphertext,
        )?;
        let mut reader = Reader::new(&plaintext);
        let operation = reader.bytes_u32(MAX_OPERATION_BYTES, "operation")?;
        let certificate = reader.bytes_u32(MAX_CERTIFICATE_BYTES, "certificate")?;
        reader.finish()?;
        let envelope = Envelope::decode(operation)?;
        if envelope.space != self.header.space {
            return Err(RelayError::Mismatch("space"));
        }
        if envelope.writer != self.header.writer {
            return Err(RelayError::Mismatch("writer"));
        }
        if envelope.seq != self.header.seq {
            return Err(RelayError::Mismatch("sequence"));
        }
        let certificate =
            SignedCertificate::decode(certificate).map_err(RelayError::Certificate)?;
        if certificate.certificate.writer_id() != self.header.writer {
            return Err(RelayError::Mismatch("certificate writer"));
        }
        Ok(OpenedRecord {
            operation: operation.to_vec(),
            certificate,
        })
    }
}

impl fmt::Debug for RelayRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayRecord")
            .field("header", &self.header)
            .field("ciphertext_bytes", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

/// The id of an asset on a relay: `HMAC-SHA256(asset-id key, plaintext SHA-256)`, so the relay
/// cannot match a known file and two spaces holding the same file get unrelated ids.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelayAssetId([u8; RELAY_ASSET_ID_BYTES]);

fn asset_id_key(space: &SpaceId, first: &SpaceKey) -> Result<hmac::Key, RelayError> {
    // Asset ids come from epoch 1 only, so they stay stable across rotations.
    if first.epoch() != 1 {
        return Err(RelayError::Crypto(CryptoError::Epoch));
    }
    let subkey = first.subkey(Subkey::RelayAssetId, space);
    Ok(hmac::Key::new(hmac::HMAC_SHA256, subkey.as_ref()))
}

impl RelayAssetId {
    /// Derives the id of the asset whose plaintext has `sha256`, from the epoch 1 key `first`.
    pub fn derive(
        space: &SpaceId,
        first: &SpaceKey,
        sha256: &[u8; 32],
    ) -> Result<Self, RelayError> {
        let tag = hmac::sign(&asset_id_key(space, first)?, sha256);
        let mut id = [0u8; RELAY_ASSET_ID_BYTES];
        id.copy_from_slice(tag.as_ref());
        Ok(Self(id))
    }

    /// Wraps id bytes.
    pub const fn from_bytes(bytes: [u8; RELAY_ASSET_ID_BYTES]) -> Self {
        Self(bytes)
    }

    /// The id bytes.
    pub const fn as_bytes(&self) -> &[u8; RELAY_ASSET_ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for RelayAssetId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RelayAssetId({})", hex(&self.0))
    }
}

/// The number of chunks an asset of `length` bytes seals into, or `None` when the length is
/// empty or above [`MAX_RELAY_ASSET_BYTES`].
pub fn relay_asset_chunk_count(length: usize) -> Option<u32> {
    if length == 0 || length > MAX_RELAY_ASSET_BYTES {
        return None;
    }
    u32::try_from(length.div_ceil(RELAY_ASSET_CHUNK_BYTES)).ok()
}

/// The clear header of a relay asset chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayAssetHeader {
    /// Space id.
    pub space: SpaceId,
    /// Asset id.
    pub asset: RelayAssetId,
    /// Key epoch the chunk is sealed under, from 1.
    pub epoch: u32,
    /// Chunk index, from 0.
    pub index: u32,
    /// Chunks of the asset.
    pub count: u32,
}

impl RelayAssetHeader {
    /// The header encoding, every byte before the nonce, which is also the chunk AAD.
    pub fn encode(&self) -> [u8; RELAY_ASSET_HEADER_BYTES] {
        let mut writer = Writer::with_capacity(RELAY_ASSET_HEADER_BYTES);
        writer.raw(RELAY_ASSET_MAGIC);
        writer.u8(RELAY_ASSET_VERSION);
        writer.raw(self.space.as_bytes());
        writer.raw(self.asset.as_bytes());
        writer.u32(self.epoch);
        writer.u32(self.index);
        writer.u32(self.count);
        let mut bytes = [0u8; RELAY_ASSET_HEADER_BYTES];
        bytes.copy_from_slice(writer.as_bytes());
        bytes
    }

    fn validate(&self) -> Result<(), RelayError> {
        if self.epoch == 0 {
            return Err(CodecError::Malformed("epoch").into());
        }
        if self.count == 0 || self.count as usize > MAX_RELAY_ASSET_CHUNKS {
            return Err(CodecError::Bounds("chunk count").into());
        }
        if self.index >= self.count {
            return Err(CodecError::Malformed("chunk index").into());
        }
        Ok(())
    }

    fn is_last(&self) -> bool {
        self.index + 1 == self.count
    }

    fn check_length(&self, length: usize) -> Result<(), RelayError> {
        let valid = if self.is_last() {
            (1..=RELAY_ASSET_CHUNK_BYTES).contains(&length)
        } else {
            length == RELAY_ASSET_CHUNK_BYTES
        };
        if valid {
            Ok(())
        } else {
            Err(CodecError::Bounds("relay asset chunk").into())
        }
    }

    fn decode_from(reader: &mut Reader<'_>) -> Result<Self, RelayError> {
        if reader.take(RELAY_ASSET_MAGIC.len())? != RELAY_ASSET_MAGIC {
            return Err(CodecError::Malformed("magic").into());
        }
        read_version(reader, RELAY_ASSET_VERSION)?;
        let header = Self {
            space: SpaceId::from_bytes(reader.array()?),
            asset: RelayAssetId(reader.array()?),
            epoch: reader.u32()?,
            index: reader.u32()?,
            count: reader.u32()?,
        };
        header.validate()?;
        Ok(header)
    }
}

/// One sealed chunk of an asset as a relay stores it.
#[derive(Clone, PartialEq, Eq)]
pub struct RelayAssetChunk {
    header: RelayAssetHeader,
    nonce: [u8; RELAY_NONCE_BYTES],
    ciphertext: Vec<u8>,
}

impl RelayAssetChunk {
    /// Seals one chunk of plaintext under `key`, with a fresh nonce. Every chunk but the last
    /// holds exactly [`RELAY_ASSET_CHUNK_BYTES`]; the last holds at least one byte.
    pub fn seal(
        key: &SpaceKey,
        space: &SpaceId,
        asset: RelayAssetId,
        index: u32,
        count: u32,
        plaintext: &[u8],
    ) -> Result<Self, RelayError> {
        Self::seal_with_nonce(key, space, asset, index, count, plaintext, random_nonce()?)
    }

    pub(crate) fn seal_with_nonce(
        key: &SpaceKey,
        space: &SpaceId,
        asset: RelayAssetId,
        index: u32,
        count: u32,
        plaintext: &[u8],
        nonce: [u8; RELAY_NONCE_BYTES],
    ) -> Result<Self, RelayError> {
        let header = RelayAssetHeader {
            space: *space,
            asset,
            epoch: key.epoch(),
            index,
            count,
        };
        header.validate()?;
        header.check_length(plaintext.len())?;
        let ciphertext = seal_payload(
            key,
            Subkey::RelayAsset,
            space,
            &header.encode(),
            plaintext,
            nonce,
        )?;
        Ok(Self {
            header,
            nonce,
            ciphertext,
        })
    }

    /// The clear header.
    pub fn header(&self) -> &RelayAssetHeader {
        &self.header
    }

    /// The encoding: header, nonce, then ciphertext and tag.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(
            RELAY_ASSET_HEADER_BYTES + RELAY_NONCE_BYTES + self.ciphertext.len(),
        );
        bytes.extend_from_slice(&self.header.encode());
        bytes.extend_from_slice(&self.nonce);
        bytes.extend_from_slice(&self.ciphertext);
        bytes
    }

    /// Parses an encoding without a key.
    pub fn decode(bytes: &[u8]) -> Result<Self, RelayError> {
        if bytes.len() > MAX_RELAY_ASSET_CHUNK_BYTES {
            return Err(CodecError::Bounds("relay asset chunk").into());
        }
        let mut reader = Reader::new(bytes);
        let header = RelayAssetHeader::decode_from(&mut reader)?;
        let nonce = reader.array()?;
        if reader.remaining() < 1 + TAG_BYTES {
            return Err(CodecError::Truncated.into());
        }
        let ciphertext = reader.take(reader.remaining())?.to_vec();
        reader.finish()?;
        Ok(Self {
            header,
            nonce,
            ciphertext,
        })
    }

    /// Opens the chunk with the key of its stated epoch.
    pub fn open(&self, key: &SpaceKey) -> Result<Vec<u8>, RelayError> {
        check_epoch(self.header.epoch, key)?;
        let plaintext = open_payload(
            key,
            Subkey::RelayAsset,
            &self.header.space,
            &self.header.encode(),
            &self.nonce,
            &self.ciphertext,
        )?;
        self.header.check_length(plaintext.len())?;
        Ok(plaintext)
    }
}

impl fmt::Debug for RelayAssetChunk {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayAssetChunk")
            .field("header", &self.header)
            .field("ciphertext_bytes", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

/// Seals a whole asset into its chunks under `key`, with its id derived from the epoch 1 key
/// `first`.
pub fn seal_relay_asset(
    space: &SpaceId,
    key: &SpaceKey,
    first: &SpaceKey,
    plaintext: &[u8],
) -> Result<(RelayAssetId, Vec<RelayAssetChunk>), RelayError> {
    let count =
        relay_asset_chunk_count(plaintext.len()).ok_or(CodecError::Bounds("relay asset"))?;
    let digest = ring::digest::digest(&SHA256, plaintext);
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(digest.as_ref());
    let asset = RelayAssetId::derive(space, first, &sha256)?;
    let chunks = plaintext
        .chunks(RELAY_ASSET_CHUNK_BYTES)
        .zip(0u32..)
        .map(|(chunk, index)| RelayAssetChunk::seal(key, space, asset, index, count, chunk))
        .collect::<Result<_, _>>()?;
    Ok((asset, chunks))
}

/// Reassembles one asset from its chunks in order, for the asset an operation references by its
/// plaintext SHA-256.
///
/// The opener refuses a chunk of another space or asset, out of order, or with another count,
/// and refuses every chunk after one it refused. [`RelayAssetOpener::finish`] refuses a set that
/// ended early and a plaintext whose digest is not the referenced one, so a caller streaming to
/// a temporary file renames it only after `finish` succeeds.
pub struct RelayAssetOpener {
    space: SpaceId,
    id_key: hmac::Key,
    sha256: [u8; 32],
    count: Option<u32>,
    next: u32,
    received: usize,
    digest: Context,
    failed: bool,
}

impl RelayAssetOpener {
    /// Starts reassembling the asset whose plaintext has `sha256`, with the epoch 1 key `first`.
    pub fn new(space: &SpaceId, first: &SpaceKey, sha256: [u8; 32]) -> Result<Self, RelayError> {
        Ok(Self {
            space: *space,
            id_key: asset_id_key(space, first)?,
            sha256,
            count: None,
            next: 0,
            received: 0,
            digest: Context::new(&SHA256),
            failed: false,
        })
    }

    /// Opens the next chunk with the key of its stated epoch and returns its plaintext.
    pub fn open_chunk(
        &mut self,
        chunk: &RelayAssetChunk,
        key: &SpaceKey,
    ) -> Result<Vec<u8>, RelayError> {
        if self.failed {
            return Err(RelayError::Order);
        }
        let result = self.accept(chunk, key);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn accept(&mut self, chunk: &RelayAssetChunk, key: &SpaceKey) -> Result<Vec<u8>, RelayError> {
        let header = chunk.header();
        if header.space != self.space {
            return Err(RelayError::Mismatch("space"));
        }
        // The asset id is a keyed digest, so it is compared in constant time.
        hmac::verify(&self.id_key, &self.sha256, header.asset.as_bytes())
            .map_err(|_| RelayError::Mismatch("asset id"))?;
        if self.count.is_some_and(|count| count != header.count) {
            return Err(RelayError::Mismatch("chunk count"));
        }
        if header.index != self.next {
            return Err(RelayError::Order);
        }
        let plaintext = chunk.open(key)?;
        // The count is kept only once a chunk authenticated under it.
        self.count.get_or_insert(header.count);
        self.received += plaintext.len();
        if self.received > MAX_RELAY_ASSET_BYTES {
            return Err(CodecError::Bounds("relay asset").into());
        }
        self.digest.update(&plaintext);
        self.next += 1;
        Ok(plaintext)
    }

    /// Finishes the asset: every chunk arrived and the plaintext has the referenced digest.
    pub fn finish(self) -> Result<(), RelayError> {
        if self.failed {
            return Err(RelayError::Order);
        }
        if self.count != Some(self.next) {
            return Err(RelayError::Incomplete);
        }
        // The referenced digest is public, so a plain comparison suffices.
        if self.digest.finish().as_ref() != self.sha256 {
            return Err(RelayError::Digest);
        }
        Ok(())
    }
}

impl fmt::Debug for RelayAssetOpener {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RelayAssetOpener")
            .field("count", &self.count)
            .field("next", &self.next)
            .finish_non_exhaustive()
    }
}

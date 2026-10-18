//! Wire shapes and bounds of the sync exchange between a linked device and the hub.
//!
//! Operations travel as base64url envelopes; the engine authenticates them, so validation here
//! only bounds sizes and text forms.

use super::is_base64url;
use serde::{Deserialize, Serialize};

/// Raw operation bytes in one push or pull page. Its base64url form plus JSON framing stays
/// below the control frame limit, and a single operation of the largest size always fits.
pub const SYNC_PAGE_BYTES: usize = 600 * 1024;
/// Entries of a version vector on the wire.
const MAX_SYNC_WRITERS: usize = ganbaru_sync_contracts::bounds::MAX_WRITERS_PER_SPACE;
/// Operations in one page; operations are never smaller than their signed header.
const MAX_SYNC_PAGE_OPS: usize = 8_192;
/// Hashes in one hash list.
pub const MAX_SYNC_HASHES: u32 = 1_024;
/// Base64url length of the largest operation.
const MAX_OPERATION_TEXT_BYTES: usize =
    (ganbaru_sync_contracts::bounds::MAX_OPERATION_BYTES * 4).div_ceil(3);
/// Hex text of a writer id.
const WRITER_TEXT_LENGTH: usize = 32;
/// Hex text of an operation hash.
const HASH_TEXT_LENGTH: usize = 64;

/// One version vector entry: the highest sequence of a writer.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSeq {
    pub writer: String,
    pub seq: u64,
}

/// An operation of the sender's own writer, which the receiver compares with its copy to find
/// a fork that no push would reveal.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProbe {
    pub writer: String,
    pub seq: u64,
    pub hash: String,
}

/// Why the hub stopped storing a pushed page.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncRefusalCode {
    /// Earlier operations of the writer are missing.
    Gap,
    /// The hub holds a different operation of the writer's chain.
    Fork,
    /// The writer was revoked and the operation is past its cutoff.
    Revoked,
    /// The operation is malformed, badly signed, or not certified for the vault.
    Invalid,
    /// The writer is unknown, or its genesis names another device.
    UnknownWriter,
    /// The writer's genesis uses a newer format than the receiver reads.
    NewerFormat,
    /// The writer's genesis would exceed the writers a device or the vault may have.
    WriterLimit,
}

/// A refused push: the code and, when it concerns one writer, where its chain stopped.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRefusal {
    pub code: SyncRefusalCode,
    pub writer: Option<String>,
    pub seq: Option<u64>,
}

pub(super) fn validate_vector(vector: &[SyncSeq]) -> Result<(), String> {
    if vector.len() > MAX_SYNC_WRITERS {
        return Err("sync version vector is too large".to_string());
    }
    for entry in vector {
        validate_writer(&entry.writer)?;
        validate_seq(entry.seq)?;
    }
    Ok(())
}

pub(super) fn validate_probe(probe: &SyncProbe) -> Result<(), String> {
    validate_writer(&probe.writer)?;
    validate_seq(probe.seq)?;
    validate_hash(&probe.hash)
}

pub(super) fn validate_ops(ops: &[String]) -> Result<(), String> {
    if ops.len() > MAX_SYNC_PAGE_OPS {
        return Err("sync page holds too many operations".to_string());
    }
    for op in ops {
        if op.is_empty() || op.len() > MAX_OPERATION_TEXT_BYTES || !is_base64url(op) {
            return Err("sync operation text is invalid".to_string());
        }
    }
    Ok(())
}

pub(super) fn validate_hashes(hashes: &[String]) -> Result<(), String> {
    if hashes.len() > MAX_SYNC_HASHES as usize {
        return Err("sync hash list is too large".to_string());
    }
    hashes.iter().try_for_each(|hash| validate_hash(hash))
}

pub(super) fn validate_refusal(refusal: &SyncRefusal) -> Result<(), String> {
    if let Some(writer) = &refusal.writer {
        validate_writer(writer)?;
    }
    if let Some(seq) = refusal.seq {
        validate_seq(seq)?;
    }
    Ok(())
}

pub(super) fn validate_page_bytes(max_bytes: u32) -> Result<(), String> {
    if max_bytes == 0 || max_bytes as usize > SYNC_PAGE_BYTES {
        return Err("sync page size is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_hash_range(from_seq: u64, limit: u32) -> Result<(), String> {
    validate_seq(from_seq)?;
    if limit == 0 || limit > MAX_SYNC_HASHES {
        return Err("sync hash range is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_writer(writer: &str) -> Result<(), String> {
    if writer.len() != WRITER_TEXT_LENGTH || !is_lower_hex(writer) {
        return Err("sync writer id is invalid".to_string());
    }
    Ok(())
}

fn validate_hash(hash: &str) -> Result<(), String> {
    if hash.len() != HASH_TEXT_LENGTH || !is_lower_hex(hash) {
        return Err("sync operation hash is invalid".to_string());
    }
    Ok(())
}

/// Sequences start at 1 and are stored as SQLite integers.
fn validate_seq(seq: u64) -> Result<(), String> {
    if seq == 0 || i64::try_from(seq).is_err() {
        return Err("sync sequence is invalid".to_string());
    }
    Ok(())
}

fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Raw bytes of one asset chunk. Its base64url form plus JSON framing stays below the control
/// frame limit.
pub const SYNC_ASSET_CHUNK_BYTES: usize = SYNC_PAGE_BYTES;
/// Asset references in one presence query.
pub const MAX_SYNC_ASSETS: usize = 512;
/// Largest asset the exchange moves; each domain enforces its own smaller limit.
pub const MAX_SYNC_ASSET_BYTES: u32 = 32 * 1024 * 1024;
/// Base64url length of the largest chunk.
const MAX_ASSET_CHUNK_TEXT_BYTES: usize = (SYNC_ASSET_CHUNK_BYTES * 4).div_ceil(3);
/// Length of a media type such as `image/png`.
const MAX_CONTENT_TYPE_BYTES: usize = 64;

/// A content-addressed file that sync moves between devices: the SHA-256 of its bytes, its
/// media type, and its exact size.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncAsset {
    pub sha256: String,
    pub content_type: String,
    pub byte_size: u32,
}

pub(super) fn validate_assets(assets: &[SyncAsset]) -> Result<(), String> {
    if assets.len() > MAX_SYNC_ASSETS {
        return Err("sync asset list is too large".to_string());
    }
    assets.iter().try_for_each(validate_asset)
}

pub(super) fn validate_asset(asset: &SyncAsset) -> Result<(), String> {
    validate_asset_digest(&asset.sha256)?;
    let content_type = &asset.content_type;
    let well_formed = content_type.len() <= MAX_CONTENT_TYPE_BYTES
        && content_type.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && content_type.bytes().filter(|byte| *byte == b'/').count() == 1
        })
        && content_type.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'/' | b'.' | b'+' | b'-')
        });
    if !well_formed {
        return Err("sync asset media type is invalid".to_string());
    }
    if asset.byte_size == 0 || asset.byte_size > MAX_SYNC_ASSET_BYTES {
        return Err("sync asset size is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_asset_digests(digests: &[String]) -> Result<(), String> {
    if digests.len() > MAX_SYNC_ASSETS {
        return Err("sync asset list is too large".to_string());
    }
    digests
        .iter()
        .try_for_each(|digest| validate_asset_digest(digest))
}

/// A chunk at `offset` of `asset`: the offset lies inside the asset, and the text is a
/// non-empty base64url chunk that ends within it.
pub(super) fn validate_asset_chunk(
    asset: &SyncAsset,
    offset: u32,
    data: &str,
) -> Result<(), String> {
    validate_asset(asset)?;
    validate_asset_offset(asset, offset)?;
    validate_asset_data(data)?;
    // Unpadded base64url carries at least `len * 3 / 4` bytes.
    let bytes = data.len() * 3 / 4;
    if u64::from(offset) + bytes as u64 > u64::from(asset.byte_size) {
        return Err("sync asset chunk ends past the asset".to_string());
    }
    Ok(())
}

pub(super) fn validate_asset_offset(asset: &SyncAsset, offset: u32) -> Result<(), String> {
    if offset >= asset.byte_size {
        return Err("sync asset offset is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_asset_data(data: &str) -> Result<(), String> {
    if data.is_empty() || data.len() > MAX_ASSET_CHUNK_TEXT_BYTES || !is_base64url(data) {
        return Err("sync asset chunk is invalid".to_string());
    }
    Ok(())
}

pub(super) fn validate_asset_received(received: u32) -> Result<(), String> {
    if received > MAX_SYNC_ASSET_BYTES {
        return Err("sync asset progress is invalid".to_string());
    }
    Ok(())
}

fn validate_asset_digest(digest: &str) -> Result<(), String> {
    if digest.len() != HASH_TEXT_LENGTH || !is_lower_hex(digest) {
        return Err("sync asset digest is invalid".to_string());
    }
    Ok(())
}

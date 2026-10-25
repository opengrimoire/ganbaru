//! Conversions between engine values and their sync wire forms.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_handoff::protocol::{ControlMessage, SyncAsset, SyncAssetKey, SyncSeq};
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync_contracts::{Digest32, ProposalRejection, Seq, VersionVector, WriterId};

/// Error code of a pull the hub can no longer answer because it compacted operations the peer
/// lacks. The peer stops pulling until it bootstraps from a fresh copy of the vault.
pub const BOOTSTRAP_REQUIRED: &str = "bootstrap_required";

/// Wire entries of a version vector, in writer order.
pub fn vector_to_wire(vector: &VersionVector) -> Vec<SyncSeq> {
    vector
        .iter()
        .map(|(writer, seq)| SyncSeq {
            writer: writer.to_hex(),
            seq,
        })
        .collect()
}

/// Reads a wire vector. Protocol validation already checked the text forms.
pub fn vector_from_wire(entries: &[SyncSeq]) -> Result<VersionVector, String> {
    let mut vector = VersionVector::new();
    for entry in entries {
        vector.advance(writer_from_wire(&entry.writer)?, entry.seq);
    }
    Ok(vector)
}

/// Wire entries of forked writers with their fork sequences.
#[cfg(desktop)]
pub fn forked_to_wire(forked: &[(WriterId, Seq)]) -> Vec<SyncSeq> {
    forked
        .iter()
        .map(|(writer, seq)| SyncSeq {
            writer: writer.to_hex(),
            seq: *seq,
        })
        .collect()
}

/// Reads wire forked writers. Protocol validation already checked the text forms.
pub fn forked_from_wire(entries: &[SyncSeq]) -> Result<Vec<(WriterId, Seq)>, String> {
    entries
        .iter()
        .map(|entry| Ok((writer_from_wire(&entry.writer)?, entry.seq)))
        .collect()
}

pub fn writer_from_wire(text: &str) -> Result<WriterId, String> {
    WriterId::from_hex(text).ok_or_else(|| "sync writer id is invalid".to_string())
}

pub fn hash_to_wire(hash: &Digest32) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn hash_from_wire(text: &str) -> Result<Digest32, String> {
    let mut hash = [0u8; 32];
    if text.len() != hash.len() * 2 {
        return Err("sync operation hash is invalid".to_string());
    }
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .map_err(|_| "sync operation hash is invalid".to_string())?;
    }
    Ok(hash)
}

pub fn ops_to_wire(ops: &[Vec<u8>]) -> Vec<String> {
    ops.iter().map(|op| URL_SAFE_NO_PAD.encode(op)).collect()
}

pub fn ops_from_wire(ops: &[String]) -> Result<Vec<Vec<u8>>, String> {
    ops.iter()
        .map(|op| {
            URL_SAFE_NO_PAD
                .decode(op)
                .map_err(|_| "sync operation text is invalid".to_string())
        })
        .collect()
}

pub fn asset_to_wire(asset: &AssetRef) -> SyncAsset {
    SyncAsset {
        class: asset.class().name.to_string(),
        sha256: asset.sha256().to_string(),
        content_type: asset.content_type().to_string(),
        byte_size: asset.byte_size(),
    }
}

/// The class and digest that identify an asset on the wire.
pub fn asset_key(asset: &AssetRef) -> SyncAssetKey {
    SyncAssetKey {
        class: asset.class().name.to_string(),
        sha256: asset.sha256().to_string(),
    }
}

/// Reads a wire asset against the classes the manifest registers, which bound its size and own
/// its path.
#[cfg(desktop)]
pub fn asset_from_wire(
    asset: &SyncAsset,
    manifest: &ganbaru_sync::manifest::Manifest,
) -> Result<AssetRef, String> {
    let (_, class) = manifest
        .asset_class(&asset.class)
        .ok_or_else(|| format!("sync asset class {} is not registered", asset.class))?;
    AssetRef::new(
        class,
        asset.sha256.clone(),
        asset.content_type.clone(),
        asset.byte_size,
    )
    .map_err(|error| format!("sync asset is invalid: {error}"))
}

pub fn bytes_to_wire(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn bytes_from_wire(text: &str) -> Result<Vec<u8>, String> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| "sync asset chunk is invalid".to_string())
}

/// The answer to a proposal: the revision that holds it, or the wire value of its rejection.
pub fn proposed_to_wire(outcome: Result<u64, ProposalRejection>) -> ControlMessage {
    match outcome {
        Ok(revision) => ControlMessage::SyncProposed {
            revision: Some(revision),
            rejection: None,
        },
        Err(rejection) => ControlMessage::SyncProposed {
            revision: None,
            rejection: Some(rejection.as_u8()),
        },
    }
}

/// The outcome an answer to a proposal names.
pub fn proposed_from_wire(
    revision: Option<u64>,
    rejection: Option<u8>,
) -> Result<Result<u64, ProposalRejection>, String> {
    match (revision, rejection.map(ProposalRejection::from_u8)) {
        (Some(revision), None) => Ok(Ok(revision)),
        (None, Some(Some(rejection))) => Ok(Err(rejection)),
        _ => Err("the hub answered a proposal without one outcome".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_round_trip_through_lowercase_hex() {
        let mut hash = [0u8; 32];
        hash[0] = 0xab;
        hash[31] = 0x01;
        let text = hash_to_wire(&hash);
        assert_eq!(text.len(), 64);
        assert!(text.starts_with("ab") && text.ends_with("01"));
        assert_eq!(hash_from_wire(&text).unwrap(), hash);
        assert!(hash_from_wire(&text[2..]).is_err());
        assert!(hash_from_wire(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn operations_round_trip_through_base64url() {
        let ops = vec![vec![0u8, 255, 62, 63], vec![1u8; 70]];
        let text = ops_to_wire(&ops);
        assert!(text.iter().all(|op| !op.contains('=') && !op.contains('+')));
        assert_eq!(ops_from_wire(&text).unwrap(), ops);
        assert!(ops_from_wire(&["not base64!".to_string()]).is_err());
    }
}

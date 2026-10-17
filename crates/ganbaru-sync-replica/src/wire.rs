//! Conversions between engine values and the sync wire forms of protocol 5.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_handoff::protocol::SyncSeq;
use ganbaru_sync_contracts::{Digest32, VersionVector, WriterId};

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

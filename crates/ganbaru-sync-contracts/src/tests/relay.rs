//! Relay records and relay asset chunks: frozen encodings, round trips, header authentication,
//! slot checks, epochs, bounds, and chunk set reassembly.

use super::fixtures::{
    certificate_for, golden_chain, other_writer_id, other_writer_key, writer_key,
};
use super::golden::assert_vector;
use crate::bounds::{
    MAX_RELAY_ASSET_BYTES, MAX_RELAY_ASSET_CHUNKS, MAX_RELAY_RECORD_PLAINTEXT_BYTES,
    RELAY_ASSET_CHUNK_BYTES,
};
use crate::codec::{CodecError, Writer};
use crate::crypto::{CryptoError, SPACE_KEY_BYTES, SpaceKey};
use crate::ids::{SpaceId, hex};
use crate::relay::{
    MAX_RELAY_RECORD_BYTES, RELAY_ASSET_HEADER_BYTES, RELAY_NONCE_BYTES, RELAY_RECORD_DOMAIN,
    RELAY_RECORD_HEADER_BYTES, RelayAssetChunk, RelayAssetId, RelayAssetOpener, RelayError,
    RelayRecord, RelayRecordHeader, relay_asset_chunk_count, relay_record_aad, seal_relay_asset,
};
use crate::{Envelope, SignedCertificate};
use ring::digest::{SHA256, digest};
use zeroize::Zeroizing;

const RECORD_NONCE: [u8; RELAY_NONCE_BYTES] = [0x24; RELAY_NONCE_BYTES];
const CHUNK_NONCE: [u8; RELAY_NONCE_BYTES] = [0x25; RELAY_NONCE_BYTES];
const GOLDEN_ASSET: &[u8] = b"relay asset golden plaintext";

fn space_key(epoch: u32, byte: u8) -> SpaceKey {
    SpaceKey::from_bytes(epoch, Zeroizing::new([byte; SPACE_KEY_BYTES])).unwrap()
}

/// The golden changes operation, sequence 2 of the golden writer.
fn operation() -> Vec<u8> {
    golden_chain().swap_remove(1).1
}

fn certificate() -> SignedCertificate {
    certificate_for(&writer_key().public_key(), Some(other_writer_id()))
}

fn record(key: &SpaceKey) -> RelayRecord {
    RelayRecord::seal_with_nonce(key, &operation(), &certificate(), RECORD_NONCE).unwrap()
}

fn plaintext(operation: &[u8], certificate: &SignedCertificate) -> Vec<u8> {
    let certificate = certificate.encode();
    let mut writer = Writer::default();
    writer.bytes_u32(operation);
    writer.bytes_u32(&certificate);
    writer.into_bytes()
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    digest(&SHA256, bytes).as_ref().try_into().unwrap()
}

fn golden_chunk() -> RelayAssetChunk {
    let first = space_key(1, 0x41);
    let space = header_of(&operation()).space;
    let asset = RelayAssetId::derive(&space, &first, &sha256(GOLDEN_ASSET)).unwrap();
    RelayAssetChunk::seal_with_nonce(&first, &space, asset, 0, 1, GOLDEN_ASSET, CHUNK_NONCE)
        .unwrap()
}

fn header_of(operation: &[u8]) -> RelayRecordHeader {
    let envelope = Envelope::decode(operation).unwrap();
    RelayRecordHeader {
        space: envelope.space,
        writer: envelope.writer,
        seq: envelope.seq,
        epoch: 1,
    }
}

#[test]
fn relay_record_encodes_to_frozen_bytes_and_round_trips() {
    let key = space_key(1, 0x41);
    let record = record(&key);
    let bytes = record.encode();
    assert_vector("relay-record", &hex(&bytes));
    assert_eq!(&bytes[..4], b"GBSR");
    assert_eq!(bytes[4], 1);
    let decoded = RelayRecord::decode(&bytes).unwrap();
    assert_eq!(decoded, record);
    assert_eq!(*decoded.header(), header_of(&operation()));
    let opened = decoded.open(&key).unwrap();
    assert_eq!(opened.operation, operation());
    assert_eq!(opened.certificate, certificate());
    assert_eq!(
        bytes.len(),
        RELAY_RECORD_HEADER_BYTES
            + RELAY_NONCE_BYTES
            + plaintext(&operation(), &certificate()).len()
            + 16
    );
}

#[test]
fn relay_asset_chunk_encodes_to_frozen_bytes_and_round_trips() {
    let first = space_key(1, 0x41);
    let chunk = golden_chunk();
    let bytes = chunk.encode();
    assert_vector("relay-asset-chunk", &hex(&bytes));
    assert_eq!(&bytes[..4], b"GBSA");
    assert_eq!(bytes[4], 1);
    let decoded = RelayAssetChunk::decode(&bytes).unwrap();
    assert_eq!(decoded, chunk);
    let mut opener =
        RelayAssetOpener::new(&chunk.header().space, &first, sha256(GOLDEN_ASSET)).unwrap();
    assert_eq!(opener.open_chunk(&decoded, &first).unwrap(), GOLDEN_ASSET);
    opener.finish().unwrap();
}

#[test]
fn relay_record_aad_is_the_domain_a_zero_byte_and_the_clear_header() {
    let header = header_of(&operation());
    let aad = relay_record_aad(&header);
    assert_eq!(
        &aad[..RELAY_RECORD_DOMAIN.len()],
        b"ganbaru-sync/relay-record/v1"
    );
    assert_eq!(aad[RELAY_RECORD_DOMAIN.len()], 0);
    assert_eq!(&aad[RELAY_RECORD_DOMAIN.len() + 1..], header.encode());
    let encoded = record(&space_key(1, 0x41)).encode();
    assert_eq!(&encoded[..RELAY_RECORD_HEADER_BYTES], header.encode());
}

#[test]
fn production_seals_draw_a_fresh_nonce_each_time() {
    let key = space_key(1, 0x41);
    let first = RelayRecord::seal(&key, &operation(), &certificate()).unwrap();
    let second = RelayRecord::seal(&key, &operation(), &certificate()).unwrap();
    let (first, second) = (first.encode(), second.encode());
    let nonce = RELAY_RECORD_HEADER_BYTES..RELAY_RECORD_HEADER_BYTES + RELAY_NONCE_BYTES;
    assert_ne!(first[nonce.clone()], second[nonce]);
    assert_ne!(first, second);
    assert_ne!(first, record(&key).encode());
}

#[test]
fn every_mutated_relay_record_header_byte_is_refused() {
    let key = space_key(1, 0x41);
    let bytes = record(&key).encode();
    for position in 0..RELAY_RECORD_HEADER_BYTES + RELAY_NONCE_BYTES {
        let mut mutated = bytes.clone();
        mutated[position] ^= 0x01;
        let Ok(decoded) = RelayRecord::decode(&mutated) else {
            continue;
        };
        assert!(decoded.open(&key).is_err(), "byte {position} opened");
        // A key of the mutated epoch with the same bytes still fails authentication, because
        // the epoch is part of the AAD.
        let matching = space_key(decoded.header().epoch, 0x41);
        assert_eq!(
            decoded.open(&matching).unwrap_err(),
            RelayError::Authentication,
            "byte {position}"
        );
    }
    let mut tag = bytes.clone();
    *tag.last_mut().unwrap() ^= 0x01;
    assert_eq!(
        RelayRecord::decode(&tag).unwrap().open(&key).unwrap_err(),
        RelayError::Authentication
    );
}

#[test]
fn every_mutated_relay_asset_header_byte_is_refused() {
    let first = space_key(1, 0x41);
    let chunk = golden_chunk();
    let bytes = chunk.encode();
    for position in 0..RELAY_ASSET_HEADER_BYTES + RELAY_NONCE_BYTES {
        let mut mutated = bytes.clone();
        mutated[position] ^= 0x01;
        let Ok(decoded) = RelayAssetChunk::decode(&mutated) else {
            continue;
        };
        assert!(decoded.open(&first).is_err(), "byte {position} opened");
        let matching = space_key(decoded.header().epoch, 0x41);
        assert_eq!(
            decoded.open(&matching).unwrap_err(),
            RelayError::Authentication,
            "byte {position}"
        );
    }
}

#[test]
fn a_relay_record_whose_contents_disagree_with_its_slot_is_refused() {
    let key = space_key(1, 0x41);
    let inner = plaintext(&operation(), &certificate());
    let header = header_of(&operation());
    let cases = [
        (
            RelayRecordHeader {
                seq: header.seq + 1,
                ..header
            },
            "sequence",
        ),
        (
            RelayRecordHeader {
                writer: other_writer_id(),
                ..header
            },
            "writer",
        ),
        (
            RelayRecordHeader {
                space: SpaceId::from_bytes([0x5a; 16]),
                ..header
            },
            "space",
        ),
    ];
    for (slot, field) in cases {
        let record = RelayRecord::seal_plaintext(&key, slot, &inner, RECORD_NONCE).unwrap();
        let decoded = RelayRecord::decode(&record.encode()).unwrap();
        assert_eq!(decoded.open(&key).unwrap_err(), RelayError::Mismatch(field));
    }
    let other = certificate_for(&other_writer_key().public_key(), None);
    assert_eq!(
        RelayRecord::seal(&key, &operation(), &other).unwrap_err(),
        RelayError::Mismatch("certificate writer")
    );
    let forged = plaintext(&operation(), &other);
    let record = RelayRecord::seal_plaintext(&key, header, &forged, RECORD_NONCE).unwrap();
    assert_eq!(
        record.open(&key).unwrap_err(),
        RelayError::Mismatch("certificate writer")
    );
}

#[test]
fn a_relay_record_opens_only_with_the_key_of_its_epoch() {
    let second = space_key(2, 0x42);
    let record = record(&second);
    assert_eq!(record.header().epoch, 2);
    assert_eq!(
        record.open(&space_key(1, 0x42)).unwrap_err(),
        RelayError::WrongEpoch { stated: 2, key: 1 }
    );
    assert_eq!(
        record.open(&space_key(3, 0x42)).unwrap_err(),
        RelayError::WrongEpoch { stated: 2, key: 3 }
    );
    assert_eq!(
        record.open(&space_key(2, 0x41)).unwrap_err(),
        RelayError::Authentication
    );
    assert!(record.open(&second).is_ok());
    let header = RelayRecordHeader {
        epoch: 1,
        ..*record.header()
    };
    assert_eq!(
        RelayRecord::seal_plaintext(&second, header, b"", RECORD_NONCE).unwrap_err(),
        RelayError::WrongEpoch { stated: 1, key: 2 }
    );
}

#[test]
fn relay_record_bounds_and_versions_are_typed_rejections() {
    let key = space_key(1, 0x41);
    let bytes = record(&key).encode();
    let mut newer = bytes.clone();
    newer[4] = 2;
    assert_eq!(
        RelayRecord::decode(&newer).unwrap_err(),
        RelayError::Version(2)
    );
    let mut zero = bytes.clone();
    zero[4] = 0;
    assert_eq!(
        RelayRecord::decode(&zero).unwrap_err(),
        RelayError::Codec(CodecError::Malformed("format version"))
    );
    let mut magic = bytes.clone();
    magic[0] = b'X';
    assert_eq!(
        RelayRecord::decode(&magic).unwrap_err(),
        RelayError::Codec(CodecError::Malformed("magic"))
    );
    let mut sequence = bytes.clone();
    sequence[37..45].fill(0);
    assert_eq!(
        RelayRecord::decode(&sequence).unwrap_err(),
        RelayError::Codec(CodecError::Malformed("sequence"))
    );
    let mut epoch = bytes.clone();
    epoch[45..49].fill(0);
    assert_eq!(
        RelayRecord::decode(&epoch).unwrap_err(),
        RelayError::Codec(CodecError::Malformed("epoch"))
    );
    for length in [
        0,
        3,
        RELAY_RECORD_HEADER_BYTES,
        RELAY_RECORD_HEADER_BYTES + 30,
    ] {
        assert_eq!(
            RelayRecord::decode(&bytes[..length]).unwrap_err(),
            RelayError::Codec(CodecError::Truncated),
            "length {length}"
        );
    }
    let mut short = bytes.clone();
    short.truncate(bytes.len() - 1);
    assert_eq!(
        RelayRecord::decode(&short).unwrap().open(&key).unwrap_err(),
        RelayError::Authentication
    );
    let mut oversized = bytes.clone();
    oversized.resize(MAX_RELAY_RECORD_BYTES + 1, 0);
    assert_eq!(
        RelayRecord::decode(&oversized).unwrap_err(),
        RelayError::Codec(CodecError::Bounds("relay record"))
    );
    let header = header_of(&operation());
    let over_cap = vec![0u8; MAX_RELAY_RECORD_PLAINTEXT_BYTES + 1];
    assert_eq!(
        RelayRecord::seal_plaintext(&key, header, &over_cap, RECORD_NONCE).unwrap_err(),
        RelayError::Codec(CodecError::Bounds("relay record"))
    );
    // The largest plaintext seals, and its operation length prefix above the bound is refused
    // after decryption.
    let mut at_cap = vec![0u8; MAX_RELAY_RECORD_PLAINTEXT_BYTES];
    at_cap[..4].copy_from_slice(&u32::MAX.to_be_bytes());
    let largest = RelayRecord::seal_plaintext(&key, header, &at_cap, RECORD_NONCE).unwrap();
    let largest = largest.encode();
    assert_eq!(largest.len(), MAX_RELAY_RECORD_BYTES);
    assert_eq!(
        RelayRecord::decode(&largest)
            .unwrap()
            .open(&key)
            .unwrap_err(),
        RelayError::Codec(CodecError::Bounds("operation"))
    );
}

#[test]
fn a_relay_asset_reassembles_from_chunks_in_order() {
    let first = space_key(1, 0x41);
    let current = space_key(3, 0x43);
    let space = SpaceId::from_bytes([0x5a; 16]);
    let asset: Vec<u8> = (0..2 * RELAY_ASSET_CHUNK_BYTES + 17)
        .map(|index| (index % 251) as u8)
        .collect();
    let (id, chunks) = seal_relay_asset(&space, &current, &first, &asset).unwrap();
    assert_eq!(
        id,
        RelayAssetId::derive(&space, &first, &sha256(&asset)).unwrap()
    );
    assert_eq!(chunks.len(), 3);
    let mut opener = RelayAssetOpener::new(&space, &first, sha256(&asset)).unwrap();
    let mut reassembled = Vec::new();
    for chunk in &chunks {
        let decoded = RelayAssetChunk::decode(&chunk.encode()).unwrap();
        assert_eq!(decoded.header().epoch, 3);
        reassembled.extend(opener.open_chunk(&decoded, &current).unwrap());
    }
    opener.finish().unwrap();
    assert_eq!(reassembled, asset);
}

#[test]
fn truncated_reordered_repeated_and_moved_chunk_sets_are_refused() {
    let first = space_key(1, 0x41);
    let space = SpaceId::from_bytes([0x5a; 16]);
    let asset = vec![7u8; 2 * RELAY_ASSET_CHUNK_BYTES + 1];
    let other = vec![8u8; 2 * RELAY_ASSET_CHUNK_BYTES + 1];
    let (_, chunks) = seal_relay_asset(&space, &first, &first, &asset).unwrap();
    let (_, others) = seal_relay_asset(&space, &first, &first, &other).unwrap();
    let opener = || RelayAssetOpener::new(&space, &first, sha256(&asset)).unwrap();

    let mut truncated = opener();
    truncated.open_chunk(&chunks[0], &first).unwrap();
    truncated.open_chunk(&chunks[1], &first).unwrap();
    assert_eq!(truncated.finish().unwrap_err(), RelayError::Incomplete);
    assert_eq!(opener().finish().unwrap_err(), RelayError::Incomplete);

    let mut reordered = opener();
    assert_eq!(
        reordered.open_chunk(&chunks[1], &first).unwrap_err(),
        RelayError::Order
    );
    assert_eq!(
        reordered.open_chunk(&chunks[0], &first).unwrap_err(),
        RelayError::Order
    );
    assert_eq!(reordered.finish().unwrap_err(), RelayError::Order);

    let mut repeated = opener();
    repeated.open_chunk(&chunks[0], &first).unwrap();
    assert_eq!(
        repeated.open_chunk(&chunks[0], &first).unwrap_err(),
        RelayError::Order
    );

    let mut moved = opener();
    moved.open_chunk(&chunks[0], &first).unwrap();
    assert_eq!(
        moved.open_chunk(&others[1], &first).unwrap_err(),
        RelayError::Mismatch("asset id")
    );

    let mut other_space =
        RelayAssetOpener::new(&SpaceId::from_bytes([0x5b; 16]), &first, sha256(&asset)).unwrap();
    assert_eq!(
        other_space.open_chunk(&chunks[0], &first).unwrap_err(),
        RelayError::Mismatch("space")
    );

    // Chunks sealed under the right id with other plaintext fail the digest check.
    let id = *chunks[0].header().asset.as_bytes();
    let forged: Vec<_> = other
        .chunks(RELAY_ASSET_CHUNK_BYTES)
        .zip(0u32..)
        .map(|(part, index)| {
            RelayAssetChunk::seal(&first, &space, RelayAssetId::from_bytes(id), index, 3, part)
                .unwrap()
        })
        .collect();
    let mut digest = opener();
    for chunk in &forged {
        digest.open_chunk(chunk, &first).unwrap();
    }
    assert_eq!(digest.finish().unwrap_err(), RelayError::Digest);
}

#[test]
fn relay_asset_chunk_lengths_and_counts_are_bounded() {
    let first = space_key(1, 0x41);
    let space = SpaceId::from_bytes([0x5a; 16]);
    let id = RelayAssetId::from_bytes([0x11; 32]);
    let full = vec![0u8; RELAY_ASSET_CHUNK_BYTES];
    let seal = |index, count, plaintext: &[u8]| {
        RelayAssetChunk::seal(&first, &space, id, index, count, plaintext)
    };
    assert!(seal(0, 2, &full).is_ok());
    assert!(seal(1, 2, &[1]).is_ok());
    let bound = RelayError::Codec(CodecError::Bounds("relay asset chunk"));
    assert_eq!(seal(0, 2, &full[1..]).unwrap_err(), bound);
    assert_eq!(seal(1, 2, &[]).unwrap_err(), bound);
    let mut over = full.clone();
    over.push(0);
    assert_eq!(seal(1, 2, &over).unwrap_err(), bound);
    assert_eq!(
        seal(0, 0, &[1]).unwrap_err(),
        RelayError::Codec(CodecError::Bounds("chunk count"))
    );
    let too_many = u32::try_from(MAX_RELAY_ASSET_CHUNKS + 1).unwrap();
    assert_eq!(
        seal(too_many - 1, too_many, &[1]).unwrap_err(),
        RelayError::Codec(CodecError::Bounds("chunk count"))
    );
    assert_eq!(
        seal(2, 2, &[1]).unwrap_err(),
        RelayError::Codec(CodecError::Malformed("chunk index"))
    );

    let bytes = seal(1, 2, &[1]).unwrap().encode();
    let mut newer = bytes.clone();
    newer[4] = 2;
    assert_eq!(
        RelayAssetChunk::decode(&newer).unwrap_err(),
        RelayError::Version(2)
    );
    assert_eq!(
        RelayAssetChunk::decode(&bytes[..RELAY_ASSET_HEADER_BYTES + RELAY_NONCE_BYTES + 16])
            .unwrap_err(),
        RelayError::Codec(CodecError::Truncated)
    );
    let mut oversized = seal(0, 2, &full).unwrap().encode();
    oversized.push(0);
    assert_eq!(
        RelayAssetChunk::decode(&oversized).unwrap_err(),
        RelayError::Codec(CodecError::Bounds("relay asset chunk"))
    );

    assert_eq!(relay_asset_chunk_count(0), None);
    assert_eq!(relay_asset_chunk_count(1), Some(1));
    assert_eq!(relay_asset_chunk_count(RELAY_ASSET_CHUNK_BYTES), Some(1));
    assert_eq!(
        relay_asset_chunk_count(RELAY_ASSET_CHUNK_BYTES + 1),
        Some(2)
    );
    assert_eq!(
        relay_asset_chunk_count(MAX_RELAY_ASSET_BYTES),
        Some(u32::try_from(MAX_RELAY_ASSET_CHUNKS).unwrap())
    );
    assert_eq!(relay_asset_chunk_count(MAX_RELAY_ASSET_BYTES + 1), None);
    assert!(seal_relay_asset(&space, &first, &first, &[]).is_err());
}

#[test]
fn relay_asset_ids_are_keyed_per_space_and_taken_from_epoch_one() {
    let first = space_key(1, 0x41);
    let digest = sha256(GOLDEN_ASSET);
    let space_a = SpaceId::from_bytes([0x5a; 16]);
    let space_b = SpaceId::from_bytes([0x5b; 16]);
    let a = RelayAssetId::derive(&space_a, &first, &digest).unwrap();
    let b = RelayAssetId::derive(&space_b, &first, &digest).unwrap();
    assert_ne!(a, b);
    assert_ne!(a.as_bytes(), &digest);
    assert_eq!(a, RelayAssetId::derive(&space_a, &first, &digest).unwrap());
    assert_ne!(
        a,
        RelayAssetId::derive(&space_a, &space_key(1, 0x42), &digest).unwrap()
    );
    assert_eq!(
        RelayAssetId::derive(&space_a, &space_key(2, 0x41), &digest).unwrap_err(),
        RelayError::Crypto(CryptoError::Epoch)
    );
    assert_eq!(
        RelayAssetOpener::new(&space_a, &space_key(2, 0x41), digest).unwrap_err(),
        RelayError::Crypto(CryptoError::Epoch)
    );
}

#[test]
fn relay_debug_output_carries_no_plaintext_or_ciphertext() {
    let key = space_key(1, 0x41);
    let record = record(&key);
    let opened = record.open(&key).unwrap();
    let chunk = golden_chunk();
    let text = format!("{record:?} {opened:?} {chunk:?}");
    let ciphertext = hex(&record.encode()[RELAY_RECORD_HEADER_BYTES + RELAY_NONCE_BYTES..][..8]);
    assert!(!text.contains(&ciphertext));
    assert!(!text.contains(&hex(&operation()[..8])));
    assert!(!text.contains("relay asset golden"));
}

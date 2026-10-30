//! The `GBBK` backup format: frozen header and chunk bytes, file key opening with the epoch key
//! and the recovery key, and refusal of tampered, moved, truncated, and oversized chunks.

use super::crypto::FixedRng;
use super::golden::assert_vector;
use crate::backup::{
    BACKUP_CHUNK_BYTES, BACKUP_INFO, BACKUP_MAGIC, BACKUP_NONCE_BYTES, BACKUP_VERSION,
    BACKUP_WRAP_DOMAIN, BackupError, BackupHeader, ChunkOpener, ChunkSealer, EPOCH_WRAP_BYTES,
    FileKey, HEADER_PREFIX_BYTES, MAX_CHUNK_CIPHERTEXT_BYTES, chunk_aad, recovery_info,
    seal_wrap, wrap_aad,
};
use crate::crypto::{
    EncryptionKeyPair, KEY_ENVELOPE_BYTES, KeyEnvelope, SPACE_KEY_BYTES, SpaceKey, TAG_BYTES,
};
use crate::ids::{SpaceId, hex};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeS, Serializable};
use zeroize::Zeroizing;

fn space() -> SpaceId {
    SpaceId::from_bytes([0x5a; 16])
}

fn space_key(epoch: u32, byte: u8) -> SpaceKey {
    SpaceKey::from_bytes(epoch, Zeroizing::new([byte; SPACE_KEY_BYTES])).unwrap()
}

fn pair(byte: u8) -> EncryptionKeyPair {
    EncryptionKeyPair::from_secret(Zeroizing::new([byte; SPACE_KEY_BYTES])).unwrap()
}

/// A header with fixed file key, wrap nonce, and ephemeral key, so its bytes are reproducible.
fn fixed_header(epoch_key: Option<&SpaceKey>, recovery: &EncryptionKeyPair) -> BackupHeader {
    let file_key = FileKey::from_bytes([0x77; 32]);
    let public =
        <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(recovery.public_key().as_bytes())
            .unwrap();
    let (encapsulated, sealed) =
        hpke::single_shot_seal_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &public,
            &recovery_info(&space()),
            file_key.bytes(),
            &[],
            &mut FixedRng::new(&[0x44; 32]),
        )
        .unwrap();
    BackupHeader {
        space: space(),
        epoch: epoch_key.map_or(0, SpaceKey::epoch),
        epoch_wrap: epoch_key.map(|key| seal_wrap(&space(), key, &file_key, [0x21; 24]).unwrap()),
        recovery: KeyEnvelope::from_parts(
            recovery.public_key().key_id(),
            &encapsulated.to_bytes(),
            &sealed,
        )
        .unwrap(),
    }
}

/// Seals `archive` into frames under a fresh header, as a backup writer does.
fn seal_frames(
    epoch_key: Option<&SpaceKey>,
    recovery: &EncryptionKeyPair,
    archive: &[u8],
) -> (BackupHeader, Vec<([u8; BACKUP_NONCE_BYTES], Vec<u8>)>) {
    let (header, key) = BackupHeader::seal(&space(), epoch_key, &recovery.public_key()).unwrap();
    let mut sealer = ChunkSealer::new(&header, key);
    let mut frames = Vec::new();
    let mut chunks = archive.chunks(BACKUP_CHUNK_BYTES).peekable();
    if chunks.peek().is_none() {
        let mut buffer = Vec::new();
        let nonce = sealer.seal_in_place(&mut buffer, true).unwrap();
        frames.push((nonce, buffer));
    }
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        let mut buffer = chunk.to_vec();
        let nonce = sealer.seal_in_place(&mut buffer, last).unwrap();
        frames.push((nonce, buffer));
    }
    assert!(sealer.finished());
    (header, frames)
}

/// Opens frames in order, marking the final frame as last, as a backup reader does.
fn open_frames(
    header: &BackupHeader,
    key: FileKey,
    frames: &[([u8; BACKUP_NONCE_BYTES], Vec<u8>)],
) -> Result<Vec<u8>, BackupError> {
    let mut opener = ChunkOpener::new(header, key);
    let mut archive = Vec::new();
    for (index, (nonce, ciphertext)) in frames.iter().enumerate() {
        let mut buffer = ciphertext.clone();
        opener.open_in_place(nonce, &mut buffer, index + 1 == frames.len())?;
        archive.extend_from_slice(&buffer);
    }
    if !opener.finished() {
        return Err(BackupError::Truncated);
    }
    Ok(archive)
}

fn archive(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}

#[test]
fn domain_strings_and_aad_are_frozen() {
    assert_eq!(BACKUP_MAGIC, *b"GBBK");
    assert_eq!(BACKUP_VERSION, 1);
    let mut expected = b"ganbaru-sync/backup/v1\0".to_vec();
    expected.extend_from_slice(space().as_bytes());
    assert_eq!(recovery_info(&space()), expected);
    assert_eq!(BACKUP_INFO, b"ganbaru-sync/backup/v1");
    let mut expected = b"ganbaru-sync/backup-wrap/v1\0".to_vec();
    expected.extend_from_slice(space().as_bytes());
    expected.extend_from_slice(&3u32.to_be_bytes());
    assert_eq!(wrap_aad(&space(), 3), expected);
    assert_eq!(BACKUP_WRAP_DOMAIN, b"ganbaru-sync/backup-wrap/v1");
    let aad = chunk_aad(&[0xab; 32], 0x0102, true);
    assert_eq!(&aad[..32], &[0xab; 32]);
    assert_eq!(&aad[32..40], &0x0102u64.to_be_bytes());
    assert_eq!(aad[40], 1);
    assert_eq!(chunk_aad(&[0xab; 32], 0x0102, false)[40], 0);
}

#[test]
fn headers_encode_to_frozen_bytes_and_decode() {
    let recovery = pair(6);
    let key = space_key(2, 9);
    let header = fixed_header(Some(&key), &recovery);
    let bytes = header.to_bytes();
    assert_eq!(
        bytes.len(),
        HEADER_PREFIX_BYTES + EPOCH_WRAP_BYTES + KEY_ENVELOPE_BYTES
    );
    assert_vector("backup-header-epoch", &hex(&bytes));
    assert_eq!(BackupHeader::decode(&bytes).unwrap(), header);
    let prefix: [u8; HEADER_PREFIX_BYTES] = bytes[..HEADER_PREFIX_BYTES].try_into().unwrap();
    assert_eq!(BackupHeader::length(&prefix).unwrap(), bytes.len());

    let unlinked = fixed_header(None, &recovery);
    let bytes = unlinked.to_bytes();
    assert_eq!(bytes.len(), HEADER_PREFIX_BYTES + KEY_ENVELOPE_BYTES);
    assert_vector("backup-header-unlinked", &hex(&bytes));
    assert_eq!(BackupHeader::decode(&bytes).unwrap(), unlinked);
}

#[test]
fn chunks_encode_to_frozen_bytes() {
    let header = fixed_header(Some(&space_key(2, 9)), &pair(6));
    let mut sealer = ChunkSealer::new(&header, FileKey::from_bytes([0x77; 32]));
    let mut buffer = b"backup archive bytes".to_vec();
    sealer
        .seal_in_place_with_nonce(&mut buffer, true, [0x31; BACKUP_NONCE_BYTES])
        .unwrap();
    assert_eq!(buffer.len(), 20 + TAG_BYTES);
    assert_vector("backup-chunk", &hex(&buffer));
    let mut opener = ChunkOpener::new(&header, FileKey::from_bytes([0x77; 32]));
    opener
        .open_in_place(&[0x31; BACKUP_NONCE_BYTES], &mut buffer, true)
        .unwrap();
    assert_eq!(buffer, b"backup archive bytes");
}

#[test]
fn backups_round_trip_at_zero_one_and_many_chunks() {
    let recovery = pair(6);
    let key = space_key(3, 1);
    for length in [
        0,
        1,
        BACKUP_CHUNK_BYTES - 1,
        BACKUP_CHUNK_BYTES,
        BACKUP_CHUNK_BYTES * 3 + 17,
    ] {
        let plaintext = archive(length);
        let (header, frames) = seal_frames(Some(&key), &recovery, &plaintext);
        let expected_frames = length.div_ceil(BACKUP_CHUNK_BYTES).max(1);
        assert_eq!(frames.len(), expected_frames, "length {length}");
        let by_epoch = header.open_with_epoch_key(&key).unwrap();
        assert_eq!(open_frames(&header, by_epoch, &frames).unwrap(), plaintext);
        let by_kit = header.open_with_recovery(&recovery).unwrap();
        assert_eq!(open_frames(&header, by_kit, &frames).unwrap(), plaintext);
    }
}

#[test]
fn unlinked_backups_open_only_with_the_kit() {
    let recovery = pair(6);
    let (header, frames) = seal_frames(None, &recovery, b"zip");
    assert_eq!(header.epoch, 0);
    assert!(header.epoch_wrap.is_none());
    assert_eq!(
        header.open_with_epoch_key(&space_key(1, 1)).err(),
        Some(BackupError::Epoch)
    );
    let key = header.open_with_recovery(&recovery).unwrap();
    assert_eq!(open_frames(&header, key, &frames).unwrap(), b"zip");
}

#[test]
fn wrong_kits_and_epoch_keys_do_not_open_the_file_key() {
    let recovery = pair(6);
    let key = space_key(3, 1);
    let (header, _) = seal_frames(Some(&key), &recovery, b"zip");
    assert_eq!(
        header.open_with_recovery(&pair(7)).err(),
        Some(BackupError::Key)
    );
    assert_eq!(
        header.open_with_epoch_key(&space_key(3, 2)).err(),
        Some(BackupError::Key)
    );
    assert_eq!(
        header.open_with_epoch_key(&space_key(2, 1)).err(),
        Some(BackupError::Epoch)
    );
    // A header moved to another space opens with neither key.
    let mut moved = header.clone();
    moved.space = SpaceId::from_bytes([0x5b; 16]);
    assert_eq!(
        moved.open_with_recovery(&recovery).err(),
        Some(BackupError::Key)
    );
    assert_eq!(
        moved.open_with_epoch_key(&key).err(),
        Some(BackupError::Key)
    );
}

#[test]
fn reordered_truncated_extended_and_swapped_chunks_fail() {
    let recovery = pair(6);
    let plaintext = archive(BACKUP_CHUNK_BYTES * 3);
    let (header, frames) = seal_frames(None, &recovery, &plaintext);
    let open = |frames: &[([u8; BACKUP_NONCE_BYTES], Vec<u8>)]| {
        open_frames(
            &header,
            header.open_with_recovery(&recovery).unwrap(),
            frames,
        )
    };

    let mut reordered = frames.clone();
    reordered.swap(0, 1);
    assert_eq!(open(&reordered).err(), Some(BackupError::Authentication));

    // Dropping the last chunk leaves a full chunk sealed as not last at the end.
    assert_eq!(
        open(&frames[..2]).err(),
        Some(BackupError::Authentication)
    );
    // Dropping the first chunk moves every index.
    assert_eq!(
        open(&frames[1..]).err(),
        Some(BackupError::Authentication)
    );

    let mut extended = frames.clone();
    extended.push(frames[2].clone());
    assert_eq!(open(&extended).err(), Some(BackupError::Authentication));

    // A chunk of another backup of the same space fails under this header.
    let (_, other) = seal_frames(None, &recovery, &plaintext);
    let mut swapped = frames.clone();
    swapped[1] = other[1].clone();
    assert_eq!(open(&swapped).err(), Some(BackupError::Authentication));

    let mut flipped = frames.clone();
    flipped[2].1[0] ^= 1;
    assert_eq!(open(&flipped).err(), Some(BackupError::Authentication));
}

#[test]
fn chunks_after_the_last_and_out_of_bound_lengths_are_refused() {
    let recovery = pair(6);
    let (header, key) = BackupHeader::seal(&space(), None, &recovery.public_key()).unwrap();
    let mut sealer = ChunkSealer::new(&header, key);
    assert_eq!(
        sealer.seal_in_place(&mut vec![0; 10], false).err(),
        Some(BackupError::Malformed)
    );
    assert_eq!(
        sealer
            .seal_in_place(&mut vec![0; BACKUP_CHUNK_BYTES + 1], true)
            .err(),
        Some(BackupError::Malformed)
    );
    let mut last = vec![1, 2, 3];
    let nonce = sealer.seal_in_place(&mut last, true).unwrap();
    assert_eq!(
        sealer.seal_in_place(&mut vec![0; 1], true).err(),
        Some(BackupError::Finished)
    );

    let mut opener = ChunkOpener::new(&header, header.open_with_recovery(&recovery).unwrap());
    assert_eq!(opener.check_length(0).err(), Some(BackupError::Malformed));
    assert_eq!(
        opener.check_length(TAG_BYTES as u32 - 1).err(),
        Some(BackupError::Malformed)
    );
    assert_eq!(
        opener
            .check_length(MAX_CHUNK_CIPHERTEXT_BYTES as u32 + 1)
            .err(),
        Some(BackupError::Malformed)
    );
    assert_eq!(opener.check_length(u32::MAX).err(), Some(BackupError::Malformed));
    assert!(opener.check_length(MAX_CHUNK_CIPHERTEXT_BYTES as u32).is_ok());
    // A short chunk that is not last is refused before decryption.
    let mut short = last.clone();
    assert_eq!(
        opener.open_in_place(&nonce, &mut short, false).err(),
        Some(BackupError::Malformed)
    );
    assert!(short.is_empty());
    opener.open_in_place(&nonce, &mut last, true).unwrap();
    assert_eq!(last, [1, 2, 3]);
    assert_eq!(opener.check_length(16).err(), Some(BackupError::Malformed));
}

#[test]
fn headers_refuse_other_magic_versions_and_lengths() {
    let header = fixed_header(Some(&space_key(2, 9)), &pair(6));
    let bytes = header.to_bytes();
    let prefix = |bytes: &[u8]| -> [u8; HEADER_PREFIX_BYTES] {
        bytes[..HEADER_PREFIX_BYTES].try_into().unwrap()
    };

    let mut zip = bytes.clone();
    zip[..4].copy_from_slice(b"PK\x03\x04");
    assert_eq!(
        BackupHeader::length(&prefix(&zip)).err(),
        Some(BackupError::NotBackup)
    );
    assert_eq!(BackupHeader::decode(&zip).err(), Some(BackupError::NotBackup));

    let mut newer = bytes.clone();
    newer[4] = 2;
    assert_eq!(
        BackupHeader::length(&prefix(&newer)).err(),
        Some(BackupError::Version(2))
    );

    assert_eq!(
        BackupHeader::decode(&bytes[..bytes.len() - 1]).err(),
        Some(BackupError::Truncated)
    );
    assert_eq!(
        BackupHeader::decode(&bytes[..10]).err(),
        Some(BackupError::Truncated)
    );
    let mut longer = bytes.clone();
    longer.push(0);
    assert_eq!(
        BackupHeader::decode(&longer).err(),
        Some(BackupError::Malformed)
    );
}

#[test]
fn every_header_byte_change_fails_the_file_key_or_the_chunks() {
    let recovery = pair(6);
    let key = space_key(2, 9);
    let (header, frames) = seal_frames(Some(&key), &recovery, b"archive");
    let bytes = header.to_bytes();
    for position in 0..bytes.len() {
        let mut tampered = bytes.clone();
        tampered[position] ^= 0x01;
        let Ok(decoded) = BackupHeader::decode(&tampered) else {
            continue;
        };
        for file_key in [
            decoded.open_with_epoch_key(&key),
            decoded.open_with_recovery(&recovery),
        ] {
            if let Ok(file_key) = file_key {
                assert_eq!(
                    open_frames(&decoded, file_key, &frames).err(),
                    Some(BackupError::Authentication),
                    "byte {position}"
                );
            }
        }
    }
}

#[test]
fn debug_output_shows_no_key_material() {
    let recovery = pair(6);
    let (header, key) =
        BackupHeader::seal(&space(), Some(&space_key(2, 9)), &recovery.public_key()).unwrap();
    let text = format!(
        "{header:?} {key:?} {:?}",
        ChunkSealer::new(&header, header.open_with_recovery(&recovery).unwrap())
    );
    let wrap = header.epoch_wrap.as_ref().unwrap();
    assert!(!text.contains(&hex(&wrap.sealed)));
    assert!(!text.contains(&hex(&header.recovery.sealed)));
    assert!(!text.to_lowercase().contains("77777777"));
}

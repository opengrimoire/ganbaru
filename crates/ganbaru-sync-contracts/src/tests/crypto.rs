//! Space key primitives: the RFC 9180 known answers for the chosen HPKE suite, the
//! XChaCha20-Poly1305 known answer, key ids, subkeys, envelopes with Ganbaru's info strings, and
//! chain links.

use super::golden::assert_vector;
use crate::crypto::{
    ChainLink, CryptoError, EncryptionKeyPair, EncryptionPublicKey, KEY_ENVELOPE_BYTES,
    KeyEnvelope, SPACE_KEY_BYTES, SpaceKey, Subkey, chain_aad, envelope_info,
};
use crate::ids::{SpaceId, hex};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::rand_core::{TryCryptoRng, TryRng};
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::path::PathBuf;
use zeroize::Zeroizing;

/// A test-only generator that replays fixed bytes, so HPKE encapsulation reproduces a known
/// ephemeral key. It exists only in this test module.
struct FixedRng {
    bytes: Vec<u8>,
    position: usize,
}

impl FixedRng {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.to_vec(),
            position: 0,
        }
    }
}

impl TryRng for FixedRng {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut bytes = [0u8; 4];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut bytes = [0u8; 8];
        self.try_fill_bytes(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), Infallible> {
        let end = self.position + destination.len();
        assert!(end <= self.bytes.len(), "fixed generator ran out of bytes");
        destination.copy_from_slice(&self.bytes[self.position..end]);
        self.position = end;
        Ok(())
    }
}

impl TryCryptoRng for FixedRng {}

fn fixture(name: &str) -> BTreeMap<String, Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/tests/vectors")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("fixture {name} is missing: {error}"));
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (key, value) = line.split_once('=').expect("fixture line has a key");
            (key.to_string(), decode_hex(value))
        })
        .collect()
}

fn decode_hex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2), "odd hex length");
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).expect("hex digit"))
        .collect()
}

fn key32(bytes: &[u8]) -> [u8; 32] {
    bytes.try_into().expect("32 bytes")
}

fn space() -> SpaceId {
    SpaceId::from_bytes([0x5a; 16])
}

fn space_key(epoch: u32, byte: u8) -> SpaceKey {
    SpaceKey::from_bytes(epoch, Zeroizing::new([byte; SPACE_KEY_BYTES])).unwrap()
}

fn pair(byte: u8) -> EncryptionKeyPair {
    EncryptionKeyPair::from_secret(Zeroizing::new([byte; SPACE_KEY_BYTES])).unwrap()
}

#[test]
fn hpke_base_mode_matches_the_rfc_9180_vector() {
    let vector = fixture("rfc9180-base-x25519-sha256-chacha20poly1305.txt");
    let (secret, public) = X25519HkdfSha256::derive_keypair(&vector["ikmR"]);
    assert_eq!(secret.to_bytes().as_slice(), vector["skRm"].as_slice());
    assert_eq!(public.to_bytes().as_slice(), vector["pkRm"].as_slice());

    // The stored secret form derives the same public key as the RFC's serialized key.
    let recipient = EncryptionKeyPair::from_secret(Zeroizing::new(key32(&vector["skRm"]))).unwrap();
    assert_eq!(
        recipient.public_key().as_bytes().as_slice(),
        vector["pkRm"].as_slice()
    );

    let (encapsulated, mut sender) =
        hpke::setup_sender_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &public,
            &vector["info"],
            &mut FixedRng::new(&vector["ikmE"]),
        )
        .unwrap();
    assert_eq!(encapsulated.to_bytes().as_slice(), vector["enc"].as_slice());
    for index in 0..2 {
        let sealed = sender
            .seal(&vector["pt"], &vector[&format!("aad{index}")])
            .unwrap();
        assert_eq!(sealed, vector[&format!("ct{index}")], "encryption {index}");
    }
    for index in 0..3 {
        let mut exported = [0u8; 32];
        sender
            .export(&vector[&format!("export_context{index}")], &mut exported)
            .unwrap();
        assert_eq!(
            exported.as_slice(),
            vector[&format!("export{index}")].as_slice()
        );
    }

    // The single-shot form the envelopes use produces the first encryption, and opens with the
    // stored secret.
    let (encapsulated, sealed) =
        hpke::single_shot_seal_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &public,
            &vector["info"],
            &vector["pt"],
            &vector["aad0"],
            &mut FixedRng::new(&vector["ikmE"]),
        )
        .unwrap();
    assert_eq!(sealed, vector["ct0"]);
    let encapsulated =
        <X25519HkdfSha256 as Kem>::EncappedKey::from_bytes(&encapsulated.to_bytes()).unwrap();
    let opened = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
        &OpModeR::Base,
        &recipient.hpke_key().unwrap(),
        &encapsulated,
        &vector["info"],
        &vector["ct0"],
        &vector["aad0"],
    )
    .unwrap();
    assert_eq!(opened, vector["pt"]);
}

#[test]
fn xchacha20_poly1305_matches_the_draft_vector() {
    let vector = fixture("xchacha20poly1305-a3.txt");
    let cipher = XChaCha20Poly1305::new_from_slice(&vector["key"]).unwrap();
    let nonce = XNonce::try_from(vector["nonce"].as_slice()).unwrap();
    let sealed = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &vector["pt"],
                aad: &vector["aad"],
            },
        )
        .unwrap();
    let mut expected = vector["ct"].clone();
    expected.extend_from_slice(&vector["tag"]);
    assert_eq!(sealed, expected);
    let opened = cipher
        .decrypt(
            &nonce,
            Payload {
                msg: &sealed,
                aad: &vector["aad"],
            },
        )
        .unwrap();
    assert_eq!(opened, vector["pt"]);
}

#[test]
fn key_ids_hash_the_public_key_under_the_device_key_domain() {
    let key = pair(7).public_key();
    let digest = crate::signing::domain_digest(b"ganbaru-sync/device-key/v1", &[key.as_bytes()]);
    assert_eq!(key.key_id().as_bytes().as_slice(), &digest[..16]);
    assert_ne!(pair(8).public_key().key_id(), key.key_id());
}

#[test]
fn small_order_encryption_keys_are_refused() {
    assert_eq!(
        EncryptionPublicKey::from_bytes([0u8; 32]),
        Err(CryptoError::InvalidKey)
    );
    let mut one = [0u8; 32];
    one[0] = 1;
    assert_eq!(
        EncryptionPublicKey::from_bytes(one),
        Err(CryptoError::InvalidKey)
    );
    assert!(EncryptionPublicKey::from_slice(&[9u8; 31]).is_err());
    assert!(EncryptionPublicKey::from_bytes(*pair(3).public_key().as_bytes()).is_ok());
}

#[test]
fn subkeys_are_independent_per_purpose_space_and_epoch() {
    let key = space_key(1, 1);
    let mut seen = Vec::new();
    for purpose in Subkey::ALL {
        let subkey = key.subkey(purpose, &space());
        assert_ne!(*subkey, *key.bytes());
        assert!(!seen.contains(&*subkey), "{purpose:?} repeats a subkey");
        seen.push(*subkey);
    }
    let other_space = SpaceId::from_bytes([0x5b; 16]);
    assert_ne!(
        *key.subkey(Subkey::RelayRecord, &space()),
        *key.subkey(Subkey::RelayRecord, &other_space)
    );
    assert_ne!(
        *key.subkey(Subkey::RelayRecord, &space()),
        *space_key(2, 2).subkey(Subkey::RelayRecord, &space())
    );
    assert_eq!(
        hex(&*key.subkey(Subkey::RelayRecord, &space())),
        hex(&*space_key(1, 1).subkey(Subkey::RelayRecord, &space()))
    );
    assert_eq!(
        key.relay_access_public_key(&space()).unwrap(),
        key.relay_access_key(&space()).unwrap().public_key()
    );
}

#[test]
fn subkeys_and_relay_access_keys_encode_to_frozen_bytes() {
    let key = space_key(1, 1);
    let mut lines: Vec<String> = Subkey::ALL
        .iter()
        .map(|purpose| hex(&*key.subkey(*purpose, &space())))
        .collect();
    lines.push(hex(key
        .relay_access_public_key(&space())
        .unwrap()
        .as_bytes()));
    assert_vector("space-key-subkeys", &lines.join("\n"));
}

#[test]
fn envelope_info_and_chain_aad_follow_the_design() {
    let recipient = pair(4).public_key().key_id();
    let info = envelope_info(&space(), 3, &recipient);
    let mut expected = b"ganbaru-sync/space-key/v1\0".to_vec();
    expected.extend_from_slice(space().as_bytes());
    expected.extend_from_slice(&3u32.to_be_bytes());
    expected.extend_from_slice(recipient.as_bytes());
    assert_eq!(info, expected);
    let mut expected = b"ganbaru-sync/key-chain/v1\0".to_vec();
    expected.extend_from_slice(space().as_bytes());
    expected.extend_from_slice(&3u32.to_be_bytes());
    assert_eq!(chain_aad(&space(), 3), expected);
}

/// An envelope sealed with a fixed ephemeral key, so its bytes are reproducible.
fn fixed_envelope(key: &SpaceKey, recipient: &EncryptionPublicKey, ikm: &[u8]) -> KeyEnvelope {
    let id = recipient.key_id();
    let public = <X25519HkdfSha256 as Kem>::PublicKey::from_bytes(recipient.as_bytes()).unwrap();
    let (encapsulated, sealed) =
        hpke::single_shot_seal_with_rng::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256>(
            &OpModeS::Base,
            &public,
            &envelope_info(&space(), key.epoch(), &id),
            key.bytes(),
            &[],
            &mut FixedRng::new(ikm),
        )
        .unwrap();
    KeyEnvelope::from_parts(id, &encapsulated.to_bytes(), &sealed).unwrap()
}

#[test]
fn envelopes_with_the_space_key_info_encode_to_frozen_bytes_and_open() {
    let recipient = pair(4);
    let key = space_key(2, 9);
    let envelope = fixed_envelope(&key, &recipient.public_key(), &[0x33; 32]);
    let bytes = envelope.to_bytes();
    assert_eq!(bytes.len(), KEY_ENVELOPE_BYTES);
    assert_vector("space-key-envelope", &hex(&bytes));
    assert_eq!(KeyEnvelope::from_slice(&bytes).unwrap(), envelope);
    let opened = envelope.open(&space(), 2, &recipient).unwrap();
    assert_eq!(opened.epoch(), 2);
    assert_eq!(*opened.bytes(), *key.bytes());
}

#[test]
fn envelopes_open_only_for_their_recipient_space_and_epoch() {
    let recipient = pair(4);
    let key = space_key(2, 9);
    let envelope = KeyEnvelope::seal(&space(), &key, &recipient.public_key()).unwrap();
    assert_eq!(
        *envelope.open(&space(), 2, &recipient).unwrap().bytes(),
        *key.bytes()
    );
    // Fresh encapsulation makes every seal distinct.
    let again = KeyEnvelope::seal(&space(), &key, &recipient.public_key()).unwrap();
    assert_ne!(again.encapsulated, envelope.encapsulated);

    assert_eq!(
        envelope.open(&space(), 3, &recipient).err(),
        Some(CryptoError::Open)
    );
    let other_space = SpaceId::from_bytes([0x5b; 16]);
    assert_eq!(
        envelope.open(&other_space, 2, &recipient).err(),
        Some(CryptoError::Open)
    );
    assert_eq!(
        envelope.open(&space(), 2, &pair(5)).err(),
        Some(CryptoError::Open)
    );
    // An envelope relabeled for another recipient does not open for that recipient either.
    let mut relabeled = envelope.clone();
    relabeled.recipient = pair(5).public_key().key_id();
    assert_eq!(
        relabeled.open(&space(), 2, &pair(5)).err(),
        Some(CryptoError::Open)
    );
    for index in 0..KEY_ENVELOPE_BYTES {
        let mut bytes = envelope.to_bytes();
        bytes[index] ^= 1;
        let tampered = KeyEnvelope::from_slice(&bytes).unwrap();
        assert!(
            tampered.open(&space(), 2, &recipient).is_err(),
            "flipping byte {index} still opens"
        );
    }
    assert!(KeyEnvelope::from_slice(&envelope.to_bytes()[1..]).is_none());
}

#[test]
fn chain_links_walk_back_and_never_forward() {
    let keys: Vec<SpaceKey> = (1..=5)
        .map(|epoch| SpaceKey::generate(epoch).unwrap())
        .collect();
    let links: Vec<ChainLink> = keys
        .windows(2)
        .map(|pair| ChainLink::seal(&space(), &pair[1], &pair[0]).unwrap())
        .collect();
    let mut current = keys[4].clone();
    for epoch in (1..5).rev() {
        let previous = links[epoch - 1].open(&space(), &current).unwrap();
        assert_eq!(previous.epoch() as usize, epoch);
        assert_eq!(*previous.bytes(), *keys[epoch - 1].bytes());
        current = previous;
    }
    // A link does not open with the key it seals, the wrong epoch, or another space.
    assert!(links[0].open(&space(), &keys[0]).is_err());
    let relabeled = SpaceKey::from_bytes(3, Zeroizing::new(*keys[1].bytes())).unwrap();
    assert!(links[0].open(&space(), &relabeled).is_err());
    assert!(
        links[0]
            .open(&SpaceId::from_bytes([1; 16]), &keys[1])
            .is_err()
    );
    let mut tampered = links[0].clone();
    tampered.sealed[0] ^= 1;
    assert!(tampered.open(&space(), &keys[1]).is_err());
    assert_eq!(
        ChainLink::seal(&space(), &keys[2], &keys[0]).err(),
        Some(CryptoError::Epoch)
    );
    assert!(ChainLink::seal(&space(), &keys[0], &keys[0]).is_err());
}

#[test]
fn chain_links_encode_to_frozen_bytes() {
    let link = ChainLink::seal_with_nonce(&space(), &space_key(2, 2), &space_key(1, 1), [0x44; 24])
        .unwrap();
    assert_vector("key-chain-link", &hex(&link.to_bytes()));
    assert_eq!(ChainLink::from_slice(&link.to_bytes()).unwrap(), link);
    assert_eq!(
        *link.open(&space(), &space_key(2, 2)).unwrap().bytes(),
        [1u8; SPACE_KEY_BYTES]
    );
}

#[test]
fn epoch_zero_holds_no_key() {
    assert_eq!(
        SpaceKey::from_bytes(0, Zeroizing::new([1; SPACE_KEY_BYTES])).err(),
        Some(CryptoError::Epoch)
    );
    assert!(SpaceKey::generate(0).is_err());
    let generated = SpaceKey::generate(1).unwrap();
    assert_ne!(*generated.bytes(), *SpaceKey::generate(1).unwrap().bytes());
    assert!(!format!("{generated:?}").contains(&hex(generated.bytes())));
}

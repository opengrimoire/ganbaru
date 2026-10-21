//! Ed25519 person keys and signature verification.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SecureRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// Raw Ed25519 public key length.
pub const PUBLIC_KEY_BYTES: usize = 32;
/// Ed25519 signature length.
pub const SIGNATURE_BYTES: usize = 64;
/// Length of a base64url (no padding) public key.
pub const PUBLIC_KEY_TEXT_LENGTH: usize = 43;

const CONTACT_ID_PREFIX: &str = "person:";
const CONTACT_ID_DIGEST_CHARS: usize = 32;

/// Ed25519 y coordinates of the points whose order divides 8, little endian with the sign bit
/// clear: 0, 1, the two order-8 values, `p - 1`, and the non-canonical `p` and `p + 1`, which
/// verifiers decode as 0 and 1.
const SMALL_ORDER_Y_COORDINATES: [[u8; PUBLIC_KEY_BYTES]; 7] = [
    [0; 32],
    [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ],
    [
        0x26, 0xe8, 0x95, 0x8f, 0xc2, 0xb2, 0x27, 0xb0, 0x45, 0xc3, 0xf4, 0x89, 0xf2, 0xef, 0x98,
        0xf0, 0xd5, 0xdf, 0xac, 0x05, 0xd3, 0xc6, 0x33, 0x39, 0xb1, 0x38, 0x02, 0x88, 0x6d, 0x53,
        0xfc, 0x05,
    ],
    [
        0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67,
        0x0f, 0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac,
        0x03, 0x7a,
    ],
    field_prime_plus(-1),
    field_prime_plus(0),
    field_prime_plus(1),
];

/// `2^255 - 19 + offset` in little endian, for offsets that only change the lowest byte.
const fn field_prime_plus(offset: i8) -> [u8; PUBLIC_KEY_BYTES] {
    let mut bytes = [0xff; PUBLIC_KEY_BYTES];
    bytes[0] = 0xed_u8.wrapping_add_signed(offset);
    bytes[PUBLIC_KEY_BYTES - 1] = 0x7f;
    bytes
}

/// Whether key bytes encode an Ed25519 point of order dividing 8, in any sign or non-canonical
/// form. Anyone can forge signatures that verify under such a key, so it never names a person
/// or a writer.
pub fn is_small_order_public_key(bytes: &[u8; PUBLIC_KEY_BYTES]) -> bool {
    let mut y = *bytes;
    y[PUBLIC_KEY_BYTES - 1] &= 0x7f;
    SMALL_ORDER_Y_COORDINATES.contains(&y)
}

/// Signature or key material that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// The key bytes are not a valid Ed25519 key.
    InvalidKey,
    /// The signature does not verify against the payload.
    Invalid,
}

impl fmt::Display for SignatureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str("person key is invalid"),
            Self::Invalid => formatter.write_str("signature does not verify"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// A person's Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersonPublicKey([u8; PUBLIC_KEY_BYTES]);

impl PersonPublicKey {
    /// Wraps raw key bytes without verifying that they describe a valid curve point. Invalid and
    /// small-order keys fail at verification.
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Parses a raw key from a slice, refusing small-order keys.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, SignatureError> {
        let bytes =
            <[u8; PUBLIC_KEY_BYTES]>::try_from(bytes).map_err(|_| SignatureError::InvalidKey)?;
        if is_small_order_public_key(&bytes) {
            return Err(SignatureError::InvalidKey);
        }
        Ok(Self(bytes))
    }

    /// Parses the base64url text form.
    pub fn from_text(text: &str) -> Result<Self, SignatureError> {
        if text.len() != PUBLIC_KEY_TEXT_LENGTH {
            return Err(SignatureError::InvalidKey);
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| SignatureError::InvalidKey)?;
        Self::from_slice(&bytes)
    }

    /// Raw key bytes.
    pub fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        &self.0
    }

    /// Base64url text form without padding.
    pub fn to_text(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Stable contact identifier derived from the key digest.
    pub fn contact_id(&self) -> String {
        let digest = Sha256::digest(self.0);
        let encoded = URL_SAFE_NO_PAD.encode(digest);
        format!("{CONTACT_ID_PREFIX}{}", &encoded[..CONTACT_ID_DIGEST_CHARS])
    }

    /// Short code both parties compare out of band. It derives from the key alone, so every card this person
    /// signs, including regenerated ones, shows the same code.
    pub fn verification_code(&self) -> String {
        crate::card::verification_code(&Sha256::digest(self.0).into())
    }
}

impl fmt::Debug for PersonPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PersonPublicKey")
            .field(&self.to_text())
            .finish()
    }
}

/// A person's Ed25519 signing key.
pub struct PersonKeyPair(Ed25519KeyPair);

impl PersonKeyPair {
    /// Generates a fresh key, returning its PKCS#8 encoding for storage.
    pub fn generate() -> Result<(Vec<u8>, Self), SignatureError> {
        let rng = ring::rand::SystemRandom::new();
        let document =
            Ed25519KeyPair::generate_pkcs8(&rng).map_err(|_| SignatureError::InvalidKey)?;
        let pair = Self::from_pkcs8(document.as_ref())?;
        Ok((document.as_ref().to_vec(), pair))
    }

    /// Restores a key from its PKCS#8 encoding.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, SignatureError> {
        Ed25519KeyPair::from_pkcs8(pkcs8)
            .map(Self)
            .map_err(|_| SignatureError::InvalidKey)
    }

    /// Derives a key from a 32-byte secret seed. The seed must come from a secure random source,
    /// except in reproducible test vectors.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, SignatureError> {
        Ed25519KeyPair::from_seed_unchecked(seed)
            .map(Self)
            .map_err(|_| SignatureError::InvalidKey)
    }

    /// Public half of the key.
    pub fn public_key(&self) -> PersonPublicKey {
        let mut bytes = [0u8; PUBLIC_KEY_BYTES];
        bytes.copy_from_slice(self.0.public_key().as_ref());
        PersonPublicKey(bytes)
    }

    /// Signs a payload.
    pub fn sign(&self, payload: &[u8]) -> [u8; SIGNATURE_BYTES] {
        let mut signature = [0u8; SIGNATURE_BYTES];
        signature.copy_from_slice(self.0.sign(payload).as_ref());
        signature
    }
}

impl fmt::Debug for PersonKeyPair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PersonKeyPair")
            .field(&self.public_key())
            .finish()
    }
}

/// Verifies an Ed25519 signature over a payload. A small-order key never verifies.
pub fn verify(
    public_key: &PersonPublicKey,
    payload: &[u8],
    signature: &[u8],
) -> Result<(), SignatureError> {
    if is_small_order_public_key(public_key.as_bytes()) {
        return Err(SignatureError::InvalidKey);
    }
    if signature.len() != SIGNATURE_BYTES {
        return Err(SignatureError::Invalid);
    }
    UnparsedPublicKey::new(&ED25519, public_key.as_bytes())
        .verify(payload, signature)
        .map_err(|_| SignatureError::Invalid)
}

/// Fills a buffer with cryptographically secure random bytes.
pub fn random_bytes(buffer: &mut [u8]) -> Result<(), SignatureError> {
    ring::rand::SystemRandom::new()
        .fill(buffer)
        .map_err(|_| SignatureError::InvalidKey)
}

/// Compares two secrets without short-circuiting on the first differing byte.
pub fn nonces_match(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let difference = left
        .iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b));
    difference == 0
}

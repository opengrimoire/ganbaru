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
    /// Wraps raw key bytes without verifying that they describe a valid curve point.
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Parses a raw key from a slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, SignatureError> {
        <[u8; PUBLIC_KEY_BYTES]>::try_from(bytes)
            .map(Self)
            .map_err(|_| SignatureError::InvalidKey)
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

/// Verifies an Ed25519 signature over a payload.
pub fn verify(
    public_key: &PersonPublicKey,
    payload: &[u8],
    signature: &[u8],
) -> Result<(), SignatureError> {
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

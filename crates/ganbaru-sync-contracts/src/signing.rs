//! Domain-separated hashing and writer signatures.
//!
//! Every digest and signature input starts with a domain string and a zero byte, so bytes
//! produced for one purpose never verify or collide under another.

use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use sha2::{Digest, Sha256};
use std::fmt;

/// Writer signature over the signed envelope bytes of an operation.
pub const OPERATION_SIGNATURE_DOMAIN: &[u8] = b"ganbaru-sync/op/v1";
/// Operation hash that forms each writer's hash chain.
pub const OPERATION_HASH_DOMAIN: &[u8] = b"ganbaru-sync/op-hash/v1";
/// Hash of one group value inside an operation.
pub const VALUE_HASH_DOMAIN: &[u8] = b"ganbaru-sync/value/v1";
/// Person signature over a writer certificate.
pub const CERTIFICATE_DOMAIN: &[u8] = b"ganbaru-sync/writer-certificate/v1";
/// Derivation of a personal space id from a vault id.
pub const PERSONAL_SPACE_DOMAIN: &[u8] = b"ganbaru-sync/personal-space/v1";
/// Derivation of a writer id from a writer public key.
pub const WRITER_ID_DOMAIN: &[u8] = b"ganbaru-sync/writer/v1";

/// Raw Ed25519 public key length.
pub const WRITER_PUBLIC_KEY_BYTES: usize = 32;
/// Ed25519 signature length.
pub const WRITER_SIGNATURE_BYTES: usize = 64;
/// SHA-256 digest length.
pub const DIGEST_BYTES: usize = 32;

/// A SHA-256 digest.
pub type Digest32 = [u8; DIGEST_BYTES];

/// SHA-256 over a domain, a zero byte, and the concatenated parts.
pub fn domain_digest(domain: &[u8], parts: &[&[u8]]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update([0u8]);
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

/// A domain, a zero byte, and the payload, as signed by Ed25519.
pub fn domain_message(domain: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut message = Vec::with_capacity(domain.len() + 1 + payload.len());
    message.extend_from_slice(domain);
    message.push(0);
    message.extend_from_slice(payload);
    message
}

/// Writer key material that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterKeyError {
    /// The key bytes are not a valid Ed25519 key.
    InvalidKey,
    /// The random source failed.
    Random,
}

impl fmt::Display for WriterKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str("writer key is invalid"),
            Self::Random => formatter.write_str("secure random source failed"),
        }
    }
}

impl std::error::Error for WriterKeyError {}

/// A writer's Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WriterPublicKey([u8; WRITER_PUBLIC_KEY_BYTES]);

impl WriterPublicKey {
    /// Wraps raw key bytes. Invalid curve points fail at verification.
    pub fn from_bytes(bytes: [u8; WRITER_PUBLIC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// Parses a raw key from a slice.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, WriterKeyError> {
        <[u8; WRITER_PUBLIC_KEY_BYTES]>::try_from(bytes)
            .map(Self)
            .map_err(|_| WriterKeyError::InvalidKey)
    }

    /// Raw key bytes.
    pub fn as_bytes(&self) -> &[u8; WRITER_PUBLIC_KEY_BYTES] {
        &self.0
    }

    /// Whether `signature` is this writer's signature over the signed envelope bytes.
    pub fn verify_operation(&self, signed_bytes: &[u8], signature: &[u8]) -> bool {
        if signature.len() != WRITER_SIGNATURE_BYTES {
            return false;
        }
        UnparsedPublicKey::new(&ED25519, &self.0)
            .verify(
                &domain_message(OPERATION_SIGNATURE_DOMAIN, signed_bytes),
                signature,
            )
            .is_ok()
    }
}

impl fmt::Debug for WriterPublicKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("WriterPublicKey")
            .field(&crate::ids::hex(&self.0))
            .finish()
    }
}

/// A writer's Ed25519 signing key. It signs operations only.
pub struct WriterKeyPair(Ed25519KeyPair);

impl WriterKeyPair {
    /// Generates a fresh key, returning its PKCS#8 encoding for storage.
    pub fn generate() -> Result<(Vec<u8>, Self), WriterKeyError> {
        let random = SystemRandom::new();
        let document =
            Ed25519KeyPair::generate_pkcs8(&random).map_err(|_| WriterKeyError::Random)?;
        let pair = Self::from_pkcs8(document.as_ref())?;
        Ok((document.as_ref().to_vec(), pair))
    }

    /// Restores a key from its PKCS#8 encoding.
    pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, WriterKeyError> {
        Ed25519KeyPair::from_pkcs8(pkcs8)
            .map(Self)
            .map_err(|_| WriterKeyError::InvalidKey)
    }

    /// Derives a key from a 32-byte secret seed. The seed must come from a secure random source,
    /// except in reproducible test vectors.
    pub fn from_seed(seed: &[u8; 32]) -> Result<Self, WriterKeyError> {
        Ed25519KeyPair::from_seed_unchecked(seed)
            .map(Self)
            .map_err(|_| WriterKeyError::InvalidKey)
    }

    /// Public half of the key.
    pub fn public_key(&self) -> WriterPublicKey {
        let mut bytes = [0u8; WRITER_PUBLIC_KEY_BYTES];
        bytes.copy_from_slice(self.0.public_key().as_ref());
        WriterPublicKey(bytes)
    }

    /// Signs the signed envelope bytes of an operation.
    pub fn sign_operation(&self, signed_bytes: &[u8]) -> [u8; WRITER_SIGNATURE_BYTES] {
        let mut signature = [0u8; WRITER_SIGNATURE_BYTES];
        signature.copy_from_slice(
            self.0
                .sign(&domain_message(OPERATION_SIGNATURE_DOMAIN, signed_bytes))
                .as_ref(),
        );
        signature
    }
}

impl fmt::Debug for WriterKeyPair {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("WriterKeyPair")
            .field(&self.public_key())
            .finish()
    }
}

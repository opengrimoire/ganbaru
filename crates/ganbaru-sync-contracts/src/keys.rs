//! Device and admin Ed25519 keys.
//!
//! A device key belongs to one installation in one vault: it certifies that installation's
//! writers (certificate v2) and is recorded in its enrollment. An admin key belongs to one admin
//! desktop, or is derived from the recovery kit, and signs authorization proposals. Both follow
//! the person key and writer key style: a public key type that refuses small-order points and a
//! key pair restored from PKCS#8 or a seed. The roles are distinct types so a signature of one
//! role can never be checked against a key of another.

use crate::signing::{WRITER_PUBLIC_KEY_BYTES, WRITER_SIGNATURE_BYTES, domain_message};
use ganbaru_contacts::is_small_order_public_key;
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use std::fmt;

/// Raw Ed25519 public key length of device and admin keys.
pub const PUBLIC_KEY_BYTES: usize = WRITER_PUBLIC_KEY_BYTES;
/// Ed25519 signature length of device and admin keys.
pub const SIGNATURE_BYTES: usize = WRITER_SIGNATURE_BYTES;
/// Ed25519 seed length.
pub const SEED_BYTES: usize = 32;

/// Device or admin key material that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// The key bytes are not a valid Ed25519 key.
    InvalidKey,
    /// The random source failed.
    Random,
}

impl fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey => formatter.write_str("signing key is invalid"),
            Self::Random => formatter.write_str("secure random source failed"),
        }
    }
}

impl std::error::Error for KeyError {}

fn verify_ed25519(
    key: &[u8; PUBLIC_KEY_BYTES],
    domain: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> bool {
    if signature.len() != SIGNATURE_BYTES || is_small_order_public_key(key) {
        return false;
    }
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&domain_message(domain, payload), signature)
        .is_ok()
}

macro_rules! ed25519_role {
    ($(#[$public_meta:meta])* $public:ident, $(#[$pair_meta:meta])* $pair:ident) => {
        $(#[$public_meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $public([u8; PUBLIC_KEY_BYTES]);

        impl $public {
            /// Wraps raw key bytes. Invalid curve points and small-order keys fail at
            /// verification.
            pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
                Self(bytes)
            }

            /// Parses a raw key from a slice, refusing small-order keys.
            pub fn from_slice(bytes: &[u8]) -> Result<Self, KeyError> {
                let bytes = <[u8; PUBLIC_KEY_BYTES]>::try_from(bytes)
                    .map_err(|_| KeyError::InvalidKey)?;
                if is_small_order_public_key(&bytes) {
                    return Err(KeyError::InvalidKey);
                }
                Ok(Self(bytes))
            }

            /// Raw key bytes.
            pub fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
                &self.0
            }

            /// Whether `signature` signs `payload` under `domain` with this key. A small-order
            /// key never verifies.
            pub fn verify(&self, domain: &[u8], payload: &[u8], signature: &[u8]) -> bool {
                verify_ed25519(&self.0, domain, payload, signature)
            }
        }

        impl fmt::Debug for $public {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($public))
                    .field(&crate::ids::hex(&self.0))
                    .finish()
            }
        }

        $(#[$pair_meta])*
        pub struct $pair(Ed25519KeyPair);

        impl $pair {
            /// Generates a fresh key, returning its PKCS#8 encoding for storage.
            pub fn generate() -> Result<(Vec<u8>, Self), KeyError> {
                let random = SystemRandom::new();
                let document =
                    Ed25519KeyPair::generate_pkcs8(&random).map_err(|_| KeyError::Random)?;
                let pair = Self::from_pkcs8(document.as_ref())?;
                Ok((document.as_ref().to_vec(), pair))
            }

            /// Restores a key from its PKCS#8 encoding.
            pub fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, KeyError> {
                Ed25519KeyPair::from_pkcs8(pkcs8)
                    .map(Self)
                    .map_err(|_| KeyError::InvalidKey)
            }

            /// Derives a key from a 32-byte secret seed. The seed must come from a secure random
            /// source or a key derivation over one, except in reproducible test vectors.
            pub fn from_seed(seed: &[u8; SEED_BYTES]) -> Result<Self, KeyError> {
                Ed25519KeyPair::from_seed_unchecked(seed)
                    .map(Self)
                    .map_err(|_| KeyError::InvalidKey)
            }

            /// Public half of the key.
            pub fn public_key(&self) -> $public {
                let mut bytes = [0u8; PUBLIC_KEY_BYTES];
                bytes.copy_from_slice(self.0.public_key().as_ref());
                $public(bytes)
            }

            /// Signs `payload` under `domain`.
            pub fn sign(&self, domain: &[u8], payload: &[u8]) -> [u8; SIGNATURE_BYTES] {
                let mut signature = [0u8; SIGNATURE_BYTES];
                signature.copy_from_slice(self.0.sign(&domain_message(domain, payload)).as_ref());
                signature
            }
        }

        impl fmt::Debug for $pair {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .debug_tuple(stringify!($pair))
                    .field(&self.public_key())
                    .finish()
            }
        }
    };
}

ed25519_role!(
    /// An installation's device public key, recorded in its enrollment.
    DevicePublicKey,
    /// An installation's device signing key. It signs writer certificates only.
    DeviceKeyPair
);

ed25519_role!(
    /// An admin public key: an admin desktop's own key or the recovery admin key.
    AdminPublicKey,
    /// An admin signing key. It signs authorization proposals only.
    AdminKeyPair
);

ed25519_role!(
    /// The relay access public key of one key epoch, published in its `KeyEpoch` entry.
    RelayAccessPublicKey,
    /// The relay access signing key of one key epoch, derived from the epoch's space key. It
    /// signs relay requests only.
    RelayAccessKeyPair
);

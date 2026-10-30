//! Recovery kit: a 32-byte secret seed, its printable code, and the keys derived from it.
//!
//! Keys derive with HKDF-SHA256 over the seed, an empty salt, and an info string of a purpose
//! label followed by a zero byte, so each purpose gets an independent key from the same kit. The
//! recovery admin key uses [`RECOVERY_ADMIN_INFO`] and the recovery encryption key, the X25519
//! pair space key envelopes are sealed to, uses [`RECOVERY_HPKE_INFO`]. Seed and derived bytes
//! are zeroized on drop, and comparisons of seed-derived values run in constant time.
//!
//! The kit code is the seed followed by a 3-byte checksum (the first bytes of the recovery kit
//! digest of the seed), written in Crockford base32 and grouped for reading aloud. Parsing
//! ignores case, hyphens, and whitespace, reads `O` as `0` and `I` or `L` as `1`, and refuses a
//! code whose checksum does not match.

use crate::crypto::{CryptoError, EncryptionKeyPair, EncryptionPublicKey};
use crate::keys::{AdminKeyPair, AdminPublicKey, KeyError, SEED_BYTES};
use crate::signing::{RECOVERY_KIT_DOMAIN, domain_digest};
use ring::hkdf::{HKDF_SHA256, Salt};
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use zeroize::Zeroizing;

/// HKDF info label of the recovery admin key.
pub const RECOVERY_ADMIN_INFO: &[u8] = b"ganbaru-sync/recovery-admin/v1";
/// HKDF info label of the recovery encryption key.
pub const RECOVERY_HPKE_INFO: &[u8] = b"ganbaru-sync/recovery-hpke/v1";
/// Checksum bytes appended to the seed in a kit code.
pub const KIT_CHECKSUM_BYTES: usize = 3;
/// Characters per group in a formatted kit code.
pub const KIT_GROUP_CHARS: usize = 4;
/// Separator between groups in a formatted kit code.
pub const KIT_GROUP_SEPARATOR: char = '-';

const KIT_PAYLOAD_BYTES: usize = SEED_BYTES + KIT_CHECKSUM_BYTES;
/// Base32 characters in a kit code, separators excluded.
pub const KIT_CODE_CHARS: usize = (KIT_PAYLOAD_BYTES * 8).div_ceil(5);

const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A recovery kit that cannot be generated or parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    /// The code holds a character outside Crockford base32 and the accepted separators.
    Character,
    /// The code has the wrong number of base32 characters.
    Length,
    /// The checksum does not match the seed.
    Checksum,
    /// The random source failed.
    Random,
    /// A derived key could not be built.
    Key(KeyError),
    /// A derived encryption key could not be built.
    Encryption(CryptoError),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Character => formatter.write_str("recovery code has an invalid character"),
            Self::Length => formatter.write_str("recovery code has the wrong length"),
            Self::Checksum => formatter.write_str("recovery code checksum does not match"),
            Self::Random => formatter.write_str("secure random source failed"),
            Self::Key(error) => write!(formatter, "recovery key: {error}"),
            Self::Encryption(error) => write!(formatter, "recovery encryption key: {error}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

impl From<KeyError> for RecoveryError {
    fn from(error: KeyError) -> Self {
        Self::Key(error)
    }
}

/// The secret seed a recovery kit carries. Its bytes are zeroized on drop; it has no `PartialEq`,
/// so seeds compare only through [`Self::ct_eq`].
#[derive(Clone)]
pub struct RecoverySeed(Zeroizing<[u8; SEED_BYTES]>);

/// The public keys a recovery kit derives, which a vault keeps without the seed: the recovery
/// admin key a charter names and the recovery encryption key space key envelopes are sealed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPublicKeys {
    /// Public half of the recovery admin key.
    pub admin: AdminPublicKey,
    /// Public half of the recovery encryption key.
    pub encryption: EncryptionPublicKey,
}

impl RecoverySeed {
    /// Generates a fresh seed from the secure random source.
    pub fn generate() -> Result<Self, RecoveryError> {
        let mut seed = Zeroizing::new([0u8; SEED_BYTES]);
        SystemRandom::new()
            .fill(seed.as_mut())
            .map_err(|_| RecoveryError::Random)?;
        Ok(Self(seed))
    }

    /// Wraps raw seed bytes.
    pub fn from_bytes(bytes: [u8; SEED_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Raw seed bytes.
    pub fn as_bytes(&self) -> &[u8; SEED_BYTES] {
        &self.0
    }

    /// Whether `other` is the same seed, compared in constant time.
    pub fn ct_eq(&self, other: &Self) -> bool {
        constant_time_eq(self.0.as_slice(), other.0.as_slice())
    }

    /// Derives 32 bytes for the purpose `label`. The label must be unique to its purpose and
    /// version.
    pub fn derive(&self, label: &[u8]) -> Zeroizing<[u8; SEED_BYTES]> {
        let mut info = Vec::with_capacity(label.len() + 1);
        info.extend_from_slice(label);
        info.push(0);
        let info = [info.as_slice()];
        let mut output = Zeroizing::new([0u8; SEED_BYTES]);
        Salt::new(HKDF_SHA256, &[])
            .extract(self.0.as_slice())
            .expand(&info, HKDF_SHA256)
            .and_then(|okm| okm.fill(output.as_mut()))
            .expect("HKDF-SHA256 expands 32 bytes");
        output
    }

    /// The recovery admin key pair.
    pub fn recovery_admin_key(&self) -> Result<AdminKeyPair, RecoveryError> {
        Ok(AdminKeyPair::from_seed(&self.derive(RECOVERY_ADMIN_INFO))?)
    }

    /// The recovery encryption key pair, the X25519 key a kit opens space key envelopes with.
    pub fn recovery_encryption_key(&self) -> Result<EncryptionKeyPair, RecoveryError> {
        EncryptionKeyPair::from_secret(self.derive(RECOVERY_HPKE_INFO))
            .map_err(RecoveryError::Encryption)
    }

    /// The public halves of both recovery keys.
    pub fn public_keys(&self) -> Result<RecoveryPublicKeys, RecoveryError> {
        Ok(RecoveryPublicKeys {
            admin: self.recovery_admin_key()?.public_key(),
            encryption: self.recovery_encryption_key()?.public_key(),
        })
    }

    /// The grouped kit code.
    pub fn kit_code(&self) -> String {
        let characters = self.code_characters();
        let mut code = String::with_capacity(KIT_CODE_CHARS + KIT_CODE_CHARS / KIT_GROUP_CHARS);
        for (index, character) in characters.iter().enumerate() {
            if index > 0 && index % KIT_GROUP_CHARS == 0 {
                code.push(KIT_GROUP_SEPARATOR);
            }
            code.push(char::from(*character));
        }
        code
    }

    /// Whether `typed` is the last group of the kit code, read as [`Self::parse_kit_code`] reads
    /// a code and compared in constant time. A person types it back to confirm a saved kit.
    pub fn last_group_matches(&self, typed: &str) -> bool {
        let characters = self.code_characters();
        let start = (KIT_CODE_CHARS - 1) / KIT_GROUP_CHARS * KIT_GROUP_CHARS;
        let expected = &characters[start..];
        let mut values = Zeroizing::new(Vec::with_capacity(expected.len()));
        for character in typed.chars() {
            if character == KIT_GROUP_SEPARATOR || character.is_whitespace() {
                continue;
            }
            match crockford_value(character) {
                Some(value) => values.push(CROCKFORD_ALPHABET[usize::from(value)]),
                None => return false,
            }
        }
        constant_time_eq(expected, &values)
    }

    /// The base32 characters of the kit code without separators.
    fn code_characters(&self) -> Zeroizing<Vec<u8>> {
        let mut payload = Zeroizing::new([0u8; KIT_PAYLOAD_BYTES]);
        payload[..SEED_BYTES].copy_from_slice(self.0.as_slice());
        payload[SEED_BYTES..].copy_from_slice(&kit_checksum(&self.0));
        Zeroizing::new(encode_base32(payload.as_slice()))
    }

    /// Parses a kit code as a person may type it.
    pub fn parse_kit_code(code: &str) -> Result<Self, RecoveryError> {
        let mut values = Zeroizing::new(Vec::with_capacity(KIT_CODE_CHARS));
        for character in code.chars() {
            if character == KIT_GROUP_SEPARATOR || character.is_whitespace() {
                continue;
            }
            values.push(crockford_value(character).ok_or(RecoveryError::Character)?);
        }
        if values.len() != KIT_CODE_CHARS {
            return Err(RecoveryError::Length);
        }
        let payload = Zeroizing::new(decode_base32(&values).ok_or(RecoveryError::Checksum)?);
        let mut seed = Zeroizing::new([0u8; SEED_BYTES]);
        seed.copy_from_slice(&payload[..SEED_BYTES]);
        if !constant_time_eq(&payload[SEED_BYTES..], &kit_checksum(&seed)) {
            return Err(RecoveryError::Checksum);
        }
        Ok(Self(seed))
    }
}

/// Compares two byte strings without branching on their contents. Only the lengths may leak.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = u8::from(left.len() != right.len());
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    std::hint::black_box(difference) == 0
}

impl fmt::Debug for RecoverySeed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoverySeed(..)")
    }
}

fn kit_checksum(seed: &[u8; SEED_BYTES]) -> [u8; KIT_CHECKSUM_BYTES] {
    let digest = domain_digest(RECOVERY_KIT_DOMAIN, &[seed]);
    let mut checksum = [0u8; KIT_CHECKSUM_BYTES];
    checksum.copy_from_slice(&digest[..KIT_CHECKSUM_BYTES]);
    checksum
}

fn crockford_value(character: char) -> Option<u8> {
    let upper = character.to_ascii_uppercase();
    let upper = match upper {
        'O' => '0',
        'I' | 'L' => '1',
        other => other,
    };
    CROCKFORD_ALPHABET
        .iter()
        .position(|candidate| char::from(*candidate) == upper)
        .and_then(|index| u8::try_from(index).ok())
}

fn encode_base32(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer: u16 = 0;
    let mut bits = 0u32;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(CROCKFORD_ALPHABET[usize::from((buffer >> bits) & 0x1f)]);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        output.push(CROCKFORD_ALPHABET[usize::from((buffer << (5 - bits)) & 0x1f)]);
    }
    output
}

/// Decodes base32 values into the kit payload, refusing nonzero padding bits so each payload has
/// one code.
fn decode_base32(values: &[u8]) -> Option<[u8; KIT_PAYLOAD_BYTES]> {
    let mut output = [0u8; KIT_PAYLOAD_BYTES];
    let mut length = 0;
    let mut buffer: u16 = 0;
    let mut bits = 0u32;
    for value in values {
        buffer = (buffer << 5) | u16::from(*value);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            *output.get_mut(length)? = (buffer >> bits) as u8;
            length += 1;
            buffer &= (1 << bits) - 1;
        }
    }
    (length == KIT_PAYLOAD_BYTES && buffer == 0).then_some(output)
}

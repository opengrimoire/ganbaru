//! Signed contact card encoding.
//!
//! Binary layout, big endian: magic (4) | version (1) | public key (32) | name length (1) and
//! name | color (1) | nonce (16) | hint length (1) and hint | fingerprint flag (1) and optional
//! fingerprint (32) | issued at milliseconds (8) | signature (64) over everything before it.

use crate::identity::{PUBLIC_KEY_BYTES, PersonKeyPair, PersonPublicKey, SIGNATURE_BYTES, verify};
use crate::request::CARD_NONCE_BYTES;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use std::fmt;

/// Leading bytes of every binary card.
pub const CARD_MAGIC: &[u8; 4] = b"GBC\x01";
/// Prefix of the pasteable text form.
pub const CARD_TEXT_PREFIX: &str = "GANB-";
/// Upper bound on a binary card.
pub const MAX_CARD_BYTES: usize = 512;
/// Upper bound on the display name in bytes.
pub const MAX_DISPLAY_NAME_BYTES: usize = 100;
/// Upper bound on the endpoint hint in bytes.
pub const MAX_ENDPOINT_HINT_BYTES: usize = 64;

const CARD_VERSION: u8 = 1;
const FINGERPRINT_BYTES: usize = 32;
const MAX_COLOR: u8 = 31;
const VERIFICATION_CODE_GROUPS: usize = 3;
const VERIFICATION_CODE_GROUP_LENGTH: usize = 4;
const CROCKFORD_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A card that failed to decode or verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardError {
    /// The text form has the wrong prefix or is not base64url.
    Text,
    /// The bytes are too short, too long, or exceed a field bound.
    Bounds,
    /// The magic or version is unknown.
    Format,
    /// The display name or hint is not valid UTF-8 or is empty.
    Field,
    /// The signature does not verify against the public key in the card.
    Signature,
}

impl fmt::Display for CardError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => formatter.write_str("contact card text is not recognized"),
            Self::Bounds => formatter.write_str("contact card exceeds its size bounds"),
            Self::Format => formatter.write_str("contact card format is unsupported"),
            Self::Field => formatter.write_str("contact card field is invalid"),
            Self::Signature => formatter.write_str("contact card signature does not verify"),
        }
    }
}

impl std::error::Error for CardError {}

/// Unsigned card contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactCard {
    /// The person's signing key.
    pub public_key: PersonPublicKey,
    /// Display name from the profile.
    pub display_name: String,
    /// Palette slot of the person's avatar color.
    pub color: u8,
    /// Nonce that new contact requests must echo; regenerating the card rotates it.
    pub nonce: [u8; CARD_NONCE_BYTES],
    /// Reachable coordinator endpoint, or empty when the person cannot receive requests.
    pub endpoint_hint: String,
    /// SHA-256 fingerprint of the coordinator's TLS certificate when a hint is present.
    pub coordinator_fingerprint: Option<[u8; FINGERPRINT_BYTES]>,
    /// Signing time in Unix milliseconds.
    pub issued_at_ms: i64,
}

/// A card with its verified encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedCard {
    /// Decoded contents.
    pub card: ContactCard,
    /// Full binary encoding including the signature.
    pub bytes: Vec<u8>,
    /// SHA-256 of the binary encoding.
    pub digest: [u8; 32],
}

impl SignedCard {
    /// Pasteable text form.
    pub fn text(&self) -> String {
        encode_card_text(&self.bytes)
    }

    /// Short code both parties compare out of band; see [`PersonPublicKey::verification_code`].
    pub fn verification_code(&self) -> String {
        self.card.public_key.verification_code()
    }
}

fn validate_fields(card: &ContactCard) -> Result<(), CardError> {
    if card.display_name.is_empty() || card.display_name.len() > MAX_DISPLAY_NAME_BYTES {
        return Err(CardError::Bounds);
    }
    if card.endpoint_hint.len() > MAX_ENDPOINT_HINT_BYTES {
        return Err(CardError::Bounds);
    }
    if card.color > MAX_COLOR {
        return Err(CardError::Bounds);
    }
    if card.display_name.trim().is_empty() {
        return Err(CardError::Field);
    }
    Ok(())
}

fn encode_unsigned(card: &ContactCard) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MAX_CARD_BYTES);
    bytes.extend_from_slice(CARD_MAGIC);
    bytes.push(CARD_VERSION);
    bytes.extend_from_slice(card.public_key.as_bytes());
    bytes.push(card.display_name.len() as u8);
    bytes.extend_from_slice(card.display_name.as_bytes());
    bytes.push(card.color);
    bytes.extend_from_slice(&card.nonce);
    bytes.push(card.endpoint_hint.len() as u8);
    bytes.extend_from_slice(card.endpoint_hint.as_bytes());
    match card.coordinator_fingerprint {
        Some(fingerprint) => {
            bytes.push(1);
            bytes.extend_from_slice(&fingerprint);
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&card.issued_at_ms.to_be_bytes());
    bytes
}

/// Signs a card with the person's key. Fails only when a field exceeds its bound.
pub fn sign_card(card: &ContactCard, key: &PersonKeyPair) -> Result<SignedCard, CardError> {
    validate_fields(card)?;
    if card.public_key != key.public_key() {
        return Err(CardError::Signature);
    }
    let mut bytes = encode_unsigned(card);
    let signature = key.sign(&bytes);
    bytes.extend_from_slice(&signature);
    let digest = Sha256::digest(&bytes).into();
    Ok(SignedCard {
        card: card.clone(),
        bytes,
        digest,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], CardError> {
        let end = self.position.checked_add(length).ok_or(CardError::Bounds)?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(CardError::Bounds)?;
        self.position = end;
        Ok(slice)
    }

    fn take_u8(&mut self) -> Result<u8, CardError> {
        Ok(self.take(1)?[0])
    }

    fn take_string(&mut self, max_bytes: usize) -> Result<String, CardError> {
        let length = usize::from(self.take_u8()?);
        if length > max_bytes {
            return Err(CardError::Bounds);
        }
        let slice = self.take(length)?;
        String::from_utf8(slice.to_vec()).map_err(|_| CardError::Field)
    }
}

/// Decodes and verifies a binary card.
pub fn decode_card(bytes: &[u8]) -> Result<SignedCard, CardError> {
    if bytes.len() > MAX_CARD_BYTES {
        return Err(CardError::Bounds);
    }
    let mut cursor = Cursor { bytes, position: 0 };
    if cursor.take(CARD_MAGIC.len())? != CARD_MAGIC {
        return Err(CardError::Format);
    }
    if cursor.take_u8()? != CARD_VERSION {
        return Err(CardError::Format);
    }
    let public_key = PersonPublicKey::from_slice(cursor.take(PUBLIC_KEY_BYTES)?)
        .map_err(|_| CardError::Bounds)?;
    let display_name = cursor.take_string(MAX_DISPLAY_NAME_BYTES)?;
    let color = cursor.take_u8()?;
    let mut nonce = [0u8; CARD_NONCE_BYTES];
    nonce.copy_from_slice(cursor.take(CARD_NONCE_BYTES)?);
    let endpoint_hint = cursor.take_string(MAX_ENDPOINT_HINT_BYTES)?;
    let coordinator_fingerprint = match cursor.take_u8()? {
        0 => None,
        1 => {
            let mut fingerprint = [0u8; FINGERPRINT_BYTES];
            fingerprint.copy_from_slice(cursor.take(FINGERPRINT_BYTES)?);
            Some(fingerprint)
        }
        _ => return Err(CardError::Format),
    };
    let mut issued = [0u8; 8];
    issued.copy_from_slice(cursor.take(8)?);
    let issued_at_ms = i64::from_be_bytes(issued);
    let signed_length = cursor.position;
    let signature = cursor.take(SIGNATURE_BYTES)?;
    if cursor.position != bytes.len() {
        return Err(CardError::Bounds);
    }
    verify(&public_key, &bytes[..signed_length], signature).map_err(|_| CardError::Signature)?;
    let card = ContactCard {
        public_key,
        display_name,
        color,
        nonce,
        endpoint_hint,
        coordinator_fingerprint,
        issued_at_ms,
    };
    validate_fields(&card)?;
    Ok(SignedCard {
        card,
        bytes: bytes.to_vec(),
        digest: Sha256::digest(bytes).into(),
    })
}

/// Pasteable text form of a binary card.
pub fn encode_card_text(bytes: &[u8]) -> String {
    format!("{CARD_TEXT_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}

/// Decodes and verifies the pasteable text form, tolerating surrounding whitespace.
pub fn decode_card_text(text: &str) -> Result<SignedCard, CardError> {
    let trimmed = text.trim();
    let encoded = trimmed
        .strip_prefix(CARD_TEXT_PREFIX)
        .ok_or(CardError::Text)?;
    if encoded.len() > MAX_CARD_BYTES.div_ceil(3) * 4 {
        return Err(CardError::Bounds);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| CardError::Text)?;
    decode_card(&bytes)
}

/// Crockford base32 verification code from the leading bits of a digest.
pub fn verification_code(digest: &[u8; 32]) -> String {
    let mut code =
        String::with_capacity(VERIFICATION_CODE_GROUPS * (VERIFICATION_CODE_GROUP_LENGTH + 1));
    let mut bits: u32 = 0;
    let mut bit_count = 0;
    let mut emitted = 0;
    for byte in digest {
        bits = (bits << 8) | u32::from(*byte);
        bit_count += 8;
        while bit_count >= 5 && emitted < VERIFICATION_CODE_GROUPS * VERIFICATION_CODE_GROUP_LENGTH
        {
            bit_count -= 5;
            let index = ((bits >> bit_count) & 0x1f) as usize;
            if emitted > 0 && emitted % VERIFICATION_CODE_GROUP_LENGTH == 0 {
                code.push('-');
            }
            code.push(char::from(CROCKFORD_ALPHABET[index]));
            emitted += 1;
        }
        if emitted >= VERIFICATION_CODE_GROUPS * VERIFICATION_CODE_GROUP_LENGTH {
            break;
        }
    }
    code
}

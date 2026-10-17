//! Person identity, signed contact cards, contact request signatures, and trust scopes.
//!
//! The crate is Tauri-free so the card format has one owner and its tests run without
//! platform integration. Platform code stores keys, delivers requests, and persists rows.

mod card;
mod identity;
mod request;
mod trust;

#[cfg(test)]
mod tests;

pub use card::{
    CARD_MAGIC, CARD_TEXT_PREFIX, CardError, ContactCard, MAX_CARD_BYTES, MAX_DISPLAY_NAME_BYTES,
    MAX_ENDPOINT_HINT_BYTES, SignedCard, decode_card, decode_card_text, encode_card_text,
    sign_card, verification_code,
};
pub use identity::{
    PUBLIC_KEY_BYTES, PUBLIC_KEY_TEXT_LENGTH, PersonKeyPair, PersonPublicKey, SIGNATURE_BYTES,
    SignatureError, nonces_match, random_bytes, verify,
};
pub use request::{
    CARD_NONCE_BYTES, decode_nonce, encode_nonce, request_signature_payload,
    status_signature_payload,
};
pub use trust::TrustKind;

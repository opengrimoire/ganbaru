//! Signature payloads for contact requests and status polls.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// Length of the card nonce that a request must echo.
pub const CARD_NONCE_BYTES: usize = 16;

const REQUEST_DOMAIN: &[u8] = b"ganbaru-contact-request-v1\0";
const STATUS_DOMAIN: &[u8] = b"ganbaru-contact-status-v1\0";

/// Payload a requester signs to prove it holds the recipient's current card.
pub fn request_signature_payload(
    recipient_nonce: &[u8; CARD_NONCE_BYTES],
    request_id: &str,
) -> Vec<u8> {
    let mut payload =
        Vec::with_capacity(REQUEST_DOMAIN.len() + CARD_NONCE_BYTES + request_id.len());
    payload.extend_from_slice(REQUEST_DOMAIN);
    payload.extend_from_slice(recipient_nonce);
    payload.extend_from_slice(request_id.as_bytes());
    payload
}

/// Payload a requester signs when polling the state of its request.
pub fn status_signature_payload(request_id: &str, issued_at_ms: i64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(STATUS_DOMAIN.len() + request_id.len() + 8);
    payload.extend_from_slice(STATUS_DOMAIN);
    payload.extend_from_slice(request_id.as_bytes());
    payload.extend_from_slice(&issued_at_ms.to_be_bytes());
    payload
}

/// Base64url text form of a nonce without padding.
pub fn encode_nonce(nonce: &[u8; CARD_NONCE_BYTES]) -> String {
    URL_SAFE_NO_PAD.encode(nonce)
}

/// Parses the text form of a nonce.
pub fn decode_nonce(text: &str) -> Option<[u8; CARD_NONCE_BYTES]> {
    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
    <[u8; CARD_NONCE_BYTES]>::try_from(bytes).ok()
}

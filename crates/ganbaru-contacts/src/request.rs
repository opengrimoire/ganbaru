//! Signature payloads for contact requests and status polls.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{Duration, NaiveDateTime};

/// Length of the card nonce that a request must echo.
pub const CARD_NONCE_BYTES: usize = 16;

/// How long a request stays pending on both sides.
pub const REQUEST_TTL_DAYS: i64 = 7;
/// How far apart two honest devices' clocks may sit when they judge a request's timestamps.
pub const CLOCK_TOLERANCE_DAYS: i64 = 1;

const STORED_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Leading bytes of every contact request signature payload.
pub const REQUEST_SIGNATURE_DOMAIN: &[u8] = b"ganbaru-contact-request-v1\0";
/// Leading bytes of every contact status poll signature payload.
pub const STATUS_SIGNATURE_DOMAIN: &[u8] = b"ganbaru-contact-status-v1\0";

/// Parses a stored timestamp, refusing every text form but the one writers produce. Queries
/// compare these timestamps as text, so another spelling of the same time would sort apart from
/// the time it parses to.
fn parse_stored_time(text: &str) -> Option<NaiveDateTime> {
    let time = NaiveDateTime::parse_from_str(text, STORED_TIME_FORMAT).ok()?;
    (time.format(STORED_TIME_FORMAT).to_string() == text).then_some(time)
}

/// Whether a request's stored timestamps parse and its deadline stays inside the request
/// lifetime plus the clock tolerance, so a replicated row cannot stay pending for longer.
pub fn is_valid_request_window(created_at: &str, expires_at: &str) -> bool {
    let (Some(created), Some(expires)) =
        (parse_stored_time(created_at), parse_stored_time(expires_at))
    else {
        return false;
    };
    created
        .checked_add_signed(Duration::days(REQUEST_TTL_DAYS + CLOCK_TOLERANCE_DAYS))
        .is_some_and(|latest| expires <= latest)
}

/// Payload a requester signs to prove it holds the recipient's current card.
pub fn request_signature_payload(
    recipient_nonce: &[u8; CARD_NONCE_BYTES],
    request_id: &str,
) -> Vec<u8> {
    let mut payload =
        Vec::with_capacity(REQUEST_SIGNATURE_DOMAIN.len() + CARD_NONCE_BYTES + request_id.len());
    payload.extend_from_slice(REQUEST_SIGNATURE_DOMAIN);
    payload.extend_from_slice(recipient_nonce);
    payload.extend_from_slice(request_id.as_bytes());
    payload
}

/// Payload a requester signs when polling the state of its request.
pub fn status_signature_payload(request_id: &str, issued_at_ms: i64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(STATUS_SIGNATURE_DOMAIN.len() + request_id.len() + 8);
    payload.extend_from_slice(STATUS_SIGNATURE_DOMAIN);
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

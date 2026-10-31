//! Reversed enrollment of an always-on anchor server.
//!
//! A server cannot scan a code or dial members, so an admin device dials it. The setup code names
//! the server's endpoints, pins its certificate, and carries a one-time secret. The device proves
//! the secret over a connection pinned to that certificate, without a client certificate, and
//! the secret itself never crosses the wire.

use super::relay::{RELAY_SETUP_TOKEN_BYTES, RelaySetupCode, RelaySetupToken};
use ring::hmac;
use serde::{Deserialize, Serialize};

/// How long a setup secret stays valid.
pub use super::relay::RELAY_SETUP_TOKEN_LIFETIME as ANCHOR_SETUP_LIFETIME;

/// Bytes of a setup proof, an HMAC-SHA-256 tag.
const ANCHOR_PROOF_BYTES: usize = 32;

/// Prefix of the text form an admin can paste.
pub const ANCHOR_SETUP_TEXT_PREFIX: &str = "ganbaru-anchor:";
/// Bytes of the random nonce each side contributes to a proof.
pub const ANCHOR_NONCE_BYTES: usize = 32;
/// Error code for a server older than the device that dialed it.
pub const SERVER_NEEDS_UPDATE_CODE: &str = "server_needs_update";
/// Error code for a device older than the server it dialed.
pub const DEVICE_NEEDS_UPDATE_CODE: &str = "device_needs_update";
/// Error code for a setup secret that is wrong, used, or expired.
pub const SETUP_REFUSED_CODE: &str = "setup_refused";

const ANCHOR_SETUP_MAGIC: &[u8; 4] = b"GBAN";
const PROOF_CONTEXT: &[u8] = b"ganbaru-sync/anchor-setup-proof/v1";
const MAX_SETUP_CODE_BYTES: usize = 4_096;

/// What a server operator hands to an admin device. It shares its binary layout with the relay
/// setup code and differs in its magic and text prefix, so neither parses as the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorSetupCode(pub RelaySetupCode);

impl AnchorSetupCode {
    /// Binary form carried by the QR code.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut bytes = self.0.to_bytes()?;
        bytes
            .get_mut(..ANCHOR_SETUP_MAGIC.len())
            .ok_or_else(|| "anchor setup code is invalid".to_string())?
            .copy_from_slice(ANCHOR_SETUP_MAGIC);
        Ok(bytes)
    }

    /// Parses the binary form.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let rest = bytes
            .strip_prefix(ANCHOR_SETUP_MAGIC)
            .ok_or_else(|| "anchor setup code is invalid".to_string())?;
        let mut relay_form = super::relay::RELAY_SETUP_MAGIC.to_vec();
        relay_form.extend_from_slice(rest);
        RelaySetupCode::from_bytes(&relay_form)
            .map(Self)
            .map_err(|_| "anchor setup code is invalid".to_string())
    }

    /// Text form an admin can paste.
    pub fn to_text(&self) -> Result<String, String> {
        Ok(format!(
            "{ANCHOR_SETUP_TEXT_PREFIX}{}",
            super::relay::encode_relay_bytes(&self.to_bytes()?)
        ))
    }

    /// Parses the text form.
    pub fn from_text(text: &str) -> Result<Self, String> {
        let encoded = text
            .trim()
            .strip_prefix(ANCHOR_SETUP_TEXT_PREFIX)
            .ok_or_else(|| "anchor setup code is invalid".to_string())?;
        Self::from_bytes(&super::relay::decode_relay_bytes(
            "anchor setup code",
            encoded,
            MAX_SETUP_CODE_BYTES,
        )?)
    }
}

/// The space an anchor server runs, which it reports to a device that proved the setup secret.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorSpace {
    pub vault_id: String,
    /// Base64url charter encoding.
    pub charter: String,
    /// Base64url person public key.
    pub person_key: String,
}

impl AnchorSpace {
    /// Checks the fields without trusting them: the receiver verifies the charter against the
    /// vault with [`crate::pairing::PinnedCharter::verify`].
    pub fn validate(&self) -> Result<(), String> {
        super::validate_identifier("vault id", &self.vault_id)?;
        super::decode_sync_charter(&self.charter, &self.person_key).map(drop)
    }
}

/// Which side of a version gap has to update, or `None` when the two can run setup.
pub fn version_gap(local: u16, remote: u16) -> Option<&'static str> {
    match local.cmp(&remote) {
        std::cmp::Ordering::Equal => None,
        std::cmp::Ordering::Less => Some(DEVICE_NEEDS_UPDATE_CODE),
        std::cmp::Ordering::Greater => Some(SERVER_NEEDS_UPDATE_CODE),
    }
}

/// Fills a fresh nonce from the system random source.
pub fn anchor_nonce() -> Result<[u8; ANCHOR_NONCE_BYTES], String> {
    let mut nonce = [0u8; ANCHOR_NONCE_BYTES];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
        .map_err(|_| "generate anchor setup nonce".to_string())?;
    Ok(nonce)
}

/// Decodes a base64url nonce of [`ANCHOR_NONCE_BYTES`] bytes.
pub fn decode_anchor_nonce(text: &str) -> Result<[u8; ANCHOR_NONCE_BYTES], String> {
    super::relay::decode_relay_bytes("anchor nonce", text, ANCHOR_NONCE_BYTES)?
        .try_into()
        .map_err(|_| "anchor nonce is invalid".to_string())
}

/// Decodes a base64url setup proof.
pub fn decode_anchor_proof(text: &str) -> Result<Vec<u8>, String> {
    let proof = super::relay::decode_relay_bytes("anchor proof", text, ANCHOR_PROOF_BYTES)?;
    if proof.len() != ANCHOR_PROOF_BYTES {
        return Err("anchor proof is invalid".to_string());
    }
    Ok(proof)
}

fn proof_key(secret: &[u8; RELAY_SETUP_TOKEN_BYTES]) -> hmac::Key {
    hmac::Key::new(hmac::HMAC_SHA256, secret)
}

fn proof_message(
    server_nonce: &[u8; ANCHOR_NONCE_BYTES],
    client_nonce: &[u8; ANCHOR_NONCE_BYTES],
    certificate_fingerprint: &[u8; 32],
) -> Vec<u8> {
    let mut message = Vec::with_capacity(PROOF_CONTEXT.len() + 3 * ANCHOR_NONCE_BYTES);
    message.extend_from_slice(PROOF_CONTEXT);
    message.extend_from_slice(server_nonce);
    message.extend_from_slice(client_nonce);
    message.extend_from_slice(certificate_fingerprint);
    message
}

/// The proof a device sends that it holds `secret`, bound to both nonces and the certificate it
/// pinned, so it cannot be replayed or moved to another server.
pub fn anchor_setup_proof(
    secret: &RelaySetupToken,
    server_nonce: &[u8; ANCHOR_NONCE_BYTES],
    client_nonce: &[u8; ANCHOR_NONCE_BYTES],
    certificate_fingerprint: &[u8; 32],
) -> Vec<u8> {
    hmac::sign(
        &proof_key(secret.as_bytes()),
        &proof_message(server_nonce, client_nonce, certificate_fingerprint),
    )
    .as_ref()
    .to_vec()
}

/// Checks a proof in constant time.
pub fn verify_anchor_setup_proof(
    secret: &RelaySetupToken,
    server_nonce: &[u8; ANCHOR_NONCE_BYTES],
    client_nonce: &[u8; ANCHOR_NONCE_BYTES],
    certificate_fingerprint: &[u8; 32],
    proof: &[u8],
) -> bool {
    hmac::verify(
        &proof_key(secret.as_bytes()),
        &proof_message(server_nonce, client_nonce, certificate_fingerprint),
        proof,
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code() -> AnchorSetupCode {
        AnchorSetupCode(RelaySetupCode {
            endpoints: vec!["sync.example.org:43823".to_string()],
            certificate_fingerprint: [7; 32],
            token: RelaySetupToken::from_bytes([9; RELAY_SETUP_TOKEN_BYTES]),
        })
    }

    #[test]
    fn a_setup_code_round_trips_and_is_not_a_relay_code() {
        let text = code().to_text().unwrap();
        assert!(text.starts_with(ANCHOR_SETUP_TEXT_PREFIX));
        assert_eq!(AnchorSetupCode::from_text(&text).unwrap(), code());
        let bytes = code().to_bytes().unwrap();
        assert_eq!(AnchorSetupCode::from_bytes(&bytes).unwrap(), code());
        assert!(RelaySetupCode::from_bytes(&bytes).is_err());
        let relay = code().0.to_bytes().unwrap();
        assert!(AnchorSetupCode::from_bytes(&relay).is_err());
    }

    #[test]
    fn a_proof_is_bound_to_the_secret_both_nonces_and_the_certificate() {
        let secret = RelaySetupToken::from_bytes([1; RELAY_SETUP_TOKEN_BYTES]);
        let proof = anchor_setup_proof(&secret, &[2; 32], &[3; 32], &[4; 32]);
        assert!(verify_anchor_setup_proof(
            &secret, &[2; 32], &[3; 32], &[4; 32], &proof
        ));
        let other = RelaySetupToken::from_bytes([5; RELAY_SETUP_TOKEN_BYTES]);
        assert!(!verify_anchor_setup_proof(
            &other, &[2; 32], &[3; 32], &[4; 32], &proof
        ));
        assert!(!verify_anchor_setup_proof(
            &secret, &[6; 32], &[3; 32], &[4; 32], &proof
        ));
        assert!(!verify_anchor_setup_proof(
            &secret, &[2; 32], &[6; 32], &[4; 32], &proof
        ));
        assert!(!verify_anchor_setup_proof(
            &secret, &[2; 32], &[3; 32], &[6; 32], &proof
        ));
    }
}

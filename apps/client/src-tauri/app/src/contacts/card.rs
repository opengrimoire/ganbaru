//! The signed local contact card, its LAN hint, and QR rendering.

use super::identity::LocalIdentity;
use super::{CardView, ContactsError, LocalCardView};
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::{decode_qr_bytes_luma, qr_matrix_for_bytes, unix_time_ms};
use ganbaru_contacts::{
    CARD_MAGIC, CARD_NONCE_BYTES, ContactCard, MAX_DISPLAY_NAME_BYTES, MAX_ENDPOINT_HINT_BYTES,
    SignedCard, decode_card, encode_nonce, random_bytes, sign_card,
};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, Runtime};

const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const PROFILE_KEY: &str = "profile";
const DISPLAY_NAME_KEY: &str = "displayName";
const COLOR_KEY: &str = "color";
const DEFAULT_COLOR: u8 = 30;
const MAX_COLOR: u8 = 31;

/// The profile fields a card carries.
pub(crate) struct CardProfile {
    pub display_name: String,
    pub color: u8,
}

/// Where other people can deliver requests for this vault.
pub(crate) struct DeliveryHint {
    pub endpoint: String,
    pub coordinator_fingerprint: Option<[u8; 32]>,
}

impl DeliveryHint {
    fn unreachable() -> Self {
        Self {
            endpoint: String::new(),
            coordinator_fingerprint: None,
        }
    }
}

/// Reads the profile display name and color from the active vault configuration.
pub(crate) fn read_profile<R: Runtime>(app: &AppHandle<R>) -> Result<CardProfile, ContactsError> {
    let raw = crate::vault::read_active_config_bounded(app, MAX_CONFIG_BYTES)?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| ContactsError::Failed(format!("parse vault configuration: {error}")))?;
    let profile = value.get(PROFILE_KEY);
    let display_name = profile
        .and_then(|profile| profile.get(DISPLAY_NAME_KEY))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| truncate_at_char_boundary(name, MAX_DISPLAY_NAME_BYTES))
        .ok_or_else(|| {
            ContactsError::ProfileIncomplete("set a display name in the profile first".to_string())
        })?;
    let color = profile
        .and_then(|profile| profile.get(COLOR_KEY))
        .and_then(serde_json::Value::as_u64)
        .and_then(|color| u8::try_from(color).ok())
        .filter(|color| *color <= MAX_COLOR)
        .unwrap_or(DEFAULT_COLOR);
    Ok(CardProfile {
        display_name,
        color,
    })
}

fn truncate_at_char_boundary(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end().to_string()
}

/// Resolves the endpoint other people can use: the coordinator pin on linked devices, the
/// running coordinator on desktop, or nothing when requests cannot be received right now.
pub(crate) fn resolve_delivery_hint<R: Runtime>(app: &AppHandle<R>) -> DeliveryHint {
    let manager = app.state::<PairingManager>();
    if let Ok(Some(pin)) = manager.coordinator_pin() {
        return bounded_hint(pin.endpoint, &pin.certificate_fingerprint);
    }
    #[cfg(desktop)]
    {
        use crate::vault::handoff::pairing::certificate_fingerprint;
        let lifecycle = app.state::<crate::vault::handoff::CoordinatorLifecycle>();
        if let Ok(Some(endpoint)) = lifecycle.endpoint()
            && let Ok((_, identity)) = manager.identity()
        {
            let fingerprint = certificate_fingerprint(identity.certificate.as_ref());
            return bounded_hint(endpoint.to_string(), &fingerprint);
        }
    }
    DeliveryHint::unreachable()
}

fn bounded_hint(endpoint: String, fingerprint_hex: &str) -> DeliveryHint {
    let Some(fingerprint) = super::store::decode_hex_digest(fingerprint_hex) else {
        return DeliveryHint::unreachable();
    };
    if endpoint.is_empty() || endpoint.len() > MAX_ENDPOINT_HINT_BYTES {
        return DeliveryHint::unreachable();
    }
    DeliveryHint {
        endpoint,
        coordinator_fingerprint: Some(fingerprint),
    }
}

/// Signs the current card for this identity with the profile and hint as they are right now.
pub(crate) fn sign_local_card<R: Runtime>(
    app: &AppHandle<R>,
    identity: &LocalIdentity,
) -> Result<SignedCard, ContactsError> {
    let key = identity.signing_key()?;
    let profile = read_profile(app)?;
    let hint = resolve_delivery_hint(app);
    let card = ContactCard {
        public_key: identity.public_key,
        display_name: profile.display_name,
        color: profile.color,
        nonce: identity.card_nonce,
        endpoint_hint: hint.endpoint,
        coordinator_fingerprint: hint.coordinator_fingerprint,
        issued_at_ms: unix_time_ms(),
    };
    sign_card(&card, key)
        .map_err(|error| ContactsError::Failed(format!("sign contact card: {error}")))
}

pub(crate) fn local_card_view<R: Runtime>(
    app: &AppHandle<R>,
    identity: &LocalIdentity,
) -> Result<LocalCardView, ContactsError> {
    if identity.key.is_none() {
        return Ok(LocalCardView {
            identity: identity.view(),
            card: None,
        });
    }
    let signed = sign_local_card(app, identity)?;
    let qr = qr_matrix_for_bytes(&signed.bytes)?;
    Ok(LocalCardView {
        identity: identity.view(),
        card: Some(CardView {
            text: signed.text(),
            qr,
            verification_code: signed.verification_code(),
            display_name: signed.card.display_name,
            color: signed.card.color,
            endpoint_hint: signed.card.endpoint_hint,
        }),
    })
}

/// Rotates the card nonce so every card issued so far stops being accepted.
pub(crate) async fn rotate_card(
    pool: &SqlitePool,
    mut identity: LocalIdentity,
) -> Result<LocalIdentity, ContactsError> {
    let mut card_nonce = [0u8; CARD_NONCE_BYTES];
    random_bytes(&mut card_nonce)
        .map_err(|error| ContactsError::Failed(format!("generate card nonce: {error}")))?;
    let now = super::store::format_time(super::store::now());
    let revision: i64 = sqlx::query_scalar(
        "UPDATE contacts_local_identity
         SET card_nonce = ?, card_revision = card_revision + 1, updated_at = ?
         WHERE singleton = 1
         RETURNING card_revision",
    )
    .bind(encode_nonce(&card_nonce))
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("rotate contact card: {error}"))?;
    identity.card_nonce = card_nonce;
    identity.card_revision = revision;
    Ok(identity)
}

/// Finds a contact card QR code in a grayscale frame and returns its text form.
pub(crate) fn decode_card_qr(
    width: usize,
    height: usize,
    luma: &[u8],
) -> Result<String, ContactsError> {
    let payload = decode_qr_bytes_luma(width, height, luma, CARD_MAGIC)
        .map_err(ContactsError::InvalidCard)?;
    let signed =
        decode_card(&payload).map_err(|error| ContactsError::InvalidCard(error.to_string()))?;
    Ok(signed.text())
}

//! The signed local contact card, its LAN hint, and QR rendering.

use super::identity::LocalIdentity;
use super::{CardView, ContactsError, LocalCardView};
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::{decode_qr_bytes_luma, qr_matrix_for_bytes, unix_time_ms};
use ganbaru_contacts::{
    CARD_MAGIC, CARD_NONCE_BYTES, ContactCard, MAX_DISPLAY_NAME_BYTES, MAX_ENDPOINT_HINT_BYTES,
    SignedCard, decode_card, encode_nonce, is_disallowed_display_character, random_bytes,
    sign_card,
};
use ganbaru_sync::domains::contacts::MAX_CONTACT_CARD_REVISION;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager, Runtime};

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

/// Reads the profile display name and color from the vault preference view.
pub(crate) async fn read_profile<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<CardProfile, ContactsError> {
    let view = crate::vault::preferences::effective_view(app).await?;
    profile_from_view(&view)
}

/// The card profile of a preference view, refused when the display name is missing.
fn profile_from_view(view: &serde_json::Value) -> Result<CardProfile, ContactsError> {
    let profile = view.get(PROFILE_KEY);
    let display_name = profile
        .and_then(|profile| profile.get(DISPLAY_NAME_KEY))
        .and_then(serde_json::Value::as_str)
        .map(shareable_display_name)
        .filter(|name| !name.is_empty())
        .map(|name| truncate_at_char_boundary(&name, MAX_DISPLAY_NAME_BYTES))
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

/// The profile name without characters every card decoder refuses. Profile settings reject them
/// on entry, so this only matters for a name written some other way, which still gets a card.
fn shareable_display_name(name: &str) -> String {
    name.chars()
        .filter(|character| !is_disallowed_display_character(*character))
        .collect::<String>()
        .trim()
        .to_string()
}

fn truncate_at_char_boundary(value: &str, max_bytes: usize) -> String {
    let mut end = value.len().min(max_bytes);
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end().to_string()
}

/// Resolves the endpoint other people can use: the coordinator pin on linked devices, the
/// running coordinator's port on the preferred local address on desktop, or nothing when
/// requests cannot be received right now.
pub(crate) fn resolve_delivery_hint<R: Runtime>(app: &AppHandle<R>) -> DeliveryHint {
    let manager = app.state::<PairingManager>();
    if let Ok(Some(pin)) = manager.coordinator_pin() {
        return bounded_hint(
            pin.current_endpoint().to_string(),
            &pin.certificate_fingerprint,
        );
    }
    #[cfg(desktop)]
    {
        use crate::vault::handoff::pairing::certificate_fingerprint;
        let lifecycle = app.state::<crate::vault::handoff::CoordinatorLifecycle>();
        if let Ok(Some(endpoint)) = lifecycle.reportable_endpoint()
            && let Ok(certificate) = manager.certificate()
        {
            let fingerprint = certificate_fingerprint(certificate.as_ref());
            return bounded_hint(endpoint.to_string(), &fingerprint);
        }
    }
    DeliveryHint::unreachable()
}

fn bounded_hint(endpoint: String, fingerprint_hex: &str) -> DeliveryHint {
    let Some(fingerprint) =
        ganbaru_handoff::protocol::decode_certificate_fingerprint(fingerprint_hex)
    else {
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
pub(crate) async fn sign_local_card<R: Runtime>(
    app: &AppHandle<R>,
    identity: &LocalIdentity,
) -> Result<SignedCard, ContactsError> {
    let nonce = identity.readable_nonce()?;
    let profile = read_profile(app).await?;
    let key = identity.signing_key()?;
    let hint = resolve_delivery_hint(app);
    let card = ContactCard {
        public_key: identity.public_key,
        display_name: profile.display_name,
        color: profile.color,
        nonce,
        endpoint_hint: hint.endpoint,
        coordinator_fingerprint: hint.coordinator_fingerprint,
        issued_at_ms: unix_time_ms(),
    };
    sign_card(&card, key)
        .map_err(|error| ContactsError::Failed(format!("sign contact card: {error}")))
}

pub(crate) async fn local_card_view<R: Runtime>(
    app: &AppHandle<R>,
    identity: &LocalIdentity,
) -> Result<LocalCardView, ContactsError> {
    if identity.key.is_none() {
        return Ok(LocalCardView {
            identity: identity.view(),
            card: None,
        });
    }
    let signed = sign_local_card(app, identity).await?;
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

/// Rotates the card nonce so every card issued so far stops being accepted. The revision advances
/// up to `MAX_CONTACT_CARD_REVISION` and stays there, so a replicated revision at the bound never
/// stops a nonce from rotating. The stored revision is lowered before it is incremented, which
/// keeps the arithmetic in integers for any value the column holds.
pub(crate) async fn rotate_card(
    pool: &SqlitePool,
    mut identity: LocalIdentity,
) -> Result<LocalIdentity, ContactsError> {
    let mut card_nonce = [0u8; CARD_NONCE_BYTES];
    random_bytes(&mut card_nonce)
        .map_err(|error| ContactsError::Failed(format!("generate card nonce: {error}")))?;
    let now = super::store::format_time(super::store::now());
    let revision: i64 = sqlx::query_scalar(
        "UPDATE contact_card
         SET card_nonce = ?, card_revision = MIN(card_revision, ? - 1) + 1, updated_at = ?
         WHERE key = 'card'
         RETURNING card_revision",
    )
    .bind(encode_nonce(&card_nonce))
    .bind(MAX_CONTACT_CARD_REVISION)
    .bind(now)
    .fetch_one(pool)
    .await
    .map_err(|error| format!("rotate contact card: {error}"))?;
    identity.card_nonce = Some(card_nonce);
    identity.card_revision = revision;
    Ok(identity)
}

/// Rotates the card when its stored nonce does not decode, so the fresh value replicates at a
/// higher revision. A readable identity passes through unchanged.
pub(crate) async fn heal_unreadable_nonce(
    pool: &SqlitePool,
    identity: LocalIdentity,
    _stored_nonce: &str,
) -> Result<LocalIdentity, ContactsError> {
    if identity.card_nonce.is_some() {
        return Ok(identity);
    }
    rotate_card(pool, identity).await
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

#[cfg(test)]
mod tests {
    use super::super::identity::{
        ExistingIdentity, IdentityRow, existing_identity, identity_from_row, load_identity_row,
    };
    use super::super::tests::migrated_memory_pool;
    use super::*;
    use ganbaru_contacts::{PersonKeyPair, decode_nonce};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const AT: &str = "2026-10-22T10:51:41.000Z";
    const UNREADABLE_NONCE: &str = "!!!!!!!!!!!!!!!!!!!!!!";

    /// A vault whose shared card row holds `revision`, and an identity reading it. The value
    /// trigger is dropped so a test can hold a row no current write admits.
    async fn card_at_revision(revision: i64) -> (SqlitePool, LocalIdentity) {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql("DROP TRIGGER sync_values_contact_card_insert")
            .execute(&pool)
            .await
            .unwrap();
        let mut card_nonce = [0u8; CARD_NONCE_BYTES];
        random_bytes(&mut card_nonce).unwrap();
        sqlx::query(
            "INSERT INTO contact_card (key, card_nonce, card_revision, updated_at)
             VALUES ('card', ?, ?, ?)",
        )
        .bind(encode_nonce(&card_nonce))
        .bind(revision)
        .bind(AT)
        .execute(&pool)
        .await
        .unwrap();
        let (_, key) = PersonKeyPair::generate().unwrap();
        let identity = LocalIdentity {
            public_key: key.public_key(),
            card_nonce: Some(card_nonce),
            card_revision: revision,
            key: None,
        };
        (pool, identity)
    }

    async fn stored_card(pool: &SqlitePool) -> (String, i64) {
        sqlx::query_as("SELECT card_nonce, card_revision FROM contact_card WHERE key = 'card'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// A vault whose stored card holds a nonce no reader can decode at `revision`, with its
    /// identity row.
    async fn unreadable_card_row(revision: i64) -> (SqlitePool, IdentityRow) {
        let (pool, identity) = card_at_revision(revision).await;
        sqlx::query("UPDATE contact_card SET card_nonce = ?")
            .bind(UNREADABLE_NONCE)
            .execute(&pool)
            .await
            .unwrap();
        insert_identity_row(&pool, &identity).await;
        let row = load_identity_row(&pool).await.unwrap().unwrap();
        (pool, row)
    }

    async fn insert_identity_row(pool: &SqlitePool, identity: &LocalIdentity) {
        sqlx::query(
            "INSERT INTO contacts_local_identity (singleton, public_key, created_at)
             VALUES (1, ?, ?)",
        )
        .bind(identity.public_key.to_text())
        .bind(AT)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn no_key_copy() -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    fn ready(existing: ExistingIdentity) -> LocalIdentity {
        match existing {
            ExistingIdentity::Ready(identity) => identity,
            ExistingIdentity::Missing(_) => panic!("the identity was reported missing"),
        }
    }

    #[tokio::test]
    async fn existing_identity_heals_an_unreadable_card() {
        let (pool, _) = unreadable_card_row(5).await;

        let identity = ready(existing_identity(&pool, no_key_copy).await.unwrap());

        let (nonce, revision) = stored_card(&pool).await;
        assert!(decode_nonce(&nonce).is_some(), "{nonce}");
        assert_eq!(revision, 6);
        assert_eq!(identity.card_nonce, decode_nonce(&nonce));
        assert_eq!(identity.card_revision, 6);
    }

    #[tokio::test]
    async fn existing_identity_takes_a_readable_card_without_a_second_read() {
        let (pool, identity) = card_at_revision(5).await;
        insert_identity_row(&pool, &identity).await;
        let stored = stored_card(&pool).await;
        let key_reads = AtomicUsize::new(0);
        let reads = &key_reads;

        let loaded = ready(
            existing_identity(&pool, move || async move {
                reads.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            })
            .await
            .unwrap(),
        );

        assert_eq!(stored_card(&pool).await, stored);
        assert_eq!(loaded.card_nonce, identity.card_nonce);
        assert_eq!(key_reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn existing_identity_reports_a_vault_without_one() {
        let (pool, _) = card_at_revision(1).await;

        let existing = existing_identity(&pool, no_key_copy).await.unwrap();

        assert!(matches!(existing, ExistingIdentity::Missing(_)));
    }

    #[tokio::test]
    async fn heal_keeps_a_peer_nonce_merged_after_the_reload() {
        let (pool, _) = unreadable_card_row(5).await;
        let mut peer_nonce = [0u8; CARD_NONCE_BYTES];
        random_bytes(&mut peer_nonce).unwrap();
        let peer_text = encode_nonce(&peer_nonce);
        let key_reads = AtomicUsize::new(0);
        let (reads, peer, writer) = (&key_reads, peer_text.as_str(), &pool);

        // The key copy is read after the card row on each pass, so the second read stands in for
        // a peer's heal that merges between the locked reload and the repair.
        let identity = ready(
            existing_identity(&pool, move || async move {
                if reads.fetch_add(1, Ordering::SeqCst) == 1 {
                    sqlx::query(
                        "UPDATE contact_card SET card_nonce = ?, card_revision = 6
                         WHERE key = 'card'",
                    )
                    .bind(peer)
                    .execute(writer)
                    .await
                    .unwrap();
                }
                Ok(None)
            })
            .await
            .unwrap(),
        );

        assert_eq!(stored_card(&pool).await, (peer_text, 6));
        assert_eq!(identity.card_nonce, Some(peer_nonce));
        assert_eq!(identity.card_revision, 6);
    }

    #[tokio::test]
    async fn heal_stores_a_decodable_nonce_at_a_higher_revision() {
        let (pool, row) = unreadable_card_row(5).await;

        let identity = identity_from_row(row, None);
        assert!(
            identity.is_ok(),
            "an unreadable nonce failed the identity: {:?}",
            identity.as_ref().err()
        );
        let identity = identity.unwrap();
        assert_eq!(identity.card_nonce, None);
        assert!(matches!(
            identity.readable_nonce(),
            Err(ContactsError::Failed(_))
        ));
        let healed = heal_unreadable_nonce(&pool, identity, UNREADABLE_NONCE)
            .await
            .unwrap();

        let (nonce, revision) = stored_card(&pool).await;
        assert!(decode_nonce(&nonce).is_some(), "{nonce}");
        assert_eq!(revision, 6);
        assert_eq!(healed.card_revision, revision);
        assert_eq!(healed.card_nonce, decode_nonce(&nonce));
    }

    #[tokio::test]
    async fn heal_leaves_a_readable_card_alone() {
        let (pool, identity) = card_at_revision(5).await;
        let stored = stored_card(&pool).await;

        let healed = heal_unreadable_nonce(&pool, identity, &stored.0)
            .await
            .unwrap();

        assert_eq!(stored_card(&pool).await, stored);
        assert_eq!(healed.card_revision, 5);
    }

    #[tokio::test]
    async fn rotate_card_saturates_at_revision_bound() {
        let (pool, identity) = card_at_revision(MAX_CONTACT_CARD_REVISION).await;
        let issued = identity.card_nonce;

        let rotated = rotate_card(&pool, identity).await.unwrap();

        assert_ne!(rotated.card_nonce, issued);
        assert_eq!(rotated.card_revision, MAX_CONTACT_CARD_REVISION);
        assert_eq!(
            stored_card(&pool).await,
            (
                encode_nonce(&rotated.card_nonce.unwrap()),
                MAX_CONTACT_CARD_REVISION
            )
        );
    }

    #[tokio::test]
    async fn rotate_card_replaces_a_nonce_held_at_the_largest_integer() {
        let (pool, identity) = card_at_revision(i64::MAX).await;
        let issued = identity.card_nonce;

        let rotated = rotate_card(&pool, identity).await;

        assert!(
            rotated.is_ok(),
            "rotation failed: {:?}",
            rotated.as_ref().err()
        );
        let rotated = rotated.unwrap();
        assert_ne!(rotated.card_nonce, issued);
        assert_eq!(rotated.card_revision, MAX_CONTACT_CARD_REVISION);
    }

    #[tokio::test]
    async fn rotate_card_advances_the_revision_below_the_bound() {
        let (pool, identity) = card_at_revision(MAX_CONTACT_CARD_REVISION - 1).await;

        let rotated = rotate_card(&pool, identity).await.unwrap();

        assert_eq!(rotated.card_revision, MAX_CONTACT_CARD_REVISION);
    }

    #[test]
    fn profile_names_drop_characters_card_decoders_refuse_and_keep_joiners() {
        assert_eq!(shareable_display_name(" Ana\u{202E}lia\u{200B} "), "Analia");
        assert_eq!(shareable_display_name("\u{2066}\u{2069}"), "");
        assert_eq!(shareable_display_name("👩\u{200D}💻"), "👩\u{200D}💻");
        assert_eq!(shareable_display_name("می\u{200C}خواهم"), "می\u{200C}خواهم");
    }

    #[test]
    fn preferences_card_profile_reads_the_view_and_defaults_the_color() {
        let profile = profile_from_view(&serde_json::json!({
            "profile": { "displayName": " Ada ", "color": 7 },
        }))
        .unwrap();
        assert_eq!((profile.display_name.as_str(), profile.color), ("Ada", 7));

        let profile =
            profile_from_view(&serde_json::json!({ "profile": { "displayName": "Ada" } })).unwrap();
        assert_eq!(profile.color, DEFAULT_COLOR);

        for view in [
            serde_json::json!({}),
            serde_json::json!({ "profile": { "displayName": "\u{2066}" } }),
        ] {
            assert!(matches!(
                profile_from_view(&view),
                Err(ContactsError::ProfileIncomplete(_))
            ));
        }
    }
}

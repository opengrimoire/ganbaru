use super::identity::LocalIdentity;
use super::requests::{
    self, CARD_REVOKED_CODE, INVALID_REQUEST_CODE, MAX_PENDING_RECEIVED, PeerReply, StatusOutcome,
};
use super::store::{self, ContactState, RequestDirection, RequestState, TrustGrant};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_contacts::{
    CARD_NONCE_BYTES, ContactCard, PersonKeyPair, SignedCard, TrustKind, encode_nonce,
    random_bytes, request_signature_payload, sign_card, status_signature_payload,
};
use ganbaru_db::run_migrations;
use sqlx::SqlitePool;

async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}

struct Person {
    key: PersonKeyPair,
    identity: LocalIdentity,
    name: &'static str,
}

impl Person {
    fn new(name: &'static str) -> Self {
        let (_, key) = PersonKeyPair::generate().unwrap();
        let mut card_nonce = [0u8; CARD_NONCE_BYTES];
        random_bytes(&mut card_nonce).unwrap();
        let identity = LocalIdentity {
            public_key: key.public_key(),
            card_nonce,
            card_revision: 1,
            key: None,
        };
        Self {
            key,
            identity,
            name,
        }
    }

    fn card(&self) -> SignedCard {
        sign_card(
            &ContactCard {
                public_key: self.key.public_key(),
                display_name: self.name.to_string(),
                color: 7,
                nonce: self.identity.card_nonce,
                endpoint_hint: "127.0.0.1:43821".to_string(),
                coordinator_fingerprint: Some([9u8; 32]),
                issued_at_ms: 1_700_000_000_000,
            },
            &self.key,
        )
        .unwrap()
    }

    fn request_signature(
        &self,
        recipient_nonce: &[u8; CARD_NONCE_BYTES],
        request_id: &str,
    ) -> String {
        let payload = request_signature_payload(recipient_nonce, request_id);
        URL_SAFE_NO_PAD.encode(self.key.sign(&payload))
    }

    fn status_signature(&self, request_id: &str, issued_at_ms: i64) -> String {
        let payload = status_signature_payload(request_id, issued_at_ms);
        URL_SAFE_NO_PAD.encode(self.key.sign(&payload))
    }
}

async fn deliver(
    pool: &SqlitePool,
    recipient: &Person,
    requester: &Person,
    request_id: &str,
) -> PeerReply {
    requests::receive_request(
        pool,
        &recipient.identity,
        store::now(),
        &encode_nonce(&recipient.identity.card_nonce),
        &requester.card().text(),
        request_id,
        &requester.request_signature(&recipient.identity.card_nonce, request_id),
    )
    .await
    .unwrap()
}

fn received(request_id: &str) -> PeerReply {
    PeerReply::Received {
        request_id: request_id.to_string(),
    }
}

#[tokio::test]
async fn verified_request_is_stored_as_pending_received() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");

    assert_eq!(
        deliver(&pool, &recipient, &requester, "contact-one").await,
        received("contact-one")
    );

    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.direction, "received");
    assert_eq!(row.state, "pending");
    assert_eq!(row.public_key, requester.key.public_key().to_text());
    assert_eq!(row.display_name, "Requester");
    assert_eq!(row.endpoint_hint, "127.0.0.1:43821");
    assert_eq!(row.coordinator_fingerprint.len(), 64);
    let view = row.view().unwrap();
    assert_eq!(view.verification_code, requester.card().verification_code());
}

#[tokio::test]
async fn stale_nonce_is_rejected_as_revoked_without_a_row() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    let mut old_nonce = [0u8; CARD_NONCE_BYTES];
    random_bytes(&mut old_nonce).unwrap();

    let reply = requests::receive_request(
        &pool,
        &recipient.identity,
        store::now(),
        &encode_nonce(&old_nonce),
        &requester.card().text(),
        "contact-old",
        &requester.request_signature(&old_nonce, "contact-old"),
    )
    .await
    .unwrap();

    assert!(matches!(
        reply,
        PeerReply::Rejected {
            code: CARD_REVOKED_CODE,
            retryable: false,
            ..
        }
    ));
    assert!(store::list_requests(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn request_signed_for_another_id_is_invalid() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");

    let reply = requests::receive_request(
        &pool,
        &recipient.identity,
        store::now(),
        &encode_nonce(&recipient.identity.card_nonce),
        &requester.card().text(),
        "contact-b",
        &requester.request_signature(&recipient.identity.card_nonce, "contact-a"),
    )
    .await
    .unwrap();

    assert!(matches!(
        reply,
        PeerReply::Rejected {
            code: INVALID_REQUEST_CODE,
            ..
        }
    ));
    assert!(store::list_requests(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn blocked_requester_gets_the_same_reply_and_no_row() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");

    let open_reply = deliver(&pool, &recipient, &requester, "contact-first").await;
    requests::block_person(&pool, &requester.key.public_key().to_text(), store::now())
        .await
        .unwrap();
    let blocked_reply = deliver(&pool, &recipient, &requester, "contact-second").await;

    assert_eq!(open_reply, received("contact-first"));
    assert_eq!(blocked_reply, received("contact-second"));
    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "blocking leaves only the declined first request"
    );
    assert_eq!(rows[0].state, "declined");
    let contacts = store::list_contacts(&pool).await.unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].state, "blocked");
    assert_eq!(contacts[0].display_name, "Requester");

    let status = requests::request_state(
        &pool,
        "contact-first",
        &requester.key.public_key().to_text(),
        0,
        &requester.status_signature("contact-first", 0),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusOutcome::Pending);
}

#[tokio::test]
async fn newer_request_from_the_same_person_replaces_the_pending_one() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");

    deliver(&pool, &recipient, &requester, "contact-first").await;
    deliver(&pool, &recipient, &requester, "contact-second").await;

    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "contact-second");
}

#[tokio::test]
async fn request_id_owned_by_another_person_is_invalid() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let first = Person::new("First");
    let second = Person::new("Second");

    deliver(&pool, &recipient, &first, "contact-shared").await;
    let reply = deliver(&pool, &recipient, &second, "contact-shared").await;

    assert!(matches!(
        reply,
        PeerReply::Rejected {
            code: INVALID_REQUEST_CODE,
            ..
        }
    ));
    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].public_key, first.key.public_key().to_text());
}

#[tokio::test]
async fn pending_received_requests_are_capped() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let total = usize::try_from(MAX_PENDING_RECEIVED).unwrap() + 3;
    let requesters: Vec<Person> = (0..total).map(|_| Person::new("Many")).collect();

    for (index, requester) in requesters.iter().enumerate() {
        let id = format!("contact-{index:03}");
        assert_eq!(
            deliver(&pool, &recipient, requester, &id).await,
            received(&id)
        );
    }

    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), total);
    let pending = rows.iter().filter(|row| row.state == "pending").count();
    let expired: Vec<&str> = rows
        .iter()
        .filter(|row| row.state == "expired")
        .map(|row| row.id.as_str())
        .collect();
    assert_eq!(pending, usize::try_from(MAX_PENDING_RECEIVED).unwrap());
    assert_eq!(expired.len(), 3);
    assert!(
        expired.contains(&"contact-000"),
        "the oldest rows expire first"
    );
}

#[tokio::test]
async fn self_addressed_request_is_invalid() {
    let pool = migrated_memory_pool().await;
    let person = Person::new("Me");

    let reply = deliver(&pool, &person, &person, "contact-self").await;

    assert!(matches!(
        reply,
        PeerReply::Rejected {
            code: INVALID_REQUEST_CODE,
            ..
        }
    ));
}

#[tokio::test]
async fn status_with_a_bad_signature_is_invalid_and_unknown_ids_look_pending() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    let other = Person::new("Other");
    deliver(&pool, &recipient, &requester, "contact-one").await;

    let forged = requests::request_state(
        &pool,
        "contact-one",
        &requester.key.public_key().to_text(),
        5,
        &other.status_signature("contact-one", 5),
    )
    .await
    .unwrap();
    let wrong_time = requests::request_state(
        &pool,
        "contact-one",
        &requester.key.public_key().to_text(),
        6,
        &requester.status_signature("contact-one", 5),
    )
    .await
    .unwrap();
    let unknown = requests::request_state(
        &pool,
        "contact-missing",
        &requester.key.public_key().to_text(),
        5,
        &requester.status_signature("contact-missing", 5),
    )
    .await
    .unwrap();
    let other_person = requests::request_state(
        &pool,
        "contact-one",
        &other.key.public_key().to_text(),
        5,
        &other.status_signature("contact-one", 5),
    )
    .await
    .unwrap();
    let genuine = requests::request_state(
        &pool,
        "contact-one",
        &requester.key.public_key().to_text(),
        5,
        &requester.status_signature("contact-one", 5),
    )
    .await
    .unwrap();

    assert_eq!(forged, StatusOutcome::Invalid);
    assert_eq!(wrong_time, StatusOutcome::Invalid);
    assert_eq!(unknown, StatusOutcome::Pending);
    assert_eq!(other_person, StatusOutcome::Pending);
    assert_eq!(genuine, StatusOutcome::Pending);
}

#[tokio::test]
async fn accepting_creates_an_active_contact_with_the_chosen_trust() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    let now = store::now();

    requests::accept_request(
        &pool,
        "contact-one",
        row.revision,
        TrustKind::SevenDays,
        TrustKind::UntilRevoked,
        now,
    )
    .await
    .unwrap();

    let contacts_rows = store::list_contacts(&pool).await.unwrap();
    assert_eq!(contacts_rows.len(), 1);
    let contact = contacts_rows[0].clone().view().unwrap();
    assert_eq!(contact.state, ContactState::Active);
    assert_eq!(contact.display_name, "Requester");
    assert_eq!(contact.color, 7);
    assert_eq!(contact.invite_trust, TrustKind::SevenDays);
    assert_eq!(
        contact.invite_trust_expires_at,
        TrustGrant::new(TrustKind::SevenDays, now).expires_at
    );
    assert_eq!(contact.message_trust, TrustKind::UntilRevoked);
    assert_eq!(contact.message_trust_expires_at, None);
    assert!(contact.accepted_at.is_some());

    let status = requests::request_state(
        &pool,
        "contact-one",
        &requester.key.public_key().to_text(),
        9,
        &requester.status_signature("contact-one", 9),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusOutcome::Accepted);
}

#[tokio::test]
async fn accepting_with_a_stale_revision_is_a_conflict() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();

    let error = requests::accept_request(
        &pool,
        "contact-one",
        row.revision + 1,
        TrustKind::Once,
        TrustKind::Once,
        store::now(),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, super::ContactsError::RevisionConflict(_)));
    assert!(store::list_contacts(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn declining_answers_declined_and_cancel_only_touches_sent_rows() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();

    let cancel = requests::close_request(
        &pool,
        "contact-one",
        row.revision,
        RequestDirection::Sent,
        RequestState::Expired,
    )
    .await;
    assert!(matches!(
        cancel,
        Err(super::ContactsError::RevisionConflict(_))
    ));

    requests::close_request(
        &pool,
        "contact-one",
        row.revision,
        RequestDirection::Received,
        RequestState::Declined,
    )
    .await
    .unwrap();

    let status = requests::request_state(
        &pool,
        "contact-one",
        &requester.key.public_key().to_text(),
        1,
        &requester.status_signature("contact-one", 1),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusOutcome::Declined);
}

#[tokio::test]
async fn trust_updates_and_removal_respect_revisions() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    let now = store::now();
    requests::accept_request(
        &pool,
        "contact-one",
        row.revision,
        TrustKind::NotAllowed,
        TrustKind::NotAllowed,
        now,
    )
    .await
    .unwrap();
    let contact = store::list_contacts(&pool).await.unwrap().remove(0);

    let stale = store::update_contact_trust(
        &pool,
        &contact.id,
        contact.revision + 1,
        TrustGrant::new(TrustKind::ThirtyDays, now),
        TrustGrant::new(TrustKind::NotAllowed, now),
        now,
    )
    .await
    .unwrap();
    let fresh = store::update_contact_trust(
        &pool,
        &contact.id,
        contact.revision,
        TrustGrant::new(TrustKind::ThirtyDays, now),
        TrustGrant::new(TrustKind::NotAllowed, now),
        now,
    )
    .await
    .unwrap();
    let updated = store::contact_by_id(&pool, &contact.id)
        .await
        .unwrap()
        .unwrap();

    assert!(!stale);
    assert!(fresh);
    assert_eq!(updated.invite_trust, "thirty_days");
    assert_eq!(updated.revision, contact.revision + 1);

    let wrong_state =
        store::delete_contact(&pool, &contact.id, updated.revision, ContactState::Blocked)
            .await
            .unwrap();
    let removed = store::delete_contact(&pool, &contact.id, updated.revision, ContactState::Active)
        .await
        .unwrap();
    assert!(!wrong_state);
    assert!(removed);
    assert!(store::list_contacts(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn blocking_an_active_contact_clears_trust_and_unblocking_removes_the_row() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    let now = store::now();
    requests::accept_request(
        &pool,
        "contact-one",
        row.revision,
        TrustKind::UntilRevoked,
        TrustKind::UntilRevoked,
        now,
    )
    .await
    .unwrap();
    let key = requester.key.public_key().to_text();

    requests::block_person(&pool, &key, now).await.unwrap();
    let blocked = store::contact_by_key(&pool, &key).await.unwrap().unwrap();
    assert_eq!(blocked.state, "blocked");
    assert_eq!(blocked.invite_trust, "not_allowed");
    assert_eq!(blocked.message_trust, "not_allowed");
    assert!(blocked.blocked_at.is_some());

    let removed =
        store::delete_contact(&pool, &blocked.id, blocked.revision, ContactState::Blocked)
            .await
            .unwrap();
    assert!(removed);
    assert!(store::contact_by_key(&pool, &key).await.unwrap().is_none());
}

#[tokio::test]
async fn expired_requests_stop_being_pending() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;

    let later = store::now() + chrono::Duration::days(requests::REQUEST_TTL_DAYS + 1);
    store::expire_stale_requests(&pool, later).await.unwrap();

    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "expired");
}

use super::identity::LocalIdentity;
use super::requests::{
    self, CARD_REVOKED_CODE, INVALID_REQUEST_CODE, MAX_PENDING_RECEIVED, PeerReply, StatusOutcome,
};
use super::store::{self, ContactState, RequestDirection, RequestState, TrustGrant};
use crate::vault::handoff::protocol::{ContactRequestOutcome, ControlMessage};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ganbaru_contacts::{
    CARD_NONCE_BYTES, ContactCard, PersonKeyPair, SignedCard, TrustKind, encode_nonce,
    random_bytes, request_signature_payload, sign_card, status_signature_payload,
};
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
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
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

/// The requester-chosen ids of stored received rows, newest first.
async fn peer_ids(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT peer_request_id FROM contact_requests WHERE peer_request_id IS NOT NULL
         ORDER BY created_at DESC, id",
    )
    .fetch_all(pool)
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

    assert_eq!(peer_ids(&pool).await, ["contact-second"]);
}

#[tokio::test]
async fn a_request_id_reused_by_another_person_gets_its_own_row() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let first = Person::new("First");
    let second = Person::new("Second");

    deliver(&pool, &recipient, &first, "contact-shared").await;
    let reply = deliver(&pool, &recipient, &second, "contact-shared").await;

    assert_eq!(reply, received("contact-shared"));
    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].id, rows[1].id);
    assert_eq!(peer_ids(&pool).await, ["contact-shared", "contact-shared"]);
    for person in [&first, &second] {
        let row = store::received_request_by_peer(
            &pool,
            &person.key.public_key().to_text(),
            "contact-shared",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(row.display_name, person.name);
    }
}

#[tokio::test]
async fn a_request_delivered_again_after_its_row_was_deleted_gets_a_fresh_local_row() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    let key = requester.key.public_key().to_text();

    deliver(&pool, &recipient, &requester, "contact-one").await;
    let first = store::received_request_by_peer(&pool, &key, "contact-one")
        .await
        .unwrap()
        .unwrap();
    sqlx::query("DELETE FROM contact_requests")
        .execute(&pool)
        .await
        .unwrap();
    let reply = deliver(&pool, &recipient, &requester, "contact-one").await;

    assert_eq!(reply, received("contact-one"));
    let again = store::received_request_by_peer(&pool, &key, "contact-one")
        .await
        .unwrap()
        .unwrap();
    assert_ne!(again.id, first.id, "a deleted row's id is never reused");
    assert_eq!(again.state, "pending");
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
    assert_eq!(rows.len(), usize::try_from(MAX_PENDING_RECEIVED).unwrap());
    assert!(rows.iter().all(|row| row.state == "pending"));
    let ids = peer_ids(&pool).await;
    let newest = format!("contact-{:03}", total - 1);
    assert!(ids.contains(&newest), "the newest request is kept");
    for evicted in ["contact-000", "contact-001", "contact-002"] {
        assert!(
            !ids.iter().any(|id| id == evicted),
            "the oldest requests are deleted"
        );
    }
}

#[tokio::test]
async fn handled_requests_are_deleted_after_the_retention_window() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let old = Person::new("Old");
    let newer = Person::new("Newer");
    let start = store::now();
    deliver(&pool, &recipient, &old, "contact-old").await;
    let expired_at = start + chrono::Duration::days(requests::REQUEST_TTL_DAYS + 1);
    store::expire_stale_requests(&pool, expired_at)
        .await
        .unwrap();
    assert!(
        store::list_pending_requests(&pool, start)
            .await
            .unwrap()
            .is_empty(),
        "handled requests are never listed"
    );

    let later = expired_at + chrono::Duration::days(requests::HANDLED_REQUEST_RETENTION_DAYS + 1);
    let reply = requests::receive_request(
        &pool,
        &recipient.identity,
        later,
        &encode_nonce(&recipient.identity.card_nonce),
        &newer.card().text(),
        "contact-new",
        &newer.request_signature(&recipient.identity.card_nonce, "contact-new"),
    )
    .await
    .unwrap();
    assert_eq!(reply, received("contact-new"));

    assert_eq!(peer_ids(&pool).await, ["contact-new"]);
}

#[tokio::test]
async fn handled_requests_are_capped_with_the_oldest_deleted_first() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let handled = usize::try_from(requests::MAX_HANDLED_REQUESTS).unwrap() + 3;
    let now = store::now();
    for index in 0..handled {
        let id = format!("contact-{index:03}");
        let requester = Person::new("Many");
        deliver(&pool, &recipient, &requester, &id).await;
        let row =
            store::received_request_by_peer(&pool, &requester.key.public_key().to_text(), &id)
                .await
                .unwrap()
                .unwrap();
        assert!(
            store::set_request_state(&pool, &row.id, None, RequestState::Declined, None, now)
                .await
                .unwrap()
        );
    }
    deliver(&pool, &recipient, &Person::new("Last"), "contact-last").await;

    let rows = store::list_requests(&pool).await.unwrap();
    let declined = rows.iter().filter(|row| row.state == "declined").count();
    assert_eq!(
        declined,
        usize::try_from(requests::MAX_HANDLED_REQUESTS).unwrap()
    );
    let ids = peer_ids(&pool).await;
    assert!(
        ids.iter().any(|id| id == "contact-last"),
        "the new request is kept"
    );
    let newest_handled = format!("contact-{:03}", handled - 1);
    assert!(ids.contains(&newest_handled));
    for pruned in ["contact-000", "contact-001", "contact-002"] {
        assert!(
            !ids.iter().any(|id| id == pruned),
            "the oldest handled requests are deleted"
        );
    }
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
    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();
    let now = store::now();

    requests::accept_request(
        &pool,
        &row.id,
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
    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();

    let error = requests::accept_request(
        &pool,
        &row.id,
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
    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();

    let cancel = requests::close_request(
        &pool,
        &row.id,
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
        &row.id,
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
    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();
    let now = store::now();
    requests::accept_request(
        &pool,
        &row.id,
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

    let wrong_state = store::remove_contact(
        &pool,
        &contact.id,
        updated.revision,
        ContactState::Blocked,
        now,
    )
    .await
    .unwrap();
    let removed = store::remove_contact(
        &pool,
        &contact.id,
        updated.revision,
        ContactState::Active,
        now,
    )
    .await
    .unwrap();
    assert!(!wrong_state);
    assert!(removed);
    assert!(store::list_contacts(&pool).await.unwrap().is_empty());
    assert!(
        store::contact_by_key(&pool, &contact.public_key)
            .await
            .unwrap()
            .is_none()
    );
    let kept = store::contact_by_id(&pool, &contact.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.state, "removed");
    assert_eq!(
        (kept.invite_trust.as_str(), kept.message_trust.as_str()),
        ("not_allowed", "not_allowed")
    );
    assert!(kept.accepted_at.is_none());
}

#[tokio::test]
async fn blocking_an_active_contact_clears_trust_and_unblocking_removes_the_contact() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();
    let now = store::now();
    requests::accept_request(
        &pool,
        &row.id,
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

    let removed = store::remove_contact(
        &pool,
        &blocked.id,
        blocked.revision,
        ContactState::Blocked,
        now,
    )
    .await
    .unwrap();
    assert!(removed);
    assert!(store::contact_by_key(&pool, &key).await.unwrap().is_none());

    // A removed person can send a new request and be accepted again.
    deliver(&pool, &recipient, &requester, "contact-two").await;
    let again = store::received_request_by_peer(&pool, &key, "contact-two")
        .await
        .unwrap()
        .unwrap();
    requests::accept_request(
        &pool,
        &again.id,
        again.revision,
        TrustKind::Once,
        TrustKind::NotAllowed,
        now,
    )
    .await
    .unwrap();
    let readded = store::contact_by_key(&pool, &key).await.unwrap().unwrap();
    assert_eq!(readded.state, "active");
    assert!(readded.blocked_at.is_none());
}

#[tokio::test]
async fn expired_requests_stop_being_pending() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    deliver(&pool, &recipient, &requester, "contact-one").await;
    let key = requester.key.public_key().to_text();
    let later = store::now() + chrono::Duration::days(requests::REQUEST_TTL_DAYS + 1);

    let pending = store::received_request_by_peer(&pool, &key, "contact-one")
        .await
        .unwrap()
        .unwrap();
    assert!(
        store::list_pending_requests(&pool, later)
            .await
            .unwrap()
            .is_empty(),
        "a request past its deadline is not listed before a poll expires it"
    );
    assert!(
        store::pending_request(&pool, RequestDirection::Received, &key, later)
            .await
            .unwrap()
            .is_none()
    );
    let late_accept = requests::accept_request(
        &pool,
        &pending.id,
        pending.revision,
        TrustKind::Once,
        TrustKind::Once,
        later,
    )
    .await;
    assert!(matches!(
        late_accept,
        Err(super::ContactsError::RevisionConflict(_))
    ));
    assert!(store::list_contacts(&pool).await.unwrap().is_empty());

    store::expire_stale_requests(&pool, later).await.unwrap();

    let row = store::received_request_by_peer(
        &pool,
        &requester.key.public_key().to_text(),
        "contact-one",
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(row.state, "expired");
}

async fn insert_sent_request(pool: &SqlitePool, recipient: &Person, request_id: &str) {
    let card = recipient.card();
    let now = store::now();
    store::insert_request(
        pool,
        store::NewRequest {
            id: request_id,
            direction: RequestDirection::Sent,
            public_key: &recipient.key.public_key().to_text(),
            peer_request_id: None,
            display_name: recipient.name,
            color: 7,
            card: &card.text(),
            card_digest: &store::encode_hex(&card.digest),
            endpoint_hint: "127.0.0.1:43821",
            coordinator_fingerprint: &store::encode_hex(&[9u8; 32]),
            invite_trust: TrustKind::UntilRevoked,
            message_trust: TrustKind::UntilRevoked,
            expires_at: now + chrono::Duration::days(requests::REQUEST_TTL_DAYS),
        },
        now,
    )
    .await
    .unwrap();
}

fn accepted_by(recipient: &Person) -> Result<ControlMessage, String> {
    Ok(ControlMessage::ContactRequestState {
        state: ContactRequestOutcome::Accepted,
        recipient_card: Some(recipient.card().text()),
    })
}

#[tokio::test]
async fn an_accepted_poll_does_not_reopen_a_request_cancelled_while_in_flight() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    insert_sent_request(&pool, &recipient, "contact-one").await;
    let in_flight = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    requests::close_request(
        &pool,
        "contact-one",
        in_flight.revision,
        RequestDirection::Sent,
        RequestState::Expired,
    )
    .await
    .unwrap();

    let resolved =
        requests::apply_status_response(&pool, &in_flight, accepted_by(&recipient), store::now())
            .await
            .unwrap();

    assert!(resolved);
    let row = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "expired");
    let key = recipient.key.public_key().to_text();
    assert!(store::contact_by_key(&pool, &key).await.unwrap().is_none());
}

#[tokio::test]
async fn an_accepted_poll_never_clears_a_block() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let key = recipient.key.public_key().to_text();
    insert_sent_request(&pool, &recipient, "contact-one").await;
    let in_flight = store::request_by_id(&pool, "contact-one")
        .await
        .unwrap()
        .unwrap();
    requests::block_person(&pool, &key, store::now())
        .await
        .unwrap();

    requests::apply_status_response(&pool, &in_flight, accepted_by(&recipient), store::now())
        .await
        .unwrap();
    let blocked = store::contact_by_key(&pool, &key).await.unwrap().unwrap();
    assert_eq!(blocked.state, "blocked");
    assert_eq!(blocked.invite_trust, "not_allowed");

    insert_sent_request(&pool, &recipient, "contact-two").await;
    let pending = store::request_by_id(&pool, "contact-two")
        .await
        .unwrap()
        .unwrap();
    let resolved =
        requests::apply_status_response(&pool, &pending, accepted_by(&recipient), store::now())
            .await
            .unwrap();
    assert!(resolved);
    let blocked = store::contact_by_key(&pool, &key).await.unwrap().unwrap();
    assert_eq!(blocked.state, "blocked");
    assert!(blocked.blocked_at.is_some());
    let row = store::request_by_id(&pool, "contact-two")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.state, "expired");
}

#[tokio::test]
async fn a_repeated_delivery_of_a_pending_request_writes_nothing() {
    let pool = migrated_memory_pool().await;
    let recipient = Person::new("Recipient");
    let requester = Person::new("Requester");
    let key = requester.key.public_key().to_text();

    deliver(&pool, &recipient, &requester, "contact-one").await;
    let first = store::received_request_by_peer(&pool, &key, "contact-one")
        .await
        .unwrap()
        .unwrap();
    let reply = deliver(&pool, &recipient, &requester, "contact-one").await;

    assert_eq!(reply, received("contact-one"));
    let rows = store::list_requests(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, first.id, "the pending row keeps its id");
    assert_eq!(rows[0].revision, first.revision);
}

#[tokio::test]
async fn duplicate_pending_sent_rows_list_once_and_an_answer_closes_them_all() {
    for (outcome, answered) in [
        (ContactRequestOutcome::Declined, "declined"),
        (ContactRequestOutcome::Accepted, "accepted"),
    ] {
        let pool = migrated_memory_pool().await;
        let recipient = Person::new("Recipient");
        insert_sent_request(&pool, &recipient, "contact-one").await;
        insert_sent_request(&pool, &recipient, "contact-two").await;

        let listed = store::list_pending_requests(&pool, store::now())
            .await
            .unwrap();
        assert_eq!(listed.len(), 1, "one pending row per direction and person");

        let first = store::request_by_id(&pool, "contact-one")
            .await
            .unwrap()
            .unwrap();
        let recipient_card =
            matches!(outcome, ContactRequestOutcome::Accepted).then(|| recipient.card().text());
        let response = Ok(ControlMessage::ContactRequestState {
            state: outcome,
            recipient_card,
        });
        let resolved = requests::apply_status_response(&pool, &first, response, store::now())
            .await
            .unwrap();

        assert!(resolved);
        let states: Vec<(String, String)> =
            sqlx::query_as("SELECT id, state FROM contact_requests ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            states,
            [
                ("contact-one".to_owned(), answered.to_owned()),
                ("contact-two".to_owned(), "expired".to_owned()),
            ]
        );
        assert!(
            store::list_pending_requests(&pool, store::now())
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn accepting_rechecks_the_stored_card_before_granting_trust() {
    let pool = migrated_memory_pool().await;
    let requester = Person::new("Requester");
    let impostor = Person::new("Impostor");
    let key = requester.key.public_key().to_text();
    let genuine = requester.card();
    let foreign = impostor.card();
    // Rows as a linked device could replicate them: another person's card, a card that does
    // not decode, and the right card with a digest that does not match it.
    let tampered = [
        (
            "contact-foreign",
            foreign.text(),
            store::encode_hex(&foreign.digest),
        ),
        (
            "contact-garbage",
            "not-a-card".to_owned(),
            store::encode_hex(&genuine.digest),
        ),
        (
            "contact-digest",
            genuine.text(),
            store::encode_hex(&[1u8; 32]),
        ),
    ];
    let now = store::now();
    for (id, card, digest) in &tampered {
        store::insert_request(
            &pool,
            store::NewRequest {
                id,
                direction: RequestDirection::Received,
                public_key: &key,
                peer_request_id: Some(id),
                display_name: "Someone else",
                color: 3,
                card,
                card_digest: digest,
                endpoint_hint: "",
                coordinator_fingerprint: "",
                invite_trust: TrustKind::NotAllowed,
                message_trust: TrustKind::NotAllowed,
                expires_at: now + chrono::Duration::days(requests::REQUEST_TTL_DAYS),
            },
            now,
        )
        .await
        .unwrap();
    }

    for (id, _, _) in &tampered {
        let row = store::request_by_id(&pool, id).await.unwrap().unwrap();
        let accepted = requests::accept_request(
            &pool,
            id,
            row.revision,
            TrustKind::UntilRevoked,
            TrustKind::UntilRevoked,
            now,
        )
        .await;

        assert!(accepted.is_err(), "{id} must not be accepted");
        assert!(store::contact_by_key(&pool, &key).await.unwrap().is_none());
        let unchanged = store::request_by_id(&pool, id).await.unwrap().unwrap();
        assert_eq!(unchanged.state, "pending");
    }
}

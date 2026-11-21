//! Concurrent contact edits: blocks, removals, trust changes, card revisions, and pending
//! requests settle on one deterministic result on both replicas.

use super::{HUB_DEVICE, PHONE_DEVICE, Replica, converge, hub};
use crate::tests::block_on;

/// Public key of the person the contacts tests add.
const PEER_KEY: &str = "QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE";
const CONTACTS_AT: &str = "2026-10-07T12:00:00.000Z";
const REQUEST_EXPIRES_AT: &str = "2026-10-14T12:00:00.000Z";

/// Runs a contact update that binds the peer key as `?1` and the change time as `?2`.
async fn update_contact(replica: &Replica, sql: &'static str) {
    sqlx::query(sql)
        .bind(PEER_KEY)
        .bind(CONTACTS_AT)
        .execute(&replica.pool)
        .await
        .expect("contacts write");
}

async fn insert_contact(replica: &Replica, state: &str) {
    let blocked_at = (state == "blocked").then_some(CONTACTS_AT);
    sqlx::query(
        "INSERT INTO contacts (id, public_key, display_name, state, blocked_at, created_at, updated_at)
         VALUES ('contact-' || ?1, ?1, 'Ana', ?2, ?3, ?4, ?4)",
    )
    .bind(PEER_KEY)
    .bind(state)
    .bind(blocked_at)
    .bind(CONTACTS_AT)
    .execute(&replica.pool)
    .await
    .expect("insert contact");
}

async fn insert_request(replica: &Replica, id: &str, direction: &str) {
    sqlx::query(
        "INSERT INTO contact_requests
         (id, direction, public_key, display_name, card, card_digest, expires_at, created_at,
          updated_at)
         VALUES (?, ?, ?, 'Ana', 'Y2FyZA', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(direction)
    .bind(PEER_KEY)
    .bind("0".repeat(64))
    .bind(REQUEST_EXPIRES_AT)
    .bind(CONTACTS_AT)
    .bind(CONTACTS_AT)
    .execute(&replica.pool)
    .await
    .expect("insert request");
}

async fn contact_standing(replica: &Replica) -> Option<(String, String)> {
    sqlx::query_as("SELECT state, invite_trust FROM contacts WHERE public_key = ?")
        .bind(PEER_KEY)
        .fetch_optional(&replica.pool)
        .await
        .expect("read contact")
}

async fn request_states(replica: &Replica) -> Vec<(String, String)> {
    sqlx::query_as("SELECT id, state FROM contact_requests ORDER BY id")
        .fetch_all(&replica.pool)
        .await
        .expect("read requests")
}

#[test]
fn a_block_outranks_a_concurrent_accept_and_trust_edit() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-block-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-block-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);

        insert_contact(&hub_replica, "blocked").await;
        hub_replica.seal().await;
        insert_contact(&phone, "active").await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a block made while the other device accepted wins"
            );
        }

        update_contact(
            &hub_replica,
            "UPDATE contacts SET state = 'active', blocked_at = NULL, accepted_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        update_contact(
            &hub_replica,
            "UPDATE contacts SET state = 'blocked', blocked_at = ?2 WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a trust edit never lifts a concurrent block"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

/// Gives the peer the removed standing, as removing or unblocking a contact does.
const REMOVE_CONTACT: &str = "UPDATE contacts
     SET state = 'removed', invite_trust = 'not_allowed', invite_trust_expires_at = NULL,
         message_trust = 'not_allowed', message_trust_expires_at = NULL, accepted_at = NULL,
         blocked_at = NULL, updated_at = ?2
     WHERE public_key = ?1";

#[test]
fn removing_and_adding_one_person_on_two_devices_settles_on_one_row() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-readd-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-readd-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        assert_eq!(
            contact_standing(&phone).await,
            Some(("removed".to_string(), "not_allowed".to_string()))
        );

        update_contact(
            &phone,
            "UPDATE contacts SET state = 'active', accepted_at = ?2, updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM contacts")
                .fetch_one(&replica.pool)
                .await
                .expect("count contacts");
            assert_eq!(count, 1, "the person comes back as the same row");
            assert_eq!(
                contact_standing(replica).await,
                Some(("active".to_string(), "not_allowed".to_string())),
                "an add made after the removal arrived revives the contact"
            );
        }

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("removed".to_string(), "not_allowed".to_string())),
                "a concurrent trust edit never revives a removed contact"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_block_survives_a_concurrent_removal() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-block-remove-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-block-remove-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(&hub_replica, REMOVE_CONTACT).await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET state = 'blocked', blocked_at = ?2, updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("blocked".to_string(), "not_allowed".to_string())),
                "a removal on one device never drops a block made on another"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn a_concurrent_trust_narrowing_beats_a_widening() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-narrow-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-narrow-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        insert_contact(&hub_replica, "active").await;
        update_contact(
            &hub_replica,
            "UPDATE contacts SET invite_trust = 'seven_days', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        update_contact(
            &hub_replica,
            "UPDATE contacts SET invite_trust = 'until_revoked', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        hub_replica.seal().await;
        update_contact(
            &phone,
            "UPDATE contacts SET invite_trust = 'not_allowed', updated_at = ?2
             WHERE public_key = ?1",
        )
        .await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                contact_standing(replica).await,
                Some(("active".to_string(), "not_allowed".to_string())),
                "a revocation made while another device widened trust stands"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn the_higher_card_revision_wins_a_concurrent_regeneration() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-card-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-card-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);
        sqlx::query(
            "INSERT INTO contact_card (key, card_nonce, card_revision, updated_at)
             VALUES ('card', 'AAAAAAAAAAAAAAAAAAAAAA', 1, ?)",
        )
        .bind(CONTACTS_AT)
        .execute(&hub_replica.pool)
        .await
        .expect("insert card");
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;

        let rotate = |nonce: &'static str| {
            sqlx::query(
                "UPDATE contact_card SET card_nonce = ?, card_revision = card_revision + 1
                 WHERE key = 'card'",
            )
            .bind(nonce)
        };
        rotate("BBBBBBBBBBBBBBBBBBBBBA")
            .execute(&hub_replica.pool)
            .await
            .expect("hub rotate");
        rotate("CCCCCCCCCCCCCCCCCCCCCA")
            .execute(&hub_replica.pool)
            .await
            .expect("hub rotate again");
        hub_replica.seal().await;
        rotate("DDDDDDDDDDDDDDDDDDDDDA")
            .execute(&phone.pool)
            .await
            .expect("phone rotate");
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            let card: (String, i64) =
                sqlx::query_as("SELECT card_nonce, card_revision FROM contact_card")
                    .fetch_one(&replica.pool)
                    .await
                    .expect("read card");
            assert_eq!(card, ("CCCCCCCCCCCCCCCCCCCCCA".to_string(), 3));
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

#[test]
fn duplicate_pending_requests_both_survive_and_a_concurrent_delete_beats_an_answer() {
    block_on(async {
        let mut hub_replica = Replica::new("contacts-requests-hub", HUB_DEVICE).await;
        let mut phone = Replica::new("contacts-requests-phone", PHONE_DEVICE).await;
        let (transport, _changes) = hub(&hub_replica);

        insert_request(&hub_replica, "request-hub", "sent").await;
        hub_replica.seal().await;
        insert_request(&phone, "request-phone", "sent").await;
        phone.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        let pending = vec![
            ("request-hub".to_string(), "pending".to_string()),
            ("request-phone".to_string(), "pending".to_string()),
        ];
        for replica in [&hub_replica, &phone] {
            assert_eq!(request_states(replica).await, pending);
        }

        insert_request(&hub_replica, "request-received", "received").await;
        hub_replica.seal().await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        hub_replica
            .edit(
                "DELETE FROM contact_requests WHERE id = ?",
                "request-received",
            )
            .await;
        phone
            .edit(
                "UPDATE contact_requests SET state = 'accepted' WHERE id = ?",
                "request-received",
            )
            .await;
        converge(&mut hub_replica, &mut phone, &transport).await;
        for replica in [&hub_replica, &phone] {
            assert_eq!(
                request_states(replica).await,
                pending,
                "the deleted request stays deleted"
            );
        }
        hub_replica.close().await;
        phone.close().await;
    });
}

use super::helpers::migrated_memory_pool;

const PUBLIC_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OTHER_KEY: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
const CONTACT_ID: &str = "contact-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const NONCE: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const NOW: &str = "2026-10-07T12:00:00.000Z";

#[test]
fn schema_creates_contacts_storage_with_an_immutable_single_local_identity() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in [
            "contacts_local_identity",
            "contact_card",
            "contacts",
            "contact_requests",
        ] {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = ?")
                    .bind(table)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, 1, "missing {table}");
        }
        let insert_identity = |singleton: i64, key: &'static str| {
            sqlx::query(
                "INSERT INTO contacts_local_identity (singleton, public_key, created_at)
                 VALUES (?, ?, ?)",
            )
            .bind(singleton)
            .bind(key)
            .bind(NOW)
        };
        insert_identity(1, PUBLIC_KEY).execute(&pool).await.unwrap();
        assert!(
            insert_identity(2, OTHER_KEY).execute(&pool).await.is_err(),
            "identity must stay a singleton"
        );
        assert!(insert_identity(1, "short").execute(&pool).await.is_err());
        let changed = sqlx::query("UPDATE contacts_local_identity SET public_key = ?")
            .bind(OTHER_KEY)
            .execute(&pool)
            .await;
        assert!(changed.is_err(), "the person key never changes");
        let deleted = sqlx::query("DELETE FROM contacts_local_identity")
            .execute(&pool)
            .await;
        assert!(deleted.is_err(), "the person key is never deleted");
    });
}

#[test]
fn contact_card_is_a_single_undeletable_row_with_a_bounded_nonce() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let insert_card = |key: &'static str| {
            sqlx::query("INSERT INTO contact_card (key, card_nonce, updated_at) VALUES (?, ?, ?)")
                .bind(key)
                .bind(NONCE)
                .bind(NOW)
        };
        insert_card("card").execute(&pool).await.unwrap();
        assert!(insert_card("other").execute(&pool).await.is_err());
        let short_nonce = sqlx::query("UPDATE contact_card SET card_nonce = 'short'")
            .execute(&pool)
            .await;
        assert!(short_nonce.is_err());
        let zero_revision = sqlx::query("UPDATE contact_card SET card_revision = 0")
            .execute(&pool)
            .await;
        assert!(zero_revision.is_err());
        let deleted = sqlx::query("DELETE FROM contact_card").execute(&pool).await;
        assert!(deleted.is_err(), "the card row is never deleted");
    });
}

#[test]
fn contacts_enforce_derived_ids_trust_kinds_state_and_revision_bumps() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let insert_contact = |id: &'static str, key: &'static str| {
            sqlx::query(
                "INSERT INTO contacts (id, public_key, display_name, created_at, updated_at)
                 VALUES (?, ?, 'Ana', ?, ?)",
            )
            .bind(id)
            .bind(key)
            .bind(NOW)
            .bind(NOW)
        };
        insert_contact(CONTACT_ID, PUBLIC_KEY)
            .execute(&pool)
            .await
            .unwrap();
        let (state, invite, color): (String, String, i64) =
            sqlx::query_as("SELECT state, invite_trust, color FROM contacts WHERE id = ?")
                .bind(CONTACT_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (state.as_str(), invite.as_str(), color),
            ("active", "not_allowed", 30)
        );
        assert!(
            insert_contact("person:b", OTHER_KEY)
                .execute(&pool)
                .await
                .is_err(),
            "the id derives from the public key"
        );
        assert!(
            insert_contact(CONTACT_ID, PUBLIC_KEY)
                .execute(&pool)
                .await
                .is_err(),
            "one contact per public key"
        );
        let bad_trust = sqlx::query("UPDATE contacts SET invite_trust = 'forever' WHERE id = ?")
            .bind(CONTACT_ID)
            .execute(&pool)
            .await;
        assert!(bad_trust.is_err());
        let bad_state = sqlx::query("UPDATE contacts SET state = 'muted' WHERE id = ?")
            .bind(CONTACT_ID)
            .execute(&pool)
            .await;
        assert!(bad_state.is_err());
        sqlx::query(
            "UPDATE contacts SET message_trust = 'seven_days', message_trust_expires_at = ?
             WHERE id = ?",
        )
        .bind(NOW)
        .bind(CONTACT_ID)
        .execute(&pool)
        .await
        .unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM contacts WHERE id = ?")
            .bind(CONTACT_ID)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(revision, 2, "updates bump the revision");
    });
}

#[test]
fn contact_requests_allow_duplicate_pending_rows_from_concurrent_devices() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let insert = |id: &'static str, direction: &'static str, peer: Option<&'static str>| {
            sqlx::query(
                "INSERT INTO contact_requests
                 (id, direction, public_key, peer_request_id, display_name, card, card_digest,
                  expires_at, created_at, updated_at)
                 VALUES (?, ?, ?, ?, 'Ana', 'Y2FyZA', ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(direction)
            .bind(PUBLIC_KEY)
            .bind(peer)
            .bind("0".repeat(64))
            .bind(NOW)
            .bind(NOW)
            .bind(NOW)
        };
        insert("request-1", "received", Some("peer-one"))
            .execute(&pool)
            .await
            .unwrap();
        insert("request-2", "received", Some("peer-one"))
            .execute(&pool)
            .await
            .unwrap();
        insert("request-3", "sent", None)
            .execute(&pool)
            .await
            .unwrap();
        insert("request-4", "sent", None)
            .execute(&pool)
            .await
            .unwrap();
        let pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM contact_requests WHERE public_key = ? AND state = 'pending'",
        )
        .bind(PUBLIC_KEY)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pending, 4, "readers collapse duplicates, not a constraint");
        let empty_peer_id =
            sqlx::query("UPDATE contact_requests SET peer_request_id = '' WHERE id = 'request-1'")
                .execute(&pool)
                .await;
        assert!(empty_peer_id.is_err());
        let bad_direction = sqlx::query(
            "UPDATE contact_requests SET direction = 'sideways' WHERE id = 'request-1'",
        )
        .execute(&pool)
        .await;
        assert!(bad_direction.is_err());
        let bad_fingerprint = sqlx::query(
            "UPDATE contact_requests SET coordinator_fingerprint = 'abc' WHERE id = 'request-3'",
        )
        .execute(&pool)
        .await;
        assert!(bad_fingerprint.is_err());
        sqlx::query("UPDATE contact_requests SET state = 'declined' WHERE id = 'request-1'")
            .execute(&pool)
            .await
            .unwrap();
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM contact_requests WHERE id = 'request-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(revision, 2);
    });
}

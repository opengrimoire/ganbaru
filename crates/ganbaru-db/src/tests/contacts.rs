use super::helpers::migrated_memory_pool;

const PUBLIC_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const OTHER_KEY: &str = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
const NONCE: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const NOW: &str = "2026-10-07T12:00:00.000Z";

#[test]
fn schema_creates_contacts_storage_with_a_single_local_identity() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in ["contacts_local_identity", "contacts", "contact_requests"] {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = ?")
                    .bind(table)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, 1, "missing {table}");
        }
        sqlx::query(
            "INSERT INTO contacts_local_identity (singleton, public_key, card_nonce, created_at, updated_at)
             VALUES (1, ?, ?, ?, ?)",
        )
        .bind(PUBLIC_KEY)
        .bind(NONCE)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let second = sqlx::query(
            "INSERT INTO contacts_local_identity (singleton, public_key, card_nonce, created_at, updated_at)
             VALUES (2, ?, ?, ?, ?)",
        )
        .bind(OTHER_KEY)
        .bind(NONCE)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(second.is_err(), "identity must stay a singleton");
        let short_key = sqlx::query(
            "INSERT INTO contacts_local_identity (singleton, public_key, card_nonce, created_at, updated_at)
             VALUES (1, 'short', ?, ?, ?)",
        )
        .bind(NONCE)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(short_key.is_err());
    });
}

#[test]
fn contacts_enforce_trust_kinds_state_and_revision_bumps() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO contacts (id, public_key, display_name, created_at, updated_at)
             VALUES ('person:a', ?, 'Ana', ?, ?)",
        )
        .bind(PUBLIC_KEY)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let (state, invite, color): (String, String, i64) =
            sqlx::query_as("SELECT state, invite_trust, color FROM contacts WHERE id = 'person:a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (state.as_str(), invite.as_str(), color),
            ("active", "not_allowed", 30)
        );
        let duplicate_key = sqlx::query(
            "INSERT INTO contacts (id, public_key, display_name, created_at, updated_at)
             VALUES ('person:b', ?, 'Twin', ?, ?)",
        )
        .bind(PUBLIC_KEY)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(duplicate_key.is_err(), "one contact per public key");
        let bad_trust =
            sqlx::query("UPDATE contacts SET invite_trust = 'forever' WHERE id = 'person:a'")
                .execute(&pool)
                .await;
        assert!(bad_trust.is_err());
        let bad_state = sqlx::query("UPDATE contacts SET state = 'muted' WHERE id = 'person:a'")
            .execute(&pool)
            .await;
        assert!(bad_state.is_err());
        sqlx::query(
            "UPDATE contacts SET message_trust = 'seven_days', message_trust_expires_at = ?
             WHERE id = 'person:a'",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM contacts WHERE id = 'person:a'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(revision, 2, "updates bump the revision");
    });
}

#[test]
fn contact_requests_allow_one_pending_request_per_peer_and_direction() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let insert = |id: &'static str, direction: &'static str, key: &'static str| {
            sqlx::query(
                "INSERT INTO contact_requests
                 (id, direction, public_key, display_name, card, card_digest, expires_at, created_at, updated_at)
                 VALUES (?, ?, ?, 'Ana', 'Y2FyZA', ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(direction)
            .bind(key)
            .bind("0".repeat(64))
            .bind(NOW)
            .bind(NOW)
            .bind(NOW)
        };
        insert("request-1", "received", PUBLIC_KEY)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            insert("request-2", "received", PUBLIC_KEY)
                .execute(&pool)
                .await
                .is_err(),
            "a second pending request from the same key is rejected"
        );
        insert("request-3", "sent", PUBLIC_KEY)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE contact_requests SET state = 'declined' WHERE id = 'request-1'")
            .execute(&pool)
            .await
            .unwrap();
        insert("request-4", "received", PUBLIC_KEY)
            .execute(&pool)
            .await
            .unwrap();
        let bad_direction = sqlx::query(
            "INSERT INTO contact_requests
             (id, direction, public_key, display_name, card, card_digest, expires_at, created_at, updated_at)
             VALUES ('request-5', 'sideways', ?, 'Ana', 'Y2FyZA', ?, ?, ?, ?)",
        )
        .bind(OTHER_KEY)
        .bind("0".repeat(64))
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(bad_direction.is_err());
        let bad_fingerprint = sqlx::query(
            "UPDATE contact_requests SET coordinator_fingerprint = 'abc' WHERE id = 'request-3'",
        )
        .execute(&pool)
        .await;
        assert!(bad_fingerprint.is_err());
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM contact_requests WHERE id = 'request-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(revision, 2);
    });
}

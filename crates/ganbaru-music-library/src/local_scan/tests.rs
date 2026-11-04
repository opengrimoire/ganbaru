use super::*;
use sqlx::SqlitePool;

async fn seeded_pool() -> SqlitePool {
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
    sqlx::raw_sql(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES ('root-1', 'Phone music', 1700000000000, 1700000000000);
         INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES ('collection-1', 'local-root', 'local-root:root-1', 'Phone music', 'root-1',
                 1700000000000, 1700000000000);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

fn values<'a>(title: &'a str, modified_at_ms: i64) -> LocalScanValues<'a> {
    LocalScanValues {
        relative_path: "Album/Song.mp3",
        media_kind: "audio",
        title,
        artist: "Artist",
        album: "Album",
        track_number: Some(1),
        duration_ms: Some(180_000),
        artwork_identity: None,
        file_size_bytes: Some(4_096),
        modified_at_ms: Some(modified_at_ms),
        lightweight_fingerprint: "fingerprint",
        strong_fingerprint: None,
    }
}

async fn persist(
    pool: &SqlitePool,
    values: &LocalScanValues<'_>,
    generation: i64,
    at_ms: i64,
) -> LocalScanOutcome {
    let mut transaction = pool.begin().await.unwrap();
    let outcome = persist_scanned_file(
        &mut transaction,
        "collection-1",
        "root-1",
        values,
        Some(0),
        generation,
        at_ms,
        at_ms,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    outcome
}

/// Item id, item update time, item version, location id, location update time, stored
/// modification time, location availability, and item availability.
type StoredRow = (String, i64, i64, String, i64, Option<i64>, String, String);

async fn stored(pool: &SqlitePool) -> StoredRow {
    sqlx::query_as(
        "SELECT item.id, item.updated_at_ms, item.version, location.id,
                location.updated_at_ms, location.modified_at_ms, location.availability,
                item.availability
         FROM music_library_items AS item
         JOIN music_local_locations AS location ON location.item_id = item.id",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn unchanged_stat_matches_only_known_equal_observations() {
    let stat = LocalLocationStat {
        file_size_bytes: Some(10),
        modified_at_ms: Some(20),
    };
    assert!(stat.matches(Some(10), Some(20)));
    assert!(!stat.matches(Some(11), Some(20)));
    assert!(!stat.matches(Some(10), Some(21)));
    assert!(!stat.matches(Some(10), None));
    let unknown = LocalLocationStat {
        file_size_bytes: Some(10),
        modified_at_ms: None,
    };
    assert!(!unknown.matches(Some(10), None));
}

#[test]
fn scanned_files_write_portable_values_only_when_new_or_changed() {
    crate::test_support::block_on(async {
        let pool = seeded_pool().await;
        assert_eq!(
            persist(&pool, &values("Song", 100), 1, 1_700_000_000_000).await,
            LocalScanOutcome::Written
        );
        let created = stored(&pool).await;
        assert_eq!(
            created.0,
            crate::identity::local_item_id("root-1", "Album/Song.mp3")
        );
        assert_eq!(
            created.3,
            crate::identity::location_id("root-1", "Album/Song.mp3")
        );

        assert_eq!(
            persist(&pool, &values("Song", 100), 2, 1_700_000_100_000).await,
            LocalScanOutcome::Unchanged
        );
        assert_eq!(stored(&pool).await, created);
        let generation: i64 = sqlx::query_scalar(
            "SELECT last_seen_generation FROM music_source_collection_items
             WHERE collection_id = 'collection-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(generation, 2);

        assert_eq!(
            persist(&pool, &values("Song", 200), 3, 1_700_000_200_000).await,
            LocalScanOutcome::Written
        );
        let touched = stored(&pool).await;
        assert_eq!((touched.1, touched.2, touched.4), (created.1, 1, created.4));
        assert_eq!(touched.5, Some(200));

        persist(&pool, &values("Retagged song", 300), 4, 1_700_000_300_000).await;
        let retagged = stored(&pool).await;
        assert_eq!((retagged.1, retagged.2), (1_700_000_300_000, 2));
        assert_eq!(retagged.4, created.4);
    });
}

#[test]
fn unseen_locations_become_missing_without_portable_writes() {
    crate::test_support::block_on(async {
        let pool = seeded_pool().await;
        persist(&pool, &values("Song", 100), 1, 1_700_000_000_000).await;
        let before = stored(&pool).await;
        let mut transaction = pool.begin().await.unwrap();
        mark_unseen(&mut transaction, "collection-1", "root-1", 2)
            .await
            .unwrap();
        reconcile_item_availability(&mut transaction, "root-1")
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let after = stored(&pool).await;
        assert_eq!((after.6.as_str(), after.7.as_str()), ("missing", "missing"));
        assert_eq!(
            (after.0, after.1, after.2, after.3, after.4),
            (before.0, before.1, before.2, before.3, before.4)
        );

        assert_eq!(
            persist(&pool, &values("Song", 100), 3, 1_700_000_100_000).await,
            LocalScanOutcome::Unchanged
        );
        let restored = stored(&pool).await;
        assert_eq!(
            (restored.6.as_str(), restored.7.as_str()),
            ("available", "available")
        );
        assert_eq!(restored.2, before.2);
    });
}

/// Records `key` of `table` in the sync row registry, as publishing its creation does.
async fn publish(pool: &SqlitePool, table: &ganbaru_sync::manifest::TableSpec, key: &str) {
    sqlx::query("INSERT INTO sync_rows (table_id, row_key, state) VALUES (?, ?, 'live')")
        .bind(i64::from(table.id.0))
        .bind(key)
        .execute(pool)
        .await
        .unwrap();
}

#[test]
fn a_file_found_again_after_a_published_deletion_takes_successor_keys() {
    use ganbaru_sync::manifest::vault::music::{ITEMS, LOCATIONS};
    crate::test_support::block_on(async {
        let pool = seeded_pool().await;
        persist(&pool, &values("Song", 100), 1, 1_700_000_000_000).await;
        let item = crate::identity::local_item_id("root-1", "Album/Song.mp3");
        let location = crate::identity::location_id("root-1", "Album/Song.mp3");
        publish(&pool, &ITEMS, &item).await;
        publish(&pool, &LOCATIONS, &location).await;
        sqlx::query("DELETE FROM music_local_locations WHERE id = ?")
            .bind(&location)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM music_library_items WHERE id = ?")
            .bind(&item)
            .execute(&pool)
            .await
            .unwrap();
        let refused = sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, original_title,
                 discovered_at_ms, updated_at_ms)
             VALUES (?, 'local:refused', 'local-file', 'audio', 'Song', 1, 1)",
        )
        .bind(&item)
        .execute(&pool)
        .await
        .expect_err("sync refuses a published key created again");
        assert!(refused.to_string().contains("cannot be created again"));

        assert_eq!(
            persist(&pool, &values("Song", 100), 2, 1_700_000_100_000).await,
            LocalScanOutcome::Written
        );
        let found = stored(&pool).await;
        assert_eq!(found.0, format!("{item}-2"));
        assert_eq!(found.3, format!("{location}-2"));

        persist(&pool, &values("Song", 200), 3, 1_700_000_200_000).await;
        assert_eq!(stored(&pool).await.0, format!("{item}-2"));
    });
}

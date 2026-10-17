use super::super::source_lifecycle;
use super::*;
use sqlx::SqlitePool;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::run_migrations(&pool).await.unwrap();
    pool
}

async fn seed(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES ('root-1', 'Music', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES ('source-1', 'local-root', 'root:1', 'Music', 'root-1', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, youtube_playlist_id, created_at_ms, updated_at_ms)
         VALUES ('source-2', 'youtube-playlist', 'youtube:2', 'Online', 'PLsource2', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    for item_id in ["item-orphan", "item-member", "item-shared"] {
        sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, original_title,
                 availability, discovered_at_ms, updated_at_ms)
             VALUES (?, ?, 'local-file', 'audio', ?, 'available', 1, 1)",
        )
        .bind(item_id)
        .bind(format!("local:{item_id}"))
        .bind(item_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, first_discovered_at_ms)
             VALUES ('source-1', ?, 1)",
        )
        .bind(item_id)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, availability, first_seen_at_ms, updated_at_ms)
         VALUES ('location-orphan', 'item-orphan', 'root-1', 'orphan.mp3', 'available', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collection_items
            (collection_id, item_id, first_discovered_at_ms)
         VALUES ('source-2', 'item-shared', 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms)
         VALUES ('playlist-1', 'Focus', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_playlist_memberships
            (id, playlist_id, item_id, position, created_at_ms, updated_at_ms)
         VALUES ('membership-1', 'playlist-1', 'item-member', 0, 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_refresh_jobs
            (id, source_collection_id, local_root_id, kind, state, generation,
             requested_at_ms, updated_at_ms)
         VALUES ('refresh-1', 'source-1', 'root-1', 'local-root', 'running', 1, 2, 2)",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn source_removal_preserves_referenced_items_and_prunes_only_reviewed_orphans() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed(&pool).await;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let media_folder = std::env::temp_dir().join(format!(
            "ganbaru-source-removal-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&media_folder).unwrap();
        let media_file = media_folder.join("orphan.mp3");
        fs::write(&media_file, b"user-owned media").unwrap();
        let impact = source_lifecycle::removal_impact(&pool, "source-1")
            .await
            .unwrap();
        assert_eq!(impact.item_count, 3);
        assert_eq!(impact.membership_count, 1);
        assert_eq!(impact.shared_item_count, 1);
        assert_eq!(impact.orphaned_item_count, 1);
        assert_eq!(impact.active_refresh_count, 1);

        source_lifecycle::remove_source(
            &pool,
            MusicSourceRemovalRequest {
                collection_id: "source-1".to_string(),
                expected_version: 1,
                expected_impact: impact,
                remove_orphaned_items: true,
                removed_at_ms: 100,
            },
        )
        .await
        .unwrap();
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT id FROM music_library_items ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec!["item-member", "item-shared"]);
        assert!(media_file.exists());
        let source_state: (i64, Option<i64>) = sqlx::query_as(
            "SELECT discovery_enabled, removed_at_ms FROM music_source_collections
             WHERE id = 'source-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(source_state, (0, Some(100)));
        let refresh_state: String =
            sqlx::query_scalar("SELECT state FROM music_refresh_jobs WHERE id = 'refresh-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(refresh_state, "cancelled");
        let membership_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM music_playlist_memberships")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(membership_count, 1);

        fs::remove_dir_all(media_folder).unwrap();
    });
}

#[test]
fn source_removal_rejects_stale_impact_without_partial_changes() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed(&pool).await;
        let impact = source_lifecycle::removal_impact(&pool, "source-1")
            .await
            .unwrap();
        sqlx::query("DELETE FROM music_playlist_memberships WHERE id = 'membership-1'")
            .execute(&pool)
            .await
            .unwrap();
        let error = source_lifecycle::remove_source(
            &pool,
            MusicSourceRemovalRequest {
                collection_id: "source-1".to_string(),
                expected_version: 1,
                expected_impact: impact,
                remove_orphaned_items: false,
                removed_at_ms: 100,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, MusicLibraryErrorCode::StaleWrite);
        let enabled: i64 = sqlx::query_scalar(
            "SELECT discovery_enabled FROM music_source_collections WHERE id = 'source-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(enabled, 1);
    });
}

#[test]
fn source_projection_updates_for_review_health_relink_and_removal() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed(&pool).await;
        sqlx::query(
            "UPDATE music_source_collections
             SET last_successful_refresh_at_ms = 1000, previous_successful_refresh_at_ms = 0
             WHERE id = 'source-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let initial = queries::source_summaries(&pool, 1_100, 0, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.id == "source-1")
            .unwrap();
        assert_eq!(initial.unreviewed_count, 3);
        assert_eq!(initial.new_count, 3);
        assert_eq!(initial.health, MusicSourceHealth::Healthy);

        sqlx::query(
            "UPDATE music_library_items SET review_state = 'ignored'
             WHERE id = 'item-member'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items SET availability = 'missing'
             WHERE id = 'item-orphan'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_relink_plans
                (id, root_id, state, ambiguous_count, created_at_ms, updated_at_ms)
             VALUES ('projection-plan', 'root-1', 'ready', 1, 10, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_relink_plan_entries
                (id, plan_id, match_kind, suggested_item_id, candidate_relative_path,
                 candidate_item_ids, created_at_ms)
             VALUES ('projection-entry', 'projection-plan', 'ambiguous', NULL,
                     'moved.mp3', '[\"item-orphan\",\"item-shared\"]', 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let changed = queries::source_summaries(&pool, 1_100, 0, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.id == "source-1")
            .unwrap();
        assert_eq!(changed.unreviewed_count, 2);
        assert_eq!(changed.missing_count, 1);
        assert!(changed.open_issue_count >= 2);
        assert_eq!(changed.health, MusicSourceHealth::Issues);

        let impact = source_lifecycle::removal_impact(&pool, "source-1")
            .await
            .unwrap();
        source_lifecycle::remove_source(
            &pool,
            MusicSourceRemovalRequest {
                collection_id: "source-1".to_string(),
                expected_version: 1,
                expected_impact: impact,
                remove_orphaned_items: false,
                removed_at_ms: 2_000,
            },
        )
        .await
        .unwrap();
        let disabled = queries::source_summaries(&pool, 2_000, 0, 20)
            .await
            .unwrap()
            .into_iter()
            .find(|source| source.id == "source-1")
            .unwrap();
        assert_eq!(disabled.health, MusicSourceHealth::Disabled);
    });
}

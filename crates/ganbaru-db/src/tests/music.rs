use super::helpers::migrated_memory_pool;
use crate::run_migrations;
use sqlx::Row;

#[test]
fn repeated_migration_startup_preserves_music_data() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO music_playlists (id, name, icon, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Deep focus', 'emoji:🎧', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let migration_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();

        run_migrations(&pool).await.unwrap();

        let playlist: (String, String) =
            sqlx::query_as("SELECT name, icon FROM music_playlists WHERE id = 'playlist-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let repeated_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(playlist, ("Deep focus".into(), "emoji:🎧".into()));
        assert_eq!(repeated_count, migration_count);
    });
}

#[test]
fn canonical_music_schema_keeps_device_paths_out_of_logical_roots() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        for object in [
            "music_library_items",
            "music_local_roots",
            "music_local_locations",
            "music_source_collections",
            "music_source_collection_items",
            "music_item_signals",
            "music_playlist_intended_uses",
            "music_playlist_memberships",
            "music_membership_skip_ranges",
            "music_snoozes",
            "music_listening_statistics",
            "music_recent_selections",
            "music_search_index_state",
            "music_search_fts",
            "idx_music_library_items_review",
            "idx_music_local_locations_root_availability",
            "idx_music_source_collection_items_order",
            "idx_music_playlist_memberships_order",
            "idx_music_snoozes_active_item",
            "idx_music_recent_selections_playlist",
            "idx_music_library_items_review_deferred_until",
        ] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE name = ?")
                    .bind(object)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{object} should exist");
        }

        let root_columns = sqlx::query("SELECT name FROM pragma_table_info('music_local_roots')")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
        assert!(!root_columns.iter().any(|column| {
            column.contains("path") || column.contains("folder") || column.contains("directory")
        }));
        let item_columns = sqlx::query("SELECT name FROM pragma_table_info('music_library_items')")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
        assert!(
            item_columns
                .iter()
                .any(|column| column == "review_deferred_until_ms")
        );
        let membership_columns =
            sqlx::query("SELECT name FROM pragma_table_info('music_playlist_memberships')")
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .map(|row| row.get::<String, _>("name"))
                .collect::<Vec<_>>();
        assert!(
            !membership_columns
                .iter()
                .any(|column| column == "focus_fit")
        );
    });
}

#[test]
fn canonical_music_schema_enforces_identity_membership_and_snooze_invariants() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Focus', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, youtube_video_id, original_title,
                 availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES
                ('item-1', 'local:item-1', 'local-file', NULL, 'Focus',
                 'available', 'unreviewed', 1700000000000, 1700000000000),
                ('item-2', 'youtube:video-2', 'youtube-video', 'video-2', 'Online',
                 'available', 'reviewed', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, position, created_at_ms, updated_at_ms)
             VALUES ('membership-1', 'playlist-1', 'item-1', 0, 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let duplicate_membership = sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, position, created_at_ms, updated_at_ms)
             VALUES ('membership-2', 'playlist-1', 'item-1', 1, 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_membership.is_err());

        let local_item_with_youtube_id = sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, youtube_video_id, discovered_at_ms, updated_at_ms)
             VALUES ('bad-item', 'local:bad', 'local-file', 'video-bad', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await;
        assert!(local_item_with_youtube_id.is_err());

        let playlist_snooze_without_playlist = sqlx::query(
            "INSERT INTO music_snoozes
                (id, item_id, scope, playlist_id, starts_at_ms, created_at_ms)
             VALUES ('bad-snooze', 'item-1', 'playlist', NULL, 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await;
        assert!(playlist_snooze_without_playlist.is_err());

        let invalid_weight = sqlx::query(
            "UPDATE music_playlist_memberships SET weight = 'always' WHERE id = 'membership-1'",
        )
        .execute(&pool)
        .await;
        assert!(invalid_weight.is_err());
    });
}

#[test]
fn default_playlists_have_a_complete_order_without_memberships_or_obsolete_resume_storage() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;

        let playlists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_playlists")
            .fetch_one(&pool)
            .await
            .unwrap();
        let memberships: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM music_playlist_memberships")
                .fetch_one(&pool)
                .await
                .unwrap();
        let sort_orders: Vec<i64> =
            sqlx::query_scalar("SELECT sort_order FROM music_playlists ORDER BY sort_order")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(playlists, 11);
        assert_eq!(memberships, 0);
        assert_eq!(sort_orders, (0..11).collect::<Vec<_>>());

        let obsolete_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'music_playback_states'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(obsolete_tables, 0);
    });
}

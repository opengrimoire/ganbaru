use super::helpers::migrated_memory_pool;
use crate::run_migrations;
use sqlx::Row;

#[test]
fn repeated_migration_startup_preserves_music_data() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO music_playlists (id, name, icon, order_key, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Deep focus', 'emoji:🎧', 'a0', 1, 1)",
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
            "idx_music_playlist_memberships_break_item",
            "idx_music_playlists_order",
            "idx_music_listening_statistics_item",
            "music_playlists_order_key_insert",
            "music_playlist_memberships_order_key_insert",
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
            "INSERT INTO music_playlists (id, name, order_key, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Focus', 'a0', 1700000000000, 1700000000000)",
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
                (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
             VALUES ('membership-1', 'playlist-1', 'item-1', 'a0', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let duplicate_membership = sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
             VALUES ('membership-2', 'playlist-1', 'item-1', 'a1', 1700000000000, 1700000000000)",
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
        let order: Vec<(String, String)> =
            sqlx::query_as("SELECT id, order_key FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(playlists, 11);
        assert_eq!(memberships, 0);
        assert_eq!(
            order.first().map(|(id, _)| id.as_str()),
            Some("playlist-default-start-of-day")
        );
        assert_eq!(
            order.last().map(|(id, _)| id.as_str()),
            Some("playlist-default-commute")
        );
        let keys = order
            .iter()
            .map(|(_, key)| key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            keys.iter()
                .all(|key| key.len() == 4 && key.starts_with('c'))
        );

        let obsolete_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'music_playback_states'",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(obsolete_tables, 0);
    });
}

#[test]
fn fresh_vaults_seed_one_rain_preset_per_generated_sound() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let presets: Vec<(String, String, i64, String, i64, String)> = sqlx::query_as(
            "SELECT p.id, p.name, p.level_milli, s.soundscape_id, s.level_milli, s.order_key
             FROM music_soundscape_presets p JOIN music_soundscape_preset_sounds s ON s.preset_id = p.id
             ORDER BY p.id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            presets,
            vec![
                (
                    "preset-generated-brown-noise".into(),
                    "Rain (soft)".into(),
                    1250,
                    "generated-brown-noise".into(),
                    1000,
                    "c000".into()
                ),
                (
                    "preset-generated-pink-noise".into(),
                    "Rain (moderate)".into(),
                    750,
                    "generated-pink-noise".into(),
                    1000,
                    "c000".into()
                ),
                (
                    "preset-generated-white-noise".into(),
                    "Rain (heavy)".into(),
                    500,
                    "generated-white-noise".into(),
                    1000,
                    "c000".into()
                ),
            ]
        );
        let state: (Option<String>, i64) = sqlx::query_as(
            "SELECT active_preset_id, desired_playing FROM music_soundscape_state WHERE singleton = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, (None, 0));
        let layers: Vec<(String, String)> =
            sqlx::query_as("SELECT id, icon FROM music_soundscapes ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            layers,
            ["brown", "pink", "white"]
                .map(|kind| (
                    format!("generated-{kind}-noise"),
                    "lucide:audio-lines".into()
                ))
                .to_vec()
        );
        let group_schema: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'music_soundscape_groups')
                + (SELECT COUNT(*) FROM pragma_table_info('music_soundscapes') WHERE name = 'group_id')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(group_schema, 0);
        let duplicate_generated = sqlx::query(
            "INSERT INTO music_soundscapes (id, source_kind, generated_kind, name, availability, created_at_ms, updated_at_ms)
             VALUES ('second-white', 'generated-noise', 'white', 'White noise', 'available', 1, 1)",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_generated.is_err());
        for sql in [
            "UPDATE music_soundscape_presets SET level_milli = 2001",
            "UPDATE music_soundscape_preset_sounds SET level_milli = 1001",
            "UPDATE music_soundscape_preset_sounds SET level_milli = -1",
            "UPDATE music_soundscape_preset_sounds SET order_key = ''",
        ] {
            assert!(sqlx::query(sql).execute(&pool).await.is_err(), "{sql}");
        }
        let pending_preset_sounds: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_soundscape_preset_sounds WHERE sync_key GLOB 'pending:*'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            pending_preset_sounds, 0,
            "seeded preset sounds carry their derived keys"
        );
    });
}

#[test]
fn deleting_a_break_item_keeps_its_range_and_deleting_a_preset_clears_it() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES ('item-1', 'local:1', 'local-file', 'available', 'reviewed', 1, 1),
                    ('item-2', 'local:2', 'local-file', 'available', 'reviewed', 1, 1);
             INSERT INTO music_playlists (id, name, order_key, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Playlist', 'a0', 1, 1);
             INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, order_key, break_item_id, break_start_ms, break_end_ms,
                 created_at_ms, updated_at_ms)
             VALUES ('membership-1', 'playlist-1', 'item-1', 'a0', 'item-2', 1000, 2000, 1, 1);
             UPDATE music_soundscape_state SET active_preset_id = 'preset-generated-pink-noise';
             DELETE FROM music_library_items WHERE id = 'item-2';
             DELETE FROM music_soundscape_presets WHERE id = 'preset-generated-pink-noise';",
        )
        .execute(&pool)
        .await
        .unwrap();

        let cleared: (Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT break_item_id, break_start_ms FROM music_playlist_memberships
             WHERE id = 'membership-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cleared, (None, Some(1000)));
        let active: Option<String> = sqlx::query_scalar(
            "SELECT active_preset_id FROM music_soundscape_state WHERE singleton = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(active, None);
        let orphaned_sounds: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_soundscape_preset_sounds
             WHERE preset_id = 'preset-generated-pink-noise'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(orphaned_sounds, 0);
    });
}

#[test]
fn music_sync_shape_rejects_missing_keys_bad_ranges_and_extra_skips() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES ('item-1', 'local:1', 'local-file', 'available', 'reviewed', 1, 1);
             INSERT INTO music_playlists (id, name, order_key, created_at_ms, updated_at_ms)
             VALUES ('p', 'P', 'a0', 1, 1);
             INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, order_key, created_at_ms, updated_at_ms)
             VALUES ('m', 'p', 'item-1', 'a0', 1, 1);
             INSERT INTO music_listening_statistics
                (device_id, item_id, play_count, updated_at_ms, sync_key)
             VALUES ('device-a', 'item-1', 2, 1, 'statistics-a'), ('device-b', 'item-1', 3, 1, 'statistics-b');",
        )
        .execute(&pool)
        .await
        .unwrap();

        for sql in [
            "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms) VALUES ('q', 'Q', 1, 1)",
            "UPDATE music_playlists SET order_key = '' WHERE id = 'p'",
            "INSERT INTO music_playlist_memberships (id, playlist_id, item_id, created_at_ms, updated_at_ms)
             VALUES ('n', 'p', 'item-1', 1, 1)",
            "UPDATE music_playlist_memberships SET order_key = '' WHERE id = 'm'",
            "UPDATE music_playlist_memberships SET volume_milli = 1001",
            "UPDATE music_playlist_memberships SET rate_milli = 249",
            "UPDATE music_playlist_memberships SET break_start_ms = 10, break_end_ms = 5",
            "UPDATE music_playlist_memberships SET break_rate_milli = 2001",
            "INSERT INTO music_membership_skip_ranges (membership_id, ordinal, start_ms, end_ms)
             VALUES ('m', 64, 0, 1)",
            "INSERT INTO music_listening_statistics (device_id, item_id, updated_at_ms, sync_key)
             VALUES (' ', 'item-1', 1, 'statistics-blank')",
            "INSERT INTO music_item_signals (item_id, signal, created_at_ms)
             VALUES ('item-1', 'calm', 1)",
        ] {
            assert!(sqlx::query(sql).execute(&pool).await.is_err(), "{sql}");
        }
        let total: i64 = sqlx::query_scalar(
            "SELECT SUM(play_count) FROM music_listening_statistics WHERE item_id = 'item-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(total, 5);
    });
}

#[test]
fn artwork_overrides_require_a_managed_reference_with_its_size() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES ('item-1', 'local:1', 'local-file', 'available', 'reviewed', 1, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let reference = format!("music/artwork/{}.png", "a".repeat(64));

        for (artwork, size) in [
            (Some("/home/user/cover.png"), Some(10)),
            (Some(reference.as_str()), None),
            (None, Some(10)),
            (Some(reference.as_str()), Some(0)),
            (Some(reference.as_str()), Some(10_485_761)),
        ] {
            let result = sqlx::query(
                "UPDATE music_library_items
                 SET artwork_override = ?, artwork_override_byte_size = ? WHERE id = 'item-1'",
            )
            .bind(artwork)
            .bind(size)
            .execute(&pool)
            .await;
            assert!(result.is_err(), "{artwork:?} {size:?}");
        }
        for (artwork, size) in [(Some(reference.as_str()), Some(10_485_760)), (None, None)] {
            sqlx::query(
                "UPDATE music_library_items
                 SET artwork_override = ?, artwork_override_byte_size = ? WHERE id = 'item-1'",
            )
            .bind(artwork)
            .bind(size)
            .execute(&pool)
            .await
            .unwrap();
        }
    });
}

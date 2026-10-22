use super::helpers::{memory_pool_migrated_through, migrated_memory_pool};
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
    });
}

#[test]
fn preset_mix_migration_turns_sound_choices_into_presets() {
    super::block_on(async {
        const BEFORE_PRESET_MIXES: i64 = 20261019120000;
        let pool = memory_pool_migrated_through(BEFORE_PRESET_MIXES).await;
        sqlx::raw_sql(
            "INSERT INTO music_soundscapes (id, source_kind, name, icon, availability, created_at_ms, updated_at_ms)
                 VALUES ('loop-1', 'local-loop', 'Thunder', 'lucide:cloud-lightning', 'available', 1, 1);
             INSERT INTO music_context_assignments
                 (owner_kind, owner_id, phase, behavior, soundscape_id, provenance_kind, updated_at_ms, soundscape_behavior)
                 VALUES ('project-default', 'project-1', 'focus', 'inherit', 'loop-1', 'explicit', 1, 'play-selected'),
                        ('project-default', 'project-1', 'short-break', 'inherit', 'generated-pink-noise', 'explicit', 1, 'play-selected'),
                        ('project-default', 'project-1', 'long-break', 'inherit', 'removed-sound', 'explicit', 1, 'play-selected');
             UPDATE music_soundscape_state SET active_soundscape_id = 'generated-brown-noise', desired_playing = 1, volume = 0.4;",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let assignments: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT phase, soundscape_preset_id FROM music_context_assignments ORDER BY phase",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            assignments,
            vec![
                ("focus".into(), Some("preset-loop-1".into())),
                ("long-break".into(), Some("preset-removed-sound".into())),
                (
                    "short-break".into(),
                    Some("preset-generated-pink-noise".into())
                ),
            ]
        );
        let local: (String, String, String) = sqlx::query_as(
            "SELECT p.name, p.icon, s.soundscape_id FROM music_soundscape_presets p
             JOIN music_soundscape_preset_sounds s ON s.preset_id = p.id WHERE p.id = 'preset-loop-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            local,
            (
                "Thunder".into(),
                "lucide:cloud-lightning".into(),
                "loop-1".into()
            )
        );
        let dangling: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_soundscape_presets WHERE id = 'preset-removed-sound'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(dangling, 0);
        let state: (Option<String>, i64) = sqlx::query_as(
            "SELECT active_preset_id, desired_playing FROM music_soundscape_state WHERE singleton = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, (Some("preset-generated-brown-noise".into()), 1));
        let removed_columns: i64 = sqlx::query_scalar(
            "SELECT
                (SELECT COUNT(*) FROM pragma_table_info('music_context_assignments') WHERE name = 'soundscape_id')
                + (SELECT COUNT(*) FROM pragma_table_info('calendar_event_archive_music_assignments') WHERE name = 'soundscape_id')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removed_columns, 0);
    });
}

#[test]
fn sync_shape_migration_keeps_order_values_breaks_skips_and_statistics() {
    super::block_on(async {
        const BEFORE_SYNC_SHAPE: i64 = 20261022061954;
        let pool = memory_pool_migrated_through(BEFORE_SYNC_SHAPE).await;
        sqlx::raw_sql(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES ('item-1', 'local:1', 'local-file', 'available', 'reviewed', 1, 1),
                    ('item-2', 'local:2', 'local-file', 'available', 'reviewed', 1, 1),
                    ('item-3', 'local:3', 'local-file', 'available', 'reviewed', 1, 1);
             INSERT INTO music_playlists (id, name, sort_order, created_at_ms, updated_at_ms)
             VALUES ('p-b', 'B', 20, 1, 1), ('p-a', 'a', 20, 1, 1);
             INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, position, volume, rate, created_at_ms, updated_at_ms)
             VALUES ('m1', 'p-a', 'item-1', 1, 0.5, 1.25, 1, 1),
                    ('m2', 'p-a', 'item-2', 0, NULL, NULL, 1, 1),
                    ('m3', 'p-b', 'item-1', 0, 0.3333, NULL, 1, 1);
             INSERT INTO music_membership_break_items
                (membership_id, item_id, start_ms, end_ms, volume, rate)
             VALUES ('m1', 'item-3', 1000, 2000, 0.25, 0.75);
             INSERT INTO music_membership_skip_ranges (id, membership_id, start_ms, end_ms, sort_order)
             VALUES ('s1', 'm1', 5000, 6000, 0), ('s2', 'm1', 1000, 2000, 1);
             INSERT INTO music_soundscape_presets (id, name, icon, level, created_at_ms, updated_at_ms)
             VALUES ('preset-custom', 'Custom', 'lucide:waves', 1.5, 1, 1);
             INSERT INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id, level)
             VALUES ('preset-custom', 1, 'generated-white-noise', 0.25),
                    ('preset-custom', 0, 'generated-pink-noise', 0.8);
             INSERT INTO music_listening_statistics
                (item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms)
             VALUES ('item-1', 100, 3, 2, 1, 100), ('item-2', NULL, 0, 0, 4, 50);",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let playlists: Vec<String> =
            sqlx::query_scalar("SELECT id FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            playlists[9..],
            [
                "playlist-default-cooking",
                "playlist-default-commute",
                "p-a",
                "p-b"
            ]
        );
        let memberships: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT id, volume_milli, rate_milli FROM music_playlist_memberships
             ORDER BY playlist_id, order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            memberships,
            vec![
                ("m2".into(), None, None),
                ("m1".into(), Some(500), Some(1250)),
                ("m3".into(), Some(333), None),
            ]
        );
        type BreakRow = (
            String,
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
        );
        let breaks: Vec<BreakRow> = sqlx::query_as(
            "SELECT id, break_item_id, break_start_ms, break_end_ms, break_volume_milli, break_rate_milli
             FROM music_playlist_memberships ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            breaks[0],
            (
                "m1".into(),
                Some("item-3".into()),
                Some(1000),
                Some(2000),
                Some(250),
                Some(750)
            )
        );
        assert!(
            breaks[1..]
                .iter()
                .all(|row| row.1.is_none() && row.2.is_none())
        );
        let skips: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT ordinal, start_ms, end_ms FROM music_membership_skip_ranges
             WHERE membership_id = 'm1' ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(skips, vec![(0, 1000, 2000), (1, 5000, 6000)]);
        let preset_level: i64 = sqlx::query_scalar(
            "SELECT level_milli FROM music_soundscape_presets WHERE id = 'preset-custom'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preset_level, 1500);
        let sounds: Vec<(String, i64)> = sqlx::query_as(
            "SELECT soundscape_id, level_milli FROM music_soundscape_preset_sounds
             WHERE preset_id = 'preset-custom' ORDER BY order_key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            sounds,
            vec![
                ("generated-pink-noise".into(), 800),
                ("generated-white-noise".into(), 250)
            ]
        );
        let statistics: Vec<(String, String, Option<i64>, i64, i64, i64)> = sqlx::query_as(
            "SELECT device_id, item_id, last_played_at_ms, play_count, completion_count, skip_count
             FROM music_listening_statistics ORDER BY item_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            statistics,
            vec![
                ("vault".into(), "item-1".into(), Some(100), 3, 2, 1),
                ("vault".into(), "item-2".into(), None, 0, 0, 4),
            ]
        );
        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);
        for removed in [
            "SELECT COUNT(*) FROM pragma_table_info('music_playlists') WHERE name = 'sort_order'",
            "SELECT COUNT(*) FROM pragma_table_info('music_playlist_memberships')
             WHERE name IN ('position', 'volume', 'rate')",
            "SELECT COUNT(*) FROM pragma_table_info('music_membership_skip_ranges')
             WHERE name IN ('id', 'sort_order')",
            "SELECT COUNT(*) FROM pragma_table_info('music_soundscape_presets') WHERE name = 'level'",
            "SELECT COUNT(*) FROM pragma_table_info('music_soundscape_preset_sounds')
             WHERE name IN ('position', 'level')",
            "SELECT COUNT(*) FROM sqlite_schema WHERE name = 'music_membership_break_items'",
        ] {
            let count: i64 = sqlx::query_scalar(removed).fetch_one(&pool).await.unwrap();
            assert_eq!(count, 0, "{removed}");
        }

        sqlx::query("DELETE FROM music_library_items WHERE id = 'item-3'")
            .execute(&pool)
            .await
            .unwrap();
        let cleared: (Option<String>, Option<i64>) = sqlx::query_as(
            "SELECT break_item_id, break_start_ms FROM music_playlist_memberships WHERE id = 'm1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cleared, (None, Some(1000)));
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
                (device_id, item_id, play_count, updated_at_ms)
             VALUES ('device-a', 'item-1', 2, 1), ('device-b', 'item-1', 3, 1);",
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
            "INSERT INTO music_listening_statistics (device_id, item_id, updated_at_ms)
             VALUES (' ', 'item-1', 1)",
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

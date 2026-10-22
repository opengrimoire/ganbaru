use super::helpers::{memory_pool_migrated_through, migrated_memory_pool};
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

#[test]
fn fresh_vaults_seed_one_rain_preset_per_generated_sound() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let presets: Vec<(String, String, f64, String, f64)> = sqlx::query_as(
            "SELECT p.id, p.name, p.level, s.soundscape_id, s.level
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
                    1.25,
                    "generated-brown-noise".into(),
                    1.0
                ),
                (
                    "preset-generated-pink-noise".into(),
                    "Rain (moderate)".into(),
                    0.75,
                    "generated-pink-noise".into(),
                    1.0
                ),
                (
                    "preset-generated-white-noise".into(),
                    "Rain (heavy)".into(),
                    0.5,
                    "generated-white-noise".into(),
                    1.0
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
            "UPDATE music_soundscape_presets SET level = 2.1",
            "UPDATE music_soundscape_preset_sounds SET level = 1.1",
            "UPDATE music_soundscape_preset_sounds SET level = -0.1",
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

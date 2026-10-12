use super::*;
use super::{library_window, membership, playlist, pool, seed_item};
use sqlx::Row;

#[test]
fn review_window_includes_ignored_items_for_local_visibility_filtering() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for id in ["unreviewed", "reviewed", "ignored", "due", "future"] {
            seed_item(&pool, id, &format!("local:{id}")).await;
        }
        sqlx::query(
            "UPDATE music_library_items
             SET review_state = CASE id
                 WHEN 'reviewed' THEN 'reviewed'
                 WHEN 'ignored' THEN 'ignored'
                 WHEN 'due' THEN 'deferred'
                 WHEN 'future' THEN 'deferred'
                 ELSE review_state END,
                 review_deferred_until_ms = CASE id
                 WHEN 'due' THEN 1700000000000
                 WHEN 'future' THEN 1800000000000
                 ELSE NULL END",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut request = library_window();
        request.destination = MusicListDestination::Review;
        request.limit = 20;
        let result = super::queries::item_window(&pool, request).await.unwrap();
        let ids = result
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec!["due", "future", "ignored", "reviewed", "unreviewed"]
        );
    });
}

#[test]
fn item_windows_are_bounded_stable_filterable_and_grouped() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut transaction = pool.begin().await.unwrap();
        for index in 0..600 {
            sqlx::query(
                "INSERT INTO music_library_items
                    (id, identity_key, source_kind, media_kind, youtube_video_id,
                     original_title, original_artist,
                     original_album, availability, review_state, discovered_at_ms, updated_at_ms)
                 VALUES (?, ?, ?, 'audio', ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(format!("item-{index:04}"))
            .bind(format!("identity-{index:04}"))
            .bind(if index % 3 == 0 {
                "youtube-video"
            } else {
                "local-file"
            })
            .bind(if index % 3 == 0 {
                Some(format!("video-{index:04}"))
            } else {
                None
            })
            .bind(format!("Track {index:04}"))
            .bind(if index % 2 == 0 {
                "Alpha Composer"
            } else {
                "Beta Composer"
            })
            .bind(format!("Album {:02}", index % 12))
            .bind(if index % 17 == 0 {
                "missing"
            } else {
                "available"
            })
            .bind(if index % 5 == 0 {
                "unreviewed"
            } else {
                "reviewed"
            })
            .bind(1_700_000_000_000_i64 + index)
            .bind(1_700_000_000_000_i64 + index)
            .execute(&mut *transaction)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
             VALUES ('window-root', 'Music', 1700000000000, 1700000000000)",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_local_locations
                (id, item_id, root_id, relative_path, availability, first_seen_at_ms, updated_at_ms)
             VALUES ('window-location', 'item-0001', 'window-root', 'Games/Nier/theme.flac',
                 'available', 1700000000000, 1700000000000)",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items
             SET original_artwork_identity = 'sidecar:Games/Nier/cover.jpg'
             WHERE id = 'item-0001'",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let first = super::queries::item_window(&pool, library_window())
            .await
            .unwrap();
        assert_eq!(first.total_count, 600);
        assert_eq!(first.items.len(), 50);
        assert_eq!(first.items.first().unwrap().title, "Track 0000");
        assert_eq!(first.items.last().unwrap().title, "Track 0049");
        assert_eq!(
            first.items[1].relative_path.as_deref(),
            Some("Games/Nier/theme.flac")
        );
        assert_eq!(first.items[1].local_root_id.as_deref(), Some("window-root"));
        assert_eq!(
            first.items[1].original_artwork_identity.as_deref(),
            Some("sidecar:Games/Nier/cover.jpg")
        );

        let mut filtered = library_window();
        filtered.search = "Alpha".to_string();
        filtered.availability = Some(MusicItemAvailability::Available);
        filtered.group_by = MusicGroupBy::SourceKind;
        filtered.limit = 25;
        let result = super::queries::item_window(&pool, filtered).await.unwrap();
        assert_eq!(result.items.len(), 25);
        assert!(result.total_count < 300);
        assert_eq!(
            result.groups.iter().map(|group| group.count).sum::<i64>(),
            result.total_count,
        );
        assert!(
            result
                .items
                .iter()
                .all(|item| item.artist == "Alpha Composer"
                    && item.availability == MusicItemAvailability::Available)
        );
    });
}

#[test]
fn playlist_window_uses_manual_order_and_returns_membership_state() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        for index in 0..3 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
            let mut entry = membership(index);
            entry.item_id = format!("item-{index}");
            entry.position = 2 - index as i64;
            super::writes::upsert_memberships(
                &pool,
                MusicBulkMembershipWrite {
                    memberships: vec![entry],
                },
            )
            .await
            .unwrap();
            sqlx::query(
                "UPDATE music_playlist_memberships SET created_at_ms = ? WHERE item_id = ?",
            )
            .bind(1_700_000_000_000_i64 + index as i64)
            .bind(format!("item-{index}"))
            .execute(&pool)
            .await
            .unwrap();
        }
        let mut request = library_window();
        request.destination = MusicListDestination::Playlist;
        request.playlist_id = Some("playlist-1".to_string());
        request.sort = MusicItemSort::ManualPosition;
        let result = super::queries::item_window(&pool, request).await.unwrap();

        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.membership_position)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)],
        );
        assert!(result.items.iter().all(|item| item.membership_id.is_some()));

        let mut added = library_window();
        added.destination = MusicListDestination::Playlist;
        added.playlist_id = Some("playlist-1".to_string());
        added.sort = MusicItemSort::AddedToPlaylist;
        added.direction = MusicSortDirection::Descending;
        let added = super::queries::item_window(&pool, added).await.unwrap();
        assert_eq!(
            added
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-2", "item-1", "item-0"]
        );

        let mut membership_filter = library_window();
        membership_filter.membership_playlist_id = Some("playlist-1".to_string());
        let filtered = super::queries::item_window(&pool, membership_filter)
            .await
            .unwrap();
        assert_eq!(filtered.total_count, 3);
    });
}

#[test]
fn source_order_and_artwork_overrides_are_projected_in_library_rows() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 0..3 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }
        sqlx::query(
            "INSERT INTO music_source_collections
                (id, kind, identity_key, name, youtube_playlist_id, created_at_ms, updated_at_ms)
             VALUES ('source-1', 'youtube-playlist', 'youtube:list', 'Source', 'list', ?, ?)",
        )
        .bind(1_700_000_000_000_i64)
        .bind(1_700_000_000_000_i64)
        .execute(&pool)
        .await
        .unwrap();
        for (item_id, position) in [("item-0", 2_i64), ("item-1", 0), ("item-2", 1)] {
            sqlx::query(
                "INSERT INTO music_source_collection_items
                    (collection_id, item_id, source_position, first_discovered_at_ms)
                 VALUES ('source-1', ?, ?, ?)",
            )
            .bind(item_id)
            .bind(position)
            .bind(1_700_000_000_000_i64)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("UPDATE music_library_items SET artwork_override = '/art/cover.png' WHERE id = 'item-2'")
            .execute(&pool)
            .await
            .unwrap();

        let mut request = library_window();
        request.sort = MusicItemSort::SourceOrder;
        let result = super::queries::item_window(&pool, request).await.unwrap();
        assert_eq!(
            result
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-1", "item-2", "item-0"]
        );
        assert!(
            result
                .items
                .iter()
                .all(|item| item.source_collection_ids == ["source-1"])
        );
        assert_eq!(
            result.items[1].artwork_override.as_deref(),
            Some("/art/cover.png")
        );
    });
}

#[test]
fn summaries_issues_and_inspector_return_composed_data_without_row_queries() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let linked = membership(1);
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![linked],
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
             VALUES ('root-1', 'Soundtracks', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_local_locations
                (id, item_id, root_id, relative_path, availability, first_seen_at_ms, updated_at_ms)
             VALUES ('location-1', 'item-1', 'root-1', 'Album/track.flac',
                 'available', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_source_collections
                (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
             VALUES ('source-1', 'local-root', 'root:root-1', 'Soundtracks', 'root-1',
                 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_source_collection_items
                (collection_id, item_id, first_discovered_at_ms)
             VALUES ('source-1', 'item-1', 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (group_by, expected_key) in [
            (MusicGroupBy::Folder, "Album"),
            (MusicGroupBy::SourceCollection, "Soundtracks"),
        ] {
            let mut request = library_window();
            request.group_by = group_by;
            let window = super::queries::item_window(&pool, request).await.unwrap();
            assert_eq!(window.groups.len(), 1);
            assert_eq!(window.groups[0].key, expected_key);
            assert_eq!(window.groups[0].count, 1);
        }
        sqlx::query(
            "INSERT INTO music_item_signals (item_id, signal, created_at_ms)
             VALUES ('item-1', 'calm', 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items
             SET availability = 'missing', updated_at_ms = 1700000000000
             WHERE id = 'item-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let playlists = super::queries::playlist_summaries(&pool, 1_700_000_100_000, 0, 20)
            .await
            .unwrap();
        let sources = super::queries::source_summaries(&pool, 1_700_000_000_000, 0, 20)
            .await
            .unwrap();
        let roots = super::queries::local_roots(&pool, 0, 20).await.unwrap();
        let collections = super::queries::source_collections(&pool, 0, 20)
            .await
            .unwrap();
        let playlist_detail = super::queries::playlist_detail(&pool, "playlist-1")
            .await
            .unwrap();
        let issues = super::queries::issues(&pool, 0, 20).await.unwrap();
        let detail = super::queries::inspector_detail(&pool, "item-1")
            .await
            .unwrap();

        assert_eq!(
            playlists
                .iter()
                .find(|playlist| playlist.id == "playlist-1")
                .map(|playlist| playlist.total_count),
            Some(1),
        );
        assert_eq!(sources[0].item_count, 1);
        assert_eq!(sources[0].open_issue_count, 1);
        assert_eq!(roots[0].name, "Soundtracks");
        assert_eq!(collections[0].kind, MusicCollectionKind::LocalRoot);
        assert_eq!(playlist_detail.intended_uses, vec![MusicIntendedUse::Focus]);
        assert_eq!(issues[0].id, "availability:item-1");
        assert_eq!(detail.locations.len(), 1);
        assert_eq!(detail.memberships.len(), 1);
        assert_eq!(detail.signals, vec![MusicItemSignal::Calm]);
        assert_eq!(detail.source_collection_ids, vec!["source-1"]);
    });
}

#[test]
fn review_and_membership_queries_use_purpose_built_indexes() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let review_plan = sqlx::query(
            "EXPLAIN QUERY PLAN
             SELECT id FROM music_library_items
             WHERE review_state = 'unreviewed'
             ORDER BY discovered_at_ms, id LIMIT 50",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
        let membership_plan = sqlx::query(
            "EXPLAIN QUERY PLAN
             SELECT id FROM music_playlist_memberships
             WHERE playlist_id = 'playlist-1'
             ORDER BY position, id LIMIT 50",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");

        assert!(review_plan.contains("idx_music_library_items_review"));
        assert!(membership_plan.contains("idx_music_playlist_memberships_order"));
    });
}

#[test]
fn search_rebuild_repairs_stale_rows_and_incremental_membership_metadata() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO music_library_items
                (id, identity_key, source_kind, media_kind, original_title, original_artist,
                 original_album, availability, review_state, discovered_at_ms, updated_at_ms)
             VALUES ('item-1', 'local:item-1', 'local-file', 'audio',
                 'Café de la pluie 雨', 'Artista', 'Lectura tranquila',
                 'available', 'unreviewed', 1700000000000, 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let rebuilt = super::search::rebuild(&pool, 1_700_000_100_000)
            .await
            .unwrap();
        assert_eq!(rebuilt.indexed_item_count, 1);

        let mut by_diacritic = library_window();
        by_diacritic.search = "cafe".to_string();
        assert_eq!(
            super::queries::item_window(&pool, by_diacritic)
                .await
                .unwrap()
                .total_count,
            1,
        );
        let mut by_cjk = library_window();
        by_cjk.search = "雨".to_string();
        assert_eq!(
            super::queries::item_window(&pool, by_cjk)
                .await
                .unwrap()
                .total_count,
            1,
        );

        sqlx::query("DELETE FROM music_search_fts")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE music_search_index_state SET fingerprint = 'stale' WHERE singleton = 1",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut repaired = library_window();
        repaired.search = "tranquila".to_string();
        assert_eq!(
            super::queries::item_window(&pool, repaired)
                .await
                .unwrap()
                .total_count,
            1,
        );

        super::writes::create_playlist(
            &pool,
            MusicPlaylistCreate {
                name: "Morning flow".to_string(),
                ..playlist("playlist-1")
            },
        )
        .await
        .unwrap();
        let linked = membership(1);
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![linked],
            },
        )
        .await
        .unwrap();
        let mut by_playlist = library_window();
        by_playlist.search = "Morning".to_string();
        assert_eq!(
            super::queries::item_window(&pool, by_playlist)
                .await
                .unwrap()
                .total_count,
            1,
        );

        super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Dawn routine".to_string(),
                icon: "lucide:sunrise".to_string(),
                shuffle_enabled: true,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::All,
                intended_uses: vec![MusicIntendedUse::General],
                expected_version: 1,
                updated_at_ms: 1_700_000_200_000,
            },
        )
        .await
        .unwrap();
        let mut by_renamed_playlist = library_window();
        by_renamed_playlist.search = "Dawn".to_string();
        assert_eq!(
            super::queries::item_window(&pool, by_renamed_playlist)
                .await
                .unwrap()
                .total_count,
            1,
        );
    });
}

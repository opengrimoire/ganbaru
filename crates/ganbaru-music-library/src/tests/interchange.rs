use super::*;

fn item(identity: &str) -> MusicInterchangeItem {
    MusicInterchangeItem {
        identity_key: identity.to_string(),
        source_kind: MusicLibrarySourceKind::LocalFile,
        youtube_video_id: None,
        title: "Quiet rain".to_string(),
        artist: "Composer".to_string(),
        album: "Soundtrack".to_string(),
        duration_ms: Some(60_000),
        signals: vec![MusicItemSignal::Calm, MusicItemSignal::Repetitive],
        locations: vec![MusicInterchangeLocation {
            root_id: "root-1".to_string(),
            relative_path: "Album/Quiet rain.flac".to_string(),
            availability: MusicLocationAvailability::Available,
        }],
    }
}

pub(super) fn request(conflict: MusicImportPlaylistConflict) -> MusicInterchangeImportRequest {
    MusicInterchangeImportRequest {
        document: MusicInterchangeDocument {
            format: "ganbaru/music-playlists".to_string(),
            version: 2,
            exported_at: 1_700_000_000_000,
            roots: vec![MusicInterchangeRoot {
                id: "root-1".to_string(),
                name: "Soundtracks".to_string(),
            }],
            playlists: vec![MusicInterchangePlaylist {
                id: "playlist-1".to_string(),
                name: "Focus".to_string(),
                icon: "lucide:laptop".to_string(),
                shuffle_enabled: true,
                mix_enabled: true,
                repeat_mode: MusicRepeatMode::All,
                intended_uses: vec![MusicIntendedUse::Focus],
                memberships: vec![MusicInterchangeMembership {
                    item: item("local:fingerprint-1"),
                    weight: MusicWeight::LessOften,
                    enabled: true,
                    start_ms: Some(1_000),
                    end_ms: Some(50_000),
                    volume_milli: Some(800),
                    rate_milli: Some(1_000),
                    skip_ranges: vec![MusicInterchangeRange {
                        start_ms: 10_000,
                        end_ms: 12_000,
                    }],
                    snoozes: vec![MusicInterchangeSnooze {
                        scope: MusicSnoozeScope::Playlist,
                        starts_at_ms: 1_700_000_000_000,
                        ends_at_ms: Some(1_700_086_400_000),
                        reason: "Rest".to_string(),
                    }],
                }],
            }],
            context_assignments: vec![],
            warnings: vec![],
        },
        playlist_conflict: conflict,
        replace_item_descriptions: false,
        import_context_assignments: false,
        imported_at: 1_700_000_100_000,
    }
}

#[test]
fn interchange_import_commits_full_playlist_state_transactionally() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let result = super::super::interchange::import(
            &pool,
            request(MusicImportPlaylistConflict::ImportCopy),
        )
        .await
        .unwrap();
        assert_eq!(result.playlist_count, 1);
        assert_eq!(result.item_count, 1);
        assert_eq!(result.membership_count, 1);
        assert!(
            super::queries::playlist_detail(&pool, "playlist-1")
                .await
                .unwrap()
                .mix_enabled
        );

        let row: (String, String, Option<i64>, Option<i64>, i64) = sqlx::query_as(
            "SELECT weight, p.name, start_ms, end_ms, volume_milli
             FROM music_playlist_memberships AS m
             JOIN music_playlists AS p ON p.id = m.playlist_id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row,
            (
                "less-often".to_string(),
                "Focus".to_string(),
                Some(1_000),
                Some(50_000),
                800,
            )
        );
        let signals: Vec<String> =
            sqlx::query_scalar("SELECT signal FROM music_item_signals ORDER BY signal")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(signals, vec!["calm", "repetitive"]);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_membership_skip_ranges")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_snoozes")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn interchange_import_rejects_unsafe_paths_before_writing() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let mut unsafe_request = request(MusicImportPlaylistConflict::ImportCopy);
        for unsafe_path in [
            "../outside.flac",
            "folder\\..\\outside.flac",
            "track:stream",
        ] {
            unsafe_request.document.playlists[0].memberships[0]
                .item
                .locations[0]
                .relative_path = unsafe_path.to_string();
            assert!(
                super::super::interchange::import(&pool, unsafe_request.clone())
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_playlists")
                .fetch_one(&pool)
                .await
                .unwrap(),
            super::defaults::BUILT_IN_MUSIC_PLAYLISTS.len() as i64
        );
    });
}

#[test]
fn interchange_import_deduplicates_item_wide_snoozes_across_playlists() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let mut import_request = request(MusicImportPlaylistConflict::ImportCopy);
        let mut second_playlist = import_request.document.playlists[0].clone();
        second_playlist.id = "playlist-2".to_string();
        second_playlist.name = "Reading".to_string();
        import_request.document.playlists[0].memberships[0].snoozes[0].scope =
            MusicSnoozeScope::AllPlaylists;
        second_playlist.memberships[0].snoozes[0].scope = MusicSnoozeScope::AllPlaylists;
        import_request.document.playlists.push(second_playlist);

        super::super::interchange::import(&pool, import_request)
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_snoozes")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    });
}

#[test]
fn interchange_conflict_policy_never_overwrites_without_replace() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        super::super::interchange::import(&pool, request(MusicImportPlaylistConflict::ImportCopy))
            .await
            .unwrap();
        let mut second = request(MusicImportPlaylistConflict::KeepExisting);
        second.imported_at += 1;
        second.document.playlists[0].name = "Changed".to_string();
        let result = super::super::interchange::import(&pool, second)
            .await
            .unwrap();
        assert_eq!(result.playlist_count, 0);
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT name FROM music_playlists WHERE id = 'playlist-1'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "Focus"
        );
    });
}

#[test]
fn interchange_import_truncates_item_descriptions_that_the_schema_caps() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let mut import_request = request(MusicImportPlaylistConflict::ImportCopy);
        let item = &mut import_request.document.playlists[0].memberships[0].item;
        item.title = "é".repeat(MAX_ITEM_DESCRIPTION_CHARS * 20);
        item.artist = "a".repeat(MAX_ITEM_DESCRIPTION_CHARS + 1);

        super::super::interchange::import(&pool, import_request)
            .await
            .unwrap();
        let stored: (i64, i64, i64) = sqlx::query_as(
            "SELECT length(original_title), length(original_artist), length(original_album)
             FROM music_library_items",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let limit = MAX_ITEM_DESCRIPTION_CHARS as i64;
        assert_eq!(stored, (limit, limit, "Soundtrack".len() as i64));

        super::seed_item(&pool, "item-direct", "local:direct").await;
        for column in ["original_title", "original_artist", "original_album"] {
            let update = format!("UPDATE music_library_items SET {column} = ? WHERE id = ?");
            assert!(
                sqlx::query(sqlx::AssertSqlSafe(update))
                    .bind("x".repeat(MAX_ITEM_DESCRIPTION_CHARS + 1))
                    .bind("item-direct")
                    .execute(&pool)
                    .await
                    .is_err(),
                "{column} accepts text past the cap"
            );
        }
    });
}

#[test]
fn interchange_replace_keeps_derived_rows_and_removes_absent_memberships() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        super::super::interchange::import(&pool, request(MusicImportPlaylistConflict::ImportCopy))
            .await
            .unwrap();
        let item_id = crate::identity::import_item_id("local:fingerprint-1");
        let membership_id = crate::identity::membership_id("playlist-1", &item_id);
        let first: (String, i64, i64) = sqlx::query_as(
            "SELECT id, version, updated_at_ms FROM music_playlist_memberships WHERE item_id = ?",
        )
        .bind(&item_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(first.0, membership_id);
        let location: String =
            sqlx::query_scalar("SELECT id FROM music_local_locations WHERE item_id = ?")
                .bind(&item_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            location,
            crate::identity::location_id("root-1", "Album/Quiet rain.flac")
        );

        let mut replace = request(MusicImportPlaylistConflict::ReplaceExisting);
        replace.imported_at += 1;
        let mut added = replace.document.playlists[0].memberships[0].clone();
        added.item = item("local:fingerprint-2");
        added.item.locations[0].relative_path = "Album/Second.flac".to_string();
        replace.document.playlists[0].memberships.push(added);
        super::super::interchange::import(&pool, replace.clone())
            .await
            .unwrap();
        let kept: (String, i64, i64) = sqlx::query_as(
            "SELECT id, version, updated_at_ms FROM music_playlist_memberships WHERE item_id = ?",
        )
        .bind(&item_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept, first);

        replace.imported_at += 1;
        replace.document.playlists[0].memberships.reverse();
        super::super::interchange::import(&pool, replace.clone())
            .await
            .unwrap();
        let ordered: Vec<(String, i64)> = sqlx::query_as(
            "SELECT item_id, version FROM music_playlist_memberships
             WHERE playlist_id = 'playlist-1' ORDER BY order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            ordered,
            vec![
                (crate::identity::import_item_id("local:fingerprint-2"), 2),
                (item_id.clone(), first.1),
            ],
            "only the moved membership takes a new key"
        );

        replace.imported_at += 1;
        replace.document.playlists[0].memberships.remove(0);
        super::super::interchange::import(&pool, replace)
            .await
            .unwrap();
        let remaining: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM music_playlist_memberships WHERE playlist_id = 'playlist-1'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, vec![membership_id]);
    });
}

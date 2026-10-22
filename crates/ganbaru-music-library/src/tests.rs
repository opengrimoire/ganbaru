use super::*;
use sqlx::SqlitePool;

mod interchange;
mod playback;
mod query;
mod relink;
mod source_lifecycle;
mod transfer;

pub(super) async fn pool() -> SqlitePool {
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
    pool
}

pub(super) async fn seed_item(pool: &SqlitePool, id: &str, identity: &str) {
    sqlx::query(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, original_title, availability, review_state,
             discovered_at_ms, updated_at_ms)
         VALUES (?, ?, 'local-file', ?, 'available', 'unreviewed',
             1700000000000, 1700000000000)",
    )
    .bind(id)
    .bind(identity)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

pub(super) fn playlist(id: &str) -> MusicPlaylistCreate {
    MusicPlaylistCreate {
        id: id.to_string(),
        name: "Focus".to_string(),
        icon: "lucide:laptop".to_string(),
        shuffle_enabled: true,
        mix_enabled: false,
        repeat_mode: MusicRepeatMode::All,
        intended_uses: vec![MusicIntendedUse::Focus],
        created_at_ms: 1_700_000_000_000,
    }
}

pub(super) fn library_window() -> MusicItemWindowRequest {
    MusicItemWindowRequest {
        destination: MusicListDestination::Library,
        playlist_id: None,
        search: String::new(),
        source_kind: None,
        availability: None,
        review_state: None,
        source_collection_id: None,
        membership_playlist_id: None,
        snoozed: None,
        sort: MusicItemSort::Title,
        direction: MusicSortDirection::Ascending,
        group_by: MusicGroupBy::None,
        now_ms: 1_700_000_100_000,
        offset: 0,
        limit: 50,
    }
}

pub(super) fn membership(index: usize) -> MusicMembershipWrite {
    MusicMembershipWrite {
        playlist_id: "playlist-1".to_string(),
        item_id: format!("item-{index}"),
        weight: MusicWeight::Normal,
        enabled: true,
        start_ms: None,
        end_ms: None,
        volume_milli: None,
        rate_milli: None,
        expected_version: None,
        updated_at_ms: 1_700_000_000_000,
    }
}

#[test]
fn typed_enums_reject_unknown_external_values() {
    let error = serde_json::from_str::<MusicWeight>(r#""always""#).unwrap_err();
    assert!(error.to_string().contains("unknown variant"));
    assert!(MusicReviewState::try_from("maybe-reviewed").is_err());
}

#[test]
fn review_selection_accepts_folders_larger_than_the_generic_bulk_limit() {
    let item_ids = (0..1_152)
        .map(|index| format!("item-{index}"))
        .collect::<Vec<_>>();

    assert!(validate_review_selection_ids(&item_ids, "itemIds").is_ok());
    assert!(validate_bounded_unique_ids(&item_ids, "itemIds").is_err());
}

#[test]
fn built_in_music_playlists_are_protected_and_localizable() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let summaries = queries::playlist_summaries(&pool, 1_700_000_000_000, 0, 50)
            .await
            .unwrap();
        assert_eq!(
            summaries
                .iter()
                .filter(|playlist| playlist.id.starts_with("playlist-default-"))
                .count(),
            defaults::BUILT_IN_MUSIC_PLAYLISTS.len(),
        );
        assert_eq!(
            summaries
                .iter()
                .find(|playlist| playlist.id == "playlist-default-break-calm")
                .map(|playlist| playlist.icon.as_str()),
            Some("lucide:armchair"),
        );

        let detail = queries::playlist_detail(&pool, "playlist-default-work-focus")
            .await
            .unwrap();
        writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: detail.id.clone(),
                name: "Renamed".to_string(),
                icon: "lucide:rocket".to_string(),
                shuffle_enabled: detail.shuffle_enabled,
                mix_enabled: detail.mix_enabled,
                repeat_mode: detail.repeat_mode,
                intended_uses: detail.intended_uses,
                expected_version: detail.version,
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        let repaired_identity: (String, String) = sqlx::query_as(
            "SELECT name, icon FROM music_playlists WHERE id = 'playlist-default-work-focus'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            repaired_identity,
            ("Work (focus)".into(), "lucide:laptop".into())
        );
        assert!(
            writes::playlist_delete_impact(&pool, "playlist-default-work-focus")
                .await
                .is_err()
        );

        sqlx::query("DELETE FROM music_playlists WHERE id = 'playlist-default-work-focus'")
            .execute(&pool)
            .await
            .unwrap();
        let listed = queries::playlist_summaries(&pool, 1_700_000_000_000, 0, 50)
            .await
            .unwrap();
        assert!(
            listed
                .iter()
                .all(|playlist| playlist.id != "playlist-default-work-focus"),
            "listing playlists never writes seed rows"
        );
    });
}

#[test]
fn built_in_music_playlists_exist_after_migrations() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let seeded: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, name, icon FROM music_playlists
             WHERE id LIKE 'playlist-default-%' ORDER BY order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            seeded,
            defaults::BUILT_IN_MUSIC_PLAYLISTS
                .iter()
                .map(|playlist| (
                    playlist.id.to_string(),
                    playlist.name.to_string(),
                    playlist.icon.to_string()
                ))
                .collect::<Vec<_>>()
        );
    });
}

#[test]
fn local_root_creation_is_atomic_and_rejects_duplicate_identity() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let request = MusicLocalRootCreate {
            root_id: "root-soundtracks".to_string(),
            name: "Soundtracks".to_string(),
            created_at_ms: 1_700_000_000_000,
        };
        let receipt = writes::create_local_root(&pool, request.clone())
            .await
            .unwrap();
        assert_eq!(
            receipt.id,
            crate::identity::collection_id("local-root:root-soundtracks")
        );
        assert_eq!(queries::local_roots(&pool, 0, 10).await.unwrap().len(), 1);
        assert_eq!(
            queries::source_collections(&pool, 0, 10)
                .await
                .unwrap()
                .len(),
            1
        );

        let duplicate = MusicLocalRootCreate {
            name: "Other".to_string(),
            ..request
        };
        assert!(writes::create_local_root(&pool, duplicate).await.is_err());
        assert_eq!(queries::local_roots(&pool, 0, 10).await.unwrap().len(), 1);

        let rename = |name: &str, updated_at_ms: i64| MusicCollectionWrite {
            kind: MusicCollectionKind::LocalRoot,
            name: name.to_string(),
            local_root_id: Some("root-soundtracks".to_string()),
            youtube_playlist_id: None,
            updated_at_ms,
        };
        let renamed = writes::upsert_source_collection(&pool, rename("Scores", 1_700_000_000_100))
            .await
            .unwrap();
        assert_eq!(
            renamed,
            MusicWriteReceipt {
                id: receipt.id.clone(),
                version: 2
            }
        );
        let unchanged =
            writes::upsert_source_collection(&pool, rename("Scores", 1_700_000_000_200))
                .await
                .unwrap();
        assert_eq!(unchanged, renamed);
        let stored: (String, i64, i64) = sqlx::query_as(
            "SELECT name, version, updated_at_ms FROM music_source_collections WHERE id = ?",
        )
        .bind(&receipt.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, ("Scores".to_string(), 2, 1_700_000_000_100));
    });
}

#[test]
fn item_location_repair_requires_weak_match_confirmation_and_can_be_undone() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "repair-item", "local:repair-item").await;
        let folder = std::env::temp_dir().join(format!(
            "ganbaru-music-repair-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("replacement.wav");
        std::fs::write(&file, b"RIFF\x04\x00\x00\x00WAVE").unwrap();

        let preview = item_repair::preview(&pool, "repair-item", &file.to_string_lossy())
            .await
            .unwrap();
        assert_eq!(preview.match_strength, MusicRepairMatchStrength::Weak);
        let mut request = MusicItemRepairApply {
            item_id: "repair-item".to_string(),
            root_id: "repair-root".to_string(),
            root_name: "Recovered".to_string(),
            folder_path: preview.folder_path.clone(),
            relative_path: preview.relative_path.clone(),
            expected_strong_fingerprint: preview.strong_fingerprint.clone(),
            accept_weak_mismatch: false,
            applied_at: 1_700_000_000_000,
        };
        assert!(item_repair::apply(&pool, request.clone()).await.is_err());
        request.accept_weak_mismatch = true;
        let receipt = item_repair::apply(&pool, request).await.unwrap();
        assert_eq!(
            receipt.id,
            crate::identity::location_id("repair-root", "replacement.wav")
        );
        let available: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_local_locations
             WHERE item_id = 'repair-item' AND availability = 'available'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(available, 1);

        item_repair::undo(&pool, &receipt.id, "repair-root")
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM music_local_locations WHERE id = ?",
            )
            .bind(&receipt.id)
            .fetch_one(&pool)
            .await
            .unwrap(),
            0
        );
        std::fs::remove_dir_all(folder).unwrap();
    });
}

#[test]
fn memberships_reject_invalid_ranges_and_duplicate_bulk_identity() {
    let mut invalid = membership(0);
    invalid.start_ms = Some(5_000);
    invalid.end_ms = Some(4_000);
    assert_eq!(
        validate_membership_write(&invalid)
            .unwrap_err()
            .field
            .as_deref(),
        Some("endMs"),
    );

    let first = membership(1);
    let mut duplicate = membership(2);
    duplicate.item_id = first.item_id.clone();
    let error = validate_bulk_membership_write(&MusicBulkMembershipWrite {
        memberships: vec![first, duplicate],
    })
    .unwrap_err();
    assert!(error.message.contains("duplicate item"));
}

#[test]
fn bulk_memberships_have_an_explicit_request_bound() {
    let request = MusicBulkMembershipWrite {
        memberships: (0..=MAX_BULK_MEMBERSHIPS).map(membership).collect(),
    };
    let error = validate_bulk_membership_write(&request).unwrap_err();
    assert!(error.message.contains("500 item limit"));
}

#[test]
fn row_mapping_rejects_unknown_persisted_enums_with_field_context() {
    let row = MusicMembershipRow {
        id: "membership-1".to_string(),
        playlist_id: "playlist-1".to_string(),
        item_id: "item-1".to_string(),
        order_key: "a0".to_string(),
        weight: "sometimes-ish".to_string(),
        enabled: 1,
        start_ms: None,
        end_ms: None,
        volume_milli: None,
        rate_milli: None,
        created_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_000_000,
        version: 1,
    };

    let error = MusicPlaylistMembership::try_from(row).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("weight"));
    assert!(error.message.contains("sometimes-ish"));
}

#[test]
fn playlist_create_update_and_stale_detection_are_transactional() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let created = super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        assert_eq!(created.version, 1);

        let updated = super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Deep focus".to_string(),
                icon: "emoji:🎧".to_string(),
                shuffle_enabled: false,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::Off,
                intended_uses: vec![MusicIntendedUse::Focus, MusicIntendedUse::Reading],
                expected_version: 1,
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.version, 2);

        let unchanged = super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Deep focus".to_string(),
                icon: "emoji:🎧".to_string(),
                shuffle_enabled: false,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::Off,
                intended_uses: vec![MusicIntendedUse::Reading, MusicIntendedUse::Focus],
                expected_version: 2,
                updated_at_ms: 1_700_000_000_150,
            },
        )
        .await
        .unwrap();
        assert_eq!(unchanged.version, 2);
        let stored: (i64, i64) = sqlx::query_as(
            "SELECT version, updated_at_ms FROM music_playlists WHERE id = 'playlist-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, (2, 1_700_000_000_100));

        let uses_only = super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Deep focus".to_string(),
                icon: "emoji:🎧".to_string(),
                shuffle_enabled: false,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::Off,
                intended_uses: vec![MusicIntendedUse::Focus],
                expected_version: 2,
                updated_at_ms: 1_700_000_000_160,
            },
        )
        .await
        .unwrap();
        assert_eq!(uses_only.version, 3);
        let restored = super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Deep focus".to_string(),
                icon: "emoji:🎧".to_string(),
                shuffle_enabled: false,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::Off,
                intended_uses: vec![MusicIntendedUse::Focus, MusicIntendedUse::Reading],
                expected_version: 3,
                updated_at_ms: 1_700_000_000_170,
            },
        )
        .await
        .unwrap();
        assert_eq!(restored.version, 4);

        let stale = super::writes::update_playlist(
            &pool,
            MusicPlaylistUpdate {
                id: "playlist-1".to_string(),
                name: "Stale edit".to_string(),
                icon: "lucide:list-music".to_string(),
                shuffle_enabled: false,
                mix_enabled: false,
                repeat_mode: MusicRepeatMode::All,
                intended_uses: Vec::new(),
                expected_version: 1,
                updated_at_ms: 1_700_000_000_200,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(stale.code, MusicLibraryErrorCode::StaleWrite);

        let uses: Vec<String> = sqlx::query_scalar(
            "SELECT intended_use FROM music_playlist_intended_uses
             WHERE playlist_id = 'playlist-1' ORDER BY intended_use",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(uses, vec!["focus", "reading"]);
        assert_eq!(
            queries::playlist_detail(&pool, "playlist-1")
                .await
                .unwrap()
                .icon,
            "emoji:🎧",
        );
    });
}

#[test]
fn mix_mode_round_trips_and_requires_shuffle() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut request = playlist("playlist-mix");
        request.mix_enabled = true;
        writes::create_playlist(&pool, request).await.unwrap();
        let detail = queries::playlist_detail(&pool, "playlist-mix")
            .await
            .unwrap();
        assert!(detail.shuffle_enabled && detail.mix_enabled);
        let summary = queries::playlist_summaries(&pool, 1_700_000_000_000, 0, 50)
            .await
            .unwrap()
            .into_iter()
            .find(|entry| entry.id == "playlist-mix")
            .unwrap();
        assert!(summary.mix_enabled);

        let mut invalid = playlist("playlist-invalid-mix");
        invalid.shuffle_enabled = false;
        invalid.mix_enabled = true;
        assert!(writes::create_playlist(&pool, invalid).await.is_err());
    });
}

#[test]
fn bulk_membership_failure_rolls_back_earlier_rows() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let first = membership(1);
        let mut missing = membership(2);
        missing.item_id = "missing-item".to_string();

        let error = super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![first, missing],
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, MusicLibraryErrorCode::Database);

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_playlist_memberships")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });
}

#[test]
fn duplicate_playlist_preserves_membership_details_and_ranges() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        let mut source = playlist("playlist-1");
        source.mix_enabled = true;
        super::writes::create_playlist(&pool, source).await.unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let mut source_membership = membership(1);
        source_membership.item_id = "item-1".to_string();
        source_membership.weight = MusicWeight::MoreOften;
        source_membership.start_ms = Some(1_000);
        source_membership.end_ms = Some(90_000);
        let receipt = super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![source_membership],
            },
        )
        .await
        .unwrap()
        .remove(0);
        sqlx::query(
            "INSERT INTO music_membership_skip_ranges
                (membership_id, ordinal, start_ms, end_ms)
             VALUES (?, 0, 10000, 12000)",
        )
        .bind(&receipt.id)
        .execute(&pool)
        .await
        .unwrap();

        super::writes::duplicate_playlist(
            &pool,
            MusicPlaylistDuplicate {
                source_playlist_id: "playlist-1".to_string(),
                new_playlist_id: "playlist-2".to_string(),
                name: "Focus copy".to_string(),
                created_at_ms: 1_700_000_001_000,
            },
        )
        .await
        .unwrap();
        assert!(
            queries::playlist_detail(&pool, "playlist-2")
                .await
                .unwrap()
                .mix_enabled
        );

        let copied: (String, Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT weight, start_ms, end_ms FROM music_playlist_memberships
             WHERE playlist_id = 'playlist-2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            copied,
            ("more-often".to_string(), Some(1_000), Some(90_000))
        );
        let skip_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_membership_skip_ranges AS skip
             JOIN music_playlist_memberships AS membership ON membership.id = skip.membership_id
             WHERE membership.playlist_id = 'playlist-2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(skip_count, 1);
    });
}

#[test]
fn deletion_requires_current_impact_and_repairs_assignments_atomically() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        super::writes::create_playlist(&pool, playlist("playlist-2"))
            .await
            .unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let mut linked = membership(1);
        linked.item_id = "item-1".to_string();
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![linked],
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, playlist_id)
             VALUES ('event-1', 'Focus', '2026-07-15T09:00:00Z',
                 '2026-07-15T10:00:00Z', 'playlist-1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        ganbaru_music::assignments::replace_assignments_in_transaction(
            &mut transaction,
            MusicAssignmentOwnerKind::EventOverride,
            "event-1",
            vec![MusicContextAssignmentDraft {
                phase: MusicActivityPhase::Focus,
                behavior: MusicAssignmentBehavior::PlayAutomatically,
                playlist_id: Some("playlist-1".to_string()),
                soundscape_preset_id: None,
                soundscape_behavior: MusicSoundscapeBehavior::Inherit,
                provenance_kind: MusicAssignmentProvenanceKind::Explicit,
                provenance_id: None,
            }],
            1_700_000_000_000,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group-1', 'Group')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, focus_playlist_id)
             VALUES ('project-1', 'group-1', 'Project', 'playlist-1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO calendar_events (id, title, start_time, end_time, playlist_id)
             VALUES ('event-2', 'Plain', '2026-07-16T09:00:00Z',
                 '2026-07-16T10:00:00Z', 'playlist-1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let impact = super::writes::playlist_delete_impact(&pool, "playlist-1")
            .await
            .unwrap();
        assert_eq!(impact.membership_count, 1);
        assert_eq!(impact.project_focus_assignment_count, 1);
        assert_eq!(impact.calendar_assignment_count, 1);
        assert_eq!(impact.context_assignment_count, 1);
        let mut stale_impact = impact.clone();
        stale_impact.context_assignment_count = 0;
        let conflict = super::writes::delete_playlist(
            &pool,
            MusicPlaylistDelete {
                playlist_id: "playlist-1".to_string(),
                replacement_playlist_id: Some("playlist-2".to_string()),
                expected_version: 1,
                expected_impact: stale_impact,
            },
        )
        .await
        .unwrap_err();
        assert_eq!(conflict.code, MusicLibraryErrorCode::Conflict);

        super::writes::delete_playlist(
            &pool,
            MusicPlaylistDelete {
                playlist_id: "playlist-1".to_string(),
                replacement_playlist_id: Some("playlist-2".to_string()),
                expected_version: 1,
                expected_impact: impact,
            },
        )
        .await
        .unwrap();
        let event_playlists: Vec<Option<String>> =
            sqlx::query_scalar("SELECT playlist_id FROM calendar_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            event_playlists,
            vec![
                Some("playlist-1".to_string()),
                Some("playlist-1".to_string())
            ]
        );
        let project_playlist: Option<String> =
            sqlx::query_scalar("SELECT focus_playlist_id FROM projects WHERE id = 'project-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(project_playlist.as_deref(), Some("playlist-1"));
        let context_assignment: Option<String> = sqlx::query_scalar(
            "SELECT playlist_id FROM music_context_assignments
             WHERE owner_kind = 'event-override' AND owner_id = 'event-1' AND phase = 'focus'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(context_assignment.as_deref(), Some("playlist-2"));
        let playlist_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_playlists")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            playlist_count,
            defaults::BUILT_IN_MUSIC_PLAYLISTS.len() as i64 + 1,
        );
    });
}

#[test]
fn deferred_review_items_remain_visible_before_and_after_their_optional_date() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        super::writes::set_review_state(
            &pool,
            MusicReviewWrite {
                item_id: "item-1".to_string(),
                review_state: MusicReviewState::Deferred,
                deferred_until: Some(1_700_000_200_000),
                expected_version: 1,
                updated_at_ms: 1_700_000_100_000,
            },
        )
        .await
        .unwrap();

        let mut request = library_window();
        request.destination = MusicListDestination::Review;
        request.now_ms = 1_700_000_150_000;
        assert_eq!(
            super::queries::item_window(&pool, request.clone())
                .await
                .unwrap()
                .total_count,
            1
        );
        request.now_ms = 1_700_000_250_000;
        assert_eq!(
            super::queries::item_window(&pool, request)
                .await
                .unwrap()
                .total_count,
            1
        );
    });
}

#[test]
fn playlist_item_window_counts_only_snoozes_effective_in_that_playlist() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for id in ["playlist-1", "playlist-2"] {
            super::writes::create_playlist(&pool, playlist(id))
                .await
                .unwrap();
        }
        seed_item(&pool, "item-1", "local:item-1").await;
        let first = membership(1);
        let mut second = membership(2);
        second.item_id = "item-1".to_string();
        second.playlist_id = "playlist-2".to_string();
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![first, second],
            },
        )
        .await
        .unwrap();
        super::playlist_edits::bulk_snooze(
            &pool,
            MusicBulkSnoozeWrite {
                action_id: "playlist-snooze".to_string(),
                item_ids: vec!["item-1".to_string()],
                scope: MusicSnoozeScope::Playlist,
                playlist_id: Some("playlist-2".to_string()),
                starts_at_ms: 1_700_000_000_000,
                ends_at_ms: Some(1_700_000_200_000),
                reason: String::new(),
                created_at_ms: 1_700_000_000_000,
            },
        )
        .await
        .unwrap();

        let mut request = library_window();
        request.destination = MusicListDestination::Playlist;
        request.playlist_id = Some("playlist-1".to_string());
        assert_eq!(
            super::queries::item_window(&pool, request.clone())
                .await
                .unwrap()
                .items[0]
                .active_snooze_count,
            0
        );
        request.playlist_id = Some("playlist-2".to_string());
        assert_eq!(
            super::queries::item_window(&pool, request)
                .await
                .unwrap()
                .items[0]
                .active_snooze_count,
            1
        );
    });
}

#[test]
fn metadata_overrides_preserve_original_values_and_refresh_search() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        let receipt = super::writes::set_metadata_overrides(
            &pool,
            MusicMetadataOverrideWrite {
                item_id: "item-1".to_string(),
                title_override: Some("Quiet focus".to_string()),
                artist_override: Some("Composer".to_string()),
                album_override: None,
                artwork_override: None,
                expected_version: 1,
                updated_at_ms: 1_700_000_100_000,
            },
        )
        .await
        .unwrap();
        assert_eq!(receipt.version, 2);

        let detail = super::queries::inspector_detail(&pool, "item-1")
            .await
            .unwrap();
        assert_eq!(detail.item.original_title, "item-1");
        assert_eq!(detail.item.title_override.as_deref(), Some("Quiet focus"));

        let mut request = library_window();
        request.search = "Quiet".to_string();
        let window = super::queries::item_window(&pool, request).await.unwrap();
        assert_eq!(window.total_count, 1);
        assert_eq!(window.items[0].title, "Quiet focus");
    });
}

#[test]
fn item_signals_replace_in_bulk_and_refresh_search() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        seed_item(&pool, "item-2", "local:item-2").await;

        let receipts = super::writes::set_item_signals(
            &pool,
            MusicItemSignalsWrite {
                item_ids: vec!["item-1".to_string(), "item-2".to_string()],
                signals: vec![MusicItemSignal::Lyrics, MusicItemSignal::SuddenChanges],
                updated_at_ms: 1_700_000_100_000,
            },
        )
        .await
        .unwrap();
        assert_eq!(receipts.len(), 2);
        assert!(receipts.iter().all(|receipt| receipt.version == 2));

        let detail = super::queries::inspector_detail(&pool, "item-1")
            .await
            .unwrap();
        assert_eq!(
            detail.signals,
            vec![MusicItemSignal::Lyrics, MusicItemSignal::SuddenChanges]
        );

        let mut request = library_window();
        request.search = "sudden-changes".to_string();
        assert_eq!(
            super::queries::item_window(&pool, request)
                .await
                .unwrap()
                .total_count,
            2
        );

        super::writes::set_item_signals(
            &pool,
            MusicItemSignalsWrite {
                item_ids: vec!["item-1".to_string()],
                signals: vec![MusicItemSignal::Calm],
                updated_at_ms: 1_700_000_200_000,
            },
        )
        .await
        .unwrap();
        let detail = super::queries::inspector_detail(&pool, "item-1")
            .await
            .unwrap();
        assert_eq!(detail.signals, vec![MusicItemSignal::Calm]);

        let unchanged = super::writes::set_item_signals(
            &pool,
            MusicItemSignalsWrite {
                item_ids: vec!["item-1".to_string()],
                signals: vec![MusicItemSignal::Calm],
                updated_at_ms: 1_700_000_300_000,
            },
        )
        .await
        .unwrap();
        assert_eq!(unchanged[0].version, detail.item.version);
        let stored: (i64, i64) = sqlx::query_as(
            "SELECT version, updated_at_ms FROM music_library_items WHERE id = 'item-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, (detail.item.version, detail.item.updated_at_ms));
    });
}

#[test]
fn advanced_membership_settings_replace_validated_skip_ranges_atomically() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let initial = membership(1);
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![initial.clone()],
            },
        )
        .await
        .unwrap();
        let mut updated = initial;
        updated.start_ms = Some(1_000);
        updated.end_ms = Some(90_000);
        updated.volume_milli = Some(600);
        updated.rate_milli = Some(1_250);
        updated.expected_version = Some(1);
        let receipt = super::writes::save_advanced_membership(
            &pool,
            MusicAdvancedMembershipWrite {
                membership: updated,
                skip_ranges: vec![
                    MusicMembershipSkipRangeWrite {
                        start_ms: 5_000,
                        end_ms: 10_000,
                    },
                    MusicMembershipSkipRangeWrite {
                        start_ms: 20_000,
                        end_ms: 25_000,
                    },
                ],
            },
        )
        .await
        .unwrap();
        assert_eq!(receipt.membership.version, 2);
        let membership_id = crate::identity::membership_id("playlist-1", "item-1");
        assert_eq!(receipt.membership.id, membership_id);
        assert_eq!(receipt.skip_ranges[1].membership_id, membership_id);
        assert_eq!(receipt.skip_ranges[1].ordinal, 1);
        let overrides: (Option<i64>, Option<i64>) = sqlx::query_as(
            "SELECT volume_milli, rate_milli FROM music_playlist_memberships WHERE id = ?",
        )
        .bind(&membership_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(overrides, (Some(600), Some(1_250)));
        let detail = super::queries::inspector_detail(&pool, "item-1")
            .await
            .unwrap();
        assert_eq!(detail.memberships[0].start_ms, Some(1_000));
        assert_eq!(detail.membership_skip_ranges.len(), 2);
        assert_eq!(detail.membership_skip_ranges[1].start_ms, 20_000);
    });
}

#[test]
fn advanced_membership_save_diffs_skip_ranges_and_skips_unchanged_values() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        seed_item(&pool, "item-1", "local:item-1").await;
        let mut request = MusicAdvancedMembershipWrite {
            membership: membership(1),
            skip_ranges: vec![
                MusicMembershipSkipRangeWrite {
                    start_ms: 1_000,
                    end_ms: 2_000,
                },
                MusicMembershipSkipRangeWrite {
                    start_ms: 3_000,
                    end_ms: 4_000,
                },
            ],
        };
        let first = super::writes::save_advanced_membership(&pool, request.clone())
            .await
            .unwrap();
        assert_eq!(first.membership.version, 1);

        request.membership.expected_version = Some(1);
        let unchanged = super::writes::save_advanced_membership(&pool, request.clone())
            .await
            .unwrap();
        assert_eq!(unchanged, first);

        request.skip_ranges = vec![MusicMembershipSkipRangeWrite {
            start_ms: 1_000,
            end_ms: 2_500,
        }];
        let trimmed = super::writes::save_advanced_membership(&pool, request)
            .await
            .unwrap();
        assert_eq!(trimmed.membership.version, 1);
        assert_eq!(trimmed.skip_ranges.len(), 1);
        assert_eq!(trimmed.skip_ranges[0].ordinal, 0);
        let stored: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT ordinal, end_ms FROM music_membership_skip_ranges WHERE membership_id = ?",
        )
        .bind(&first.membership.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(stored, vec![(0, 2_500)]);
    });
}

#[test]
fn bulk_membership_edits_preserve_existing_settings_and_commit_as_one_change() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 1..=3 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        let mut existing = membership(1);
        existing.weight = MusicWeight::Rarely;
        existing.start_ms = Some(5_000);
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![existing],
            },
        )
        .await
        .unwrap();

        let added = super::playlist_edits::bulk_edit_memberships(
            &pool,
            MusicBulkMembershipEdit {
                action_id: "bulk-add".to_string(),
                item_ids: vec!["item-1".to_string(), "item-2".to_string()],
                add_playlist_ids: vec!["playlist-1".to_string()],
                remove_playlist_ids: vec![],
                weight_playlist_ids: vec![],
                weight: None,
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(added.changed_count, 1);
        let preserved: (String, Option<i64>) = sqlx::query_as(
            "SELECT weight, start_ms FROM music_playlist_memberships WHERE playlist_id = 'playlist-1' AND item_id = 'item-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preserved, ("rarely".to_string(), Some(5_000)));

        let weighted = super::playlist_edits::bulk_edit_memberships(
            &pool,
            MusicBulkMembershipEdit {
                action_id: "bulk-weight".to_string(),
                item_ids: vec!["item-1".to_string(), "item-2".to_string()],
                add_playlist_ids: vec![],
                remove_playlist_ids: vec![],
                weight_playlist_ids: vec!["playlist-1".to_string()],
                weight: Some(MusicWeight::MoreOften),
                updated_at_ms: 1_700_000_000_200,
            },
        )
        .await
        .unwrap();
        assert_eq!(weighted.changed_count, 2);
        let added_id: String = sqlx::query_scalar(
            "SELECT id FROM music_playlist_memberships WHERE playlist_id = 'playlist-1' AND item_id = 'item-2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            added_id,
            crate::identity::membership_id("playlist-1", "item-2")
        );

        let repeated = super::playlist_edits::bulk_edit_memberships(
            &pool,
            MusicBulkMembershipEdit {
                action_id: "bulk-weight-again".to_string(),
                item_ids: vec!["item-1".to_string(), "item-2".to_string()],
                add_playlist_ids: vec![],
                remove_playlist_ids: vec![],
                weight_playlist_ids: vec!["playlist-1".to_string()],
                weight: Some(MusicWeight::MoreOften),
                updated_at_ms: 1_700_000_000_250,
            },
        )
        .await
        .unwrap();
        assert_eq!(repeated.changed_count, 0);

        let removed = super::playlist_edits::bulk_edit_memberships(
            &pool,
            MusicBulkMembershipEdit {
                action_id: "bulk-remove".to_string(),
                item_ids: vec!["item-1".to_string(), "item-3".to_string()],
                add_playlist_ids: vec![],
                remove_playlist_ids: vec!["playlist-1".to_string()],
                weight_playlist_ids: vec![],
                weight: None,
                updated_at_ms: 1_700_000_000_300,
            },
        )
        .await
        .unwrap();
        assert_eq!(removed.changed_count, 1);
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_playlist_memberships WHERE playlist_id = 'playlist-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 1);
    });
}

#[test]
fn playlist_collection_reorder_rekeys_only_moved_playlists_atomically() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        super::writes::create_playlist(&pool, playlist("playlist-2"))
            .await
            .unwrap();
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT id, version FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows.iter()
                .take(super::defaults::BUILT_IN_MUSIC_PLAYLISTS.len())
                .map(|(playlist_id, _)| playlist_id.as_str())
                .collect::<Vec<_>>(),
            super::defaults::BUILT_IN_MUSIC_PLAYLISTS
                .iter()
                .map(|playlist| playlist.id)
                .collect::<Vec<_>>()
        );
        let original = rows.clone();
        let mut reordered = rows;
        let last = reordered.pop().unwrap();
        reordered.insert(0, last);

        let receipts = super::playlist_edits::reorder_playlists(
            &pool,
            MusicPlaylistsReorder {
                playlists: reordered
                    .iter()
                    .map(|(playlist_id, expected_version)| MusicPlaylistOrderEntry {
                        playlist_id: playlist_id.clone(),
                        expected_version: *expected_version,
                    })
                    .collect(),
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(receipts.len(), reordered.len());
        let versions: Vec<(String, i64)> =
            sqlx::query_as("SELECT id, version FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            versions,
            reordered
                .iter()
                .enumerate()
                .map(|(index, (playlist_id, version))| (
                    playlist_id.clone(),
                    if index == 0 { version + 1 } else { *version }
                ))
                .collect::<Vec<_>>(),
            "only the moved playlist takes a new key and version"
        );
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT id FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let reordered_ids = reordered
            .iter()
            .map(|(playlist_id, _)| playlist_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(stored, reordered_ids);
        let stored_keys: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT id, order_key, version FROM music_playlists ORDER BY order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            receipts
                .into_iter()
                .map(|receipt| (receipt.id, receipt.order_key, receipt.version))
                .collect::<Vec<_>>(),
            stored_keys,
            "receipts carry every playlist's stored key and version in the requested order"
        );

        let stale_result = super::playlist_edits::reorder_playlists(
            &pool,
            MusicPlaylistsReorder {
                playlists: original
                    .into_iter()
                    .map(|(playlist_id, expected_version)| MusicPlaylistOrderEntry {
                        playlist_id,
                        expected_version,
                    })
                    .collect(),
                updated_at_ms: 1_700_000_000_200,
            },
        )
        .await;
        assert!(stale_result.is_err());
        let stored_after_conflict: Vec<String> =
            sqlx::query_scalar("SELECT id FROM music_playlists ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(stored_after_conflict, reordered_ids);
    });
}

#[test]
fn playlist_reorder_and_playback_projection_share_canonical_memberships() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        for index in 1..=3 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
            super::writes::upsert_memberships(
                &pool,
                MusicBulkMembershipWrite {
                    memberships: vec![membership(index)],
                },
            )
            .await
            .unwrap();
        }
        let reordered = super::playlist_edits::reorder_playlist(
            &pool,
            MusicPlaylistReorder {
                playlist_id: "playlist-1".to_string(),
                item_id: "item-3".to_string(),
                target_index: 0,
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(reordered.item_ids, vec!["item-3", "item-1", "item-2"]);
        let versions: Vec<(String, i64)> = sqlx::query_as(
            "SELECT item_id, version FROM music_playlist_memberships
             WHERE playlist_id = 'playlist-1' ORDER BY order_key, id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            versions,
            vec![
                ("item-3".to_string(), 2),
                ("item-1".to_string(), 1),
                ("item-2".to_string(), 1)
            ],
            "only the moved membership takes a new key and version"
        );

        sqlx::query(
            "INSERT INTO music_membership_skip_ranges
                (membership_id, ordinal, start_ms, end_ms)
             VALUES (?, 0, 1000, 2000)",
        )
        .bind(crate::identity::membership_id("playlist-1", "item-1"))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_snoozes
                (id, item_id, scope, playlist_id, starts_at_ms, ends_at_ms, reason, created_at_ms)
             VALUES ('snooze-2', 'item-2', 'playlist', 'playlist-1',
                1700000000000, 1700000001000, '', 1700000000000)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items
             SET source_kind = 'youtube-video', youtube_video_id = 'abcdefghijk',
                 youtube_resolution_state = 'embedding-blocked'
             WHERE id = 'item-3'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items
             SET original_artwork_identity = 'sidecar:album/cover.jpg'
             WHERE id = 'item-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_library_items
             SET artwork_override = '/custom/artwork.png'
             WHERE id = 'item-2'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let entries =
            super::queries::playlist_playback_entries(&pool, "playlist-1", 1_700_000_000_200)
                .await
                .unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.item_id.as_str())
                .collect::<Vec<_>>(),
            vec!["item-3", "item-1", "item-2"]
        );
        assert!(entries.iter().all(|entry| entry.enabled));
        assert_eq!(entries[0].youtube_video_id.as_deref(), Some("abcdefghijk"));
        assert_eq!(
            entries[0].youtube_resolution_state,
            Some(MusicYouTubeResolutionState::EmbeddingBlocked)
        );
        assert_eq!(entries[1].skip_ranges.len(), 1);
        assert_eq!(entries[1].skip_ranges[0].end_ms, 2_000);
        assert_eq!(
            entries[1].original_artwork_identity.as_deref(),
            Some("sidecar:album/cover.jpg")
        );
        assert_eq!(
            entries[2].artwork_override.as_deref(),
            Some("/custom/artwork.png")
        );
        assert!(entries[2].snoozed);
        assert_eq!(entries[2].snoozed_until, Some(1_700_000_001_000));
    });
}

#[test]
fn bulk_review_and_snooze_updates_are_atomic() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 1..=2 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }
        let result = super::playlist_edits::bulk_set_review_state(
            &pool,
            MusicBulkReviewWrite {
                items: vec![
                    MusicVersionedItem {
                        item_id: "item-1".to_string(),
                        expected_version: 1,
                    },
                    MusicVersionedItem {
                        item_id: "item-2".to_string(),
                        expected_version: 1,
                    },
                ],
                review_state: MusicReviewState::Reviewed,
                deferred_until: None,
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(result.changed_count, 2);
        let snoozed = super::playlist_edits::bulk_snooze(
            &pool,
            MusicBulkSnoozeWrite {
                action_id: "snooze-action".to_string(),
                item_ids: vec!["item-1".to_string(), "item-2".to_string()],
                scope: MusicSnoozeScope::AllPlaylists,
                playlist_id: None,
                starts_at_ms: 1_700_000_000_100,
                ends_at_ms: Some(1_700_086_400_100),
                reason: String::new(),
                created_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();
        assert_eq!(snoozed.changed_count, 2);
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT COUNT(*) FROM music_library_items WHERE review_state = 'reviewed'),
                (SELECT COUNT(*) FROM music_snoozes)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (2, 2));
    });
}

#[test]
fn review_selection_applies_memberships_and_review_state_in_one_transaction() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 1..=2 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();

        let result = super::playlist_edits::apply_review_selection(
            &pool,
            MusicReviewSelectionWrite {
                action_id: "review-selection".to_string(),
                items: vec![
                    MusicVersionedItem {
                        item_id: "item-1".to_string(),
                        expected_version: 1,
                    },
                    MusicVersionedItem {
                        item_id: "item-2".to_string(),
                        expected_version: 1,
                    },
                ],
                review_state: MusicReviewState::Reviewed,
                add_playlist_ids: vec!["playlist-1".to_string()],
                remove_playlist_ids: vec![],
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.membership_changed_count, 2);
        assert_eq!(result.review_changed_count, 2);
        assert!(result.items.iter().all(|receipt| receipt.version == 2));
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT COUNT(*) FROM music_library_items WHERE review_state = 'reviewed'),
                (SELECT COUNT(*) FROM music_playlist_memberships WHERE playlist_id = 'playlist-1')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (2, 2));
    });
}

#[test]
fn stale_review_selection_does_not_apply_partial_memberships() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 1..=2 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();

        let error = super::playlist_edits::apply_review_selection(
            &pool,
            MusicReviewSelectionWrite {
                action_id: "stale-review-selection".to_string(),
                items: vec![
                    MusicVersionedItem {
                        item_id: "item-1".to_string(),
                        expected_version: 1,
                    },
                    MusicVersionedItem {
                        item_id: "item-2".to_string(),
                        expected_version: 2,
                    },
                ],
                review_state: MusicReviewState::Reviewed,
                add_playlist_ids: vec!["playlist-1".to_string()],
                remove_playlist_ids: vec![],
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, MusicLibraryErrorCode::StaleWrite);
        let counts: (i64, i64) = sqlx::query_as(
            "SELECT
                (SELECT COUNT(*) FROM music_library_items WHERE review_state = 'reviewed'),
                (SELECT COUNT(*) FROM music_playlist_memberships WHERE playlist_id = 'playlist-1')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counts, (0, 0));
    });
}

#[test]
fn review_selection_ignores_unassigned_items_in_one_transaction() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        for index in 1..=2 {
            seed_item(
                &pool,
                &format!("item-{index}"),
                &format!("local:item-{index}"),
            )
            .await;
        }

        let result = super::playlist_edits::apply_review_selection(
            &pool,
            MusicReviewSelectionWrite {
                action_id: "ignore-review-selection".to_string(),
                items: vec![
                    MusicVersionedItem {
                        item_id: "item-1".to_string(),
                        expected_version: 1,
                    },
                    MusicVersionedItem {
                        item_id: "item-2".to_string(),
                        expected_version: 1,
                    },
                ],
                review_state: MusicReviewState::Ignored,
                add_playlist_ids: vec![],
                remove_playlist_ids: vec![],
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.membership_changed_count, 0);
        assert_eq!(result.review_changed_count, 2);
        assert!(result.items.iter().all(|receipt| receipt.version == 2));
        let ignored_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_library_items WHERE review_state = 'ignored'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ignored_count, 2);
    });
}

#[test]
fn review_selection_rejects_ignoring_items_assigned_to_playlists() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![membership(1)],
            },
        )
        .await
        .unwrap();

        let error = super::playlist_edits::apply_review_selection(
            &pool,
            MusicReviewSelectionWrite {
                action_id: "blocked-ignore-selection".to_string(),
                items: vec![MusicVersionedItem {
                    item_id: "item-1".to_string(),
                    expected_version: 1,
                }],
                review_state: MusicReviewState::Ignored,
                add_playlist_ids: vec![],
                remove_playlist_ids: vec![],
                updated_at_ms: 1_700_000_000_100,
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, MusicLibraryErrorCode::Validation);
        let state: String =
            sqlx::query_scalar("SELECT review_state FROM music_library_items WHERE id = 'item-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(state, "unreviewed");
    });
}

#[test]
fn overlapping_snoozes_expire_and_resume_independently() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_item(&pool, "item-1", "local:item-1").await;
        super::writes::create_playlist(&pool, playlist("playlist-1"))
            .await
            .unwrap();
        super::writes::upsert_memberships(
            &pool,
            MusicBulkMembershipWrite {
                memberships: vec![membership(1)],
            },
        )
        .await
        .unwrap();
        for (id, scope, playlist_id, ends_at_ms) in [
            (
                "snooze-global",
                MusicSnoozeScope::AllPlaylists,
                None,
                Some(1_700_000_000_300),
            ),
            (
                "snooze-playlist",
                MusicSnoozeScope::Playlist,
                Some("playlist-1".to_string()),
                None,
            ),
        ] {
            super::playlist_edits::bulk_snooze(
                &pool,
                MusicBulkSnoozeWrite {
                    action_id: id.to_string(),
                    item_ids: vec!["item-1".to_string()],
                    scope,
                    playlist_id,
                    starts_at_ms: 1_700_000_000_000,
                    ends_at_ms,
                    reason: String::new(),
                    created_at_ms: 1_700_000_000_000,
                },
            )
            .await
            .unwrap();
        }
        let active =
            super::queries::playlist_playback_entries(&pool, "playlist-1", 1_700_000_000_200)
                .await
                .unwrap();
        assert!(active[0].snoozed);
        assert!(active[0].snoozed_indefinitely);

        let playlist_snooze_id: String = sqlx::query_scalar(
            "SELECT id FROM music_snoozes WHERE item_id = 'item-1' AND playlist_id = 'playlist-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        super::writes::remove_snooze(
            &pool,
            MusicSnoozeRemove {
                snooze_id: playlist_snooze_id,
            },
        )
        .await
        .unwrap();
        let overlapping =
            super::queries::playlist_playback_entries(&pool, "playlist-1", 1_700_000_000_200)
                .await
                .unwrap();
        assert!(overlapping[0].snoozed);
        assert!(!overlapping[0].snoozed_indefinitely);

        let expired =
            super::queries::playlist_playback_entries(&pool, "playlist-1", 1_700_000_000_301)
                .await
                .unwrap();
        assert!(!expired[0].snoozed);
    });
}

use super::super::transfer::{self, MusicTransferCommit, MusicTransferSource, TransferBinding};
use super::super::transfer_codec as codec;
use super::*;

fn source() -> MusicTransferSource {
    MusicTransferSource {
        contents: codec::serialize_json(
            super::interchange::request(MusicImportPlaylistConflict::ImportCopy).document,
        )
        .unwrap(),
        playlist_name: "Imported playlist".to_string(),
        relative_root_id: None,
        selected_at_ms: 1_700_000_000_000,
    }
}

async fn reviewed(
    pool: &sqlx::SqlitePool,
    source: MusicTransferSource,
    action: &str,
) -> MusicTransferCommit {
    let preview = transfer::preview(pool, &source, &[]).await.unwrap();
    MusicTransferCommit {
        action_id: action.to_string(),
        source,
        expected_revision: preview.revision,
        playlist_conflict: MusicImportPlaylistConflict::ImportCopy,
        replace_item_descriptions: false,
        import_context_assignments: false,
    }
}

#[test]
fn music_transfer_m3u_preserves_titles_unicode_and_rejects_external_urls() {
    let entries = codec::parse_m3u("\u{feff}#EXTM3U\n#EXTINF:-1,Quiet rain\nÁlbum/Rain.flac\nhttps://youtu.be/abcdefghi01\nhttps://example.org/live\n").unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].title.as_deref(), Some("Quiet rain"));
    assert_eq!(entries[0].value, "Álbum/Rain.flac");
    assert_eq!(entries[1].video_id.as_deref(), Some("abcdefghi01"));
    assert!(entries[2].unsupported);
    assert_eq!(
        codec::local_identity("root", "Album\\Rain.flac"),
        codec::local_identity("root", "Album/Rain.flac")
    );
    assert_ne!(
        codec::local_identity("root", "Album/Rain.flac"),
        codec::local_identity("root", "album/rain.flac")
    );
}

#[test]
fn music_transfer_json_requires_mix_and_retains_unknown_record_diagnostics() {
    let mut document: serde_json::Value = serde_json::from_str(&source().contents).unwrap();
    document["playlists"][0]
        .as_object_mut()
        .unwrap()
        .remove("mixEnabled");
    assert!(codec::parse_json(&document.to_string()).is_err());
    document["playlists"][0]["mixEnabled"] = serde_json::json!(false);
    document["playlists"][0]["memberships"][0]["item"]["sourceKind"] =
        serde_json::json!("future-source");
    let parsed = codec::parse_json(&document.to_string()).unwrap();
    assert!(parsed.playlists[0].memberships.is_empty());
    assert_eq!(parsed.warnings.len(), 1);
    assert!(parsed.warnings[0].starts_with("Unsupported record skipped:"));
    for path in [
        "../secret.mp3",
        "folder\\..\\secret.mp3",
        "folder//track.mp3",
        "track:stream.mp3",
    ] {
        let mut document: serde_json::Value = serde_json::from_str(&source().contents).unwrap();
        document["playlists"][0]["memberships"][0]["item"]["locations"][0]["relativePath"] =
            serde_json::json!(path);
        assert!(codec::parse_json(&document.to_string()).is_err());
    }
    assert!(codec::parse_json("{").is_err());
    assert!(codec::parse_json(&"x".repeat(codec::MAX_BYTES + 1)).is_err());
    assert!(codec::document_bytes(&"x".repeat(codec::MAX_BYTES)).is_err());
    assert!(codec::parse_m3u(&"song.flac\n".repeat(codec::MAX_MEMBERSHIPS + 1)).is_err());
}

#[test]
fn music_transfer_admits_database_bytes_before_loading_selected_families() {
    crate::test_support::block_on(async {
        let oversized = "x".repeat(super::transfer_read::MAX_READ_BYTES / 4);
        for statement in [
            "UPDATE music_local_roots SET name=?",
            "UPDATE music_playlists SET icon=?",
            "UPDATE music_library_items SET original_title=?",
            "UPDATE music_snoozes SET reason=?",
            "UPDATE music_context_assignments SET provenance_id=?",
        ] {
            let pool = super::pool().await;
            let initial = reviewed(&pool, source(), "initial").await;
            transfer::commit(&pool, initial, &[]).await.unwrap();
            sqlx::query("INSERT INTO music_context_assignments (owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, updated_at_ms) VALUES ('project-default', 'project', 'focus', 'play-automatically', 'playlist-1', 'explicit', 1700000000000)")
                .execute(&pool).await.unwrap();
            let changed = sqlx::query(statement)
                .bind(&oversized)
                .execute(&pool)
                .await
                .unwrap();
            assert!(changed.rows_affected() > 0);
            let error = transfer::export(
                &pool,
                &MusicTransferExport {
                    playlist_ids: vec!["playlist-1".into()],
                    format: MusicTransferFormat::Json,
                },
                &[],
            )
            .await
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("shared record or byte allowance"),
                "{statement}: {error}"
            );
            let error = transfer::preview(&pool, &source(), &[]).await.unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("shared record or byte allowance"),
                "{statement}: {error}"
            );
            let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_transfer_receipts")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(receipts, 1);
        }
    });
}

#[test]
fn music_transfer_shares_database_allowance_across_items_children_and_conflict_reads() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let initial = reviewed(&pool, source(), "initial").await;
        transfer::commit(&pool, initial, &[]).await.unwrap();
        let text = "x".repeat(super::transfer_read::MAX_READ_BYTES / 8);
        sqlx::query("UPDATE music_library_items SET original_title=?")
            .bind(&text)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE music_snoozes SET reason=?")
            .bind(&text)
            .execute(&pool)
            .await
            .unwrap();
        let export = MusicTransferExport {
            playlist_ids: vec!["playlist-1".into()],
            format: MusicTransferFormat::Json,
        };
        assert!(
            transfer::export(&pool, &export, &[])
                .await
                .unwrap_err()
                .to_string()
                .contains("shared record or byte allowance")
        );
        assert!(
            transfer::preview(&pool, &source(), &[])
                .await
                .unwrap_err()
                .to_string()
                .contains("shared record or byte allowance")
        );
        sqlx::query("UPDATE music_library_items SET original_title='Quiet rain'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE music_snoozes SET reason='Rest'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM numbers WHERE n<100000) INSERT INTO music_membership_skip_ranges (id, membership_id, start_ms, end_ms, sort_order) SELECT 'range-' || n, (SELECT id FROM music_playlist_memberships LIMIT 1), 10000, 12000, n FROM numbers")
            .execute(&pool).await.unwrap();
        assert!(
            transfer::export(&pool, &export, &[])
                .await
                .unwrap_err()
                .to_string()
                .contains("shared record or byte allowance")
        );
    });
}

#[test]
fn music_transfer_rolls_back_every_write_if_receipt_persistence_fails() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let request = reviewed(&pool, source(), "rollback").await;
        sqlx::query("CREATE TRIGGER fail_receipt BEFORE INSERT ON music_transfer_receipts BEGIN SELECT RAISE(ABORT, 'receipt failure'); END").execute(&pool).await.unwrap();
        assert!(transfer::commit(&pool, request.clone(), &[]).await.is_err());
        for table in [
            "music_library_items",
            "music_local_roots",
            "music_playlist_memberships",
            "music_item_signals",
            "music_snoozes",
            "music_transfer_receipts",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(count, 0, "{table} must roll back");
        }
        sqlx::query("DROP TRIGGER fail_receipt")
            .execute(&pool)
            .await
            .unwrap();
        let result = transfer::commit(&pool, request.clone(), &[]).await.unwrap();
        assert_eq!(
            transfer::commit(&pool, request.clone(), &[]).await.unwrap(),
            result
        );
        let mut changed = request;
        changed.replace_item_descriptions = true;
        assert_eq!(
            transfer::commit(&pool, changed, &[])
                .await
                .unwrap_err()
                .code,
            MusicLibraryErrorCode::Conflict
        );
    });
}

#[test]
fn music_transfer_revalidates_child_records_and_bound_roots() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let initial = reviewed(&pool, source(), "initial").await;
        transfer::commit(&pool, initial, &[]).await.unwrap();
        let mut request = reviewed(&pool, source(), "replace").await;
        request.playlist_conflict = MusicImportPlaylistConflict::ReplaceExisting;
        sqlx::query("UPDATE music_snoozes SET reason = 'Edited after preview'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            transfer::commit(&pool, request, &[])
                .await
                .unwrap_err()
                .code,
            MusicLibraryErrorCode::StaleWrite
        );
        let request = reviewed(&pool, source(), "binding").await;
        let bindings = vec![TransferBinding {
            root_id: "root-1".to_string(),
            folder_path: "/music".to_string(),
            available: true,
            windows: false,
        }];
        assert_eq!(
            transfer::commit(&pool, request, &bindings)
                .await
                .unwrap_err()
                .code,
            MusicLibraryErrorCode::StaleWrite
        );
    });
}

#[test]
fn music_transfer_matches_only_incoming_records_in_a_larger_library() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let initial = reviewed(&pool, source(), "initial").await;
        transfer::commit(&pool, initial, &[]).await.unwrap();
        sqlx::query("WITH RECURSIVE numbers(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM numbers WHERE n < 12000) INSERT INTO music_library_items (id, identity_key, source_kind, original_title, discovered_at_ms, updated_at_ms) SELECT 'unrelated-' || n, 'unrelated:' || n, 'local-file', 'Unrelated', 1700000000000, 1700000000000 FROM numbers").execute(&pool).await.unwrap();
        let preview = transfer::preview(&pool, &source(), &[]).await.unwrap();
        assert_eq!((preview.matched_items, preview.new_items), (1, 0));
        let mut transaction = pool.begin().await.unwrap();
        let snapshot = super::transfer_export::snapshot(
            &mut transaction,
            &["playlist-1".to_string()],
            1_700_000_000_000,
            &[],
        )
        .await
        .unwrap();
        assert_eq!(snapshot.playlists[0].memberships.len(), 1);
        assert_eq!(snapshot.roots.len(), 1);
        assert_eq!(
            codec::parse_json(&codec::serialize_json(snapshot.clone()).unwrap()).unwrap(),
            snapshot
        );
    });
}

#[test]
fn music_transfer_matches_windows_case_without_changing_portable_identity() {
    crate::test_support::block_on(async {
        let pool = super::pool().await;
        let initial = reviewed(&pool, source(), "initial").await;
        transfer::commit(&pool, initial, &[]).await.unwrap();
        sqlx::query("UPDATE music_local_locations SET relative_path = 'Album/Café.flac', availability = 'available'").execute(&pool).await.unwrap();
        let bindings = vec![TransferBinding {
            root_id: "root-1".to_string(),
            folder_path: "C:\\Music".to_string(),
            available: true,
            windows: true,
        }];
        let source = MusicTransferSource { contents: "#EXTM3U\nC:\\MUSIC\\ALBUM\\CAFÉ.FLAC\n../outside.flac\nhttps://example.org/file.mp3".to_string(), relative_root_id: Some("root-1".to_string()), ..source() };
        let preview = transfer::preview(&pool, &source, &bindings).await.unwrap();
        assert_eq!(
            (
                preview.matched_items,
                preview.new_items,
                preview.unresolved_local_count,
                preview.unsupported_count
            ),
            (1, 0, 1, 1)
        );
        let android = vec![TransferBinding {
            root_id: "root-1".to_string(),
            folder_path: "content://media/tree/root".to_string(),
            available: true,
            windows: false,
        }];
        let source = MusicTransferSource {
            contents: "Album/Café.flac".to_string(),
            ..source
        };
        assert_eq!(
            transfer::preview(&pool, &source, &android)
                .await
                .unwrap()
                .matched_items,
            1
        );
        let mut transaction = pool.begin().await.unwrap();
        let snapshot = super::transfer_export::snapshot(
            &mut transaction,
            &["playlist-1".to_string()],
            1_700_000_000_000,
            &android,
        )
        .await
        .unwrap();
        assert_eq!(
            super::transfer_export::m3u(&snapshot, &android).unwrap(),
            "#EXTM3U\n"
        );
        assert!(
            !codec::serialize_json(snapshot)
                .unwrap()
                .contains("content://")
        );
    });
}

struct TransferDatabase(std::path::PathBuf);

impl TransferDatabase {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ganbaru-music-transfer-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    async fn open(&self) -> sqlx::SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(self.0.join("test.sqlite"))
                    .create_if_missing(true)
                    .foreign_keys(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
            )
            .await
            .unwrap()
    }
}

impl Drop for TransferDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn music_transfer_retry_after_restart_returns_the_committed_receipt() {
    crate::test_support::block_on(async {
        let database = TransferDatabase::new();
        let pool = database.open().await;
        ganbaru_db::run_migrations(&pool).await.unwrap();
        let request = reviewed(&pool, source(), "lost-response").await;
        let expected = transfer::commit(&pool, request.clone(), &[]).await.unwrap();
        pool.close().await;
        let reopened = database.open().await;
        assert_eq!(
            transfer::commit(&reopened, request, &[]).await.unwrap(),
            expected
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_playlist_memberships")
                .fetch_one(&reopened)
                .await
                .unwrap(),
            1
        );
        reopened.close().await;
    });
}

#[test]
fn music_transfer_snapshot_never_mixes_a_concurrent_edit() {
    crate::test_support::block_on(async {
        let database = TransferDatabase::new();
        let pool = database.open().await;
        ganbaru_db::run_migrations(&pool).await.unwrap();
        let request = reviewed(&pool, source(), "initial").await;
        transfer::commit(&pool, request, &[]).await.unwrap();
        let mut snapshot = pool.begin().await.unwrap();
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM music_playlists")
            .fetch_one(&mut *snapshot)
            .await
            .unwrap();
        let mut edit = pool.begin().await.unwrap();
        sqlx::query("UPDATE music_library_items SET original_title = 'Edited concurrently', version = version + 1").execute(&mut *edit).await.unwrap();
        sqlx::query(
            "UPDATE music_playlist_memberships SET weight = 'more-often', version = version + 1",
        )
        .execute(&mut *edit)
        .await
        .unwrap();
        edit.commit().await.unwrap();
        let document = super::transfer_export::snapshot(
            &mut snapshot,
            &["playlist-1".to_string()],
            1_700_000_000_000,
            &[],
        )
        .await
        .unwrap();
        snapshot.commit().await.unwrap();
        assert_eq!(
            document.playlists[0].memberships[0].item.title,
            "Quiet rain"
        );
        assert_eq!(
            document.playlists[0].memberships[0].weight,
            MusicWeight::LessOften
        );
        let contents = transfer::export(
            &pool,
            &MusicTransferExport {
                playlist_ids: vec!["playlist-1".to_string()],
                format: MusicTransferFormat::Json,
            },
            &[],
        )
        .await
        .unwrap();
        let changed = codec::parse_json(&contents).unwrap();
        assert_eq!(
            changed.playlists[0].memberships[0].item.title,
            "Edited concurrently"
        );
        assert_eq!(
            changed.playlists[0].memberships[0].weight,
            MusicWeight::MoreOften
        );
        pool.close().await;
    });
}

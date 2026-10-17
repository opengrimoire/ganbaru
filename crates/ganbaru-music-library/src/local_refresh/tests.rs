use super::*;
use sqlx::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

async fn migrated_pool() -> SqlitePool {
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

async fn seed_root(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES ('root-1', 'Test music', 1700000000000, 1700000000000)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES ('collection-1', 'local-root', 'local-root:root-1',
                 'Test music', 'root-1', 1700000000000, 1700000000000)",
    )
    .execute(pool)
    .await
    .unwrap();
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ganbaru-music-refresh-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn request(job_id: &str, root: &Path, requested_at_ms: i64) -> MusicLocalRefreshRequest {
    MusicLocalRefreshRequest {
        job_id: job_id.to_string(),
        root_id: "root-1".to_string(),
        collection_id: "collection-1".to_string(),
        folder_path: root.to_string_lossy().to_string(),
        available_roots: vec![super::super::MusicAvailableRootPath {
            root_id: "root-1".to_string(),
            folder_path: root.to_string_lossy().to_string(),
        }],
        requested_at_ms,
    }
}

#[test]
fn complete_refresh_catalogs_large_file_sets_with_bounded_staging() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("dense");
        for index in 0..5_001_u32 {
            let album = root.join(format!("Album {:02}", index % 17));
            fs::create_dir_all(&album).unwrap();
            fs::write(
                album.join(format!("Track {index:05}.mp3")),
                index.to_le_bytes(),
            )
            .unwrap();
        }
        fs::write(root.join("ignored.txt"), b"not media").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Album 00"), root.join("linked-album")).unwrap();

        let refresh = request("refresh-dense", &root, 1_700_000_000_000);
        let queued = prepare(&pool, &refresh).await.unwrap();
        assert_eq!(queued.state, super::super::MusicRefreshJobState::Queued);
        let completed = run_prepared(&pool, refresh).await.unwrap();
        assert_eq!(
            completed.state,
            super::super::MusicRefreshJobState::Completed
        );
        assert_eq!(completed.discovered_count, 5_001);
        assert_eq!(completed.processed_count, 5_001);
        assert!(completed.skipped_count >= 1);
        assert!(completed.absence_determined);
        assert_eq!(completed.truncated_count, 0);

        let locations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_local_locations WHERE root_id = 'root-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let staged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_refresh_job_entries WHERE job_id = 'refresh-dense'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(locations, 5_001);
        assert_eq!(staged, 0);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn only_a_complete_current_generation_marks_absent_locations_missing() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("reconcile");
        fs::write(root.join("kept.mp3"), b"kept media").unwrap();
        fs::write(root.join("removed.mp3"), b"removed media").unwrap();

        let first = request("refresh-first", &root, 1_700_000_000_000);
        prepare(&pool, &first).await.unwrap();
        let first_result = run_prepared(&pool, first).await.unwrap();
        assert!(first_result.absence_determined);

        fs::remove_file(root.join("removed.mp3")).unwrap();
        let stale = request("refresh-stale", &root, 1_700_000_100_000);
        prepare(&pool, &stale).await.unwrap();
        persistence::mark_running(&pool, &stale.job_id)
            .await
            .unwrap();
        let newer = request("refresh-newer", &root, 1_700_000_200_000);
        prepare(&pool, &newer).await.unwrap();
        assert!(!persistence::is_current(&pool, &stale.job_id).await.unwrap());
        let availability_before: String = sqlx::query_scalar(
            "SELECT availability FROM music_local_locations WHERE relative_path = 'removed.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(availability_before, "available");

        let completed = run_prepared(&pool, newer).await.unwrap();
        assert!(completed.absence_determined);
        let availability_after: String = sqlx::query_scalar(
            "SELECT availability FROM music_local_locations WHERE relative_path = 'removed.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(availability_after, "missing");
        let stale_progress = progress(&pool, "refresh-stale").await.unwrap();
        assert_eq!(
            stale_progress.state,
            super::super::MusicRefreshJobState::Cancelled
        );
        assert!(!stale_progress.absence_determined);

        fs::write(root.join("removed.mp3"), b"restored media").unwrap();
        let moved_root = root.with_file_name(format!(
            "{}-moved",
            root.file_name().unwrap().to_string_lossy()
        ));
        fs::rename(&root, &moved_root).unwrap();
        let restored = request("refresh-restored", &moved_root, 1_700_000_300_000);
        prepare(&pool, &restored).await.unwrap();
        run_prepared(&pool, restored).await.unwrap();
        let restored_availability: String = sqlx::query_scalar(
            "SELECT availability FROM music_local_locations WHERE relative_path = 'removed.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(restored_availability, "available");
        fs::remove_dir_all(moved_root).unwrap();
    });
}

#[test]
fn tagged_metadata_updates_without_overwriting_user_authored_fields() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("metadata");
        fs::write(root.join("track.mp3"), id3_title("Original tagged title")).unwrap();
        let first = request("refresh-metadata-1", &root, 1_700_000_000_000);
        prepare(&pool, &first).await.unwrap();
        run_prepared(&pool, first).await.unwrap();
        sqlx::query(
            "UPDATE music_library_items SET title_override = 'My preferred title',
             review_state = 'reviewed'",
        )
        .execute(&pool)
        .await
        .unwrap();

        fs::write(root.join("track.mp3"), id3_title("Updated tagged title")).unwrap();
        let second = request("refresh-metadata-2", &root, 1_700_000_100_000);
        prepare(&pool, &second).await.unwrap();
        run_prepared(&pool, second).await.unwrap();
        let stored: (String, Option<String>, String, String, i64, i64) = sqlx::query_as(
            "SELECT original_title, title_override, review_state, media_kind,
                    file_size_bytes, modified_at_ms
             FROM music_library_items AS item
             JOIN music_local_locations AS location ON location.item_id = item.id
             WHERE location.relative_path = 'track.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored.0, "Updated tagged title");
        assert_eq!(stored.1.as_deref(), Some("My preferred title"));
        assert_eq!(stored.2, "reviewed");
        assert_eq!(stored.3, "audio");
        assert!(stored.4 > 0);
        assert!(stored.5 > 0);
        fs::remove_dir_all(root).unwrap();
    });
}

fn id3_title(title: &str) -> Vec<u8> {
    let mut content = vec![3];
    content.extend_from_slice(title.as_bytes());
    let mut frame = b"TIT2".to_vec();
    frame.extend_from_slice(&(content.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend(content);
    let size = frame.len() as u32;
    let mut bytes = b"ID3\x03\0\0".to_vec();
    bytes.extend_from_slice(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ]);
    bytes.extend(frame);
    bytes
}

#[test]
fn local_paths_remain_distinct_even_when_content_matches() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        seed_extra_root(&pool, "root-2", "collection-2").await;
        seed_extra_root(&pool, "root-3", "collection-3").await;
        let root_1 = temporary_root("identity-a");
        let root_2 = temporary_root("identity-b");
        let root_3 = temporary_root("identity-copy");
        let content_a = collision_fixture(1);
        let content_b = collision_fixture(2);
        fs::write(root_1.join("Theme.mp3"), &content_a).unwrap();
        fs::write(root_2.join("Theme.mp3"), &content_b).unwrap();
        fs::write(root_3.join("Copied theme.mp3"), &content_a).unwrap();
        let roots = vec![
            available_root("root-1", &root_1),
            available_root("root-2", &root_2),
            available_root("root-3", &root_3),
        ];

        run_root_refresh(
            &pool,
            "identity-1",
            "root-1",
            "collection-1",
            &root_1,
            roots.clone(),
            1_700_000_000_000,
        )
        .await;
        run_root_refresh(
            &pool,
            "identity-2",
            "root-2",
            "collection-2",
            &root_2,
            roots.clone(),
            1_700_000_100_000,
        )
        .await;
        let distinct_after_collision: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM music_library_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(distinct_after_collision, 2);

        run_root_refresh(
            &pool,
            "identity-3",
            "root-3",
            "collection-3",
            &root_3,
            roots.clone(),
            1_700_000_200_000,
        )
        .await;
        let distinct_after_copy: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM music_library_items")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(distinct_after_copy, 3);
        let copied_item_locations: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_local_locations
             WHERE item_id = (
                SELECT item_id FROM music_local_locations
                WHERE root_id = 'root-1' AND relative_path = 'Theme.mp3'
             )",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(copied_item_locations, 1);

        let root_1_item: String = sqlx::query_scalar(
            "SELECT item_id FROM music_local_locations
             WHERE root_id = 'root-1' AND relative_path = 'Theme.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE music_local_locations SET item_id = ?
             WHERE root_id = 'root-3' AND relative_path = 'Copied theme.mp3'",
        )
        .bind(&root_1_item)
        .execute(&pool)
        .await
        .unwrap();
        run_root_refresh(
            &pool,
            "identity-repair",
            "root-3",
            "collection-3",
            &root_3,
            roots.clone(),
            1_700_000_250_000,
        )
        .await;
        let repaired_distinct_items: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT item_id) FROM music_local_locations
             WHERE (root_id = 'root-1' AND relative_path = 'Theme.mp3')
                OR (root_id = 'root-3' AND relative_path = 'Copied theme.mp3')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(repaired_distinct_items, 2);

        fs::rename(root_1.join("Theme.mp3"), root_1.join("Renamed theme.mp3")).unwrap();
        run_root_refresh(
            &pool,
            "identity-4",
            "root-1",
            "collection-1",
            &root_1,
            roots,
            1_700_000_300_000,
        )
        .await;
        let renamed_item_matches: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT item_id) FROM music_local_locations
             WHERE (root_id = 'root-1' AND relative_path = 'Renamed theme.mp3')
                OR (root_id = 'root-3' AND relative_path = 'Copied theme.mp3')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(renamed_item_matches, 2);
        let old_location: String = sqlx::query_scalar(
            "SELECT availability FROM music_local_locations
             WHERE root_id = 'root-1' AND relative_path = 'Theme.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_location, "missing");

        fs::remove_dir_all(root_1).unwrap();
        fs::remove_dir_all(root_2).unwrap();
        fs::remove_dir_all(root_3).unwrap();
    });
}

#[test]
fn refresh_normalizes_cross_platform_separators_without_collapsing_path_case() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("path-normalization");
        fs::create_dir_all(root.join("Album")).unwrap();
        fs::write(root.join("Album/Track.mp3"), b"upper path content").unwrap();
        let first = request("path-first", &root, 1_700_000_000_000);
        prepare(&pool, &first).await.unwrap();
        run_prepared(&pool, first).await.unwrap();
        sqlx::query(
            "UPDATE music_local_locations SET relative_path = 'Album\\Track.mp3'
             WHERE root_id = 'root-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let second = request("path-second", &root, 1_700_000_100_000);
        prepare(&pool, &second).await.unwrap();
        run_prepared(&pool, second).await.unwrap();
        let locations: Vec<String> = sqlx::query_scalar(
            "SELECT relative_path FROM music_local_locations WHERE root_id = 'root-1'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(locations, vec!["Album/Track.mp3"]);

        #[cfg(not(target_os = "windows"))]
        {
            fs::write(root.join("Album/track.mp3"), b"lower path content").unwrap();
            let third = request("path-third", &root, 1_700_000_200_000);
            prepare(&pool, &third).await.unwrap();
            run_prepared(&pool, third).await.unwrap();
            let distinct: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM music_local_locations
                 WHERE relative_path IN ('Album/Track.mp3', 'Album/track.mp3')",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(distinct, 2);
        }
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn unavailable_matching_content_remains_distinct_and_available() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        seed_extra_root(&pool, "root-2", "collection-2").await;
        let old_root = temporary_root("ambiguous-unavailable-old");
        let new_root = temporary_root("ambiguous-unavailable-new");
        fs::write(old_root.join("Original.mp3"), b"shared media content").unwrap();
        fs::write(new_root.join("Copy.mp3"), b"shared media content").unwrap();
        run_root_refresh(
            &pool,
            "ambiguous-old",
            "root-1",
            "collection-1",
            &old_root,
            vec![available_root("root-1", &old_root)],
            1_700_000_000_000,
        )
        .await;
        fs::remove_dir_all(&old_root).unwrap();
        run_root_refresh(
            &pool,
            "ambiguous-new",
            "root-2",
            "collection-2",
            &new_root,
            vec![available_root("root-2", &new_root)],
            1_700_000_100_000,
        )
        .await;
        let ambiguous: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_library_items WHERE availability = 'ambiguous'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ambiguous, 0);
        let canonical_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM music_library_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(canonical_count, 2);
        fs::remove_dir_all(new_root).unwrap();
    });
}

#[test]
fn inaccessible_refresh_preserves_last_known_availability() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("inaccessible");
        fs::write(root.join("known.mp3"), b"known content").unwrap();
        let initial = request("accessible", &root, 1_700_000_000_000);
        prepare(&pool, &initial).await.unwrap();
        run_prepared(&pool, initial).await.unwrap();

        let failed = request("inaccessible", &root, 1_700_000_100_000);
        prepare(&pool, &failed).await.unwrap();
        fs::remove_dir_all(&root).unwrap();
        let progress = run_prepared(&pool, failed).await.unwrap();
        assert_eq!(progress.state, super::super::MusicRefreshJobState::Failed);
        assert!(!progress.absence_determined);
        let availability: String =
            sqlx::query_scalar("SELECT availability FROM music_local_locations")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(availability, "available");
    });
}

#[test]
fn partial_refresh_keeps_unseen_locations_available_and_reports_uncertainty() {
    crate::test_support::block_on(async {
        let pool = migrated_pool().await;
        seed_root(&pool).await;
        let root = temporary_root("partial");
        fs::write(root.join("seen.mp3"), b"seen content").unwrap();
        fs::write(root.join("unseen.mp3"), b"unseen content").unwrap();
        let initial = request("partial-initial", &root, 1_700_000_000_000);
        prepare(&pool, &initial).await.unwrap();
        run_prepared(&pool, initial).await.unwrap();

        let partial = request("partial-job", &root, 1_700_000_100_000);
        prepare(&pool, &partial).await.unwrap();
        persistence::mark_running(&pool, &partial.job_id)
            .await
            .unwrap();
        let initial_progress = persistence::load_progress(&pool, &partial.job_id)
            .await
            .unwrap();
        persistence::save_discovery_batch(
            &pool,
            &partial.job_id,
            "",
            &[traversal::DiscoveredEntry {
                relative_path: "seen.mp3".to_string(),
                kind: traversal::DiscoveredEntryKind::Media,
            }],
            true,
        )
        .await
        .unwrap();
        let mut artwork_cache = traversal::ArtworkCache::new();
        let evidence = traversal::inspect_media(&root, "seen.mp3", &mut artwork_cache).unwrap();
        persistence::reconcile_batch(&pool, &partial, initial_progress.generation, &[evidence])
            .await
            .unwrap();
        persistence::finish_incomplete(
            &pool,
            &partial,
            "A subfolder became inaccessible. Absence was not determined.",
        )
        .await
        .unwrap();
        let result = progress(&pool, &partial.job_id).await.unwrap();
        assert_eq!(result.state, super::super::MusicRefreshJobState::Partial);
        assert!(!result.absence_determined);
        let unseen_availability: String = sqlx::query_scalar(
            "SELECT availability FROM music_local_locations WHERE relative_path = 'unseen.mp3'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unseen_availability, "available");
        fs::remove_dir_all(root).unwrap();
    });
}

async fn seed_extra_root(pool: &SqlitePool, root_id: &str, collection_id: &str) {
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?)",
    )
    .bind(root_id)
    .bind(root_id)
    .bind(1_700_000_000_000_i64)
    .bind(1_700_000_000_000_i64)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES (?, 'local-root', ?, ?, ?, ?, ?)",
    )
    .bind(collection_id)
    .bind(format!("local-root:{root_id}"))
    .bind(root_id)
    .bind(root_id)
    .bind(1_700_000_000_000_i64)
    .bind(1_700_000_000_000_i64)
    .execute(pool)
    .await
    .unwrap();
}

fn available_root(root_id: &str, path: &Path) -> super::super::MusicAvailableRootPath {
    super::super::MusicAvailableRootPath {
        root_id: root_id.to_string(),
        folder_path: path.to_string_lossy().to_string(),
    }
}

async fn run_root_refresh(
    pool: &SqlitePool,
    job_id: &str,
    root_id: &str,
    collection_id: &str,
    root: &Path,
    available_roots: Vec<super::super::MusicAvailableRootPath>,
    requested_at_ms: i64,
) {
    let refresh = MusicLocalRefreshRequest {
        job_id: job_id.to_string(),
        root_id: root_id.to_string(),
        collection_id: collection_id.to_string(),
        folder_path: root.to_string_lossy().to_string(),
        available_roots,
        requested_at_ms,
    };
    prepare(pool, &refresh).await.unwrap();
    let result = run_prepared(pool, refresh).await.unwrap();
    assert_eq!(result.state, super::super::MusicRefreshJobState::Completed);
}

fn collision_fixture(middle: u8) -> Vec<u8> {
    let mut bytes = vec![7_u8; 192 * 1024];
    bytes[64 * 1024..128 * 1024].fill(middle);
    bytes
}
#[test]
fn inspection_workers_are_bounded_by_files_cpus_and_disk_pressure_cap() {
    assert_eq!(inspection_worker_count(1, 16), 1);
    assert_eq!(inspection_worker_count(64, 1), 1);
    assert_eq!(inspection_worker_count(64, 2), 2);
    assert_eq!(inspection_worker_count(64, 16), 4);
}

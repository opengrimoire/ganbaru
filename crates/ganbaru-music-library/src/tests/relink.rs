use super::super::relink;
use super::*;
use sqlx::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-music-relink-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative_path: &str, bytes: &[u8]) {
        let path = self.0.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

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

async fn seed_root(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO music_local_roots (id, name, created_at_ms, updated_at_ms)
         VALUES ('root-1', 'Soundtracks', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collections
            (id, kind, identity_key, name, local_root_id, created_at_ms, updated_at_ms)
         VALUES ('collection-1', 'local-root', 'root:1', 'Soundtracks', 'root-1', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_location(
    pool: &SqlitePool,
    old_root: &TestDirectory,
    item_id: &str,
    location_id: &str,
    relative_path: &str,
) {
    let mut artwork = local_refresh::RelinkArtworkCache::new();
    let evidence =
        local_refresh::inspect_for_relink(old_root.path(), relative_path, &mut artwork).unwrap();
    sqlx::query(
        "INSERT INTO music_library_items
            (id, identity_key, source_kind, media_kind, original_title,
             availability, discovered_at_ms, updated_at_ms)
         VALUES (?, ?, 'local-file', 'audio', ?, 'missing', 1, 1)",
    )
    .bind(item_id)
    .bind(format!("local:{item_id}"))
    .bind(&evidence.title)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_local_locations
            (id, item_id, root_id, relative_path, file_size_bytes,
             lightweight_fingerprint, availability, first_seen_at_ms, updated_at_ms)
         VALUES (?, ?, 'root-1', ?, ?, ?, 'missing', 1, 1)",
    )
    .bind(location_id)
    .bind(item_id)
    .bind(relative_path)
    .bind(evidence.file_size_bytes)
    .bind(&evidence.lightweight_fingerprint)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO music_source_collection_items
            (collection_id, item_id, first_discovered_at_ms)
         VALUES ('collection-1', ?, 1)",
    )
    .bind(item_id)
    .execute(pool)
    .await
    .unwrap();
}

fn plan_request(folder: &TestDirectory, plan_id: &str) -> MusicRelinkPlanRequest {
    MusicRelinkPlanRequest {
        plan_id: plan_id.to_string(),
        root_id: "root-1".to_string(),
        replacement_folder_path: folder.path().to_string_lossy().into_owned(),
        created_at_ms: 100,
    }
}

#[test]
fn relink_plan_classifies_and_applies_reorganized_partial_roots() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_root(&pool).await;
        let old = TestDirectory::new("old");
        old.write("album/exact.mp3", b"exact media bytes");
        old.write("old/likely.mp3", b"likely media bytes");
        old.write("missing.mp3", b"missing media bytes");
        seed_location(
            &pool,
            &old,
            "item-exact",
            "location-exact",
            "album/exact.mp3",
        )
        .await;
        seed_location(
            &pool,
            &old,
            "item-likely",
            "location-likely",
            "old/likely.mp3",
        )
        .await;
        seed_location(
            &pool,
            &old,
            "item-missing",
            "location-missing",
            "missing.mp3",
        )
        .await;
        sqlx::query(
            "INSERT INTO music_playlists (id, name, created_at_ms, updated_at_ms)
             VALUES ('playlist-1', 'Focus', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO music_playlist_memberships
                (id, playlist_id, item_id, position, created_at_ms, updated_at_ms)
             VALUES ('membership-1', 'playlist-1', 'item-likely', 0, 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let replacement = TestDirectory::new("replacement");
        replacement.write("album/exact.mp3", b"exact media bytes");
        replacement.write("moved/likely.mp3", b"likely media bytes");
        replacement.write("new.mp3", b"new media bytes");
        let summary = relink::create_plan(&pool, plan_request(&replacement, "plan-1"))
            .await
            .unwrap();
        assert_eq!(summary.exact_count, 1);
        assert_eq!(summary.likely_count, 1);
        assert_eq!(summary.missing_count, 1);
        assert_eq!(summary.new_count, 1);

        let window = relink::plan_entries(&pool, "plan-1", 0, 20).await.unwrap();
        let likely = window
            .entries
            .iter()
            .find(|entry| entry.match_kind == MusicRelinkMatchKind::Likely)
            .unwrap();
        let applied = relink::apply_plan(
            &pool,
            MusicRelinkApplyRequest {
                plan_id: "plan-1".to_string(),
                decisions: vec![MusicRelinkDecision {
                    entry_id: likely.id.clone(),
                    item_id: "item-likely".to_string(),
                }],
                applied_at: 200,
            },
        )
        .await
        .unwrap();
        assert_eq!(applied.state, MusicRelinkPlanState::Applied);
        let paths: Vec<(String, String)> = sqlx::query_as(
            "SELECT item_id, relative_path FROM music_local_locations ORDER BY item_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            paths,
            vec![
                ("item-exact".to_string(), "album/exact.mp3".to_string()),
                ("item-likely".to_string(), "moved/likely.mp3".to_string()),
                ("item-missing".to_string(), "missing.mp3".to_string()),
            ]
        );
        let membership_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM music_playlist_memberships WHERE item_id = 'item-likely'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(membership_count, 1);
    });
}

#[test]
fn ambiguous_relink_candidates_remain_visible_until_confirmed() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_root(&pool).await;
        let old = TestDirectory::new("ambiguous-old");
        old.write("one.mp3", b"identical media bytes");
        old.write("two.mp3", b"identical media bytes");
        seed_location(&pool, &old, "item-one", "location-one", "one.mp3").await;
        seed_location(&pool, &old, "item-two", "location-two", "two.mp3").await;
        let replacement = TestDirectory::new("ambiguous-new");
        replacement.write("renamed.mp3", b"identical media bytes");

        let summary = relink::create_plan(&pool, plan_request(&replacement, "plan-ambiguous"))
            .await
            .unwrap();
        assert_eq!(summary.ambiguous_count, 1);
        assert_eq!(summary.missing_count, 0);
        let entry = relink::plan_entries(&pool, "plan-ambiguous", 0, 20)
            .await
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.match_kind == MusicRelinkMatchKind::Ambiguous)
            .unwrap();
        assert_eq!(entry.candidate_item_ids.len(), 2);

        relink::apply_plan(
            &pool,
            MusicRelinkApplyRequest {
                plan_id: "plan-ambiguous".to_string(),
                decisions: Vec::new(),
                applied_at: 200,
            },
        )
        .await
        .unwrap();
        let issues = queries::issues(&pool, 0, 20).await.unwrap();
        assert!(issues.iter().any(|issue| {
            issue.issue_kind == "relink-ambiguous" && issue.root_id.as_deref() == Some("root-1")
        }));
    });
}

#[test]
fn cancelling_a_relink_plan_performs_no_location_writes() {
    crate::test_support::block_on(async {
        let pool = pool().await;
        seed_root(&pool).await;
        let old = TestDirectory::new("cancel-old");
        old.write("old.mp3", b"cancel media bytes");
        seed_location(&pool, &old, "item-1", "location-1", "old.mp3").await;
        let replacement = TestDirectory::new("cancel-new");
        replacement.write("moved.mp3", b"cancel media bytes");
        relink::create_plan(&pool, plan_request(&replacement, "plan-cancel"))
            .await
            .unwrap();
        let cancelled = relink::cancel_plan(&pool, "plan-cancel", 150)
            .await
            .unwrap();
        assert_eq!(cancelled.state, MusicRelinkPlanState::Cancelled);
        let stored_path: String = sqlx::query_scalar(
            "SELECT relative_path FROM music_local_locations WHERE id = 'location-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_path, "old.mp3");
    });
}

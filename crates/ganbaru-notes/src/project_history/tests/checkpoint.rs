use super::*;

#[test]
fn checkpoints_deduplicate_rows_and_skip_unchanged_manifests() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        let first = create_checkpoint(&pool, PROJECT_ID, "baseline", None, None, "Initial version")
            .await
            .unwrap();
        assert!(first.is_some());
        let initial_bundle_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles")
                .fetch_one(&pool)
                .await
                .unwrap();
        let unchanged = create_checkpoint(&pool, PROJECT_ID, "checkpoint", None, None, "No change")
            .await
            .unwrap();
        assert!(unchanged.is_none());
        let unchanged_bundle_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(unchanged_bundle_count, initial_bundle_count);

        sqlx::query(
            "UPDATE notes_blocks
             SET plain_text = 'Second version',
                 payload = json_set(payload, '$.paragraph.rich_text[0].text.content', 'Second version')
             WHERE id = ?",
        )
        .bind(BLOCK_ID)
        .execute(&pool)
        .await
        .unwrap();
        let changed = create_checkpoint(
            &pool,
            PROJECT_ID,
            "checkpoint",
            None,
            None,
            "Edited one note",
        )
        .await
        .unwrap();
        assert!(changed.is_some());
        let changed_bundle_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(changed_bundle_count, initial_bundle_count + 2);
    });
}

#[test]
fn checkpoint_rolls_back_bundles_when_version_insertion_fails() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        sqlx::raw_sql(
            "CREATE TRIGGER reject_project_history_version
             BEFORE INSERT ON notes_project_history_versions
             BEGIN
               SELECT RAISE(ABORT, 'late checkpoint failure');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            create_checkpoint(&pool, PROJECT_ID, "baseline", None, None, "Initial version",)
                .await
                .is_err()
        );
        let versions: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_project_history_versions")
                .fetch_one(&pool)
                .await
                .unwrap();
        let bundles: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(versions, 0);
        assert_eq!(bundles, 0);
    });
}

#[test]
fn checkpoint_pins_assets_referenced_by_project_pages() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        let asset_id = "60606060-6060-4060-8060-606060606060";
        sqlx::query(
            "INSERT INTO notes_assets (
                id, asset_path, kind, source_type, content_type, byte_size, sha256
             ) VALUES (?, 'notes/page-icons/history.png', 'image', 'local_upload',
                       'image/png', 1, ?)",
        )
        .bind(asset_id)
        .bind("a".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_asset_references (
                asset_id, owner_type, owner_id, page_id, role
             ) VALUES (?, 'page', ?, ?, 'page_icon')",
        )
        .bind(asset_id)
        .bind(PAGE_ID)
        .bind(PAGE_ID)
        .execute(&pool)
        .await
        .unwrap();

        let version =
            create_checkpoint(&pool, PROJECT_ID, "baseline", None, None, "Initial version")
                .await
                .unwrap()
                .unwrap();
        let pinned: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_project_history_asset_pins
             WHERE version_id = ? AND asset_id = ?",
        )
        .bind(version.id)
        .bind(asset_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pinned, 1);
    });
}

#[test]
fn checkpoint_rechecks_retention_after_loading_the_graph() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;

        let version = create_checkpoint_after_graph_load(
            &pool,
            PROJECT_ID,
            "baseline",
            None,
            None,
            "Initial version",
            |pool| async move {
                sqlx::query("UPDATE projects SET notes_history_retention_days = 0 WHERE id = ?")
                    .bind(PROJECT_ID)
                    .execute(&pool)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(())
            },
        )
        .await
        .unwrap();

        assert!(version.is_none());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_project_history_versions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });
}

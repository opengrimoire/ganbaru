use super::*;

#[test]
fn orphan_maintenance_runs_only_after_its_vault_deadline() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_empty_project(&pool).await;
        flush_due_checkpoints(&pool).await.unwrap();
        let orphan_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        sqlx::query(
            "INSERT INTO notes_history_bundles (
                hash, kind, encoding, payload, uncompressed_bytes, stored_bytes
             ) VALUES (?, 'row', 'raw-json-v1', X'7B7D', 2, 2)",
        )
        .bind(orphan_hash)
        .execute(&pool)
        .await
        .unwrap();

        flush_due_checkpoints(&pool).await.unwrap();
        let retained: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles WHERE hash = ?")
                .bind(orphan_hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(retained, 1);

        sqlx::query(
            "UPDATE notes_history_maintenance_state
             SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-7 hours')
             WHERE id = 1",
        )
        .execute(&pool)
        .await
        .unwrap();
        flush_due_checkpoints(&pool).await.unwrap();
        let collected: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles WHERE hash = ?")
                .bind(orphan_hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(collected, 0);
    });
}

#[test]
fn failed_maintenance_rolls_back_garbage_collection() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_empty_project(&pool).await;
        flush_due_checkpoints(&pool).await.unwrap();
        let orphan_hash = "1111111111111111111111111111111111111111111111111111111111111111";
        sqlx::query(
            "INSERT INTO notes_history_bundles (
                hash, kind, encoding, payload, uncompressed_bytes, stored_bytes
             ) VALUES (?, 'row', 'raw-json-v1', X'7B7D', 2, 2)",
        )
        .bind(orphan_hash)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE notes_history_maintenance_state
             SET last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-7 hours')
             WHERE id = 1",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_history_maintenance_update
             BEFORE UPDATE ON notes_history_maintenance_state
             BEGIN
               SELECT RAISE(ABORT, 'maintenance failure');
             END",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            super::super::retention::run_due_maintenance(&pool)
                .await
                .is_err()
        );
        let retained: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_history_bundles WHERE hash = ?")
                .bind(orphan_hash)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(retained, 1);
    });
}

#[test]
fn disabled_project_history_prunes_versions_and_skips_new_checkpoints() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_project(&pool).await;
        assert!(
            create_checkpoint(&pool, PROJECT_ID, "baseline", None, None, "Initial version",)
                .await
                .unwrap()
                .is_some()
        );

        sqlx::query(
            "UPDATE projects
             SET notes_history_retention_days = 0
             WHERE id = ?",
        )
        .bind(PROJECT_ID)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            create_checkpoint(
                &pool,
                PROJECT_ID,
                "checkpoint",
                None,
                None,
                "This version must not be stored",
            )
            .await
            .unwrap()
            .is_none()
        );

        let version_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_project_history_versions
             WHERE project_id = ?",
        )
        .bind(PROJECT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(version_count, 0);
    });
}

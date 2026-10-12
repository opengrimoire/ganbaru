use super::*;

#[test]
fn checkpoint_planner_handles_active_idle_restart_and_forced_flushes() {
    assert!(!checkpoint_is_due(599, 119, false));
    assert!(checkpoint_is_due(600, 1, false));
    assert!(checkpoint_is_due(30, 120, false));
    assert!(checkpoint_is_due(0, 0, true));
    assert!(checkpoint_is_due(3_600, 3_600, false));
}

#[test]
fn first_mutation_creates_one_baseline_and_due_flush_returns_no_dirty_deadline() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_empty_project(&pool).await;
        ensure_project_baseline_for_mutation(&pool, PROJECT_ID)
            .await
            .unwrap();
        ensure_project_baseline_for_mutation(&pool, PROJECT_ID)
            .await
            .unwrap();
        let baseline_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_project_history_versions WHERE project_id = ?",
        )
        .bind(PROJECT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(baseline_count, 1);

        insert_project_page(&pool, PAGE_ID, BLOCK_ID, "First version").await;
        let mut tx = pool.begin().await.unwrap();
        mark_project_dirty_tx(&mut tx, PROJECT_ID, "First version", false)
            .await
            .unwrap();
        mark_project_dirty_tx(&mut tx, PROJECT_ID, "Edited again", false)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let pending = super::history_schedule(&pool, 0).await.unwrap();
        assert!(pending.next_checkpoint_at.is_some());
        let write_result = mutation_result(&pool, "saved").await.unwrap();
        let write_json = serde_json::to_value(write_result).unwrap();
        assert_eq!(write_json["value"], "saved");
        assert!(write_json["nextHistoryCheckpointAt"].is_string());

        sqlx::query(
            "UPDATE notes_project_history_dirty
             SET last_dirty_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-3 minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let flushed = flush_due_checkpoints(&pool).await.unwrap();
        assert_eq!(flushed.created_count, 1);
        assert!(flushed.next_checkpoint_at.is_none());
        let version_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_project_history_versions WHERE project_id = ?",
        )
        .bind(PROJECT_ID)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(version_count, 2);
    });
}

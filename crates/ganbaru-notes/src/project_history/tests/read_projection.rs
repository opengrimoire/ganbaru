use super::*;

#[test]
fn selecting_an_empty_project_does_not_create_a_baseline() {
    crate::test_block_on(async {
        let pool = migrated_pool().await;
        seed_empty_project(&pool).await;
        let initialized = initialize_project_history(&pool, PROJECT_ID).await.unwrap();
        assert!(initialized.is_none());
        let version_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_project_history_versions")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(version_count, 0);
    });
}

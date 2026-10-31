use super::helpers::migrated_memory_pool;

const NOW: &str = "2026-10-22T10:51:41.000Z";

async fn insert(pool: &sqlx::SqlitePool, key: &str, value: &str, updated_at: &str) -> bool {
    sqlx::query("INSERT INTO vault_preferences (key, value, updated_at) VALUES (?, ?, ?)")
        .bind(key)
        .bind(value)
        .bind(updated_at)
        .execute(pool)
        .await
        .is_ok()
}

#[test]
fn schema_bounds_vault_preference_keys_values_and_timestamps() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        assert!(insert(&pool, "preferences.language", "\"es\"", NOW).await);
        assert!(
            !insert(&pool, "preferences.language", "\"en\"", NOW).await,
            "keys are unique"
        );
        assert!(insert(&pool, &"k".repeat(128), "true", NOW).await);
        assert!(!insert(&pool, &"k".repeat(129), "true", NOW).await);
        assert!(
            !insert(&pool, &"é".repeat(65), "true", NOW).await,
            "the key bound counts bytes"
        );
        assert!(!insert(&pool, "", "true", NOW).await);
        assert!(!insert(&pool, "profile.color", "not json", NOW).await);
        let limit = format!("\"{}\"", "a".repeat(65_534));
        assert!(insert(&pool, "profile.fullName", &limit, NOW).await);
        let over = format!("\"{}\"", "a".repeat(65_535));
        assert!(!insert(&pool, "profile.displayName", &over, NOW).await);
        assert!(!insert(&pool, "profile.useImage", "true", "2026-10-22").await);
    });
}

use super::helpers::{memory_pool_migrated_through, migrated_memory_pool};
use crate::run_migrations;

const BEFORE_THEMES_REPLICATION: i64 = 20261022173839;

const INSERT_THEME: &str = "INSERT INTO themes
     (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version,
      created_at_ms, updated_at_ms, icon_label, seed_icon_label)
     VALUES (?, 'Forest', '#101010', '#101010', 1, 1, 1, 'dark', 'dark')";

#[test]
fn replication_migration_keeps_the_highest_dismissed_upgrade_on_the_theme() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_THEMES_REPLICATION).await;
        for id in ["dismissed", "untouched"] {
            sqlx::query(INSERT_THEME)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::raw_sql(
            "INSERT INTO theme_upgrade_dismissals (theme_id, engine_version, dismissed_at_ms)
             VALUES ('dismissed', 2, 10), ('dismissed', 4, 20), ('dismissed', 3, 30)",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT id, upgrade_dismissed_engine_version FROM themes ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rows, vec![("dismissed".into(), 4), ("untouched".into(), 0)]);
        let table: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE name = 'theme_upgrade_dismissals'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(table, 0);
    });
}

#[test]
fn theme_seed_cannot_change_outside_sync_apply() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(INSERT_THEME)
            .bind("theme-1")
            .execute(&pool)
            .await
            .unwrap();
        for sql in [
            "UPDATE themes SET seed_blend_canvas = '#202020'",
            "UPDATE themes SET seed_icon_label = 'light'",
            "UPDATE themes SET created_at_ms = 2",
            "UPDATE themes SET upgrade_dismissed_engine_version = -1",
            "UPDATE themes SET display_name = '   '",
        ] {
            assert!(sqlx::query(sql).execute(&pool).await.is_err(), "{sql}");
        }
        sqlx::query("UPDATE themes SET display_name = 'Grove', blend_canvas = '#202020'")
            .execute(&pool)
            .await
            .unwrap();
    });
}

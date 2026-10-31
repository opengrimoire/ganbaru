use super::helpers::migrated_memory_pool;

const INSERT_THEME: &str = "INSERT INTO themes
     (id, display_name, blend_canvas, seed_blend_canvas, derivation_engine_version,
      created_at_ms, updated_at_ms, icon_label, seed_icon_label)
     VALUES (?, 'Forest', '#101010', '#101010', 1, 1, 1, 'dark', 'dark')";

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

use super::helpers::memory_pool_migrated_through;
use crate::run_migrations;

/// Last migration before retained sync versions may elide their value.
const BEFORE_ELIDED_VERSIONS: i64 = 20261022142807;

#[test]
fn version_rebuild_migrations_keep_versions_and_references() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_ELIDED_VERSIONS).await;
        sqlx::raw_sql(
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (1, 'row-a', 'live');
             INSERT INTO sync_register_versions
                 (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
             VALUES (1, 'row-a', 2, zeroblob(16), 3, 7, zeroblob(32), x'01'),
                    (1, 'row-a', 4, zeroblob(16), 3, 7, zeroblob(32), x'02');
             INSERT INTO sync_version_refs (table_id, row_key, group_id, writer_id, field, ref_key)
             VALUES (1, 'row-a', 4, zeroblob(16), 0, 'target-a'),
                    (1, 'row-a', 4, zeroblob(16), 0, 'target-b');",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let versions: Vec<(i64, i64, i64, Option<Vec<u8>>)> = sqlx::query_as(
            "SELECT group_id, seq, clock, value FROM sync_register_versions ORDER BY group_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            versions,
            vec![(2, 3, 7, Some(vec![1])), (4, 3, 7, Some(vec![2]))]
        );
        let references: Vec<String> =
            sqlx::query_scalar("SELECT ref_key FROM sync_version_refs ORDER BY ref_key")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(references, ["target-a", "target-b"]);
        let value_required: i64 = sqlx::query_scalar(
            "SELECT \"notnull\" FROM pragma_table_info('sync_register_versions')
             WHERE name = 'value'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(value_required, 0);
        let parent: String = sqlx::query_scalar(
            "SELECT \"table\" FROM pragma_foreign_key_list('sync_version_refs')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(parent, "sync_register_versions");
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);

        sqlx::query("UPDATE sync_register_versions SET value = NULL WHERE group_id = 4")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM sync_rows")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_version_refs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}

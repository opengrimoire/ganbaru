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

/// Last migration before sync writers record how far compaction reached.
const BEFORE_COMPACTION: i64 = 20261022205249;

#[test]
fn compaction_migration_starts_writers_uncompacted_and_adds_the_stable_vector() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_COMPACTION).await;
        sqlx::raw_sql(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1);
             INSERT INTO sync_writers
                 (space_id, writer_id, public_key, device_id, certificate, state, stored_seq,
                  applied_seq, head_hash, head_clock, created_at_ms)
             VALUES (zeroblob(16), zeroblob(16), zeroblob(32), 'device-a', x'00', 'active', 4,
                     4, zeroblob(32), 9, 1);",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let compacted: i64 = sqlx::query_scalar("SELECT compacted_seq FROM sync_writers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(compacted, 0);
        sqlx::query("UPDATE sync_writers SET compacted_seq = -1")
            .execute(&pool)
            .await
            .expect_err("a negative compacted sequence is refused");

        sqlx::query(
            "INSERT INTO sync_stable (space_id, writer_id, seq)
             VALUES (zeroblob(16), zeroblob(16), 3)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sync_stable (space_id, writer_id, seq)
             VALUES (zeroblob(16), x'01010101010101010101010101010101', 0)",
        )
        .execute(&pool)
        .await
        .expect_err("a stable entry names a stored sequence");
        sqlx::query("DELETE FROM sync_spaces")
            .execute(&pool)
            .await
            .unwrap();
        let stable: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sync_stable")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stable, 0, "the stable vector goes with its space");
    });
}

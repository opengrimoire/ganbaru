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

/// Last migration before the sync authorization log.
const BEFORE_AUTHORIZATION_LOG: i64 = 20261023071334;

#[test]
fn authorization_log_migration_keeps_writers_and_operations_and_constrains_devices() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_AUTHORIZATION_LOG).await;
        sqlx::raw_sql(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1);
             INSERT INTO sync_writers
                 (space_id, writer_id, public_key, device_id, certificate, state, stored_seq,
                  applied_seq, head_hash, head_clock, created_at_ms)
             VALUES (zeroblob(16), zeroblob(16), zeroblob(32), 'device-a', x'00', 'active', 2,
                     2, zeroblob(32), 9, 1);
             INSERT INTO sync_ops
                 (space_id, writer_id, seq, kind, hash, clock, context, envelope, state,
                  stored_at_ms)
             VALUES (zeroblob(16), zeroblob(16), 1, 1, zeroblob(32), 1, x'', x'01', 'applied', 1),
                    (zeroblob(16), zeroblob(16), 2, 4, zeroblob(32), 2, x'', x'02', 'applied', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let ops: Vec<(i64, i64)> =
            sqlx::query_as("SELECT seq, authorization_revision FROM sync_ops ORDER BY seq")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            ops,
            [(1, 0), (2, 0)],
            "the writer rebuild keeps its operations"
        );
        sqlx::query("UPDATE sync_writers SET state = 'revoked', cutoff_seq = 0")
            .execute(&pool)
            .await
            .expect("a removed writer the anchor never stored keeps nothing");

        let entry = "INSERT INTO sync_auth_entries
                (space_id, writer_id, seq, epoch, revision, previous_hash, entry_hash, entry_kind,
                 proposal_hash, selected)
             VALUES (zeroblob(16), zeroblob(16), ?, 0, 1, zeroblob(32), zeroblob(32), 1,
                     zeroblob(32), 1)";
        sqlx::query(entry).bind(2).execute(&pool).await.unwrap();
        sqlx::query(entry)
            .bind(1)
            .execute(&pool)
            .await
            .expect_err("one selected entry per revision");
        sqlx::query(entry)
            .bind(3)
            .execute(&pool)
            .await
            .expect_err("an entry names a stored operation");

        let device = "INSERT INTO sync_devices
                (space_id, device_id, device_key, tls_fingerprint, kind, role, admin_key,
                 enrolled_revision)
             VALUES (zeroblob(16), ?, zeroblob(32), zeroblob(32), ?, ?, ?, 0)";
        let admin_key = Some(vec![0_u8; 32]);
        sqlx::query(device)
            .bind("device-a")
            .bind("desktop")
            .bind("admin")
            .bind(&admin_key)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(device)
            .bind("device-b")
            .bind("mobile")
            .bind("admin")
            .bind(&admin_key)
            .execute(&pool)
            .await
            .expect_err("only desktops can be admins");
        sqlx::query(device)
            .bind("device-c")
            .bind("desktop")
            .bind("member")
            .bind(&admin_key)
            .execute(&pool)
            .await
            .expect_err("only admins have an admin key");
        sqlx::query(device)
            .bind("device-d")
            .bind("desktop")
            .bind("admin")
            .bind(None::<Vec<u8>>)
            .execute(&pool)
            .await
            .expect_err("every admin has an admin key");

        sqlx::query("DELETE FROM sync_spaces")
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM sync_ops) + (SELECT COUNT(*) FROM sync_auth_entries)
                 + (SELECT COUNT(*) FROM sync_devices)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(left, 0, "the log and its devices go with their space");
    });
}

/// Last migration before removal cutoffs exclude operations.
const BEFORE_REMOVAL_EXCLUSION: i64 = 20261023094450;

#[test]
fn removal_exclusion_migration_starts_unreached_and_constrains_resolutions() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_REMOVAL_EXCLUSION).await;
        sqlx::raw_sql(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1);
             INSERT INTO sync_late_changes (removal_revision, table_id, row_key, change)
             VALUES (2, 1, 'row-a', 'created');",
        )
        .execute(&pool)
        .await
        .unwrap();

        run_migrations(&pool).await.unwrap();

        let space: (i64, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT reachable_revision, anchor_device_id, local_device_id FROM sync_spaces",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(space, (0, None, None));
        let resolution: Option<String> =
            sqlx::query_scalar("SELECT resolution FROM sync_late_changes")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(resolution, None, "an existing late change stays unresolved");

        sqlx::query("UPDATE sync_late_changes SET resolution = 'discard', resolved_revision = 3")
            .execute(&pool)
            .await
            .expect("a discard resolves a late change");
        sqlx::query("UPDATE sync_late_changes SET resolution = 'keep'")
            .execute(&pool)
            .await
            .expect_err("a resolution restores or discards");
        sqlx::query("UPDATE sync_spaces SET reachable_revision = -1")
            .execute(&pool)
            .await
            .expect_err("a reachable revision is never negative");
        sqlx::query("UPDATE sync_spaces SET anchor_device_id = ''")
            .execute(&pool)
            .await
            .expect_err("an anchor names a device");
    });
}

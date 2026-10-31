use super::helpers::migrated_memory_pool;

#[test]
fn version_values_may_be_elided_and_references_follow_their_row() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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

#[test]
fn writers_start_uncompacted_and_the_stable_vector_goes_with_its_space() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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

#[test]
fn authorization_log_constrains_entries_envelopes_and_devices() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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

        let ops: Vec<(i64, i64)> =
            sqlx::query_as("SELECT seq, authorization_revision FROM sync_ops ORDER BY seq")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            ops,
            [(1, 0), (2, 0)],
            "operations start before any authorization revision"
        );
        sqlx::query("UPDATE sync_writers SET state = 'revoked', cutoff_seq = 0")
            .execute(&pool)
            .await
            .expect("a removed writer the anchor never stored keeps nothing");

        let selected = "INSERT INTO sync_auth_selected
                (space_id, revision, epoch, entry_hash, proposal_hash, envelope_hash)
             VALUES (zeroblob(16), 1, 0, ?, zeroblob(32), zeroblob(32))";
        sqlx::query(selected)
            .bind(vec![0_u8; 32])
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(selected)
            .bind(vec![1_u8; 32])
            .execute(&pool)
            .await
            .expect_err("one selected entry per revision");
        let envelope = "INSERT INTO sync_auth_envelopes
                (space_id, role, envelope_hash, writer_id, seq, previous_hash, stamp, entry_hash,
                 previous_entry_hash,
                 device_id, device_key, envelope, verified, stored_order, stored_at_ms)
             VALUES (zeroblob(16), ?1, ?2, zeroblob(16), 1, zeroblob(32), 0, ?3, ?3, ?4, ?5, x'01', 1,
                     ?6, 1)";
        sqlx::query(envelope)
            .bind("record")
            .bind(vec![0_u8; 32])
            .bind(Some(vec![0_u8; 32]))
            .bind(None::<String>)
            .bind(None::<Vec<u8>>)
            .bind(1)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(envelope)
            .bind("record")
            .bind(vec![1_u8; 32])
            .bind(None::<Vec<u8>>)
            .bind(None::<String>)
            .bind(None::<Vec<u8>>)
            .bind(2)
            .execute(&pool)
            .await
            .expect_err("every stored record names its entry hash");
        sqlx::query(envelope)
            .bind("certificate")
            .bind(vec![2_u8; 32])
            .bind(None::<Vec<u8>>)
            .bind(None::<String>)
            .bind(None::<Vec<u8>>)
            .bind(3)
            .execute(&pool)
            .await
            .expect_err("every stored certificate names its device and key");
        sqlx::query(envelope)
            .bind("evidence")
            .bind(vec![3_u8; 32])
            .bind(None::<Vec<u8>>)
            .bind(None::<String>)
            .bind(None::<Vec<u8>>)
            .bind(1)
            .execute(&pool)
            .await
            .expect_err("each stored envelope has its own position");

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
            "SELECT (SELECT COUNT(*) FROM sync_ops) + (SELECT COUNT(*) FROM sync_auth_selected)
                 + (SELECT COUNT(*) FROM sync_auth_envelopes)
                 + (SELECT COUNT(*) FROM sync_devices)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(left, 0, "the log and its devices go with their space");
    });
}

#[test]
fn spaces_start_unreached_and_late_change_resolutions_are_constrained() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1);
             INSERT INTO sync_late_changes (removal_revision, table_id, row_key, change)
             VALUES (2, 1, 'row-a', 'created');",
        )
        .execute(&pool)
        .await
        .unwrap();

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
        assert_eq!(resolution, None, "a late change starts unresolved");

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

#[test]
fn fork_evidence_starts_unsettled() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1);
             INSERT INTO sync_fork_evidence
                (space_id, writer_id, seq, first_envelope, second_envelope, received_at_ms)
             VALUES (zeroblob(16), zeroblob(16), 3, x'01', x'02', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();

        let settled: i64 = sqlx::query_scalar("SELECT settled FROM sync_fork_evidence")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            settled, 0,
            "stored evidence settles on the next engine write"
        );
        sqlx::query("UPDATE sync_fork_evidence SET settled = 2")
            .execute(&pool)
            .await
            .expect_err("settlement is a flag");
    });
}

#[test]
fn rows_start_visible_and_creations_cascade_with_their_row() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (1, 'row-a', 'live');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let pending: i64 = sqlx::query_scalar("SELECT creation_pending FROM sync_rows")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(pending, 0, "a row starts visible");
        sqlx::query("UPDATE sync_rows SET creation_pending = 2")
            .execute(&pool)
            .await
            .expect_err("a pending creation is a flag");
        sqlx::query(
            "INSERT INTO sync_row_creations (table_id, row_key, writer_id, seq)
             VALUES (1, 'row-a', zeroblob(16), 1)",
        )
        .execute(&pool)
        .await
        .expect("a published row records its creation");
        sqlx::query(
            "INSERT INTO sync_row_creations (table_id, row_key, writer_id, seq)
             VALUES (1, 'row-b', zeroblob(16), 1)",
        )
        .execute(&pool)
        .await
        .expect_err("a creation names a published row");
        sqlx::query("DELETE FROM sync_rows")
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_row_creations")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "creations go with their row");
    });
}

#[test]
fn stale_rows_and_restated_groups_keep_one_bounded_entry_each() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        let stale = "INSERT INTO sync_stale_rows (table_id, row_key) VALUES (1, 'row-a')";
        sqlx::query(stale).execute(&pool).await.unwrap();
        sqlx::query(stale)
            .execute(&pool)
            .await
            .expect_err("a stale row is listed once");
        sqlx::query("INSERT INTO sync_stale_rows (table_id, row_key) VALUES (0, 'row-b')")
            .execute(&pool)
            .await
            .expect_err("a stale row names a replicated table");

        let restated = "INSERT INTO sync_restated_groups (table_id, row_key, group_id, value_hash)
                        VALUES (1, 'row-a', ?, zeroblob(?))";
        for (group, length, accepted, reason) in [
            (1, 32, true, "a restated group records its value hash"),
            (1, 32, false, "a group is restated once"),
            (64, 32, false, "a restated group is a field group"),
            (2, 16, false, "a restated value hash is a digest"),
        ] {
            let result = sqlx::query(restated)
                .bind(group)
                .bind(length)
                .execute(&pool)
                .await;
            assert_eq!(result.is_ok(), accepted, "{reason}");
        }
    });
}

#[test]
fn released_marks_keep_one_positive_sequence_per_writer_and_go_with_their_space() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let released = "INSERT INTO sync_released_marks (space_id, writer_id, seq)
                        VALUES (zeroblob(16), ?, ?)";
        for (writer, seq, accepted, reason) in [
            (
                vec![1_u8; 16],
                3,
                true,
                "a released mark names a writer and a sequence",
            ),
            (vec![1_u8; 16], 4, false, "a writer has one released mark"),
            (vec![2_u8; 8], 1, false, "a released mark names a writer id"),
            (vec![3_u8; 16], 0, false, "a released sequence is positive"),
        ] {
            let result = sqlx::query(released)
                .bind(writer)
                .bind(seq)
                .execute(&pool)
                .await;
            assert_eq!(result.is_ok(), accepted, "{reason}");
        }
        sqlx::query("DELETE FROM sync_spaces")
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_released_marks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "released marks go with their space");
    });
}

#[test]
fn space_keys_keep_one_bounded_row_per_epoch_and_go_with_their_space() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO sync_spaces (space_id, vault_id, created_at_ms)
             VALUES (zeroblob(16), 'vault-a', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let insert = "INSERT INTO sync_space_keys
                          (space_id, epoch, wrapped_previous, own_envelope, sequenced_revision)
                      VALUES (zeroblob(16), ?1,
                              CASE WHEN ?2 = 0 THEN NULL ELSE zeroblob(?2) END,
                              CASE WHEN ?3 = 0 THEN NULL ELSE zeroblob(?3) END, ?4)";
        for (epoch, link, envelope, revision, accepted, reason) in [
            (1, 0, 96, 4, true, "epoch 1 has no chain link"),
            (1, 0, 0, 5, false, "an epoch has one row"),
            (2, 72, 0, 6, true, "a later epoch links the previous key"),
            (3, 0, 96, 7, false, "a later epoch needs its link"),
            (4, 72, 64, 8, false, "an envelope has its sealed length"),
            (5, 40, 96, 9, false, "a chain link has its sealed length"),
            (0, 0, 96, 10, false, "key epochs start at 1"),
            (6, 72, 96, 0, false, "a key epoch has a sequenced revision"),
        ] {
            let result = sqlx::query(insert)
                .bind(epoch)
                .bind(link)
                .bind(envelope)
                .bind(revision)
                .execute(&pool)
                .await;
            assert_eq!(result.is_ok(), accepted, "{reason}");
        }
        sqlx::query("DELETE FROM sync_spaces")
            .execute(&pool)
            .await
            .unwrap();
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_space_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "space keys go with their space");
    });
}

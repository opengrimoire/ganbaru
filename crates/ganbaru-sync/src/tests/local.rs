//! Installation helpers: space context, writer high-water check, snapshot copies, and
//! carry-forward across a database replacement.

use super::replica::{Replica, VAULT_ID, connection_from, person};
use super::{block_on, migrated_pool};
use crate::local::{
    self, LocalWriterHead, decode_bundle, encode_bundle, has_pending_captures,
    prepare_snapshot_copy, replacement_vector, restore_local_state, space_context,
};
use crate::log::WriterState;
use ganbaru_sync_contracts::{RecoverySeed, VersionVector};
use sqlx::{Connection, SqliteConnection};

async fn add_note(replica: &mut Replica, id: &str, title: &str) {
    replica
        .exec(&format!(
            "INSERT INTO quick_notes (id, title, order_key) VALUES ('{id}', '{title}', 'a0')"
        ))
        .await;
}

#[test]
fn space_context_needs_the_person_identity() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        assert!(
            space_context(&mut replica.conn, VAULT_ID)
                .await
                .unwrap()
                .is_none()
        );
        replica
            .exec(&format!(
                "INSERT INTO contacts_local_identity
                    (singleton, public_key, created_at)
                 VALUES (1, '{}', '2026-01-01T00:00:00Z')",
                person().public_key().to_text()
            ))
            .await;
        let ctx = space_context(&mut replica.conn, VAULT_ID)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ctx, replica.ctx);
    });
}

#[test]
fn writer_head_tracks_the_sealed_chain_and_detects_tampering() {
    block_on(async {
        let mut replica = Replica::new(0, 2).await;
        let key = replica.identity.key.public_key();
        let space = replica.ctx.space;
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(head, LocalWriterHead::Absent);

        add_note(&mut replica, "note-1", "One").await;
        replica.seal().await.unwrap();
        add_note(&mut replica, "note-2", "Two").await;
        let last = replica.seal().await.unwrap().last_seq.unwrap();
        assert!(last >= 2);
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(
            head,
            LocalWriterHead::Genuine {
                stored: last,
                state: WriterState::Active
            }
        );

        replica
            .exec(&format!(
                "UPDATE sync_ops SET envelope = (SELECT envelope FROM sync_ops AS earlier
                    WHERE earlier.writer_id = sync_ops.writer_id AND earlier.seq = {})
                 WHERE seq = {last}",
                last - 1
            ))
            .await;
        let head = replica
            .engine
            .local_writer_head(&mut replica.conn, space, &key)
            .await
            .unwrap();
        assert_eq!(head, LocalWriterHead::Mismatch);
    });
}

#[test]
fn snapshot_copies_drop_unsealed_captures_and_device_state() {
    block_on(async {
        let mut replica = Replica::new(0, 3).await;
        add_note(&mut replica, "note-1", "One").await;
        replica
            .exec(
                "UPDATE quick_notes SET revision = 4 WHERE id = 'note-1';
                 INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
                 UPDATE fixture_items SET label = 'Two' WHERE id = 'item-1';",
            )
            .await;
        assert_eq!(
            replica
                .count("SELECT count(*) FROM fixture_item_marks")
                .await,
            1
        );
        assert!(has_pending_captures(&mut replica.conn).await.unwrap());
        let mut copy = connection_from(&replica.snapshot().await).await;
        prepare_snapshot_copy(&mut copy, replica.engine.manifest())
            .await
            .unwrap();
        assert!(!has_pending_captures(&mut copy).await.unwrap());
        assert!(has_pending_captures(&mut replica.conn).await.unwrap());
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM quick_notes")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(revision, 1);
        let item: (String, i64) = sqlx::query_as("SELECT label, label_edits FROM fixture_items")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(item, ("Two".to_owned(), 0));
        let marks: i64 = sqlx::query_scalar("SELECT count(*) FROM fixture_item_marks")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(marks, 0);
        let indexed: String = sqlx::query_scalar("SELECT label FROM fixture_items_index")
            .fetch_one(&mut copy)
            .await
            .unwrap();
        assert_eq!(indexed, "Two");
        let edits: i64 = sqlx::query_scalar("SELECT label_edits FROM fixture_items")
            .fetch_one(&mut replica.conn)
            .await
            .unwrap();
        assert_eq!(edits, 1);
        let applying: i64 =
            sqlx::query_scalar("SELECT applying FROM sync_apply_state WHERE singleton = 1")
                .fetch_one(&mut copy)
                .await
                .unwrap();
        assert_eq!(applying, 0);
    });
}

#[test]
fn carry_forward_moves_local_operations_into_a_replacement() {
    block_on(async {
        let mut hub = Replica::new(0, 4).await;
        let mut device = Replica::new(1, 4).await;
        add_note(&mut hub, "note-hub", "Hub").await;
        hub.seal().await.unwrap();
        device.pull(&mut hub).await;
        add_note(&mut device, "note-device", "Device").await;
        let device_last = device.seal().await.unwrap().last_seq.unwrap();
        add_note(&mut hub, "note-later", "Later").await;
        hub.seal().await.unwrap();

        // The replacement is the hub's newer snapshot, which lacks the device's operations.
        let mut replacement = connection_from(&hub.snapshot().await).await;
        let staged = replacement_vector(&mut replacement, device.ctx.space)
            .await
            .unwrap();
        let ops = device
            .engine
            .carry_forward_ops(&mut device.conn, &device.ctx, &staged)
            .await
            .unwrap();
        assert_eq!(u64::try_from(ops.len()).unwrap(), device_last);

        let (space, decoded) = decode_bundle(&encode_bundle(device.ctx.space, &ops)).unwrap();
        assert_eq!(space, device.ctx.space);
        assert_eq!(decoded, ops);

        for _ in 0..2 {
            let report = device
                .engine
                .import_carried(
                    &mut replacement,
                    &device.ctx,
                    VAULT_ID,
                    &decoded,
                    device.now_ms,
                )
                .await
                .unwrap();
            assert_eq!(report.refused, 0);
            assert!(!report.pending_captures);
        }
        let titles: Vec<String> = sqlx::query_scalar("SELECT title FROM quick_notes ORDER BY id")
            .fetch_all(&mut replacement)
            .await
            .unwrap();
        assert_eq!(titles, ["Device", "Hub", "Later"]);
        let head = device
            .engine
            .local_writer_head(
                &mut replacement,
                device.ctx.space,
                &device.identity.key.public_key(),
            )
            .await
            .unwrap();
        assert_eq!(
            head,
            LocalWriterHead::Genuine {
                stored: device_last,
                state: WriterState::Active
            }
        );

        // The device keeps sealing on the replacement.
        device.conn = replacement;
        add_note(&mut device, "note-after", "After").await;
        assert_eq!(device.seal().await.unwrap().last_seq, Some(device_last + 1));
        device.check_invariants().await;
    });
}

#[test]
fn carry_forward_from_a_database_without_a_space_is_empty() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut fresh = pool.acquire().await.unwrap();
        let replica = Replica::new(1, 5).await;
        let ops = replica
            .engine
            .carry_forward_ops(&mut fresh, &replica.ctx, &Default::default())
            .await
            .unwrap();
        assert!(ops.is_empty());
        assert!(
            replacement_vector(&mut fresh, replica.ctx.space)
                .await
                .unwrap()
                .iter()
                .next()
                .is_none()
        );
    });
}

#[test]
fn pages_keep_arrival_order_across_writer_chunks() {
    block_on(async {
        let mut hub = Replica::new(0, 6).await;
        let mut device = Replica::new(1, 6).await;
        for index in 0..150 {
            add_note(&mut hub, &format!("hub-{index}"), "Hub").await;
            hub.seal().await.unwrap();
            add_note(&mut device, &format!("device-{index}"), "Device").await;
            device.seal().await.unwrap();
            hub.pull(&mut device).await;
        }
        let arrival: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT envelope FROM sync_ops WHERE space_id = ? ORDER BY rowid")
                .bind(hub.ctx.space.as_bytes().as_slice())
                .fetch_all(&mut hub.conn)
                .await
                .unwrap();
        assert!(arrival.len() >= 300);

        let whole = hub
            .engine
            .ops_after(&mut hub.conn, &hub.ctx, &Default::default(), usize::MAX)
            .await
            .unwrap();
        assert!(!whole.more);
        assert_eq!(whole.ops, arrival);

        let mut known = VersionVector::default();
        let mut paged = Vec::new();
        loop {
            let page = hub
                .engine
                .ops_after(&mut hub.conn, &hub.ctx, &known, 1)
                .await
                .unwrap();
            assert_eq!(page.ops.len(), 1);
            for bytes in &page.ops {
                let envelope = ganbaru_sync_contracts::Envelope::decode(bytes).unwrap();
                known.advance(envelope.writer, envelope.seq);
            }
            paged.extend(page.ops);
            if !page.more {
                break;
            }
        }
        assert_eq!(paged, arrival);
    });
}

#[test]
fn bundles_refuse_truncated_or_foreign_input() {
    let space = ganbaru_sync_contracts::SpaceId::personal(VAULT_ID);
    let bytes = encode_bundle(space, &[vec![1, 2, 3], vec![4]]);
    assert_eq!(
        decode_bundle(&bytes).unwrap(),
        (space, vec![vec![1, 2, 3], vec![4]])
    );
    assert!(decode_bundle(&bytes[..bytes.len() - 1]).is_err());
    assert!(decode_bundle(b"NOTABUNDLE").is_err());
    assert!(
        decode_bundle(&encode_bundle(space, &[]))
            .unwrap()
            .1
            .is_empty()
    );
    let _ = local::MAX_BUNDLE_BYTES;
}

/// A file path under the system temporary directory with a space, `%`, and `#` in its name, so
/// a restore proves the attached URI keeps them part of the file name.
fn temporary_database(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ganbaru live %23 #{name} {}-{nanos}.sqlite",
        std::process::id()
    ))
}

#[test]
fn restore_copies_device_state_onto_carried_rows_and_drops_orphans() {
    block_on(async {
        let mut hub = Replica::new(0, 7).await;
        let mut device = Replica::new(1, 7).await;
        add_note(&mut hub, "note-shared", "Shared").await;
        hub.exec(
            "INSERT INTO fixture_items (id, created_at, label, label_edits)
                 VALUES ('item-gone', '2027-01-01T00:00:00.000Z', 'Gone', 0)",
        )
        .await;
        hub.seal().await.unwrap();
        device.pull(&mut hub).await;

        // Device state: a mark on an item only the carried operations create, a mark on an
        // item the hub deletes, label edits counted twice, and a raised note revision.
        device
            .exec(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-carried', '2027-01-01T00:00:00.000Z', 'One', 0);
                 UPDATE fixture_items SET label = 'Two' WHERE id = 'item-carried';
                 UPDATE fixture_items SET label = 'Three' WHERE id = 'item-carried';",
            )
            .await;
        device.seal().await.unwrap();
        device
            .exec(
                "UPDATE fixture_item_marks SET mark = 5 WHERE item_id = 'item-carried';
                 INSERT INTO fixture_item_marks (item_id, mark) VALUES ('item-gone', 3);
                 UPDATE quick_notes SET revision = 7 WHERE id = 'note-shared';",
            )
            .await;
        assert!(!has_pending_captures(&mut device.conn).await.unwrap());
        hub.exec("DELETE FROM fixture_items WHERE id = 'item-gone'")
            .await;
        hub.exec("UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-shared'")
            .await;
        hub.seal().await.unwrap();

        let mut staged = connection_from(&hub.snapshot().await).await;
        let manifest = device.engine.manifest();
        prepare_snapshot_copy(&mut staged, manifest).await.unwrap();
        let vector = replacement_vector(&mut staged, device.ctx.space)
            .await
            .unwrap();
        let ops = device
            .engine
            .carry_forward_ops(&mut device.conn, &device.ctx, &vector)
            .await
            .unwrap();
        let report = device
            .engine
            .import_carried(&mut staged, &device.ctx, VAULT_ID, &ops, device.now_ms)
            .await
            .unwrap();
        assert_eq!(report.refused, 0);

        let live = temporary_database("restore");
        std::fs::write(&live, device.snapshot().await).unwrap();
        let first = restore_local_state(&mut staged, manifest, &live)
            .await
            .unwrap();
        // Two fixture marks, the Music soundscape and Focus execution states, the Notes history
        // settings and maintenance state, and the execution environment of each of the twelve
        // working folders every vault seeds.
        assert_eq!(first.local_rows, 18);
        assert_eq!(first.repaired, 1);
        // A retry after a failure later in the replacement restores the same rows again.
        let second = restore_local_state(&mut staged, manifest, &live)
            .await
            .unwrap();
        std::fs::remove_file(&live).unwrap();
        assert_eq!(second.local_rows, 18);

        let marks: Vec<(String, i64)> =
            sqlx::query_as("SELECT item_id, mark FROM fixture_item_marks ORDER BY item_id")
                .fetch_all(&mut staged)
                .await
                .unwrap();
        assert_eq!(marks, [("item-carried".to_owned(), 5)]);
        let item: (String, i64) = sqlx::query_as(
            "SELECT label, label_edits FROM fixture_items WHERE id = 'item-carried'",
        )
        .fetch_one(&mut staged)
        .await
        .unwrap();
        assert_eq!(item, ("Three".to_owned(), 2));
        // The revision is one past the larger value after each restore.
        let note: (String, i64) =
            sqlx::query_as("SELECT title, revision FROM quick_notes WHERE id = 'note-shared'")
                .fetch_one(&mut staged)
                .await
                .unwrap();
        assert_eq!(note, ("Edited".to_owned(), 9));
        assert!(!has_pending_captures(&mut staged).await.unwrap());
        let violations: i64 = sqlx::query_scalar("SELECT count(*) FROM pragma_foreign_key_check")
            .fetch_one(&mut staged)
            .await
            .unwrap();
        assert_eq!(violations, 0);
        let schemas: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_database_list")
            .fetch_all(&mut staged)
            .await
            .unwrap();
        assert_eq!(schemas, ["main"]);
        let applying: i64 =
            sqlx::query_scalar("SELECT applying FROM sync_apply_state WHERE singleton = 1")
                .fetch_one(&mut staged)
                .await
                .unwrap();
        assert_eq!(applying, 0);
    });
}

#[test]
fn restore_detaches_and_changes_nothing_when_the_live_database_is_missing() {
    block_on(async {
        let mut replica = Replica::new(0, 8).await;
        let missing = temporary_database("missing");
        let manifest = replica.engine.manifest();
        assert!(
            restore_local_state(&mut replica.conn, manifest, &missing)
                .await
                .is_err()
        );
        assert!(!missing.exists());
        assert_eq!(
            replica
                .count("SELECT count(*) FROM pragma_database_list")
                .await,
            1
        );
        replica.check_invariants().await;
    });
}

#[test]
fn repair_follows_each_declared_delete_action() {
    block_on(async {
        let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             CREATE TABLE parents (id TEXT PRIMARY KEY);
             CREATE TABLE cascaded (
                 id TEXT PRIMARY KEY,
                 parent_id TEXT REFERENCES parents ON DELETE CASCADE
             );
             CREATE TABLE nested (
                 id TEXT PRIMARY KEY,
                 cascaded_id TEXT NOT NULL REFERENCES cascaded(id) ON DELETE CASCADE
             ) WITHOUT ROWID;
             CREATE TABLE nulled (
                 id TEXT PRIMARY KEY,
                 parent_id TEXT REFERENCES parents(id) ON DELETE SET NULL
             ) WITHOUT ROWID;
             INSERT INTO parents VALUES ('kept');
             INSERT INTO cascaded VALUES ('a', 'kept'), ('b', 'gone'), ('c', NULL);
             INSERT INTO nested VALUES ('a1', 'a'), ('b1', 'b'), ('x1', 'missing');
             INSERT INTO nulled VALUES ('n1', 'kept'), ('n2', 'gone');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut conn)
        .await
        .unwrap();

        // `b` and `x1` are dangling and `n2` is nulled; `b1` goes with `b` by the cascade.
        assert_eq!(local::repair_foreign_keys(&mut conn).await.unwrap(), 3);
        let cascaded: Vec<String> = sqlx::query_scalar("SELECT id FROM cascaded ORDER BY id")
            .fetch_all(&mut conn)
            .await
            .unwrap();
        assert_eq!(cascaded, ["a", "c"]);
        let nested: Vec<String> = sqlx::query_scalar("SELECT id FROM nested ORDER BY id")
            .fetch_all(&mut conn)
            .await
            .unwrap();
        assert_eq!(nested, ["a1"]);
        let nulled: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT id, parent_id FROM nulled ORDER BY id")
                .fetch_all(&mut conn)
                .await
                .unwrap();
        assert_eq!(
            nulled,
            [
                ("n1".to_owned(), Some("kept".to_owned())),
                ("n2".to_owned(), None)
            ]
        );

        sqlx::raw_sql(
            "PRAGMA foreign_keys = OFF;
             CREATE TABLE restricted (
                 id TEXT PRIMARY KEY,
                 parent_id TEXT REFERENCES parents(id) ON DELETE RESTRICT
             );
             INSERT INTO restricted VALUES ('r1', 'gone');
             PRAGMA foreign_keys = ON;",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        let refused = local::repair_foreign_keys(&mut conn).await.unwrap_err();
        assert!(refused.to_string().contains("RESTRICT"), "{refused}");
    });
}

#[test]
fn read_only_uris_keep_reserved_characters_in_the_file_name() {
    let uri = local::read_only_uri(std::path::Path::new("/data/a b%c?d#e/\u{fc}.sqlite")).unwrap();
    assert_eq!(uri, "file:/data/a%20b%25c%3Fd%23e/%C3%BC.sqlite?mode=ro");
}

#[test]
fn reset_sync_state_clears_engine_tables_and_the_next_open_republishes_every_row() {
    block_on(async {
        let mut replica = Replica::anchor(0, 31).await;
        add_note(&mut replica, "note-1", "One").await;
        add_note(&mut replica, "note-2", "Two").await;
        replica.seal().await.unwrap();
        assert!(replica.count("SELECT COUNT(*) FROM sync_ops").await > 0);
        let kit = RecoverySeed::from_bytes([0x31; 32]).public_keys().unwrap();
        local::store_recovery_kit_keys(&mut replica.conn, &kit, replica.now_ms)
            .await
            .unwrap();

        local::reset_sync_state(&mut replica.conn).await.unwrap();

        for table in crate::manifest::SYNC_TABLES
            .iter()
            .chain(crate::manifest::SYNC_LOCAL_TABLES)
        {
            let rows = replica
                .count(&format!("SELECT COUNT(*) FROM \"{table}\""))
                .await;
            let expected = i64::from(local::RESET_KEEPS.contains(table));
            assert_eq!(rows, expected, "{table} after the reset");
        }
        assert_eq!(
            local::recovery_kit_keys(&mut replica.conn).await.unwrap(),
            Some(kit),
            "the saved kit founds the next space"
        );
        assert_eq!(
            replica
                .count("SELECT applying FROM sync_apply_state WHERE singleton = 1")
                .await,
            0
        );
        assert_eq!(replica.count("SELECT COUNT(*) FROM quick_notes").await, 2);

        let ctx = replica.ctx.clone();
        let space = replica
            .engine
            .init_space(&mut replica.conn, VAULT_ID, replica.now_ms)
            .await
            .unwrap();
        assert_eq!(space, ctx.space);
        assert!(
            replica
                .engine
                .charter(&mut replica.conn, &ctx)
                .await
                .unwrap()
                .is_none()
        );
        replica
            .engine
            .refuse_legacy_state(&mut replica.conn, &ctx)
            .await
            .unwrap();
        let seeded = replica
            .engine
            .seed_captures(&mut replica.conn, replica.now_ms)
            .await
            .unwrap();
        assert!(seeded.contains(&"quick_notes"), "{seeded:?}");
        assert!(has_pending_captures(&mut replica.conn).await.unwrap());

        let charter = super::replica::test_charter(&replica.person, ctx.space);
        replica
            .engine
            .install_charter(&mut replica.conn, &ctx, &charter)
            .await
            .unwrap();
        replica.rotate();
        replica.seal().await.unwrap();
        assert!(!has_pending_captures(&mut replica.conn).await.unwrap());
        assert_eq!(
            replica
                .count(
                    "SELECT COUNT(DISTINCT row_key) FROM sync_rows
                     WHERE row_key IN ('note-1', 'note-2')"
                )
                .await,
            2,
            "every current note is published again"
        );
    });
}

/// Storing a kit replaces the single row whole, and a snapshot copy carries the kit's public
/// keys to the receiving device.
#[test]
fn the_recovery_kit_keys_are_replaced_whole_and_copied_with_snapshots() {
    block_on(async {
        let mut replica = Replica::anchor(0, 32).await;
        assert_eq!(
            local::recovery_kit_keys(&mut replica.conn).await.unwrap(),
            None
        );
        let first = RecoverySeed::from_bytes([0x41; 32]).public_keys().unwrap();
        let second = RecoverySeed::from_bytes([0x42; 32]).public_keys().unwrap();
        assert_ne!(first, second);
        local::store_recovery_kit_keys(&mut replica.conn, &first, replica.now_ms)
            .await
            .unwrap();
        local::store_recovery_kit_keys(&mut replica.conn, &second, replica.now_ms + 1)
            .await
            .unwrap();
        assert_eq!(
            replica
                .count("SELECT COUNT(*) FROM sync_recovery_kit")
                .await,
            1
        );
        assert_eq!(
            local::recovery_kit_keys(&mut replica.conn).await.unwrap(),
            Some(second)
        );

        let mut copy = connection_from(&replica.snapshot().await).await;
        prepare_snapshot_copy(&mut copy, replica.engine.manifest())
            .await
            .unwrap();
        assert_eq!(
            local::recovery_kit_keys(&mut copy).await.unwrap(),
            Some(second)
        );
    });
}

/// The app rotates once when sealing or acknowledging finds its writer inactive. Captures are
/// not bound to a writer and a refused seal rolls back, so the successor seals every capture
/// taken under the inactive writer and none is orphaned.
#[test]
fn rotating_after_an_inactive_writer_seals_every_pending_capture() {
    block_on(async {
        for state in ["retired", "revoked", "forked"] {
            let mut replica = Replica::anchor(0, 31).await;
            add_note(&mut replica, "note-1", "One").await;
            replica.seal().await.unwrap();
            add_note(&mut replica, "note-2", "Two").await;
            replica
                .exec("UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-1'")
                .await;
            let old = replica.writer();
            let hex: String = old.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
            replica
                .exec(&format!(
                    "UPDATE sync_writers SET state = '{state}', cutoff_seq = 2
                     WHERE writer_id = x'{hex}'"
                ))
                .await;

            let acknowledged = replica
                .engine
                .acknowledge(
                    &mut replica.conn,
                    &replica.ctx,
                    &mut replica.identity.local(),
                    replica.now_ms,
                )
                .await;
            assert!(
                matches!(acknowledged, Err(crate::SyncError::WriterInactive(id)) if id == old),
                "{state}: {acknowledged:?}"
            );
            assert!(
                matches!(
                    replica.seal().await,
                    Err(crate::SyncError::WriterInactive(_))
                ),
                "{state}"
            );
            assert!(
                has_pending_captures(&mut replica.conn).await.unwrap(),
                "{state}: the refused seal keeps its captures"
            );

            replica.rotate();
            replica
                .engine
                .acknowledge(
                    &mut replica.conn,
                    &replica.ctx,
                    &mut replica.identity.local(),
                    replica.now_ms,
                )
                .await
                .unwrap();
            replica.seal().await.unwrap();
            assert!(
                !has_pending_captures(&mut replica.conn).await.unwrap(),
                "{state}"
            );
            let successor = replica.writer();
            let successor_hex: String = successor
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert!(
                replica
                    .count(&format!(
                        "SELECT COUNT(*) FROM sync_ops WHERE writer_id = x'{successor_hex}'"
                    ))
                    .await
                    > 1,
                "{state}: the successor sealed the captures after its genesis"
            );
            assert_eq!(
                replica
                    .count("SELECT COUNT(*) FROM sync_rows WHERE row_key = 'note-2'")
                    .await,
                1,
                "{state}"
            );
        }
    });
}

//! Installation helpers: space context, writer high-water check, snapshot copies, and
//! carry-forward across a database replacement.

use super::calendar::{calendar_sql, event_sql};
use super::focus::open_run_sql;
use super::replica::{Replica, VAULT_ID, connection_from, person};
use super::{block_on, migrated_pool};
use crate::local::{
    self, LocalWriterHead, decode_bundle, encode_bundle, has_pending_captures,
    prepare_snapshot_copy, restore_local_state, space_context, verified_replacement_vector,
};
use crate::log::WriterState;
use crate::manifest::Manifest;
use crate::manifest::vault::VAULT_MANIFEST;
use crate::{COMPACTION_BATCH_OPS, StoreRefusal, SyncError};
use ganbaru_sync_contracts::{RecoverySeed, VersionVector, WriterId};
use sqlx::{AssertSqlSafe, Connection, SqliteConnection};
use std::collections::{BTreeMap, BTreeSet};

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
        let staged = verified_replacement_vector(
            &mut device.conn,
            &mut replacement,
            device.ctx.space,
            &device.device_id,
        )
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
                    &device.device_id,
                    &BTreeSet::new(),
                    &decoded,
                    device.now_ms,
                )
                .await
                .unwrap();
            assert_eq!(report.refused_foreign, 0);
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
        let mut replica = Replica::new(1, 5).await;
        let ops = replica
            .engine
            .carry_forward_ops(&mut fresh, &replica.ctx, &Default::default())
            .await
            .unwrap();
        assert!(ops.is_empty());
        assert!(
            verified_replacement_vector(
                &mut replica.conn,
                &mut fresh,
                replica.ctx.space,
                &replica.device_id
            )
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
        let vector = verified_replacement_vector(
            &mut device.conn,
            &mut staged,
            device.ctx.space,
            &device.device_id,
        )
        .await
        .unwrap();
        let ops = device
            .engine
            .carry_forward_ops(&mut device.conn, &device.ctx, &vector)
            .await
            .unwrap();
        let report = device
            .engine
            .import_carried(
                &mut staged,
                &device.ctx,
                VAULT_ID,
                &device.device_id,
                &BTreeSet::new(),
                &ops,
                device.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.refused_foreign, 0);

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

/// `Local` tables whose rows Chat creates on demand, so a new device starts without them.
const ON_DEMAND_LOCAL_TABLES: &[&str] = &["chat_execution_environments"];

/// Every row of each `Local` table, leaving out the columns a seed takes from the clock.
async fn local_rows(
    conn: &mut SqliteConnection,
    manifest: &Manifest,
) -> BTreeMap<&'static str, Vec<String>> {
    let mut rows = BTreeMap::new();
    for table in manifest.local {
        if ON_DEMAND_LOCAL_TABLES.contains(table) {
            continue;
        }
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info(?)
             WHERE name NOT IN ('updated_at', 'updated_at_ms')",
        )
        .bind(*table)
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        let values: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT json_array({}) FROM {table} ORDER BY 1",
            columns.join(", ")
        )))
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        rows.insert(*table, values);
    }
    rows
}

#[test]
fn a_snapshot_copy_of_a_new_vault_keeps_every_seeded_device_row() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        let seeded = local_rows(&mut conn, &VAULT_MANIFEST).await;
        assert_eq!(seeded["pomodoro_execution_state"].len(), 1);
        assert_eq!(seeded["music_soundscape_state"].len(), 1);
        prepare_snapshot_copy(&mut conn, &VAULT_MANIFEST)
            .await
            .unwrap();
        assert_eq!(local_rows(&mut conn, &VAULT_MANIFEST).await, seeded);
    });
}

#[test]
fn a_snapshot_copy_drops_the_source_values_of_seeded_device_rows() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut conn = pool.acquire().await.unwrap();
        sqlx::raw_sql(
            "UPDATE pomodoro_execution_state SET state_json = '{\"run\":1}', revision = 9;
             UPDATE notes_page_history_settings SET retention_days = 7;",
        )
        .execute(&mut *conn)
        .await
        .unwrap();
        prepare_snapshot_copy(&mut conn, &VAULT_MANIFEST)
            .await
            .unwrap();
        let focus: (String, i64) =
            sqlx::query_as("SELECT state_json, revision FROM pomodoro_execution_state")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(focus, ("{}".to_owned(), 0));
        let retention: i64 =
            sqlx::query_scalar("SELECT retention_days FROM notes_page_history_settings")
                .fetch_one(&mut *conn)
                .await
                .unwrap();
        assert_eq!(retention, 30);
    });
}

#[test]
fn a_restore_keeps_seeded_rows_the_live_database_lacks_and_live_values_it_has() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        let manifest = replica.engine.manifest();
        replica
            .exec(
                "DELETE FROM pomodoro_execution_state;
                 UPDATE notes_page_history_settings SET retention_days = 7;",
            )
            .await;
        let live = temporary_database("seeds");
        std::fs::write(&live, replica.snapshot().await).unwrap();
        let mut staged = connection_from(&replica.snapshot().await).await;
        prepare_snapshot_copy(&mut staged, manifest).await.unwrap();
        restore_local_state(&mut staged, manifest, &live)
            .await
            .unwrap();
        std::fs::remove_file(&live).unwrap();
        let focus: i64 = sqlx::query_scalar("SELECT count(*) FROM pomodoro_execution_state")
            .fetch_one(&mut staged)
            .await
            .unwrap();
        assert_eq!(focus, 1);
        let retention: i64 =
            sqlx::query_scalar("SELECT retention_days FROM notes_page_history_settings")
                .fetch_one(&mut staged)
                .await
                .unwrap();
        assert_eq!(retention, 7);
    });
}

/// Keys of `sql` on `conn`.
async fn keys(conn: &mut SqliteConnection, sql: &str) -> Vec<String> {
    sqlx::query_scalar(AssertSqlSafe(sql))
        .fetch_all(conn)
        .await
        .unwrap()
}

#[test]
fn a_snapshot_copy_leaves_local_drafts_behind() {
    block_on(async {
        let mut replica = Replica::new(0, 2).await;
        let manifest = replica.engine.manifest();
        let focus = format!(
            "{}{}{}",
            calendar_sql(),
            event_sql("event-a", "uid-a", "Deep work"),
            open_run_sql("run-a", "event-a", 3, 4)
        );
        replica.exec(&focus).await;
        replica
            .exec(
                "INSERT INTO fixture_drafts (id, title, published) VALUES
                     ('draft-open', 'Open', 0), ('draft-done', 'Done', 1);
                 INSERT INTO fixture_draft_parts (id, draft_id, body) VALUES
                     ('part-open', 'draft-open', 'body'), ('part-done', 'draft-done', 'body');
                 INSERT INTO fixture_draft_notes (id, draft_id, body) VALUES
                     ('note-open', 'draft-open', 'body'), ('note-free', NULL, 'body');",
            )
            .await;

        let mut copy = connection_from(&replica.snapshot().await).await;
        prepare_snapshot_copy(&mut copy, manifest).await.unwrap();

        for (sql, expected) in [
            ("SELECT id FROM fixture_drafts", vec!["draft-done"]),
            ("SELECT id FROM fixture_draft_parts", vec!["part-done"]),
            ("SELECT id FROM fixture_draft_notes", vec!["note-free"]),
            ("SELECT id FROM pomodoro_runs", Vec::new()),
            ("SELECT id FROM pomodoro_segments", Vec::new()),
            ("SELECT segment_id FROM pomodoro_pauses", Vec::new()),
            ("SELECT id FROM pomodoro_run_events", Vec::new()),
            ("SELECT id FROM calendar_events", vec!["event-a"]),
        ] {
            assert_eq!(keys(&mut copy, sql).await, expected, "{sql}");
        }
        assert_eq!(
            replica.count("SELECT count(*) FROM pomodoro_runs").await,
            1,
            "the source keeps its open run"
        );
    });
}

/// Makes the staged copy `staged` claim `seq` and `hash` as the stored head of `writer`.
async fn claim_staged_head(staged: &mut SqliteConnection, writer: WriterId, seq: u64, hash: &[u8]) {
    sqlx::query("UPDATE sync_writers SET stored_seq = ?, head_hash = ? WHERE writer_id = ?")
        .bind(i64::try_from(seq).unwrap())
        .bind(hash)
        .bind(writer.as_bytes().as_slice())
        .execute(&mut *staged)
        .await
        .unwrap();
}

/// The operations of `device` a replacement whose vector is `vector` lacks.
async fn carried(device: &mut Replica, vector: &VersionVector) -> Vec<Vec<u8>> {
    device
        .engine
        .carry_forward_ops(&mut device.conn, &device.ctx, vector)
        .await
        .unwrap()
}

#[test]
fn inflated_staged_head_is_refused() {
    block_on(async {
        let mut hub = Replica::new(0, 40).await;
        let mut device = Replica::new(1, 40).await;
        add_note(&mut device, "note-sent", "Sent").await;
        let sent = device.seal().await.unwrap().last_seq.unwrap();
        hub.pull(&mut device).await;
        add_note(&mut device, "note-unsent", "Unsent").await;
        let last = device.seal().await.unwrap().last_seq.unwrap();
        assert!(last > sent);
        let writer = device.writer();
        let space = device.ctx.space;

        // The sender claims this device's unsent head, or a head past it, so nothing carries.
        for claimed in [last, last + 3] {
            let mut staged = connection_from(&hub.snapshot().await).await;
            claim_staged_head(&mut staged, writer, claimed, &[7; 32]).await;
            let result = verified_replacement_vector(
                &mut device.conn,
                &mut staged,
                space,
                &device.device_id,
            )
            .await;
            assert!(
                matches!(
                    result,
                    Err(SyncError::StagedHead { writer: refused, seq })
                        if refused == writer && seq == claimed
                ),
                "claim {claimed}: {result:?}"
            );
        }

        let mut staged = connection_from(&hub.snapshot().await).await;
        let vector =
            verified_replacement_vector(&mut device.conn, &mut staged, space, &device.device_id)
                .await
                .unwrap();
        assert_eq!(vector.get(&writer), sent);
        let ops = carried(&mut device, &vector).await;
        assert_eq!(u64::try_from(ops.len()).unwrap(), last - sent);
    });
}

#[test]
fn compacted_staged_log_with_real_heads_is_accepted() {
    block_on(async {
        let mut hub = Replica::new(0, 41).await;
        let mut device = Replica::new(1, 41).await;
        add_note(&mut hub, "hub-1", "One").await;
        for title in ["Two", "Three"] {
            hub.exec(&format!(
                "UPDATE quick_notes SET title = '{title}' WHERE id = 'hub-1'"
            ))
            .await;
            hub.seal().await.unwrap();
        }
        add_note(&mut device, "device-1", "Device").await;
        device.seal().await.unwrap();
        hub.pull(&mut device).await;
        device.pull(&mut hub).await;
        let stable = hub.stored_vector().await;
        let report = hub.compact(&stable, COMPACTION_BATCH_OPS).await.unwrap();
        assert!(report.compacted > 0);
        let sent = device.stored_vector().await.get(&device.writer());

        // The staged head of the hub's writer is past what this device holds, and this device
        // has unsent work.
        add_note(&mut hub, "hub-late", "Late").await;
        hub.seal().await.unwrap();
        add_note(&mut device, "device-late", "Late").await;
        let last = device.seal().await.unwrap().last_seq.unwrap();

        let mut staged = connection_from(&hub.snapshot().await).await;
        let vector = verified_replacement_vector(
            &mut device.conn,
            &mut staged,
            device.ctx.space,
            &device.device_id,
        )
        .await
        .unwrap();
        let hub_stored = hub.stored_vector().await;
        assert_eq!(vector.get(&hub.writer()), hub_stored.get(&hub.writer()));
        assert_eq!(vector.get(&device.writer()), sent);
        let ops = carried(&mut device, &vector).await;
        assert_eq!(u64::try_from(ops.len()).unwrap(), last - sent);
    });
}

#[test]
fn staged_foreign_fork_does_not_block_the_copy() {
    block_on(async {
        let mut hub = Replica::new(0, 42).await;
        let mut device = Replica::new(1, 42).await;
        add_note(&mut hub, "hub-1", "One").await;
        hub.seal().await.unwrap();
        add_note(&mut hub, "hub-2", "Two").await;
        let head = hub.seal().await.unwrap().last_seq.unwrap();
        device.pull(&mut hub).await;

        // The staged log of the hub's writer holds another operation at a sequence this device
        // also holds.
        let mut staged = connection_from(&hub.snapshot().await).await;
        claim_staged_head(&mut staged, hub.writer(), head, &[9; 32]).await;
        let vector = verified_replacement_vector(
            &mut device.conn,
            &mut staged,
            device.ctx.space,
            &device.device_id,
        )
        .await
        .unwrap();
        assert_eq!(vector.get(&hub.writer()), head);
    });
}

#[test]
fn foreign_fork_in_a_carried_chunk_loses_nothing_behind_it() {
    block_on(async {
        let mut hub = Replica::new(0, 43).await;
        let mut device = Replica::new(1, 43).await;
        add_note(&mut hub, "hub-1", "One").await;
        hub.seal().await.unwrap();
        let known = hub.stored_vector().await;
        let mut clone = hub.clone_device().await;
        add_note(&mut hub, "hub-2", "Kept").await;
        hub.seal().await.unwrap();
        add_note(&mut clone, "hub-2", "Forked").await;
        clone.seal().await.unwrap();
        let fork = clone.ops_since(&known).await;
        assert_eq!(fork.len(), 1);

        add_note(&mut device, "device-1", "Device one").await;
        device.seal().await.unwrap();
        add_note(&mut device, "device-2", "Device two").await;
        device.seal().await.unwrap();
        let own = device.own_ops().await;
        assert!(own.len() >= 2);

        let mut staged = connection_from(&hub.snapshot().await).await;
        let ops: Vec<Vec<u8>> = fork.iter().chain(&own).cloned().collect();
        let report = device
            .engine
            .import_carried(
                &mut staged,
                &device.ctx,
                VAULT_ID,
                &device.device_id,
                &BTreeSet::new(),
                &ops,
                device.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.refused_foreign, 1);
        assert_eq!(report.stored, own.len());
        let titles: Vec<String> = sqlx::query_scalar(
            "SELECT title FROM quick_notes WHERE id LIKE 'device-%' ORDER BY id",
        )
        .fetch_all(&mut staged)
        .await
        .unwrap();
        assert_eq!(titles, ["Device one", "Device two"]);
    });
}

#[test]
fn relabeled_own_writer_in_the_copy_is_refused() {
    block_on(async {
        let mut hub = Replica::new(0, 45).await;
        let mut device = Replica::new(1, 45).await;
        let mut other = Replica::new(2, 45).await;
        add_note(&mut other, "other-1", "Other").await;
        other.seal().await.unwrap();
        let other_key: Vec<u8> =
            sqlx::query_scalar("SELECT public_key FROM sync_writers WHERE writer_id = ?")
                .bind(other.writer().as_bytes().as_slice())
                .fetch_one(&mut other.conn)
                .await
                .unwrap();
        add_note(&mut device, "note-sent", "Sent").await;
        let sent = device.seal().await.unwrap().last_seq.unwrap();
        hub.pull(&mut device).await;
        add_note(&mut device, "note-unsent", "Unsent").await;
        device.seal().await.unwrap();
        let writer = device.writer();

        // The copy keeps the genuine head of this device's writer but names another device and
        // key for it.
        let mut staged = connection_from(&hub.snapshot().await).await;
        sqlx::query(
            "UPDATE sync_writers SET device_id = 'other-device', public_key = ? WHERE writer_id = ?",
        )
        .bind(&other_key)
        .bind(writer.as_bytes().as_slice())
        .execute(&mut staged)
        .await
        .unwrap();
        let result = verified_replacement_vector(
            &mut device.conn,
            &mut staged,
            device.ctx.space,
            &device.device_id,
        )
        .await;
        assert!(
            matches!(
                result,
                Err(SyncError::StagedHead { writer: refused, seq })
                    if refused == writer && seq == sent
            ),
            "{result:?}"
        );

        // The writers the live log certifies to this device stay its own in the import, so a
        // refusal of the unsent operations fails it.
        let mut vector = VersionVector::default();
        vector.set(writer, sent);
        let ops = carried(&mut device, &vector).await;
        assert!(!ops.is_empty());
        let result = device
            .engine
            .import_carried(
                &mut staged,
                &device.ctx,
                VAULT_ID,
                &device.device_id,
                &BTreeSet::from([writer]),
                &ops,
                device.now_ms,
            )
            .await;
        assert!(
            matches!(
                result,
                Err(SyncError::CarriedOwnRefused { writer: refused, .. }) if refused == writer
            ),
            "{result:?}"
        );
    });
}

#[test]
fn rotated_own_writer_counts_as_own_without_a_target_row() {
    block_on(async {
        let mut hub = Replica::new(0, 44).await;
        let mut device = Replica::new(1, 44).await;
        add_note(&mut device, "device-1", "One").await;
        device.seal().await.unwrap();
        hub.pull(&mut device).await;
        device.rotate();
        add_note(&mut device, "device-2", "Two").await;
        device.seal().await.unwrap();
        let rotated = device.writer();

        // The copy holds another key for this device, so it refuses the new writer's genesis.
        let mut staged = connection_from(&hub.snapshot().await).await;
        sqlx::query("UPDATE sync_devices SET device_key = zeroblob(32) WHERE device_id = ?")
            .bind(&device.device_id)
            .execute(&mut staged)
            .await
            .unwrap();
        let vector = verified_replacement_vector(
            &mut device.conn,
            &mut staged,
            device.ctx.space,
            &device.device_id,
        )
        .await
        .unwrap();
        assert_eq!(vector.get(&rotated), 0);
        let ops = carried(&mut device, &vector).await;
        assert!(!ops.is_empty());
        let result = device
            .engine
            .import_carried(
                &mut staged,
                &device.ctx,
                VAULT_ID,
                &device.device_id,
                &BTreeSet::new(),
                &ops,
                device.now_ms,
            )
            .await;
        assert!(
            matches!(
                result,
                Err(SyncError::CarriedOwnRefused {
                    writer,
                    seq: 1,
                    refusal: StoreRefusal::Untrusted,
                }) if writer == rotated
            ),
            "{result:?}"
        );
    });
}

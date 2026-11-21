//! Recovery offers for edits that outlived a deletion: how they are listed, restored,
//! discarded, and closed on every replica.

use ganbaru_sync_contracts::{Field, TableId};

use super::block_on;
use super::engine::{
    MEMBERSHIP_PARENTS, add_membership, add_node, assert_converged, close_recovery,
    membership_rows, memberships, node_parents, replicas, seed_note, title,
};
use super::fixture::{ITEMS_TABLE, NODES_TABLE, membership_key};
use super::replica::{Replica, converge, pair};
use crate::SyncError;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, note_group};

async fn deleted_while_edited() -> Vec<Replica> {
    let mut replicas = replicas(2).await;
    seed_note(&mut replicas[0]).await;
    converge(&mut replicas).await;
    replicas[0]
        .exec("DELETE FROM quick_notes WHERE id = 'note-1'")
        .await;
    replicas[1]
        .exec("UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-1'")
        .await;
    converge(&mut replicas).await;
    assert_converged(&mut replicas).await;
    for replica in &mut replicas {
        assert_eq!(replica.count("SELECT count(*) FROM quick_notes").await, 0);
        let entries = replica
            .engine
            .recovery_entries(&mut replica.conn, &replica.ctx)
            .await
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].table, NOTES_TABLE);
        assert_eq!(entries[0].row_key, "note-1");
        assert_eq!(entries[0].presentation.title, "Edited");
        assert_eq!(entries[0].device_id, "device-1");
        let status = replica
            .engine
            .status(&mut replica.conn, &replica.ctx)
            .await
            .unwrap();
        assert_eq!(status.recovery, vec![("quick_notes", 1)]);
    }
    replicas
}

async fn assert_no_recovery(replicas: &mut [Replica]) {
    for replica in replicas {
        assert!(
            replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            replica
                .engine
                .recovery_values(&mut replica.conn, NOTES_TABLE, "note-1")
                .await
                .unwrap(),
            None
        );
    }
}

#[test]
fn restoring_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere() {
    block_on(async {
        let mut replicas = deleted_while_edited().await;
        let restorer = &mut replicas[0];
        let values = restorer
            .engine
            .recovery_values(&mut restorer.conn, NOTES_TABLE, "note-1")
            .await
            .unwrap()
            .unwrap();
        let restored = values
            .iter()
            .find(|(group, _)| *group == note_group::TITLE)
            .map(|(_, value)| match value.fields() {
                [Field::Text(text)] => text.clone(),
                other => panic!("unexpected title value {other:?}"),
            })
            .unwrap();
        sqlx::query("INSERT INTO quick_notes (id, title, order_key) VALUES ('note-2', ?, 'a0')")
            .bind(&restored)
            .execute(&mut restorer.conn)
            .await
            .unwrap();
        close_recovery(restorer, "note-1").await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_no_recovery(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(title(replica, "note-2").await.as_deref(), Some("Edited"));
        }
    });
}

#[test]
fn discarding_an_edit_that_outlived_a_deletion_closes_the_offer_everywhere() {
    block_on(async {
        let mut replicas = deleted_while_edited().await;
        close_recovery(&mut replicas[1], "note-1").await.unwrap();
        assert!(matches!(
            close_recovery(&mut replicas[1], "note-1").await,
            Err(SyncError::InvalidRequest(_))
        ));
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_no_recovery(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM quick_notes").await, 0);
        }
    });
}

async fn close_group(replica: &mut Replica, table: TableId, key: &str) {
    replica
        .engine
        .close_recovery(
            &mut replica.conn,
            &replica.ctx,
            table,
            key,
            &mut replica.identity.local(),
            replica.now_ms,
        )
        .await
        .unwrap();
}

/// The single recovery entry of a replica, as table, key, descendants, and device of the
/// newest unseen edit.
async fn single_entry(replica: &mut Replica) -> (TableId, String, usize, String) {
    let entries = replica
        .engine
        .recovery_entries(&mut replica.conn, &replica.ctx)
        .await
        .unwrap();
    assert_eq!(entries.len(), 1, "entries on replica {}", replica.index);
    let entry = entries.into_iter().next().unwrap();
    let status = replica
        .engine
        .status(&mut replica.conn, &replica.ctx)
        .await
        .unwrap();
    let root = replica.engine.manifest().table(entry.table).unwrap().name;
    assert_eq!(status.recovery, vec![(root, 1)], "one offer per root");
    (
        entry.table,
        entry.row_key,
        entry.descendants,
        entry.device_id,
    )
}

async fn descendant_keys(replica: &mut Replica, table: TableId, key: &str) -> Vec<String> {
    replica
        .engine
        .recovery_descendants(&mut replica.conn, table, key)
        .await
        .unwrap()
        .unwrap()
        .into_iter()
        .map(|descendant| descendant.row_key)
        .collect()
}

#[test]
fn a_child_edited_while_its_parent_is_deleted_is_offered_under_the_parent() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                add_node("n-1", None, 0, "root"),
                add_node("n-2", Some("n-1"), 0, "child"),
                add_node("n-3", Some("n-2"), 0, "grandchild"),
            ))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_nodes WHERE id = 'n-1'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_nodes SET label = 'edited' WHERE id = 'n-3'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM fixture_nodes").await, 0);
            assert_eq!(
                single_entry(replica).await,
                (NODES_TABLE, "n-1".to_string(), 2, "device-1".to_string())
            );
            assert_eq!(
                descendant_keys(replica, NODES_TABLE, "n-1").await,
                ["n-2", "n-3"]
            );
            // A descendant is offered only through its root.
            assert_eq!(
                replica
                    .engine
                    .recovery_values(&mut replica.conn, NODES_TABLE, "n-3")
                    .await
                    .unwrap(),
                None
            );
        }
        close_group(&mut replicas[0], NODES_TABLE, "n-1").await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_no_recovery(&mut replicas).await;
    });
}

/// Stores what `from` has and `to` lacks, then applies on `to` without a local writer, so no
/// repair is sealed and rows hidden by cascade stay live.
pub(super) async fn pull_without_writer(to: &mut Replica, from: &mut Replica) {
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
    loop {
        let report = to
            .engine
            .apply(&mut to.conn, &to.ctx, None, to.now_ms)
            .await
            .unwrap();
        if !report.more {
            return;
        }
    }
}

#[test]
fn a_root_stops_being_flagged_when_its_edited_hidden_child_is_shown_again() {
    block_on(async {
        let mut replicas = replicas(4).await;
        replicas[0]
            .exec(&format!(
                "{}{}",
                add_node("n-1", None, 0, "root"),
                add_node("n-2", None, 1, "other"),
            ))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_nodes WHERE id = 'n-1'")
            .await;
        replicas[0].seal().await.unwrap();
        replicas[1]
            .exec("UPDATE fixture_nodes SET parent_id = 'n-1', label = 'edited' WHERE id = 'n-2'")
            .await;
        replicas[1].seal().await.unwrap();
        // Replica 2 sees the move and the edit but not the deletion, and moves the row out.
        let (mover, editor) = pair(&mut replicas, 2, 1);
        mover.pull(editor).await;
        mover
            .exec("UPDATE fixture_nodes SET parent_id = NULL WHERE id = 'n-2'")
            .await;
        mover.seal().await.unwrap();

        let (observer, deleter) = pair(&mut replicas, 3, 0);
        pull_without_writer(observer, deleter).await;
        let (observer, editor) = pair(&mut replicas, 3, 1);
        pull_without_writer(observer, editor).await;
        let observer = &mut replicas[3];
        assert!(node_parents(observer).await.is_empty());
        assert_eq!(
            single_entry(observer).await,
            (NODES_TABLE, "n-1".to_string(), 1, "device-1".to_string())
        );

        let (observer, mover) = pair(&mut replicas, 3, 2);
        pull_without_writer(observer, mover).await;
        let observer = &mut replicas[3];
        assert_eq!(node_parents(observer).await, ["n-2>-"]);
        assert_eq!(
            observer
                .count("SELECT count(*) FROM sync_rows WHERE recovery = 1")
                .await,
            0
        );
        assert!(
            observer
                .engine
                .recovery_entries(&mut observer.conn, &observer.ctx)
                .await
                .unwrap()
                .is_empty()
        );
        observer.check_invariants().await;

        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_hidden_derived_key_child_is_offered_with_its_deleted_parent() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(MEMBERSHIP_PARENTS).await;
        replicas[0]
            .exec(&add_membership("item-1", "a-1", "member"))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_items WHERE id = 'item-1'")
            .await;
        replicas[1]
            .exec(&format!(
                "UPDATE fixture_memberships SET role = 'lead' WHERE sync_key = '{}'",
                membership_key("item-1", "a-1")
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let key = membership_key("item-1", "a-1");
        for replica in &mut replicas {
            assert!(memberships(replica).await.is_empty());
            assert_eq!(
                single_entry(replica).await,
                (ITEMS_TABLE, "item-1".to_string(), 1, "device-1".to_string())
            );
            let descendants = replica
                .engine
                .recovery_descendants(&mut replica.conn, ITEMS_TABLE, "item-1")
                .await
                .unwrap()
                .unwrap();
            assert_eq!(descendants.len(), 1);
            assert_eq!(descendants[0].row_key, key);
            assert!(descendants[0].values.iter().any(|(group, value)| {
                *group == super::fixture::membership_group::DETAILS
                    && value.fields().get(1) == Some(&Field::Text("lead".to_string()))
            }));
        }
        close_group(&mut replicas[1], ITEMS_TABLE, "item-1").await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_no_recovery(&mut replicas).await;
        for replica in &mut replicas {
            // Closing removes the hidden child instead of tombstoning it.
            assert_eq!(
                membership_rows(
                    replica,
                    "SELECT count(*) FROM sync_rows WHERE table_id = {table} AND state = 'live'"
                )
                .await,
                1
            );
            assert_eq!(
                membership_rows(
                    replica,
                    "SELECT count(*) FROM sync_tombstones WHERE table_id = {table}"
                )
                .await,
                0
            );
        }
    });
}

//! Duplicate classes and delete cascades across replicas: collision repair, reference
//! redirects, the class index, and cascade causes.

use std::collections::BTreeSet;

use ganbaru_sync_contracts::TableId;

use super::block_on;
use super::engine::{
    MEMBERSHIP_PARENTS, add_membership, assert_converged, membership_rows, memberships, replicas,
    texts,
};
use super::exclusion::{deliver, edit, restore_all};
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, HANDLES_TABLE, ITEMS_TABLE, LINKS_TABLE, NOCASE_LABEL_MANIFEST,
    REORDERED_RULES_MANIFEST, test_engine, test_engine_of,
};
use super::replica::{Replica, converge, pair};
use crate::repair;

#[test]
fn duplicate_tag_names_keep_the_lower_key_and_redirect_references() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-a', 'Work', 'a0')",
            )
            .await;
        replicas[1]
            .exec(
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-b', 'work', 'a1');
                 INSERT INTO quick_notes (id, title, tag_id, order_key)
                     VALUES ('note-1', 'One', 'tag-b', 'a0');",
            )
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica.count("SELECT count(*) FROM quick_note_tags").await,
                1
            );
            assert_eq!(
                replica
                    .scalar_text("SELECT id FROM quick_note_tags")
                    .await
                    .as_deref(),
                Some("tag-a")
            );
            assert_eq!(
                replica
                    .scalar_text("SELECT tag_id FROM quick_notes WHERE id = 'note-1'")
                    .await
                    .as_deref(),
                Some("tag-a")
            );
            assert_eq!(
                replica
                    .scalar_text(
                        "SELECT replaced_by FROM sync_tombstones
                         WHERE table_id = 1 AND row_key = 'tag-b' AND replaced_by IS NOT NULL",
                    )
                    .await
                    .as_deref(),
                Some("tag-a")
            );
        }
    });
}

#[test]
fn deleting_a_tag_clears_concurrent_assignments() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
                 INSERT INTO quick_notes (id, title, order_key) VALUES ('note-1', 'One', 'a0');",
            )
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec(
                "UPDATE quick_notes SET revision = revision + 1 WHERE tag_id = 'tag-1';
                 DELETE FROM quick_note_tags WHERE id = 'tag-1';",
            )
            .await;
        replicas[1]
            .exec("UPDATE quick_notes SET tag_id = 'tag-1' WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica.count("SELECT count(*) FROM quick_note_tags").await,
                0
            );
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM quick_notes WHERE tag_id IS NOT NULL")
                    .await,
                0
            );
        }
    });
}

#[test]
fn a_cascade_reference_cycle_materializes_in_one_component() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'A');
                 INSERT INTO fixture_cycle_b (id, label, peer_id) VALUES ('b-1', 'B', 'a-1');
                 UPDATE fixture_cycle_a SET peer_id = 'b-1' WHERE id = 'a-1';
                 INSERT INTO fixture_links (id, note, left_id, right_id)
                     VALUES ('link-1', 'ends', 'a-1', 'b-1');",
            )
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || '>' || peer_id FROM fixture_cycle_a
                     UNION ALL SELECT id || '>' || peer_id FROM fixture_cycle_b"
                )
                .await,
                ["a-1>b-1", "b-1>a-1"]
            );
        }

        replicas[1]
            .exec("DELETE FROM fixture_cycle_a WHERE id = 'a-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            for table in ["fixture_cycle_a", "fixture_cycle_b"] {
                assert_eq!(
                    replica
                        .count(&format!("SELECT count(*) FROM {table}"))
                        .await,
                    0,
                    "{table} on replica {}",
                    replica.index
                );
            }
            assert_eq!(
                texts(
                    replica,
                    "SELECT quote(left_id) || quote(right_id) FROM fixture_links"
                )
                .await,
                ["NULLNULL"]
            );
            // The cycle deleted together has one root, its lowest row, named by the other.
            assert_eq!(
                texts(
                    replica,
                    "SELECT row_key || ':' || quote(cause_row) FROM sync_tombstones
                     WHERE table_id IN (60001, 60002) ORDER BY row_key"
                )
                .await,
                ["a-1:NULL", "b-1:'a-1'"]
            );
        }
    });
}

async fn tombstone_causes(replica: &mut Replica, table: TableId) -> Vec<String> {
    texts(
        replica,
        &format!(
            "SELECT DISTINCT row_key || ':' || quote(cause_row) FROM sync_tombstones
             WHERE table_id = {} ORDER BY 1",
            table.0
        ),
    )
    .await
}

#[test]
fn duplicates_redirect_every_reference_column_of_a_group() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'Same')")
            .await;
        replicas[1]
            .exec(
                "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-2', 'Same');
                 INSERT INTO fixture_cycle_b (id, label, peer_id) VALUES ('b-1', 'B', 'a-2');
                 INSERT INTO fixture_links (id, note, left_id, right_id)
                     VALUES ('link-1', 'ends', 'a-2', 'b-1');
                 INSERT INTO fixture_links (id, note, left_id, parent_id)
                     VALUES ('link-2', 'child', 'a-2', 'link-1');",
            )
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ':' || quote(left_id) || quote(right_id) || quote(parent_id)
                     FROM fixture_links ORDER BY id"
                )
                .await,
                ["link-1:'a-1''b-1'NULL", "link-2:'a-1'NULL'link-1'"]
            );
            assert_eq!(
                texts(replica, "SELECT peer_id FROM fixture_cycle_b").await,
                ["a-1"]
            );
        }

        replicas[0]
            .exec("DELETE FROM fixture_links WHERE id = 'link-1'")
            .await;
        replicas[2]
            .exec("UPDATE fixture_links SET note = 'edited' WHERE id = 'link-2'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(
                    replica,
                    "SELECT id || ':' || note || ':' || quote(parent_id) FROM fixture_links"
                )
                .await,
                ["link-2:edited:NULL"]
            );
        }
    });
}

/// Redirect targets of the tombstones of a table, as `row:target`.
async fn redirects(replica: &mut Replica, table: TableId) -> Vec<String> {
    texts(
        replica,
        &format!(
            "SELECT DISTINCT row_key || ':' || replaced_by FROM sync_tombstones
             WHERE table_id = {} AND replaced_by IS NOT NULL ORDER BY 1",
            table.0
        ),
    )
    .await
}

/// The replica that holds the surviving duplicate learns in one pass that the lower key is
/// hidden by cascade, so that row must not keep the class.
#[test]
fn a_duplicate_hidden_by_cascade_does_not_hide_the_row_it_collides_with() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_cycle_b (id, label) VALUES ('b-1', 'B');
                 INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'Same');",
            )
            .await;
        replicas[0].seal_rotating().await.unwrap();
        let (to, from) = pair(&mut replicas, 1, 0);
        to.pull(from).await;
        replicas[0]
            .exec("DELETE FROM fixture_cycle_b WHERE id = 'b-1'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_cycle_a SET peer_id = 'b-1' WHERE id = 'a-1'")
            .await;
        replicas[2]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-2', 'Same')")
            .await;
        for replica in &mut replicas {
            replica.seal_rotating().await.unwrap();
        }
        for source in [0, 1] {
            let (to, from) = pair(&mut replicas, 2, source);
            let known = to.stored_vector().await;
            let ops = from.ops_since(&known).await;
            to.receive(&ops).await;
        }
        replicas[2].apply_all().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM fixture_cycle_a ORDER BY id").await,
                ["a-2"]
            );
            assert_eq!(
                tombstone_causes(replica, CYCLE_A_TABLE).await,
                ["a-1:'b-1'"]
            );
            assert_eq!(
                redirects(replica, CYCLE_A_TABLE).await,
                Vec::<String>::new()
            );
        }
    });
}

/// Stores what `from` has on `to` without applying it.
async fn store_from(to: &mut Replica, from: &mut Replica) {
    let known = to.stored_vector().await;
    let ops = from.ops_since(&known).await;
    to.receive(&ops).await;
}

/// Applies everything ready the way a replica without an active writer does, so collisions stay
/// unrepaired.
async fn apply_without_writer(replica: &mut Replica) {
    loop {
        let report = replica
            .engine
            .apply(&mut replica.conn, &replica.ctx, None, replica.now_ms)
            .await
            .unwrap();
        if !report.more {
            return;
        }
    }
}

/// Every version of every row but the dirty one is undecodable, so any read of those rows
/// outside the contested classes fails.
#[test]
fn contested_reads_no_row_outside_the_contested_classes() {
    const BYSTANDERS: usize = 32;
    block_on(async {
        let mut replicas = replicas(3).await;
        let bystanders: String = (0..BYSTANDERS)
            .map(|index| {
                format!("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-{index}', 'L{index}');")
            })
            .collect();
        replicas[0].exec(&bystanders).await;
        replicas[0]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('x-1', 'Same')")
            .await;
        replicas[1]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('x-2', 'Same')")
            .await;
        for source in [0, 1] {
            replicas[source].seal_rotating().await.unwrap();
            let (to, from) = pair(&mut replicas, 2, source);
            store_from(to, from).await;
        }
        let observer = &mut replicas[2];
        apply_without_writer(observer).await;
        observer
            .exec(&format!(
                "UPDATE sync_register_versions SET value = x'ff'
                 WHERE table_id = {} AND row_key <> 'a-0'",
                CYCLE_A_TABLE.0
            ))
            .await;

        let dirty_row = (CYCLE_A_TABLE, "a-0".to_owned());
        let mut dirty = BTreeSet::from([dirty_row.clone()]);
        let result = repair::contested(&observer.engine, &mut observer.conn, &mut dirty).await;

        assert!(
            result.is_ok(),
            "the scan reached a row outside the contested classes: {result:?}"
        );
        assert_eq!(
            result.unwrap().get(&CYCLE_A_TABLE),
            Some(&vec![BTreeSet::from(["x-1".to_owned(), "x-2".to_owned()])])
        );
        assert_eq!(dirty, BTreeSet::from([dirty_row]));
    });
}

/// `a-1` keeps its class until a delete of the row it references reaches an observer, which then
/// shows `a-2` although no write named `a-2` and the class only went dirty after it was read.
#[test]
fn keeper_changes_when_a_row_elsewhere_is_published() {
    block_on(async {
        let mut replicas = replicas(4).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_cycle_b (id, label) VALUES ('b-1', 'B');
                 INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'Same');",
            )
            .await;
        replicas[0].seal_rotating().await.unwrap();
        let (to, from) = pair(&mut replicas, 1, 0);
        to.pull(from).await;
        replicas[1]
            .exec("UPDATE fixture_cycle_a SET peer_id = 'b-1' WHERE id = 'a-1'")
            .await;
        replicas[3]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-2', 'Same')")
            .await;
        for source in [0, 1, 3] {
            replicas[source].seal_rotating().await.unwrap();
            let (to, from) = pair(&mut replicas, 2, source);
            store_from(to, from).await;
        }
        apply_without_writer(&mut replicas[2]).await;
        assert_eq!(
            texts(
                &mut replicas[2],
                "SELECT id FROM fixture_cycle_a ORDER BY id"
            )
            .await,
            ["a-1"]
        );

        replicas[0]
            .exec("DELETE FROM fixture_cycle_b WHERE id = 'b-1'")
            .await;
        replicas[0].seal_rotating().await.unwrap();
        let (to, from) = pair(&mut replicas, 2, 0);
        store_from(to, from).await;
        apply_without_writer(&mut replicas[2]).await;

        assert_eq!(
            texts(
                &mut replicas[2],
                "SELECT id FROM fixture_cycle_a ORDER BY id"
            )
            .await,
            ["a-2"]
        );
    });
}

/// Writes, repair tombstones, deletes, an exclusion, and a restore each leave the class index
/// equal to a full scan of the merge state on every replica.
#[test]
fn class_index_matches_a_full_recompute() {
    async fn check(replicas: [&mut Replica; 3]) {
        for replica in replicas {
            replica.check_class_index().await;
        }
    }
    block_on(async {
        let mut anchor = Replica::anchor(0, 215).await;
        let mut removed = Replica::new(1, 215).await;
        let mut honest = Replica::new(2, 215).await;

        edit(
            &mut honest,
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'L1', 'C1');
             INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', 'L2', 'C2');
             INSERT INTO fixture_handles (id, label, code) VALUES ('h-3', 'L3', 'C3');",
        )
        .await;
        // A concurrent create collides, and the duplicate is repaired with a tombstone.
        edit(
            &mut anchor,
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-4', 'L1', 'C4')",
        )
        .await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut removed, &mut anchor).await;
        check([&mut anchor, &mut removed, &mut honest]).await;
        assert_eq!(redirects(&mut anchor, HANDLES_TABLE).await, ["h-4:h-1"]);

        edit(
            &mut honest,
            "UPDATE fixture_handles SET label = 'L9' WHERE id = 'h-2'",
        )
        .await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        check([&mut anchor, &mut removed, &mut honest]).await;

        edit(&mut honest, "DELETE FROM fixture_handles WHERE id = 'h-3'").await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut removed, &mut honest).await;
        check([&mut anchor, &mut removed, &mut honest]).await;

        // The removed device creates a row and edits another past its cutoff.
        edit(
            &mut removed,
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-5', 'L5', 'C5');
             UPDATE fixture_handles SET label = 'L8' WHERE id = 'h-2';",
        )
        .await;
        deliver(&mut honest, &mut removed).await;
        check([&mut anchor, &mut removed, &mut honest]).await;
        let device = removed.device_id.clone();
        let removal = anchor.remove(&device).await.unwrap();
        deliver(&mut honest, &mut anchor).await;
        deliver(&mut anchor, &mut honest).await;
        deliver(&mut anchor, &mut removed).await;
        deliver(&mut honest, &mut anchor).await;
        check([&mut anchor, &mut removed, &mut honest]).await;
        assert_eq!(
            texts(&mut anchor, "SELECT id FROM fixture_handles ORDER BY id").await,
            ["h-1", "h-2"]
        );

        restore_all(&mut anchor, removal).await;
        deliver(&mut honest, &mut anchor).await;
        check([&mut anchor, &mut removed, &mut honest]).await;
        assert_eq!(
            texts(&mut anchor, "SELECT id FROM fixture_handles ORDER BY id").await,
            ["h-1", "h-2", "h-5"]
        );
    });
}

/// An observer that applied `h-1` and `h-2` from replica 0, before `prepare` changes its class
/// index or its engine and `h-3` from replica 1, which never saw them, arrives. `h-3` must be
/// hidden behind the row it collides with, not refused by the domain unique constraint.
async fn assert_collision_with_an_untouched_row_is_repaired(
    prepare: impl AsyncFnOnce(&mut Replica),
    collision: &str,
) {
    let mut replicas = replicas(3).await;
    replicas[0]
        .exec(
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'L1', 'C1');
             INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', 'L2', 'C2');",
        )
        .await;
    replicas[1]
        .exec(&format!(
            "INSERT INTO fixture_handles (id, label, code) VALUES ('h-3', {collision})"
        ))
        .await;
    replicas[0].seal_rotating().await.unwrap();
    replicas[1].seal_rotating().await.unwrap();
    let (to, from) = pair(&mut replicas, 2, 0);
    store_from(to, from).await;
    apply_without_writer(&mut replicas[2]).await;
    prepare(&mut replicas[2]).await;

    let (to, from) = pair(&mut replicas, 2, 1);
    store_from(to, from).await;
    let observer = &mut replicas[2];
    apply_without_writer(observer).await;

    assert_eq!(
        observer
            .count("SELECT count(*) FROM sync_refused_rows")
            .await,
        0,
        "the collision was refused instead of repaired"
    );
    assert_eq!(
        texts(observer, "SELECT id FROM fixture_handles ORDER BY id").await,
        ["h-1", "h-2"]
    );
    observer.check_class_index().await;
}

/// Merge state from before the class index existed, as an older vault or a rollback copy holds
/// it: the index starts empty, so it must be built from merge state before it is read.
#[test]
fn an_empty_class_index_is_rebuilt_from_merge_state() {
    block_on(async {
        assert_collision_with_an_untouched_row_is_repaired(
            async |observer: &mut Replica| {
                observer
                    .exec(
                        "DELETE FROM sync_contested_classes;
                         DELETE FROM sync_unique_classes;
                         DELETE FROM sync_unique_class_rules;",
                    )
                    .await;
            },
            "'L1', 'C3'",
        )
        .await;
    });
}

/// A manifest update that reorders the duplicate rules of a table moves every class to another
/// rule position, so entries of untouched rows must follow.
#[test]
fn reordered_duplicate_rules_rebuild_the_class_index() {
    block_on(async {
        assert_collision_with_an_untouched_row_is_repaired(
            async |observer: &mut Replica| {
                observer.engine = test_engine_of(&REORDERED_RULES_MANIFEST);
            },
            "'L3', 'C1'",
        )
        .await;
    });
}

/// Replicas 0 and 1 create `h-1` and `h-4`, whose labels differ only in case, and the observer,
/// replica 2 running `engine`, applies both without a writer, so a collision stays unrepaired.
async fn observer_of_case_variants(engine: crate::Engine) -> Vec<Replica> {
    let mut replicas = replicas(3).await;
    replicas[2].engine = engine;
    replicas[0]
        .exec("INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'L1', 'C1')")
        .await;
    replicas[1]
        .exec("INSERT INTO fixture_handles (id, label, code) VALUES ('h-4', 'l1', 'C4')")
        .await;
    replicas[0].seal_rotating().await.unwrap();
    replicas[1].seal_rotating().await.unwrap();
    for from in [0, 1] {
        let (to, from) = pair(&mut replicas, 2, from);
        store_from(to, from).await;
    }
    apply_without_writer(&mut replicas[2]).await;
    replicas
}

/// Replica 0 creates an unrelated handle, and the observer applies it without a writer.
async fn apply_unrelated_handle(replicas: &mut [Replica]) {
    replicas[0]
        .exec("INSERT INTO fixture_handles (id, label, code) VALUES ('h-9', 'L9', 'C9')")
        .await;
    replicas[0].seal_rotating().await.unwrap();
    let (to, from) = pair(replicas, 2, 0);
    store_from(to, from).await;
    apply_without_writer(&mut replicas[2]).await;
}

/// Ids of the materialized handles, in order.
async fn handle_ids(replica: &mut Replica) -> Vec<String> {
    texts(replica, "SELECT id FROM fixture_handles ORDER BY id").await
}

/// The class index with its rule fingerprint, one line per stored row.
async fn class_index_rows(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT 'rules:' || hex(fingerprint) FROM sync_unique_class_rules
         UNION ALL SELECT 'class:' || table_id || ':' || rule_index || ':' || hex(class_key)
             || ':' || row_key FROM sync_unique_classes
         UNION ALL SELECT 'contested:' || table_id || ':' || rule_index || ':' || hex(class_key)
             FROM sync_contested_classes
         ORDER BY 1",
    )
    .await
}

/// A recovery read under changed duplicate rules writes nothing, so the next apply still
/// rebuilds the class index and hides the row that now collides.
#[test]
fn a_recovery_read_under_changed_rules_leaves_the_rebuild_to_the_next_apply() {
    block_on(async {
        let mut replicas = observer_of_case_variants(test_engine()).await;
        assert_eq!(handle_ids(&mut replicas[2]).await, ["h-1", "h-4"]);
        replicas[2].engine = test_engine_of(&NOCASE_LABEL_MANIFEST);
        let before = class_index_rows(&mut replicas[2]).await;
        let observer = &mut replicas[2];
        let offered = observer
            .engine
            .recovery_values(&mut observer.conn, HANDLES_TABLE, "h-1")
            .await
            .unwrap();
        assert!(offered.is_none());
        assert_eq!(
            class_index_rows(observer).await,
            before,
            "a recovery read wrote the class index"
        );

        apply_unrelated_handle(&mut replicas).await;
        assert_eq!(
            handle_ids(&mut replicas[2]).await,
            ["h-1", "h-9"],
            "the row that now collides stayed shown"
        );
        replicas[2].check_class_index().await;
    });
}

/// A rule change that dissolves a contested class shows its hidden rows again, although no
/// operation touches them.
#[test]
fn rows_of_a_class_that_stops_colliding_are_shown_after_the_rebuild() {
    block_on(async {
        let mut replicas = observer_of_case_variants(test_engine_of(&NOCASE_LABEL_MANIFEST)).await;
        assert_eq!(handle_ids(&mut replicas[2]).await, ["h-1"]);
        replicas[2].engine = test_engine();

        apply_unrelated_handle(&mut replicas).await;
        assert_eq!(
            handle_ids(&mut replicas[2]).await,
            ["h-1", "h-4", "h-9"],
            "the row of the dissolved class stayed hidden"
        );
        replicas[2].check_class_index().await;
    });
}

/// A class key is a digest of the folded unique values, so a long value takes the same index
/// space as a short one, and every replica derives the same key.
#[test]
fn class_keys_are_fixed_size_digests_equal_on_every_replica() {
    block_on(async {
        let long = "x".repeat(40);
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', '{long}', 'C1')"
            ))
            .await;
        replicas[1]
            .exec(&format!(
                "INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', '{long}', 'C2')"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM sync_unique_classes WHERE length(class_key) <> 32")
                    .await,
                0,
                "replica {} stores an unbounded class key",
                replica.index
            );
            replica.check_class_index().await;
        }
        let keys: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT class_key FROM sync_unique_classes WHERE row_key = 'h-1' ORDER BY rule_index",
        )
        .fetch_all(&mut replicas[0].conn)
        .await
        .unwrap();
        let other: Vec<Vec<u8>> = sqlx::query_scalar(
            "SELECT class_key FROM sync_unique_classes WHERE row_key = 'h-1' ORDER BY rule_index",
        )
        .fetch_all(&mut replicas[1].conn)
        .await
        .unwrap();
        assert_eq!(keys, other, "replicas disagree on a class key");
    });
}

/// `h-2` collides with `h-1` on its label and with `h-3` on its code. Hidden behind `h-1`, it
/// must not take `h-3` down with it.
#[test]
fn a_row_hidden_under_one_unique_rule_keeps_no_class_under_another() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_handles (id, label, code) VALUES ('h-1', 'L', 'X');
                 INSERT INTO fixture_handles (id, label, code) VALUES ('h-3', 'N', 'C');",
            )
            .await;
        replicas[1]
            .exec("INSERT INTO fixture_handles (id, label, code) VALUES ('h-2', 'L', 'C')")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                texts(replica, "SELECT id FROM fixture_handles ORDER BY id").await,
                ["h-1", "h-3"]
            );
            assert_eq!(redirects(replica, HANDLES_TABLE).await, ["h-2:h-1"]);
        }
    });
}

/// The referrer is listed before its target's table, and its cascading reference is written
/// concurrently with the target's deletion.
#[test]
fn a_cascade_into_a_concurrently_deleted_target_hides_the_referrer_with_its_cause() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'A');
                 INSERT INTO fixture_cycle_b (id, label) VALUES ('b-1', 'B');",
            )
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_cycle_b WHERE id = 'b-1'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_cycle_a SET peer_id = 'b-1' WHERE id = 'a-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            for table in ["fixture_cycle_a", "fixture_cycle_b"] {
                assert_eq!(
                    replica
                        .count(&format!("SELECT count(*) FROM {table}"))
                        .await,
                    0,
                    "{table} on replica {}",
                    replica.index
                );
            }
            assert_eq!(
                tombstone_causes(replica, CYCLE_A_TABLE).await,
                ["a-1:'b-1'"]
            );
            assert_eq!(tombstone_causes(replica, CYCLE_B_TABLE).await, ["b-1:NULL"]);
        }
    });
}

#[test]
fn a_local_cascade_names_its_cause_and_keeps_derived_key_children_live() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(MEMBERSHIP_PARENTS).await;
        replicas[0]
            .exec(&format!(
                "INSERT INTO fixture_links (id, owner_id, note) VALUES ('link-1', 'item-1', 'owned');
                 {};",
                add_membership("item-1", "a-1", "member")
            ))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_items WHERE id = 'item-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM fixture_links").await, 0);
            assert!(memberships(replica).await.is_empty());
            assert_eq!(
                tombstone_causes(replica, LINKS_TABLE).await,
                ["link-1:'item-1'"]
            );
            assert_eq!(
                tombstone_causes(replica, ITEMS_TABLE).await,
                ["item-1:NULL"]
            );
            // The membership is hidden by its deleted item, with no existence write.
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
                    "SELECT count(*) FROM sync_register_versions
                     WHERE table_id = {table} AND group_id = 2"
                )
                .await,
                1
            );
        }
    });
}

//! Engine entry points across replicas of the vault and fixture domains.

use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync_contracts::bounds::{MAX_DEPENDENCIES, MAX_WRITERS_PER_DEVICE};
use ganbaru_sync_contracts::hlc::CLOCK_WARNING_AHEAD_MS;
use ganbaru_sync_contracts::{
    Content, Envelope, Field, GroupMask, RevokeReason, TableId, VersionVector, WriterId,
};

use super::block_on;
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, ITEMS_TABLE, LINKS_TABLE, MEMBERSHIPS_TABLE, membership_key,
};
use super::replica::{Identity, Replica, START_MS, VAULT_ID, converge, device_id, pair, person};
use crate::error::failpoints;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use crate::{
    ConflictRow, HoldReason, StoreOutcome, StoreRefusal, SyncError, SyncResult, WriterState,
};

const STORED: StoreOutcome = StoreOutcome::Stored { held: None };

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 1).await);
    }
    replicas
}

async fn seed_note(replica: &mut Replica) {
    replica
        .exec(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
             INSERT INTO quick_notes (id, title, tag_id, order_key, body_plain_text)
                 VALUES ('note-1', 'One', 'tag-1', 'a0', 'body');
             INSERT INTO quick_note_text_runs (note_id, sort_order, content)
                 VALUES ('note-1', 0, 'body');",
        )
        .await;
}

async fn assert_converged(replicas: &mut [Replica]) {
    let first = replicas[0].dump().await;
    for replica in replicas.iter_mut().skip(1) {
        assert_eq!(
            replica.dump().await,
            first,
            "replica {} diverged",
            replica.index
        );
    }
    for replica in replicas.iter_mut() {
        let status = replica
            .engine
            .status(&mut replica.conn, &replica.ctx)
            .await
            .unwrap();
        assert_eq!(status.waiting, 0);
        assert_eq!(status.held.total(), 0);
        assert_eq!(status.pending_captures, 0);
    }
}

async fn title(replica: &mut Replica, id: &str) -> Option<String> {
    replica
        .scalar_text(&format!("SELECT title FROM quick_notes WHERE id = '{id}'"))
        .await
}

fn text(value: &crate::ConflictVersion) -> String {
    match value.value.fields() {
        [Field::Text(text)] => text.clone(),
        other => panic!("unexpected title value {other:?}"),
    }
}

#[test]
fn sealed_changes_converge_on_every_replica() {
    block_on(async {
        let mut replicas = replicas(3).await;
        seed_note(&mut replicas[0]).await;
        let report = replicas[0].seal().await.unwrap();
        assert_eq!(report.last_seq, Some(2));
        assert_eq!(report.changes, 2);
        assert!(report.invalid_rows.is_empty());
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(title(replica, "note-1").await.as_deref(), Some("One"));
            let body = replica
                .scalar_text("SELECT body_plain_text FROM quick_notes WHERE id = 'note-1'")
                .await;
            assert_eq!(body.as_deref(), Some("body"));
        }
    });
}

#[test]
fn applied_rows_take_local_defaults_and_skip_gated_triggers() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
                 UPDATE fixture_items SET label = 'Two' WHERE id = 'item-1';
                 UPDATE fixture_items SET label = 'Three' WHERE id = 'item-1';",
            )
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        let edits = "SELECT label_edits FROM fixture_items WHERE id = 'item-1'";
        let marks = "SELECT count(*) FROM fixture_item_marks";
        assert_eq!(replicas[0].count(edits).await, 2);
        assert_eq!(replicas[0].count(marks).await, 1);
        assert_eq!(replicas[1].count(edits).await, 0);
        assert_eq!(replicas[1].count(marks).await, 0);
        let indexed = "SELECT label FROM fixture_items_index WHERE item_id = 'item-1'";
        assert_eq!(
            replicas[1].scalar_text(indexed).await.as_deref(),
            Some("Three")
        );

        replicas[0]
            .exec("UPDATE fixture_items SET label = 'Four' WHERE id = 'item-1'")
            .await;
        replicas[0].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(replicas[1].count(edits).await, 1);
        assert_eq!(
            replicas[1].scalar_text(indexed).await.as_deref(),
            Some("Four")
        );

        replicas[1]
            .exec("DELETE FROM fixture_items WHERE id = 'item-1'")
            .await;
        replicas[1].seal().await.unwrap();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count(marks).await, 0);
            assert_eq!(
                replica
                    .count("SELECT count(*) FROM fixture_items_index")
                    .await,
                0
            );
        }
    });
}

#[test]
fn store_refuses_unknown_writers_gaps_and_tampered_operations() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let [a, b] = replicas.as_mut_slice() else {
            unreachable!()
        };
        a.exec("INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')")
            .await;
        a.seal().await.unwrap();
        a.exec("UPDATE quick_note_tags SET name = 'Home' WHERE id = 'tag-1'")
            .await;
        a.seal().await.unwrap();
        let ops = a.ops_since(&VersionVector::new()).await;
        assert_eq!(ops.len(), 3);

        assert_eq!(
            b.receive(&ops[1..2]).await,
            vec![StoreOutcome::Refused(StoreRefusal::UnknownWriter)]
        );
        assert_eq!(b.receive(&ops[0..1]).await, vec![STORED]);
        assert_eq!(b.receive(&ops[0..1]).await, vec![StoreOutcome::Duplicate]);
        assert_eq!(
            b.receive(&ops[2..3]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Gap { expected: 2 })]
        );

        let mut signature = ops[1].clone();
        *signature.last_mut().unwrap() ^= 1;
        assert_eq!(
            b.receive(&[signature]).await,
            vec![StoreOutcome::Refused(StoreRefusal::BadSignature)]
        );
        let envelope = Envelope::decode(&ops[1]).unwrap();
        let body_end = ops[1].len() - envelope.signature.len();
        let mut body = ops[1].clone();
        body[body_end - 1] ^= 1;
        assert_eq!(
            b.receive(&[body]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Malformed)]
        );
        assert_eq!(
            b.receive(&[vec![1, 2, 3]]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Malformed)]
        );

        assert_eq!(b.receive(&ops[1..]).await, vec![STORED, STORED]);
        b.apply_all().await.unwrap();
        assert_eq!(b.domain_dump().await, a.domain_dump().await);
    });
}

#[test]
fn store_refuses_other_spaces_and_untrusted_writers() {
    block_on(async {
        let mut vault = Replica::new(0, 1).await;
        let mut other = Replica::with_space(1, 1, "vault-other", person()).await;
        let stranger = PersonKeyPair::from_seed(&[9; 32]).unwrap();
        let mut untrusted = Replica::with_space(2, 1, VAULT_ID, stranger).await;
        for replica in [&mut other, &mut untrusted] {
            replica.seal().await.unwrap();
        }
        let foreign = other.ops_since(&VersionVector::new()).await;
        assert_eq!(
            vault.receive(&foreign).await,
            vec![StoreOutcome::Refused(StoreRefusal::WrongSpace)]
        );
        let untrusted_ops = untrusted.ops_since(&VersionVector::new()).await;
        assert_eq!(
            vault.receive(&untrusted_ops).await,
            vec![StoreOutcome::Refused(StoreRefusal::Untrusted)]
        );
        assert!(vault.stored_vector().await.is_empty());
    });
}

#[test]
fn a_cloned_installation_forks_and_storing_stops_at_the_fork() {
    block_on(async {
        let mut a = Replica::new(0, 1).await;
        let mut b = Replica::new(1, 1).await;
        a.exec("INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')")
            .await;
        a.seal().await.unwrap();
        let mut clone = a.clone_installation(2).await;
        a.exec("UPDATE quick_note_tags SET name = 'Original' WHERE id = 'tag-1'")
            .await;
        a.seal().await.unwrap();
        clone
            .exec("UPDATE quick_note_tags SET name = 'Clone' WHERE id = 'tag-1'")
            .await;
        clone.seal().await.unwrap();

        let ops = a.ops_since(&VersionVector::new()).await;
        assert_eq!(b.receive(&ops).await, vec![STORED; 3]);
        let forked = clone.ops_since(&VersionVector::new()).await;
        assert_eq!(forked[..2], ops[..2]);
        assert_ne!(forked[2], ops[2]);
        assert_eq!(
            b.receive(&[forked[2].clone(), ops[0].clone()]).await,
            vec![StoreOutcome::Refused(StoreRefusal::Fork { seq: 3 })]
        );
    });
}

#[test]
fn a_forked_installation_reseals_past_the_divergence_and_takes_the_other_copy() {
    block_on(async {
        let mut a = Replica::new(0, 1).await;
        let mut hub = Replica::new(1, 1).await;
        a.exec("INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')")
            .await;
        a.seal().await.unwrap();
        let mut clone = a.clone_installation(2).await;
        a.exec("UPDATE quick_note_tags SET name = 'Original' WHERE id = 'tag-1'")
            .await;
        a.seal().await.unwrap();
        clone
            .exec("UPDATE quick_note_tags SET name = 'Clone' WHERE id = 'tag-1'")
            .await;
        clone.seal().await.unwrap();
        clone
            .exec(
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-2', 'Home', 'a1')",
            )
            .await;
        clone.seal().await.unwrap();
        hub.pull(&mut a).await;
        let forked = clone.ops_since(&hub.stored_vector().await).await;
        assert!(matches!(
            hub.receive(&forked).await[..],
            [StoreOutcome::Refused(StoreRefusal::Fork { .. })]
        ));

        let old = clone.writer();
        clone.rotate();
        let report = clone
            .engine
            .reseal_fork(
                &mut clone.conn,
                &clone.ctx,
                old,
                2,
                &mut clone.identity.local(),
                clone.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, 2);
        assert!(clone.pull(&mut hub).await.contains(&STORED));
        assert_eq!(clone.stored_vector().await.get(&old), 3);
        hub.pull(&mut clone).await;
        a.pull(&mut hub).await;
        let mut replicas = vec![a, hub, clone];
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM quick_note_tags WHERE id = 'tag-2'")
                .await,
            1
        );
    });
}

#[test]
fn concurrent_titles_surface_a_conflict_that_a_forced_choice_resolves() {
    block_on(async {
        let mut replicas = replicas(2).await;
        seed_note(&mut replicas[0]).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'From A' WHERE id = 'note-1'")
            .await;
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'From B' WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;

        let mut hidden = None;
        for replica in &mut replicas {
            let rows = replica
                .engine
                .conflicts(&mut replica.conn, NOTES_TABLE)
                .await
                .unwrap();
            assert_eq!(
                rows,
                vec![ConflictRow {
                    table: NOTES_TABLE,
                    row_key: "note-1".to_owned(),
                    groups: GroupMask::of(note_group::TITLE),
                }]
            );
            let details = replica
                .engine
                .conflict_details(&mut replica.conn, replica.ctx.space, NOTES_TABLE, "note-1")
                .await
                .unwrap();
            assert_eq!(details.len(), 1);
            assert_eq!(details[0].group, note_group::TITLE);
            let versions = &details[0].versions;
            assert_eq!(versions.len(), 2);
            assert_eq!(
                versions.iter().filter(|version| version.displayed).count(),
                1
            );
            let shown = versions.iter().find(|version| version.displayed).unwrap();
            assert_eq!(title(replica, "note-1").await, Some(text(shown)));
            hidden = Some(text(
                versions.iter().find(|version| !version.displayed).unwrap(),
            ));
            let status = replica
                .engine
                .status(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(status.conflicts, vec![("quick_notes", 1)]);
        }

        let chosen = hidden.unwrap();
        let resolver = &mut replicas[1];
        assert!(matches!(
            resolver
                .engine
                .force_group(
                    &mut resolver.conn,
                    NOTES_TABLE,
                    "note-1",
                    note_group::COLOR,
                    1
                )
                .await,
            Err(SyncError::InvalidRequest(_))
        ));
        assert!(matches!(
            resolver
                .engine
                .force_group(
                    &mut resolver.conn,
                    NOTES_TABLE,
                    "note-9",
                    note_group::TITLE,
                    1
                )
                .await,
            Err(SyncError::InvalidRequest(_))
        ));
        {
            let mut tx = sqlx::Connection::begin(&mut resolver.conn).await.unwrap();
            sqlx::query("UPDATE quick_notes SET title = ? WHERE id = 'note-1'")
                .bind(&chosen)
                .execute(&mut *tx)
                .await
                .unwrap();
            resolver
                .engine
                .force_group(&mut tx, NOTES_TABLE, "note-1", note_group::TITLE, 1)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, NOTES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(title(replica, "note-1").await, Some(chosen.clone()));
        }
    });
}

#[test]
fn keeping_the_displayed_version_also_resolves_a_conflict() {
    block_on(async {
        let mut replicas = replicas(2).await;
        seed_note(&mut replicas[0]).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'From A' WHERE id = 'note-1'")
            .await;
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'From B' WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        let shown = title(&mut replicas[0], "note-1").await.unwrap();
        let keeper = &mut replicas[0];
        keeper
            .engine
            .force_group(
                &mut keeper.conn,
                NOTES_TABLE,
                "note-1",
                note_group::TITLE,
                1,
            )
            .await
            .unwrap();
        assert_eq!(keeper.seal().await.unwrap().changes, 1);
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(title(replica, "note-1").await, Some(shown.clone()));
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, NOTES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    });
}

#[test]
fn concurrent_values_of_groups_that_are_not_surfaced_merge_silently() {
    block_on(async {
        let mut replicas = replicas(3).await;
        seed_note(&mut replicas[0]).await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("UPDATE quick_notes SET color = 1, pinned = 1 WHERE id = 'note-1'")
            .await;
        replicas[1]
            .exec(
                "UPDATE quick_notes SET color = 2, archived = 1, order_key = 'a5' WHERE id = 'note-1'",
            )
            .await;
        replicas[2]
            .exec(
                "UPDATE quick_notes SET trashed_at = '2027-01-01T00:00:00.000Z', order_key = 'a7'
                 WHERE id = 'note-1'",
            )
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, NOTES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    });
}

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

async fn texts(replica: &mut Replica, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut replica.conn)
        .await
        .unwrap()
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
    }
    replicas
}

async fn close_recovery(replica: &mut Replica, key: &str) -> SyncResult<crate::SealReport> {
    replica
        .engine
        .close_recovery(
            &mut replica.conn,
            &replica.ctx,
            NOTES_TABLE,
            key,
            &mut replica.identity.local(),
            replica.now_ms,
        )
        .await
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

async fn seal_revoke(
    revoker: &mut Replica,
    target: ganbaru_sync_contracts::WriterId,
    cutoff: u64,
) -> SyncResult<crate::SealReport> {
    revoker
        .engine
        .seal_revoke(
            &mut revoker.conn,
            &revoker.ctx,
            target,
            cutoff,
            RevokeReason::Revoked,
            &mut revoker.identity.local(),
            revoker.now_ms,
        )
        .await
}

#[test]
fn revoking_a_writer_reseals_its_unpublished_operations() {
    block_on(async {
        let mut replicas = replicas(3).await;
        seed_note(&mut replicas[1]).await;
        replicas[1].seal().await.unwrap();
        let (a, rest) = replicas.split_at_mut(1);
        let (b, c) = rest.split_at_mut(1);
        let (a, b, c) = (&mut a[0], &mut b[0], &mut c[0]);
        a.pull(b).await;
        c.pull(b).await;
        let old = b.writer();
        b.exec("UPDATE quick_notes SET title = 'Unpublished' WHERE id = 'note-1'")
            .await;
        b.seal().await.unwrap();
        let unpublished = b.ops_since(&a.stored_vector().await).await;
        assert_eq!(unpublished.len(), 1);

        seal_revoke(a, old, 2).await.unwrap();
        c.pull(a).await;
        assert_eq!(
            a.receive(&unpublished).await,
            vec![StoreOutcome::Refused(StoreRefusal::Revoked)]
        );
        b.pull(a).await;
        assert_ne!(b.writer(), old);
        assert_eq!(
            b.receive(&unpublished).await,
            vec![StoreOutcome::Refused(StoreRefusal::Revoked)]
        );
        assert_eq!(title(b, "note-1").await.as_deref(), Some("Unpublished"));

        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                title(replica, "note-1").await.as_deref(),
                Some("Unpublished")
            );
            let status = replica
                .engine
                .status(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            let revoked = status
                .writers
                .iter()
                .find(|writer| writer.writer == old)
                .unwrap();
            assert_eq!(revoked.state, WriterState::Revoked);
            assert_eq!(revoked.cutoff, Some(2));
            assert_eq!(revoked.applied, 2);
        }
    });
}

#[test]
fn revocations_must_keep_applied_operations_of_another_writer() {
    block_on(async {
        let mut replicas = replicas(2).await;
        seed_note(&mut replicas[1]).await;
        replicas[1].seal().await.unwrap();
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'Two' WHERE id = 'note-1'")
            .await;
        replicas[1].seal().await.unwrap();
        let (a, b) = replicas.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        let target = b.writer();
        let own = a.writer();
        assert!(matches!(
            seal_revoke(a, target, 1).await,
            Err(SyncError::InvalidRevoke(_))
        ));
        a.pull(b).await;
        assert!(matches!(
            seal_revoke(a, own, 1).await,
            Err(SyncError::InvalidRevoke(_))
        ));
        assert!(matches!(
            seal_revoke(a, target, 0).await,
            Err(SyncError::InvalidRevoke(_))
        ));
        assert!(matches!(
            seal_revoke(a, target, 2).await,
            Err(SyncError::InvalidRevoke(_))
        ));
        seal_revoke(a, target, 3).await.unwrap();
    });
}

#[test]
fn a_remote_revocation_below_applied_operations_is_blocked() {
    block_on(async {
        let mut replicas = replicas(3).await;
        seed_note(&mut replicas[1]).await;
        replicas[1].seal().await.unwrap();
        let (a, rest) = replicas.split_at_mut(1);
        let (b, c) = rest.split_at_mut(1);
        let (a, b, c) = (&mut a[0], &mut b[0], &mut c[0]);
        a.pull(b).await;
        b.exec("UPDATE quick_notes SET title = 'Seen' WHERE id = 'note-1'")
            .await;
        b.seal().await.unwrap();
        c.pull(b).await;
        let target = b.writer();
        seal_revoke(a, target, 2).await.unwrap();

        let known = c.stored_vector().await;
        let ops = a.ops_since(&known).await;
        c.receive(&ops).await;
        let reports = c.apply_all().await.unwrap();
        assert!(
            reports
                .iter()
                .any(|report| !report.blocked_revocations.is_empty())
        );
        let status = c.engine.status(&mut c.conn, &c.ctx).await.unwrap();
        assert!(status.waiting > 0);
        assert_eq!(title(c, "note-1").await.as_deref(), Some("Seen"));
    });
}

#[test]
fn revoke_from_non_coordinator_writer_is_not_applied() {
    block_on(async {
        let mut replicas = replicas(4).await;
        seed_note(&mut replicas[1]).await;
        replicas[1].seal().await.unwrap();
        let (a, rest) = replicas.split_at_mut(1);
        let (b, rest) = rest.split_at_mut(1);
        let (c, d) = rest.split_at_mut(1);
        let (a, b, c, d) = (&mut a[0], &mut b[0], &mut c[0], &mut d[0]);
        a.pull(b).await;
        c.pull(b).await;
        d.pull(b).await;
        let target = b.writer();
        let cutoff = c.applied_vector().await.get(&target);
        assert!(matches!(
            seal_revoke(c, target, cutoff).await,
            Err(SyncError::InvalidRevoke(_))
        ));

        // A modified build on C seals the revocation anyway.
        c.ctx.coordinator_device_id = device_id(c.index);
        seal_revoke(c, target, cutoff).await.unwrap();
        let ops = c.ops_since(&a.stored_vector().await).await;
        let outcomes = a.receive(&ops).await;
        assert_eq!(
            outcomes.last(),
            Some(&StoreOutcome::Refused(StoreRefusal::UnauthorizedRevoke))
        );
        a.apply_all().await.unwrap();
        assert_eq!(title(a, "note-1").await.as_deref(), Some("One"));

        // D stored it while C coordinated; once A coordinates it waits without revoking.
        d.ctx.coordinator_device_id = device_id(c.index);
        let ops = c.ops_since(&d.stored_vector().await).await;
        assert!(
            d.receive(&ops)
                .await
                .iter()
                .all(|outcome| *outcome == STORED)
        );
        d.ctx.coordinator_device_id = device_id(a.index);
        let reports = d.apply_all().await.unwrap();
        assert!(reports.iter().all(|report| report.held == 0));
        let status = d.engine.status(&mut d.conn, &d.ctx).await.unwrap();
        assert!(status.waiting > 0);
        assert_eq!(title(d, "note-1").await.as_deref(), Some("One"));
    });
}

#[test]
fn a_reseal_is_refused_once_another_writer_saw_the_replaced_operations() {
    block_on(async {
        let mut replicas = replicas(2).await;
        seed_note(&mut replicas[0]).await;
        replicas[0].seal().await.unwrap();
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'Seen' WHERE id = 'note-1'")
            .await;
        replicas[0].seal().await.unwrap();
        let (b, c) = replicas.split_at_mut(1);
        let (b, c) = (&mut b[0], &mut c[0]);
        c.pull(b).await;
        c.exec("UPDATE quick_notes SET color = 3 WHERE id = 'note-1'")
            .await;
        c.seal().await.unwrap();
        b.pull(c).await;

        let old = b.writer();
        b.rotate();
        let now = b.now_ms;
        for keep_through in [0, 2, 4] {
            let result = b
                .engine
                .reseal(
                    &mut b.conn,
                    &b.ctx,
                    old,
                    keep_through,
                    &mut b.identity.local(),
                    now,
                )
                .await;
            assert!(
                matches!(result, Err(SyncError::InvalidReseal(_))),
                "keep_through {keep_through}: {result:?}"
            );
        }
        let same = b.writer();
        let result = b
            .engine
            .reseal(&mut b.conn, &b.ctx, same, 1, &mut b.identity.local(), now)
            .await;
        assert!(matches!(result, Err(SyncError::InvalidReseal(_))));
    });
}

#[test]
fn operations_with_a_newer_manifest_are_held_until_an_update_can_read_them() {
    block_on(async {
        let mut replicas = replicas(2).await;
        seed_note(&mut replicas[0]).await;
        replicas[0].seal().await.unwrap();
        let (a, b) = replicas.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        let ops = a.ops_since(&VersionVector::new()).await;
        let mut newer = Envelope::decode(&ops[1]).unwrap().operation().unwrap();
        newer.header.manifest_version += 1;
        let newer = newer.seal(&a.identity.key).unwrap().into_bytes();
        assert_eq!(b.receive(&ops[..1]).await, vec![STORED]);
        assert_eq!(
            b.receive(&[newer]).await,
            vec![StoreOutcome::Stored {
                held: Some(HoldReason::NewerManifest)
            }]
        );
        b.apply_all().await.unwrap();
        let status = b.engine.status(&mut b.conn, &b.ctx).await.unwrap();
        assert_eq!(status.held.newer_manifest, 1);
        assert_eq!(status.held.total(), 1);
        let held = b.engine.held_ops(&mut b.conn, &b.ctx, 10).await.unwrap();
        assert_eq!(held.len(), 1);
        assert_eq!((held[0].writer, held[0].seq), (a.writer(), 2));
        assert_eq!(held[0].reason, HoldReason::NewerManifest);
        assert_eq!(
            b.engine.reevaluate_held(&mut b.conn, &b.ctx).await.unwrap(),
            0
        );
        assert_eq!(b.count("SELECT count(*) FROM quick_notes").await, 0);
        assert_eq!(b.applied_vector().await.get(&a.writer()), 1);
    });
}

/// Arms `name`, checks that `call` fails at it without changing the database, then checks
/// that the call succeeds once disarmed.
async fn assert_rolls_back<T: std::fmt::Debug>(
    replica: &mut Replica,
    name: &'static str,
    mut call: impl AsyncFnMut(&mut Replica) -> SyncResult<T>,
) -> T {
    let before = replica.snapshot().await;
    failpoints::arm(name);
    let result = call(replica).await;
    failpoints::clear();
    match result {
        Err(SyncError::Failpoint(hit)) => assert_eq!(hit, name),
        other => panic!("{name} did not fail: {other:?}"),
    }
    assert!(replica.snapshot().await == before, "{name} left changes");
    replica.check_invariants().await;
    call(replica).await.unwrap()
}

#[test]
fn failpoints_roll_back_every_entry_point() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let (a, rest) = replicas.split_at_mut(1);
        let (b, c) = rest.split_at_mut(1);
        let (a, b, c) = (&mut a[0], &mut b[0], &mut c[0]);

        seed_note(a).await;
        assert_rolls_back(a, "seal.reserve", async |r: &mut Replica| r.seal().await).await;
        a.exec("UPDATE quick_notes SET title = 'Again' WHERE id = 'note-1'")
            .await;
        assert_rolls_back(a, "seal.before_commit", async |r: &mut Replica| {
            r.seal().await
        })
        .await;

        let ops = a.ops_since(&VersionVector::new()).await;
        assert_rolls_back(b, "store.before_commit", async |r: &mut Replica| {
            r.engine
                .store_many(&mut r.conn, &r.ctx, &ops, r.now_ms)
                .await
        })
        .await;
        assert_rolls_back(b, "apply.before_commit", async |r: &mut Replica| {
            r.apply_once().await
        })
        .await;
        assert_eq!(b.domain_dump().await, a.domain_dump().await);

        b.exec("DELETE FROM quick_notes WHERE id = 'note-1'").await;
        b.seal().await.unwrap();
        c.pull(a).await;
        c.exec("UPDATE quick_notes SET title = 'Kept' WHERE id = 'note-1'")
            .await;
        c.seal().await.unwrap();
        c.pull(b).await;
        assert_rolls_back(c, "recovery.before_commit", async |r: &mut Replica| {
            close_recovery(r, "note-1").await
        })
        .await;
        assert_rolls_back(c, "reevaluate.before_commit", async |r: &mut Replica| {
            r.engine.reevaluate_held(&mut r.conn, &r.ctx).await
        })
        .await;

        b.pull(c).await;
        c.exec("UPDATE quick_note_tags SET name = 'Unpublished' WHERE id = 'tag-1'")
            .await;
        c.seal().await.unwrap();
        let target = c.writer();
        let cutoff = b.applied_vector().await.get(&target);
        // B coordinates here, so it may revoke and C accepts the revocation.
        b.ctx.coordinator_device_id = device_id(b.index);
        c.ctx.coordinator_device_id = device_id(b.index);
        seal_revoke(b, target, cutoff).await.unwrap();
        let known = c.stored_vector().await;
        let ops = b.ops_since(&known).await;
        c.receive(&ops).await;
        let report = loop {
            let report = c.apply_once().await.unwrap();
            if report.reseal_needed.is_some() || !report.more {
                break report;
            }
        };
        let needed = report
            .reseal_needed
            .expect("the revocation cuts off an applied local operation");
        assert_rolls_back(c, "reseal.before_commit", async |r: &mut Replica| {
            r.reseal(needed).await
        })
        .await;
    });
}

/// Index of the replica that seals geneses for other installations.
const MINTER_INDEX: usize = 9;

/// Geneses of new writers numbered `writers`, each on `device(writer)`, as other installations
/// would seal them.
async fn mint_writers(
    writers: std::ops::Range<usize>,
    device: impl Fn(usize) -> String,
) -> Vec<Vec<u8>> {
    let mut minter = Replica::new(MINTER_INDEX, 1).await;
    for writer in writers {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&(writer as u64).to_le_bytes());
        seed[31] = 2;
        minter.identity = Identity::new(seed, &minter.person, &device(writer), None);
        minter.seal().await.unwrap();
    }
    minter.ops_since(&VersionVector::new()).await
}

/// Dependency counts of every stored operation of `writer`, in chain order, with whether
/// each carries no changes.
async fn chain_shape(replica: &mut Replica, writer: WriterId) -> Vec<(usize, bool)> {
    let mut shape = Vec::new();
    for bytes in replica.ops_since(&VersionVector::new()).await {
        let operation = Envelope::decode(&bytes).unwrap().operation().unwrap();
        if operation.writer == writer {
            let empty =
                matches!(&operation.content, Content::Changes(changes) if changes.is_empty());
            shape.push((operation.header.dependencies.len(), empty));
        }
    }
    shape
}

#[test]
fn seal_succeeds_when_more_than_max_dependencies_writers_advanced() {
    block_on(async {
        let mut replicas = replicas(2).await;
        let minted = MAX_DEPENDENCIES + MAX_DEPENDENCIES / 2;
        let geneses = mint_writers(0..minted, |writer| device_id(100 + writer)).await;
        let (a, b) = replicas.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        assert_eq!(a.receive(&geneses).await, vec![STORED; minted]);
        a.apply_all().await.unwrap();

        seed_note(a).await;
        let report = a.seal().await.unwrap();
        let shape = chain_shape(a, a.writer()).await;
        // The genesis, one operation that only advances dependencies, then the note with the
        // rest.
        assert_eq!(report.last_seq, Some(3));
        assert_eq!(
            shape,
            vec![
                (0, false),
                (MAX_DEPENDENCIES, true),
                (minted - MAX_DEPENDENCIES, false)
            ]
        );
        assert_eq!(a.applied_vector().await.len(), minted + 1);

        b.pull(a).await;
        assert_eq!(title(b, "note-1").await.as_deref(), Some("One"));
        assert_converged(&mut replicas).await;
    });
}

#[test]
fn a_fork_reseals_operations_that_only_advance_dependencies() {
    block_on(async {
        let mut a = Replica::new(0, 1).await;
        let mut hub = Replica::new(1, 1).await;
        a.exec("INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0')")
            .await;
        a.seal().await.unwrap();
        let mut clone = a.clone_installation(2).await;
        a.exec("UPDATE quick_note_tags SET name = 'Original' WHERE id = 'tag-1'")
            .await;
        a.seal().await.unwrap();
        let geneses = mint_writers(0..MAX_DEPENDENCIES + 1, |writer| device_id(100 + writer)).await;
        clone.receive(&geneses).await;
        clone.apply_all().await.unwrap();
        clone
            .exec(
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-2', 'Home', 'a1')",
            )
            .await;
        clone.seal().await.unwrap();
        let old = clone.writer();
        assert!(
            chain_shape(&mut clone, old)
                .await
                .iter()
                .any(|(_, empty)| *empty)
        );
        hub.pull(&mut a).await;
        let forked = clone.ops_since(&hub.stored_vector().await).await;
        assert!(
            hub.receive(&forked)
                .await
                .contains(&StoreOutcome::Refused(StoreRefusal::Fork { seq: 3 }))
        );

        clone.rotate();
        let report = clone
            .engine
            .reseal_fork(
                &mut clone.conn,
                &clone.ctx,
                old,
                2,
                &mut clone.identity.local(),
                clone.now_ms,
            )
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, 2);
        let new = clone.writer();
        let shape = chain_shape(&mut clone, new).await;
        assert!(
            shape
                .iter()
                .all(|(dependencies, _)| *dependencies <= MAX_DEPENDENCIES)
        );
        assert!(shape.iter().any(|(_, empty)| *empty));
        hub.pull(&mut clone).await;
        a.pull(&mut hub).await;
        let mut replicas = vec![a, hub, clone];
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        assert_eq!(
            replicas[0]
                .count("SELECT count(*) FROM quick_note_tags WHERE id = 'tag-2'")
                .await,
            1
        );
    });
}

#[test]
fn store_refuses_geneses_past_the_writers_of_one_device() {
    block_on(async {
        let mut a = Replica::new(0, 1).await;
        let geneses = mint_writers(0..MAX_WRITERS_PER_DEVICE + 1, |_| device_id(100)).await;
        let mut expected = vec![STORED; MAX_WRITERS_PER_DEVICE];
        expected.push(StoreOutcome::Refused(StoreRefusal::WriterLimit));
        assert_eq!(a.receive(&geneses).await, expected);
        assert_eq!(a.stored_vector().await.len(), MAX_WRITERS_PER_DEVICE);

        let other_device = mint_writers(
            MAX_WRITERS_PER_DEVICE + 1..MAX_WRITERS_PER_DEVICE + 2,
            |_| device_id(101),
        )
        .await;
        assert_eq!(a.receive(&other_device).await, vec![STORED]);
    });
}

#[test]
fn merged_versions_with_far_ahead_clocks_name_their_writer_until_local_time_catches_up() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].now_ms = START_MS + CLOCK_WARNING_AHEAD_MS + 60 * 60_000;
        seed_note(&mut replicas[0]).await;
        replicas[0].seal().await.unwrap();
        let writer = replicas[0].writer();
        let (b, a) = pair(&mut replicas, 1, 0);
        let known = b.stored_vector().await;
        let ops = a.ops_since(&known).await;
        b.receive(&ops).await;
        let reports = b.apply_all().await.unwrap();
        assert!(reports.iter().any(|report| {
            report
                .clock_warnings
                .iter()
                .any(|warning| warning.writer == writer)
        }));

        let warnings = b
            .engine
            .far_ahead_writers(&mut b.conn, &b.ctx, b.now_ms)
            .await
            .unwrap();
        assert_eq!(
            warnings
                .iter()
                .map(|warning| warning.writer)
                .collect::<Vec<_>>(),
            vec![writer]
        );
        let edge = warnings[0].clock.physical_ms() - CLOCK_WARNING_AHEAD_MS;
        let still_ahead = b
            .engine
            .far_ahead_writers(&mut b.conn, &b.ctx, edge - 1)
            .await
            .unwrap();
        assert_eq!(still_ahead, warnings);
        let caught_up = b
            .engine
            .far_ahead_writers(&mut b.conn, &b.ctx, edge)
            .await
            .unwrap();
        assert!(caught_up.is_empty());
    });
}

const MEMBERSHIP_PARENTS: &str = "INSERT INTO fixture_items (id, created_at, label, label_edits)
         VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
     INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'A');";

/// SQL adding the membership of `item` in `end` with `role`.
fn add_membership(item: &str, end: &str, role: &str) -> String {
    format!(
        "INSERT INTO fixture_memberships (item_id, end_id, sync_key, created_at, role)
         VALUES ('{item}', '{end}', '{}', '2027-01-01T00:00:00.000Z', '{role}')",
        membership_key(item, end)
    )
}

async fn memberships(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT item_id || '>' || end_id || ':' || role FROM fixture_memberships ORDER BY sync_key",
    )
    .await
}

async fn membership_rows(replica: &mut Replica, sql: &str) -> i64 {
    replica
        .count(&sql.replace("{table}", &MEMBERSHIPS_TABLE.0.to_string()))
        .await
}

#[test]
fn a_derived_key_removed_on_one_device_is_added_again_on_another() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(MEMBERSHIP_PARENTS).await;
        replicas[0]
            .exec(&add_membership("item-1", "a-1", "first"))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_memberships WHERE item_id = 'item-1'")
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert!(memberships(replica).await.is_empty());
        }

        // No recreate guard refuses the published key, and the re-add writes every group.
        replicas[1]
            .exec(&add_membership("item-1", "a-1", "again"))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memberships(replica).await, ["item-1>a-1:again"]);
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

#[test]
fn a_derived_key_removed_and_edited_concurrently_converges() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0].exec(MEMBERSHIP_PARENTS).await;
        replicas[0]
            .exec(&add_membership("item-1", "a-1", "first"))
            .await;
        converge(&mut replicas).await;
        replicas[0]
            .exec("DELETE FROM fixture_memberships WHERE item_id = 'item-1'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_memberships SET role = 'edited' WHERE item_id = 'item-1'")
            .await;
        replicas[2]
            .exec(&format!(
                "DELETE FROM fixture_memberships WHERE item_id = 'item-1'; {}",
                add_membership("item-1", "a-1", "readded")
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let rows = memberships(&mut replicas[0]).await;
        assert!(rows.len() <= 1, "{rows:?}");
    });
}

#[test]
fn a_derived_key_row_is_hidden_while_its_identity_target_is_a_hidden_duplicate() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec("INSERT INTO fixture_cycle_a (id, label) VALUES ('a-1', 'Same')")
            .await;
        replicas[1]
            .exec(&format!(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
                 INSERT INTO fixture_cycle_a (id, label) VALUES ('a-2', 'Same');
                 {};",
                add_membership("item-1", "a-2", "member")
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let key = membership_key("item-1", "a-2");
        for replica in &mut replicas {
            // The identity reference is not redirected to the kept duplicate.
            assert!(memberships(replica).await.is_empty());
            assert_eq!(
                replica
                    .count(&format!(
                        "SELECT count(*) FROM sync_rows WHERE row_key = '{key}' AND state = 'live'"
                    ))
                    .await,
                1
            );
        }
    });
}

#[test]
fn derived_key_pending_placeholders_are_refused_at_startup_and_rekeyed_once() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0].exec(MEMBERSHIP_PARENTS).await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_memberships (item_id, end_id, sync_key, created_at, role)
                 VALUES ('item-1', 'a-1', 'pending:1', '2027-01-01T00:00:00.000Z', 'member')",
            )
            .await;
        let replica = &mut replicas[0];
        let refused = replica
            .engine
            .init_space(&mut replica.conn, VAULT_ID, START_MS)
            .await;
        assert!(
            matches!(refused, Err(SyncError::PendingKey("fixture_memberships"))),
            "{refused:?}"
        );
        let report = replica.seal().await.unwrap();
        assert_eq!(
            report
                .invalid_rows
                .iter()
                .map(|row| (row.row_key.as_str(), row.reason))
                .collect::<Vec<_>>(),
            [("pending:1", "pending key")]
        );
        for refused in ["pending:2", ""] {
            assert!(
                !replica
                    .try_exec(&format!(
                        "UPDATE fixture_memberships SET sync_key = '{refused}'"
                    ))
                    .await,
                "{refused}"
            );
        }

        let key = membership_key("item-1", "a-1");
        replica
            .exec(&format!(
                "UPDATE fixture_memberships SET sync_key = '{key}'"
            ))
            .await;
        replica
            .engine
            .init_space(&mut replica.conn, VAULT_ID, START_MS)
            .await
            .unwrap();
        assert!(
            !replica
                .try_exec("UPDATE fixture_memberships SET sync_key = 'pending:3'")
                .await
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(memberships(replica).await, ["item-1>a-1:member"]);
            assert_eq!(
                texts(replica, "SELECT sync_key FROM fixture_memberships").await,
                [key.as_str()]
            );
        }
    });
}

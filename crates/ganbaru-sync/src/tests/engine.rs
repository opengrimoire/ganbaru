//! Engine entry points across replicas of the vault and fixture domains.

use ganbaru_contacts::PersonKeyPair;
use ganbaru_sync_contracts::bounds::{MAX_DEPENDENCIES, MAX_WRITERS_PER_DEVICE};
use ganbaru_sync_contracts::hlc::CLOCK_WARNING_AHEAD_MS;
use ganbaru_sync_contracts::{
    Content, Envelope, Field, GroupMask, TableId, VersionVector, WriterId,
};

use super::block_on;
use super::fixture::{
    CYCLE_A_TABLE, CYCLE_B_TABLE, EDGES_TABLE, HANDLES_TABLE, ITEMS_TABLE, LINKS_TABLE,
    MEMBERSHIPS_TABLE, NODES_TABLE, TEST_MANIFEST, edge_group, edge_key, membership_key,
};
use super::replica::{
    Replica, START_MS, VAULT_ID, converge, mint_writers, pair, person, rank, spread_devices,
};
use crate::error::failpoints;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, TAGS_TABLE, note_group};
use crate::{ConflictRow, HoldReason, StoreOutcome, StoreRefusal, SyncError, SyncResult};

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
fn a_change_referencing_an_invalid_unsealed_create_stays_captured_until_it_is_fixed() {
    block_on(async {
        let mut replicas = replicas(2).await;
        // Without its value guard, the domain stores an item no device would accept.
        replicas[0]
            .exec("DROP TRIGGER sync_values_fixture_items_insert")
            .await;
        replicas[0]
            .exec(
                "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027' || char(0), 'One', 0);
                 INSERT INTO fixture_links (id, owner_id, note) VALUES ('link-1', 'item-1', 'L');
                 INSERT INTO fixture_links (id, note, parent_id) VALUES ('link-2', 'M', 'link-1');",
            )
            .await;
        let report = replicas[0].seal().await.unwrap();
        let mut invalid: Vec<(&str, &str)> = report
            .invalid_rows
            .iter()
            .map(|row| (row.row_key.as_str(), row.reason))
            .collect();
        invalid.sort_unstable();
        assert_eq!(invalid.len(), 3, "{invalid:?}");
        assert_eq!(
            invalid[1..],
            [
                ("link-1", "invalid dependency"),
                ("link-2", "invalid dependency")
            ]
        );
        let sealed = "SELECT count(*) FROM sync_rows";
        let captured = "SELECT count(*) FROM sync_capture";
        assert_eq!(replicas[0].count(sealed).await, 0);
        assert_eq!(replicas[0].count(captured).await, 3);

        replicas[0]
            .exec(
                "DELETE FROM fixture_items WHERE id = 'item-1';
                 INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('item-1', '2027-01-01T00:00:00.000Z', 'One', 0);
                 INSERT INTO fixture_links (id, owner_id, note) VALUES ('link-1', 'item-1', 'L');
                 UPDATE fixture_links SET parent_id = 'link-1' WHERE id = 'link-2';",
            )
            .await;
        let report = replicas[0].seal().await.unwrap();
        assert!(report.invalid_rows.is_empty(), "{:?}", report.invalid_rows);
        assert_eq!(replicas[0].count(captured).await, 0);
        converge(&mut replicas).await;
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM fixture_links WHERE owner_id = 'item-1'")
                .await,
            1
        );
        assert_eq!(
            replicas[1]
                .count("SELECT count(*) FROM fixture_links WHERE parent_id = 'link-1'")
                .await,
            1
        );
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
        let ops = a.own_ops().await;
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
        let template = vault.stored_vector().await;
        for replica in [&mut other, &mut untrusted] {
            replica.seal().await.unwrap();
        }
        let foreign = other.own_ops().await;
        assert_eq!(
            vault.receive(&foreign).await,
            vec![StoreOutcome::Refused(StoreRefusal::WrongSpace)]
        );
        let untrusted_ops = untrusted.own_ops().await;
        assert_eq!(
            vault.receive(&untrusted_ops).await,
            vec![StoreOutcome::Refused(StoreRefusal::Untrusted)]
        );
        assert_eq!(vault.stored_vector().await, template);
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

        let ops = a.own_ops().await;
        assert_eq!(b.receive(&ops).await, vec![STORED; 3]);
        let forked = clone.own_ops().await;
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
            let open = replica
                .engine
                .open_conflicts(&mut replica.conn, replica.ctx.space, NOTES_TABLE)
                .await
                .unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(open[0].row_key, "note-1");
            assert_eq!(open[0].groups, details);
            assert!(
                replica
                    .engine
                    .open_conflicts(&mut replica.conn, replica.ctx.space, TAGS_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
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

/// SQL inserting a node under `parent` with a label.
fn add_node(id: &str, parent: Option<&str>, position: u32, label: &str) -> String {
    format!(
        "INSERT INTO fixture_nodes (id, parent_id, position, label) VALUES ('{id}', {}, '{}', '{label}');",
        parent.map_or_else(|| "NULL".to_string(), |parent| format!("'{parent}'")),
        rank(position)
    )
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
async fn pull_without_writer(to: &mut Replica, from: &mut Replica) {
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

/// Every node as `id>parent`, with `-` for a root.
async fn node_parents(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT id || '>' || coalesce(parent_id, '-') FROM fixture_nodes ORDER BY id",
    )
    .await
}

#[test]
fn a_cycle_of_three_concurrent_moves_breaks_at_the_move_of_the_greatest_writer() {
    block_on(async {
        let mut replicas = replicas(3).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}",
                add_node("a", None, 0, "A"),
                add_node("b", None, 1, "B"),
                add_node("c", None, 2, "C"),
            ))
            .await;
        converge(&mut replicas).await;
        let moves = [("a", "b"), ("b", "c"), ("c", "a")];
        let mut ranks = Vec::new();
        for (replica, (row, parent)) in replicas.iter_mut().zip(moves) {
            replica
                .exec(&format!(
                    "UPDATE fixture_nodes SET parent_id = '{parent}' WHERE id = '{row}'"
                ))
                .await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = '{row}' AND group_id = 0",
                    NODES_TABLE.0
                ))
                .await;
            ranks.push((clock, replica.writer(), row));
        }
        // Every move carries the same clock, so the writer breaks the tie.
        assert!(ranks.iter().all(|(clock, _, _)| *clock == ranks[0].0));
        let broken = ranks.iter().max().map(|(_, _, row)| *row).unwrap();
        let expected: Vec<String> = moves
            .iter()
            .map(|(row, parent)| {
                let parent = if *row == broken { "-" } else { parent };
                format!("{row}>{parent}")
            })
            .collect();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(node_parents(replica).await, expected);
        }
    });
}

#[test]
fn concurrent_moves_that_together_exceed_the_maximum_depth_move_the_deepest_row_to_the_root() {
    block_on(async {
        let mut replicas = replicas(2).await;
        replicas[0]
            .exec(&format!(
                "{}{}{}{}",
                add_node("p", None, 0, "P"),
                add_node("q", None, 1, "Q"),
                add_node("q-c", Some("q"), 0, "Q child"),
                add_node("s", None, 2, "S"),
            ))
            .await;
        converge(&mut replicas).await;
        // Each move alone keeps every node within the maximum depth of 2.
        replicas[0]
            .exec("UPDATE fixture_nodes SET parent_id = 'p' WHERE id = 'q'")
            .await;
        replicas[1]
            .exec("UPDATE fixture_nodes SET parent_id = 'q-c' WHERE id = 's'")
            .await;
        replicas[0].seal().await.unwrap();
        replicas[1].seal().await.unwrap();
        assert_eq!(
            node_parents(&mut replicas[1]).await,
            ["p>-", "q>-", "q-c>q", "s>q-c"]
        );
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(node_parents(replica).await, ["p>-", "q>p", "q-c>q", "s>-"]);
            // The repair is sealed: the newest parent version of the moved row holds the same
            // null parent as the root `p`.
            let newest = texts(
                replica,
                &format!(
                    "SELECT quote(v.value) FROM sync_register_versions v
                     WHERE v.table_id = {} AND v.group_id = 0 AND v.row_key IN ('p', 's')
                     AND v.clock = (SELECT max(w.clock) FROM sync_register_versions w
                         WHERE w.table_id = v.table_id AND w.group_id = 0
                         AND w.row_key = v.row_key)
                     ORDER BY v.row_key",
                    NODES_TABLE.0
                ),
            )
            .await;
            assert_eq!(newest.len(), 2);
            assert_eq!(newest[0], newest[1]);
        }
    });
}

/// Every materialized edge as `from>to`.
async fn edge_rows(replica: &mut Replica) -> Vec<String> {
    texts(
        replica,
        "SELECT from_id || '>' || to_id FROM fixture_edges ORDER BY from_id, to_id",
    )
    .await
}

/// SQL adding the edge from `from` to `to`.
fn add_edge(from: &str, to: &str) -> String {
    format!(
        "INSERT INTO fixture_edges (from_id, to_id, sync_key) VALUES ('{from}', '{to}', '{}');",
        edge_key(from, to)
    )
}

#[test]
fn concurrent_edges_that_close_a_cycle_sever_the_newest_until_another_edge_is_removed() {
    block_on(async {
        let mut replicas = replicas(3).await;
        let items: String = ["x", "y", "z"]
            .iter()
            .map(|id| {
                format!(
                    "INSERT INTO fixture_items (id, created_at, label, label_edits)
                     VALUES ('{id}', '2027-01-01T00:00:00.000Z', '{id}', 0);"
                )
            })
            .collect();
        replicas[0].exec(&items).await;
        converge(&mut replicas).await;
        let added = [("x", "y"), ("y", "z"), ("z", "x")];
        let mut ranks = Vec::new();
        for (replica, (from, to)) in replicas.iter_mut().zip(added) {
            replica.exec(&add_edge(from, to)).await;
            replica.seal().await.unwrap();
            let clock = replica
                .count(&format!(
                    "SELECT max(clock) FROM sync_register_versions
                     WHERE table_id = {} AND row_key = '{}' AND group_id = {}",
                    EDGES_TABLE.0,
                    edge_key(from, to),
                    edge_group::EXISTS.get()
                ))
                .await;
            ranks.push((clock, replica.writer(), (from, to)));
        }
        let (severed_from, severed_to) = ranks.iter().max().map(|(_, _, edge)| *edge).unwrap();
        let severed = format!("{severed_from}>{severed_to}");
        let kept: Vec<String> = added
            .iter()
            .map(|(from, to)| format!("{from}>{to}"))
            .filter(|edge| *edge != severed)
            .collect();
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        for replica in &mut replicas {
            let mut edges = edge_rows(replica).await;
            edges.sort();
            assert_eq!(edges, kept);
            assert_eq!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, EDGES_TABLE)
                    .await
                    .unwrap(),
                vec![ConflictRow {
                    table: EDGES_TABLE,
                    row_key: edge_key(severed_from, severed_to),
                    groups: GroupMask::of(edge_group::EXISTS),
                }]
            );
        }
        // Removing another edge of the cycle shows the severed edge again.
        let (from, to) = kept[0].split_once('>').unwrap();
        replicas[0]
            .exec(&format!(
                "DELETE FROM fixture_edges WHERE from_id = '{from}' AND to_id = '{to}'"
            ))
            .await;
        converge(&mut replicas).await;
        assert_converged(&mut replicas).await;
        let mut expected = vec![kept[1].clone(), severed.clone()];
        expected.sort();
        for replica in &mut replicas {
            let mut edges = edge_rows(replica).await;
            edges.sort();
            assert_eq!(edges, expected);
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, EDGES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
    });
}

#[test]
fn a_reseal_past_operations_another_writer_saw_keeps_that_writers_operations() {
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
        for keep_through in [0, 4] {
            let result = b
                .engine
                .reseal_fork(
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
            .reseal_fork(&mut b.conn, &b.ctx, same, 1, &mut b.identity.local(), now)
            .await;
        assert!(matches!(result, Err(SyncError::InvalidReseal(_))));

        // The operation C saw is replaced, and C's own edit on top of it stays.
        let report = b
            .engine
            .reseal_fork(&mut b.conn, &b.ctx, old, 2, &mut b.identity.local(), now)
            .await
            .unwrap();
        assert_eq!(report.replaced_ops, 1);
        assert_eq!(b.stored_vector().await.get(&old), 2);
        assert_eq!(
            b.count("SELECT count(*) FROM quick_notes WHERE title = 'Seen' AND color = 3")
                .await,
            1
        );
        b.check_invariants().await;
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
        let ops = a.own_ops().await;
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
        let acknowledged =
            assert_rolls_back(b, "acknowledge.before_commit", async |r: &mut Replica| {
                r.engine
                    .acknowledge(&mut r.conn, &r.ctx, &mut r.identity.local(), r.now_ms)
                    .await
            })
            .await;
        assert_eq!(
            acknowledged.sealed_ops, 2,
            "the genesis and the acknowledgement"
        );

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

        let mut anchor = Replica::anchor(3, 1).await;
        anchor.pull(c).await;
        let device = c.device_id.clone();
        assert_rolls_back(
            &mut anchor,
            "sequence.before_commit",
            async |r: &mut Replica| r.remove(&device).await,
        )
        .await;
    });
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
        let minted_writers = mint_writers(0..minted, spread_devices).await;
        let (a, b) = replicas.split_at_mut(1);
        let (a, b) = (&mut a[0], &mut b[0]);
        minted_writers.enroll(a).await;
        assert_eq!(
            a.receive(&minted_writers.geneses).await,
            vec![STORED; minted]
        );
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
                // The rest of the minted writers and the authority writer.
                (minted + 1 - MAX_DEPENDENCIES, false)
            ]
        );
        assert_eq!(a.applied_vector().await.len(), minted + 2);

        minted_writers.enroll(b).await;
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
        let minted = mint_writers(0..MAX_DEPENDENCIES + 1, spread_devices).await;
        minted.enroll(&mut clone).await;
        clone.receive(&minted.geneses).await;
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
        let geneses = mint_writers(0..MAX_WRITERS_PER_DEVICE + 1, |_| 10)
            .await
            .geneses;
        let mut expected = vec![STORED; MAX_WRITERS_PER_DEVICE];
        expected.push(StoreOutcome::Refused(StoreRefusal::WriterLimit));
        assert_eq!(a.receive(&geneses).await, expected);
        // The minted writers and the authority writer of the space template.
        assert_eq!(a.stored_vector().await.len(), MAX_WRITERS_PER_DEVICE + 1);

        let other_device = mint_writers(
            MAX_WRITERS_PER_DEVICE + 1..MAX_WRITERS_PER_DEVICE + 2,
            |_| 11,
        )
        .await
        .geneses;
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

fn image_row(note: &str, position: u32, digest: char, content_type: &str, size: u32) -> String {
    format!(
        "INSERT INTO quick_note_images
             (note_id, sort_order, sha256, content_type, byte_size, width, height)
         VALUES ('{note}', {position}, '{}', '{content_type}', {size}, 1, 1);",
        digest.to_string().repeat(64)
    )
}

#[test]
fn referenced_assets_cover_live_rows_and_retained_versions() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        replica
            .exec(&format!(
                "INSERT INTO quick_notes (id, order_key) VALUES ('note-1', 'a0'), ('note-2', 'a1');
                 {}{}",
                image_row("note-1", 0, 'a', "image/png", 10),
                image_row("note-1", 1, 'b', "image/jpeg", 20),
            ))
            .await;
        replica.seal().await.unwrap();
        // The sealed list keeps referencing the removed image while its version is stored.
        replica
            .exec(&format!(
                "DELETE FROM quick_note_images WHERE note_id = 'note-1' AND sort_order = 0;
                 UPDATE quick_note_images SET sort_order = 0 WHERE note_id = 'note-1';
                 {}{}",
                image_row("note-2", 0, 'c', "image/webp", 30),
                image_row("note-2", 1, 'b', "image/jpeg", 20),
            ))
            .await;

        let referenced = replica
            .engine
            .referenced_assets(&mut replica.conn)
            .await
            .unwrap();
        let names = referenced
            .iter()
            .map(|asset| (asset.class().name, asset.file_name(), asset.byte_size()))
            .collect::<Vec<_>>();
        let digest = |character: char| character.to_string().repeat(64);
        assert_eq!(
            names,
            [
                ("quick-notes/images", format!("{}.png", digest('a')), 10),
                ("quick-notes/images", format!("{}.jpg", digest('b')), 20),
                ("quick-notes/images", format!("{}.webp", digest('c')), 30),
            ]
        );
    });
}

#[test]
fn referenced_assets_keep_references_that_disagree_on_one_digest() {
    block_on(async {
        let mut replica = Replica::new(0, 1).await;
        // Whichever row is read last must not hide the other description of the digest.
        replica
            .exec(&format!(
                "INSERT INTO quick_notes (id, order_key) VALUES ('note-1', 'a0'), ('note-2', 'a1'),
                     ('note-3', 'a2');
                 {}{}{}",
                image_row("note-1", 0, 'a', "image/png", 10),
                image_row("note-2", 0, 'a', "image/png", 11),
                image_row("note-3", 0, 'a', "image/jpeg", 10),
            ))
            .await;

        let referenced = replica
            .engine
            .referenced_assets(&mut replica.conn)
            .await
            .unwrap();
        let names = referenced
            .iter()
            .map(|asset| (asset.file_name(), asset.byte_size()))
            .collect::<Vec<_>>();
        let digest = "a".repeat(64);
        assert_eq!(
            names,
            [
                (format!("{digest}.jpg"), 10),
                (format!("{digest}.png"), 10),
                (format!("{digest}.png"), 11),
            ]
        );
    });
}

#[test]
fn asset_tables_name_the_rows_that_hold_a_class() {
    let engine = crate::Engine::vault();
    assert_eq!(
        engine.asset_tables(&crate::manifest::vault::quick_notes::IMAGES_CLASS),
        ["quick_note_images"]
    );
    let unregistered = crate::assets::AssetClass {
        name: "unregistered",
        max_bytes: 1,
    };
    assert!(engine.asset_tables(&unregistered).is_empty());
}

#[test]
fn recovery_root_tables_follow_cascades_and_leave_out_derived_keys() {
    let roots: Vec<TableId> = TEST_MANIFEST
        .recovery_root_tables()
        .into_iter()
        .map(|spec| spec.id)
        .collect();
    // Memberships are recoverable but derived-key, so their cascade parents root their offers.
    assert_eq!(
        roots,
        vec![
            NOTES_TABLE,
            crate::manifest::vault::themes::THEMES_TABLE,
            crate::manifest::vault::music::ITEMS_TABLE,
            crate::manifest::vault::music::PLAYLISTS_TABLE,
            // Groups, sections, and statuses cascade into projects and tasks.
            crate::manifest::vault::projects::GROUPS_TABLE,
            crate::manifest::vault::projects::PROJECTS_TABLE,
            crate::manifest::vault::projects::SECTIONS_TABLE,
            crate::manifest::vault::projects::STATUSES_TABLE,
            crate::manifest::vault::projects::TASKS_TABLE,
            // Calendars, objects, components, and events carry recoverable groups.
            crate::manifest::vault::calendar::CALENDARS_TABLE,
            crate::manifest::vault::calendar::OBJECTS_TABLE,
            crate::manifest::vault::calendar::COMPONENTS_TABLE,
            crate::manifest::vault::calendar::EVENTS_TABLE,
            // Settings, custom categories, and usage limits carry recoverable groups.
            crate::manifest::vault::distractions::SETTINGS_TABLE,
            crate::manifest::vault::distractions::CUSTOM_CATEGORIES_TABLE,
            crate::manifest::vault::distractions::USAGE_LIMITS_TABLE,
            // Participants and teammates carry recoverable names and roles.
            crate::manifest::vault::chat::PARTICIPANTS_TABLE,
            crate::manifest::vault::chat::TEAMMATES_TABLE,
            // Channels carry recoverable names, and their conversations, like those of the
            // derived-key memberships and grants, root the offers that cascade into them.
            crate::manifest::vault::chat::CONVERSATIONS_TABLE,
            crate::manifest::vault::chat::CHANNELS_TABLE,
            ITEMS_TABLE,
            CYCLE_A_TABLE,
            CYCLE_B_TABLE,
            NODES_TABLE
        ]
    );
    assert!(!roots.contains(&MEMBERSHIPS_TABLE));
}

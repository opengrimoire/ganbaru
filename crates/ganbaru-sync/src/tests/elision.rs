//! Winning values elided from merge state and captured groups elided at seal.

use ganbaru_sync_contracts::{Field, GroupId};

use super::block_on;
use super::replica::{Replica, converge};
use crate::local::retained_group_values;
use crate::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use crate::merge;

async fn replicas(count: usize) -> Vec<Replica> {
    let mut replicas = Vec::with_capacity(count);
    for index in 0..count {
        replicas.push(Replica::new(index, 11).await);
    }
    replicas
}

/// Two replicas sharing `note-1` with a title, a tag, and one body run.
async fn shared_note() -> Vec<Replica> {
    let mut replicas = replicas(2).await;
    replicas[0]
        .exec(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
             INSERT INTO quick_notes (id, title, tag_id, order_key, body_plain_text)
                 VALUES ('note-1', 'One', 'tag-1', 'a0', 'body');
             INSERT INTO quick_note_text_runs (note_id, sort_order, content)
                 VALUES ('note-1', 0, 'body');",
        )
        .await;
    replicas[0].seal().await.unwrap();
    converge(&mut replicas).await;
    replicas
}

/// Whether the stored value of `group` of `note-1` by any writer is null.
async fn elided(replica: &mut Replica, group: GroupId) -> bool {
    replica
        .count(&format!(
            "SELECT count(*) FROM sync_register_versions
             WHERE table_id = {} AND row_key = 'note-1' AND group_id = {} AND value IS NULL",
            NOTES_TABLE.0,
            group.get()
        ))
        .await
        > 0
}

/// Retained title texts of `note-1`, read through the engine.
async fn retained_titles(replica: &mut Replica) -> Vec<String> {
    let versions = merge::versions(&replica.engine, &mut replica.conn, NOTES_TABLE, "note-1")
        .await
        .unwrap();
    versions
        .into_iter()
        .filter(|version| version.group == note_group::TITLE)
        .map(|version| match version.value.fields() {
            [Field::Text(text)] => text.clone(),
            other => panic!("unexpected title value {other:?}"),
        })
        .collect()
}

#[test]
fn winning_values_the_domain_row_holds_are_elided_and_read_back() {
    block_on(async {
        let mut replicas = shared_note().await;
        for replica in &mut replicas {
            assert!(elided(replica, note_group::TITLE).await);
            assert!(
                elided(replica, note_group::BODY).await,
                "owned groups elide"
            );
            assert!(
                !elided(replica, note_group::TAG).await,
                "a reference group keeps its value for table-wide reads"
            );
            assert_eq!(retained_titles(replica).await, ["One"]);
            let bodies = retained_group_values(
                &replica.engine,
                &mut replica.conn,
                NOTES_TABLE,
                note_group::BODY,
            )
            .await
            .unwrap();
            assert_eq!(bodies.len(), 1);
            let (stored, full) = replica.check_version_values().await;
            assert!(stored < full);
        }
    });
}

#[test]
fn a_pending_edit_reads_the_elided_winner_from_its_operation() {
    block_on(async {
        let mut replicas = shared_note().await;
        let editor = &mut replicas[0];
        editor
            .exec("UPDATE quick_notes SET title = 'Unsealed' WHERE id = 'note-1'")
            .await;
        assert!(elided(editor, note_group::TITLE).await);
        assert_eq!(retained_titles(editor).await, ["One"]);
        editor.check_version_values().await;
        editor.seal().await.unwrap();
        assert_eq!(retained_titles(editor).await, ["Unsealed"]);
        assert!(elided(editor, note_group::TITLE).await);
    });
}

#[test]
fn a_captured_group_equal_to_its_uncontested_winner_is_not_sealed() {
    block_on(async {
        let mut replicas = shared_note().await;
        let editor = &mut replicas[0];
        editor
            .exec(
                "UPDATE quick_notes SET title = 'Two' WHERE id = 'note-1';
                 UPDATE quick_notes SET title = 'One' WHERE id = 'note-1';",
            )
            .await;
        assert_eq!(editor.count("SELECT count(*) FROM sync_capture").await, 1);
        let report = editor.seal().await.unwrap();
        assert_eq!(report.sealed_ops, 0);
        assert_eq!(report.changes, 0);
        assert_eq!(editor.count("SELECT count(*) FROM sync_capture").await, 0);
        editor
            .exec(
                "UPDATE quick_notes SET title = 'Two', color = 3 WHERE id = 'note-1';
                 UPDATE quick_notes SET title = 'One' WHERE id = 'note-1';",
            )
            .await;
        let before = editor.seal_snapshot().await;
        assert_eq!(editor.seal().await.unwrap().changes, 1);
        editor.check_sealed_writes(&before).await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(
                replica
                    .count("SELECT color FROM quick_notes WHERE id = 'note-1'")
                    .await,
                3
            );
            assert_eq!(retained_titles(replica).await, ["One"]);
        }
    });
}

#[test]
fn a_captured_group_equal_to_one_side_of_an_open_conflict_is_sealed() {
    block_on(async {
        let mut replicas = shared_note().await;
        replicas[0]
            .exec("UPDATE quick_notes SET title = 'From A' WHERE id = 'note-1'")
            .await;
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'From B' WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        let editor = &mut replicas[0];
        let shown = editor
            .scalar_text("SELECT title FROM quick_notes WHERE id = 'note-1'")
            .await
            .unwrap();
        assert_eq!(retained_titles(editor).await.len(), 2);
        editor
            .exec(&format!(
                "UPDATE quick_notes SET title = 'Other' WHERE id = 'note-1';
                 UPDATE quick_notes SET title = '{shown}' WHERE id = 'note-1';"
            ))
            .await;
        assert_eq!(editor.seal().await.unwrap().changes, 1);
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert!(
                replica
                    .engine
                    .conflicts(&mut replica.conn, NOTES_TABLE)
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(retained_titles(replica).await, std::slice::from_ref(&shown));
        }
    });
}

#[test]
fn a_deleted_row_stores_its_elided_values_again() {
    block_on(async {
        let mut replicas = shared_note().await;
        replicas[1]
            .exec("UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-1'")
            .await;
        replicas[0]
            .exec("DELETE FROM quick_notes WHERE id = 'note-1'")
            .await;
        converge(&mut replicas).await;
        for replica in &mut replicas {
            assert_eq!(replica.count("SELECT count(*) FROM quick_notes").await, 0);
            assert_eq!(
                replica
                    .count(
                        "SELECT count(*) FROM sync_register_versions
                         WHERE row_key = 'note-1' AND value IS NULL"
                    )
                    .await,
                0
            );
            replica.check_version_values().await;
            let entries = replica
                .engine
                .recovery_entries(&mut replica.conn, &replica.ctx)
                .await
                .unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].presentation.title, "Edited");
        }
    });
}

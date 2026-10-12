//! Capture triggers and the key, creation time, and re-create guards.

use sqlx::SqlitePool;

use super::{block_on, execute, migrated_pool, rejects, set_applying};

/// `(table_id, row_key, mask, created, deleted)`.
type Capture = (i64, String, i64, i64, i64);

async fn captures(pool: &SqlitePool) -> Vec<Capture> {
    sqlx::query_as(
        "SELECT table_id, row_key, mask, created, deleted FROM sync_capture
         ORDER BY table_id, row_key",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn clear(pool: &SqlitePool) {
    execute(pool, "DELETE FROM sync_capture").await;
}

async fn seed(pool: &SqlitePool) {
    execute(
        pool,
        "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0'), ('tag-2', 'Home', 'a1');
         INSERT INTO quick_notes (id, title, tag_id, order_key) VALUES ('note-1', 'One', 'tag-1', 'a0');
         INSERT INTO quick_notes (id, title, order_key) VALUES ('note-2', 'Two', 'a1');
         INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('note-1', 0, 'body');",
    )
    .await;
    clear(pool).await;
}

fn note(mask: i64) -> Capture {
    (2, "note-1".to_string(), mask, 0, 0)
}

#[test]
fn inserts_capture_created_rows_with_every_group() {
    block_on(async {
        let pool = migrated_pool().await;
        execute(
            &pool,
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
             INSERT INTO quick_notes (id, order_key) VALUES ('note-1', 'a0');",
        )
        .await;
        assert_eq!(
            captures(&pool).await,
            [
                (1, "tag-1".to_string(), 15, 1, 0),
                (2, "note-1".to_string(), 255, 1, 0),
            ]
        );
    });
}

#[test]
fn updates_capture_one_bit_per_changed_group() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        for (sql, mask) in [
            (
                "UPDATE quick_notes SET title = 'New' WHERE id = 'note-1'",
                2,
            ),
            ("UPDATE quick_notes SET color = 3 WHERE id = 'note-1'", 8),
            (
                "UPDATE quick_notes SET tag_id = 'tag-2' WHERE id = 'note-1'",
                16,
            ),
            (
                "UPDATE quick_notes SET tag_id = NULL WHERE id = 'note-1'",
                16,
            ),
            ("UPDATE quick_notes SET pinned = 1 WHERE id = 'note-1'", 32),
            (
                "UPDATE quick_notes SET pinned = 0, archived = 1 WHERE id = 'note-1'",
                32,
            ),
            (
                "UPDATE quick_notes SET archived = 0, trashed_at = '2026-10-08T00:00:00Z' WHERE id = 'note-1'",
                32,
            ),
            (
                "UPDATE quick_notes SET order_key = 'a0V' WHERE id = 'note-1'",
                64,
            ),
            (
                "UPDATE quick_notes SET updated_at = '2026-10-08T00:00:01Z' WHERE id = 'note-1'",
                128,
            ),
            (
                "UPDATE quick_notes SET title = 'Both', color = 4, updated_at = '2026-10-08T00:00:02Z' WHERE id = 'note-1'",
                2 | 8 | 128,
            ),
        ] {
            execute(&pool, sql).await;
            assert_eq!(captures(&pool).await, [note(mask)], "{sql}");
            clear(&pool).await;
        }
    });
}

#[test]
fn local_and_derived_columns_and_unchanged_values_are_not_captured() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        execute(
            &pool,
            "UPDATE quick_notes SET revision = revision + 1, body_plain_text = 'derived' WHERE id = 'note-1';
             UPDATE quick_notes SET title = 'One', color = color WHERE id = 'note-1';
             UPDATE quick_note_tags SET name = 'Work' WHERE id = 'tag-1';",
        )
        .await;
        assert_eq!(captures(&pool).await, []);
    });
}

#[test]
fn case_only_renames_are_captured_despite_nocase() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        execute(
            &pool,
            "UPDATE quick_note_tags SET name = 'WORK' WHERE id = 'tag-1'",
        )
        .await;
        assert_eq!(captures(&pool).await, [(1, "tag-1".to_string(), 2, 0, 0)]);
    });
}

#[test]
fn updates_merge_into_pending_captures() {
    block_on(async {
        let pool = migrated_pool().await;
        execute(
            &pool,
            "INSERT INTO quick_notes (id, order_key) VALUES ('note-1', 'a0');
             UPDATE quick_notes SET title = 'Later' WHERE id = 'note-1';",
        )
        .await;
        assert_eq!(
            captures(&pool).await,
            [(2, "note-1".to_string(), 255, 1, 0)]
        );
        clear(&pool).await;
        execute(
            &pool,
            "UPDATE quick_notes SET title = 'Again' WHERE id = 'note-1';
             UPDATE quick_notes SET color = 1 WHERE id = 'note-1';",
        )
        .await;
        assert_eq!(captures(&pool).await, [note(2 | 8)]);
    });
}

#[test]
fn deletes_capture_deleted_rows() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        execute(
            &pool,
            "UPDATE quick_notes SET title = 'Edited' WHERE id = 'note-2'",
        )
        .await;
        execute(
            &pool,
            "DELETE FROM quick_notes WHERE id IN ('note-1', 'note-2')",
        )
        .await;
        let deleted = captures(&pool)
            .await
            .into_iter()
            .map(|(table, key, _, created, deleted)| (table, key, created, deleted))
            .collect::<Vec<_>>();
        assert_eq!(
            deleted,
            [
                (2, "note-1".to_string(), 0, 1),
                (2, "note-2".to_string(), 0, 1),
            ]
        );
        let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM quick_note_text_runs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(runs, 0);
    });
}

#[test]
fn deleting_then_recreating_an_unpublished_row_keeps_one_capture() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        execute(
            &pool,
            "DELETE FROM quick_notes WHERE id = 'note-2';
             INSERT INTO quick_notes (id, order_key) VALUES ('note-2', 'a2');",
        )
        .await;
        assert_eq!(
            captures(&pool).await,
            [(2, "note-2".to_string(), 255, 1, 0)]
        );
    });
}

#[test]
fn tag_deletion_does_not_capture_the_foreign_key_action() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        execute(&pool, "DELETE FROM quick_note_tags WHERE id = 'tag-1'").await;
        let tag_id: Option<String> =
            sqlx::query_scalar("SELECT tag_id FROM quick_notes WHERE id = 'note-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(tag_id, None);
        assert_eq!(captures(&pool).await, [(1, "tag-1".to_string(), 0, 0, 1)]);
    });
}

#[test]
fn text_runs_mark_their_note_body() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        for sql in [
            "INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('note-1', 1, 'more')",
            "UPDATE quick_note_text_runs SET bold = 1 WHERE note_id = 'note-1' AND sort_order = 1",
            "DELETE FROM quick_note_text_runs WHERE note_id = 'note-1' AND sort_order = 1",
        ] {
            execute(&pool, sql).await;
            assert_eq!(captures(&pool).await, [note(4)], "{sql}");
            clear(&pool).await;
        }
        execute(
            &pool,
            "UPDATE quick_note_text_runs SET note_id = 'note-2' WHERE note_id = 'note-1'",
        )
        .await;
        assert_eq!(
            captures(&pool).await,
            [note(4), (2, "note-2".to_string(), 4, 0, 0)]
        );
    });
}

#[test]
fn applying_suppresses_capture() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        set_applying(&pool, true).await;
        execute(
            &pool,
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-3', 'Later', 'a2');
             UPDATE quick_notes SET title = 'Remote', tag_id = 'tag-3' WHERE id = 'note-1';
             INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('note-1', 1, 'x');
             DELETE FROM quick_notes WHERE id = 'note-2';
             DELETE FROM quick_note_tags WHERE id = 'tag-2';",
        )
        .await;
        assert_eq!(captures(&pool).await, []);
        set_applying(&pool, false).await;
        execute(
            &pool,
            "UPDATE quick_notes SET title = 'Local' WHERE id = 'note-1'",
        )
        .await;
        assert_eq!(captures(&pool).await, [note(2)]);
    });
}

#[test]
fn keys_never_change_and_creation_times_change_only_while_applying() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        assert!(
            rejects(
                &pool,
                "UPDATE quick_notes SET id = 'renamed' WHERE id = 'note-1'"
            )
            .await
        );
        assert!(
            rejects(
                &pool,
                "UPDATE quick_note_tags SET id = 'TAG-1' WHERE id = 'tag-1'"
            )
            .await
        );
        assert!(
            rejects(
                &pool,
                "UPDATE quick_notes SET created_at = '2020-01-01T00:00:00Z' WHERE id = 'note-1'"
            )
            .await
        );
        set_applying(&pool, true).await;
        assert!(
            rejects(
                &pool,
                "UPDATE quick_notes SET id = 'renamed' WHERE id = 'note-1'"
            )
            .await
        );
        execute(
            &pool,
            "UPDATE quick_notes SET created_at = '2020-01-01T00:00:00Z' WHERE id = 'note-1'",
        )
        .await;
    });
}

#[test]
fn published_keys_that_are_gone_cannot_be_created_again() {
    block_on(async {
        let pool = migrated_pool().await;
        execute(
            &pool,
            "INSERT INTO sync_rows (table_id, row_key, state) VALUES (2, 'gone', 'tombstoned'), (1, 'moved', 'live')",
        )
        .await;
        assert!(
            rejects(
                &pool,
                "INSERT INTO quick_notes (id, order_key) VALUES ('gone', 'a0')"
            )
            .await
        );
        assert!(
            rejects(
                &pool,
                "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('moved', 'Moved', 'a0')"
            )
            .await
        );
        execute(
            &pool,
            "INSERT INTO quick_notes (id, order_key) VALUES ('moved', 'a0')",
        )
        .await;
        set_applying(&pool, true).await;
        execute(
            &pool,
            "INSERT INTO quick_notes (id, order_key) VALUES ('gone', 'a0')",
        )
        .await;
    });
}

use super::helpers::{memory_pool_migrated_through, migrated_memory_pool};
use crate::run_migrations;

/// Last migration before Quick notes replicate.
const BEFORE_REPLICATION: i64 = 20261007120000;

#[test]
fn schema_creates_normalized_quick_notes_storage() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in [
            "quick_note_tags",
            "quick_notes",
            "quick_note_text_runs",
            "quick_notes_search_fts",
        ] {
            let exists: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = ?")
                    .bind(table)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, 1, "missing {table}");
        }
        let invalid_color = sqlx::query(
            "INSERT INTO quick_notes (id, color, order_key) VALUES ('bad-color', 32, 'a0')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_color.is_err());
        sqlx::query("INSERT INTO quick_notes (id, order_key) VALUES ('default-color', 'a0')")
            .execute(&pool)
            .await
            .unwrap();
        let default_color: i64 =
            sqlx::query_scalar("SELECT color FROM quick_notes WHERE id = 'default-color'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(default_color, 30);
        let null_color =
            sqlx::query("UPDATE quick_notes SET color = NULL WHERE id = 'default-color'")
                .execute(&pool)
                .await;
        assert!(null_color.is_err());
        let missing_order = sqlx::query("INSERT INTO quick_notes (id) VALUES ('unordered')")
            .execute(&pool)
            .await;
        assert!(missing_order.is_err());
        let invalid_run = sqlx::query(
            "INSERT INTO quick_note_text_runs (note_id, sort_order, content) VALUES ('missing', 0, 'text')",
        )
        .execute(&pool)
        .await;
        assert!(invalid_run.is_err());
        sqlx::query(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag', 'Work', 'a0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let duplicate_name = sqlx::query(
            "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-2', 'work', 'a1')",
        )
        .execute(&pool)
        .await;
        assert!(duplicate_name.is_err());
        let missing_tag_order =
            sqlx::query("INSERT INTO quick_note_tags (id, name) VALUES ('tag-3', 'Later')")
                .execute(&pool)
                .await;
        assert!(missing_tag_order.is_err());
    });
}

#[test]
fn tags_have_no_count_limit() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for index in 0..12 {
            sqlx::query("INSERT INTO quick_note_tags (id, name, order_key) VALUES (?, ?, 'a0')")
                .bind(format!("tag-{index}"))
                .bind(format!("Tag {index}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM quick_note_tags")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 12);
    });
}

#[test]
fn replication_migration_preserves_quick_notes() {
    super::block_on(async {
        let pool = memory_pool_migrated_through(BEFORE_REPLICATION).await;
        for (id, name, sort_order) in [
            ("tag-b", "Later", 2),
            ("tag-a", "Work", 0),
            ("tag-c", "Home", 1),
        ] {
            sqlx::query("INSERT INTO quick_note_tags (id, name, sort_order) VALUES (?, ?, ?)")
                .bind(id)
                .bind(name)
                .bind(sort_order)
                .execute(&pool)
                .await
                .unwrap();
        }
        let notes = [
            ("note-1", Some("tag-a"), 3.5, 0, "alpha"),
            ("note-2", None, -2048.0, 1, "bravo"),
            ("note-3", Some("tag-b"), 0.25, 0, "charlie"),
            ("note-4", Some("tag-a"), 0.25, 0, "delta"),
        ];
        for (id, tag_id, manual_order, pinned, body) in notes {
            sqlx::query(
                "INSERT INTO quick_notes (id, title, body_plain_text, tag_id, pinned, manual_order)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(id)
            .bind(format!("Title {id}"))
            .bind(body)
            .bind(tag_id)
            .bind(pinned)
            .bind(manual_order)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO quick_note_text_runs (note_id, sort_order, content, bold)
                 VALUES (?, 0, ?, 1)",
            )
            .bind(id)
            .bind(body)
            .execute(&pool)
            .await
            .unwrap();
        }

        run_migrations(&pool).await.unwrap();

        let tags: Vec<(String, String)> =
            sqlx::query_as("SELECT id, order_key FROM quick_note_tags ORDER BY order_key, id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            tags,
            [
                ("tag-a".to_string(), "c000".to_string()),
                ("tag-c".to_string(), "c001".to_string()),
                ("tag-b".to_string(), "c002".to_string()),
            ]
        );
        let converted: Vec<(String, Option<String>, String)> =
            sqlx::query_as("SELECT id, tag_id, order_key FROM quick_notes ORDER BY order_key")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            converted,
            [
                ("note-2".to_string(), None, "c000".to_string()),
                (
                    "note-3".to_string(),
                    Some("tag-b".to_string()),
                    "c001".to_string()
                ),
                (
                    "note-4".to_string(),
                    Some("tag-a".to_string()),
                    "c002".to_string()
                ),
                (
                    "note-1".to_string(),
                    Some("tag-a".to_string()),
                    "c003".to_string()
                ),
            ]
        );
        let runs: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM quick_note_text_runs WHERE bold = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(runs, 4);
        let matched: String = sqlx::query_scalar(
            "SELECT note_id FROM quick_notes_search_fts WHERE quick_notes_search_fts MATCH 'charlie'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(matched, "note-3");
        let foreign_key_errors: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(foreign_key_errors, 0);
        let manual_order_columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('quick_notes') WHERE name = 'manual_order'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(manual_order_columns, 0);

        let captures: Vec<(i64, String, i64, i64)> = sqlx::query_as(
            "SELECT table_id, row_key, mask, created FROM sync_capture ORDER BY table_id, row_key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(captures.len(), 7);
        assert!(captures.iter().all(|(table_id, _, mask, created)| {
            *created == 1 && *mask == if *table_id == 1 { 15 } else { 255 }
        }));

        // The rebuilt tag table still enforces the reference from notes.
        sqlx::query("DELETE FROM quick_note_tags WHERE id = 'tag-a'")
            .execute(&pool)
            .await
            .unwrap();
        let untagged: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM quick_notes WHERE id IN ('note-1', 'note-4') AND tag_id IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(untagged, 2);
        let dangling = sqlx::query("UPDATE quick_notes SET tag_id = 'missing' WHERE id = 'note-2'")
            .execute(&pool)
            .await;
        assert!(dangling.is_err());
    });
}

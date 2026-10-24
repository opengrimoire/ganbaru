//! Notes database sync preparation: templates, database blocks, and property storage.

use super::notes::NOTES_SYNC_STRUCTURE;

/// The Notes databases preparation migration.
const NOTES_SYNC_DATABASES: i64 = 20261024161650;

/// Inserts a page with a `child_database` block, its database, and one data source.
async fn insert_database_graph(pool: &sqlx::SqlitePool, schema: &serde_json::Value) {
    sqlx::query(
        "INSERT INTO notes_pages (id, parent_type, title, properties)
         VALUES ('page', 'workspace', 'Host', '{}')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
         VALUES ('db', 'page', 'page_id', 'page', 'child_database', '{}', 'a0')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notes_databases (id, parent_type, parent_page_id, title, title_rich_text, description)
         VALUES ('db', 'page_id', 'page', 'Tasks', '[]', '[]')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description, properties)
         VALUES ('ds', 'db', 'Tasks', '[]', '[]', ?)",
    )
    .bind(schema.to_string())
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn notes_sync_databases_migration_rekeys_templates_and_names_database_blocks() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(NOTES_SYNC_STRUCTURE).await;
        insert_database_graph(&pool, &serde_json::json!({})).await;
        // Sort orders run against the ids, and the nested block sorts before its parent's
        // sibling, so the ordinals must follow a depth-first walk in sort order.
        sqlx::raw_sql(
            "INSERT INTO notes_data_source_templates (id, data_source_id, name, properties, is_default)
               VALUES ('t-plain', 'ds', 'Plain', '{}', 0), ('t-default', 'ds', 'Default', '{\"Done\":{\"type\":\"checkbox\",\"checkbox\":true}}', 1);
             INSERT INTO notes_data_source_template_blocks
               (template_id, id, parent_type, parent_block_id, has_children, type, payload, plain_text, sort_order)
             VALUES
               ('t-default', 'z-first', 'template', NULL, 1, 'toggle', '{\"rich_text\":[]}', 'first', 10),
               ('t-default', 'a-second', 'template', NULL, 0, 'paragraph', '{\"rich_text\":[]}', 'second', 20),
               ('t-default', 'm-child', 'block_id', 'z-first', 0, 'paragraph', '{\"rich_text\":[{\"plain_text\":\"kept\"}]}', 'kept', 30),
               ('t-default', 'stray', 'block_id', 'missing', 0, 'paragraph', '{}', '', 1);
             INSERT INTO notes_page_templates (id, name) VALUES ('pt', 'Page template');
             INSERT INTO notes_page_template_blocks
               (template_id, id, parent_type, parent_block_id, has_children, type, payload, plain_text, sort_order)
             VALUES
               ('pt', 'b', 'template', NULL, 0, 'paragraph', '{\"rich_text\":[]}', 'b', 2),
               ('pt', 'a', 'template', NULL, 0, 'paragraph', '{\"rich_text\":[]}', 'a', 1);",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_DATABASES)
            .await
            .unwrap();

        let default: Option<String> = sqlx::query_scalar(
            "SELECT default_template_id FROM notes_data_sources WHERE id = 'ds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(default.as_deref(), Some("t-default"));
        let properties: String = sqlx::query_scalar(
            "SELECT properties FROM notes_data_source_templates WHERE id = 't-default'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(properties.contains("checkbox"));
        let blocks: Vec<(i64, String, Option<String>, String)> = sqlx::query_as(
            "SELECT ordinal, id, parent_block_id, payload FROM notes_data_source_template_blocks
             WHERE template_id = 't-default' ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let order = blocks
            .iter()
            .map(|(ordinal, id, _, _)| (*ordinal, id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(order, vec![(0, "z-first"), (1, "m-child"), (2, "a-second")]);
        assert_eq!(blocks[1].2.as_deref(), Some("z-first"));
        assert!(blocks[1].3.contains("kept"));
        let page_order: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notes_page_template_blocks WHERE template_id = 'pt' ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(page_order, vec!["a".to_string(), "b".to_string()]);

        let block_id: String =
            sqlx::query_scalar("SELECT block_id FROM notes_databases WHERE id = 'db'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(block_id, "db");
        let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(violations, 0);

        // Deleting the default template clears the data source default; deleting the database
        // block removes the database with it.
        sqlx::query("DELETE FROM notes_data_source_templates WHERE id = 't-default'")
            .execute(&pool)
            .await
            .unwrap();
        let default: Option<String> = sqlx::query_scalar(
            "SELECT default_template_id FROM notes_data_sources WHERE id = 'ds'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(default, None);
        sqlx::query("DELETE FROM notes_blocks WHERE id = 'db'")
            .execute(&pool)
            .await
            .unwrap();
        let databases: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_databases")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(databases, 0);
    });
}

#[test]
fn fresh_vault_template_blocks_are_keyed_by_ordinal_with_unique_ids() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO notes_page_templates (id, name) VALUES ('pt', 'Template');
             INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 0, 'root', 'template', 'toggle', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();
        for statement in [
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 0, 'other', 'template', 'paragraph', '{}')",
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 1, 'root', 'template', 'paragraph', '{}')",
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', -1, 'negative', 'template', 'paragraph', '{}')",
        ] {
            assert!(
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                    .execute(&pool)
                    .await
                    .is_err(),
                "{statement}"
            );
        }
        // The parent link is deferred, so a snapshot may list a child before its parent.
        let mut tx = pool.begin().await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, parent_block_id, type, payload)
               VALUES ('pt', 2, 'child', 'block_id', 'later', 'paragraph', '{}');
             INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 1, 'later', 'template', 'toggle', '{}');",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM notes_page_template_blocks WHERE id = 'later'")
            .execute(&pool)
            .await
            .unwrap();
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notes_page_template_blocks WHERE template_id = 'pt' ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(ids, vec!["root".to_string()], "the parent link cascades");
    });
}

#[test]
fn notes_sync_databases_migration_moves_properties_and_options_into_rows() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(NOTES_SYNC_STRUCTURE).await;
        let schema = serde_json::json!({
            "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
            "Stage": {"id": "stage", "name": "Stage", "type": "status", "status": {
                "options": [
                    {"id": "todo", "name": "To do", "color": "gray"},
                    {"id": "done", "name": "Done", "color": "green"},
                    {"id": "loose", "name": "Loose"}
                ],
                "groups": [
                    {"id": "g-open", "name": "To-do", "color": "gray", "option_ids": ["todo"]},
                    {"id": "g-closed", "name": "Complete", "color": "green", "option_ids": ["done"]}
                ]
            }},
            "Tags": {"id": "tags", "name": "Tags", "type": "multi_select", "multi_select": {
                "options": [{"id": "b", "name": "Beta", "color": "blue"}, {"id": "a", "name": "Alpha", "color": "red"}]
            }},
            "Count": {"id": "count", "name": "Count", "type": "number", "number": {"format": "dollar"}},
            "Broken": "not an object"
        });
        insert_database_graph(&pool, &schema).await;

        super::helpers::migrate_through(&pool, NOTES_SYNC_DATABASES)
            .await
            .unwrap();

        let properties: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT property_id, name, type, config, sync_key FROM notes_data_source_properties
             WHERE data_source_id = 'ds' ORDER BY order_key",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        // The stored object text is ordered by key, so the rank order is the key order, and the
        // entry that is not an object is skipped.
        let ids = properties
            .iter()
            .map(|(id, ..)| id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["count", "title", "stage", "tags"]);
        for (.., sync_key) in &properties {
            assert!(sync_key.starts_with("pending:"), "{sync_key}");
        }
        let config = |id: &str| -> serde_json::Value {
            let (.., config, _) = properties.iter().find(|row| row.0 == id).unwrap();
            serde_json::from_str(config).unwrap()
        };
        assert_eq!(
            config("count"),
            serde_json::json!({"number": {"format": "dollar"}})
        );
        assert_eq!(config("tags"), serde_json::json!({"multi_select": {}}));
        assert_eq!(
            config("stage"),
            serde_json::json!({"status": {"groups": [
                {"id": "g-open", "name": "To-do", "color": "gray"},
                {"id": "g-closed", "name": "Complete", "color": "green"}
            ]}})
        );

        let options: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT property_id, option_id, name, color, status_group
             FROM notes_data_source_property_options WHERE data_source_id = 'ds'
             ORDER BY property_id, position",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let option = |property: &str, id: &str, name: &str, color: &str, group: Option<&str>| {
            (
                property.to_string(),
                id.to_string(),
                name.to_string(),
                color.to_string(),
                group.map(str::to_string),
            )
        };
        assert_eq!(
            options,
            vec![
                option("stage", "todo", "To do", "gray", Some("To-do")),
                option("stage", "done", "Done", "green", Some("Complete")),
                option("stage", "loose", "Loose", "default", None),
                option("tags", "b", "Beta", "blue", None),
                option("tags", "a", "Alpha", "red", None),
            ]
        );

        let columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('notes_data_sources') WHERE name = 'properties'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(columns, 0);
        assert!(
            sqlx::query(
                "UPDATE notes_data_source_properties SET sync_key = '' WHERE property_id = 'tags'"
            )
            .execute(&pool)
            .await
            .is_err()
        );
        sqlx::query("DELETE FROM notes_data_source_properties WHERE property_id = 'tags'")
            .execute(&pool)
            .await
            .unwrap();
        let tag_options: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_data_source_property_options WHERE property_id = 'tags'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(tag_options, 0, "options cascade with their property");
    });
}

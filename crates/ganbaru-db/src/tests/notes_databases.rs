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

#[test]
fn notes_sync_databases_migration_moves_row_values_and_relation_links_into_rows() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(NOTES_SYNC_STRUCTURE).await;
        let tasks = serde_json::json!({
            "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
            "Notes": {"id": "notes", "name": "Notes", "type": "rich_text", "rich_text": {}},
            "Count": {"id": "count", "name": "Count", "type": "number", "number": {"format": "number"}},
            "Stage": {"id": "stage", "name": "Stage", "type": "select", "select": {"options": [{"id": "s1", "name": "Open", "color": "blue"}]}},
            "Tags": {"id": "tags", "name": "Tags", "type": "multi_select", "multi_select": {"options": [{"id": "t1", "name": "Home", "color": "red"}]}},
            "State": {"id": "state", "name": "State", "type": "status", "status": {"options": [{"id": "st1", "name": "Doing", "color": "green"}], "groups": []}},
            "Due": {"id": "due", "name": "Due", "type": "date", "date": {}},
            "Owner": {"id": "owner", "name": "Owner", "type": "people", "people": {}},
            "Files": {"id": "files", "name": "Files", "type": "files", "files": {}},
            "Done": {"id": "done", "name": "Done", "type": "checkbox", "checkbox": {}},
            "Link": {"id": "link", "name": "Link", "type": "url", "url": {}},
            "Mail": {"id": "mail", "name": "Mail", "type": "email", "email": {}},
            "Phone": {"id": "phone", "name": "Phone", "type": "phone_number", "phone_number": {}},
            "Calc": {"id": "calc", "name": "Calc", "type": "formula", "formula": {"expression": "1 + 1"}},
            "Projects": {"id": "projects", "name": "Projects", "type": "relation", "relation": {
                "data_source_id": "ds2",
                "dual_property": {"synced_property_id": "tasks", "synced_property_name": "Tasks"}
            }},
            "Back lead": {"id": "backlead", "name": "Back lead", "type": "relation", "relation": {"data_source_id": "ds2"}},
            "Peers": {"id": "peers", "name": "Peers", "type": "relation", "relation": {
                "data_source_id": "ds",
                "dual_property": {"synced_property_id": "peers", "synced_property_name": "Peers"}
            }},
            "Total": {"id": "total", "name": "Total", "type": "rollup", "rollup": {
                "relation_property_id": "projects", "relation_property_name": "Projects",
                "rollup_property_id": "title", "rollup_property_name": "Name", "function": "count"
            }}
        });
        insert_database_graph(&pool, &tasks).await;
        let projects = serde_json::json!({
            "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
            "Tasks": {"id": "tasks", "name": "Tasks", "type": "relation", "relation": {
                "data_source_id": "ds",
                "dual_property": {"synced_property_id": "projects", "synced_property_name": "Projects"}
            }},
            "Lead": {"id": "lead", "name": "Lead", "type": "relation", "relation": {
                "data_source_id": "ds",
                "dual_property": {"synced_property_id": "backlead", "synced_property_name": "Back lead"}
            }}
        });
        let full_row = serde_json::json!({
            "Name": {"id": "title", "type": "title", "title": [{"plain_text": "First"}]},
            "Notes": {"id": "notes", "type": "rich_text", "rich_text": [{"plain_text": "Kept text"}]},
            "Count": {"id": "count", "type": "number", "number": 5},
            "Stage": {"id": "stage", "type": "select", "select": {"id": "s1", "name": "Open", "color": "blue"}},
            "Tags": {"id": "tags", "type": "multi_select", "multi_select": [{"id": "t1", "name": "Home", "color": "red"}]},
            "State": {"id": "state", "type": "status", "status": {"id": "st1", "name": "Doing", "color": "green"}},
            "Due": {"id": "due", "type": "date", "date": {"start": "2026-10-24", "end": null}},
            "Owner": {"id": "owner", "type": "people", "people": [{"id": "user-1"}]},
            "Files": {"id": "files", "type": "files", "files": [{"name": "a.pdf", "type": "file"}]},
            "Done": {"id": "done", "type": "checkbox", "checkbox": false},
            "Link": {"id": "link", "type": "url", "url": "https://example.com"},
            "Mail": {"id": "mail", "type": "email", "email": "a@example.com"},
            "Phone": {"id": "phone", "type": "phone_number", "phone_number": "+1 555"},
            "Calc": {"id": "calc", "type": "formula", "formula": {"type": "number", "number": 2}},
            "Projects": {"id": "projects", "type": "relation", "relation": [{"id": "p1"}]},
            "Total": {"id": "total", "type": "rollup", "rollup": {"type": "number", "number": 1}},
            "Gone": {"id": "gone", "type": "rich_text", "rich_text": []}
        });
        // `Count` holds a value written before a type change, and `Done` names its property by
        // name only.
        let second_row = serde_json::json!({
            "Count": {"id": "count", "type": "rich_text", "rich_text": [{"plain_text": "five"}]},
            "Done": {"type": "checkbox", "checkbox": true}
        });
        sqlx::query(
            "INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description, properties)
             VALUES ('ds2', 'db', 'Projects', '[]', '[]', ?)",
        )
        .bind(projects.to_string())
        .execute(&pool)
        .await
        .unwrap();
        for (id, data_source, title, properties) in [
            ("r1", "ds", "First", full_row.to_string()),
            ("r2", "ds", "Second", second_row.to_string()),
            ("p1", "ds2", "Project", "{}".to_string()),
        ] {
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title, properties)
                 VALUES (?, 'data_source_id', ?, ?, ?)",
            )
            .bind(id)
            .bind(data_source)
            .bind(title)
            .bind(properties)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::raw_sql(
            "INSERT INTO notes_data_source_relation_links
               (source_page_id, source_data_source_id, source_property_id, target_page_id, target_data_source_id, created_time)
             VALUES
               ('r1', 'ds', 'projects', 'p1', 'ds2', '2026-01-01T00:00:01.000Z'),
               ('p1', 'ds2', 'tasks', 'r1', 'ds', '2026-01-01T00:00:01.000Z'),
               ('r2', 'ds', 'peers', 'r1', 'ds', '2026-01-01T00:00:02.000Z'),
               ('r1', 'ds', 'peers', 'r2', 'ds', '2026-01-01T00:00:02.000Z'),
               ('p1', 'ds2', 'lead', 'r2', 'ds', '2026-01-01T00:00:03.000Z'),
               ('r2', 'ds', 'backlead', 'p1', 'ds2', '2026-01-01T00:00:03.000Z');
             INSERT INTO notes_data_source_templates (id, data_source_id, name, properties)
               VALUES ('tpl', 'ds', 'Template', '{\"Notes\":{\"id\":\"notes\",\"type\":\"rich_text\",\"rich_text\":[{\"plain_text\":\"Template text\"}]}}');",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_DATABASES)
            .await
            .unwrap();

        let values: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT page_id, property_id, value_type, value, sync_key
             FROM notes_page_property_values ORDER BY page_id, property_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        for (.., sync_key) in &values {
            assert!(sync_key.starts_with("pending:"), "{sync_key}");
        }
        let stored = values
            .iter()
            .map(|(page, property, value_type, value, _)| {
                (
                    format!("{page}.{property}"),
                    value_type.clone(),
                    serde_json::from_str::<serde_json::Value>(value).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let mut expected = Vec::new();
        for (key, entry) in full_row.as_object().unwrap() {
            let property_type = entry["type"].as_str().unwrap();
            if ["Name", "Calc", "Projects", "Total"].contains(&key.as_str()) {
                continue;
            }
            expected.push((
                format!("r1.{}", entry["id"].as_str().unwrap()),
                property_type.to_string(),
                entry[property_type].clone(),
            ));
        }
        expected.push((
            "r2.count".to_string(),
            "rich_text".to_string(),
            serde_json::json!([{"plain_text": "five"}]),
        ));
        expected.push((
            "r2.done".to_string(),
            "checkbox".to_string(),
            serde_json::json!(true),
        ));
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(stored, expected);

        let edges: Vec<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT source_page_id, source_property_id, target_page_id, created_time, sync_key
             FROM notes_relation_edges ORDER BY source_page_id, source_property_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let edge_keys = edges
            .iter()
            .map(|(source, property, target, created, _)| {
                (
                    source.as_str(),
                    property.as_str(),
                    target.as_str(),
                    created.as_str(),
                )
            })
            .collect::<Vec<_>>();
        // Each mirrored pair collapses into one edge: under the smaller property of a two-way
        // relation, from the smaller page of a self-synced one, and under the property that the
        // one-sided synced property names.
        assert_eq!(
            edge_keys,
            vec![
                ("r1", "peers", "r2", "2026-01-01T00:00:02.000Z"),
                ("r1", "projects", "p1", "2026-01-01T00:00:01.000Z"),
                ("r2", "backlead", "p1", "2026-01-01T00:00:03.000Z"),
            ]
        );
        assert!(edges.iter().all(|edge| edge.4.starts_with("pending:")));

        let links: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT source_page_id, source_data_source_id, source_property_id, target_page_id
             FROM notes_relation_links
             ORDER BY source_page_id, source_property_id, target_page_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let link = |source: &str, data_source: &str, property: &str, target: &str| {
            (
                source.to_string(),
                data_source.to_string(),
                property.to_string(),
                target.to_string(),
            )
        };
        assert_eq!(
            links,
            vec![
                link("p1", "ds2", "lead", "r2"),
                link("p1", "ds2", "tasks", "r1"),
                link("r1", "ds", "peers", "r2"),
                link("r1", "ds", "projects", "p1"),
                link("r2", "ds", "backlead", "p1"),
                link("r2", "ds", "peers", "r1"),
            ]
        );

        let rollup: String = sqlx::query_scalar(
            "SELECT config FROM notes_data_source_properties
             WHERE data_source_id = 'ds' AND property_id = 'total' AND type = 'rollup'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let rollup: serde_json::Value = serde_json::from_str(&rollup).unwrap();
        assert_eq!(rollup["rollup"]["relation_property_id"], "projects");
        let template: String = sqlx::query_scalar(
            "SELECT properties FROM notes_data_source_templates WHERE id = 'tpl'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(template.contains("Template text"));
        let columns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('notes_pages') WHERE name = 'properties'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(columns, 0);
    });
}

#[test]
fn notes_sync_databases_migration_keeps_orphans_and_live_json_links_and_resolves_ids_first() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(NOTES_SYNC_STRUCTURE).await;
        // Both `dup` entries share an id; the first in the stored object order (`A dup`) wins.
        let schema = serde_json::json!({
            "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
            "A dup": {"id": "dup", "name": "A dup", "type": "number", "number": {}},
            "B dup": {"id": "dup", "name": "B dup", "type": "rich_text", "rich_text": {}},
            "Owner": {"id": "owner-new", "name": "Owner", "type": "rich_text", "rich_text": {}},
            "Code": {"id": "code", "name": "Code", "type": "unique_id", "unique_id": {"prefix": "T"}},
            "Made": {"id": "made", "name": "Made", "type": "created_time", "created_time": {}},
            "Links": {"id": "links", "name": "Links", "type": "relation", "relation": {"data_source_id": "ds"}}
        });
        insert_database_graph(&pool, &schema).await;
        // `Owner` was written under a deleted property that a later property reuses the name of.
        let first_row = serde_json::json!({
            "Owner": {"id": "owner-old", "type": "rich_text", "rich_text": [{"plain_text": "Old"}]},
            "Code": {"id": "code", "type": "unique_id", "unique_id": {"prefix": "T", "number": 7}},
            "Made": {"id": "made", "type": "created_time", "created_time": "2026-01-01T00:00:00.000Z"},
            "Links": {"id": "links", "type": "relation", "relation": [{"id": "r2"}, {"id": "missing"}]}
        });
        let loose_page = serde_json::json!({
            "title": {"id": "title", "type": "title", "title": [{"plain_text": "Loose"}]},
            "Extra": {"id": "extra", "type": "rich_text", "rich_text": [{"plain_text": "Kept"}]}
        });
        sqlx::query(
            "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title, properties, in_trash, last_edited_time)
             VALUES ('r1', 'data_source_id', 'ds', 'First', ?, 0, '2026-02-01T00:00:00.000Z'),
                    ('r2', 'data_source_id', 'ds', 'Trashed', '{}', 1, '2026-02-01T00:00:00.000Z'),
                    ('loose', 'workspace', NULL, 'Loose', ?, 0, '2026-02-01T00:00:00.000Z')",
        )
        .bind(first_row.to_string())
        .bind(loose_page.to_string())
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_DATABASES)
            .await
            .unwrap();

        let duplicate: (String, String) = sqlx::query_as(
            "SELECT name, type FROM notes_data_source_properties
             WHERE data_source_id = 'ds' AND property_id = 'dup'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(duplicate, ("A dup".to_string(), "number".to_string()));

        let values: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT page_id, property_id, value_type, value
             FROM notes_page_property_values ORDER BY page_id, property_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let value = |page: &str, property: &str, value_type: &str, value: serde_json::Value| {
            (
                page.to_string(),
                property.to_string(),
                value_type.to_string(),
                value.to_string(),
            )
        };
        assert_eq!(
            values,
            vec![
                value(
                    "loose",
                    "extra",
                    "rich_text",
                    serde_json::json!([{"plain_text": "Kept"}])
                ),
                value(
                    "r1",
                    "code",
                    "unique_id",
                    serde_json::json!({"number": 7, "prefix": "T"})
                ),
                value(
                    "r1",
                    "made",
                    "created_time",
                    serde_json::json!("2026-01-01T00:00:00.000Z")
                ),
                value(
                    "r1",
                    "owner-old",
                    "rich_text",
                    serde_json::json!([{"plain_text": "Old"}])
                ),
            ]
        );

        // The link only page JSON held survives because its trashed target still exists; the
        // reference to a page that does not exist is dropped.
        let edges: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT source_page_id, source_property_id, target_page_id, created_time
             FROM notes_relation_edges ORDER BY target_page_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            edges,
            vec![(
                "r1".to_string(),
                "links".to_string(),
                "r2".to_string(),
                "2026-02-01T00:00:00.000Z".to_string()
            )]
        );
    });
}

#[test]
fn fresh_vault_relation_pairs_accept_one_sided_synced_properties() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Host');
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
               VALUES ('db', 'page', 'page_id', 'page', 'child_database', '{}', 'a0');
             INSERT INTO notes_databases (id, block_id, parent_type, parent_page_id, title, title_rich_text, description)
               VALUES ('db', 'db', 'page_id', 'page', 'Tasks', '[]', '[]');
             INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description)
               VALUES ('a', 'db', 'A', '[]', '[]'), ('b', 'db', 'B', '[]', '[]');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let insert_property = |data_source: &'static str, id: &'static str, config: String| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO notes_data_source_properties
                       (data_source_id, property_id, sync_key, name, type, config, order_key)
                     VALUES (?, ?, ?, ?, 'relation', ?, ?)",
                )
                .bind(data_source)
                .bind(id)
                .bind(format!("key-{data_source}-{id}"))
                .bind(id)
                .bind(config)
                .bind(format!("a{id}"))
                .execute(&pool)
                .await
                .unwrap();
            }
        };
        let relation = |target: &str, synced: Option<&str>| {
            let mut relation = serde_json::json!({"data_source_id": target});
            if let Some(synced) = synced {
                relation["dual_property"] = serde_json::json!({
                    "synced_property_id": synced, "synced_property_name": synced
                });
            }
            serde_json::json!({ "relation": relation }).to_string()
        };
        // `a.x` names `b.y`, which names nobody; `a.z` names `b.y` too but loses to the smaller
        // `a.x`. `a.m` and `b.n` name each other. `b.w` names `a.m`, which already has a peer.
        insert_property("a", "x", relation("b", Some("y"))).await;
        insert_property("a", "z", relation("b", Some("y"))).await;
        insert_property("b", "y", relation("a", None)).await;
        insert_property("a", "m", relation("b", Some("n"))).await;
        insert_property("b", "n", relation("a", Some("m"))).await;
        insert_property("b", "w", relation("a", Some("m"))).await;

        let pairs: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT data_source_id, property_id, peer_data_source_id, peer_property_id
             FROM notes_relation_pairs ORDER BY data_source_id, property_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let pair = |ds: &str, id: &str, peer_ds: &str, peer_id: &str| {
            (
                ds.to_string(),
                id.to_string(),
                peer_ds.to_string(),
                peer_id.to_string(),
            )
        };
        assert_eq!(
            pairs,
            vec![
                pair("a", "m", "b", "n"),
                pair("a", "x", "b", "y"),
                pair("b", "n", "a", "m"),
                pair("b", "y", "a", "x"),
            ]
        );
    });
}

#[test]
fn notes_sync_databases_migration_moves_row_sub_items_onto_the_child_row() {
    super::block_on(async {
        let pool = super::helpers::memory_pool_migrated_through(NOTES_SYNC_STRUCTURE).await;
        insert_database_graph(&pool, &serde_json::json!({})).await;
        sqlx::raw_sql(
            "INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description, properties)
               VALUES ('ds2', 'db', 'Other', '[]', '[]', '{}');
             INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title, properties)
             VALUES
               ('parent', 'data_source_id', 'ds', 'Parent', '{}'),
               ('child', 'data_source_id', 'ds', 'Child', '{}'),
               ('stale', 'data_source_id', 'ds', 'Stale source', '{}'),
               ('foreign', 'data_source_id', 'ds', 'Foreign parent', '{}'),
               ('other', 'data_source_id', 'ds2', 'Other row', '{}');
             INSERT INTO notes_data_source_row_hierarchy (row_page_id, data_source_id, parent_row_page_id)
             VALUES
               ('child', 'ds', 'parent'),
               ('stale', 'ds2', 'parent'),
               ('foreign', 'ds', 'other');",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::helpers::migrate_through(&pool, NOTES_SYNC_DATABASES)
            .await
            .unwrap();

        let parents: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, parent_row_page_id FROM notes_pages
             WHERE parent_type = 'data_source_id' ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            parents,
            vec![
                ("child".to_string(), Some("parent".to_string())),
                ("foreign".to_string(), None),
                ("other".to_string(), None),
                ("parent".to_string(), None),
                ("stale".to_string(), None),
            ]
        );
        let hierarchy_table: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_schema WHERE name = 'notes_data_source_row_hierarchy'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(hierarchy_table, None);

        let self_parent =
            sqlx::query("UPDATE notes_pages SET parent_row_page_id = id WHERE id = 'child'")
                .execute(&pool)
                .await;
        assert!(self_parent.is_err(), "a row must not be its own sub-item");
        sqlx::query("DELETE FROM notes_pages WHERE id = 'parent'")
            .execute(&pool)
            .await
            .unwrap();
        let orphan: Option<String> =
            sqlx::query_scalar("SELECT parent_row_page_id FROM notes_pages WHERE id = 'child'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(orphan, None);
    });
}

//! Notes databases: templates, database blocks, property storage, and row hierarchy.

/// Inserts a page with a `child_database` block, its database, and one data source.
async fn insert_database_graph(pool: &sqlx::SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO notes_pages (id, parent_type, title) VALUES ('page', 'workspace', 'Host');
         INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, attrs, order_key)
           VALUES ('db', 'page', 'page_id', 'page', 'child_database', '{}', 'a0');
         INSERT INTO notes_databases (id, block_id, parent_type, parent_page_id, title, title_rich_text, description)
           VALUES ('db', 'db', 'page_id', 'page', 'Tasks', '[]', '[]');
         INSERT INTO notes_data_sources (id, database_id, title, title_rich_text, description)
           VALUES ('ds', 'db', 'Tasks', '[]', '[]');",
    )
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn deleting_a_default_template_clears_it_and_deleting_a_database_block_removes_the_database() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        insert_database_graph(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO notes_data_source_templates (id, data_source_id, name)
               VALUES ('t-default', 'ds', 'Default');
             UPDATE notes_data_sources SET default_template_id = 't-default' WHERE id = 'ds';
             DELETE FROM notes_data_source_templates WHERE id = 't-default';",
        )
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
fn data_source_properties_require_a_sync_key_and_own_their_options() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        insert_database_graph(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO notes_data_source_properties
               (data_source_id, property_id, sync_key, name, type, config, order_key)
               VALUES ('ds', 'tags', 'key-tags', 'Tags', 'multi_select', '{}', 'a0');
             INSERT INTO notes_data_source_property_options
               (data_source_id, property_id, property_key, option_id, sync_key, name, position)
               VALUES ('ds', 'tags', 'key-tags', 'a', 'key-a', 'Alpha', 'a0');",
        )
        .execute(&pool)
        .await
        .unwrap();

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
        let options: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_data_source_property_options WHERE property_id = 'tags'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(options, 0, "options cascade with their property");
    });
}

#[test]
fn a_row_cannot_parent_itself_and_a_deleted_parent_row_clears_its_sub_items() {
    super::block_on(async {
        let pool = super::helpers::migrated_memory_pool().await;
        insert_database_graph(&pool).await;
        sqlx::raw_sql(
            "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
               VALUES ('parent', 'data_source_id', 'ds', 'Parent');
             INSERT INTO notes_pages
               (id, parent_type, parent_data_source_id, parent_row_page_id, title)
               VALUES ('child', 'data_source_id', 'ds', 'parent', 'Child');",
        )
        .execute(&pool)
        .await
        .unwrap();

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

#[test]
fn fresh_vault_template_blocks_are_owned_rows_keyed_by_ordinal() {
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
               VALUES ('pt', -1, 'negative', 'template', 'paragraph', '{}')",
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 1, ' ', 'template', 'paragraph', '{}')",
        ] {
            assert!(
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                    .execute(&pool)
                    .await
                    .is_err(),
                "{statement}"
            );
        }
        // Block ids and parent links inside a template are checked by Notes save validation and
        // the sync adapter, not by the schema, so the owned rows of one template can arrive in
        // any order.
        sqlx::raw_sql(
            "INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, parent_block_id, type, payload)
               VALUES ('pt', 2, 'child', 'block_id', 'later', 'paragraph', '{}');
             INSERT INTO notes_page_template_blocks (template_id, ordinal, id, parent_type, type, payload)
               VALUES ('pt', 1, 'later', 'template', 'toggle', '{}');",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM notes_page_templates WHERE id = 'pt'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_page_template_blocks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0, "template blocks go with their template");
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

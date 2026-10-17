use super::helpers::*;
use crate::databases;
use crate::models::{NoteDatabaseRename, NoteDatabaseViewDuplicate, NoteDatabaseViewRename};

#[test]
fn database_creation_preserves_an_explicit_empty_title_instead_of_slash_search_text() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("paragraph", paragraph_payload("/datab")),
        )
        .await
        .unwrap();
        let created = databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: BLOCK_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: String::new(),
                parent: None,
                after_block_id: None,
                replace_block_id: Some(BLOCK_A.to_string()),
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        let created_json = serde_json::to_value(created).unwrap();
        assert_eq!(created_json["block"]["child_database"]["title"], "");
        assert_eq!(created_json["database"]["title"], "");
        assert_eq!(created_json["data_source"]["title"], "");
        let stored = reads::get_block(&pool, BLOCK_A, false).await.unwrap();
        assert_eq!(
            serde_json::to_value(stored).unwrap()["child_database"]["title"],
            ""
        );
    });
}

#[test]
fn database_titles_can_be_cleared_without_replacing_them_with_a_default() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "",
            BLOCK_A,
        )
        .await;
        for title in ["Planning", ""] {
            let saved = databases::rename_database(
                &pool,
                DATABASE_A,
                NoteDatabaseRename {
                    title: title.to_string(),
                },
            )
            .await
            .unwrap();
            assert_eq!(saved, title);
            let block = reads::get_block(&pool, DATABASE_A, false).await.unwrap();
            assert_eq!(
                serde_json::to_value(block).unwrap()["child_database"]["title"],
                title
            );
            let database_title: String =
                sqlx::query_scalar("SELECT title FROM notes_databases WHERE id = ?")
                    .bind(DATABASE_A)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            let source_title: String =
                sqlx::query_scalar("SELECT title FROM notes_data_sources WHERE id = ?")
                    .bind(DATA_SOURCE_A)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(database_title, title);
            assert_eq!(source_title, title);
        }
    });
}

#[test]
fn create_local_database_inside_notes_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let created = databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: "Tasks".to_string(),
                parent: Some(page_parent(PAGE_A)),
                after_block_id: Some(BLOCK_A.to_string()),
                replace_block_id: None,
                icon: Some(json!({
                    "type": "icon",
                    "icon": {
                        "name": "table",
                        "color": "blue"
                    }
                })),
                cover: None,
            },
        )
        .await
        .unwrap();

        let created_json = serde_json::to_value(created).unwrap();
        assert_eq!(created_json["database"]["id"], DATABASE_A);
        assert_eq!(created_json["database"]["parent"]["page_id"], PAGE_A);
        assert_eq!(
            created_json["database"]["data_sources"][0]["id"],
            DATA_SOURCE_A
        );
        assert_eq!(
            created_json["data_source"]["parent"]["database_id"],
            DATABASE_A
        );
        assert_eq!(created_json["view"]["type"], "table");
        assert_eq!(created_json["block"]["type"], "child_database");
        assert_eq!(
            created_json["block"]["child_database"]["database_id"],
            DATABASE_A
        );

        let properties: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let properties_json: serde_json::Value = serde_json::from_str(&properties).unwrap();
        assert_eq!(properties_json["Name"]["type"], "title");

        let children = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        assert_eq!(children_json["results"][0]["id"], BLOCK_A);
        assert_eq!(children_json["results"][1]["id"], DATABASE_A);
        assert_eq!(children_json["results"][1]["type"], "child_database");
    });
}

#[test]
fn saved_database_views_keep_their_settings_and_default_reference() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        sqlx::query("UPDATE notes_database_views SET sorts = ? WHERE id = ?")
            .bind(r#"[{"property_id":"title","direction":"ascending"}]"#)
            .bind(DATABASE_VIEW_A)
            .execute(&pool)
            .await
            .unwrap();

        let copied = databases::view_management::duplicate_database_view(
            &pool,
            NoteDatabaseViewDuplicate {
                id: DATABASE_VIEW_B.to_string(),
                database_id: DATABASE_A.to_string(),
                source_view_id: DATABASE_VIEW_A.to_string(),
                name: "Planning".to_string(),
            },
        )
        .await
        .unwrap();
        let copied = serde_json::to_value(copied).unwrap();
        assert_eq!(copied["name"], "Planning");
        assert_eq!(copied["type"], "table");
        assert_eq!(copied["data_source_id"], DATA_SOURCE_A);
        assert_eq!(copied["sorts"][0]["property_id"], "title");
        sqlx::query("UPDATE notes_database_views SET sorts = '[]' WHERE id = ?")
            .bind(DATABASE_VIEW_B)
            .execute(&pool)
            .await
            .unwrap();
        let source_sorts: String =
            sqlx::query_scalar("SELECT sorts FROM notes_database_views WHERE id = ?")
                .bind(DATABASE_VIEW_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(source_sorts.contains("title"));
        assert_eq!(
            databases::view_management::list_database_views(&pool, DATABASE_A)
                .await
                .unwrap()
                .len(),
            2
        );

        let renamed = databases::view_management::rename_database_view(
            &pool,
            DATABASE_A,
            DATABASE_VIEW_B,
            NoteDatabaseViewRename {
                name: "Backlog".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(serde_json::to_value(renamed).unwrap()["name"], "Backlog");
        assert!(
            databases::view_management::rename_database_view(
                &pool,
                DATABASE_A,
                DATABASE_VIEW_B,
                NoteDatabaseViewRename {
                    name: " ".to_string()
                },
            )
            .await
            .is_err()
        );

        let reordered = databases::view_management::reorder_database_views(
            &pool,
            DATABASE_A,
            &[DATABASE_VIEW_B.to_string(), DATABASE_VIEW_A.to_string()],
        )
        .await
        .unwrap();
        let reordered_ids: Vec<String> = reordered
            .into_iter()
            .map(|view| {
                serde_json::to_value(view).unwrap()["id"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(reordered_ids, [DATABASE_VIEW_B, DATABASE_VIEW_A]);
        let listed = databases::view_management::list_database_views(&pool, DATABASE_A)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&listed[0]).unwrap()["id"],
            DATABASE_VIEW_B
        );
        for invalid in [
            vec![DATABASE_VIEW_B.to_string()],
            vec![DATABASE_VIEW_B.to_string(), DATABASE_VIEW_B.to_string()],
            vec![
                DATABASE_VIEW_B.to_string(),
                DATABASE_VIEW_A.to_string(),
                LINKED_DATABASE_VIEW_A.to_string(),
            ],
        ] {
            assert!(
                databases::view_management::reorder_database_views(&pool, DATABASE_A, &invalid)
                    .await
                    .is_err()
            );
        }

        databases::view_management::delete_database_view(&pool, DATABASE_A, DATABASE_VIEW_A)
            .await
            .unwrap();
        let payload: String = sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
            .bind(DATABASE_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(payload["view_id"], DATABASE_VIEW_B);
        sqlx::query("INSERT INTO notes_database_views (id, database_id, data_source_id, name, type, sorts, configuration) VALUES (?, ?, ?, 'Board', 'board', '[]', '{}')")
            .bind(LINKED_DATABASE_VIEW_A)
            .bind(DATABASE_A)
            .bind(DATA_SOURCE_A)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            databases::view_management::delete_database_view(&pool, DATABASE_A, DATABASE_VIEW_B)
                .await
                .is_err()
        );
    });
}

#[test]
fn renaming_linked_database_does_not_rename_shared_source() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        databases::rename_database(
            &pool,
            DATABASE_A,
            NoteDatabaseRename {
                title: "Planning".to_string(),
            },
        )
        .await
        .unwrap();
        let original_payload: String =
            sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
                .bind(DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let original_payload: serde_json::Value = serde_json::from_str(&original_payload).unwrap();
        assert_eq!(original_payload["title"], "Planning");
        databases::create_linked_database_view(
            &pool,
            NoteLinkedDatabaseCreate {
                id: LINKED_DATABASE_A.to_string(),
                view_id: LINKED_DATABASE_VIEW_A.to_string(),
                source_block_id: DATABASE_A.to_string(),
                title: Some("Linked tasks".to_string()),
                parent: None,
                after_block_id: None,
                replace_block_id: None,
            },
        )
        .await
        .unwrap();
        databases::rename_database(
            &pool,
            LINKED_DATABASE_A,
            NoteDatabaseRename {
                title: "Sprint tasks".to_string(),
            },
        )
        .await
        .unwrap();
        let source_title: String =
            sqlx::query_scalar("SELECT title FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source_title, "Planning");
        let linked_title: String =
            sqlx::query_scalar("SELECT title FROM notes_databases WHERE id = ?")
                .bind(LINKED_DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked_title, "Sprint tasks");
    });
}

#[test]
fn linked_database_view_shares_source_and_keeps_view_settings_independent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: "Tasks".to_string(),
                parent: Some(page_parent(PAGE_A)),
                after_block_id: Some(BLOCK_A.to_string()),
                replace_block_id: None,
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Details": {
                        "id": "details",
                        "name": "Details",
                        "type": "rich_text",
                        "rich_text": {}
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "type": "number",
                        "number": { "format": "number" }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            Some(DATABASE_A),
            Some(DATABASE_VIEW_A),
            NoteDataSourceTableViewUpdate {
                filter: vec![],
                sorts: vec![NoteDataSourceTableSort {
                    property_id: "estimate".to_string(),
                    direction: "descending".to_string(),
                }],
                configuration: NoteDataSourceTableConfigurationUpdate {
                    property_order: vec![
                        "title".to_string(),
                        "details".to_string(),
                        "estimate".to_string(),
                    ],
                    hidden_property_ids: vec!["details".to_string()],
                    column_widths: json!({ "title": 260, "details": 180, "estimate": 120 }),
                    row_open_mode: "side_panel".to_string(),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();

        let linked = databases::create_linked_database_view(
            &pool,
            NoteLinkedDatabaseCreate {
                id: LINKED_DATABASE_A.to_string(),
                view_id: LINKED_DATABASE_VIEW_A.to_string(),
                source_block_id: DATABASE_A.to_string(),
                title: Some("Task mirror".to_string()),
                parent: None,
                after_block_id: None,
                replace_block_id: None,
            },
        )
        .await
        .unwrap();
        let linked_json = serde_json::to_value(linked).unwrap();
        assert_eq!(linked_json["database"]["id"], LINKED_DATABASE_A);
        assert_eq!(
            linked_json["data_source"]["parent"]["database_id"],
            DATABASE_A
        );
        assert_eq!(
            linked_json["view"]["parent"]["database_id"],
            LINKED_DATABASE_A
        );
        assert_eq!(linked_json["view"]["data_source_id"], DATA_SOURCE_A);
        assert_eq!(
            linked_json["block"]["child_database"]["database_id"],
            LINKED_DATABASE_A
        );
        assert_eq!(
            linked_json["block"]["child_database"]["data_source_id"],
            DATA_SOURCE_A
        );

        let linked_data_source_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_sources WHERE database_id = ?")
                .bind(LINKED_DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(linked_data_source_count, 0);

        data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            Some(LINKED_DATABASE_A),
            Some(LINKED_DATABASE_VIEW_A),
            NoteDataSourceTableViewUpdate {
                filter: vec![],
                sorts: vec![NoteDataSourceTableSort {
                    property_id: "title".to_string(),
                    direction: "ascending".to_string(),
                }],
                configuration: NoteDataSourceTableConfigurationUpdate {
                    property_order: vec![
                        "title".to_string(),
                        "estimate".to_string(),
                        "details".to_string(),
                    ],
                    hidden_property_ids: vec!["estimate".to_string()],
                    column_widths: json!({ "title": 320, "details": 180, "estimate": 120 }),
                    row_open_mode: "full_page".to_string(),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();

        let source_table = data_sources::layouts::table::data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            Some(DATABASE_A),
            Some(DATABASE_VIEW_A),
        )
        .await
        .unwrap();
        let source_json = serde_json::to_value(source_table).unwrap();
        assert_eq!(
            source_json["view"]["configuration"]["table"]["hidden_property_ids"],
            json!(["details"])
        );
        assert_eq!(
            source_json["view"]["configuration"]["table"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(source_json["view"]["sorts"][0]["property_id"], "estimate");

        let linked_table = data_sources::layouts::table::data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            Some(LINKED_DATABASE_A),
            Some(LINKED_DATABASE_VIEW_A),
        )
        .await
        .unwrap();
        let linked_table_json = serde_json::to_value(linked_table).unwrap();
        assert_eq!(
            linked_table_json["view"]["configuration"]["table"]["hidden_property_ids"],
            json!(["estimate"])
        );
        assert_eq!(
            linked_table_json["view"]["configuration"]["table"]["row_open_mode"],
            "full_page"
        );
        assert_eq!(
            linked_table_json["view"]["sorts"][0]["property_id"],
            "title"
        );

        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            Some(LINKED_DATABASE_VIEW_A),
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Details": {
                        "id": "details",
                        "name": "Details",
                        "type": "rich_text",
                        "rich_text": {}
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "type": "number",
                        "number": { "format": "number" }
                    },
                    "Due": {
                        "id": "due",
                        "name": "Due",
                        "type": "date",
                        "date": {}
                    }
                }),
            },
        )
        .await
        .unwrap();

        let reloaded_linked = data_sources::layouts::table::data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            Some(LINKED_DATABASE_A),
            Some(LINKED_DATABASE_VIEW_A),
        )
        .await
        .unwrap();
        let reloaded_linked_json = serde_json::to_value(reloaded_linked).unwrap();
        assert_eq!(
            reloaded_linked_json["data_source"]["properties"]["Due"]["type"],
            "date"
        );
        assert_eq!(
            reloaded_linked_json["view"]["parent"]["database_id"],
            LINKED_DATABASE_A
        );
        assert_eq!(
            reloaded_linked_json["view"]["configuration"]["table"]["hidden_property_ids"],
            json!(["estimate"])
        );
    });
}

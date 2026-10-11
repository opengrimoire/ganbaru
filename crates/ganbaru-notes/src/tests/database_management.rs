use super::helpers::*;
use crate::models::{
    NoteDataSourceAttach, NoteDataSourceCreate, NoteDataSourcePropertyAction,
    NoteDataSourcePropertyInsertionSide, NoteDatabaseRename, NoteDatabaseViewDuplicate,
    NoteDatabaseViewRename,
};
use crate::{data_sources, databases};
use serde_json::Value;

#[test]
fn database_shell_rename_preserves_independent_source_names_and_respects_lock() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Study",
            BLOCK_A,
        )
        .await;
        databases::rename_database(
            &pool,
            DATABASE_A,
            NoteDatabaseRename {
                title: "School".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT title FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "School"
        );
        data_sources::management::create_data_source(
            &pool,
            NoteDataSourceCreate {
                id: DATA_SOURCE_B.to_string(),
                database_id: DATABASE_A.to_string(),
                view_id: DATABASE_VIEW_B.to_string(),
                title: "Workout".to_string(),
                view_name: "Workout table".to_string(),
            },
        )
        .await
        .unwrap();
        databases::rename_database(
            &pool,
            DATABASE_A,
            NoteDatabaseRename {
                title: "Life".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT title FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "School"
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT title FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_B)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "Workout"
        );
        databases::editing_lock::set_database_editing_lock(&pool, DATABASE_A, true)
            .await
            .unwrap();
        assert!(
            databases::rename_database(
                &pool,
                DATABASE_A,
                NoteDatabaseRename {
                    title: "Changed".to_string()
                }
            )
            .await
            .expect_err("mutation must be rejected")
            .contains("locked")
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT title FROM notes_databases WHERE id = ?")
                .bind(DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap(),
            "Life"
        );
    });
}

#[test]
fn deleting_default_view_replaces_its_source_pair_and_linked_creation_uses_that_source() {
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
        data_sources::management::create_data_source(
            &pool,
            NoteDataSourceCreate {
                id: DATA_SOURCE_B.to_string(),
                database_id: DATABASE_A.to_string(),
                view_id: DATABASE_VIEW_B.to_string(),
                title: "Second source".to_string(),
                view_name: "Second table".to_string(),
            },
        )
        .await
        .unwrap();
        databases::view_management::duplicate_database_view(
            &pool,
            NoteDatabaseViewDuplicate {
                id: BLOCK_C.to_string(),
                database_id: DATABASE_A.to_string(),
                source_view_id: DATABASE_VIEW_A.to_string(),
                name: "First copy".to_string(),
            },
        )
        .await
        .unwrap();
        databases::view_management::delete_database_view(&pool, DATABASE_A, DATABASE_VIEW_A)
            .await
            .unwrap();
        let raw: String = sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
            .bind(DATABASE_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["view_id"], DATABASE_VIEW_B);
        assert_eq!(payload["data_source_id"], DATA_SOURCE_B);
        let linked = databases::create_linked_database_view(
            &pool,
            NoteLinkedDatabaseCreate {
                id: LINKED_DATABASE_A.to_string(),
                view_id: LINKED_DATABASE_VIEW_A.to_string(),
                source_block_id: DATABASE_A.to_string(),
                title: None,
                parent: Some(page_parent(PAGE_A)),
                after_block_id: Some(DATABASE_A.to_string()),
                replace_block_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(linked).unwrap()["view"]["data_source_id"],
            DATA_SOURCE_B
        );
    });
}

#[test]
fn database_sources_create_attach_and_keep_shared_rows_with_independent_views() {
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
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Other",
            DATABASE_A,
        )
        .await;
        for invalid_view in [DATABASE_A, DATA_SOURCE_B] {
            assert!(
                data_sources::management::attach_data_source(
                    &pool,
                    NoteDataSourceAttach {
                        data_source_id: DATA_SOURCE_B.to_string(),
                        database_id: DATABASE_A.to_string(),
                        view_id: invalid_view.to_string(),
                        view_name: "Invalid view".to_string(),
                    }
                )
                .await
                .err()
                .expect("mutation must be rejected")
                .contains("identities must differ")
            );
        }
        let created = data_sources::management::create_data_source(
            &pool,
            NoteDataSourceCreate {
                id: BLOCK_C.to_string(),
                database_id: DATABASE_A.to_string(),
                view_id: BLOCK_D.to_string(),
                title: "Independent".to_string(),
                view_name: "Table".to_string(),
            },
        )
        .await
        .unwrap();
        let created = serde_json::to_value(created).unwrap();
        assert_eq!(created["data_source"]["parent"]["database_id"], DATABASE_A);
        assert_eq!(
            created["data_source"]["properties"]["Name"]["type"],
            "title"
        );
        assert_eq!(created["view"]["data_source_id"], BLOCK_C);
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                first_block_id: BLOCK_B.to_string(),
                title: "Shared page".to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        let attached = data_sources::management::attach_data_source(
            &pool,
            NoteDataSourceAttach {
                data_source_id: DATA_SOURCE_B.to_string(),
                database_id: DATABASE_A.to_string(),
                view_id: BLOCK_E.to_string(),
                view_name: "Linked table".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(attached).unwrap()["data_source"]["parent"]["database_id"],
            DATABASE_B
        );
        let linked = data_sources::layouts::table::data_source_table_view(
            &pool,
            DATA_SOURCE_B,
            Some(DATABASE_A),
            Some(BLOCK_E),
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(linked).unwrap()["rows"][0]["id"],
            PAGE_B
        );
        assert!(
            data_sources::management::attach_data_source(
                &pool,
                NoteDataSourceAttach {
                    data_source_id: DATA_SOURCE_B.to_string(),
                    database_id: DATABASE_A.to_string(),
                    view_id: BLOCK_F.to_string(),
                    view_name: "Duplicate attachment".to_string(),
                }
            )
            .await
            .is_err()
        );
        let view_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_database_views WHERE database_id = ?")
                .bind(DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(view_count, 3);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM notes_data_sources")
                .fetch_one(&pool)
                .await
                .unwrap(),
            3
        );
        let folder_id = "00000000-0000-4000-8000-000000000301";
        sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group', 'Group')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name) VALUES ('project', 'group', 'Project')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notes_folders (id, project_id, name) VALUES (?, 'project', 'Folder')",
        )
        .bind(folder_id)
        .execute(&pool)
        .await
        .unwrap();
        create_page(&pool, PAGE_C, "99999999-9999-4999-8999-999999999999").await;
        sqlx::query("UPDATE notes_pages SET title = 'Projects', folder_id = ?, properties = json_set(properties, '$.__ganbaru_project_id', 'project') WHERE id = ?")
            .bind(folder_id)
            .bind(PAGE_C)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notes_pages SET parent_type = 'page_id', parent_page_id = ? WHERE id = ?",
        )
        .bind(PAGE_C)
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        let listed = serde_json::to_value(
            data_sources::schema::list_data_sources(&pool)
                .await
                .unwrap(),
        )
        .unwrap();
        let listed = listed.as_array().unwrap();
        assert_eq!(listed.len(), 3);
        for source in listed {
            assert_eq!(source["object"], "data_source");
            assert_eq!(
                source["location"],
                serde_json::json!(["Group", "Project", "Folder", "Projects", "First page"])
            );
        }
    });
}

#[test]
fn database_layout_lock_uses_requesting_shell_and_allows_row_edits() {
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
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Other",
            DATABASE_A,
        )
        .await;
        data_sources::management::attach_data_source(
            &pool,
            NoteDataSourceAttach {
                data_source_id: DATA_SOURCE_A.to_string(),
                database_id: DATABASE_B.to_string(),
                view_id: BLOCK_C.to_string(),
                view_name: "Shared table".to_string(),
            },
        )
        .await
        .unwrap();
        let locked = databases::editing_lock::set_database_editing_lock(&pool, DATABASE_A, true)
            .await
            .unwrap();
        assert!(
            serde_json::to_value(locked).unwrap()["editing_locked"]
                .as_bool()
                .unwrap()
        );
        assert!(
            databases::view_management::rename_database_view(
                &pool,
                DATABASE_A,
                DATABASE_VIEW_A,
                NoteDatabaseViewRename {
                    name: "Changed".to_string(),
                }
            )
            .await
            .err()
            .expect("mutation must be rejected")
            .contains("locked")
        );
        assert!(
            databases::view_management::reorder_database_views(
                &pool,
                DATABASE_A,
                &[DATABASE_VIEW_A.to_string()]
            )
            .await
            .err()
            .expect("mutation must be rejected")
            .contains("locked")
        );
        assert!(data_sources::schema::update_data_source_schema(&pool, DATA_SOURCE_A, Some(DATABASE_A), Some(DATABASE_VIEW_A), NoteDataSourceSchemaUpdate {
            properties: json!({"Name":{"id":"title","name":"Name","type":"title","title":{}}}),
        }).await.err().expect("mutation must be rejected").contains("locked"));
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                first_block_id: BLOCK_B.to_string(),
                title: "Still editable".to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "title".to_string(),
                value: json!("Edited while locked"),
            },
        )
        .await
        .unwrap();
        let properties = json!({
            "Name":{"id":"title","name":"Name","type":"title","title":{}},
            "Estimate":{"id":"estimate","name":"Estimate","type":"number","number":{"format":"number"}},
        });
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            Some(DATABASE_B),
            Some(BLOCK_C),
            NoteDataSourceSchemaUpdate {
                properties: properties.clone(),
            },
        )
        .await
        .unwrap();
        databases::editing_lock::set_database_editing_lock(&pool, DATABASE_B, true)
            .await
            .unwrap();
        databases::editing_lock::set_database_editing_lock(&pool, DATABASE_A, false)
            .await
            .unwrap();
        let table_update = || {
            serde_json::from_value::<NoteDataSourceTableViewUpdate>(json!({"filter":[],"sorts":[],"configuration":{
            "property_order":["title","estimate"],"hidden_property_ids":[],"column_widths":{},"row_open_mode":"full_page"
        }})).unwrap()
        };
        assert!(
            data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                Some(BLOCK_C),
                table_update()
            )
            .await
            .err()
            .expect("actual requesting shell must be locked")
            .contains("locked")
        );
        assert!(
            data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                Some(DATABASE_A),
                Some(BLOCK_C),
                table_update()
            )
            .await
            .err()
            .expect("mismatched scope must fail")
            .contains("requesting database")
        );
        assert!(
            data_sources::schema::update_data_source_schema(
                &pool,
                DATA_SOURCE_A,
                Some(DATABASE_B),
                Some(BLOCK_C),
                NoteDataSourceSchemaUpdate {
                    properties: properties.clone()
                }
            )
            .await
            .err()
            .expect("mutation must be rejected")
            .contains("locked")
        );
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            Some(DATABASE_A),
            Some(DATABASE_VIEW_A),
            NoteDataSourceSchemaUpdate { properties },
        )
        .await
        .unwrap();
        assert!(
            !serde_json::to_value(
                databases::database_reference(&pool, DATABASE_A)
                    .await
                    .unwrap()
            )
            .unwrap()["editing_locked"]
                .as_bool()
                .unwrap()
        );
        assert!(
            serde_json::to_value(
                databases::database_reference(&pool, DATABASE_B)
                    .await
                    .unwrap()
            )
            .unwrap()["editing_locked"]
                .as_bool()
                .unwrap()
        );
    });
}

#[test]
fn contextual_property_insert_and_empty_duplicate_preserve_view_state_and_roll_back_failures() {
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
        let properties = json!({
            "Name":{"id":"title","name":"Name","type":"title","title":{}},
            "Priority":{"id":"priority","name":"Priority","type":"select","select":{"options":[{"id":"high","name":"High","color":"red"}]}},
            "Related":{"id":"related","name":"Related","type":"relation","relation":{"data_source_id":DATA_SOURCE_A,"type":"single_property","single_property":{}}},
        });
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate { properties },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                first_block_id: BLOCK_B.to_string(),
                title: "Task".to_string(),
                properties: Some(json!({"Priority":{"type":"select","select":{"id":"high"}}})),
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_database_views SET configuration = ?, filter = ?, sorts = ? WHERE id = ?")
            .bind(json!({"type":"table","table":{"property_order":["title","related","priority"],"hidden_property_ids":["related"],"column_widths":{"priority":240},"row_open_mode":"side_panel"}}).to_string())
            .bind(json!({"type":"and","filters":[{"property_id":"priority","condition":"equals","value":"High"}]}).to_string())
            .bind(json!([{"property_id":"priority","direction":"descending"}]).to_string()).bind(DATABASE_VIEW_A).execute(&pool).await.unwrap();
        let inserted = data_sources::property_actions::apply_property_action(&pool, DATA_SOURCE_A, DATABASE_A, DATABASE_VIEW_A, NoteDataSourcePropertyAction::Insert {
            property_id: "priority".to_string(), side: NoteDataSourcePropertyInsertionSide::Left,
            property: json!({"id":"estimate","name":"Estimate","type":"number","number":{"format":"number"}}),
        }).await.unwrap();
        let inserted = serde_json::to_value(inserted).unwrap();
        assert_eq!(
            inserted["schema"]["view"]["configuration"]["table"]["property_order"],
            json!(["title", "related", "estimate", "priority"])
        );
        let duplicated = data_sources::property_actions::apply_property_action(
            &pool,
            DATA_SOURCE_A,
            DATABASE_A,
            DATABASE_VIEW_A,
            NoteDataSourcePropertyAction::Duplicate {
                property_id: "priority".to_string(),
                name: "Priority copy".to_string(),
            },
        )
        .await
        .unwrap();
        let duplicate_id = duplicated.property_id.clone();
        let duplicated = serde_json::to_value(duplicated).unwrap();
        let view = &duplicated["schema"]["view"];
        assert_eq!(
            view["configuration"]["table"]["property_order"],
            json!(["title", "related", "estimate", "priority", duplicate_id])
        );
        assert_eq!(
            view["configuration"]["table"]["hidden_property_ids"],
            json!(["related"])
        );
        assert_eq!(
            view["configuration"]["table"]["column_widths"]["priority"],
            240
        );
        assert_eq!(
            view["configuration"]["table"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(view["filter"]["filters"][0]["property_id"], "priority");
        assert_eq!(view["sorts"][0]["direction"], "descending");
        let option = &duplicated["schema"]["data_source"]["properties"]["Priority copy"]["select"]
            ["options"][0];
        assert_ne!(option["id"], "high");
        assert_eq!(option["name"], "High");
        let row: String = sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = ?")
            .bind(PAGE_B)
            .fetch_one(&pool)
            .await
            .unwrap();
        let row: Value = serde_json::from_str(&row).unwrap();
        assert!(!row.as_object().unwrap().values().any(|value| value.get("id").and_then(Value::as_str) == Some(duplicate_id.as_str())));
        let before: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        for action in [
            NoteDataSourcePropertyAction::Insert {
                property_id: "title".to_string(),
                side: NoteDataSourcePropertyInsertionSide::Left,
                property: json!({"id":"invalid","name":"Invalid","type":"rich_text","rich_text":{}}),
            },
            NoteDataSourcePropertyAction::Duplicate {
                property_id: "title".to_string(),
                name: "Name copy".to_string(),
            },
            NoteDataSourcePropertyAction::Duplicate {
                property_id: "priority".to_string(),
                name: "Estimate".to_string(),
            },
        ] {
            assert!(
                data_sources::property_actions::apply_property_action(
                    &pool,
                    DATA_SOURCE_A,
                    DATABASE_A,
                    DATABASE_VIEW_A,
                    action
                )
                .await
                .is_err()
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT properties FROM notes_data_sources WHERE id = ?"
            )
            .bind(DATA_SOURCE_A)
            .fetch_one(&pool)
            .await
            .unwrap(),
            before
        );
    });
}

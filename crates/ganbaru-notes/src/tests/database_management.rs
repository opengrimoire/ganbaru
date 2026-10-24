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
        sqlx::query(
            "INSERT INTO project_groups (id, name, order_key) VALUES ('group', 'Group', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, order_key) VALUES ('project', 'group', 'Project', 'a1')",
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
        sqlx::query("UPDATE notes_pages SET title = 'Projects', folder_id = ?, project_id = 'project' WHERE id = ?")
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

#[test]
fn contextual_property_actions_keep_row_page_layout() {
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
            "Priority":{"id":"priority","name":"Priority","type":"select","select":{"options":[]},"page_position":1,"page_visibility":"hide_when_empty"},
            "Due":{"id":"due","name":"Due","type":"date","date":{},"page_position":0},
            "Notes":{"id":"notes","name":"Notes","type":"rich_text","rich_text":{}},
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
        let positions = |schema: &Value| {
            let properties = schema["schema"]["data_source"]["properties"]
                .as_object()
                .unwrap();
            let mut positioned: Vec<(u64, String)> = properties
                .values()
                .filter_map(|property| {
                    Some((
                        property.get("page_position")?.as_u64()?,
                        property["id"].as_str()?.to_string(),
                    ))
                })
                .collect();
            positioned.sort();
            positioned.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
        };

        let inserted = run_property_action(
            &pool,
            NoteDataSourcePropertyAction::Insert {
                property_id: "due".to_string(),
                side: NoteDataSourcePropertyInsertionSide::Right,
                property: json!({"id":"estimate","name":"Estimate","type":"number","number":{"format":"number"}}),
            },
        )
        .await
        .unwrap();
        assert_eq!(positions(&inserted), vec!["due", "estimate", "priority"]);

        let duplicated = run_property_action(
            &pool,
            NoteDataSourcePropertyAction::Duplicate {
                property_id: "priority".to_string(),
                name: "Priority copy".to_string(),
            },
        )
        .await
        .unwrap();
        let duplicate_id = duplicated["property_id"].as_str().unwrap().to_string();
        assert_eq!(
            positions(&duplicated),
            vec![
                "due".to_string(),
                "estimate".to_string(),
                "priority".to_string(),
                duplicate_id
            ]
        );
        assert_eq!(
            duplicated["schema"]["data_source"]["properties"]["Priority copy"]["page_visibility"],
            "hide_when_empty"
        );

        let beside_unpositioned = run_property_action(
            &pool,
            NoteDataSourcePropertyAction::Insert {
                property_id: "notes".to_string(),
                side: NoteDataSourcePropertyInsertionSide::Left,
                property: json!({"id":"owner","name":"Owner","type":"rich_text","rich_text":{}}),
            },
        )
        .await
        .unwrap();
        assert!(
            beside_unpositioned["schema"]["data_source"]["properties"]["Owner"]
                .get("page_position")
                .is_none()
        );

        let changed = run_property_action(&pool, change_type("priority", "rich_text", None))
            .await
            .unwrap();
        let priority = &changed["schema"]["data_source"]["properties"]["Priority"];
        assert_eq!(priority["page_position"], 2);
        assert_eq!(priority["page_visibility"], "hide_when_empty");
    });
}

async fn run_property_action(
    pool: &sqlx::SqlitePool,
    action: NoteDataSourcePropertyAction,
) -> Result<Value, String> {
    data_sources::property_actions::apply_property_action(
        pool,
        DATA_SOURCE_A,
        DATABASE_A,
        DATABASE_VIEW_A,
        action,
    )
    .await
    .map(|result| serde_json::to_value(result).unwrap())
}

fn change_type(
    property_id: &str,
    property_type: &str,
    config: Option<Value>,
) -> NoteDataSourcePropertyAction {
    NoteDataSourcePropertyAction::ChangeType {
        property_id: property_id.to_string(),
        property_type: property_type.to_string(),
        config,
    }
}

async fn row_properties(pool: &sqlx::SqlitePool, page_id: &str) -> Value {
    let raw: String = sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = ?")
        .bind(page_id)
        .fetch_one(pool)
        .await
        .unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn property_type_changes_convert_row_values_and_deletion_strips_them() {
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
            "Notes":{"id":"notes","name":"Notes","type":"rich_text","rich_text":{}},
            "Tags":{"id":"tags","name":"Tags","type":"multi_select","multi_select":{"options":[{"id":"alpha","name":"Alpha","color":"red"},{"id":"beta","name":"Beta","color":"blue"}]}},
            "Count":{"id":"count","name":"Count","type":"number","number":{"format":"number"}},
            "Due":{"id":"due","name":"Due","type":"date","date":{}},
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
                properties: Some(json!({
                    "Notes":{"type":"rich_text","rich_text":[{"type":"text","text":{"content":"Red, Blue"},"plain_text":"Red, Blue"}]},
                    "Tags":{"type":"multi_select","multi_select":[{"id":"beta","name":"Beta","color":"blue"},{"id":"alpha","name":"Alpha","color":"red"}]},
                    "Count":{"type":"number","number":3},
                    "Due":{"type":"date","date":{"start":"2026-03-04","end":null,"time_zone":null}},
                })),
            },
        )
        .await
        .unwrap();

        let converted = run_property_action(&pool, change_type("notes", "multi_select", None))
            .await
            .unwrap();
        let notes = &converted["schema"]["data_source"]["properties"]["Notes"];
        assert_eq!(notes["type"], "multi_select");
        let option_names: Vec<&str> = notes["multi_select"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["name"].as_str().unwrap())
            .collect();
        assert_eq!(option_names, vec!["Red", "Blue"]);
        let row = row_properties(&pool, PAGE_B).await;
        let values: Vec<&str> = row["Notes"]["multi_select"]
            .as_array()
            .unwrap()
            .iter()
            .map(|option| option["name"].as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["Red", "Blue"]);
        assert_eq!(row["Notes"]["type"], "multi_select");

        let converted = run_property_action(&pool, change_type("tags", "select", None))
            .await
            .unwrap();
        assert_eq!(
            converted["schema"]["data_source"]["properties"]["Tags"]["select"]["options"][0]["id"],
            "alpha"
        );
        let row = row_properties(&pool, PAGE_B).await;
        assert_eq!(
            row["Tags"]["select"],
            json!({"id":"beta","name":"Beta","color":"blue"})
        );

        run_property_action(&pool, change_type("count", "rich_text", None))
            .await
            .unwrap();
        let row = row_properties(&pool, PAGE_B).await;
        assert_eq!(row["Count"]["rich_text"][0]["plain_text"], "3");

        let converted = run_property_action(&pool, change_type("count", "relation", Some(json!({"data_source_id": DATA_SOURCE_A, "dual_property": {"synced_property_id": "x", "synced_property_name": "X"}})))).await.unwrap();
        assert_eq!(
            converted["schema"]["data_source"]["properties"]["Count"]["relation"],
            json!({"data_source_id": DATA_SOURCE_A})
        );
        let row = row_properties(&pool, PAGE_B).await;
        assert_eq!(row["Count"]["relation"], json!([]));

        let converted = run_property_action(&pool, change_type("tags", "status", None))
            .await
            .unwrap();
        let status = &converted["schema"]["data_source"]["properties"]["Tags"]["status"];
        assert_eq!(status["default_option_id"], "alpha");
        assert_eq!(status["groups"][0]["option_ids"], json!(["alpha", "beta"]));
        let row = row_properties(&pool, PAGE_B).await;
        assert_eq!(row["Tags"]["status"]["id"], "beta");
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                first_block_id: BLOCK_C.to_string(),
                title: "Next".to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            row_properties(&pool, PAGE_C).await["Tags"]["status"]["id"],
            "alpha"
        );

        let deleted = run_property_action(
            &pool,
            NoteDataSourcePropertyAction::Delete {
                property_id: "due".to_string(),
            },
        )
        .await
        .unwrap();
        assert!(
            deleted["schema"]["data_source"]["properties"]
                .get("Due")
                .is_none()
        );
        assert!(
            !deleted["schema"]["view"]["configuration"]["table"]["property_order"]
                .as_array()
                .unwrap()
                .contains(&json!("due"))
        );
        assert!(row_properties(&pool, PAGE_B).await.get("Due").is_none());

        let before: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        for action in [
            change_type("title", "rich_text", None),
            change_type("notes", "multi_select", None),
            change_type("notes", "title", None),
            change_type("notes", "relation", None),
            NoteDataSourcePropertyAction::Delete {
                property_id: "title".to_string(),
            },
        ] {
            assert!(run_property_action(&pool, action).await.is_err());
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

#[test]
fn removing_a_reciprocal_relation_turns_its_counterpart_one_way() {
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
            "Parent":{"id":"parent","name":"Parent","type":"relation","relation":{"data_source_id":DATA_SOURCE_A,"dual_property":{"synced_property_id":"children","synced_property_name":"Children"}}},
            "Children":{"id":"children","name":"Children","type":"relation","relation":{"data_source_id":DATA_SOURCE_A,"dual_property":{"synced_property_id":"parent","synced_property_name":"Parent"}}},
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
        let deleted = run_property_action(
            &pool,
            NoteDataSourcePropertyAction::Delete {
                property_id: "parent".to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            deleted["schema"]["data_source"]["properties"]["Children"]["relation"],
            json!({"data_source_id": DATA_SOURCE_A})
        );
    });
}

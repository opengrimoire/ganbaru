use super::helpers::*;
use crate::models::NoteDatabaseDuplicate;
use serde_json::Value;

async fn seed_database(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    create_page(pool, PAGE_C, BLOCK_C).await;
    create_database(
        pool,
        DATABASE_A,
        DATA_SOURCE_A,
        DATABASE_VIEW_A,
        "Tasks",
        BLOCK_A,
    )
    .await;
    data_sources::schema::update_data_source_schema(pool, DATA_SOURCE_A, None, None, NoteDataSourceSchemaUpdate {
        properties: json!({
            "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
            "Priority": {"id": "priority", "name": "Priority", "type": "select", "select": {
                "options": [{"id": "high", "name": "High", "color": "red"}]}},
            "Related": {"id": "related", "name": "Related", "type": "relation", "relation": {"data_source_id": DATA_SOURCE_A}}
        }),
    }).await.unwrap();
    data_sources::rows::create_data_source_row_page(
        pool,
        DATA_SOURCE_A,
        NoteDataSourceRowPageCreate {
            id: PAGE_B.into(),
            title: "First row".into(),
            first_block_id: BLOCK_B.into(),
            properties: None,
        },
    )
    .await
    .unwrap();
    for (property_id, value) in [("priority", json!("high")), ("related", json!([PAGE_B]))] {
        data_sources::layouts::table::update_data_source_row_property(
            pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: property_id.into(),
                value,
            },
        )
        .await
        .unwrap();
    }
    writes::update_block(
        pool,
        BLOCK_B,
        block_update("paragraph", paragraph_payload("Row body")),
    )
    .await
    .unwrap();
}

fn duplicate_request(id: &str) -> NoteDatabaseDuplicate {
    NoteDatabaseDuplicate {
        source_block_id: DATABASE_A.into(),
        id: id.into(),
        parent: Some(page_parent(PAGE_C)),
        after_block_id: Some(BLOCK_C.into()),
        replace_block_id: None,
    }
}

async fn copied_row(pool: &SqlitePool, source_id: &str) -> String {
    sqlx::query_scalar(
        "SELECT id FROM notes_pages WHERE parent_data_source_id = ? ORDER BY id LIMIT 1",
    )
    .bind(source_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn database_copy_across_notes_keeps_schema_views_rows_templates_and_self_relations_independent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        data_sources::templates::create_data_source_template_from_row(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceTemplateCreateFromRow {
                id: TEMPLATE_A.into(),
                source_page_id: PAGE_B.into(),
                name: "Task template".into(),
                is_default: Some(true),
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_database_views SET configuration = ?, filter = ?, sorts = ? WHERE id = ?")
            .bind(json!({"type": "table", "table": {"property_order": ["title", "priority", "related"],
                "hidden_property_ids": ["related"], "column_widths": {"priority": 220}, "row_open_mode": "full_page",
                "presentation": { "frozen_property_id": "priority", "columns": { "priority": { "wrap": true, "calculation": "unique" } }, "color_rules": [{ "id": "rule", "property_id": "priority", "color": "red", "filters": [{ "property_id": "priority", "condition": "equals", "value": "High" }] }] }
            }}).to_string())
            .bind(json!({"filters": [{"property_id": "priority", "condition": "is", "value": "high"}]}).to_string())
            .bind(json!([{"property_id": "priority", "direction": "ascending"}]).to_string())
            .bind(DATABASE_VIEW_A).execute(&pool).await.unwrap();
        for (index, view_type) in ["board", "list", "gallery", "calendar", "timeline"]
            .into_iter()
            .enumerate()
        {
            sqlx::query("INSERT INTO notes_database_views (id, database_id, data_source_id, name, type, configuration, sort_order) VALUES (?, ?, ?, ?, ?, ?, ?)")
                .bind(format!("98989898-9898-4989-8989-{index:012}"))
                .bind(DATABASE_A).bind(DATA_SOURCE_A).bind(view_type).bind(view_type)
                .bind(json!({"type": view_type, (view_type): {"property_order": ["title", "priority"]}}).to_string())
                .bind((index + 1) as f64).execute(&pool).await.unwrap();
        }
        let copied = serde_json::to_value(
            databases::duplicate_database(&pool, duplicate_request(DATABASE_B))
                .await
                .unwrap(),
        )
        .unwrap();
        let source = copied["data_source"]["id"].as_str().unwrap();
        let view = copied["view"]["id"].as_str().unwrap();
        // Copied identities must remain valid for frontend navigation and clipboard links.
        let navigation_row_id = copied_row(&pool, source).await;
        for id in [source, view, navigation_row_id.as_str()] {
            assert_eq!(id.as_bytes()[14], b'4');
            assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        }
        let properties = &copied["data_source"]["properties"];
        let priority = properties["Priority"]["id"].as_str().unwrap();
        let option = properties["Priority"]["select"]["options"][0]["id"]
            .as_str()
            .unwrap();
        let relation = properties["Related"]["id"].as_str().unwrap();
        assert_ne!(source, DATA_SOURCE_A);
        assert_ne!(view, DATABASE_VIEW_A);
        assert_ne!(priority, "priority");
        assert_ne!(option, "high");
        assert_eq!(properties["Related"]["relation"]["data_source_id"], source);
        assert_eq!(copied["block"]["child_database"]["database_id"], DATABASE_B);
        assert_eq!(copied["block"]["parent"]["page_id"], PAGE_C);
        assert_eq!(
            copied["view"]["configuration"]["table"]["column_widths"][priority],
            220
        );
        assert_eq!(
            copied["view"]["configuration"]["table"]["presentation"]["frozen_property_id"],
            priority
        );
        assert_eq!(
            copied["view"]["configuration"]["table"]["presentation"]["columns"][priority]["wrap"],
            true
        );
        assert!(
            copied["view"]["configuration"]["table"]["presentation"]["columns"]
                .get("priority")
                .is_none()
        );
        assert_eq!(
            copied["view"]["configuration"]["table"]["presentation"]["color_rules"][0]["property_id"],
            priority
        );
        assert_eq!(
            copied["view"]["configuration"]["table"]["presentation"]["color_rules"][0]["filters"]
                [0]["property_id"],
            priority
        );
        assert_eq!(
            copied["view"]["filter"]["filters"][0]["property_id"],
            priority
        );
        assert_eq!(copied["view"]["filter"]["filters"][0]["value"], "high");
        let row = copied_row(&pool, source).await;
        assert_ne!(row, PAGE_B);
        let stored =
            serde_json::to_value(reads::get_page(&pool, &row, false).await.unwrap()).unwrap();
        assert_eq!(stored["properties"]["Related"]["relation"][0]["id"], row);
        assert_eq!(stored["properties"]["Priority"]["id"], priority);
        assert_eq!(stored["properties"]["Priority"]["select"]["id"], option);
        let link: (String, String, String) = sqlx::query_as("SELECT source_property_id, target_page_id, target_data_source_id FROM notes_data_source_relation_links WHERE source_page_id = ?")
            .bind(&row).fetch_one(&pool).await.unwrap();
        assert_eq!(link, (relation.into(), row.clone(), source.into()));
        let views: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_database_views WHERE database_id = ?")
                .bind(DATABASE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(views, 6);
        let template = serde_json::to_value(
            data_sources::templates::list_data_source_templates(&pool, source)
                .await
                .unwrap(),
        )
        .unwrap();
        let template_id = template[0]["id"].as_str().unwrap();
        assert_ne!(template_id, TEMPLATE_A);
        assert_eq!(template[0]["source_page_id"], row);
        assert_eq!(template[0]["properties"]["Priority"]["id"], priority);
        assert_eq!(template[0]["is_default"], true);
        let applied = serde_json::to_value(
            data_sources::templates::apply_data_source_template(
                &pool,
                source,
                template_id,
                NoteDataSourceTemplateApply {
                    id: None,
                    title: Some("From copied template".into()),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            applied["page"]["properties"]["Priority"]["select"]["id"],
            option
        );
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            source,
            &row,
            NoteDataSourceRowPropertyUpdate {
                property_id: priority.into(),
                value: Value::Null,
            },
        )
        .await
        .unwrap();
        let original =
            serde_json::to_value(reads::get_page(&pool, PAGE_B, false).await.unwrap()).unwrap();
        assert_eq!(original["properties"]["Priority"]["select"]["id"], "high");
        assert!(
            !page_history::list_page_history_snapshots(&pool, PAGE_C)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

async fn add_nested_graph(pool: &SqlitePool) {
    writes::create_page(
        pool,
        NotePageCreate {
            id: BLOCK_D.into(),
            title: "Nested note".into(),
            parent: page_parent(PAGE_B),
            folder_id: None,
            first_block_id: BLOCK_E.into(),
            after_block_id: None,
            properties: None,
        },
    )
    .await
    .unwrap();
    databases::create_database(
        pool,
        NoteDatabaseCreate {
            id: DATABASE_B.into(),
            data_source_id: DATA_SOURCE_B.into(),
            view_id: DATABASE_VIEW_B.into(),
            title: "Nested data".into(),
            parent: Some(page_parent(BLOCK_D)),
            after_block_id: None,
            replace_block_id: None,
            icon: None,
            cover: None,
        },
    )
    .await
    .unwrap();
    data_sources::rows::create_data_source_row_page(
        pool,
        DATA_SOURCE_B,
        NoteDataSourceRowPageCreate {
            id: COMMENT_A.into(),
            title: "Nested row".into(),
            first_block_id: BLOCK_F.into(),
            properties: None,
        },
    )
    .await
    .unwrap();
    writes::update_block(
        pool,
        BLOCK_F,
        block_update("paragraph", paragraph_payload("Deep body")),
    )
    .await
    .unwrap();
}

#[test]
fn database_copy_captures_nested_row_notes_and_databases_before_copying_into_its_own_row() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        add_nested_graph(&pool).await;
        let mut request = duplicate_request(LINKED_DATABASE_A);
        request.parent = Some(page_parent(PAGE_B));
        request.after_block_id = None;
        let copied =
            serde_json::to_value(databases::duplicate_database(&pool, request).await.unwrap())
                .unwrap();
        let row = copied_row(&pool, copied["data_source"]["id"].as_str().unwrap()).await;
        let note: String =
            sqlx::query_scalar("SELECT id FROM notes_pages WHERE parent_page_id = ?")
                .bind(row)
                .fetch_one(&pool)
                .await
                .unwrap();
        let nested_source: String = sqlx::query_scalar("SELECT source.id FROM notes_data_sources AS source JOIN notes_databases AS db ON db.id = source.database_id WHERE db.parent_page_id = ?")
            .bind(note).fetch_one(&pool).await.unwrap();
        assert_ne!(nested_source, DATA_SOURCE_B);
        let deep_row = copied_row(&pool, &nested_source).await;
        let body: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE page_id = ?")
                .bind(deep_row)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(body, "Deep body");
        let databases: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_databases")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(databases, 4);
    });
}

#[test]
fn block_and_page_duplication_copy_database_graphs_instead_of_sharing_source_ids() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        let duplicated = serde_json::to_value(
            writes::duplicate_blocks(
                &pool,
                NoteDuplicateBlocks {
                    block_ids: vec![DATABASE_A.into()],
                    parent: page_parent(PAGE_C),
                    after: Some(BLOCK_C.into()),
                    before: None,
                    include_trashed_sources: None,
                    duplicated_block_ids: vec![NoteDuplicatedBlockId {
                        source_id: DATABASE_A.into(),
                        duplicate_id: DATABASE_B.into(),
                    }],
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_ne!(
            duplicated["results"][0]["child_database"]["data_source_id"],
            DATA_SOURCE_A
        );
        let duplicated = serde_json::to_value(
            writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
                .await
                .unwrap(),
        )
        .unwrap();
        let page_id = duplicated["page"]["id"].as_str().unwrap();
        let source: String = sqlx::query_scalar("SELECT source.id FROM notes_data_sources AS source JOIN notes_blocks AS block ON block.id = source.database_id WHERE block.page_id = ?")
            .bind(page_id).fetch_one(&pool).await.unwrap();
        assert_ne!(source, DATA_SOURCE_A);
        assert_ne!(copied_row(&pool, &source).await, PAGE_B);
    });
}

fn linked_request() -> NoteLinkedDatabaseCreate {
    NoteLinkedDatabaseCreate {
        id: LINKED_DATABASE_A.into(),
        view_id: LINKED_DATABASE_VIEW_A.into(),
        source_block_id: DATABASE_A.into(),
        title: None,
        parent: Some(page_parent(PAGE_C)),
        after_block_id: Some(BLOCK_C.into()),
        replace_block_id: None,
    }
}

#[test]
fn linked_database_placement_reference_and_trash_stay_in_destination_scope() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        let created = serde_json::to_value(
            databases::create_linked_database_view(&pool, linked_request())
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(created["block"]["parent"]["page_id"], PAGE_C);
        assert_eq!(created["data_source"]["id"], DATA_SOURCE_A);
        let reference = serde_json::to_value(
            databases::database_reference(&pool, LINKED_DATABASE_A)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reference["source_block_id"], LINKED_DATABASE_A);
        assert_eq!(reference["canonical_source_block_id"], DATABASE_A);
        assert_eq!(reference["canonical_source_page_id"], PAGE_A);
        assert_eq!(reference["page_id"], PAGE_C);
        assert_eq!(reference["owned_data_source_count"], 0);
        assert_eq!(reference["is_linked"], true);
        writes::trash_page(&pool, PAGE_C, true).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT in_trash FROM notes_databases WHERE id = ?")
                .bind(LINKED_DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT in_trash FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_ok());
        writes::trash_page(&pool, PAGE_C, false).await.unwrap();
        let copied = serde_json::to_value(
            databases::duplicate_database(
                &pool,
                NoteDatabaseDuplicate {
                    source_block_id: LINKED_DATABASE_A.into(),
                    id: DATABASE_B.into(),
                    parent: None,
                    after_block_id: None,
                    replace_block_id: None,
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_ne!(copied["data_source"]["id"], DATA_SOURCE_A);
        assert_eq!(
            databases::database_reference(&pool, DATABASE_B)
                .await
                .unwrap()
                .owned_data_source_count,
            1
        );
    });
}

#[test]
fn database_replacement_is_atomic_and_missing_or_structural_targets_leave_no_objects() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        let mut request = linked_request();
        request.id = BLOCK_C.into();
        request.parent = None;
        request.after_block_id = None;
        request.replace_block_id = Some(BLOCK_C.into());
        let original_order: f64 =
            sqlx::query_scalar("SELECT sort_order FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        databases::create_linked_database_view(&pool, request)
            .await
            .unwrap();
        let order: f64 = sqlx::query_scalar("SELECT sort_order FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_C)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(order, original_order);
        assert_eq!(
            reads::get_block_row(&pool, BLOCK_C, false)
                .await
                .unwrap()
                .block_type,
            "child_database"
        );
        let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mut missing = duplicate_request(DATABASE_B);
        missing.parent = Some(page_parent(COMMENT_B));
        assert!(
            databases::duplicate_database(&pool, missing)
                .await
                .err()
                .unwrap()
                .contains("parent page not found")
        );
        let mut missing = duplicate_request(BLOCK_D);
        missing.parent = None;
        missing.after_block_id = None;
        missing.replace_block_id = Some(BLOCK_D.into());
        assert!(
            databases::duplicate_database(&pool, missing)
                .await
                .err()
                .unwrap()
                .contains("block not found")
        );
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before, after);
    });
}

#[test]
fn database_trash_and_restore_cascade_nested_graphs_without_resurrecting_individually_trashed_rows()
{
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        add_nested_graph(&pool).await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: COMMENT_B.into(),
                title: "Already deleted".into(),
                first_block_id: COMMENT_C.into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::trash_page(&pool, COMMENT_B, true).await.unwrap();
        let previous_time: String =
            sqlx::query_scalar("SELECT trashed_time FROM notes_pages WHERE id = ?")
                .bind(COMMENT_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        writes::trash_block(&pool, DATABASE_A, true).await.unwrap();
        for id in [PAGE_B, BLOCK_D, COMMENT_A] {
            assert!(reads::get_page(&pool, id, false).await.is_err());
        }
        assert!(
            data_sources::schema::data_source_schema(&pool, DATA_SOURCE_A, None, None)
                .await
                .is_err()
        );
        writes::trash_block(&pool, DATABASE_A, true).await.unwrap();
        writes::trash_block(&pool, DATABASE_A, false).await.unwrap();
        for id in [PAGE_B, BLOCK_D, COMMENT_A] {
            assert!(reads::get_page(&pool, id, false).await.is_ok());
        }
        assert!(reads::get_block(&pool, BLOCK_F, false).await.is_ok());
        assert!(
            data_sources::schema::data_source_schema(&pool, DATA_SOURCE_A, None, None)
                .await
                .is_ok()
        );
        assert!(reads::get_page(&pool, COMMENT_B, false).await.is_err());
        let time: String = sqlx::query_scalar("SELECT trashed_time FROM notes_pages WHERE id = ?")
            .bind(COMMENT_B)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(time, previous_time);
        assert!(
            !page_history::list_page_history_snapshots(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn page_trash_cascades_owned_databases_and_respects_rows_deleted_again_during_restore() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        writes::trash_page(&pool, PAGE_B, false).await.unwrap();
        assert!(reads::get_block(&pool, BLOCK_B, false).await.is_ok());
        writes::trash_page(&pool, PAGE_B, true).await.unwrap();
        writes::trash_page(&pool, PAGE_A, false).await.unwrap();
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        assert!(reads::get_block(&pool, BLOCK_B, false).await.is_err());
        assert!(reads::get_block(&pool, DATABASE_A, false).await.is_ok());
        assert!(
            data_sources::schema::data_source_schema(&pool, DATA_SOURCE_A, None, None)
                .await
                .is_ok()
        );
    });
}

#[test]
fn copying_a_cut_database_includes_its_previously_live_nested_rows_and_excludes_older_trash() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        add_nested_graph(&pool).await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: COMMENT_B.into(),
                title: "Older trash".into(),
                first_block_id: COMMENT_C.into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::trash_page(&pool, COMMENT_B, true).await.unwrap();
        writes::trash_blocks(
            &pool,
            NoteTrashBlocks {
                block_ids: vec![DATABASE_A.into()],
                in_trash: Some(true),
            },
        )
        .await
        .unwrap();
        let copied = serde_json::to_value(
            writes::duplicate_blocks(
                &pool,
                NoteDuplicateBlocks {
                    block_ids: vec![DATABASE_A.into()],
                    parent: page_parent(PAGE_C),
                    after: Some(BLOCK_C.into()),
                    before: None,
                    include_trashed_sources: Some(true),
                    duplicated_block_ids: vec![NoteDuplicatedBlockId {
                        source_id: DATABASE_A.into(),
                        duplicate_id: LINKED_DATABASE_A.into(),
                    }],
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let source = copied["results"][0]["child_database"]["data_source_id"]
            .as_str()
            .unwrap();
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages WHERE parent_data_source_id = ?")
                .bind(source)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(rows, 1);
        let row = copied_row(&pool, source).await;
        let nested_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_pages WHERE title = 'Nested row' AND in_trash = 0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(nested_rows, 1);
        let page =
            serde_json::to_value(reads::get_page(&pool, &row, false).await.unwrap()).unwrap();
        assert_eq!(page["properties"]["Related"]["relation"][0]["id"], row);
        assert!(page["properties"].get("__ganbaru_trash_owner").is_none());
        assert!(
            copied["results"][0]["child_database"]
                .get("__ganbaru_trash")
                .is_none()
        );
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        assert!(reads::get_page(&pool, COMMENT_B, false).await.is_err());
    });
}

#[test]
fn copying_a_cut_note_preserves_nested_database_graphs() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: BLOCK_D.into(),
                title: "Containing note".into(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_E.into(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::move_blocks(
            &pool,
            NoteMoveBlocks {
                block_ids: vec![DATABASE_A.into()],
                parent: page_parent(BLOCK_D),
                after: None,
                before: None,
                include_trashed_sources: None,
            },
        )
        .await
        .unwrap();
        writes::trash_blocks(
            &pool,
            NoteTrashBlocks {
                block_ids: vec![BLOCK_D.into()],
                in_trash: Some(true),
            },
        )
        .await
        .unwrap();
        writes::duplicate_blocks(
            &pool,
            NoteDuplicateBlocks {
                block_ids: vec![BLOCK_D.into()],
                parent: page_parent(PAGE_C),
                after: None,
                before: None,
                include_trashed_sources: Some(true),
                duplicated_block_ids: vec![NoteDuplicatedBlockId {
                    source_id: BLOCK_D.into(),
                    duplicate_id: LINKED_DATABASE_A.into(),
                }],
            },
        )
        .await
        .unwrap();
        let source: String = sqlx::query_scalar("SELECT source.id FROM notes_data_sources AS source JOIN notes_blocks AS block ON block.id = source.database_id WHERE block.page_id = ?")
            .bind(LINKED_DATABASE_A).fetch_one(&pool).await.unwrap();
        assert_ne!(source, DATA_SOURCE_A);
        assert_ne!(copied_row(&pool, &source).await, PAGE_B);
    });
}

#[test]
fn moving_a_cut_database_restores_existing_identities_atomically_and_rejects_owned_row_destinations()
 {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        add_nested_graph(&pool).await;
        writes::trash_block(&pool, DATABASE_A, true).await.unwrap();
        let missing = writes::move_blocks(
            &pool,
            NoteMoveBlocks {
                block_ids: vec![DATABASE_A.into()],
                parent: page_parent(PAGE_C),
                after: Some(COMMENT_B.into()),
                before: None,
                include_trashed_sources: Some(true),
            },
        )
        .await;
        assert!(missing.is_err());
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        assert!(reads::get_block(&pool, DATABASE_A, false).await.is_err());
        let cycle = writes::move_blocks(
            &pool,
            NoteMoveBlocks {
                block_ids: vec![DATABASE_A.into()],
                parent: page_parent(PAGE_B),
                after: None,
                before: None,
                include_trashed_sources: Some(true),
            },
        )
        .await;
        assert!(
            cycle
                .err()
                .unwrap()
                .contains("page contained by its subtree")
        );
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        let moved = serde_json::to_value(
            writes::move_blocks(
                &pool,
                NoteMoveBlocks {
                    block_ids: vec![DATABASE_A.into()],
                    parent: page_parent(PAGE_C),
                    after: Some(BLOCK_C.into()),
                    before: None,
                    include_trashed_sources: Some(true),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(moved["results"][0]["id"], DATABASE_A);
        assert_eq!(
            moved["results"][0]["child_database"]["data_source_id"],
            DATA_SOURCE_A
        );
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_ok());
        assert!(reads::get_block(&pool, BLOCK_F, false).await.is_ok());
        let parent: String =
            sqlx::query_scalar("SELECT parent_page_id FROM notes_databases WHERE id = ?")
                .bind(DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parent, PAGE_C);
        assert_eq!(
            databases::database_reference(&pool, DATABASE_A)
                .await
                .unwrap()
                .canonical_source_page_id,
            PAGE_C
        );
    });
}

#[test]
fn copying_history_database_blocks_creates_independent_rows_and_sources() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let snapshot =
            page_history::record_page_snapshot_tx(&mut tx, PAGE_A, "copy_history_blocks")
                .await
                .unwrap()
                .unwrap();
        tx.commit().await.unwrap();
        let copied = serde_json::to_value(
            page_history::copy_page_history_blocks(
                &pool,
                PAGE_A,
                &snapshot,
                NotePageHistoryCopyBlocks {
                    after_block_id: Some(DATABASE_A.into()),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let block = copied["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["type"] == "child_database")
            .unwrap();
        let source = block["child_database"]["data_source_id"].as_str().unwrap();
        assert_ne!(source, DATA_SOURCE_A);
        assert_ne!(copied_row(&pool, source).await, PAGE_B);
    });
}

#[test]
fn duplicating_imported_database_placeholders_preserves_the_placeholder_without_inventing_data() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: None,
                children: vec![block(
                    DATABASE_A,
                    "child_database",
                    json!({"title": "Imported database"}),
                )],
            },
        )
        .await
        .unwrap();
        let copied = serde_json::to_value(
            writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
                .await
                .unwrap(),
        )
        .unwrap();
        let page = copied["page"]["id"].as_str().unwrap();
        let payload: String = sqlx::query_scalar(
            "SELECT payload FROM notes_blocks WHERE page_id = ? AND type = 'child_database'",
        )
        .bind(page)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&payload).unwrap()["title"],
            "Imported database"
        );
        let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sources, 0);
    });
}

#[test]
fn copying_database_rows_preserves_assets_literal_values_and_unavailable_relation_references() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: COMMENT_B.into(),
                title: "Deleted target".into(),
                first_block_id: COMMENT_C.into(),
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
                property_id: "related".into(),
                value: json!([PAGE_B, COMMENT_B]),
            },
        )
        .await
        .unwrap();
        writes::trash_page(&pool, COMMENT_B, true).await.unwrap();
        let asset =
            "notes/files/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png";
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_B),
                after: None,
                children: vec![block(
                    BLOCK_D,
                    "image",
                    local_media_payload(
                        asset,
                        "image/png",
                        42,
                        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
                        "Local image",
                        Some("image.png"),
                    ),
                )],
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_pages SET properties = json_set(properties, '$.Literal', json(?)) WHERE id = ?")
            .bind(json!({"id": "literal", "type": "email", "email": "high"}).to_string()).bind(PAGE_B).execute(&pool).await.unwrap();
        let copied = serde_json::to_value(
            databases::duplicate_database(&pool, duplicate_request(DATABASE_B))
                .await
                .unwrap(),
        )
        .unwrap();
        let row = copied_row(&pool, copied["data_source"]["id"].as_str().unwrap()).await;
        let page =
            serde_json::to_value(reads::get_page(&pool, &row, false).await.unwrap()).unwrap();
        assert_eq!(page["properties"]["Literal"]["email"], "high");
        assert_eq!(
            page["properties"]["Related"]["relation"][1]["id"],
            COMMENT_B
        );
        let links: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_data_source_relation_links WHERE source_page_id = ?",
        )
        .bind(&row)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(links, 1);
        let pins: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_asset_references WHERE asset_id = ? AND owner_type = 'block' AND page_id = ?")
            .bind(asset).bind(&row).fetch_one(&pool).await.unwrap();
        assert_eq!(pins, 1);
        writes::trash_block(&pool, DATABASE_A, true).await.unwrap();
        assert!(reads::get_page(&pool, &row, false).await.is_ok());
    });
}

#[test]
fn page_history_restore_refuses_to_delete_database_graphs() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        let mut tx = pool.begin().await.unwrap();
        let snapshot =
            page_history::record_page_snapshot_tx(&mut tx, PAGE_A, "copy_history_blocks")
                .await
                .unwrap()
                .unwrap();
        tx.commit().await.unwrap();
        let error = page_history::restore_page_history_snapshot(&pool, PAGE_A, &snapshot)
            .await
            .err()
            .unwrap();
        assert!(error.contains("project version"));
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_ok());
        assert!(
            data_sources::schema::data_source_schema(&pool, DATA_SOURCE_A, None, None)
                .await
                .is_ok()
        );
        assert_eq!(
            databases::database_reference(&pool, DATABASE_A)
                .await
                .unwrap()
                .owned_data_source_count,
            1
        );
    });
}

#[test]
fn containing_block_trash_restores_its_owned_database_but_keeps_previously_deleted_children_in_trash()
 {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("toggle", paragraph_payload("Container")),
        )
        .await
        .unwrap();
        writes::move_blocks(
            &pool,
            NoteMoveBlocks {
                block_ids: vec![DATABASE_A.into()],
                parent: block_parent(BLOCK_A),
                after: None,
                before: None,
                include_trashed_sources: None,
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Deleted first"),
                )],
            },
        )
        .await
        .unwrap();
        writes::trash_block(&pool, BLOCK_D, true).await.unwrap();
        writes::trash_blocks(
            &pool,
            NoteTrashBlocks {
                block_ids: vec![BLOCK_A.into(), DATABASE_A.into()],
                in_trash: Some(true),
            },
        )
        .await
        .unwrap();
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_err());
        writes::trash_block(&pool, BLOCK_A, false).await.unwrap();
        assert!(reads::get_page(&pool, PAGE_B, false).await.is_ok());
        assert!(reads::get_block(&pool, DATABASE_A, false).await.is_ok());
        assert!(reads::get_block(&pool, BLOCK_D, false).await.is_err());
    });
}

#[test]
fn multi_source_database_copy_remaps_inverse_relations_with_overlapping_schema_ids() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        sqlx::query("INSERT INTO notes_data_sources (id, database_id, title, properties) VALUES (?, ?, 'Second source', ?)")
            .bind(DATA_SOURCE_B).bind(DATABASE_A).bind(json!({
                "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
                "Priority": {"id": "priority", "name": "Priority", "type": "select", "select": {
                    "options": [{"id": "high", "name": "High", "color": "red"}]}},
                "Related": {"id": "related", "name": "Related", "type": "relation", "relation": {
                    "data_source_id": DATA_SOURCE_A, "dual_property": {"synced_property_id": "related", "synced_property_name": "Related"}}}
            }).to_string()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO notes_database_views (id, database_id, data_source_id, name, type) VALUES (?, ?, ?, 'Second table', 'table')")
            .bind(DATABASE_VIEW_B).bind(DATABASE_A).bind(DATA_SOURCE_B).execute(&pool).await.unwrap();
        sqlx::query("UPDATE notes_data_sources SET properties = json_set(properties, '$.Related.relation', json(?)) WHERE id = ?")
            .bind(json!({"data_source_id": DATA_SOURCE_B, "dual_property": {"synced_property_id": "related", "synced_property_name": "Related"}}).to_string())
            .bind(DATA_SOURCE_A).execute(&pool).await.unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: COMMENT_A.into(),
                title: "Second source row".into(),
                first_block_id: BLOCK_D.into(),
                properties: None,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_pages SET properties = json_set(properties, '$.Related.relation', json(?)) WHERE id = ?")
            .bind(json!([{"id": COMMENT_A}]).to_string()).bind(PAGE_B).execute(&pool).await.unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_B,
            COMMENT_A,
            NoteDataSourceRowPropertyUpdate {
                property_id: "related".into(),
                value: json!([PAGE_B]),
            },
        )
        .await
        .unwrap();
        databases::duplicate_database(&pool, duplicate_request(DATABASE_B))
            .await
            .unwrap();
        let sources: Vec<(String, String)> =
            sqlx::query_as("SELECT id, properties FROM notes_data_sources WHERE database_id = ?")
                .bind(DATABASE_B)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(sources.len(), 2);
        let schema_a: Value = serde_json::from_str(&sources[0].1).unwrap();
        let schema_b: Value = serde_json::from_str(&sources[1].1).unwrap();
        assert_ne!(schema_a["Related"]["id"], schema_b["Related"]["id"]);
        for (source_id, raw) in &sources {
            let schema: Value = serde_json::from_str(raw).unwrap();
            let target_source = schema["Related"]["relation"]["data_source_id"]
                .as_str()
                .unwrap();
            let target_schema: Value = serde_json::from_str(
                &sources
                    .iter()
                    .find(|(id, _)| id == target_source)
                    .unwrap()
                    .1,
            )
            .unwrap();
            assert_eq!(
                schema["Related"]["relation"]["dual_property"]["synced_property_id"],
                target_schema["Related"]["id"]
            );
            let row = copied_row(&pool, source_id).await;
            let target_row = copied_row(&pool, target_source).await;
            let page =
                serde_json::to_value(reads::get_page(&pool, &row, false).await.unwrap()).unwrap();
            assert_eq!(
                page["properties"]["Related"]["relation"][0]["id"],
                target_row
            );
        }
    });
}

#[test]
fn copied_database_templates_apply_independent_nested_notes_and_database_bodies() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_database(&pool).await;
        add_nested_graph(&pool).await;
        data_sources::templates::create_data_source_template_from_row(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceTemplateCreateFromRow {
                id: TEMPLATE_A.into(),
                source_page_id: PAGE_B.into(),
                name: "Nested template".into(),
                is_default: None,
            },
        )
        .await
        .unwrap();
        let copied = serde_json::to_value(
            databases::duplicate_database(&pool, duplicate_request(LINKED_DATABASE_A))
                .await
                .unwrap(),
        )
        .unwrap();
        let source = copied["data_source"]["id"].as_str().unwrap();
        let templates = serde_json::to_value(
            data_sources::templates::list_data_source_templates(&pool, source)
                .await
                .unwrap(),
        )
        .unwrap();
        let applied = serde_json::to_value(
            data_sources::templates::apply_data_source_template(
                &pool,
                source,
                templates[0]["id"].as_str().unwrap(),
                NoteDataSourceTemplateApply {
                    id: None,
                    title: None,
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let child: String =
            sqlx::query_scalar("SELECT id FROM notes_pages WHERE parent_page_id = ?")
                .bind(applied["page"]["id"].as_str().unwrap())
                .fetch_one(&pool)
                .await
                .unwrap();
        let nested_source: String = sqlx::query_scalar("SELECT source.id FROM notes_data_sources AS source JOIN notes_blocks AS block ON block.id = source.database_id WHERE block.page_id = ?")
            .bind(child).fetch_one(&pool).await.unwrap();
        let row = copied_row(&pool, &nested_source).await;
        let body: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE page_id = ?")
                .bind(&row)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(body, "Deep body");
        assert_ne!(nested_source, DATA_SOURCE_B);
        assert_ne!(row, COMMENT_A);
    });
}

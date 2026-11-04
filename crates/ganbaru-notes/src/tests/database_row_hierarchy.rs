use super::helpers::*;
use crate::data_sources::row_hierarchy::NoteDataSourceRowParentUpdate;
use crate::models::NoteDatabaseDuplicate;
use serde_json::Value;

async fn seed_source(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    create_database(
        pool,
        DATABASE_A,
        DATA_SOURCE_A,
        DATABASE_VIEW_A,
        "Tree",
        BLOCK_A,
    )
    .await;
    data_sources::rows::create_data_source_row_page(
        pool,
        DATA_SOURCE_A,
        NoteDataSourceRowPageCreate {
            id: PAGE_B.into(),
            first_block_id: BLOCK_B.into(),
            title: "Parent".into(),
            properties: None,
        },
    )
    .await
    .unwrap();
}

async fn metadata(pool: &SqlitePool, ids: &[&str]) -> Value {
    let mut tx = pool.begin().await.unwrap();
    let rows = data_sources::row_hierarchy::row_metadata_tx(
        &mut tx,
        DATA_SOURCE_A,
        &ids.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    serde_json::to_value(rows).unwrap()
}

async fn create_child_row(pool: &SqlitePool) {
    data_sources::row_hierarchy::create_subitem(
        pool,
        DATA_SOURCE_A,
        PAGE_B,
        NoteDataSourceRowPageCreate {
            id: PAGE_C.into(),
            first_block_id: BLOCK_C.into(),
            title: "Child".into(),
            properties: None,
        },
    )
    .await
    .unwrap();
}

#[test]
fn database_subitems_keep_source_ownership_and_reject_cycles_and_foreign_parents_atomically() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_source(&pool).await;
        create_child_row(&pool).await;
        let page = serde_json::to_value(reads::load_page(&pool, PAGE_C).await.unwrap()).unwrap();
        assert_eq!(page["page"]["parent"]["data_source_id"], DATA_SOURCE_A);
        for (row, parent) in [(PAGE_B, PAGE_C), (PAGE_C, PAGE_C), (PAGE_C, PAGE_A)] {
            assert!(
                data_sources::row_hierarchy::set_row_parent(
                    &pool,
                    DATA_SOURCE_A,
                    row,
                    NoteDataSourceRowParentUpdate {
                        parent_row_page_id: Some(parent.into()),
                    }
                )
                .await
                .is_err()
            );
        }
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Other",
            BLOCK_A,
        )
        .await;
        let foreign_row = "44444444-4444-4444-8444-444444444444";
        sqlx::query("INSERT INTO notes_pages(id, parent_type, parent_data_source_id, title) VALUES (?, 'data_source_id', ?, 'Foreign')")
            .bind(foreign_row).bind(DATA_SOURCE_B).execute(&pool).await.unwrap();
        assert!(
            data_sources::row_hierarchy::set_row_parent(
                &pool,
                DATA_SOURCE_A,
                PAGE_C,
                NoteDataSourceRowParentUpdate {
                    parent_row_page_id: Some(foreign_row.into()),
                }
            )
            .await
            .is_err()
        );
        let rows = metadata(&pool, &[PAGE_B, PAGE_C]).await;
        assert_eq!(rows[PAGE_C]["parent_row_page_id"], PAGE_B);
        assert_eq!(rows[PAGE_C]["ancestor_row_page_ids"], json!([PAGE_B]));
        assert_eq!(rows[PAGE_B]["child_count"], 1);
        data_sources::row_hierarchy::set_row_parent(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
            NoteDataSourceRowParentUpdate {
                parent_row_page_id: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(metadata(&pool, &[PAGE_C]).await[PAGE_C]["depth"], 0);
    });
}

#[test]
fn database_subitems_report_unloaded_ancestors_and_counts_and_restore_trashed_relationships() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_source(&pool).await;
        create_child_row(&pool).await;
        let unloaded = metadata(&pool, &[PAGE_C]).await;
        assert_eq!(unloaded[PAGE_C]["depth"], 1);
        assert_eq!(unloaded[PAGE_C]["parent_row_page_id"], PAGE_B);
        sqlx::query("UPDATE notes_database_views SET filter = ? WHERE id = ?")
            .bind(json!({"type": "and", "filters": [{"property_id": "title", "condition": "equals", "value": "Child"}]}).to_string())
            .bind(DATABASE_VIEW_A).execute(&pool).await.unwrap();
        let window = serde_json::to_value(
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(window["rows"].as_array().unwrap().len(), 1);
        assert_eq!(
            window["row_hierarchy"][PAGE_C]["ancestor_row_page_ids"],
            json!([PAGE_B])
        );
        sqlx::query("UPDATE notes_pages SET in_trash = 1 WHERE id = ?")
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        let orphan = metadata(&pool, &[PAGE_C]).await;
        assert_eq!(orphan[PAGE_C]["depth"], 0);
        assert!(orphan[PAGE_C]["parent_row_page_id"].is_null());
        sqlx::query("UPDATE notes_pages SET in_trash = 0 WHERE id = ?")
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(metadata(&pool, &[PAGE_C]).await[PAGE_C]["depth"], 1);
        writes::update_page(
            &pool,
            PAGE_B,
            NotePageUpdate {
                title: None,
                parent: Some(workspace_parent()),
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        assert_eq!(metadata(&pool, &[PAGE_C]).await[PAGE_C]["depth"], 0);
        let edge_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_pages WHERE parent_row_page_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(edge_count, 0);
    });
}

#[test]
fn database_subitems_validate_subtree_depth_and_roll_back_new_row_documents_at_the_bound() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_source(&pool).await;
        let mut parent = PAGE_B.to_string();
        for depth in 1..=data_sources::row_hierarchy::MAX_ROW_HIERARCHY_DEPTH {
            let id = format!("40000000-0000-4000-8000-{depth:012}");
            sqlx::query("INSERT INTO notes_pages(id, parent_type, parent_data_source_id, title) VALUES (?, 'data_source_id', ?, ?)")
                .bind(&id).bind(DATA_SOURCE_A).bind(format!("Depth {depth}")).execute(&pool).await.unwrap();
            data_sources::row_hierarchy::set_row_parent(
                &pool,
                DATA_SOURCE_A,
                &id,
                NoteDataSourceRowParentUpdate {
                    parent_row_page_id: Some(parent),
                },
            )
            .await
            .unwrap();
            parent = id;
        }
        assert!(
            data_sources::row_hierarchy::create_subitem(
                &pool,
                DATA_SOURCE_A,
                &parent,
                NoteDataSourceRowPageCreate {
                    id: PAGE_C.into(),
                    first_block_id: BLOCK_C.into(),
                    title: "Too deep".into(),
                    properties: None,
                }
            )
            .await
            .is_err()
        );
        let absent: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM notes_pages WHERE id = ?) + (SELECT COUNT(*) FROM notes_blocks WHERE id = ?)")
            .bind(PAGE_C).bind(BLOCK_C).fetch_one(&pool).await.unwrap();
        assert_eq!(absent, 0);
        sqlx::query("INSERT INTO notes_pages(id, parent_type, parent_data_source_id, title) VALUES (?, 'data_source_id', ?, 'Other root')")
            .bind(PAGE_C).bind(DATA_SOURCE_A).execute(&pool).await.unwrap();
        assert!(
            data_sources::row_hierarchy::set_row_parent(
                &pool,
                DATA_SOURCE_A,
                PAGE_B,
                NoteDataSourceRowParentUpdate {
                    parent_row_page_id: Some(PAGE_C.into()),
                }
            )
            .await
            .is_err()
        );
        assert_eq!(
            metadata(&pool, &[&parent]).await[&parent]["depth"],
            data_sources::row_hierarchy::MAX_ROW_HIERARCHY_DEPTH
        );
        sqlx::query("UPDATE notes_pages SET parent_row_page_id = ? WHERE id = ?")
            .bind(PAGE_C)
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        assert!(
            data_sources::row_hierarchy::validate_sources_tx(&mut tx, &[DATA_SOURCE_A.into()])
                .await
                .unwrap_err()
                .contains("depth exceeds")
        );
        tx.rollback().await.unwrap();
    });
}

#[test]
fn database_subitems_copy_and_export_canonical_relationships_with_independent_row_identities() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_source(&pool).await;
        create_child_row(&pool).await;
        let copied = serde_json::to_value(
            databases::duplicate_database(
                &pool,
                NoteDatabaseDuplicate {
                    source_block_id: DATABASE_A.into(),
                    id: DATABASE_B.into(),
                    parent: Some(page_parent(PAGE_A)),
                    after_block_id: Some(BLOCK_A.into()),
                    replace_block_id: None,
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let copied_source = copied["data_source"]["id"].as_str().unwrap();
        let edge = sqlx::query("SELECT child.id AS row_page_id, child.parent_row_page_id, child.title AS child_title, parent.title AS parent_title
            FROM notes_pages AS child JOIN notes_pages AS parent ON parent.id = child.parent_row_page_id
            WHERE child.parent_data_source_id = ?")
            .bind(copied_source).fetch_one(&pool).await.unwrap();
        assert_eq!(edge.get::<String, _>("child_title"), "Child");
        assert_eq!(edge.get::<String, _>("parent_title"), "Parent");
        assert_ne!(edge.get::<String, _>("row_page_id"), PAGE_C);
        assert_ne!(edge.get::<String, _>("parent_row_page_id"), PAGE_B);
        let export = transfers::json_graph_export::export_graph(
            &pool,
            serde_json::from_value(json!({})).unwrap(),
        )
        .await
        .unwrap();
        let graph: Value = serde_json::from_str(&export.json).unwrap();
        assert_eq!(
            graph["graph"]["pages"]["notes_pages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|page| !page["parent_row_page_id"].is_null())
                .count(),
            2
        );
        let duplicate = serde_json::to_value(
            writes::duplicate_page(
                &pool,
                PAGE_C,
                NoteDuplicatePage {
                    title: Some("Child copy".into()),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let duplicate_id = duplicate["page"]["id"].as_str().unwrap();
        assert_eq!(
            metadata(&pool, &[duplicate_id]).await[duplicate_id]["parent_row_page_id"],
            PAGE_B
        );
        let duplicated_parent = serde_json::to_value(
            writes::duplicate_page(
                &pool,
                PAGE_B,
                NoteDuplicatePage {
                    title: Some("Parent copy".into()),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        let duplicated_parent_id = duplicated_parent["page"]["id"].as_str().unwrap();
        let parent_children: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages WHERE parent_row_page_id = ?")
                .bind(duplicated_parent_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parent_children, 2);
        assert_eq!(
            metadata(&pool, &[duplicated_parent_id]).await[duplicated_parent_id]["depth"],
            0
        );
        sqlx::query("UPDATE notes_pages SET parent_row_page_id = ? WHERE id = ?")
            .bind(PAGE_C)
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            databases::duplicate_database(
                &pool,
                NoteDatabaseDuplicate {
                    source_block_id: DATABASE_A.into(),
                    id: LINKED_DATABASE_A.into(),
                    parent: Some(page_parent(PAGE_A)),
                    after_block_id: Some(BLOCK_A.into()),
                    replace_block_id: None,
                }
            )
            .await
            .is_err()
        );
        let no_partial_copy: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE id = ?")
                .bind(LINKED_DATABASE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(no_partial_copy, 0);
    });
}

#[test]
fn row_page_context_hydrates_computed_values_with_parent_children_and_lock_state() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        seed_source(&pool).await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                    "Estimate": { "id": "estimate", "name": "Estimate", "type": "number", "number": { "format": "number" } },
                    "Score": { "id": "score", "name": "Score", "type": "formula", "formula": { "expression": "prop(\"Estimate\") * 2" }, "page_visibility": "hide_when_empty" }
                }),
            },
        )
        .await
        .unwrap();
        create_child_row(&pool).await;
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "estimate".to_string(),
                value: json!(3),
            },
        )
        .await
        .unwrap();

        let child = serde_json::to_value(
            data_sources::row_page::row_page_context(&pool, DATA_SOURCE_A, PAGE_C)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(child["row"]["id"], PAGE_C);
        assert_eq!(
            child["row"]["properties"]["Score"]["formula"]["number"].as_f64(),
            Some(6.0)
        );
        assert_eq!(child["parent"]["id"], PAGE_B);
        assert_eq!(child["hierarchy"]["depth"], 1);
        assert_eq!(child["children"], json!([]));
        assert_eq!(
            child["schema"]["data_source"]["properties"]["Score"]["page_visibility"],
            "hide_when_empty"
        );
        assert_eq!(child["schema"]["view"]["id"], DATABASE_VIEW_A);
        assert_eq!(child["editing_locked"], false);

        databases::editing_lock::set_database_editing_lock(&pool, DATABASE_A, true)
            .await
            .unwrap();
        let parent = serde_json::to_value(
            data_sources::row_page::row_page_context(&pool, DATA_SOURCE_A, PAGE_B)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(parent["parent"].is_null());
        assert_eq!(parent["hierarchy"]["child_count"], 1);
        assert_eq!(
            parent["children"]
                .as_array()
                .unwrap()
                .iter()
                .map(|child| child["id"].clone())
                .collect::<Vec<_>>(),
            vec![json!(PAGE_C)]
        );
        assert_eq!(parent["editing_locked"], true);

        assert!(
            data_sources::row_page::row_page_context(&pool, DATA_SOURCE_A, PAGE_A)
                .await
                .is_err()
        );
    });
}

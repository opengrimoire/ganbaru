use super::helpers::*;

#[test]
fn database_relations_persist_links_backlinks_and_search() {
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
            "Projects",
            DATABASE_A,
        )
        .await;
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
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B
                        }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                title: "Project Alpha".to_string(),
                first_block_id: BLOCK_C.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Write relation tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
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
                property_id: "project_relation".to_string(),
                value: json!([PAGE_C]),
            },
        )
        .await
        .unwrap();

        let links: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT source_page_id, source_property_id, target_page_id, target_data_source_id
             FROM notes_data_source_relation_links",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            links,
            vec![(
                PAGE_B.to_string(),
                "project_relation".to_string(),
                PAGE_C.to_string(),
                DATA_SOURCE_B.to_string()
            )]
        );

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        assert_eq!(
            table_json["rows"][0]["properties"]["Project"]["relation"][0]["title"],
            "Project Alpha"
        );

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_C)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert!(backlinks_json.as_array().unwrap().iter().any(|backlink| {
            backlink["reference_type"] == "database_relation"
                && backlink["source_page"]["id"] == PAGE_B
        }));

        let search_results = search::search(&pool, "Project Alpha", Some(10), false)
            .await
            .unwrap();
        let search_json = serde_json::to_value(search_results).unwrap();
        assert!(
            search_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["page"]["id"] == PAGE_B })
        );

        let invalid = data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "project_relation".to_string(),
                value: json!([PAGE_B]),
            },
        )
        .await;
        assert_eq!(
            invalid.err().unwrap(),
            "relation target page must belong to the configured data source"
        );
    });
}

#[test]
fn database_relations_sync_two_way_when_inverse_property_is_configured() {
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
            "Projects",
            DATABASE_A,
        )
        .await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_B,
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
                    "Tasks": {
                        "id": "tasks_relation",
                        "name": "Tasks",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_A
                        }
                    }
                }),
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
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B,
                            "dual_property": {
                                "synced_property_id": "tasks_relation",
                                "synced_property_name": "Tasks"
                            }
                        }
                    }
                }),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_B,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                title: "Project Alpha".to_string(),
                first_block_id: BLOCK_C.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Write relation tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
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
                property_id: "project_relation".to_string(),
                value: json!([PAGE_C]),
            },
        )
        .await
        .unwrap();

        let project = reads::get_page(&pool, PAGE_C, false).await.unwrap();
        let project_json = serde_json::to_value(project).unwrap();
        assert_eq!(
            project_json["properties"]["Tasks"]["relation"][0]["id"],
            PAGE_B
        );
        let link_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_source_relation_links")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(link_count, 2);

        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "project_relation".to_string(),
                value: json!([]),
            },
        )
        .await
        .unwrap();

        let project = reads::get_page(&pool, PAGE_C, false).await.unwrap();
        let project_json = serde_json::to_value(project).unwrap();
        assert_eq!(project_json["properties"]["Tasks"]["relation"], json!([]));
        let link_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_data_source_relation_links")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(link_count, 0);
    });
}

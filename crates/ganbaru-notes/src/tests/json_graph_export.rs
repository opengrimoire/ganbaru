use super::helpers::*;

const LOCAL_ASSET_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn json_graph_export_includes_canonical_tables_counts_schema_and_warnings() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Graph Root".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "image",
                    local_media_payload(
                        "notes/files/graph.png",
                        "image/png",
                        12,
                        LOCAL_ASSET_SHA,
                        "Graph",
                        Some("graph.png"),
                    ),
                )],
            },
        )
        .await
        .unwrap();
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_B,
        )
        .await;
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Graph note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        search::rebuild_index(&pool).await.unwrap();
        links::backlinks::rebuild_index(&pool).await.unwrap();
        links::facts::rebuild_index(&pool).await.unwrap();

        let result = transfers::json_graph_export::export_graph(
            &pool,
            NoteJsonGraphExportRequest {
                include_indexes: Some(true),
                include_history: Some(true),
                include_templates: Some(true),
                include_local_state: Some(true),
                pretty: Some(true),
            },
        )
        .await
        .unwrap();
        let result_json = serde_json::to_value(&result).unwrap();
        let graph: serde_json::Value =
            serde_json::from_str(result_json["json"].as_str().unwrap()).unwrap();

        assert_eq!(result_json["object"], "notes_json_graph_export");
        assert_eq!(result_json["schema_version"], "notes-json-graph.v1");
        assert_eq!(result_json["exported_page_count"], 1);
        assert!(result_json["exported_block_count"].as_i64().unwrap() >= 3);
        assert_eq!(result_json["exported_comment_count"], 1);
        assert_eq!(result_json["exported_data_source_count"], 1);
        assert_eq!(result_json["exported_file_count"], 1);
        assert!(result_json["exported_index_record_count"].as_i64().unwrap() > 0);
        assert_eq!(result_json["exported_property_schema_count"], 1);
        assert!(result_json["warning_count"].as_i64().unwrap() >= 2);
        assert_eq!(graph["object"], "notes_json_graph");
        assert_eq!(
            graph["database_schema"]["migration_table"],
            "_sqlx_migrations"
        );
        assert_eq!(graph["counts"]["tables"]["notes_pages"], 1);
        assert_eq!(graph["counts"]["tables"]["notes_data_sources"], 1);

        let pages = graph["graph"]["pages"]["notes_pages"].as_array().unwrap();
        assert_eq!(pages[0]["properties"]["title"]["type"], "title");
        let blocks = graph["graph"]["blocks"]["notes_blocks"].as_array().unwrap();
        assert!(blocks.iter().any(|block| block["payload"].is_object()));
        let data_sources = graph["graph"]["data_sources"]["notes_data_sources"]
            .as_array()
            .unwrap();
        assert_eq!(data_sources[0]["properties"]["Name"]["type"], "title");
        assert_eq!(
            graph["graph"]["comments"]["notes_comments"][0]["rich_text"][0]["plain_text"],
            "Graph note"
        );
        assert_eq!(
            graph["graph"]["files"]["notes_assets"][0]["asset_path"],
            "notes/files/graph.png"
        );
        assert!(
            graph["graph"]["indexes"]["notes_search_index"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["page_id"] == PAGE_A)
        );
        assert!(graph["graph"]["indexes"].get("notes_search_fts").is_none());
        assert!(
            result_json["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "json_graph_rebuildable_fts_omitted")
        );
        assert!(
            result_json["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "json_graph_asset_bytes_not_embedded")
        );
    });
}

#[test]
fn json_graph_export_can_exclude_rebuildable_indexes() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        search::rebuild_index(&pool).await.unwrap();

        let result = transfers::json_graph_export::export_graph(
            &pool,
            NoteJsonGraphExportRequest {
                include_indexes: Some(false),
                include_history: Some(true),
                include_templates: Some(true),
                include_local_state: Some(true),
                pretty: Some(false),
            },
        )
        .await
        .unwrap();
        let result_json = serde_json::to_value(&result).unwrap();
        let graph: serde_json::Value =
            serde_json::from_str(result_json["json"].as_str().unwrap()).unwrap();

        assert!(graph["graph"].get("indexes").is_none());
        assert_eq!(result_json["exported_index_record_count"], 0);
        assert!(
            result_json["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "json_graph_indexes_excluded")
        );
    });
}

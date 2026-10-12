use super::helpers::*;

#[test]
fn search_returns_page_block_and_comment_matches() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "Target page".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        create_page(&pool, PAGE_B, BLOCK_B).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_B),
                after: Some(BLOCK_B.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("Target block"),
                )],
            },
        )
        .await
        .unwrap();
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_B)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Target comment")],
                attachments: None,
            },
        )
        .await
        .unwrap();

        let results = search::search(&pool, "target", Some(10), false)
            .await
            .unwrap();
        let results_json = serde_json::to_value(results).unwrap();
        assert_eq!(results_json.as_array().unwrap().len(), 3);
        assert_eq!(results_json[0]["type"], "page");
        assert_eq!(results_json[0]["page"]["id"], PAGE_A);
        assert_eq!(results_json[1]["type"], "block");
        assert_eq!(results_json[1]["block_id"], BLOCK_C);
        assert_eq!(results_json[2]["type"], "comment");
        assert_eq!(results_json[2]["comment_id"], COMMENT_A);
    });
}

#[test]
fn search_keyset_pages_equal_titles_without_full_page_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::raw_sql(
            "WITH RECURSIVE sequence(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM sequence WHERE value < 60
             )
             INSERT INTO notes_pages (id, parent_type, title, properties, last_edited_time)
             SELECT printf('10000000-0000-4000-8000-%012d', value), 'workspace', 'Needle same',
                    json_object('title', json_object('id', 'title', 'type', 'title', 'title', json_array())),
                    '2026-07-11T00:00:00.000Z'
             FROM sequence;",
        )
        .execute(&pool)
        .await
        .unwrap();
        search::rebuild_index(&pool).await.unwrap();

        let first = serde_json::to_value(
            search::search_window(&pool, "needle", Some(20), false, None)
                .await
                .unwrap(),
        )
        .unwrap();
        let cursor = first["next_cursor"].as_str().unwrap();
        let second = serde_json::to_value(
            search::search_window(&pool, "needle", Some(20), false, Some(cursor))
                .await
                .unwrap(),
        )
        .unwrap();
        let first_ids = first["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| result["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        let second_results = second["results"].as_array().unwrap();

        assert_eq!(first_ids.len(), 20);
        assert_eq!(second_results.len(), 20);
        assert!(
            second_results
                .iter()
                .all(|result| !first_ids.contains(result["id"].as_str().unwrap()))
        );
        let page = &first["results"][0]["page"];
        assert!(page.get("properties").is_none());
        assert!(page.get("cover").is_none());
        assert!(page.get("source_provider").is_none());
        let plan = sqlx::query(
            "EXPLAIN QUERY PLAN SELECT idx.id
             FROM notes_search_fts
             JOIN notes_search_index AS idx ON idx.id = notes_search_fts.index_id
             WHERE notes_search_fts MATCH 'needle*' LIMIT 20",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let details = plan
            .iter()
            .map(|row| sqlx::Row::get::<String, _>(row, "detail"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            details.contains("VIRTUAL TABLE INDEX"),
            "unexpected plan: {details}"
        );
    });
}

#[test]
fn search_indexes_comment_targets_authors_anchors_and_resolved_filter() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    paragraph_payload("Anchor text for comments"),
                )],
            },
        )
        .await
        .unwrap();

        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("discussion needle")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_B.to_string(),
                parent: Some(block_parent(BLOCK_B)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("block needle")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let inline_thread = comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_C.to_string(),
                parent: Some(block_parent(BLOCK_B)),
                discussion_id: None,
                anchor: Some(NoteCommentAnchorCreate {
                    start: 0,
                    end: 6,
                    text: "Anchor".to_string(),
                    prefix: String::new(),
                    suffix: " text".to_string(),
                }),
                rich_text: vec![rich_text("inline needle")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        let inline_thread_json = serde_json::to_value(inline_thread).unwrap();
        let inline_thread_id = inline_thread_json["id"].as_str().unwrap();
        comments::resolve_comment_thread(&pool, inline_thread_id, true)
            .await
            .unwrap();

        let discussion_results = search::search(&pool, "discussion needle", Some(10), false)
            .await
            .unwrap();
        let discussion_json = serde_json::to_value(discussion_results).unwrap();
        assert_eq!(discussion_json.as_array().unwrap().len(), 1);
        assert_eq!(discussion_json[0]["type"], "comment");
        assert_eq!(discussion_json[0]["comment_id"], COMMENT_A);
        assert_eq!(discussion_json[0]["block_id"], serde_json::Value::Null);
        assert_eq!(discussion_json[0]["comment_status"], "open");
        assert_eq!(discussion_json[0]["comment_author"]["resolved_name"], "You");

        let block_results = search::search(&pool, "block needle", Some(10), false)
            .await
            .unwrap();
        let block_json = serde_json::to_value(block_results).unwrap();
        assert_eq!(block_json.as_array().unwrap().len(), 1);
        assert_eq!(block_json[0]["type"], "comment");
        assert_eq!(block_json[0]["comment_id"], COMMENT_B);
        assert_eq!(block_json[0]["block_id"], BLOCK_B);
        assert_eq!(block_json[0]["comment_anchor"], serde_json::Value::Null);

        let hidden_resolved = search::search(&pool, "inline needle", Some(10), false)
            .await
            .unwrap();
        assert!(
            serde_json::to_value(hidden_resolved)
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );

        let included_resolved = search::search(&pool, "inline needle", Some(10), true)
            .await
            .unwrap();
        let included_json = serde_json::to_value(included_resolved).unwrap();
        assert_eq!(included_json.as_array().unwrap().len(), 1);
        assert_eq!(included_json[0]["comment_id"], COMMENT_C);
        assert_eq!(included_json[0]["block_id"], BLOCK_B);
        assert_eq!(included_json[0]["comment_status"], "resolved");
        assert_eq!(included_json[0]["comment_anchor"]["text"], "Anchor");
    });
}

#[test]
fn search_fts_rebuilds_and_indexes_properties_files_and_metadata() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        sqlx::query(
            "UPDATE notes_pages
             SET properties = ?,
                 last_edited_time = '2035-07-02T10:00:00.000Z'
             WHERE id = ?",
        )
        .bind(
            json!({
                "Name": {
                    "id": "title",
                    "name": "Name",
                    "type": "title",
                    "title": [rich_text("Search Host")]
                },
                "Status": {
                    "id": "status",
                    "name": "Status",
                    "type": "status",
                    "status": {
                        "id": "status-review",
                        "name": "Deep Review",
                        "color": "green"
                    }
                },
                "Due": {
                    "id": "due",
                    "name": "Due",
                    "type": "date",
                    "date": {
                        "start": "2035-07-02",
                        "end": null
                    }
                }
            })
            .to_string(),
        )
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "image",
                    local_media_payload(
                        "notes/files/c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3.png",
                        "image/png",
                        128,
                        "c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3",
                        "Quarterly diagram caption",
                        Some("roadmap-sketch.png"),
                    ),
                )],
            },
        )
        .await
        .unwrap();

        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Comment body")],
                attachments: Some(vec![local_comment_attachment(
                    "notes/files/d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4.pdf",
                    "application/pdf",
                    256,
                    "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4",
                    "meeting-notes.pdf",
                )]),
            },
        )
        .await
        .unwrap();

        let property_results = search::search(&pool, "Deep Review", Some(10), false)
            .await
            .unwrap();
        let property_json = serde_json::to_value(property_results).unwrap();
        assert!(
            property_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "page" && result["page"]["id"] == PAGE_A })
        );

        let caption_results = search::search(&pool, "Quarterly", Some(10), false)
            .await
            .unwrap();
        let caption_json = serde_json::to_value(caption_results).unwrap();
        assert!(
            caption_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "block" && result["block_id"] == BLOCK_C })
        );

        let file_results = search::search(&pool, "roadmap", Some(10), false)
            .await
            .unwrap();
        let file_json = serde_json::to_value(file_results).unwrap();
        assert!(
            file_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "block" && result["block_id"] == BLOCK_C })
        );

        let comment_file_results = search::search(&pool, "meeting notes", Some(10), false)
            .await
            .unwrap();
        let comment_file_json = serde_json::to_value(comment_file_results).unwrap();
        assert!(
            comment_file_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "comment" && result["comment_id"] == COMMENT_A })
        );

        sqlx::query("DELETE FROM notes_search_fts")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM notes_search_index")
            .execute(&pool)
            .await
            .unwrap();
        let rebuilt_results = search::search(&pool, "roadmap", Some(10), false)
            .await
            .unwrap();
        let rebuilt_json = serde_json::to_value(rebuilt_results).unwrap();
        assert!(
            rebuilt_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "block" && result["block_id"] == BLOCK_C })
        );

        sqlx::query(
            "UPDATE notes_pages
             SET properties = ?,
                 last_edited_time = '2035-07-02T10:01:00.000Z'
             WHERE id = ?",
        )
        .bind(
            json!({
                "Name": {
                    "id": "title",
                    "name": "Name",
                    "type": "title",
                    "title": [rich_text("Search Host")]
                },
                "Status": {
                    "id": "status",
                    "name": "Status",
                    "type": "status",
                    "status": {
                        "id": "status-fresh",
                        "name": "Fresh Signal",
                        "color": "blue"
                    }
                }
            })
            .to_string(),
        )
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        let stale_guard_results = search::search(&pool, "Fresh Signal", Some(10), false)
            .await
            .unwrap();
        let stale_guard_json = serde_json::to_value(stale_guard_results).unwrap();
        assert!(
            stale_guard_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| { result["type"] == "page" && result["page"]["id"] == PAGE_A })
        );

        let rebuilt_count = search::rebuild_index(&pool).await.unwrap();
        let index_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_search_index")
            .fetch_one(&pool)
            .await
            .unwrap();
        let fts_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_search_fts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rebuilt_count, index_count);
        assert_eq!(index_count, fts_count);
        assert!(rebuilt_count >= 4);
    });
}

#[test]
fn property_search_indexes_database_values_cached_rollups_and_formulas() {
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
                    "Budget": {
                        "id": "budget",
                        "name": "Budget",
                        "type": "number",
                        "number": {
                            "format": "number"
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
                    "Details": {
                        "id": "details",
                        "name": "Details",
                        "type": "rich_text",
                        "rich_text": {}
                    },
                    "Status": {
                        "id": "status",
                        "name": "Status",
                        "type": "status",
                        "status": {
                            "options": [
                                { "id": "todo", "name": "Todo", "color": "gray" },
                                { "id": "progress", "name": "In progress", "color": "blue" }
                            ]
                        }
                    },
                    "Due": {
                        "id": "due",
                        "name": "Due",
                        "type": "date",
                        "date": {}
                    },
                    "Done": {
                        "id": "done",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    },
                    "Project": {
                        "id": "project_relation",
                        "name": "Project",
                        "type": "relation",
                        "relation": {
                            "data_source_id": DATA_SOURCE_B
                        }
                    },
                    "Project budget": {
                        "id": "project_budget",
                        "name": "Project budget",
                        "type": "rollup",
                        "rollup": {
                            "relation_property_id": "project_relation",
                            "relation_property_name": "Project",
                            "rollup_property_id": "budget",
                            "rollup_property_name": "Budget",
                            "function": "sum"
                        }
                    },
                    "Formula state": {
                        "id": "formula_state",
                        "name": "Formula state",
                        "type": "formula",
                        "formula": {
                            "expression": "if(prop(\"Done\"), \"Ready Searchable\", \"Open\")"
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
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_B,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "budget".to_string(),
                value: json!(11),
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Property search row".to_string(),
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
                property_id: "details".to_string(),
                value: json!("Deep property text"),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "status".to_string(),
                value: json!("In progress"),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "due".to_string(),
                value: json!({ "start": "2035-07-02", "end": null }),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "done".to_string(),
                value: json!(true),
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

        data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
            .await
            .unwrap();

        assert_search_finds_page(&pool, "Deep property text", PAGE_B).await;
        assert_search_finds_page(&pool, "In progress", PAGE_B).await;
        assert_search_finds_page(&pool, "2035-07-02", PAGE_B).await;
        assert_search_finds_page(&pool, "checked", PAGE_B).await;
        assert_search_finds_page(&pool, "Project Alpha", PAGE_B).await;
        assert_search_finds_page(&pool, "11", PAGE_B).await;
        assert_search_finds_page(&pool, "Ready Searchable", PAGE_B).await;

        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_B,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "budget".to_string(),
                value: json!(13),
            },
        )
        .await
        .unwrap();
        assert_search_does_not_find_page(&pool, "11", PAGE_B).await;

        data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
            .await
            .unwrap();
        assert_search_finds_page(&pool, "13", PAGE_B).await;
    });
}

async fn assert_search_finds_page(pool: &SqlitePool, query: &str, page_id: &str) {
    let results = search::search(pool, query, Some(10), false).await.unwrap();
    let results_json = serde_json::to_value(results).unwrap();
    assert!(
        results_json
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["type"] == "page" && result["page"]["id"] == page_id })
    );
}

async fn assert_search_does_not_find_page(pool: &SqlitePool, query: &str, page_id: &str) {
    let results = search::search(pool, query, Some(10), false).await.unwrap();
    let results_json = serde_json::to_value(results).unwrap();
    assert!(
        !results_json
            .as_array()
            .unwrap()
            .iter()
            .any(|result| { result["type"] == "page" && result["page"]["id"] == page_id })
    );
}

#[test]
fn search_rejects_empty_queries() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        assert_eq!(
            search::search(&pool, "  ", Some(10), false).await.err(),
            Some("search query must not be empty".to_string())
        );
    });
}

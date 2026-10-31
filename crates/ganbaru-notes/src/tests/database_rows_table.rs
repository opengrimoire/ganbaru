use super::helpers::*;

/// A database with a `When` date property and one row page, `Meeting`, at `PAGE_B`.
async fn dated_database(pool: &sqlx::SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    databases::create_database(
        pool,
        NoteDatabaseCreate {
            id: DATABASE_A.to_string(),
            data_source_id: DATA_SOURCE_A.to_string(),
            view_id: DATABASE_VIEW_A.to_string(),
            title: "Dates".to_string(),
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
        pool,
        DATA_SOURCE_A,
        None,
        None,
        NoteDataSourceSchemaUpdate {
            properties: json!({
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                "When": { "id": "when", "name": "When", "type": "date", "date": {} }
            }),
        },
    )
    .await
    .unwrap();
    data_sources::rows::create_data_source_row_page(
        pool,
        DATA_SOURCE_A,
        NoteDataSourceRowPageCreate {
            id: PAGE_B.to_string(),
            first_block_id: BLOCK_B.to_string(),
            title: "Meeting".to_string(),
            properties: None,
        },
    )
    .await
    .unwrap();
}

async fn set_when(
    pool: &sqlx::SqlitePool,
    value: serde_json::Value,
) -> Result<serde_json::Value, String> {
    data_sources::layouts::table::update_data_source_row_property(
        pool,
        DATA_SOURCE_A,
        PAGE_B,
        NoteDataSourceRowPropertyUpdate {
            property_id: "when".to_string(),
            value,
        },
    )
    .await
    .map(|page| serde_json::to_value(page).unwrap())
}

async fn pending_reminders(pool: &sqlx::SqlitePool) -> Vec<serde_json::Value> {
    serde_json::to_value(mention_notifications::list_pending(pool).await.unwrap())
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn database_date_edits_preserve_ranges_and_zones_and_reject_invalid_ranges_atomically() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        dated_database(&pool).await;
        let original = json!({ "start": "2026-10-01T09:00:00", "end": "2026-10-01T11:00:00", "time_zone": "America/Monterrey" });
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "when".to_string(),
                value: original.clone(),
            },
        )
        .await
        .unwrap();
        let updated = data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "when".to_string(),
                value: json!("2026-10-01T10:00:00"),
            },
        )
        .await
        .unwrap();
        let updated = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated["properties"]["When"]["date"],
            json!({
                "start": "2026-10-01T10:00:00", "end": original["end"], "time_zone": original["time_zone"]
            })
        );
        for invalid in [
            json!("2026-10-02"),
            json!({ "start": "2026-02-30", "end": null, "time_zone": null }),
            json!({ "start": "2026-10-01", "end": false, "time_zone": null }),
            json!({ "start": "2026-10-01", "end": null, "time_zone": 123 }),
        ] {
            assert!(
                data_sources::layouts::table::update_data_source_row_property(
                    &pool,
                    DATA_SOURCE_A,
                    PAGE_B,
                    NoteDataSourceRowPropertyUpdate {
                        property_id: "when".to_string(),
                        value: invalid,
                    }
                )
                .await
                .is_err()
            );
        }
        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table = serde_json::to_value(table).unwrap();
        assert_eq!(
            table["rows"][0]["properties"]["When"]["date"],
            updated["properties"]["When"]["date"]
        );
        let cleared = data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "when".to_string(),
                value: json!(null),
            },
        )
        .await
        .unwrap();
        assert!(serde_json::to_value(cleared).unwrap()["properties"]["When"]["date"].is_null());
    });
}

#[test]
fn bulk_row_property_edits_change_every_listed_row_or_none() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        dated_database(&pool).await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                first_block_id: BLOCK_C.to_string(),
                title: "Review".to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        let rows = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        let set_all = |page_ids: Vec<String>, value: serde_json::Value| {
            let pool = pool.clone();
            async move {
                data_sources::layouts::table::update_data_source_rows_property(
                    &pool,
                    DATA_SOURCE_A,
                    &page_ids,
                    NoteDataSourceRowPropertyUpdate {
                        property_id: "when".to_string(),
                        value,
                    },
                )
                .await
            }
        };
        let dates = || async {
            let table = data_sources::layouts::table::data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
            )
            .await
            .unwrap();
            let table = serde_json::to_value(table).unwrap();
            table["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row["properties"]["When"]["date"]["start"].clone())
                .collect::<Vec<_>>()
        };

        let updated = set_all(rows(&[PAGE_C, PAGE_B]), json!("2026-10-05"))
            .await
            .unwrap();
        assert_eq!(updated, rows(&[PAGE_C, PAGE_B]));
        assert_eq!(
            dates().await,
            vec![json!("2026-10-05"), json!("2026-10-05")]
        );

        for invalid in [
            rows(&[]),
            rows(&[PAGE_B, PAGE_B]),
            rows(&[PAGE_B, PAGE_A]),
            rows(&[PAGE_B, "not-a-uuid"]),
        ] {
            assert!(set_all(invalid, json!("2026-11-01")).await.is_err());
        }
        assert!(
            set_all(rows(&[PAGE_B, PAGE_C]), json!("2026-02-30"))
                .await
                .is_err()
        );
        assert_eq!(
            dates().await,
            vec![json!("2026-10-05"), json!("2026-10-05")]
        );
        assert!(
            data_sources::layouts::table::update_data_source_rows_property(
                &pool,
                DATA_SOURCE_A,
                &rows(&[PAGE_B]),
                NoteDataSourceRowPropertyUpdate {
                    property_id: "missing".to_string(),
                    value: json!(null),
                },
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn database_view_window_keyset_pages_ten_thousand_rows_with_bounded_payloads() {
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
        let mut tx = pool.begin().await.unwrap();
        for index in 0..10_000 {
            let id = format!("40000000-0000-4000-8000-{index:012}");
            let title = format!("Task {index:05}");
            sqlx::query(
                "INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title)
                 VALUES (?, 'data_source_id', ?, ?)",
            )
            .bind(id)
            .bind(DATA_SOURCE_A)
            .bind(&title)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        sqlx::query(
            "UPDATE notes_database_views
             SET filter = ?, sorts = ?
             WHERE id = ?",
        )
        .bind(
            json!({
                "filters": [{ "property_id": "title", "condition": "contains", "value": "Task" }]
            })
            .to_string(),
        )
        .bind(json!([{ "property_id": "title", "direction": "descending" }]).to_string())
        .bind(DATABASE_VIEW_A)
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let first = data_sources::layouts::table::data_source_table_view_window(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceViewWindowRequest {
                start_cursor: None,
                page_size: Some(80),
                range_start: None,
                range_end: None,
            },
        )
        .await
        .unwrap();
        let first_json = serde_json::to_value(&first).unwrap();
        assert_eq!(first_json["total_row_count"], 10_000);
        assert_eq!(first_json["rows"].as_array().unwrap().len(), 80);
        assert_eq!(
            first_json["rows"][0]["properties"]["Name"]["title"][0]["plain_text"],
            "Task 09999"
        );
        assert!(serde_json::to_vec(&first).unwrap().len() < 256 * 1024);
        let plan = sqlx::query(
            "EXPLAIN QUERY PLAN
             SELECT page.id
             FROM notes_pages AS page
             WHERE page.parent_type = 'data_source_id'
               AND page.parent_data_source_id = ?
               AND page.in_trash = 0
               AND page.archived = 0
             ORDER BY page.title COLLATE NOCASE DESC, page.id ASC
             LIMIT 80",
        )
        .bind(DATA_SOURCE_A)
        .fetch_all(&pool)
        .await
        .unwrap();
        let plan_details = plan
            .iter()
            .map(|row| row.get::<String, _>("detail"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            plan_details.contains("SEARCH page USING INDEX"),
            "{plan_details}"
        );

        let cursor = first_json["next_cursor"].as_str().unwrap().to_string();
        let second = data_sources::layouts::table::data_source_table_view_window(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceViewWindowRequest {
                start_cursor: Some(cursor),
                page_size: Some(80),
                range_start: None,
                range_end: None,
            },
        )
        .await
        .unwrap();
        let second_json = serde_json::to_value(second).unwrap();
        assert_eq!(second_json["rows"].as_array().unwrap().len(), 80);
        assert_eq!(
            second_json["rows"][0]["properties"]["Name"]["title"][0]["plain_text"],
            "Task 09919"
        );
        let first_ids = first_json["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert!(
            second_json["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| { !first_ids.contains(row["id"].as_str().unwrap()) })
        );
    });
}

#[test]
fn database_row_pages_are_real_pages_with_page_lifecycle() {
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
                    "Done": {
                        "id": "done_checkbox",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    }
                }),
            },
        )
        .await
        .unwrap();

        let loaded = data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: String::new(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        let blank_json = serde_json::to_value(&loaded).unwrap();
        assert_eq!(
            blank_json["page"]["properties"]["Name"]["title"][0]["plain_text"],
            ""
        );
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "title".to_string(),
                value: json!("Write spec"),
            },
        )
        .await
        .unwrap();
        let loaded = reads::load_page(&pool, PAGE_B).await.unwrap();
        let loaded_json = serde_json::to_value(&loaded).unwrap();
        assert_eq!(loaded_json["page"]["parent"]["type"], "data_source_id");
        assert_eq!(
            loaded_json["page"]["parent"]["data_source_id"],
            DATA_SOURCE_A
        );
        assert_eq!(
            loaded_json["page"]["properties"]["Name"]["title"][0]["plain_text"],
            "Write spec"
        );
        assert_eq!(loaded_json["page"]["properties"]["Done"]["checkbox"], false);
        assert_eq!(loaded_json["blocks"]["results"][0]["type"], "paragraph");

        let sidebar_pages = reads::list_sidebar_pages(
            &pool,
            NoteSidebarPagesRequest {
                expanded_page_ids: vec![],
                seed_page_ids: vec![],
                selected_page_id: None,
            },
        )
        .await
        .unwrap();
        let sidebar_json = serde_json::to_value(sidebar_pages).unwrap();
        assert!(
            !sidebar_json["pages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|page| page["id"] == PAGE_B)
        );

        let rows = data_sources::rows::list_data_source_row_pages(&pool, DATA_SOURCE_A)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);

        let search_results = search::search(&pool, "Write", Some(10), false)
            .await
            .unwrap();
        let search_json = serde_json::to_value(search_results).unwrap();
        assert!(
            search_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["page"]["id"] == PAGE_B)
        );

        let duplicated = writes::duplicate_page(&pool, PAGE_B, NoteDuplicatePage { title: None })
            .await
            .unwrap();
        let duplicated_json = serde_json::to_value(&duplicated).unwrap();
        let duplicate_id = duplicated_json["page"]["id"].as_str().unwrap().to_string();
        assert_ne!(duplicate_id, PAGE_B);
        assert_eq!(
            duplicated_json["page"]["parent"]["data_source_id"],
            DATA_SOURCE_A
        );
        assert_eq!(
            duplicated_json["page"]["properties"]["Name"]["title"][0]["plain_text"],
            "Write spec"
        );

        writes::trash_page(&pool, PAGE_B, true).await.unwrap();
        let rows_after_trash = data_sources::rows::list_data_source_row_pages(&pool, DATA_SOURCE_A)
            .await
            .unwrap();
        assert_eq!(rows_after_trash.len(), 1);
        let trashed = reads::list_trashed_pages(&pool).await.unwrap();
        let trashed_json = serde_json::to_value(trashed).unwrap();
        assert!(
            trashed_json
                .as_array()
                .unwrap()
                .iter()
                .any(|page| page["id"] == PAGE_B)
        );

        let restored = writes::trash_page(&pool, PAGE_B, false).await.unwrap();
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(restored_json["parent"]["data_source_id"], DATA_SOURCE_A);
        let rows_after_restore =
            data_sources::rows::list_data_source_row_pages(&pool, DATA_SOURCE_A)
                .await
                .unwrap();
        assert_eq!(rows_after_restore.len(), 2);

        sqlx::query("UPDATE notes_data_sources SET in_trash = 1 WHERE id = ?")
            .bind(DATA_SOURCE_A)
            .execute(&pool)
            .await
            .unwrap();
        let hidden_rows = data_sources::rows::list_data_source_row_pages(&pool, DATA_SOURCE_A)
            .await
            .unwrap();
        assert!(hidden_rows.is_empty());
        let hidden_search = search::search(&pool, "Write", Some(10), false)
            .await
            .unwrap();
        let hidden_search_json = serde_json::to_value(hidden_search).unwrap();
        assert!(
            !hidden_search_json
                .as_array()
                .unwrap()
                .iter()
                .any(|result| result["page"]["parent"]["type"] == "data_source_id")
        );
    });
}

#[test]
fn editable_database_table_view_persists_cells_configuration_filters_and_sorts() {
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
                    },
                    "Priority": {
                        "id": "priority",
                        "name": "Priority",
                        "type": "select",
                        "select": {
                            "options": [
                                { "id": "low", "name": "Low", "color": "blue" },
                                { "id": "high", "name": "High", "color": "red" }
                            ]
                        }
                    },
                    "Done": {
                        "id": "done_checkbox",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    }
                }),
            },
        )
        .await
        .unwrap();

        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Beta".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_C.to_string(),
                title: "Alpha".to_string(),
                first_block_id: BLOCK_C.to_string(),
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
                value: json!("Write backend"),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "estimate".to_string(),
                value: json!(5),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "priority".to_string(),
                value: json!("High"),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceRowPropertyUpdate {
                property_id: "done_checkbox".to_string(),
                value: json!(true),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "details".to_string(),
                value: json!("Write frontend"),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "estimate".to_string(),
                value: json!(2),
            },
        )
        .await
        .unwrap();
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
            NoteDataSourceRowPropertyUpdate {
                property_id: "priority".to_string(),
                value: json!("Low"),
            },
        )
        .await
        .unwrap();

        let table = data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceTableViewUpdate {
                filter: vec![
                    serde_json::from_value::<NoteDataSourceTableFilter>(json!({
                        "property_id": "title", "condition": "contains", "value": "a"
                    }))
                    .unwrap(),
                ],
                sorts: vec![NoteDataSourceTableSort {
                    property_id: "estimate".to_string(),
                    direction: "descending".to_string(),
                }],
                configuration: NoteDataSourceTableConfigurationUpdate {
                    property_order: vec![
                        "title".to_string(),
                        "estimate".to_string(),
                        "priority".to_string(),
                        "done_checkbox".to_string(),
                        "details".to_string(),
                    ],
                    hidden_property_ids: vec!["details".to_string()],
                    column_widths: json!({
                        "title": 260,
                        "estimate": 160,
                        "priority": 180,
                        "done_checkbox": 112,
                        "details": 240
                    }),
                    row_open_mode: "side_panel".to_string(),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();

        let table_json = serde_json::to_value(table).unwrap();
        assert_eq!(
            table_json["view"]["configuration"]["table"]["hidden_property_ids"],
            json!(["details"])
        );
        assert_eq!(
            table_json["view"]["configuration"]["table"]["column_widths"]["estimate"],
            160
        );
        assert_eq!(
            table_json["view"]["configuration"]["table"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(
            table_json["view"]["filter"]["filters"][0]["property_id"],
            "title"
        );
        assert_eq!(table_json["view"]["sorts"][0]["property_id"], "estimate");
        assert_eq!(table_json["rows"].as_array().unwrap().len(), 2);
        assert_eq!(table_json["rows"][0]["id"], PAGE_B);
        assert_eq!(table_json["rows"][0]["properties"]["Estimate"]["number"], 5);
        assert_eq!(
            table_json["rows"][0]["properties"]["Priority"]["select"]["name"],
            "High"
        );
        assert_eq!(
            table_json["rows"][0]["properties"]["Details"]["rich_text"][0]["plain_text"],
            "Write backend"
        );

        let filtered = data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceTableViewUpdate {
                filter: vec![
                    serde_json::from_value::<NoteDataSourceTableFilter>(json!({
                        "property_id": "done_checkbox", "condition": "checked", "value": null
                    }))
                    .unwrap(),
                ],
                sorts: vec![],
                configuration: NoteDataSourceTableConfigurationUpdate {
                    property_order: vec![
                        "title".to_string(),
                        "estimate".to_string(),
                        "priority".to_string(),
                        "done_checkbox".to_string(),
                        "details".to_string(),
                    ],
                    hidden_property_ids: vec!["details".to_string()],
                    column_widths: json!({
                        "title": 260,
                        "estimate": 160,
                        "priority": 180,
                        "done_checkbox": 112,
                        "details": 240
                    }),
                    row_open_mode: "side_panel".to_string(),
                    ..Default::default()
                },
            },
        )
        .await
        .unwrap();
        let filtered_json = serde_json::to_value(filtered).unwrap();
        assert_eq!(filtered_json["rows"].as_array().unwrap().len(), 1);
        assert_eq!(filtered_json["rows"][0]["id"], PAGE_B);
    });
}

#[test]
fn database_date_reminders_follow_the_row_value_and_its_schema() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        dated_database(&pool).await;
        let stored = set_when(
            &pool,
            json!({
                "start": "2026-10-20T15:00",
                "end": null,
                "time_zone": null,
                "color": "red",
                "ganbaru_reminder": { "enabled": true, "minutes_before": 30, "sound": "bell" }
            }),
        )
        .await
        .unwrap();
        assert_eq!(
            stored["properties"]["When"]["date"],
            json!({
                "start": "2026-10-20T15:00",
                "end": null,
                "time_zone": null,
                "ganbaru_reminder": { "enabled": true, "minutes_before": 30 }
            })
        );
        let pending = pending_reminders(&pool).await;
        assert_eq!(pending.len(), 1);
        let reminder = &pending[0];
        assert_eq!(reminder["source_type"], "page_property");
        assert_eq!(reminder["source_id"], PAGE_B);
        assert_eq!(reminder["page_id"], PAGE_B);
        assert!(reminder["block_id"].is_null());
        assert_eq!(reminder["kind"], "reminder");
        assert_eq!(reminder["target_id"], "when");
        assert_eq!(reminder["trigger_at"], "2026-10-20T15:00");
        assert_eq!(reminder["remind_minutes_before"], 30);
        assert_eq!(reminder["plain_text"], "When");
        assert_eq!(reminder["source_plain_text"], "Meeting");

        for invalid in [
            json!({ "enabled": true, "minutes_before": 40_321 }),
            json!({ "enabled": "yes" }),
            json!("on"),
        ] {
            let value = json!({ "start": "2026-10-21", "end": null, "time_zone": null, "ganbaru_reminder": invalid });
            assert!(set_when(&pool, value).await.is_err());
        }
        assert_eq!(pending_reminders(&pool).await, pending);

        mention_notifications::mark_delivered(
            &pool,
            NoteMentionNotificationDeliveryUpdate {
                ids: vec![reminder["id"].as_str().unwrap().to_string()],
            },
        )
        .await
        .unwrap();
        let earlier = json!({ "enabled": true, "minutes_before": 60 });
        set_when(&pool, json!({ "start": "2026-10-20T15:00", "end": null, "time_zone": null, "ganbaru_reminder": earlier }))
            .await
            .unwrap();
        assert!(pending_reminders(&pool).await.is_empty());
        set_when(&pool, json!({ "start": "2026-10-22", "end": null, "time_zone": null, "ganbaru_reminder": earlier }))
            .await
            .unwrap();
        let moved = pending_reminders(&pool).await;
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0]["trigger_at"], "2026-10-22");
        assert_eq!(moved[0]["remind_minutes_before"], 60);

        set_when(&pool, json!({ "start": "2026-10-22", "end": null, "time_zone": null, "ganbaru_reminder": { "enabled": false } }))
            .await
            .unwrap();
        assert!(pending_reminders(&pool).await.is_empty());
        set_when(&pool, json!({ "start": "2026-10-22", "end": null, "time_zone": null, "ganbaru_reminder": { "enabled": true } }))
            .await
            .unwrap();
        assert_eq!(pending_reminders(&pool).await.len(), 1);

        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} }
                }),
            },
        )
        .await
        .unwrap();
        assert!(pending_reminders(&pool).await.is_empty());
    });
}

#[test]
fn row_values_written_under_an_older_property_type_read_with_their_own_type() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        dated_database(&pool).await;
        let mut connection = pool.acquire().await.unwrap();
        crate::page_property_storage::write_page_property_value(
            &mut connection,
            PAGE_B,
            "when",
            "rich_text",
            &json!([rich_text("next Tuesday")]),
        )
        .await
        .unwrap();
        drop(connection);

        let page =
            serde_json::to_value(reads::get_page(&pool, PAGE_B, false).await.unwrap()).unwrap();
        assert_eq!(page["properties"]["When"]["id"], "when");
        assert_eq!(page["properties"]["When"]["type"], "rich_text");
        assert_eq!(
            page["properties"]["When"]["rich_text"][0]["plain_text"],
            "next Tuesday"
        );
        assert!(page["properties"]["When"].get("date").is_none());
        let table = serde_json::to_value(
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(table["rows"][0]["properties"]["When"]["type"], "rich_text");
        assert!(pending_reminders(&pool).await.is_empty());

        set_when(&pool, json!("2026-10-05")).await.unwrap();
        let stored: Vec<(String, String)> = sqlx::query_as(
            "SELECT value_type, json_extract(value, '$.start') FROM notes_page_property_values
             WHERE page_id = ? AND property_id = 'when'",
        )
        .bind(PAGE_B)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(stored, vec![("date".to_string(), "2026-10-05".to_string())]);
    });
}

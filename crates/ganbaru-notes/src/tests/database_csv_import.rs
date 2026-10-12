use super::helpers::*;

#[test]
fn csv_import_previews_mapping_and_row_errors() {
    crate::test_block_on(async {
        let pool = csv_import_pool().await;
        let result = data_sources::csv_import::import_csv(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceCsvImportRequest {
                csv: concat!(
                    "Name,Details,Estimate,Priority,Done,Ticket,Unknown\n",
                    "Alpha,Build backend,3,High,yes,G-1,ignored\n",
                    "Beta,Bad number,nope,Low,no,,ignored\n",
                )
                .to_string(),
                has_header: Some(true),
                dry_run: Some(true),
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["object"], "notes_data_source_csv_import");
        assert_eq!(value["dry_run"], true);
        assert_eq!(value["total_row_count"], 2);
        assert_eq!(value["valid_row_count"], 1);
        assert_eq!(value["skipped_row_count"], 1);
        assert_eq!(value["imported_row_count"], 0);
        assert!(
            value["columns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|column| column["source_name"] == "Ticket" && column["read_only"] == true)
        );
        assert!(
            value["columns"]
                .as_array()
                .unwrap()
                .iter()
                .any(|column| column["source_name"] == "Unknown" && column["mapped"] == false)
        );
        assert!(value["diagnostics"].as_array().unwrap().iter().any(
            |diagnostic| diagnostic["code"] == "invalid_cell"
                && diagnostic["row_number"] == 3
                && diagnostic["property_id"] == "estimate"
        ));
        let row_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages WHERE parent_data_source_id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row_count, 0);
    });
}

#[test]
fn csv_import_writes_valid_rows_as_canonical_database_pages() {
    crate::test_block_on(async {
        let pool = csv_import_pool().await;
        let result = data_sources::csv_import::import_csv(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceCsvImportRequest {
                csv: concat!(
                    "Name,Details,Estimate,Priority,Done\n",
                    "Alpha,Build backend,3,High,yes\n",
                    "Beta,\"Design, docs\",2,Low,no\n",
                )
                .to_string(),
                has_header: Some(true),
                dry_run: Some(false),
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["imported_row_count"], 2);
        assert_eq!(value["valid_row_count"], 2);

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        let rows = table_json["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let alpha = rows
            .iter()
            .find(|row| row["properties"]["Name"]["title"][0]["plain_text"] == "Alpha")
            .unwrap();
        assert_eq!(
            alpha["properties"]["Details"]["rich_text"][0]["plain_text"],
            "Build backend"
        );
        assert_eq!(alpha["properties"]["Estimate"]["number"], 3.0);
        assert_eq!(alpha["properties"]["Priority"]["select"]["name"], "High");
        assert_eq!(alpha["properties"]["Done"]["checkbox"], true);
        let beta = rows
            .iter()
            .find(|row| row["properties"]["Name"]["title"][0]["plain_text"] == "Beta")
            .unwrap();
        assert_eq!(
            beta["properties"]["Details"]["rich_text"][0]["plain_text"],
            "Design, docs"
        );
        assert_eq!(beta["source_provider"], "csv");

        let first_blocks: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE source_provider = 'csv'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(first_blocks, 2);
    });
}

#[test]
fn csv_import_skips_invalid_rows_while_importing_valid_rows() {
    crate::test_block_on(async {
        let pool = csv_import_pool().await;
        let result = data_sources::csv_import::import_csv(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceCsvImportRequest {
                csv: concat!(
                    "Name,Estimate,Priority\n",
                    "Alpha,3,High\n",
                    "Broken,not-a-number,Missing\n",
                )
                .to_string(),
                has_header: Some(true),
                dry_run: Some(false),
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["imported_row_count"], 1);
        assert_eq!(value["valid_row_count"], 1);
        assert_eq!(value["skipped_row_count"], 1);
        assert!(
            value["diagnostics"].as_array().unwrap().iter().any(
                |diagnostic| diagnostic["row_number"] == 3 && diagnostic["severity"] == "error"
            )
        );
        let row_titles: Vec<String> = sqlx::query_scalar(
            "SELECT title FROM notes_pages WHERE parent_data_source_id = ? ORDER BY title ASC",
        )
        .bind(DATA_SOURCE_A)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(row_titles, vec!["Alpha".to_string()]);
    });
}

async fn csv_import_pool() -> SqlitePool {
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
                    "id": "done",
                    "name": "Done",
                    "type": "checkbox",
                    "checkbox": {}
                },
                "Ticket": {
                    "id": "ticket",
                    "name": "Ticket",
                    "type": "unique_id",
                    "unique_id": { "prefix": "G-" }
                }
            }),
        },
    )
    .await
    .unwrap();
    pool
}

use super::helpers::*;

#[test]
fn csv_export_view_uses_visible_columns_filters_sorts_and_csv_quoting() {
    crate::test_block_on(async {
        let pool = csv_export_pool().await;
        configure_filtered_table_view(&pool).await;

        let export = data_sources::csv_export::export_csv(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceCsvExportRequest {
                database_id: None,
                view_id: None,
                scope: Some("view".to_string()),
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(export).unwrap();

        assert_eq!(value["object"], "notes_data_source_csv_export");
        assert_eq!(value["scope"], "view");
        assert_eq!(value["exported_row_count"], 2);
        assert_eq!(value["exported_property_count"], 3);
        assert_eq!(
            value["csv"].as_str().unwrap(),
            "Name,Details,Estimate\nGamma,Zeta,5\nAlpha,\"Design, docs\",3\n"
        );
        assert!(value["diagnostics"].as_array().unwrap().is_empty());
    });
}

#[test]
fn csv_export_all_includes_hidden_properties_and_unfiltered_rows() {
    crate::test_block_on(async {
        let pool = csv_export_pool().await;
        configure_filtered_table_view(&pool).await;

        let export = data_sources::csv_export::export_csv(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceCsvExportRequest {
                database_id: None,
                view_id: None,
                scope: Some("all".to_string()),
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(export).unwrap();

        assert_eq!(value["scope"], "all");
        assert_eq!(value["file_name"], "tasks-full.csv");
        assert_eq!(value["exported_row_count"], 3);
        assert_eq!(value["exported_property_count"], 5);
        assert_eq!(
            value["csv"].as_str().unwrap(),
            concat!(
                "Name,Details,Estimate,Done,Files\n",
                "Alpha,\"Design, docs\",3,true,\n",
                "Beta,Build backend,1,false,\n",
                "Gamma,Zeta,5,true,\n",
            )
        );
        assert!(value["diagnostics"].as_array().unwrap().iter().any(
            |diagnostic| diagnostic["code"] == "csv_export_plain_text_property"
                && diagnostic["property_id"] == "files"
        ));
    });
}

async fn configure_filtered_table_view(pool: &SqlitePool) {
    data_sources::layouts::table::update_data_source_table_view(
        pool,
        DATA_SOURCE_A,
        None,
        None,
        NoteDataSourceTableViewUpdate {
            filter: vec![
                serde_json::from_value::<NoteDataSourceTableFilter>(json!({
                    "property_id": "done", "condition": "checked", "value": null
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
                    "details".to_string(),
                    "estimate".to_string(),
                    "done".to_string(),
                    "files".to_string(),
                ],
                hidden_property_ids: vec!["done".to_string(), "files".to_string()],
                column_widths: json!({}),
                row_open_mode: "full_page".to_string(),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap();
}

async fn csv_export_pool() -> SqlitePool {
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
                "Done": {
                    "id": "done",
                    "name": "Done",
                    "type": "checkbox",
                    "checkbox": {}
                },
                "Files": {
                    "id": "files",
                    "name": "Files",
                    "type": "files",
                    "files": {}
                }
            }),
        },
    )
    .await
    .unwrap();
    data_sources::csv_import::import_csv(
        &pool,
        DATA_SOURCE_A,
        NoteDataSourceCsvImportRequest {
            csv: concat!(
                "Name,Details,Estimate,Done\n",
                "Alpha,\"Design, docs\",3,yes\n",
                "Beta,Build backend,1,no\n",
                "Gamma,Zeta,5,yes\n",
            )
            .to_string(),
            has_header: Some(true),
            dry_run: Some(false),
        },
    )
    .await
    .unwrap();
    pool
}

use super::helpers::*;

#[test]
fn calendar_database_view_uses_date_ranges_filters_sorts_and_configuration() {
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
                    "Due": {
                        "id": "due",
                        "name": "Due",
                        "type": "date",
                        "date": {}
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "type": "number",
                        "number": { "format": "number" }
                    },
                    "Status": {
                        "id": "status",
                        "name": "Status",
                        "type": "status",
                        "status": {
                            "options": [
                                { "id": "todo", "name": "To-do", "color": "gray" },
                                { "id": "doing", "name": "Doing", "color": "blue" }
                            ]
                        }
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
                title: "Alpha".to_string(),
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
                title: "Beta".to_string(),
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
                property_id: "due".to_string(),
                value: json!("2026-07-10"),
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
                property_id: "due".to_string(),
                value: json!({
                    "start": "2026-07-30",
                    "end": "2026-08-02",
                    "time_zone": null
                }),
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
                value: json!(5),
            },
        )
        .await
        .unwrap();

        let default_calendar = data_sources::layouts::calendar::data_source_calendar_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
        )
        .await
        .unwrap();
        let default_json = serde_json::to_value(default_calendar).unwrap();
        assert_eq!(default_json["view"]["type"], "calendar");
        assert_eq!(
            default_json["view"]["configuration"]["calendar"]["date_property_id"],
            "due"
        );
        assert_eq!(
            default_json["view"]["configuration"]["calendar"]["row_open_mode"],
            "side_panel"
        );

        let invalid_date_property =
            data_sources::layouts::calendar::update_data_source_calendar_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                NoteDataSourceCalendarViewUpdate {
                    filter: vec![],
                    sorts: vec![],
                    configuration: NoteDataSourceCalendarConfigurationUpdate {
                        date_property_id: Some("estimate".to_string()),
                        range_start: "2026-07-01".to_string(),
                        range_end: "2026-07-31".to_string(),
                        visible_property_ids: vec![],
                        row_open_mode: "side_panel".to_string(),
                    },
                },
            )
            .await;
        match invalid_date_property {
            Ok(_) => panic!("calendar accepted a non-date property"),
            Err(error) => assert!(error.contains("date property")),
        }

        let updated = data_sources::layouts::calendar::update_data_source_calendar_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceCalendarViewUpdate {
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
                configuration: NoteDataSourceCalendarConfigurationUpdate {
                    date_property_id: Some("due".to_string()),
                    range_start: "2026-07-01".to_string(),
                    range_end: "2026-07-31".to_string(),
                    visible_property_ids: vec![
                        "estimate".to_string(),
                        "due".to_string(),
                        "title".to_string(),
                        "estimate".to_string(),
                        "missing".to_string(),
                    ],
                    row_open_mode: "full_page".to_string(),
                },
            },
        )
        .await
        .unwrap();
        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated_json["view"]["configuration"]["calendar"]["range_start"],
            "2026-07-01"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["calendar"]["range_end"],
            "2026-07-31"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["calendar"]["visible_property_ids"],
            json!(["estimate"])
        );
        assert_eq!(
            updated_json["view"]["configuration"]["calendar"]["row_open_mode"],
            "full_page"
        );
        assert_eq!(updated_json["rows"][0]["id"], PAGE_C);
        assert_eq!(updated_json["rows"][1]["id"], PAGE_B);

        let august = data_sources::layouts::calendar::update_data_source_calendar_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceCalendarViewUpdate {
                filter: vec![],
                sorts: vec![],
                configuration: NoteDataSourceCalendarConfigurationUpdate {
                    date_property_id: Some("due".to_string()),
                    range_start: "2026-08-01".to_string(),
                    range_end: "2026-08-31".to_string(),
                    visible_property_ids: vec![],
                    row_open_mode: "side_panel".to_string(),
                },
            },
        )
        .await
        .unwrap();
        let august_json = serde_json::to_value(august).unwrap();
        assert_eq!(august_json["rows"].as_array().unwrap().len(), 1);
        assert_eq!(august_json["rows"][0]["id"], PAGE_C);
    });
}

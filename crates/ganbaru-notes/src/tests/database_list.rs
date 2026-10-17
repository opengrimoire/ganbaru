use super::helpers::*;

#[test]
fn list_database_view_persists_visible_properties_grouping_filters_and_sorts() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: "List tasks".to_string(),
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
            None, None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "type": "title",
                        "title": {}
                    },
                    "Status": {
                        "id": "status",
                        "name": "Status",
                        "type": "status",
                        "status": {
                            "options": [
                                { "id": "todo", "name": "To-do", "color": "gray", "group": "To-do" },
                                { "id": "doing", "name": "Doing", "color": "blue", "group": "In progress" }
                            ]
                        }
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "type": "number",
                        "number": { "format": "number" }
                    },
                    "Published": {
                        "id": "published",
                        "name": "Published",
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
                property_id: "status".to_string(),
                value: json!("Doing"),
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
                property_id: "status".to_string(),
                value: json!("To-do"),
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

        let default_list =
            data_sources::layouts::list::data_source_list_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let default_json = serde_json::to_value(default_list).unwrap();
        assert_eq!(default_json["view"]["type"], "list");
        assert_eq!(
            default_json["view"]["configuration"]["list"]["group_property_id"],
            serde_json::Value::Null
        );
        assert_eq!(
            default_json["view"]["configuration"]["list"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(default_json["rows"].as_array().unwrap().len(), 2);

        let invalid_group = data_sources::layouts::list::update_data_source_list_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceListViewUpdate {
                filter: vec![],
                sorts: vec![],
                configuration: NoteDataSourceListConfigurationUpdate {
                    group_property_id: Some("estimate".to_string()),
                    group_order: vec![],
                    hidden_group_ids: vec![],
                    visible_property_ids: vec![],
                    row_open_mode: "side_panel".to_string(),
                },
            },
        )
        .await;
        match invalid_group {
            Ok(_) => panic!("list accepted a non-groupable property"),
            Err(error) => assert!(error.contains("group property type")),
        }

        let updated = data_sources::layouts::list::update_data_source_list_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceListViewUpdate {
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
                configuration: NoteDataSourceListConfigurationUpdate {
                    group_property_id: Some("status".to_string()),
                    group_order: vec!["todo".to_string(), "doing".to_string()],
                    hidden_group_ids: vec!["todo".to_string()],
                    visible_property_ids: vec![
                        "estimate".to_string(),
                        "status".to_string(),
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
            updated_json["view"]["configuration"]["list"]["group_property_id"],
            "status"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["list"]["hidden_group_ids"],
            json!(["todo"])
        );
        assert_eq!(
            updated_json["view"]["configuration"]["list"]["visible_property_ids"],
            json!(["estimate"])
        );
        assert_eq!(
            updated_json["view"]["configuration"]["list"]["row_open_mode"],
            "full_page"
        );
        assert_eq!(updated_json["rows"][0]["id"], PAGE_C);
        assert_eq!(updated_json["rows"][1]["id"], PAGE_B);
    });
}

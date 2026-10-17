use super::helpers::*;

#[test]
fn board_database_view_groups_filters_sorts_and_moves_rows() {
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
                                { "id": "doing", "name": "Doing", "color": "blue", "group": "In progress" },
                                { "id": "done", "name": "Done", "color": "green", "group": "Complete" }
                            ]
                        }
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
                    },
                    "Due": {
                        "id": "due",
                        "name": "Due",
                        "type": "date",
                        "date": {}
                    },
                    "Owner": {
                        "id": "owner",
                        "name": "Owner",
                        "type": "people",
                        "people": {}
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
                property_id: "priority".to_string(),
                value: json!("High"),
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
        data_sources::layouts::table::update_data_source_row_property(
            &pool,
            DATA_SOURCE_A,
            PAGE_C,
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
                property_id: "due".to_string(),
                value: json!("2026-07-02"),
            },
        )
        .await
        .unwrap();

        let default_board =
            data_sources::layouts::board::data_source_board_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let default_json = serde_json::to_value(default_board).unwrap();
        assert_eq!(default_json["view"]["type"], "board");
        assert_eq!(
            default_json["view"]["configuration"]["board"]["group_property_id"],
            "status"
        );
        assert!(
            default_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "todo" && group["rows"][0]["id"] == PAGE_C)
        );
        assert!(
            default_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "doing" && group["rows"][0]["id"] == PAGE_B)
        );
        assert!(
            default_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "__empty__"
                    && group["rows"].as_array().unwrap().is_empty())
        );

        let priority_board = data_sources::layouts::board::update_data_source_board_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceBoardViewUpdate {
                filter: vec![
                    serde_json::from_value::<NoteDataSourceTableFilter>(json!({
                        "property_id": "title", "condition": "contains", "value": "a"
                    }))
                    .unwrap(),
                ],
                sorts: vec![NoteDataSourceTableSort {
                    property_id: "title".to_string(),
                    direction: "ascending".to_string(),
                }],
                configuration: NoteDataSourceBoardConfigurationUpdate {
                    group_property_id: Some("priority".to_string()),
                    group_order: vec![
                        "high".to_string(),
                        "low".to_string(),
                        "__empty__".to_string(),
                    ],
                    hidden_group_ids: vec!["__empty__".to_string()],
                    visible_property_ids: vec![
                        "status".to_string(),
                        "done_checkbox".to_string(),
                        "due".to_string(),
                    ],
                    row_open_mode: "side_panel".to_string(),
                },
            },
        )
        .await
        .unwrap();
        let priority_json = serde_json::to_value(priority_board).unwrap();
        assert_eq!(
            priority_json["view"]["configuration"]["board"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(
            priority_json["view"]["configuration"]["board"]["visible_property_ids"],
            json!(["status", "done_checkbox", "due"])
        );
        assert!(
            priority_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "low" && group["rows"][0]["id"] == PAGE_C)
        );
        assert!(
            priority_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "__empty__" && group["hidden"] == true)
        );

        let moved = data_sources::layouts::board::move_data_source_board_row(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceBoardRowMove {
                page_id: PAGE_C.to_string(),
                group_id: "high".to_string(),
            },
        )
        .await
        .unwrap();
        let moved_json = serde_json::to_value(moved).unwrap();
        let high_group = moved_json["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| group["id"] == "high")
            .unwrap();
        assert_eq!(high_group["rows"].as_array().unwrap().len(), 2);
        assert!(
            high_group["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["id"] == PAGE_C
                    && row["properties"]["Priority"]["select"]["name"] == "High")
        );
        assert!(
            moved_json["groups"]
                .as_array()
                .unwrap()
                .iter()
                .any(|group| group["id"] == "low" && group["rows"].as_array().unwrap().is_empty())
        );
    });
}

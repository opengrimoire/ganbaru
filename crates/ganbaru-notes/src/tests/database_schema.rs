use super::helpers::*;

#[test]
fn update_local_data_source_schema() {
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

        let updated = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {
                        "id": "title",
                        "name": "Name",
                        "description": "Row title",
                        "type": "title",
                        "title": {}
                    },
                    "Details": {
                        "id": "details",
                        "name": "Details",
                        "description": "",
                        "type": "rich_text",
                        "rich_text": {}
                    },
                    "Estimate": {
                        "id": "estimate",
                        "name": "Estimate",
                        "description": "",
                        "type": "number",
                        "number": { "format": "percent" }
                    },
                    "Priority": {
                        "id": "priority",
                        "name": "Priority",
                        "description": "",
                        "type": "select",
                        "select": {
                            "options": [
                                { "id": "low", "name": "Low", "color": "blue" },
                                { "id": "high", "name": "High", "color": "red" }
                            ]
                        }
                    },
                    "Tags": {
                        "id": "tags",
                        "name": "Tags",
                        "description": "",
                        "type": "multi_select",
                        "multi_select": {
                            "options": [
                                { "id": "home", "name": "Home", "color": "green" }
                            ]
                        }
                    },
                    "Status": {
                        "id": "status",
                        "name": "Status",
                        "description": "",
                        "type": "status",
                        "status": {
                            "options": [
                                {
                                    "id": "todo",
                                    "name": "Todo",
                                    "color": "default",
                                    "group": "To-do"
                                },
                                {
                                    "id": "doing",
                                    "name": "Doing",
                                    "color": "blue",
                                    "group": "In progress"
                                },
                                {
                                    "id": "done",
                                    "name": "Done",
                                    "color": "green",
                                    "group": "Complete"
                                }
                            ]
                        }
                    },
                    "Due": { "id": "due", "name": "Due", "type": "date", "date": {} },
                    "Done": {
                        "id": "done_checkbox",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    },
                    "URL": { "id": "url", "name": "URL", "type": "url", "url": {} },
                    "Email": { "id": "email", "name": "Email", "type": "email", "email": {} },
                    "Phone": {
                        "id": "phone",
                        "name": "Phone",
                        "type": "phone_number",
                        "phone_number": {}
                    },
                    "Files": { "id": "files", "name": "Files", "type": "files", "files": {} },
                    "People": {
                        "id": "people",
                        "name": "People",
                        "type": "people",
                        "people": {}
                    },
                    "Created": {
                        "id": "created",
                        "name": "Created",
                        "type": "created_time",
                        "created_time": {}
                    },
                    "Created by": {
                        "id": "created_by",
                        "name": "Created by",
                        "type": "created_by",
                        "created_by": {}
                    },
                    "Edited": {
                        "id": "edited",
                        "name": "Edited",
                        "type": "last_edited_time",
                        "last_edited_time": {}
                    },
                    "Edited by": {
                        "id": "edited_by",
                        "name": "Edited by",
                        "type": "last_edited_by",
                        "last_edited_by": {}
                    },
                    "Task ID": {
                        "id": "task_id",
                        "name": "Task ID",
                        "type": "unique_id",
                        "unique_id": { "prefix": "TASK" }
                    },
                    "Place": { "id": "place", "name": "Place", "type": "place", "place": {} }
                }),
            },
        )
        .await
        .unwrap();

        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated_json["data_source"]["properties"]["Priority"]["select"]["options"][1]["name"],
            "High"
        );
        assert_eq!(
            updated_json["data_source"]["properties"]["Estimate"]["number"]["format"],
            "percent"
        );
        assert_eq!(
            updated_json["data_source"]["properties"]["Task ID"]["unique_id"]["prefix"],
            "TASK"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["table"]["property_order"][0],
            "title"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["table"]["hidden_property_ids"],
            json!([])
        );

        let stored_properties: String =
            sqlx::query_scalar("SELECT properties FROM notes_data_sources WHERE id = ?")
                .bind(DATA_SOURCE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored_properties_json: serde_json::Value =
            serde_json::from_str(&stored_properties).unwrap();
        assert_eq!(stored_properties_json["Place"]["type"], "place");

        let configuration: String =
            sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
                .bind(DATABASE_VIEW_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        let configuration_json: serde_json::Value = serde_json::from_str(&configuration).unwrap();
        assert_eq!(
            configuration_json["table"]["hidden_property_ids"],
            json!([])
        );
    });
}

#[test]
fn source_schema_keeps_native_property_icons_and_rejects_other_icon_sources() {
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
        let schema_with_icon = |icon: serde_json::Value| NoteDataSourceSchemaUpdate {
            properties: json!({
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {}, "icon": null },
                "Due": { "id": "due", "name": "Due", "type": "date", "date": {}, "icon": icon }
            }),
        };

        let updated = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            schema_with_icon(json!({
                "type": "icon",
                "icon": { "name": "alarm-clock", "color": "red", "extra": true },
                "extra": true
            })),
        )
        .await
        .unwrap();
        let properties = &serde_json::to_value(updated).unwrap()["data_source"]["properties"];
        assert_eq!(
            properties["Due"]["icon"],
            json!({ "type": "icon", "icon": { "name": "alarm-clock", "color": "red" } })
        );
        assert!(properties["Name"].get("icon").is_none());

        for (icon, message) in [
            (
                json!({ "type": "emoji", "emoji": "x" }),
                "property.icon must be a native icon",
            ),
            (
                json!({ "type": "icon", "icon": { "name": "Alarm Clock" } }),
                "property.icon.icon.name must be an icon slug",
            ),
            (
                json!({ "type": "icon", "icon": { "name": "alarm", "color": "teal" } }),
                "property.icon.icon.color must be a supported Notion icon color",
            ),
            (
                json!("lucide:alarm"),
                "property.icon must be an icon object",
            ),
        ] {
            let result = data_sources::schema::update_data_source_schema(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                schema_with_icon(icon),
            )
            .await;
            assert_eq!(result.err().unwrap(), message);
        }
    });
}

#[test]
fn update_local_data_source_schema_rejects_title_removal() {
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

        let result = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Details": {
                        "id": "details",
                        "name": "Details",
                        "type": "rich_text",
                        "rich_text": {}
                    }
                }),
            },
        )
        .await;

        assert_eq!(
            result.err().unwrap(),
            "data source schema must contain exactly one title property"
        );
    });
}

#[test]
fn source_schema_updates_preserve_current_saved_and_linked_view_presentation() {
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
        let initial = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None, None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
                    "Details": {"id": "details", "name": "Details", "type": "rich_text", "rich_text": {}},
                    "Estimate": {"id": "estimate", "name": "Estimate", "type": "number", "number": {"format": "number"}},
                    "Obsolete": {"id": "obsolete", "name": "Obsolete", "type": "select", "select": {"options": []}}
                }),
            },
        )
        .await
        .unwrap();
        let mut draft = serde_json::to_value(initial).unwrap()["data_source"]["properties"].clone();
        databases::create_linked_database_view(
            &pool,
            NoteLinkedDatabaseCreate {
                id: LINKED_DATABASE_A.to_string(),
                view_id: LINKED_DATABASE_VIEW_A.to_string(),
                source_block_id: DATABASE_A.to_string(),
                title: Some("Linked tasks".to_string()),
                parent: None,
                after_block_id: None,
                replace_block_id: None,
            },
        )
        .await
        .unwrap();
        let board =
            data_sources::layouts::board::data_source_board_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let board_json = serde_json::to_value(board).unwrap();
        let board_id = board_json["view"]["id"].as_str().unwrap();

        let filter = json!({"type": "and", "filters": [
            {"property_id": "details", "condition": "contains", "value": "keep"},
            {"property_id": "obsolete", "condition": "is_empty", "value": null}
        ]});
        let sorts = json!([
            {"property_id": "estimate", "direction": "descending"},
            {"property_id": "obsolete", "direction": "ascending"}
        ]);
        let current_table = json!({"type": "table", "table": {
            "property_order": ["title", "estimate", "obsolete", "details"],
            "hidden_property_ids": ["details", "obsolete"],
            "column_widths": {"title": 330, "estimate": 154, "obsolete": 199, "details": 280},
            "row_open_mode": "side_panel"
        }});
        let linked_table = json!({"type": "table", "table": {
            "property_order": ["title", "details", "estimate", "obsolete"],
            "hidden_property_ids": ["estimate"],
            "column_widths": {"title": 220, "details": 312, "estimate": 108},
            "row_open_mode": "full_page"
        }});
        let board_configuration = json!({"type": "board", "board": {
            "group_property_id": "obsolete",
            "group_order": ["old-option"],
            "hidden_group_ids": ["old-option"],
            "visible_property_ids": ["details", "obsolete", "estimate"],
            "row_open_mode": "side_panel"
        }});
        for (id, configuration) in [
            (DATABASE_VIEW_A, current_table),
            (LINKED_DATABASE_VIEW_A, linked_table),
            (board_id, board_configuration),
        ] {
            sqlx::query("UPDATE notes_database_views SET configuration = ?, filter = ?, sorts = ? WHERE id = ?")
                .bind(configuration.to_string())
                .bind(filter.to_string())
                .bind(sorts.to_string())
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }

        let properties = draft.as_object_mut().unwrap();
        let mut renamed = properties.remove("Details").unwrap();
        renamed["name"] = json!("Description");
        properties.insert("Description".to_string(), renamed);
        properties.remove("Obsolete");
        properties.insert(
            "Due".to_string(),
            json!({
                "id": "due", "name": "Due", "type": "date", "date": {}
            }),
        );
        let updated = data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            Some(DATABASE_VIEW_A),
            NoteDataSourceSchemaUpdate { properties: draft },
        )
        .await
        .unwrap();
        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated_json["data_source"]["properties"]["Description"]["id"],
            "details"
        );
        assert!(
            updated_json["data_source"]["properties"]
                .get("Obsolete")
                .is_none()
        );

        for (id, expected_order, expected_hidden, expected_widths, expected_mode) in [
            (
                DATABASE_VIEW_A,
                json!(["title", "estimate", "details", "due"]),
                json!(["details"]),
                json!({"title": 330, "estimate": 154, "details": 280}),
                "side_panel",
            ),
            (
                LINKED_DATABASE_VIEW_A,
                json!(["title", "details", "estimate", "due"]),
                json!(["estimate"]),
                json!({"title": 220, "details": 312, "estimate": 108}),
                "full_page",
            ),
        ] {
            let (configuration, filter, sorts): (String, String, String) = sqlx::query_as(
                "SELECT configuration, filter, sorts FROM notes_database_views WHERE id = ?",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            let configuration: serde_json::Value = serde_json::from_str(&configuration).unwrap();
            assert_eq!(configuration["table"]["property_order"], expected_order);
            assert_eq!(
                configuration["table"]["hidden_property_ids"],
                expected_hidden
            );
            assert_eq!(configuration["table"]["column_widths"], expected_widths);
            assert_eq!(configuration["table"]["row_open_mode"], expected_mode);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&filter).unwrap(),
                json!({
                    "type": "and", "filters": [{"property_id": "details", "condition": "contains", "value": "keep"}]
                })
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&sorts).unwrap(),
                json!([
                    {"property_id": "estimate", "direction": "descending"}
                ])
            );
        }
        let configuration: String =
            sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
                .bind(board_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let configuration: serde_json::Value = serde_json::from_str(&configuration).unwrap();
        assert_eq!(
            configuration["board"]["group_property_id"],
            serde_json::Value::Null
        );
        assert_eq!(configuration["board"]["group_order"], json!([]));
        assert_eq!(configuration["board"]["hidden_group_ids"], json!([]));
        assert_eq!(
            configuration["board"]["visible_property_ids"],
            json!(["details", "estimate"])
        );
        assert_eq!(configuration["board"]["row_open_mode"], "side_panel");
        data_sources::layouts::board::data_source_board_view(
            &pool,
            DATA_SOURCE_A,
            None,
            Some(board_id),
        )
        .await
        .unwrap();
    });
}

#[test]
fn source_schema_request_rejects_view_presentation_fields() {
    assert!(
        serde_json::from_value::<NoteDataSourceSchemaUpdate>(json!({
            "properties": {}, "property_order": ["title"]
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<NoteDataSourceSchemaUpdate>(json!({
            "properties": {}, "hidden_property_ids": []
        }))
        .is_err()
    );
}

#[test]
fn source_property_type_changes_clear_invalid_view_bindings_without_resetting_presentation() {
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
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None, None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
                    "Group": {"id": "group", "name": "Group", "type": "select", "select": {"options": []}},
                    "Due": {"id": "due", "name": "Due", "type": "date", "date": {}},
                    "Cover": {"id": "cover", "name": "Cover", "type": "files", "files": {}}
                }),
            },
        )
        .await
        .unwrap();
        let mut views = Vec::new();
        for view in [
            serde_json::to_value(
                data_sources::layouts::board::data_source_board_view(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                )
                .await
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(
                data_sources::layouts::list::data_source_list_view(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                )
                .await
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(
                data_sources::layouts::gallery::data_source_gallery_view(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                )
                .await
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(
                data_sources::layouts::calendar::data_source_calendar_view(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                )
                .await
                .unwrap(),
            )
            .unwrap(),
            serde_json::to_value(
                data_sources::layouts::timeline::data_source_timeline_view(
                    &pool,
                    DATA_SOURCE_A,
                    None,
                    None,
                )
                .await
                .unwrap(),
            )
            .unwrap(),
        ] {
            let id = view["view"]["id"].as_str().unwrap().to_string();
            let kind = view["view"]["type"].as_str().unwrap().to_string();
            let mut configuration = view["view"]["configuration"].clone();
            let settings = configuration[&kind].as_object_mut().unwrap();
            settings.insert(
                "visible_property_ids".to_string(),
                json!(["cover", "group", "due"]),
            );
            if matches!(kind.as_str(), "board" | "list" | "timeline") {
                settings.insert("group_property_id".to_string(), json!("group"));
                settings.insert("group_order".to_string(), json!(["old-group"]));
                settings.insert("hidden_group_ids".to_string(), json!(["old-group"]));
            }
            if matches!(kind.as_str(), "calendar" | "timeline") {
                settings.insert("date_property_id".to_string(), json!("due"));
            }
            if kind == "gallery" {
                settings.insert("cover_source".to_string(), json!("files_property"));
                settings.insert("cover_property_id".to_string(), json!("cover"));
            }
            sqlx::query("UPDATE notes_database_views SET configuration = ? WHERE id = ?")
                .bind(configuration.to_string())
                .bind(&id)
                .execute(&pool)
                .await
                .unwrap();
            views.push((id, kind, configuration));
        }
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None, None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": {"id": "title", "name": "Name", "type": "title", "title": {}},
                    "Group": {"id": "group", "name": "Group", "type": "number", "number": {"format": "number"}},
                    "Due": {"id": "due", "name": "Due", "type": "rich_text", "rich_text": {}},
                    "Cover": {"id": "cover", "name": "Cover", "type": "rich_text", "rich_text": {}}
                }),
            },
        )
        .await
        .unwrap();
        for (id, kind, mut expected) in views {
            let settings = expected[&kind].as_object_mut().unwrap();
            if matches!(kind.as_str(), "board" | "list" | "timeline") {
                settings.insert("group_property_id".to_string(), serde_json::Value::Null);
                settings.insert("group_order".to_string(), json!([]));
                settings.insert("hidden_group_ids".to_string(), json!([]));
            }
            if matches!(kind.as_str(), "calendar" | "timeline") {
                settings.insert("date_property_id".to_string(), serde_json::Value::Null);
            }
            if kind == "gallery" {
                settings.insert("cover_source".to_string(), json!("none"));
                settings.insert("cover_property_id".to_string(), serde_json::Value::Null);
            }
            let stored: String =
                sqlx::query_scalar("SELECT configuration FROM notes_database_views WHERE id = ?")
                    .bind(id)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&stored).unwrap(),
                expected
            );
        }
    });
}

use super::helpers::*;

#[test]
fn database_templates_create_row_pages_with_properties_and_body_blocks() {
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
                    "Priority": {
                        "id": "priority",
                        "name": "Priority",
                        "type": "select",
                        "select": {
                            "options": [
                                { "id": "high", "name": "High", "color": "red" }
                            ]
                        }
                    },
                    "Done": {
                        "id": "done",
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
                title: "Template source".to_string(),
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
                property_id: "priority".to_string(),
                value: json!("high"),
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
        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("paragraph", paragraph_payload("Plan first step")),
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_B),
                after: Some(BLOCK_B.to_string()),
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Review risks"),
                )],
            },
        )
        .await
        .unwrap();

        let template = data_sources::templates::create_data_source_template_from_row(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceTemplateCreateFromRow {
                id: TEMPLATE_A.to_string(),
                source_page_id: PAGE_B.to_string(),
                name: "QA task".to_string(),
                is_default: Some(true),
            },
        )
        .await
        .unwrap();
        let template_json = serde_json::to_value(template).unwrap();
        assert_eq!(template_json["name"], "QA task");
        assert_eq!(template_json["block_count"], 2);
        assert_eq!(template_json["is_default"], true);

        let applied = data_sources::templates::apply_data_source_template(
            &pool,
            DATA_SOURCE_A,
            TEMPLATE_A,
            NoteDataSourceTemplateApply {
                id: None,
                title: Some("QA pass".to_string()),
            },
        )
        .await
        .unwrap();
        let applied_json = serde_json::to_value(applied).unwrap();
        assert_eq!(
            applied_json["page"]["parent"]["data_source_id"],
            DATA_SOURCE_A
        );
        assert_eq!(
            applied_json["page"]["properties"]["Name"]["title"][0]["plain_text"],
            "QA pass"
        );
        assert_eq!(
            applied_json["page"]["properties"]["Priority"]["select"]["name"],
            "High"
        );
        assert_eq!(applied_json["page"]["properties"]["Done"]["checkbox"], true);
        assert_eq!(
            applied_json["blocks"]["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Plan first step"
        );
        assert_eq!(
            applied_json["blocks"]["results"][1]["paragraph"]["rich_text"][0]["plain_text"],
            "Review risks"
        );
        let blank = data_sources::templates::apply_data_source_template(
            &pool,
            DATA_SOURCE_A,
            TEMPLATE_A,
            NoteDataSourceTemplateApply {
                id: Some(PAGE_C.to_string()),
                title: Some(String::new()),
            },
        )
        .await
        .unwrap();
        let blank_json = serde_json::to_value(blank).unwrap();
        assert_eq!(blank_json["page"]["id"], PAGE_C);
        assert_eq!(
            blank_json["page"]["properties"]["Name"]["title"][0]["plain_text"],
            ""
        );
        assert_eq!(blank_json["blocks"]["results"].as_array().unwrap().len(), 2);
        assert!(
            data_sources::templates::apply_data_source_template(
                &pool,
                DATA_SOURCE_A,
                TEMPLATE_A,
                NoteDataSourceTemplateApply {
                    id: Some(PAGE_C.to_string()),
                    title: Some(String::new()),
                },
            )
            .await
            .is_err()
        );
        let rows = data_sources::rows::list_data_source_row_pages(&pool, DATA_SOURCE_A)
            .await
            .unwrap();
        let rows_json = serde_json::to_value(rows).unwrap();
        assert_eq!(
            rows_json
                .as_array()
                .unwrap()
                .iter()
                .filter(|page| page["id"] == PAGE_C)
                .count(),
            1
        );
    });
}

#[test]
fn database_buttons_require_confirmation_and_update_current_row_properties() {
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
                    "Done": {
                        "id": "done",
                        "name": "Done",
                        "type": "checkbox",
                        "checkbox": {}
                    },
                    "Finish": {
                        "id": "finish_button",
                        "name": "Finish",
                        "type": "button",
                        "button": {
                            "label": "Mark done",
                            "requires_confirmation": true,
                            "actions": [{
                                "type": "update_current_row_property",
                                "property_id": "done",
                                "property_name": "Done",
                                "property_type": "checkbox",
                                "value": true
                            }]
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
                title: "Write button tests".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();

        let stored_properties: String =
            sqlx::query_scalar("SELECT properties FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        let stored_json: serde_json::Value = serde_json::from_str(&stored_properties).unwrap();
        assert!(stored_json.get("Finish").is_none());

        let table =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        assert_eq!(
            table_json["rows"][0]["properties"]["Finish"]["button"]["label"],
            "Mark done"
        );

        let unconfirmed = data_sources::buttons::click_data_source_button(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceButtonClick {
                property_id: "finish_button".to_string(),
                confirmed: None,
            },
        )
        .await;
        assert_eq!(
            unconfirmed.err().unwrap(),
            "button action requires confirmation"
        );

        let updated = data_sources::buttons::click_data_source_button(
            &pool,
            DATA_SOURCE_A,
            PAGE_B,
            NoteDataSourceButtonClick {
                property_id: "finish_button".to_string(),
                confirmed: Some(true),
            },
        )
        .await
        .unwrap();
        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(updated_json["properties"]["Done"]["checkbox"], true);

        let invalid_broad_action = data_sources::schema::update_data_source_schema(
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
                    "Finish": {
                        "id": "finish_button",
                        "name": "Finish",
                        "type": "button",
                        "button": {
                            "label": "Broad change",
                            "requires_confirmation": false,
                            "actions": [{
                                "type": "delete_pages"
                            }]
                        }
                    }
                }),
            },
        )
        .await;
        assert_eq!(
            invalid_broad_action.err().unwrap(),
            "broad or destructive button actions require confirmation"
        );
    });
}

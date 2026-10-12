use super::helpers::*;

#[test]
fn table_group_counts_and_calculations_cover_filtered_rows_outside_the_window() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: "Calculations".to_string(),
                parent: Some(page_parent(PAGE_A)),
                after_block_id: Some(BLOCK_A.to_string()),
                replace_block_id: None,
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        data_sources::schema::update_data_source_schema(&pool, DATA_SOURCE_A, None, None, NoteDataSourceSchemaUpdate {
            properties: json!({
                "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                "Estimate": { "id": "estimate", "name": "Estimate", "type": "number", "number": { "format": "number" } },
                "Category": { "id": "category", "name": "Category", "type": "select", "select": { "options": [
                    { "id": "__all__", "name": "Even", "color": "blue" }, { "id": "odd", "name": "Odd", "color": "green" }
                ] } },
                "Double": { "id": "double", "name": "Double", "type": "formula", "formula": { "expression": "prop(\"Estimate\") * 2" } }
            }),
        }).await.unwrap();
        for index in 0..325 {
            let title = format!("{} {index:03}", if index >= 100 { "keep" } else { "skip" });
            let category = if index % 2 == 0 { "__all__" } else { "odd" };
            sqlx::query("INSERT INTO notes_pages (id, parent_type, parent_data_source_id, title, properties) VALUES (?, 'data_source_id', ?, ?, ?)")
                .bind(format!("60000000-0000-4000-8000-{index:012}"))
                .bind(DATA_SOURCE_A).bind(&title).bind(json!({
                    "Name": { "id": "title", "type": "title", "title": [{ "plain_text": title, "type": "text", "text": { "content": title, "link": null }, "annotations": { "bold": false, "italic": false, "strikethrough": false, "underline": false, "code": false, "color": "default" }, "href": null }] },
                    "Estimate": { "id": "estimate", "type": "number", "number": index },
                    "Category": { "id": "category", "type": "select", "select": { "id": category, "name": if category == "__all__" { "Even" } else { "Odd" }, "color": "default" } }
                }).to_string()).execute(&pool).await.unwrap();
        }
        let update = serde_json::from_value(json!({
            "filter": [{ "property_id": "title", "condition": "contains", "value": "keep" }], "sorts": [],
            "configuration": { "property_order": ["title", "estimate", "category", "double"], "hidden_property_ids": [], "column_widths": {}, "row_open_mode": "full_page",
                "group_property_id": "category", "collapsed_group_ids": ["odd"], "hide_empty_groups": true,
                "presentation": { "frozen_property_id": "estimate", "columns": { "estimate": { "wrap": true, "calculation": "sum" }, "double": { "calculation": "sum" }, "title": { "calculation": "count_all" } } }
            }
        })).unwrap();
        data_sources::layouts::table::update_data_source_table_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            update,
        )
        .await
        .unwrap();
        let dto = data_sources::layouts::table::data_source_table_view_window(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceViewWindowRequest {
                page_size: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let dto = serde_json::to_value(dto).unwrap();
        assert_eq!(dto["rows"].as_array().unwrap().len(), 5);
        assert_eq!(dto["total_row_count"], 225);
        assert_eq!(dto["group_counts"]["__all__"], 113);
        assert_eq!(dto["group_counts"]["odd"], 112);
        assert_eq!(dto["calculations"]["overall"]["estimate"], 47700.0);
        assert_eq!(dto["calculations"]["overall"]["double"], 95400.0);
        assert_eq!(dto["calculations"]["overall"]["title"], 225);
        assert_eq!(
            dto["calculations"]["groups"]["__all__"]["estimate"],
            23956.0
        );
        assert_eq!(
            dto["view"]["configuration"]["table"]["collapsed_group_ids"],
            json!(["odd"])
        );
        let invalid = serde_json::from_value(json!({
            "filter": [], "sorts": [], "configuration": { "property_order": [], "hidden_property_ids": [], "column_widths": {}, "row_open_mode": "full_page", "presentation": { "columns": { "category": { "calculation": "sum" } } } }
        })).unwrap();
        assert!(
            data_sources::layouts::table::update_data_source_table_view(
                &pool,
                DATA_SOURCE_A,
                None,
                None,
                invalid
            )
            .await
            .is_err()
        );
        let dto =
            data_sources::layouts::table::data_source_table_view(&pool, DATA_SOURCE_A, None, None)
                .await
                .unwrap();
        assert_eq!(serde_json::to_value(dto).unwrap()["total_row_count"], 225);
    });
}

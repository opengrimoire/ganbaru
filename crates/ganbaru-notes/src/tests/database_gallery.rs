use super::helpers::*;

#[test]
fn gallery_database_view_persists_card_preview_filters_and_sorts() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_A.to_string(),
                data_source_id: DATA_SOURCE_A.to_string(),
                view_id: DATABASE_VIEW_A.to_string(),
                title: "Media tasks".to_string(),
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
                    "Cover": {
                        "id": "cover_files",
                        "name": "Cover",
                        "type": "files",
                        "files": {}
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
                properties: Some(json!({
                    "Cover": {
                        "id": "cover_files",
                        "type": "files",
                        "files": [
                            {
                                "type": "external",
                                "external": { "url": "https://example.com/alpha.png" }
                            }
                        ]
                    }
                })),
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
                property_id: "estimate".to_string(),
                value: json!(5),
            },
        )
        .await
        .unwrap();

        let default_gallery = data_sources::layouts::gallery::data_source_gallery_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
        )
        .await
        .unwrap();
        let default_json = serde_json::to_value(default_gallery).unwrap();
        assert_eq!(default_json["view"]["type"], "gallery");
        assert_eq!(
            default_json["view"]["configuration"]["gallery"]["cover_source"],
            "page_cover"
        );
        let mut default_visible_property_ids =
            default_json["view"]["configuration"]["gallery"]["visible_property_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>();
        default_visible_property_ids.sort_unstable();
        assert_eq!(
            default_visible_property_ids,
            vec!["cover_files", "estimate", "published", "status"]
        );
        assert_eq!(default_json["rows"].as_array().unwrap().len(), 2);

        let invalid_cover = data_sources::layouts::gallery::update_data_source_gallery_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceGalleryViewUpdate {
                filter: vec![],
                sorts: vec![],
                configuration: NoteDataSourceGalleryConfigurationUpdate {
                    cover_source: "files_property".to_string(),
                    cover_property_id: Some("estimate".to_string()),
                    visible_property_ids: vec![],
                    card_size: "medium".to_string(),
                    fit_image: false,
                    row_open_mode: "full_page".to_string(),
                },
            },
        )
        .await;
        match invalid_cover {
            Ok(_) => panic!("gallery accepted a non-files cover property"),
            Err(error) => assert!(error.contains("files property")),
        }

        let updated = data_sources::layouts::gallery::update_data_source_gallery_view(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceGalleryViewUpdate {
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
                configuration: NoteDataSourceGalleryConfigurationUpdate {
                    cover_source: "files_property".to_string(),
                    cover_property_id: Some("cover_files".to_string()),
                    visible_property_ids: vec![
                        "estimate".to_string(),
                        "status".to_string(),
                        "estimate".to_string(),
                        "title".to_string(),
                        "missing".to_string(),
                    ],
                    card_size: "large".to_string(),
                    fit_image: true,
                    row_open_mode: "side_panel".to_string(),
                },
            },
        )
        .await
        .unwrap();
        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(
            updated_json["view"]["configuration"]["gallery"]["cover_property_id"],
            "cover_files"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["gallery"]["visible_property_ids"],
            json!(["estimate", "status"])
        );
        assert_eq!(
            updated_json["view"]["configuration"]["gallery"]["card_size"],
            "large"
        );
        assert_eq!(
            updated_json["view"]["configuration"]["gallery"]["row_open_mode"],
            "side_panel"
        );
        assert_eq!(updated_json["rows"][0]["id"], PAGE_C);
        assert_eq!(updated_json["rows"][1]["id"], PAGE_B);
        let alpha = updated_json["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == PAGE_B)
            .unwrap();
        assert_eq!(
            alpha["properties"]["Cover"]["files"][0]["external"]["url"],
            "https://example.com/alpha.png"
        );
    });
}

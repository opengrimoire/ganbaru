use super::helpers::*;

#[test]
fn append_and_update_table_of_contents_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "table_of_contents",
                    json!({ "color": "default" }),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("table_of_contents", json!({ "color": "blue_background" })),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "table_of_contents");
        assert_eq!(stored_json["table_of_contents"]["color"], "blue_background");
    });
}

#[test]
fn append_and_update_table_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "table", table_payload(2))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![
                    block(BLOCK_C, "table_row", table_row_payload(&["Name", "Status"])),
                    block(
                        BLOCK_D,
                        "table_row",
                        table_row_payload(&["Ganbaru", "Local"]),
                    ),
                ],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "table",
                json!({
                    "table_width": 3,
                    "has_column_header": true,
                    "has_row_header": true
                }),
            ),
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_D,
            block_update("table_row", table_row_payload(&["Ganbaru AI", "Offline"])),
        )
        .await
        .unwrap();

        let table_children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let table_children_json = serde_json::to_value(table_children).unwrap();
        assert_eq!(table_children_json["results"].as_array().unwrap().len(), 2);
        assert_eq!(table_children_json["results"][1]["type"], "table_row");
        let stored_table = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_table_json = serde_json::to_value(stored_table).unwrap();
        assert_eq!(stored_table_json["table"]["table_width"], 3);
        assert_eq!(stored_table_json["table"]["has_column_header"], true);
        assert_eq!(stored_table_json["table"]["has_row_header"], true);
        assert_eq!(
            table_children_json["results"][1]["table_row"]["cells"][0][0]["plain_text"],
            "Ganbaru AI"
        );
        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_D)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Ganbaru AI\tOffline");
    });
}

#[test]
fn append_children_rejects_invalid_table_children() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let row_under_page = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "table_row", table_row_payload(&["Nope"]))],
            },
        )
        .await;
        assert_eq!(
            row_under_page.err(),
            Some("table_row blocks must be children of table blocks".to_string())
        );

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "table", table_payload(2))],
            },
        )
        .await
        .unwrap();
        let paragraph_under_table = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Nope"))],
            },
        )
        .await;

        assert_eq!(
            paragraph_under_table.err(),
            Some("paragraph blocks cannot be children of table blocks".to_string())
        );
    });
}

#[test]
fn append_and_update_tab_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "tab", tab_payload())],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![
                    block(BLOCK_C, "paragraph", paragraph_icon_payload("Overview")),
                    block(BLOCK_D, "paragraph", paragraph_payload("Details")),
                ],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_C),
                after: None,
                children: vec![block(BLOCK_E, "to_do", todo_payload("Read notes", false))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_D),
                after: None,
                children: vec![block(BLOCK_F, "paragraph", paragraph_payload("Draft"))],
            },
        )
        .await
        .unwrap();

        writes::update_block(&pool, BLOCK_B, block_update("tab", tab_payload()))
            .await
            .unwrap();
        writes::update_block(
            &pool,
            BLOCK_C,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [rich_text("Plan")],
                    "color": "default",
                    "icon": {
                        "type": "emoji",
                        "emoji": "✅"
                    }
                }),
            ),
        )
        .await
        .unwrap();
        writes::move_block(
            &pool,
            BLOCK_E,
            NoteMoveBlock {
                parent: block_parent(BLOCK_D),
                after: Some(BLOCK_F.to_string()),
                before: None,
            },
        )
        .await
        .unwrap();
        writes::move_block(
            &pool,
            BLOCK_D,
            NoteMoveBlock {
                parent: block_parent(BLOCK_B),
                after: None,
                before: Some(BLOCK_C.to_string()),
            },
        )
        .await
        .unwrap();

        let tab_children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let tab_children_json = serde_json::to_value(tab_children).unwrap();
        assert_eq!(tab_children_json["results"].as_array().unwrap().len(), 2);
        assert_eq!(tab_children_json["results"][0]["type"], "paragraph");
        assert_eq!(
            tab_children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Details"
        );
        assert_eq!(
            tab_children_json["results"][1]["paragraph"]["rich_text"][0]["plain_text"],
            "Plan"
        );
        assert_eq!(
            tab_children_json["results"][1]["paragraph"]["icon"]["emoji"],
            "✅"
        );

        let first_tab_panel = reads::block_children(&pool, BLOCK_D, None, Some(10))
            .await
            .unwrap();
        let first_tab_panel_json = serde_json::to_value(first_tab_panel).unwrap();
        assert_eq!(first_tab_panel_json["results"][0]["type"], "paragraph");
        assert_eq!(
            first_tab_panel_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Draft"
        );
        assert_eq!(first_tab_panel_json["results"][1]["type"], "to_do");
        assert_eq!(
            first_tab_panel_json["results"][1]["to_do"]["rich_text"][0]["plain_text"],
            "Read notes"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "");
    });
}

#[test]
fn append_children_rejects_invalid_tab_children() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "tab", tab_payload())],
            },
        )
        .await
        .unwrap();

        let todo_under_tab = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "to_do", todo_payload("Nope", false))],
            },
        )
        .await;
        assert_eq!(
            todo_under_tab.err(),
            Some("to_do blocks cannot be children of tab blocks".to_string())
        );

        let icon_paragraph_under_page = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_B.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_icon_payload("Icon outside tab"),
                )],
            },
        )
        .await;
        assert_eq!(
            icon_paragraph_under_page.err(),
            Some("paragraph.icon is only supported for tab labels".to_string())
        );

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_icon_payload("Overview"),
                )],
            },
        )
        .await
        .unwrap();
        let moved_icon_label = writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_B.to_string()),
                before: None,
            },
        )
        .await;
        assert_eq!(
            moved_icon_label.err(),
            Some("paragraph.icon is only supported for tab labels".to_string())
        );
    });
}

#[test]
fn update_blocks_rejects_tab_child_shape_breaks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "to_do", todo_payload("Child", false))],
            },
        )
        .await
        .unwrap();

        let result = writes::update_block(&pool, BLOCK_B, block_update("tab", tab_payload())).await;

        assert_eq!(
            result.err(),
            Some("tab blocks can only contain paragraph blocks".to_string())
        );

        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "tab", tab_payload())],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Overview"))],
            },
        )
        .await
        .unwrap();

        let result = writes::update_block(
            &pool,
            BLOCK_B,
            block_update("paragraph", paragraph_payload("")),
        )
        .await;

        assert_eq!(
            result.err(),
            Some("tab blocks with labels cannot be converted to paragraph blocks".to_string())
        );
    });
}

#[test]
fn update_blocks_rejects_table_child_shape_breaks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        let result =
            writes::update_block(&pool, BLOCK_B, block_update("table", table_payload(2))).await;

        assert_eq!(
            result.err(),
            Some("table blocks can only contain table_row blocks".to_string())
        );
    });
}

#[test]
fn update_blocks_rejects_toggle_heading_child_shape_breaks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "heading_1",
                    heading_payload("Parent", true, Some(true)),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        let result = writes::update_block(
            &pool,
            BLOCK_B,
            block_update("heading_1", paragraph_payload("Parent")),
        )
        .await;

        assert_eq!(
            result.err(),
            Some("heading_1 blocks with children must stay toggleable".to_string())
        );
    });
}

#[test]
fn append_and_read_column_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "column_list", json!({}))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![
                    block(BLOCK_C, "column", column_payload(Some(0.5))),
                    block(BLOCK_D, "column", column_payload(Some(0.5))),
                ],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_C),
                after: None,
                children: vec![block(BLOCK_E, "paragraph", paragraph_payload("Left"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_D),
                after: None,
                children: vec![block(BLOCK_F, "paragraph", paragraph_payload("Right"))],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_C,
            block_update("column", column_payload(Some(0.7))),
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_D,
            block_update("column", column_payload(Some(0.3))),
        )
        .await
        .unwrap();
        writes::move_block(
            &pool,
            BLOCK_F,
            NoteMoveBlock {
                parent: block_parent(BLOCK_C),
                after: Some(BLOCK_E.to_string()),
                before: None,
            },
        )
        .await
        .unwrap();

        let column_list_children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let column_list_children_json = serde_json::to_value(column_list_children).unwrap();
        assert_eq!(
            column_list_children_json["results"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(column_list_children_json["results"][0]["type"], "column");
        assert_eq!(
            column_list_children_json["results"][0]["column"]["width_ratio"],
            0.7
        );
        assert_eq!(
            column_list_children_json["results"][1]["column"]["width_ratio"],
            0.3
        );

        let left_column_children = reads::block_children(&pool, BLOCK_C, None, Some(10))
            .await
            .unwrap();
        let left_column_children_json = serde_json::to_value(left_column_children).unwrap();
        assert_eq!(left_column_children_json["results"][0]["type"], "paragraph");
        assert_eq!(
            left_column_children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Left"
        );
        assert_eq!(
            left_column_children_json["results"][1]["paragraph"]["rich_text"][0]["plain_text"],
            "Right"
        );
    });
}

#[test]
fn append_children_rejects_invalid_column_children() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let column_under_page = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "column", column_payload(None))],
            },
        )
        .await;
        assert_eq!(
            column_under_page.err(),
            Some("column blocks must be children of column_list blocks".to_string())
        );

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "column_list", json!({}))],
            },
        )
        .await
        .unwrap();
        let paragraph_under_column_list = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Nope"))],
            },
        )
        .await;

        assert_eq!(
            paragraph_under_column_list.err(),
            Some("paragraph blocks cannot be children of column_list blocks".to_string())
        );
    });
}

#[test]
fn update_blocks_rejects_column_list_child_shape_breaks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        let result =
            writes::update_block(&pool, BLOCK_B, block_update("column_list", json!({}))).await;

        assert_eq!(
            result.err(),
            Some("column_list blocks can only contain column blocks".to_string())
        );
    });
}

use super::helpers::*;

#[test]
fn append_children_rejects_equation_parent_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "equation", equation_payload("e=mc^2"))],
            },
        )
        .await
        .unwrap();

        let result = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("equation blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_equation_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "equation", equation_payload("e=mc^2"))],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("equation", equation_payload("\\frac{a}{b}")),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "equation");
        assert_eq!(stored_json["equation"]["expression"], "\\frac{a}{b}");

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "\\frac{a}{b}");
    });
}

#[test]
fn append_and_update_unsupported_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "unsupported", unsupported_payload("form"))],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "unsupported",
                json!({
                    "block_type": "button",
                    "source_type": "notion",
                    "raw": {
                        "type": "unsupported",
                        "unsupported": {
                            "block_type": "button"
                        }
                    },
                    "warnings": ["Action content is not exposed"]
                }),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "unsupported");
        assert_eq!(stored_json["unsupported"]["block_type"], "button");
        assert_eq!(
            stored_json["unsupported"]["raw"]["unsupported"]["block_type"],
            "button"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            plain_text,
            "button notion Action content is not exposed raw payload preserved"
        );
    });
}

#[test]
fn update_block_persists_supported_block_color() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [rich_text("Colored")],
                    "color": "red_background"
                }),
            ),
        )
        .await
        .unwrap();

        let payload: String = sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(payload.contains("\"color\":\"red_background\""));
        assert!(payload.contains("Colored"));
    });
}

#[test]
fn update_block_round_trips_inline_formatting_annotations() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [{
                        "type": "text",
                        "text": {
                            "content": "Shortcut text",
                            "link": null
                        },
                        "annotations": {
                            "bold": true,
                            "italic": true,
                            "strikethrough": true,
                            "underline": true,
                            "code": true,
                            "color": "default"
                        },
                        "plain_text": "Shortcut text",
                        "href": null
                    }],
                    "color": "default"
                }),
            ),
        )
        .await
        .unwrap();

        let children = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        let annotations = &children_json["results"][0]["paragraph"]["rich_text"][0]["annotations"];
        assert_eq!(annotations["bold"], true);
        assert_eq!(annotations["italic"], true);
        assert_eq!(annotations["strikethrough"], true);
        assert_eq!(annotations["underline"], true);
        assert_eq!(annotations["code"], true);
        assert_eq!(
            children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Shortcut text"
        );
    });
}

#[test]
fn rich_text_paste_payloads_round_trip_current_and_appended_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [
                        rich_text("Before "),
                        annotated_rich_text("styled", "blue_background"),
                        linked_rich_text(" docs", "https://example.com/docs")
                    ],
                    "color": "default"
                }),
            ),
        )
        .await
        .unwrap();

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("Next "),
                            linked_rich_text("reference", "mailto:team@example.com")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let children = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        let first_rich_text = &children_json["results"][0]["paragraph"]["rich_text"];
        assert_eq!(first_rich_text[0]["plain_text"], "Before ");
        assert_eq!(first_rich_text[1]["annotations"]["bold"], true);
        assert_eq!(
            first_rich_text[1]["annotations"]["color"],
            "blue_background"
        );
        assert_eq!(
            first_rich_text[2]["text"]["link"]["url"],
            "https://example.com/docs"
        );
        assert_eq!(first_rich_text[2]["href"], "https://example.com/docs");

        let second_rich_text = &children_json["results"][1]["paragraph"]["rich_text"];
        assert_eq!(second_rich_text[0]["plain_text"], "Next ");
        assert_eq!(
            second_rich_text[1]["text"]["link"]["url"],
            "mailto:team@example.com"
        );
        assert_eq!(second_rich_text[1]["href"], "mailto:team@example.com");

        let stored_plain_text: Vec<String> = sqlx::query_scalar(
            "SELECT plain_text FROM notes_blocks WHERE id IN (?, ?) ORDER BY sort_order",
        )
        .bind(BLOCK_A)
        .bind(BLOCK_B)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            stored_plain_text,
            vec![
                "Before styled docs".to_string(),
                "Next reference".to_string()
            ]
        );
    });
}

#[test]
fn update_block_round_trips_inline_equations() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [
                        rich_text("Use "),
                        inline_equation_rich_text("\\frac{a}{b}"),
                        rich_text(" here")
                    ],
                    "color": "default"
                }),
            ),
        )
        .await
        .unwrap();

        let children = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        let rich_text = &children_json["results"][0]["paragraph"]["rich_text"];
        assert_eq!(rich_text[0]["plain_text"], "Use ");
        assert_eq!(rich_text[1]["type"], "equation");
        assert_eq!(rich_text[1]["equation"]["expression"], "\\frac{a}{b}");
        assert_eq!(rich_text[1]["plain_text"], "\\frac{a}{b}");
        assert_eq!(rich_text[1]["href"], serde_json::Value::Null);
        assert_eq!(rich_text[2]["plain_text"], " here");
        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Use \\frac{a}{b} here");
    });
}

#[test]
fn append_and_update_toggle_blocks_round_trip() {
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
                    "toggle",
                    json!({
                        "rich_text": [rich_text("Details")],
                        "color": "blue_background",
                        "ganbaru_open": true
                    }),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "toggle",
                json!({
                    "rich_text": [rich_text("Details")],
                    "color": "blue_background",
                    "ganbaru_open": false
                }),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "toggle");
        assert_eq!(stored_json["toggle"]["ganbaru_open"], false);
        assert_eq!(stored_json["toggle"]["color"], "blue_background");
    });
}

#[test]
fn append_and_update_callout_blocks_round_trip() {
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
                    "callout",
                    json!({
                        "rich_text": [rich_text("Remember this")],
                        "color": "yellow_background",
                        "icon": {
                            "type": "icon",
                            "icon": {
                                "name": "info",
                                "color": "gray"
                            }
                        }
                    }),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "callout",
                json!({
                    "rich_text": [rich_text("Updated")],
                    "color": "blue_background",
                    "icon": {
                        "type": "icon",
                        "icon": {
                            "name": "info",
                            "color": "blue"
                        }
                    }
                }),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "callout");
        assert_eq!(stored_json["callout"]["color"], "blue_background");
        assert_eq!(stored_json["callout"]["icon"]["icon"]["color"], "blue");
        assert_eq!(
            stored_json["callout"]["rich_text"][0]["plain_text"],
            "Updated"
        );
    });
}

#[test]
fn callout_icon_assets_follow_icon_changes_and_block_conversion() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let asset_path =
            "notes/page-icons/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png";
        let callout_with_file = json!({
            "rich_text": [rich_text("Notice")],
            "color": "gray_background",
            "icon": {
                "type": "file",
                "file": {
                    "url": format!("ganbaru-asset:{asset_path}"),
                    "ganbaru_asset_path": asset_path,
                    "content_type": "image/png",
                    "byte_size": 42,
                    "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                }
            }
        });
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "callout", callout_with_file.clone())],
            },
        )
        .await
        .unwrap();

        assert_eq!(callout_icon_reference_count(&pool, asset_path).await, 1);

        let mut emoji = callout_with_file.clone();
        emoji["icon"] = json!({ "type": "emoji", "emoji": "💡" });
        writes::update_block(&pool, BLOCK_B, block_update("callout", emoji))
            .await
            .unwrap();
        assert_eq!(callout_icon_reference_count(&pool, asset_path).await, 0);

        writes::update_block(&pool, BLOCK_B, block_update("callout", callout_with_file))
            .await
            .unwrap();
        assert_eq!(callout_icon_reference_count(&pool, asset_path).await, 1);

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("paragraph", json!({ "rich_text": [rich_text("Text")] })),
        )
        .await
        .unwrap();
        assert_eq!(callout_icon_reference_count(&pool, asset_path).await, 0);
    });
}

async fn callout_icon_reference_count(pool: &SqlitePool, asset_path: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM notes_asset_references
         WHERE owner_type = 'block' AND owner_id = ? AND asset_id = ?
           AND role = 'callout_icon'",
    )
    .bind(BLOCK_B)
    .bind(asset_path)
    .fetch_one(pool)
    .await
    .unwrap()
}

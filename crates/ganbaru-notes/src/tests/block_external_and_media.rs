use super::helpers::*;

#[test]
fn append_children_rejects_bookmark_parent_blocks() {
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
                    "bookmark",
                    bookmark_payload("https://example.com", ""),
                )],
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
            Some("bookmark blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_bookmark_blocks_round_trip() {
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
                    "bookmark",
                    bookmark_payload("https://example.com", "Reference"),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "bookmark",
                bookmark_payload("https://example.com/updated", "Updated"),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "bookmark");
        assert_eq!(
            stored_json["bookmark"]["url"],
            "https://example.com/updated"
        );
        assert_eq!(
            stored_json["bookmark"]["caption"][0]["plain_text"],
            "Updated"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Updated https://example.com/updated");
    });
}

#[test]
fn append_children_rejects_link_preview_parent_blocks() {
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
                    "link_preview",
                    link_preview_payload("https://github.com/example/repo/pull/123"),
                )],
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
            Some("link_preview blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_link_preview_blocks_round_trip() {
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
                    "link_preview",
                    link_preview_payload("https://example.com"),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "link_preview",
                link_preview_payload("https://github.com/example/repo/pull/123"),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "link_preview");
        assert_eq!(
            stored_json["link_preview"]["url"],
            "https://github.com/example/repo/pull/123"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "https://github.com/example/repo/pull/123");
    });
}

#[test]
fn append_original_synced_block_accepts_children() {
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
                    "synced_block",
                    synced_block_payload_original(),
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
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("Synced child"),
                )],
            },
        )
        .await
        .unwrap();

        let children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        assert_eq!(children_json["results"][0]["type"], "paragraph");
        assert_eq!(
            children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Synced child"
        );
    });
}

#[test]
fn append_children_rejects_duplicate_synced_block_parent_blocks() {
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
                    "synced_block",
                    synced_block_payload_duplicate(BLOCK_A),
                )],
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
            Some("synced_block blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_synced_blocks_round_trip() {
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
                    "synced_block",
                    synced_block_payload_original(),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("synced_block", synced_block_payload_duplicate(BLOCK_A)),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "synced_block");
        assert_eq!(
            stored_json["synced_block"]["synced_from"]["block_id"],
            BLOCK_A
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, BLOCK_A);
    });
}

#[test]
fn update_synced_block_with_children_rejects_duplicate_payload() {
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
                    "synced_block",
                    synced_block_payload_original(),
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
            block_update("synced_block", synced_block_payload_duplicate(BLOCK_A)),
        )
        .await;

        assert_eq!(
            result.err(),
            Some("synced_block blocks with children must stay original".to_string())
        );
    });
}

#[test]
fn six_heading_levels_append_update_and_preserve_toggle_children() {
    for heading in [
        "heading_1",
        "heading_2",
        "heading_3",
        "heading_4",
        "heading_5",
        "heading_6",
    ] {
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
                        heading,
                        heading_payload("Details", true, Some(false)),
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
            writes::update_block(
                &pool,
                BLOCK_B,
                block_update(
                    heading,
                    heading_payload("Updated details", true, Some(true)),
                ),
            )
            .await
            .unwrap();

            let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
            let stored_json = serde_json::to_value(stored).unwrap();
            assert_eq!(stored_json["type"], heading);
            assert_eq!(stored_json["has_children"], true);
            assert_eq!(
                stored_json[heading]["rich_text"][0]["plain_text"],
                "Updated details"
            );
            assert_eq!(stored_json[heading]["is_toggleable"], true);
            assert_eq!(stored_json[heading]["ganbaru_open"], true);

            let children = reads::block_children(&pool, BLOCK_B, None, Some(10))
                .await
                .unwrap();
            let children_json = serde_json::to_value(children).unwrap();
            assert_eq!(children_json["results"][0]["id"], BLOCK_C);

            let result = writes::update_block(
                &pool,
                BLOCK_B,
                block_update(heading, paragraph_payload("Not toggleable")),
            )
            .await;
            assert_eq!(
                result.err(),
                Some(format!(
                    "{heading} blocks with children must stay toggleable"
                ))
            );

            let plain_text: String =
                sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                    .bind(BLOCK_B)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(plain_text, "Updated details");
        });
    }
}

#[test]
fn append_and_update_child_database_blocks_round_trip() {
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
                    "child_database",
                    child_database_payload("Tasks"),
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
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("Database child"),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("child_database", child_database_payload("Roadmap")),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "child_database");
        assert_eq!(stored_json["has_children"], true);
        assert_eq!(stored_json["child_database"]["title"], "Roadmap");

        let children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        assert_eq!(children_json["results"][0]["type"], "paragraph");
        assert_eq!(
            children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Database child"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Roadmap");
    });
}

#[test]
fn append_update_and_duplicate_template_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "template", template_payload("Add task"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(
                    BLOCK_C,
                    "to_do",
                    todo_payload("Template item", false),
                )],
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("template", template_payload("Plan day")),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "template");
        assert_eq!(stored_json["has_children"], true);
        assert_eq!(
            stored_json["template"]["rich_text"][0]["plain_text"],
            "Plan day"
        );

        writes::duplicate_block(
            &pool,
            BLOCK_B,
            NoteDuplicateBlock {
                duplicated_block_ids: vec![
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_B.to_string(),
                        duplicate_id: BLOCK_E.to_string(),
                    },
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_C.to_string(),
                        duplicate_id: BLOCK_F.to_string(),
                    },
                ],
            },
        )
        .await
        .unwrap();

        let duplicate = reads::get_block(&pool, BLOCK_E, false).await.unwrap();
        let duplicate_json = serde_json::to_value(duplicate).unwrap();
        assert_eq!(duplicate_json["type"], "template");
        assert_eq!(
            duplicate_json["template"]["rich_text"][0]["plain_text"],
            "Plan day"
        );
        assert_eq!(duplicate_json["has_children"], true);

        let duplicate_children = reads::block_children(&pool, BLOCK_E, None, Some(10))
            .await
            .unwrap();
        let duplicate_children_json = serde_json::to_value(duplicate_children).unwrap();
        assert_eq!(duplicate_children_json["results"][0]["id"], BLOCK_F);
        assert_eq!(duplicate_children_json["results"][0]["type"], "to_do");

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Plan day");
    });
}

#[test]
fn append_update_and_duplicate_button_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "button", button_payload("Add agenda"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    paragraph_payload("Generated agenda item"),
                )],
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("button", button_payload("Add checklist")),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "button");
        assert_eq!(stored_json["has_children"], true);
        assert_eq!(
            stored_json["button"]["rich_text"][0]["plain_text"],
            "Add checklist"
        );
        assert_eq!(
            stored_json["button"]["actions"][0]["position"],
            "below_button"
        );

        writes::duplicate_block(
            &pool,
            BLOCK_B,
            NoteDuplicateBlock {
                duplicated_block_ids: vec![
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_B.to_string(),
                        duplicate_id: BLOCK_E.to_string(),
                    },
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_C.to_string(),
                        duplicate_id: BLOCK_F.to_string(),
                    },
                ],
            },
        )
        .await
        .unwrap();

        let duplicate = reads::get_block(&pool, BLOCK_E, false).await.unwrap();
        let duplicate_json = serde_json::to_value(duplicate).unwrap();
        assert_eq!(duplicate_json["type"], "button");
        assert_eq!(
            duplicate_json["button"]["rich_text"][0]["plain_text"],
            "Add checklist"
        );
        assert_eq!(duplicate_json["has_children"], true);

        let duplicate_children = reads::block_children(&pool, BLOCK_E, None, Some(10))
            .await
            .unwrap();
        let duplicate_children_json = serde_json::to_value(duplicate_children).unwrap();
        assert_eq!(duplicate_children_json["results"][0]["id"], BLOCK_F);
        assert_eq!(duplicate_children_json["results"][0]["type"], "paragraph");

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "Add checklist");
    });
}

#[test]
fn append_children_rejects_embed_parent_blocks() {
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
                    "embed",
                    embed_payload("https://example.com"),
                )],
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
            Some("embed blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_embed_blocks_round_trip() {
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
                    "embed",
                    embed_payload("https://example.com"),
                )],
            },
        )
        .await
        .unwrap();

        writes::update_block(
            &pool,
            BLOCK_B,
            block_update(
                "embed",
                embed_payload("https://player.vimeo.com/video/226053498"),
            ),
        )
        .await
        .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "embed");
        assert_eq!(
            stored_json["embed"]["url"],
            "https://player.vimeo.com/video/226053498"
        );

        let plain_text: String =
            sqlx::query_scalar("SELECT plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(plain_text, "https://player.vimeo.com/video/226053498");
    });
}

#[test]
fn text_indentation_survives_save_reopen_and_outline_reads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        for level in [1, 12, 0] {
            let mut payload = paragraph_payload("Indented text");
            payload["ganbaru_indent"] = json!(level);
            writes::update_block(&pool, BLOCK_A, block_update("paragraph", payload))
                .await
                .unwrap();
            let saved =
                serde_json::to_value(reads::get_block(&pool, BLOCK_A, false).await.unwrap())
                    .unwrap();
            assert_eq!(saved["paragraph"]["ganbaru_indent"], level);
            let reopened =
                serde_json::to_value(reads::open_page(&pool, PAGE_A).await.unwrap()).unwrap();
            assert_eq!(reopened["outlines"][0]["ganbaru_indent"], level);
            let markdown = serde_json::to_value(
                transfers::markdown_export::export_page(
                    &pool,
                    NoteMarkdownExportRequest {
                        page_id: PAGE_A.to_string(),
                        include_page_title: Some(false),
                        include_comments: Some(false),
                        include_resolved_comments: Some(false),
                    },
                )
                .await
                .unwrap(),
            )
            .unwrap();
            assert!(
                markdown["markdown"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("{}Indented text", "  ".repeat(level as usize)))
            );
            let html = serde_json::to_value(
                transfers::html_export::export_page(
                    &pool,
                    NoteHtmlExportRequest {
                        page_id: PAGE_A.to_string(),
                        include_page_tree: Some(false),
                        include_comments: Some(false),
                        include_resolved_comments: Some(false),
                        include_assets: Some(false),
                        include_database_views: Some(false),
                    },
                )
                .await
                .unwrap(),
            )
            .unwrap();
            let contents = html["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|file| file["path"] == "index.html")
                .unwrap()["contents"]
                .as_str()
                .unwrap();
            if level > 0 {
                assert!(contents.contains(&format!("margin-inline-start:calc({level} * 1.25rem)")));
            }
        }
        for invalid in [
            json!(-1),
            json!(1.5),
            json!("2"),
            json!(9_007_199_254_740_992_u64),
        ] {
            let mut payload = paragraph_payload("Rejected");
            payload["ganbaru_indent"] = invalid;
            assert!(
                writes::update_block(&pool, BLOCK_A, block_update("paragraph", payload))
                    .await
                    .is_err()
            );
        }
        let saved =
            serde_json::to_value(reads::get_block(&pool, BLOCK_A, false).await.unwrap()).unwrap();
        assert_eq!(
            saved["paragraph"]["rich_text"][0]["plain_text"],
            "Indented text"
        );
    });
}

#[test]
fn embedded_block_indentation_survives_save_and_reopen() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        for (block_type, mut payload) in [
            ("divider", json!({})),
            ("tab", tab_payload()),
            ("code", code_payload("value", "plain text")),
            (
                "image",
                media_payload("https://example.com/image.png", "Caption", None),
            ),
            ("table", table_payload(2)),
        ] {
            payload["ganbaru_indent"] = json!(3);
            writes::update_block(&pool, BLOCK_A, block_update(block_type, payload.clone()))
                .await
                .unwrap();
            let saved =
                serde_json::to_value(reads::get_block(&pool, BLOCK_A, false).await.unwrap())
                    .unwrap();
            assert_eq!(saved[block_type], payload);
            let reopened =
                serde_json::to_value(reads::open_page(&pool, PAGE_A).await.unwrap()).unwrap();
            assert_eq!(reopened["outlines"][0]["ganbaru_indent"], 3);
            payload["ganbaru_indent"] = json!(-1);
            assert!(
                writes::update_block(&pool, BLOCK_A, block_update(block_type, payload))
                    .await
                    .is_err()
            );
        }
        assert!(
            crate::validation::validate_block_payload(
                "tab",
                &json!({"ganbaru_indent": 1, "unexpected": true})
            )
            .is_err()
        );
    });
}

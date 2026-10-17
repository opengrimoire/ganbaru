use super::helpers::*;

const BLOCK_G: &str = "12121212-1212-4212-8212-121212121212";
const BLOCK_H: &str = "13131313-1313-4313-8313-131313131313";
const BLOCK_I: &str = "14141414-1414-4414-8414-141414141414";
const BLOCK_J: &str = "15151515-1515-4515-8515-151515151515";
const BLOCK_L: &str = "17171717-1717-4717-8717-171717171717";
const BLOCK_M: &str = "18181818-1818-4818-8818-181818181818";
const BLOCK_N: &str = "19191919-1919-4919-8919-191919191919";

#[test]
fn markdown_export_renders_canonical_blocks_rich_text_media_tables_and_comments() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Export page".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [
                        rich_text("Read "),
                        linked_rich_text("docs", "https://example.com/docs"),
                        rich_text(" with "),
                        page_mention(PAGE_B, "Target"),
                        rich_text(" and "),
                        inline_equation_rich_text("x^2")
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
                children: vec![
                    block(BLOCK_B, "heading_2", heading_payload("Plan", false, None)),
                    block(BLOCK_C, "to_do", todo_payload("Done", true)),
                    block(BLOCK_D, "bulleted_list_item", paragraph_payload("Bullet")),
                    block(BLOCK_E, "numbered_list_item", paragraph_payload("Numbered")),
                    block(BLOCK_F, "toggle", paragraph_payload("More")),
                    block(BLOCK_G, "callout", paragraph_icon_payload("Remember")),
                    block(BLOCK_H, "code", code_payload("let value = 1;", "rust")),
                    block(BLOCK_I, "table", table_payload(2)),
                    block(
                        BLOCK_J,
                        "image",
                        media_payload("https://example.com/diagram.png", "Diagram", None),
                    ),
                ],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_F),
                after: None,
                children: vec![block(
                    BLOCK_L,
                    "paragraph",
                    paragraph_payload("Nested child"),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_I),
                after: None,
                children: vec![
                    block(BLOCK_M, "table_row", table_row_payload(&["Name", "Status"])),
                    block(BLOCK_N, "table_row", table_row_payload(&["Task", "Done"])),
                ],
            },
        )
        .await
        .unwrap();
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Page note")],
                attachments: None,
            },
        )
        .await
        .unwrap();
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_B.to_string(),
                parent: Some(block_parent(BLOCK_A)),
                discussion_id: None,
                anchor: Some(NoteCommentAnchorCreate {
                    start: 0,
                    end: 4,
                    text: "Read".to_string(),
                    prefix: String::new(),
                    suffix: " docs".to_string(),
                }),
                rich_text: vec![rich_text("Block note")],
                attachments: None,
            },
        )
        .await
        .unwrap();

        let result = transfers::markdown_export::export_page(
            &pool,
            NoteMarkdownExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_title: Some(true),
                include_comments: Some(true),
                include_resolved_comments: Some(false),
            },
        )
        .await
        .unwrap();
        let result_json = serde_json::to_value(result).unwrap();
        let markdown = result_json["markdown"].as_str().unwrap();

        assert!(markdown.contains("# Export page"));
        assert!(markdown.contains("[docs](https://example.com/docs)"));
        assert!(markdown.contains("$x^2$"));
        assert!(markdown.contains("## Plan"));
        assert!(markdown.contains("- [x] Done\n- Bullet\n1. Numbered"));
        assert!(markdown.contains("<details><summary>More</summary>"));
        assert!(markdown.contains("Nested child"));
        assert!(markdown.contains("> [!NOTE] [star] Remember"));
        assert!(markdown.contains("```rust\nlet value = 1;\n```"));
        assert!(markdown.contains("| Name | Status |\n| --- | --- |\n| Task | Done |"));
        assert!(markdown.contains("![Diagram](https://example.com/diagram.png)"));
        assert!(markdown.contains("## Comments"));
        assert!(markdown.contains("Page discussion"));
        assert!(markdown.contains("Block `aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa` on \"Read\""));
        assert!(markdown.contains("You: Page note"));
        assert!(markdown.contains("You: Block note"));
        assert_eq!(result_json["exported_block_count"], 13);
        assert_eq!(result_json["exported_comment_count"], 2);
    });
}

#[test]
fn markdown_export_is_deterministic_and_reports_unsupported_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "unsupported", unsupported_payload("form")),
                    block(
                        BLOCK_C,
                        "file",
                        local_media_payload(
                            "notes/files/local.txt",
                            "text/plain",
                            12,
                            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                            "Local file",
                            Some("local.txt"),
                        ),
                    ),
                ],
            },
        )
        .await
        .unwrap();

        let request = NoteMarkdownExportRequest {
            page_id: PAGE_A.to_string(),
            include_page_title: None,
            include_comments: None,
            include_resolved_comments: None,
        };
        let first = serde_json::to_value(
            transfers::markdown_export::export_page(&pool, request)
                .await
                .unwrap(),
        )
        .unwrap();
        let second = serde_json::to_value(
            transfers::markdown_export::export_page(
                &pool,
                NoteMarkdownExportRequest {
                    page_id: PAGE_A.to_string(),
                    include_page_title: None,
                    include_comments: None,
                    include_resolved_comments: None,
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();

        assert_eq!(first["markdown"], second["markdown"]);
        assert_eq!(first["exported_comment_count"], 0);
        let diagnostics = first["diagnostics"].as_array().unwrap();
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == "markdown_export_unsupported_block"
                && diagnostic["block_id"] == BLOCK_B
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["code"] == "markdown_export_local_asset_reference"
                && diagnostic["block_id"] == BLOCK_C
        }));
    });
}

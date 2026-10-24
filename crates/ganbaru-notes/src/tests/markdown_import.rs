use super::helpers::*;

#[test]
fn markdown_import_creates_canonical_page_blocks_and_table_children() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let result = transfers::markdown_import::import_page(
            &pool,
            NoteMarkdownImportRequest {
                parent: workspace_parent(),
                title: None,
                source_name: Some("weekly-plan.md".to_string()),
                after_block_id: None,
                markdown: r#"---
title: Weekly plan
tags: ignored
---
# Overview

Paragraph with [docs](https://example.com/docs) and [mail](mailto:team@example.com).

- [x] Done item
- Bullet item
1. Numbered item

> Quote line
> continues

```rust
let value = 1;
```

---

![Diagram](https://example.com/diagram.png)

| Name | Status |
| --- | --- |
| Task | Done |
"#
                .to_string(),
            },
        )
        .await
        .unwrap();

        let result_json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            result_json["page"]["page"]["properties"]["title"]["title"][0]["plain_text"],
            "Weekly plan"
        );
        assert_eq!(result_json["imported_block_count"], 12);
        assert!(
            result_json["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| {
                    diagnostic["code"] == "markdown_frontmatter_field_ignored"
                        && diagnostic["severity"] == "info"
                })
        );

        let blocks = result_json["page"]["blocks"]["results"].as_array().unwrap();
        let block_types = blocks
            .iter()
            .map(|block| block["type"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            block_types,
            vec![
                "heading_1",
                "paragraph",
                "to_do",
                "bulleted_list_item",
                "numbered_list_item",
                "quote",
                "code",
                "divider",
                "image",
                "table"
            ]
        );
        assert_eq!(
            blocks[1]["paragraph"]["rich_text"][1]["text"]["link"]["url"],
            "https://example.com/docs"
        );
        assert_eq!(blocks[2]["to_do"]["checked"], true);
        assert_eq!(
            blocks[5]["quote"]["rich_text"][0]["plain_text"],
            "Quote line\ncontinues"
        );
        assert_eq!(blocks[6]["code"]["language"], "rust");
        assert_eq!(
            blocks[8]["image"]["external"]["url"],
            "https://example.com/diagram.png"
        );

        let table_id = blocks[9]["id"].as_str().unwrap();
        let rows = reads::block_children(&pool, table_id, None, Some(10))
            .await
            .unwrap();
        let rows_json = serde_json::to_value(rows).unwrap();
        assert_eq!(rows_json["results"].as_array().unwrap().len(), 2);
        assert_eq!(
            rows_json["results"][0]["table_row"]["cells"][0][0]["plain_text"],
            "Name"
        );
        assert_eq!(
            rows_json["results"][1]["table_row"]["cells"][1][0]["plain_text"],
            "Done"
        );

        let stored_page_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_pages
             WHERE title = 'Weekly plan'
               AND source_provider = 'markdown'
               AND source_object_id = 'weekly-plan.md'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_page_count, 1);
    });
}

#[test]
fn markdown_import_reports_unsupported_or_unsafe_markdown_without_copying_files() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let result = transfers::markdown_import::import_page(
            &pool,
            NoteMarkdownImportRequest {
                parent: workspace_parent(),
                title: Some("Unsafe import".to_string()),
                source_name: None,
                after_block_id: None,
                markdown: r#"# Deep

###### Small heading

![Local](images/local.png)

<aside>Raw HTML</aside>

[ref]: https://example.com

Paragraph with [relative](docs/page.md) link and inline ![image](https://example.com/a.png).
"#
                .to_string(),
            },
        )
        .await
        .unwrap();

        let result_json = serde_json::to_value(result).unwrap();
        let diagnostics = result_json["diagnostics"].as_array().unwrap();
        for code in [
            "markdown_image_reference_blocked",
            "markdown_html_unsupported",
            "markdown_reference_definition_unsupported",
            "markdown_link_url_unsupported",
            "markdown_inline_image_preserved_as_text",
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == code),
                "missing diagnostic {code}"
            );
        }

        let blocks = result_json["page"]["blocks"]["results"].as_array().unwrap();
        assert!(blocks.iter().any(|block| block["type"] == "unsupported"
            && block["unsupported"]["source_type"] == "markdown_import"));
        let imported_file_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets WHERE source_type = 'imported'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(imported_file_assets, 0);
    });
}

#[test]
fn six_heading_levels_survive_markdown_html_import_and_export() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let markdown = (1..=6)
            .map(|level| format!("{} Level {level}", "#".repeat(level)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let html = (1..=6)
            .map(|level| format!("<h{level}>Level {level}</h{level}>"))
            .collect::<Vec<_>>()
            .join("\n");
        let imported_markdown = transfers::markdown_import::import_page(
            &pool,
            NoteMarkdownImportRequest {
                parent: workspace_parent(),
                title: Some("Headings".to_string()),
                source_name: None,
                after_block_id: None,
                markdown: markdown.clone(),
            },
        )
        .await
        .unwrap();
        let imported_html = transfers::html_import::import_page(
            &pool,
            NoteHtmlImportRequest {
                parent: workspace_parent(),
                title: Some("Headings".to_string()),
                source_name: None,
                after_block_id: None,
                html,
                keep_external_file_references: Some(false),
                project_id: None,
            },
        )
        .await
        .unwrap();
        for imported in [
            serde_json::to_value(imported_markdown).unwrap(),
            serde_json::to_value(imported_html).unwrap(),
        ] {
            assert!(imported["diagnostics"].as_array().unwrap().is_empty());
            let blocks = imported["page"]["blocks"]["results"].as_array().unwrap();
            assert_eq!(blocks.len(), 6);
            for (index, block) in blocks.iter().enumerate() {
                assert_eq!(block["type"], format!("heading_{}", index + 1));
            }
            let page_id = imported["page"]["page"]["id"].as_str().unwrap().to_string();
            let exported = transfers::markdown_export::export_page(
                &pool,
                NoteMarkdownExportRequest {
                    page_id: page_id.clone(),
                    include_page_title: Some(false),
                    include_comments: Some(false),
                    include_resolved_comments: Some(false),
                },
            )
            .await
            .unwrap();
            let exported = serde_json::to_value(exported).unwrap();
            assert_eq!(exported["markdown"].as_str().unwrap().trim(), markdown);
            let exported_html = transfers::html_export::export_page(
                &pool,
                NoteHtmlExportRequest {
                    page_id,
                    include_page_tree: Some(false),
                    include_comments: Some(false),
                    include_resolved_comments: Some(false),
                    include_assets: Some(false),
                    include_database_views: Some(false),
                },
            )
            .await
            .unwrap();
            let exported_html = serde_json::to_value(exported_html).unwrap();
            let files = exported_html["files"].as_array().unwrap();
            let index = files
                .iter()
                .find(|file| file["path"] == "index.html")
                .unwrap()["contents"]
                .as_str()
                .unwrap();
            for level in 1..=6 {
                assert!(index.contains(&format!("<h{level}>Level {level}</h{level}>")));
            }
        }
    });
}

#[test]
fn markdown_import_skips_tables_wider_than_notes_supports_without_padding_rows() {
    let markdown = format!(
        "{}|\n|---|\n{}After\n",
        "|a".repeat(250_000),
        "|\n".repeat(200_000)
    );
    let started = std::time::Instant::now();
    let plan = transfers::markdown_import::parse_markdown(&markdown, 1_000).unwrap();
    assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    assert!(plan.blocks.len() <= 2, "{}", plan.blocks.len());
    assert_eq!(plan.blocks[0].block_type, "unsupported");
    let diagnostics = serde_json::to_value(&plan.diagnostics).unwrap();
    assert!(
        diagnostics
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "table_too_wide")
    );
}

#[test]
fn markdown_import_stops_table_rows_at_the_block_budget() {
    let header = format!("{}|\n", "| h ".repeat(100));
    let delimiter = format!("{}|\n", "|---".repeat(100));
    let markdown = format!("{header}{delimiter}{}", "|\n".repeat(400_000));
    let started = std::time::Instant::now();
    let error = transfers::markdown_import::parse_markdown(&markdown, 1_000)
        .err()
        .expect("rows beyond the block budget must be rejected");
    assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    assert!(error.contains("up to 1000 blocks"), "{error}");
}

#[test]
fn markdown_import_splits_oversized_text_and_keeps_large_unsupported_source_as_text() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let long = "word \u{e9}".repeat(crate::block_storage::MAX_BLOCK_TEXT_BYTES / 5);
        let html = format!(
            "<div>{}</div>",
            "y".repeat(crate::block_storage::MAX_BLOCK_ATTRS_BYTES)
        );
        let result = transfers::markdown_import::import_page(
            &pool,
            NoteMarkdownImportRequest {
                parent: workspace_parent(),
                title: Some("Long".to_string()),
                source_name: None,
                after_block_id: None,
                markdown: format!("{long}\n\n{html}\n"),
            },
        )
        .await
        .unwrap();
        let page_id = serde_json::to_value(&result).unwrap()["page"]["page"]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT type, plain_text, length(CAST(text AS BLOB)), length(CAST(attrs AS BLOB))
             FROM notes_blocks WHERE page_id = ? ORDER BY order_key, id",
        )
        .bind(&page_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(rows.len() > 2, "{}", rows.len());
        assert!(
            rows.iter()
                .all(|(block_type, ..)| block_type == "paragraph")
        );
        for (_, _, text_bytes, attrs_bytes) in &rows {
            assert!(*text_bytes as usize <= crate::block_storage::MAX_BLOCK_TEXT_BYTES);
            assert!(*attrs_bytes as usize <= crate::block_storage::MAX_BLOCK_ATTRS_BYTES);
        }
        let (html_row, text_rows) = rows.split_last().unwrap();
        let joined: String = text_rows
            .iter()
            .map(|(_, plain, ..)| plain.as_str())
            .collect();
        assert_eq!(joined.trim_end(), long.trim_end());
        assert_eq!(html_row.1, html);
    });
}

#[test]
fn split_import_blocks_keep_text_before_children_and_give_continuations_neutral_attributes() {
    use crate::transfers::import_writer::{ImportBlock, split_oversized_import_blocks};
    let long = "word ".repeat(crate::block_storage::MAX_BLOCK_TEXT_BYTES / 3);
    let mut payload = writes::default_text_payload(&long);
    payload["checked"] = serde_json::Value::Bool(true);
    payload["color"] = serde_json::Value::String("blue".to_string());
    let child = ImportBlock::new(
        "paragraph",
        writes::default_text_payload("Child"),
        None,
        Vec::new(),
    );
    let todo = ImportBlock::new(
        "to_do",
        payload,
        Some("source-todo".to_string()),
        vec![child],
    );

    let split = split_oversized_import_blocks(vec![todo]).unwrap();

    assert!(split.len() > 1, "{}", split.len());
    let (first, continuations) = split.split_first().unwrap();
    assert_eq!(first.source_object_id.as_deref(), Some("source-todo"));
    assert_eq!(first.payload["checked"], true);
    assert!(first.children.is_empty());
    for (index, continuation) in continuations.iter().enumerate() {
        assert_eq!(continuation.block_type, "to_do");
        assert_eq!(continuation.source_object_id, None);
        assert_eq!(continuation.payload["checked"], false);
        assert_eq!(continuation.payload["color"], "blue");
        crate::validation::validate_block_payload("to_do", &continuation.payload).unwrap();
        let holds_children = index + 1 == continuations.len();
        assert_eq!(!continuation.children.is_empty(), holds_children);
    }
    let joined: String = split
        .iter()
        .flat_map(|block| block.payload["rich_text"].as_array().unwrap())
        .map(|item| item["text"]["content"].as_str().unwrap())
        .collect();
    assert_eq!(joined, long);
}

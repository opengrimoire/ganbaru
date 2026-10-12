use super::helpers::*;

#[test]
fn html_import_creates_canonical_blocks_rich_text_tables_media_and_toggles() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, sort_order)
             VALUES ('project-group-a', 'Import destinations', 'lucide:folder', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, sort_order, status)
             VALUES ('project-a', 'project-group-a', 'Project A', 'lucide:folder', 0, 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let result = transfers::html_import::import_page(
            &pool,
            NoteHtmlImportRequest {
                parent: workspace_parent(),
                title: None,
                source_name: Some("notion-export.html".to_string()),
                after_block_id: None,
                keep_external_file_references: Some(true),
                project_id: Some("project-a".to_string()),
                html: r#"<!doctype html>
<html>
<head><title>Imported workspace</title></head>
<body>
  <h1>Overview</h1>
  <p><strong>Bold</strong> <em>italic</em> <u>under</u> <s>gone</s> <code>code</code> <a href="https://example.com/docs">docs</a></p>
  <ul>
    <li><input type="checkbox" checked>Done</li>
    <li>Bullet</li>
  </ul>
  <ol><li>First</li></ol>
  <blockquote>Quote <br>line</blockquote>
  <pre><code>let value = 1;</code></pre>
  <hr>
  <aside>Remember this</aside>
  <details open><summary>Toggle title</summary><p>Toggle body</p></details>
  <img src="https://example.com/diagram.png" alt="Diagram">
  <table>
    <tr><th>Name</th><th>Status</th></tr>
    <tr><td>Task</td><td>Done</td></tr>
  </table>
</body>
</html>"#
                    .to_string(),
            },
        )
        .await
        .unwrap();

        let result_json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            result_json["page"]["page"]["properties"]["title"]["title"][0]["plain_text"],
            "Imported workspace"
        );
        assert_eq!(
            result_json["page"]["page"]["properties"]["__ganbaru_project_id"],
            "project-a"
        );
        assert_eq!(result_json["imported_block_count"], 15);

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
                "callout",
                "toggle",
                "image",
                "table"
            ]
        );
        assert_eq!(
            blocks[1]["paragraph"]["rich_text"][0]["annotations"]["bold"],
            true
        );
        assert!(
            blocks[1]["paragraph"]["rich_text"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item["text"]["link"]["url"] == "https://example.com/docs")
        );
        assert_eq!(blocks[2]["to_do"]["checked"], true);
        assert_eq!(blocks[9]["toggle"]["ganbaru_open"], true);
        assert_eq!(
            blocks[10]["image"]["external"]["url"],
            "https://example.com/diagram.png"
        );

        let toggle_id = blocks[9]["id"].as_str().unwrap();
        let toggle_children = reads::block_children(&pool, toggle_id, None, Some(10))
            .await
            .unwrap();
        let toggle_children_json = serde_json::to_value(toggle_children).unwrap();
        assert_eq!(
            toggle_children_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Toggle body"
        );

        let table_id = blocks[11]["id"].as_str().unwrap();
        let rows = reads::block_children(&pool, table_id, None, Some(10))
            .await
            .unwrap();
        let rows_json = serde_json::to_value(rows).unwrap();
        assert_eq!(rows_json["results"].as_array().unwrap().len(), 2);
        assert_eq!(
            rows_json["results"][0]["table_row"]["cells"][0][0]["plain_text"],
            "Name"
        );

        let stored_page_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_pages
             WHERE title = 'Imported workspace'
               AND source_provider = 'html'
               AND source_object_id = 'notion-export.html'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored_page_count, 1);
    });
}

#[test]
fn html_import_sanitizes_unsafe_markup_and_requires_media_policy() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let result = transfers::html_import::import_page(
            &pool,
            NoteHtmlImportRequest {
                parent: workspace_parent(),
                title: Some("Unsafe HTML".to_string()),
                source_name: None,
                after_block_id: None,
                keep_external_file_references: Some(false),
                project_id: None,
                html: r#"<h6>Small heading</h6>
<p onclick="alert(1)">Text <a href="javascript:alert(1)">bad</a></p>
<script>alert("x")</script>
<iframe src="https://example.com/embed"></iframe>
<img src="images/local.png" alt="Local">
<img src="https://example.com/remote.png" alt="Remote">
<custom-widget>Unsupported</custom-widget>"#
                    .to_string(),
            },
        )
        .await
        .unwrap();

        let result_json = serde_json::to_value(result).unwrap();
        let diagnostics = result_json["diagnostics"].as_array().unwrap();
        for code in [
            "html_unsafe_attribute_removed",
            "html_unsafe_markup_removed",
            "html_media_reference_skipped",
            "html_element_unsupported",
            "html_markup_sanitized",
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == code),
                "missing diagnostic {code}"
            );
        }

        let serialized = serde_json::to_string(&result_json).unwrap();
        assert!(!serialized.contains("alert"));
        let blocks = result_json["page"]["blocks"]["results"].as_array().unwrap();
        assert!(blocks.iter().any(|block| block["type"] == "unsupported"
            && block["unsupported"]["source_type"] == "html_import"));
        let imported_file_assets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets WHERE source_type = 'imported'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(imported_file_assets, 0);
    });
}

use super::helpers::*;

#[test]
fn html_import_creates_canonical_blocks_rich_text_tables_media_and_toggles() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO project_groups (id, name, icon, order_key)
             VALUES ('project-group-a', 'Import destinations', 'lucide:folder', 'a1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO projects (id, group_id, name, icon, order_key, status)
             VALUES ('project-a', 'project-group-a', 'Project A', 'lucide:folder', 'a1', 'active')",
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
        assert_eq!(result_json["page"]["page"]["project_id"], "project-a");
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

const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;
const TEST_BLOCK_BUDGET: usize = 10_000;

fn on_worker_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(WORKER_STACK_BYTES)
        .spawn(work)
        .unwrap()
        .join()
        .expect("HTML import must not overflow a worker stack")
}

fn nested_html(open: &str, close: &str, depth: usize) -> String {
    format!("{}text{}", open.repeat(depth), close.repeat(depth))
}

fn html_request(html: String) -> NoteHtmlImportRequest {
    NoteHtmlImportRequest {
        parent: workspace_parent(),
        title: None,
        source_name: Some("nested.html".to_string()),
        after_block_id: None,
        keep_external_file_references: Some(false),
        project_id: None,
        html,
    }
}

#[test]
fn html_import_rejects_nesting_beyond_the_depth_limit_without_overflowing() {
    for open in ["<div>", "<b>"] {
        let error = on_worker_stack(move || {
            transfers::html_import::parse_html(&open.repeat(20_000), false, TEST_BLOCK_BUDGET)
                .err()
                .expect("deep nesting must be rejected")
        });
        assert!(error.contains("nesting depth limit"), "{error}");
    }
    let error = on_worker_stack(|| {
        crate::test_block_on(async {
            let pool = migrated_memory_pool().await;
            transfers::html_import::import_page(&pool, html_request("<div>".repeat(20_000)))
                .await
                .err()
                .expect("deep nesting must be rejected")
        })
    });
    assert!(error.contains("nesting depth limit"), "{error}");
}

#[test]
fn html_import_accepts_nesting_at_the_depth_limit_on_a_worker_stack() {
    let depth = transfers::html_import::syntax::MAX_HTML_IMPORT_DEPTH;
    for (open, close) in [
        ("<div>", "</div>"),
        ("<b>", "</b>"),
        ("<ul><li>", "</li></ul>"),
        ("<blockquote>", "</blockquote>"),
    ] {
        let levels = depth / open.matches('<').count();
        let imported = on_worker_stack(move || {
            crate::test_block_on(async {
                let pool = migrated_memory_pool().await;
                transfers::html_import::import_page(
                    &pool,
                    html_request(nested_html(open, close, levels)),
                )
                .await
            })
        });
        let imported = imported.unwrap_or_else(|error| panic!("{open}: {error}"));
        let value = serde_json::to_value(imported).unwrap();
        assert!(value["imported_block_count"].as_i64().unwrap() >= 1);
    }
}

#[test]
fn html_import_rejects_deep_raw_markup_before_sanitizing() {
    for html in [
        "<div>".repeat(200_000),
        "<div/>".repeat(150_000),
        "<div></b>".repeat(100_000),
        "<blockquote><span>".repeat(100_000),
    ] {
        let started = std::time::Instant::now();
        let error = transfers::html_import::parse_html(&html, false, TEST_BLOCK_BUDGET)
            .err()
            .expect("deep raw markup must be rejected");
        assert!(error.contains("nesting depth limit"), "{error}");
        assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    }
}

#[test]
fn html_import_depth_check_accepts_wide_and_implicitly_closed_markup() {
    let svg_paths = "<path d=\"M0 0\"/>".repeat(2_000);
    let script_markup = "'<div>' + ".repeat(2_000);
    for html in [
        "<p>Paragraph".repeat(2_000),
        format!("<ul>{}</ul>", "<li>Item".repeat(2_000)),
        format!("<table>{}</table>", "<tr><td>Cell".repeat(2_000)),
        "<div>Sibling</div>".repeat(2_000),
        format!("<svg>{svg_paths}</svg><p>After</p>"),
        format!("<script>{script_markup}</script><p>After</p>"),
        format!("<!-- {} --><p>After</p>", "<div>".repeat(2_000)),
    ] {
        transfers::html_import::parse_html(&html, false, TEST_BLOCK_BUDGET)
            .unwrap_or_else(|error| panic!("{error}: {}", &html[..60]));
    }
}

#[test]
fn html_import_skips_tables_wider_than_notes_supports_without_padding_rows() {
    let html = format!(
        "<table><tr>{}</tr>{}</table><p>After</p>",
        "<td>".repeat(100_000),
        "<tr></tr>".repeat(50_000)
    );
    let started = std::time::Instant::now();
    let plan = transfers::html_import::parse_html(&html, false, 1_000).unwrap();
    assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    assert_eq!(plan.blocks.len(), 2);
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
fn html_import_stops_table_rows_at_the_block_budget() {
    let row = format!("<tr>{}</tr>", "<td>x</td>".repeat(100));
    let html = format!("<table>{}</table>", row.repeat(1_001));
    let error = transfers::html_import::parse_html(&html, false, 1_000)
        .err()
        .expect("rows beyond the block budget must be rejected");
    assert!(error.contains("up to 1000 blocks"), "{error}");
}

#[test]
fn html_import_decodes_adversarial_entity_runs_in_linear_time() {
    let title = format!("<title>{};</title>", "&".repeat(1_000_000));
    let body = format!("<p>{};</p>", "&".repeat(1_000_000));
    let started = std::time::Instant::now();
    let titled = transfers::html_import::parse_html(&title, false, 1_000).unwrap();
    let text = transfers::html_import::parse_html(&body, false, 1_000).unwrap();
    assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    assert_eq!(titled.title.as_deref().map(str::len), Some(240));
    assert_eq!(text.blocks.len(), 1);
}

#[test]
fn html_import_reports_many_unsupported_tags_with_correct_lines() {
    let html = (0..200_000)
        .map(|index| format!("<x{index:x}></x{index:x}>\n"))
        .collect::<String>();
    let started = std::time::Instant::now();
    let plan = transfers::html_import::parse_html(&html, false, 1_000).unwrap();
    assert!(started.elapsed() < ADVERSARIAL_PARSE_LIMIT);
    let diagnostics = serde_json::to_value(&plan.diagnostics).unwrap();
    let last = diagnostics
        .as_array()
        .unwrap()
        .iter()
        .rfind(|diagnostic| diagnostic["code"] == "html_element_unsupported")
        .unwrap()
        .clone();
    assert_eq!(last["line"], 200_000);
}

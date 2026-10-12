use super::helpers::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn notion_export_import_reconstructs_pages_database_rows_links_and_provenance() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        fs::create_dir_all(root.join("Tasks")).unwrap();
        fs::write(
            root.join("index.html"),
            "<html><body>Workspace export</body></html>",
        )
        .unwrap();
        fs::write(
            root.join("Project Hub 11111111111111111111111111111111.md"),
            concat!(
                "# Project Hub\n\n",
                "See [Task A](Tasks/Task%20A%2022222222222222222222222222222222.md).\n\n",
                "![Local](assets/local.png)\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("Standalone.html"),
            concat!(
                "<html><head><title>Standalone</title></head><body>",
                "<p><a href=\"Project%20Hub%2011111111111111111111111111111111.md\">Hub</a></p>",
                "</body></html>",
            ),
        )
        .unwrap();
        fs::write(
            root.join("Tasks").join("Tasks.csv"),
            concat!(
                "Name,Status,Notes\n",
                "Task A,Doing,\"Design, docs\"\n",
                "Task B,Done,Complete\n",
            ),
        )
        .unwrap();
        fs::write(
            root.join("Tasks")
                .join("Task A 22222222222222222222222222222222.md"),
            "# Task A\n\nBody detail from row page.\n",
        )
        .unwrap();

        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
                export_root_path: root.to_string_lossy().to_string(),
                source_workspace_id: Some("workspace-a".to_string()),
                keep_external_file_references: Some(false),
                copy_local_file_references: Some(false),
                import_markdown: Some(true),
                import_html: Some(true),
                import_csv: Some(true),
                project_id: None,
            },
        )
        .await
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["object"], "notes_notion_export_import");
        assert_eq!(value["imported_data_source_count"], 1);
        assert_eq!(value["imported_page_count"], 5);
        assert_eq!(value["imported_file_count"], 0);
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "sitemap_skipped"
                    && diagnostic["source_path"] == "index.html")
        );

        let serialized = serde_json::to_string(&value).unwrap();
        assert!(serialized.contains("http://localhost:1420/?view=notes#notes?title=Task+A"));
        assert!(serialized.contains("http://localhost:1420/?view=notes#notes?title=Project+Hub"));

        let notion_export_pages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_pages
             WHERE source_provider = 'notion_export'
               AND source_workspace_id = 'workspace-a'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(notion_export_pages, 5);

        let data_source_id = value["imported_data_sources"][0]["local_id"]
            .as_str()
            .unwrap();
        let table =
            data_sources::layouts::table::data_source_table_view(&pool, data_source_id, None, None)
                .await
                .unwrap();
        let table_json = serde_json::to_value(table).unwrap();
        let rows = table_json["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        let task_a = rows
            .iter()
            .find(|row| row["properties"]["Name"]["title"][0]["plain_text"] == "Task A")
            .unwrap();
        assert_eq!(
            task_a["properties"]["Notes"]["rich_text"][0]["plain_text"],
            "Design, docs"
        );
        assert_eq!(task_a["source_provider"], "notion_export");

        let task_a_page_id: String = sqlx::query_scalar(
            "SELECT id
             FROM notes_pages
             WHERE parent_data_source_id = ?
               AND title = 'Task A'",
        )
        .bind(data_source_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let row_body_blocks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_blocks
             WHERE page_id = ?
               AND source_provider = 'notion_export'
               AND plain_text = 'Body detail from row page.'",
        )
        .bind(task_a_page_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row_body_blocks, 1);

        fs::remove_dir_all(root).unwrap();
    });
}

fn unique_export_folder() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "ganbaru-notion-export-import-test-{}-{nanos}",
        std::process::id()
    ))
}

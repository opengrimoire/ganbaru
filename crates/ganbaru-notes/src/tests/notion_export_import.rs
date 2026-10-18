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
            &root,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
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

#[cfg(unix)]
fn export_request() -> NoteNotionExportImportRequest {
    NoteNotionExportImportRequest {
        parent: workspace_parent(),
        source_workspace_id: None,
        keep_external_file_references: Some(false),
        copy_local_file_references: Some(false),
        import_markdown: Some(true),
        import_html: Some(true),
        import_csv: Some(true),
        project_id: None,
    }
}

#[cfg(unix)]
#[test]
fn notion_export_import_skips_symbolic_links_to_outside_files() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let base = unique_export_folder();
        let root = base.join("export");
        fs::create_dir_all(root.join("Workspace")).unwrap();
        let secret = base.join("id_ed25519");
        fs::write(&secret, "PRIVATE-KEY-MATERIAL").unwrap();
        std::os::unix::fs::symlink(&secret, root.join("Workspace").join("Credentials 1234.md"))
            .unwrap();
        std::os::unix::fs::symlink(&base, root.join("Workspace").join("Outside")).unwrap();
        fs::write(root.join("Page.md"), "# Page\n\nVisible text.\n").unwrap();

        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            &root,
            export_request(),
        )
        .await
        .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["imported_page_count"], 1);
        let skipped = value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|diagnostic| diagnostic["code"] == "symlink_skipped")
            .count();
        assert_eq!(skipped, 2);
        let leaked: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_blocks WHERE plain_text LIKE '%PRIVATE-KEY-MATERIAL%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(leaked, 0);
        assert!(
            !serde_json::to_string(&value)
                .unwrap()
                .contains("PRIVATE-KEY-MATERIAL")
        );

        fs::remove_dir_all(base).unwrap();
    });
}

#[cfg(unix)]
#[test]
fn notion_export_import_does_not_traverse_directory_link_cycles() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        fs::create_dir_all(&root).unwrap();
        for name in ["a", "b", "c"] {
            std::os::unix::fs::symlink(".", root.join(name)).unwrap();
        }
        fs::write(root.join("Page.md"), "# Page\n").unwrap();

        let started = std::time::Instant::now();
        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            &root,
            export_request(),
        )
        .await
        .unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["imported_page_count"], 1);

        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn notion_export_request_rejects_a_webview_supplied_folder_path() {
    let error = serde_json::from_value::<NoteNotionExportImportRequest>(serde_json::json!({
        "parent": { "type": "workspace", "workspace": true },
        "export_root_path": "/home/user",
    }))
    .err()
    .expect("export_root_path must not deserialize");
    assert!(error.to_string().contains("export_root_path"));
}

#[test]
fn notion_export_root_rejects_home_its_ancestors_root_and_vault_folders() {
    let base = unique_export_folder();
    let home = base.join("home");
    let vault = home.join("Documents").join("Ganbaru");
    let export = home.join("Downloads").join("Notion export");
    fs::create_dir_all(vault.join("notes")).unwrap();
    fs::create_dir_all(&export).unwrap();
    let validate = |selected: &std::path::Path| {
        transfers::notion_export_import::validated_export_root(selected, Some(&home), Some(&vault))
    };

    assert!(validate(&home).unwrap_err().contains("home folder"));
    assert!(validate(&base).unwrap_err().contains("home folder"));
    let filesystem_root = home.ancestors().last().unwrap();
    assert!(validate(filesystem_root).is_err());
    assert!(validate(&vault).unwrap_err().contains("inside the vault"));
    assert!(
        validate(&vault.join("notes"))
            .unwrap_err()
            .contains("inside the vault")
    );
    assert!(validate(std::path::Path::new("relative/export")).is_err());
    assert_eq!(
        validate(&export).unwrap(),
        fs::canonicalize(&export).unwrap()
    );
    assert!(validate(&home.join("Downloads")).is_ok());

    fs::remove_dir_all(base).unwrap();
}

#[test]
fn notion_export_import_skips_an_unparseable_document_and_imports_the_rest() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Deep.html"), "<div>".repeat(20_000)).unwrap();
        fs::write(root.join("Page.md"), "# Page\n\nVisible text.\n").unwrap();

        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            &root,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
                source_workspace_id: None,
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
        assert_eq!(value["imported_page_count"], 1);
        assert!(
            value["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| diagnostic["code"] == "file_skipped"
                    && diagnostic["source_path"] == "Deep.html"
                    && diagnostic["message"]
                        .as_str()
                        .unwrap()
                        .contains("nesting depth limit"))
        );

        fs::remove_dir_all(root).unwrap();
    });
}

fn export_diagnostics(result: crate::models::NoteNotionExportImportDto) -> Vec<serde_json::Value> {
    serde_json::to_value(result).unwrap()["diagnostics"]
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn notion_export_import_rewrites_adversarial_references_in_linear_time() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("Brackets.md"),
            format!("{}](x)", "[".repeat(999_000)),
        )
        .unwrap();
        fs::write(
            root.join("Images.md"),
            format!("{}](x)", "![".repeat(499_000)),
        )
        .unwrap();
        fs::write(
            root.join("Sources.html"),
            format!("<p {}>Text</p>", "src=\"\"".repeat(160_000)),
        )
        .unwrap();
        fs::write(
            root.join("Media.html"),
            format!("<img {}>", "src=\"\" ".repeat(140_000)),
        )
        .unwrap();

        let started = std::time::Instant::now();
        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            &root,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
                source_workspace_id: None,
                keep_external_file_references: Some(false),
                copy_local_file_references: Some(false),
                import_markdown: Some(true),
                import_html: Some(true),
                import_csv: Some(true),
                project_id: None,
            },
        )
        .await;
        let elapsed = started.elapsed();
        fs::remove_dir_all(root).unwrap();
        result.unwrap();
        assert!(elapsed < ADVERSARIAL_PARSE_LIMIT, "took {elapsed:?}");
    });
}

#[test]
fn notion_export_import_parses_a_shared_row_body_once() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        fs::create_dir_all(&root).unwrap();
        let rows = "A,Doing\n".repeat(1_000);
        fs::write(root.join("Tasks.csv"), format!("Name,Status\n{rows}")).unwrap();
        fs::write(
            root.join("A.md"),
            "# A\n\nShared body.\n\n[ref]: https://example.com\n",
        )
        .unwrap();

        let result = transfers::notion_export_import::import_folder_without_file_copy(
            &pool,
            &root,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
                source_workspace_id: None,
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
        fs::remove_dir_all(root).unwrap();
        let summary = serde_json::to_value(&result).unwrap();
        // The database page plus one page per row.
        assert_eq!(summary["imported_page_count"], 1_001);
        let parse_notes = export_diagnostics(result)
            .into_iter()
            .filter(|diagnostic| {
                diagnostic["source_path"] == "A.md"
                    && diagnostic["code"]
                        .as_str()
                        .unwrap()
                        .ends_with("reference_definition_unsupported")
            })
            .count();
        assert_eq!(parse_notes, 1);
        let body_blocks: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_blocks WHERE plain_text = 'Shared body.'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(body_blocks, 1_000);
    });
}

#[test]
fn notion_export_import_copies_a_repeatedly_referenced_asset_once() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let root = unique_export_folder();
        let vault = root.with_extension("vault");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&vault).unwrap();
        fs::write(
            root.join("a.png"),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0],
        )
        .unwrap();
        fs::write(root.join("Gallery.md"), "![](a.png)\n".repeat(90_000)).unwrap();

        let result = transfers::notion_export_import::import_folder(
            &pool,
            &vault,
            &root,
            None,
            NoteNotionExportImportRequest {
                parent: workspace_parent(),
                source_workspace_id: None,
                keep_external_file_references: Some(false),
                copy_local_file_references: Some(true),
                import_markdown: Some(true),
                import_html: Some(true),
                import_csv: Some(true),
                project_id: None,
            },
        )
        .await
        .unwrap();
        let copied_files = fs::read_dir(vault.join("assets").join("notes").join("files"))
            .unwrap()
            .count();
        fs::remove_dir_all(&root).unwrap();
        fs::remove_dir_all(&vault).unwrap();
        let copies = export_diagnostics(result)
            .into_iter()
            .filter(|diagnostic| diagnostic["code"] == "import_reference_copied")
            .count();
        assert_eq!(copies, 1);
        assert_eq!(copied_files, 1);
    });
}

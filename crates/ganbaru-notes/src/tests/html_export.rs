use super::helpers::*;

const LOCAL_ASSET_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const UNSAFE_MEDIA_CODE: &str = "html_export_unsafe_media_skipped";
const BLOCK_G: &str = "12121212-1212-4212-8212-121212121212";

/// Creates one paragraph per id after `BLOCK_A`, to be overwritten as replicated rows.
async fn append_placeholder_blocks(pool: &SqlitePool, ids: &[&str]) {
    writes::append_block_children(
        pool,
        NoteAppendBlockChildren {
            parent: page_parent(PAGE_A),
            after: Some(BLOCK_A.to_string()),
            children: ids
                .iter()
                .map(|id| block(id, "paragraph", paragraph_payload("placeholder")))
                .collect(),
        },
    )
    .await
    .unwrap();
}

/// Stores a media row directly, as apply does, which skips the checks of the local write path.
async fn store_replicated_media(
    pool: &SqlitePool,
    id: &str,
    block_type: &str,
    payload: serde_json::Value,
) {
    let payload = payload.to_string();
    sqlx::query(
        "UPDATE notes_blocks
         SET type = ?, attrs = json_remove(?, '$.caption'), text = json_object('caption', json_extract(?, '$.caption'))
         WHERE id = ?",
    )
    .bind(block_type)
    .bind(&payload)
    .bind(&payload)
    .bind(id)
    .execute(pool)
    .await
    .unwrap();
}

/// Exports the page and returns its `index.html` with the export diagnostics.
async fn export_index(pool: &SqlitePool, include_assets: bool) -> (String, Vec<serde_json::Value>) {
    let export = serde_json::to_value(
        transfers::html_export::export_page(
            pool,
            NoteHtmlExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_tree: Some(false),
                include_comments: Some(false),
                include_resolved_comments: Some(false),
                include_assets: Some(include_assets),
                include_database_views: Some(false),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let index = export["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "index.html")
        .and_then(|file| file["contents"].as_str())
        .unwrap()
        .to_string();
    (index, export["diagnostics"].as_array().unwrap().clone())
}

fn has_diagnostic(diagnostics: &[serde_json::Value], code: &str, block_id: &str) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic["code"] == code && diagnostic["block_id"] == block_id)
}

#[test]
fn html_export_skips_javascript_media_url() {
    crate::test_block_on(async {
        let hostile = [
            (BLOCK_B, "pdf", "javascript:alert(1)"),
            (BLOCK_C, "file", " JavaScript:alert(2)"),
            (BLOCK_D, "image", "data:text/html,alert(3)"),
            (BLOCK_E, "video", "vbscript:alert(4)"),
            (BLOCK_F, "audio", "file:///etc/passwd"),
            (BLOCK_G, "file", "java\tscript:alert(5)"),
        ];
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let ids = hostile.iter().map(|row| row.0).collect::<Vec<_>>();
        append_placeholder_blocks(&pool, &ids).await;
        for (id, block_type, url) in hostile {
            let label = format!("Label {id}");
            store_replicated_media(&pool, id, block_type, media_payload(url, &label, None)).await;
        }

        let (index, diagnostics) = export_index(&pool, false).await;

        for (id, block_type, url) in hostile {
            assert!(
                !index.contains(url.trim()),
                "{block_type} wrote {url} into the export"
            );
            assert!(
                index.contains(&format!("<p>Label {id}</p>")),
                "{block_type} keeps its label as plain text"
            );
            assert!(
                has_diagnostic(&diagnostics, UNSAFE_MEDIA_CODE, id),
                "{block_type} reports the skipped URL"
            );
        }
    });
}

#[test]
fn html_export_skips_a_file_upload_reference() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        append_placeholder_blocks(&pool, &[BLOCK_B]).await;
        store_replicated_media(
            &pool,
            BLOCK_B,
            "file",
            json!({
                "caption": [rich_text("Upload")],
                "type": "file_upload",
                "file_upload": { "id": "upload-1" }
            }),
        )
        .await;

        let (index, diagnostics) = export_index(&pool, false).await;

        assert!(!index.contains("file-upload:"));
        assert!(index.contains("<p>Upload</p>"));
        assert!(has_diagnostic(&diagnostics, UNSAFE_MEDIA_CODE, BLOCK_B));
    });
}

#[test]
fn html_export_reports_an_empty_media_url_as_missing() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        append_placeholder_blocks(&pool, &[BLOCK_B]).await;
        store_replicated_media(&pool, BLOCK_B, "image", media_payload(" ", "Pending", None)).await;

        let (index, diagnostics) = export_index(&pool, false).await;

        assert!(!index.contains("<img"));
        assert!(index.contains("<p>Pending</p>"));
        assert!(has_diagnostic(
            &diagnostics,
            "html_export_media_missing_reference",
            BLOCK_B
        ));
        assert!(!has_diagnostic(&diagnostics, UNSAFE_MEDIA_CODE, BLOCK_B));
    });
}

#[test]
fn html_export_keeps_asset_and_web_media_links() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        append_placeholder_blocks(&pool, &[BLOCK_B, BLOCK_C, BLOCK_D, BLOCK_E, BLOCK_F]).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_F.to_string()),
                children: vec![block(
                    BLOCK_G,
                    "pdf",
                    local_media_payload(
                        "notes/files/local.pdf",
                        "application/pdf",
                        12,
                        LOCAL_ASSET_SHA,
                        "Local PDF",
                        Some("local.pdf"),
                    ),
                )],
            },
        )
        .await
        .unwrap();
        for (id, block_type, url) in [
            (BLOCK_B, "image", "https://example.com/a.png"),
            (BLOCK_C, "video", "https://example.com/a.mp4"),
            (BLOCK_D, "audio", "http://example.com/a.mp3"),
            (BLOCK_E, "pdf", "https://example.com/a.pdf"),
            (BLOCK_F, "file", "HTTPS://example.com/a.txt"),
        ] {
            store_replicated_media(&pool, id, block_type, media_payload(url, "Web", None)).await;
        }

        let (index, diagnostics) = export_index(&pool, true).await;

        assert!(index.contains("<img src=\"https://example.com/a.png\""));
        assert!(index.contains("<video controls src=\"https://example.com/a.mp4\""));
        assert!(index.contains("<audio controls src=\"http://example.com/a.mp3\""));
        assert!(index.contains("<a href=\"https://example.com/a.pdf\""));
        assert!(index.contains("<a href=\"HTTPS://example.com/a.txt\""));
        assert!(index.contains("<a href=\"assets/notes/files/local.pdf\""));
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic["code"] != UNSAFE_MEDIA_CODE)
        );
    });
}

#[test]
fn html_export_renders_an_excluded_asset_as_its_label() {
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
                    "file",
                    local_media_payload(
                        "notes/files/local.txt",
                        "text/plain",
                        12,
                        LOCAL_ASSET_SHA,
                        "Local file",
                        Some("local.txt"),
                    ),
                )],
            },
        )
        .await
        .unwrap();

        let (index, diagnostics) = export_index(&pool, false).await;

        assert!(!index.contains("ganbaru-asset:"));
        assert!(index.contains("<p>Local file</p>"));
        assert!(has_diagnostic(
            &diagnostics,
            "html_export_local_asset_not_included",
            BLOCK_B
        ));
        assert!(!has_diagnostic(&diagnostics, UNSAFE_MEDIA_CODE, BLOCK_B));
    });
}

#[test]
fn html_export_template_sets_script_policy() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let (index, _) = export_index(&pool, false).await;

        let policy = index
            .find(
                "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; object-src 'none'; base-uri 'none'\">",
            )
            .expect("the page carries the script policy");
        assert!(
            policy < index.find("<title>").unwrap(),
            "the policy precedes everything it governs"
        );
    });
}

#[test]
fn html_export_writes_page_tree_assets_comments_and_database_metadata() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Archive Root".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Child Page".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                project_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: Some(BLOCK_A.to_string()),
                properties: None,
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
                        rich_text("Open "),
                        page_mention(PAGE_B, "Child Page"),
                        rich_text(" or "),
                        linked_rich_text("docs", "https://example.com/docs")
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
                after: Some(PAGE_B.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "file",
                    local_media_payload(
                        "notes/files/local.txt",
                        "text/plain",
                        12,
                        LOCAL_ASSET_SHA,
                        "Local file",
                        Some("local.txt"),
                    ),
                )],
            },
        )
        .await
        .unwrap();
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_C,
        )
        .await;
        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Archive note")],
                attachments: None,
            },
        )
        .await
        .unwrap();

        let result = transfers::html_export::export_page(
            &pool,
            NoteHtmlExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_tree: Some(true),
                include_comments: Some(true),
                include_resolved_comments: Some(false),
                include_assets: Some(true),
                include_database_views: Some(true),
            },
        )
        .await
        .unwrap();
        let result_json = serde_json::to_value(result).unwrap();
        let files = result_json["files"].as_array().unwrap();
        let index = files
            .iter()
            .find(|file| file["path"] == "index.html")
            .and_then(|file| file["contents"].as_str())
            .unwrap();

        assert_eq!(result_json["object"], "notes_html_archive_export");
        assert_eq!(result_json["exported_page_count"], 2);
        assert_eq!(result_json["exported_comment_count"], 1);
        assert_eq!(result_json["exported_database_view_count"], 1);
        assert!(index.contains("<title>Archive Root</title>"));
        assert!(index.contains("pages/child-page-22222222/index.html"));
        assert!(index.contains("https://example.com/docs"));
        assert!(index.contains("Local file"));
        assert!(index.contains("Database manifest"));
        assert!(index.contains("Archive note"));
        assert!(
            files
                .iter()
                .any(|file| file["path"] == "databases/tasks-80808080.json")
        );
        assert!(
            files
                .iter()
                .any(|file| file["path"] == "assets/ganbaru-notes-export.css")
        );
        assert_eq!(
            result_json["assets"][0]["archive_path"],
            "assets/notes/files/local.txt"
        );
        assert_eq!(result_json["assets"][0]["exported"], true);
        let manifest = result_json["manifest_json"].as_str().unwrap();
        assert!(manifest.contains("\"object\": \"notes_html_archive_manifest\""));
        assert!(manifest.contains("\"include_page_tree\": true"));
    });
}

#[test]
fn html_export_is_deterministic_and_warns_when_assets_are_excluded() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "file",
                    local_media_payload(
                        "notes/files/local.txt",
                        "text/plain",
                        12,
                        LOCAL_ASSET_SHA,
                        "Local file",
                        Some("local.txt"),
                    ),
                )],
            },
        )
        .await
        .unwrap();
        let request = NoteHtmlExportRequest {
            page_id: PAGE_A.to_string(),
            include_page_tree: Some(false),
            include_comments: Some(false),
            include_resolved_comments: Some(false),
            include_assets: Some(false),
            include_database_views: Some(false),
        };
        let first = serde_json::to_value(
            transfers::html_export::export_page(&pool, request)
                .await
                .unwrap(),
        )
        .unwrap();
        let second = serde_json::to_value(
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

        assert_eq!(first["manifest_json"], second["manifest_json"]);
        assert_eq!(first["exported_asset_count"], 0);
        assert!(
            first["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diagnostic| {
                    diagnostic["code"] == "html_export_local_asset_not_included"
                        && diagnostic["block_id"] == BLOCK_C
                })
        );
    });
}

#[test]
fn html_archive_reads_a_referenced_path_of_the_same_bytes_when_the_row_path_holds_no_file() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("ganbaru-notes-html-fallback-{nonce}"));
        let vault = base.join("vault");
        let files = vault.join("assets").join("notes").join("files");
        std::fs::create_dir_all(&files).unwrap();
        let bytes = b"same bytes under two names";
        let sha = assets::files::hex_hash(bytes);
        let row_path = format!("notes/files/{sha}.txt");
        let referenced_path = format!("notes/files/{sha}.csv");
        std::fs::write(files.join(format!("{sha}.csv")), bytes).unwrap();
        sqlx::query(
            "INSERT INTO notes_assets
                 (id, asset_path, kind, source_type, content_type, byte_size, sha256)
             VALUES (?1, ?2, 'file', 'local_upload', 'text/plain', ?3, ?4)",
        )
        .bind(format!("notes/files:{sha}"))
        .bind(&row_path)
        .bind(bytes.len() as i64)
        .bind(&sha)
        .execute(&pool)
        .await
        .unwrap();
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "file",
                    local_media_payload(
                        &referenced_path,
                        "text/csv",
                        bytes.len() as i64,
                        &sha,
                        "Shared file",
                        Some("shared.csv"),
                    ),
                )],
            },
        )
        .await
        .unwrap();

        let mut archive = transfers::html_export::export_archive(
            &pool,
            NoteHtmlExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_tree: Some(false),
                include_comments: Some(false),
                include_resolved_comments: Some(false),
                include_assets: Some(true),
                include_database_views: Some(false),
            },
        )
        .await
        .unwrap();
        let exported = &archive.dto.assets;
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].source_path, row_path);
        assert_eq!(
            exported[0].fallback_source_paths,
            std::slice::from_ref(&referenced_path)
        );
        assert!(exported[0].exported);
        let target = base.join("export.zip");

        transfers::html_export::archive::write_archive(&vault, &target, &mut archive).unwrap();

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
        let mut entry = zip
            .by_name(&format!("assets/{row_path}"))
            .expect("the row's archive path holds the referenced bytes");
        let mut read = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut read).unwrap();
        assert_eq!(read, bytes);
        drop(entry);
        assert!(
            archive
                .dto
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "html_export_asset_read_failed"),
            "the fallback read leaves no read failure"
        );
        let _ = std::fs::remove_dir_all(&base);
    });
}

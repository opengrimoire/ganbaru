use super::helpers::*;

const LOCAL_ASSET_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

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

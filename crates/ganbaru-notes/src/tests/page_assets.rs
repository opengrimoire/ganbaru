use super::helpers::*;
use serde_json::Value;

#[test]
fn page_icon_set_and_remove_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let page = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Value(json!({
                    "type": "emoji",
                    "emoji": "📌"
                })),
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        let page_json = serde_json::to_value(page).unwrap();
        assert_eq!(page_json["icon"]["type"], "emoji");
        assert_eq!(page_json["icon"]["emoji"], "📌");
        assert!(page_json["cover"].is_null());

        let page = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Null,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        let page_json = serde_json::to_value(page).unwrap();
        assert!(page_json["icon"].is_null());
        let stored_icon: Option<String> =
            sqlx::query_scalar("SELECT icon FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(stored_icon.is_none());
    });
}

#[test]
fn page_icon_validation_rejects_empty_emoji() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let result = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Value(json!({
                    "type": "emoji",
                    "emoji": ""
                })),
                cover: OptionalJsonValue::Unset,
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("icon.emoji must be a non-empty string".to_string())
        );
    });
}

#[test]
fn page_icon_variants_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let variants = [
            json!({
                "type": "icon",
                "icon": {
                    "name": "home",
                    "color": "blue"
                }
            }),
            json!({
                "type": "custom_emoji",
                "custom_emoji": {
                    "id": "emoji-1",
                    "name": "Focus",
                    "url": "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
                    "ganbaru_asset_path": "project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                }
            }),
            json!({
                "type": "external",
                "external": {
                    "url": "https://example.com/icon.webp"
                }
            }),
            json!({
                "type": "file",
                "file": {
                    "url": "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                    "name": "focus.png",
                    "content_type": "image/png",
                    "byte_size": 42,
                    "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                    "ganbaru_asset_path": "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                }
            }),
        ];

        for icon in variants {
            let page = writes::update_page(
                &pool,
                PAGE_A,
                NotePageUpdate {
                    title: None,
                    parent: None,
                    properties: None,
                    icon: OptionalJsonValue::Value(icon.clone()),
                    cover: OptionalJsonValue::Unset,
                },
            )
            .await
            .unwrap();
            let page_json = serde_json::to_value(page).unwrap();
            assert_eq!(page_json["icon"], icon);
        }
    });
}

#[test]
fn page_icon_validation_rejects_unsafe_external_and_local_files() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        for (icon, expected_error) in [
            (
                json!({
                    "type": "external",
                    "external": { "url": "http://example.com/icon.png" }
                }),
                "icon.external.url must be a supported HTTPS image URL",
            ),
            (
                json!({
                    "type": "external",
                    "external": { "url": "https://example.com/icon.svg" }
                }),
                "icon.external.url must be a supported HTTPS image URL",
            ),
            (
                json!({
                    "type": "custom_emoji",
                    "custom_emoji": { "name": "Missing id" }
                }),
                "icon.custom_emoji.id must be a non-empty string",
            ),
            (
                json!({
                    "type": "custom_emoji",
                    "custom_emoji": {
                        "id": "wrong-directory",
                        "url": "ganbaru-asset:notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                    }
                }),
                "icon.custom_emoji.url must stay under a managed image asset directory",
            ),
            (
                json!({
                    "type": "custom_emoji",
                    "custom_emoji": {
                        "id": "missing-asset-path",
                        "url": "ganbaru-asset:project-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                    }
                }),
                "icon.custom_emoji.url must reference the managed icon asset path",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru://assets/notes/page-icons/bad.svg",
                        "content_type": "image/svg+xml",
                        "byte_size": 42,
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "ganbaru_asset_path": "notes/page-icons/bad.svg"
                    }
                }),
                "icon.file.ganbaru_asset_path must stay under a managed image asset directory",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                        "content_type": "image/png",
                        "byte_size": 0,
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "ganbaru_asset_path": "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                    }
                }),
                "icon.file.byte_size must be positive",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-icons/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc.png",
                        "content_type": "image/png",
                        "byte_size": 42,
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "ganbaru_asset_path": "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                    }
                }),
                "icon.file.url must reference the managed icon asset path",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png"
                    }
                }),
                "icon.file.url must include managed asset metadata",
            ),
        ] {
            let result = writes::update_page(
                &pool,
                PAGE_A,
                NotePageUpdate {
                    title: None,
                    parent: None,
                    properties: None,
                    icon: OptionalJsonValue::Value(icon),
                    cover: OptionalJsonValue::Unset,
                },
            )
            .await;
            assert_eq!(result.err(), Some(expected_error.to_string()));
        }
    });
}

#[test]
fn page_cover_set_and_remove_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let page = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Value(json!({
                    "type": "external",
                    "external": {
                        "url": "https://example.com/cover.jpg"
                    }
                })),
            },
        )
        .await
        .unwrap();
        let page_json = serde_json::to_value(page).unwrap();
        assert_eq!(page_json["cover"]["type"], "external");
        assert_eq!(
            page_json["cover"]["external"]["url"],
            "https://example.com/cover.jpg"
        );

        let page = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Null,
            },
        )
        .await
        .unwrap();
        let page_json = serde_json::to_value(page).unwrap();
        assert!(page_json["cover"].is_null());
    });
}

#[test]
fn page_cover_variants_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let variants = [
            json!({"type": "design", "design": {"pattern": "studio", "color": 31}}),
            json!({"type": "external", "external": {"url": "https://example.com/cover.png"}, "focal_point": {"x": 0.25, "y": 1.0}}),
            json!({
                "type": "file",
                "file": {
                    "url": "https://example.com/imported-cover.webp",
                    "expiry_time": "2026-07-01T12:00:00.000Z"
                }
            }),
            json!({
                "type": "file_upload",
                "file_upload": {
                    "id": "55555555-5555-4555-8555-555555555555"
                }
            }),
            json!({
                "type": "file",
                "file": {
                    "url": "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
                    "name": "cover.png",
                    "content_type": "image/png",
                    "byte_size": 42,
                    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                    "ganbaru_asset_path": "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                }
            }),
        ];

        for cover in variants {
            let page = writes::update_page(
                &pool,
                PAGE_A,
                NotePageUpdate {
                    title: None,
                    parent: None,
                    properties: None,
                    icon: OptionalJsonValue::Unset,
                    cover: OptionalJsonValue::Value(cover.clone()),
                },
            )
            .await
            .unwrap();
            let page_json = serde_json::to_value(page).unwrap();
            assert_eq!(page_json["cover"], cover);
        }
    });
}

#[test]
fn page_media_asset_references_follow_local_file_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let icon_path =
            "notes/page-icons/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png";
        let cover_path = "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png";

        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Value(json!({
                    "type": "file",
                    "file": {
                        "url": format!("ganbaru-asset:{icon_path}"),
                        "name": "focus.png",
                        "content_type": "image/png",
                        "byte_size": 42,
                        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "ganbaru_asset_path": icon_path
                    }
                })),
                cover: OptionalJsonValue::Value(json!({
                    "type": "file",
                    "file": {
                        "url": format!("ganbaru-asset:{cover_path}"),
                        "name": "cover.png",
                        "content_type": "image/png",
                        "byte_size": 84,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "ganbaru_asset_path": cover_path
                    }
                })),
            },
        )
        .await
        .unwrap();

        let asset_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets WHERE kind = 'image'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(asset_count, 2);

        for (asset_path, role) in [(icon_path, "page_icon"), (cover_path, "page_cover")] {
            let reference_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*)
                 FROM notes_asset_references
                 WHERE asset_id = ? AND owner_type = 'page' AND owner_id = ? AND role = ?",
            )
            .bind(asset_path)
            .bind(PAGE_A)
            .bind(role)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(reference_count, 1);
        }

        assets::mark_managed_asset_storage_state(&pool, icon_path, true, "page icon")
            .await
            .unwrap();
        let missing_icon: (String, Option<String>) = sqlx::query_as(
            "SELECT storage_state, missing_at FROM notes_assets WHERE asset_path = ?",
        )
        .bind(icon_path)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(missing_icon.0, "missing");
        assert!(missing_icon.1.is_some());

        assets::mark_managed_asset_storage_state(&pool, icon_path, false, "page icon")
            .await
            .unwrap();
        let recovered_icon: (String, Option<String>) = sqlx::query_as(
            "SELECT storage_state, missing_at FROM notes_assets WHERE asset_path = ?",
        )
        .bind(icon_path)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(recovered_icon, ("available".to_string(), None));

        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Null,
                cover: OptionalJsonValue::Value(json!({
                    "type": "external",
                    "external": {
                        "url": "https://example.com/cover.webp"
                    }
                })),
            },
        )
        .await
        .unwrap();

        let remaining_references: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_asset_references WHERE owner_type = 'page' AND owner_id = ?",
        )
        .bind(PAGE_A)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_references, 0);

        let retained_assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_assets")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(retained_assets, 2);
    });
}

#[test]
fn page_cover_validation_rejects_unsafe_file_objects() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        for (cover, expected_error) in [
            (
                json!({
                    "type": "external",
                    "external": {
                        "url": "https://example.com/document.pdf"
                    }
                }),
                "cover.external.url must be a supported HTTPS image URL",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "https://example.com/document.pdf",
                        "expiry_time": "2026-07-01T12:00:00.000Z"
                    }
                }),
                "cover.file.url must be a supported HTTPS image URL",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-covers/bad.svg",
                        "content_type": "image/svg+xml",
                        "byte_size": 42,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "ganbaru_asset_path": "notes/page-covers/bad.svg"
                    }
                }),
                "cover.file.ganbaru_asset_path must stay under a managed image asset directory",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-covers/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.png",
                        "content_type": "image/png",
                        "byte_size": 42,
                        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "ganbaru_asset_path": "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                    }
                }),
                "cover.file.url must reference the managed cover asset path",
            ),
            (
                json!({
                    "type": "file",
                    "file": {
                        "url": "ganbaru-asset:notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
                    }
                }),
                "cover.file.url must include managed asset metadata",
            ),
        ] {
            let result = writes::update_page(
                &pool,
                PAGE_A,
                NotePageUpdate {
                    title: None,
                    parent: None,
                    properties: None,
                    icon: OptionalJsonValue::Unset,
                    cover: OptionalJsonValue::Value(cover),
                },
            )
            .await;
            assert_eq!(result.err(), Some(expected_error.to_string()));
        }
    });
}

#[test]
fn page_cover_design_and_focal_validation() {
    for pattern in [
        "contours",
        "studio",
        "botanical",
        "orbit",
        "atlas",
        "study",
        "mathematics",
        "programming",
        "finance",
        "solid",
        "gradient",
        "mosaic",
        "dots",
        "grid",
    ] {
        for color in 0..32 {
            validation::validate_page_cover_value(&json!({
                "type": "design", "design": {"pattern": pattern, "color": color}
            }))
            .unwrap();
        }
    }
    validation::validate_page_cover_value(&json!({
        "type": "design", "design": {"pattern": "contours", "color": "default"}
    }))
    .unwrap();
    for color in [json!(-1), json!(32), json!(0.5), json!("1"), Value::Null] {
        assert!(
            validation::validate_page_cover_value(&json!({
                "type": "design", "design": {"pattern": "grid", "color": color}
            }))
            .is_err()
        );
    }
    for cover in [
        json!({"type": "design", "design": {"pattern": "unknown", "color": 0}}),
        json!({"type": "design", "design": {"pattern": "glow", "color": 0}}),
        json!({"type": "design", "design": {"pattern": "grid", "color": 0}, "focal_point": {"x": 0, "y": 0}}),
        json!({"type": "design", "design": null}),
    ] {
        assert!(validation::validate_page_cover_value(&cover).is_err());
    }
    for point in [
        Value::Null,
        json!({}),
        json!({"x": -0.1, "y": 0}),
        json!({"x": 0, "y": 1.1}),
        json!({"x": "0", "y": 0}),
    ] {
        assert!(validation::validate_page_cover_value(&json!({
            "type": "external", "external": {"url": "https://example.com/image.png"}, "focal_point": point
        })).is_err());
    }
}

#[test]
fn page_cover_metadata_survives_templates_duplication_history_and_exports() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let cover = json!({"type": "design", "design": {"pattern": "dots", "color": 4}});
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: None,
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Value(cover.clone()),
            },
        )
        .await
        .unwrap();

        let duplicate = writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(duplicate).unwrap()["page"]["cover"],
            cover
        );
        templates::create_page_template_from_page(
            &pool,
            NotePageTemplateCreateFromPage {
                id: TEMPLATE_A.to_string(),
                source_page_id: PAGE_A.to_string(),
                name: "Cover template".to_string(),
            },
        )
        .await
        .unwrap();
        let applied = templates::apply_page_template(
            &pool,
            TEMPLATE_A,
            NotePageTemplateApply {
                parent: workspace_parent(),
                title: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            serde_json::to_value(applied).unwrap()["page"]["cover"],
            cover
        );

        let mut tx = pool.begin().await.unwrap();
        let snapshot = page_history::record_page_snapshot_tx(&mut tx, PAGE_A, "restore")
            .await
            .unwrap()
            .unwrap();
        tx.commit().await.unwrap();
        let historical = page_history::load_page_history_snapshot(&pool, PAGE_A, &snapshot)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(historical).unwrap()["page"]["cover"],
            cover
        );

        let graph = transfers::json_graph_export::export_graph(
            &pool,
            NoteJsonGraphExportRequest {
                include_indexes: Some(false),
                include_history: Some(true),
                include_templates: Some(true),
                include_local_state: Some(false),
                pretty: Some(false),
            },
        )
        .await
        .unwrap();
        let graph = serde_json::to_value(graph).unwrap();
        let graph: Value = serde_json::from_str(graph["json"].as_str().unwrap()).unwrap();
        let page = graph["graph"]["pages"]["notes_pages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|page| page["id"] == PAGE_A)
            .unwrap();
        assert_eq!(page["cover"], cover);

        let markdown = transfers::markdown_export::export_page(
            &pool,
            NoteMarkdownExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_title: None,
                include_comments: None,
                include_resolved_comments: None,
            },
        )
        .await
        .unwrap();
        assert!(
            serde_json::to_value(markdown).unwrap()["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning["code"] == "page_cover_omitted")
        );
        let html = transfers::html_export::export_page(
            &pool,
            NoteHtmlExportRequest {
                page_id: PAGE_A.to_string(),
                include_page_tree: Some(false),
                include_comments: None,
                include_resolved_comments: None,
                include_assets: None,
                include_database_views: None,
            },
        )
        .await
        .unwrap();
        assert!(
            serde_json::to_value(html).unwrap()["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning["code"] == "page_cover_omitted")
        );
    });
}

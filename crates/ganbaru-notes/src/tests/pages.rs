use super::helpers::*;

#[test]
fn create_page_persists_title_and_initial_paragraph() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let pages = reads::list_pages(&pool).await.unwrap();
        assert_eq!(pages.len(), 1);

        let block_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notes_blocks WHERE page_id = ? AND type = 'paragraph'",
        )
        .bind(PAGE_A)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(block_count, 1);
    });
}

#[test]
fn exact_page_create_retry_returns_the_existing_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        let request = || NotePageCreate {
            id: PAGE_A.to_string(),
            title: "First page".to_string(),
            parent: workspace_parent(),
            folder_id: None,
            first_block_id: BLOCK_A.to_string(),
            after_block_id: None,
            properties: None,
        };

        writes::create_page(&pool, request()).await.unwrap();
        let retried = writes::create_page(&pool, request()).await.unwrap();
        let retried = serde_json::to_value(retried).unwrap();

        assert_eq!(retried["page"]["id"], PAGE_A);
        assert_eq!(retried["blocks"]["results"][0]["id"], BLOCK_A);
        let page_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(page_count, 1);
    });
}

#[test]
fn page_create_retry_rejects_mismatched_or_partial_data() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let mismatch = writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "Different title".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .err()
        .expect("mismatched retry must fail");
        assert!(mismatch.contains("different data"));

        sqlx::query("DELETE FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_A)
            .execute(&pool)
            .await
            .unwrap();
        let partial = writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "First page".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .err()
        .expect("partial retry must fail");
        assert!(partial.contains("missing its initial block"));
    });
}

#[test]
fn page_rename_preserves_project_metadata_properties() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_A.to_string(),
                title: "Untitled".to_string(),
                parent: workspace_parent(),
                folder_id: None,
                first_block_id: BLOCK_A.to_string(),
                after_block_id: None,
                properties: Some(json!({
                    "__ganbaru_project_id": "project-a",
                    "custom": "value"
                })),
            },
        )
        .await
        .unwrap();

        let renamed = writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Saved title".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        let renamed_json = serde_json::to_value(renamed).unwrap();

        assert_eq!(
            renamed_json["properties"]["__ganbaru_project_id"],
            "project-a"
        );
        assert_eq!(renamed_json["properties"]["custom"], "value");
        assert_eq!(
            renamed_json["properties"]["title"]["title"][0]["plain_text"],
            "Saved title"
        );
    });
}

#[test]
fn create_nested_page_appends_child_page_block_to_parent_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Nested page".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: Some(BLOCK_A.to_string()),
                properties: None,
            },
        )
        .await
        .unwrap();

        let parent_blocks = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let parent_json = serde_json::to_value(parent_blocks).unwrap();
        assert_eq!(parent_json["results"][1]["id"], PAGE_B);
        assert_eq!(parent_json["results"][1]["type"], "child_page");
        assert_eq!(
            parent_json["results"][1]["child_page"]["title"],
            "Nested page"
        );

        let child_blocks = reads::block_children(&pool, PAGE_B, None, Some(10))
            .await
            .unwrap();
        let child_json = serde_json::to_value(child_blocks).unwrap();
        assert_eq!(child_json["results"][0]["id"], BLOCK_B);
        assert_eq!(child_json["results"][0]["type"], "paragraph");
    });
}

#[test]
fn page_templates_create_apply_update_duplicate_and_delete() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Weekly review".to_string()),
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
            block_update("paragraph", paragraph_payload("Reflect on the week")),
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "to_do",
                    todo_payload("Choose next focus", false),
                )],
            },
        )
        .await
        .unwrap();

        let template = templates::create_page_template_from_page(
            &pool,
            NotePageTemplateCreateFromPage {
                id: TEMPLATE_A.to_string(),
                source_page_id: PAGE_A.to_string(),
                name: "Weekly review".to_string(),
            },
        )
        .await
        .unwrap();
        let template_json = serde_json::to_value(template).unwrap();
        assert_eq!(template_json["name"], "Weekly review");
        assert_eq!(template_json["block_count"], 2);
        assert!(template_json["properties"].get("title").is_some());

        let loaded = templates::apply_page_template(
            &pool,
            TEMPLATE_A,
            NotePageTemplateApply {
                parent: workspace_parent(),
                title: Some("Friday review".to_string()),
            },
        )
        .await
        .unwrap();
        let loaded_json = serde_json::to_value(loaded).unwrap();
        let applied_page_id = loaded_json["page"]["id"].as_str().unwrap().to_string();
        assert_eq!(
            loaded_json["page"]["properties"]["title"]["title"][0]["plain_text"],
            "Friday review"
        );
        assert_eq!(
            loaded_json["blocks"]["results"].as_array().unwrap().len(),
            2
        );
        assert_eq!(loaded_json["blocks"]["results"][0]["type"], "paragraph");
        assert_eq!(
            loaded_json["blocks"]["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Reflect on the week"
        );
        assert_eq!(loaded_json["blocks"]["results"][1]["type"], "to_do");

        create_page(&pool, PAGE_C, BLOCK_C).await;
        writes::update_page(
            &pool,
            PAGE_C,
            NotePageUpdate {
                title: Some("Daily plan".to_string()),
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
            BLOCK_C,
            block_update("paragraph", paragraph_payload("Plan the day")),
        )
        .await
        .unwrap();
        let updated = templates::update_page_template(
            &pool,
            TEMPLATE_A,
            NotePageTemplateUpdate {
                name: Some("Daily plan".to_string()),
                source_page_id: Some(PAGE_C.to_string()),
            },
        )
        .await
        .unwrap();
        let updated_json = serde_json::to_value(updated).unwrap();
        assert_eq!(updated_json["name"], "Daily plan");
        assert_eq!(updated_json["block_count"], 1);

        let duplicate = templates::duplicate_page_template(
            &pool,
            TEMPLATE_A,
            NotePageTemplateDuplicate {
                id: TEMPLATE_B.to_string(),
                name: "Daily plan copy".to_string(),
            },
        )
        .await
        .unwrap();
        let duplicate_json = serde_json::to_value(duplicate).unwrap();
        assert_eq!(duplicate_json["name"], "Daily plan copy");
        assert_eq!(duplicate_json["block_count"], 1);

        assert_eq!(
            templates::delete_page_template(&pool, TEMPLATE_A)
                .await
                .unwrap(),
            TEMPLATE_A
        );
        let templates = templates::list_page_templates(&pool).await.unwrap();
        let templates_json = serde_json::to_value(templates).unwrap();
        assert_eq!(templates_json.as_array().unwrap().len(), 1);
        assert_eq!(templates_json[0]["id"], TEMPLATE_B);

        let applied_page_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM notes_pages WHERE id = ?")
                .bind(applied_page_id)
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(applied_page_exists, Some(1));
    });
}

#[test]
fn page_history_snapshots_restore_copy_and_retention_settings() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let settings = page_history::load_page_history_settings(&pool)
            .await
            .unwrap();
        let settings_json = serde_json::to_value(settings).unwrap();
        assert_eq!(settings_json["retention_days"], 30);

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("paragraph", paragraph_payload("Draft one")),
        )
        .await
        .unwrap();
        let snapshots = page_history::list_page_history_snapshots(&pool, PAGE_A)
            .await
            .unwrap();
        let snapshots_json = serde_json::to_value(&snapshots).unwrap();
        let local_user_json =
            serde_json::to_value(local_user::load_local_user(&pool).await.unwrap()).unwrap();
        assert_eq!(snapshots_json.as_array().unwrap().len(), 1);
        assert_eq!(snapshots_json[0]["block_count"], 1);
        assert_eq!(snapshots_json[0]["created_by"]["id"], local_user_json["id"]);
        let initial_snapshot_id = snapshots_json[0]["id"].as_str().unwrap().to_string();

        sqlx::query(
            "UPDATE notes_page_history_snapshots
             SET created_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-10 minutes')
             WHERE id = ?",
        )
        .bind(&initial_snapshot_id)
        .execute(&pool)
        .await
        .unwrap();

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_B,
                    "to_do",
                    todo_payload("Review the draft", false),
                )],
            },
        )
        .await
        .unwrap();

        let snapshots = page_history::list_page_history_snapshots(&pool, PAGE_A)
            .await
            .unwrap();
        let snapshots_json = serde_json::to_value(&snapshots).unwrap();
        assert_eq!(snapshots_json.as_array().unwrap().len(), 2);
        let draft_snapshot_id = snapshots_json[0]["id"].as_str().unwrap().to_string();

        let initial_bundle_hash: String = sqlx::query_scalar(
            "SELECT block_bundle_hash
             FROM notes_page_history_snapshots
             WHERE id = ?",
        )
        .bind(&initial_snapshot_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let initial_blocks =
            project_history::load_page_history_blocks_tx(&mut tx, &initial_bundle_hash)
                .await
                .unwrap();
        tx.commit().await.unwrap();
        sqlx::query(
            "UPDATE notes_page_history_snapshots
             SET blocks = ?, block_bundle_hash = NULL
             WHERE id = ?",
        )
        .bind(&initial_blocks)
        .bind(&initial_snapshot_id)
        .execute(&pool)
        .await
        .unwrap();

        let initial_version =
            page_history::load_page_history_snapshot(&pool, PAGE_A, &initial_snapshot_id)
                .await
                .unwrap();
        let initial_json = serde_json::to_value(initial_version).unwrap();
        assert_eq!(
            initial_json["blocks"]["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            ""
        );

        let draft_version =
            page_history::load_page_history_snapshot(&pool, PAGE_A, &draft_snapshot_id)
                .await
                .unwrap();
        let draft_json = serde_json::to_value(draft_version).unwrap();
        assert_eq!(
            draft_json["blocks"]["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Draft one"
        );

        let copied = page_history::copy_page_history_blocks(
            &pool,
            PAGE_A,
            &draft_snapshot_id,
            NotePageHistoryCopyBlocks {
                after_block_id: None,
            },
        )
        .await
        .unwrap();
        let copied_json = serde_json::to_value(copied).unwrap();
        assert_eq!(copied_json["results"].as_array().unwrap().len(), 1);
        assert_eq!(
            copied_json["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            "Draft one"
        );

        let restored =
            page_history::restore_page_history_snapshot(&pool, PAGE_A, &initial_snapshot_id)
                .await
                .unwrap();
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(
            restored_json["blocks"]["results"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            restored_json["blocks"]["results"][0]["paragraph"]["rich_text"][0]["plain_text"],
            ""
        );

        let forever = page_history::update_page_history_settings(
            &pool,
            NotePageHistorySettingsUpdate {
                retention_days: None,
            },
        )
        .await;
        assert!(forever.is_err());

        let retained = page_history::update_page_history_settings(
            &pool,
            NotePageHistorySettingsUpdate {
                retention_days: Some(180),
            },
        )
        .await
        .unwrap();
        let retained_json = serde_json::to_value(retained).unwrap();
        assert_eq!(retained_json["retention_days"], 180);

        sqlx::query(
            "UPDATE notes_page_history_snapshots
             SET created_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-400 days')
             WHERE page_id = ?",
        )
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        page_history::update_page_history_settings(
            &pool,
            NotePageHistorySettingsUpdate {
                retention_days: Some(7),
            },
        )
        .await
        .unwrap();
        assert!(
            page_history::list_page_history_snapshots(&pool, PAGE_A)
                .await
                .unwrap()
                .is_empty()
        );
        let retained_block_bundles: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_history_bundles
             WHERE kind = 'row'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(retained_block_bundles, 0);

        page_history::update_page_history_settings(
            &pool,
            NotePageHistorySettingsUpdate {
                retention_days: Some(0),
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("paragraph", paragraph_payload("History disabled")),
        )
        .await
        .unwrap();
        assert!(
            page_history::list_page_history_snapshots(&pool, PAGE_A)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn page_history_coalesces_rapid_editor_snapshots() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_block(
            &pool,
            BLOCK_A,
            block_update("paragraph", paragraph_payload("First edit")),
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Second row"))],
            },
        )
        .await
        .unwrap();
        writes::update_block(
            &pool,
            BLOCK_B,
            block_update("paragraph", paragraph_payload("Second row edited")),
        )
        .await
        .unwrap();

        let snapshots = page_history::list_page_history_snapshots(&pool, PAGE_A)
            .await
            .unwrap();
        let snapshots_json = serde_json::to_value(snapshots).unwrap();
        assert_eq!(snapshots_json.as_array().unwrap().len(), 1);
        assert_eq!(snapshots_json[0]["reason"], "update_block");
        assert_eq!(snapshots_json[0]["block_count"], 1);
    });
}

#[test]
fn sidebar_pages_load_roots_expanded_children_and_selected_ancestors() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Nested".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_C.to_string(),
                title: "Leaf".to_string(),
                parent: page_parent(PAGE_B),
                folder_id: None,
                first_block_id: BLOCK_C.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();

        let roots = reads::list_sidebar_pages(
            &pool,
            NoteSidebarPagesRequest {
                expanded_page_ids: vec![],
                seed_page_ids: vec![],
                selected_page_id: None,
            },
        )
        .await
        .unwrap();
        let roots_json = serde_json::to_value(roots).unwrap();
        assert_eq!(roots_json["pages"].as_array().unwrap().len(), 1);
        assert_eq!(roots_json["pages"][0]["id"], PAGE_A);
        assert!(roots_json["pages"][0]["blocks"].is_null());
        assert_eq!(roots_json["page_ids_with_children"], json!([PAGE_A]));

        let expanded = reads::list_sidebar_pages(
            &pool,
            NoteSidebarPagesRequest {
                expanded_page_ids: vec![PAGE_A.to_string()],
                seed_page_ids: vec![],
                selected_page_id: None,
            },
        )
        .await
        .unwrap();
        let expanded_json = serde_json::to_value(expanded).unwrap();
        let expanded_ids = expanded_json["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|page| page["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(expanded_ids, vec![PAGE_B, PAGE_A]);
        assert_eq!(
            expanded_json["page_ids_with_children"],
            json!([PAGE_A, PAGE_B])
        );

        let selected = reads::list_sidebar_pages(
            &pool,
            NoteSidebarPagesRequest {
                expanded_page_ids: vec![],
                seed_page_ids: vec![],
                selected_page_id: Some(PAGE_C.to_string()),
            },
        )
        .await
        .unwrap();
        let selected_json = serde_json::to_value(selected).unwrap();
        let selected_ids = selected_json["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|page| page["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(selected_ids.len(), 3);
        assert!(selected_ids.contains(&PAGE_A));
        assert!(selected_ids.contains(&PAGE_B));
        assert!(selected_ids.contains(&PAGE_C));
    });
}

#[test]
fn sidebar_pages_report_trashed_parents_for_seed_pages() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Nested".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_pages SET in_trash = 1 WHERE id = ?")
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .unwrap();

        let sidebar_pages = reads::list_sidebar_pages(
            &pool,
            NoteSidebarPagesRequest {
                expanded_page_ids: vec![],
                seed_page_ids: vec![PAGE_B.to_string()],
                selected_page_id: None,
            },
        )
        .await
        .unwrap();
        let sidebar_json = serde_json::to_value(sidebar_pages).unwrap();
        assert_eq!(sidebar_json["pages"].as_array().unwrap().len(), 1);
        assert_eq!(sidebar_json["pages"][0]["id"], PAGE_B);
        assert_eq!(sidebar_json["trashed_parent_page_ids"], json!([PAGE_A]));
    });
}

#[test]
fn create_nested_page_can_insert_after_block_parent_sibling() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Nested"))],
            },
        )
        .await
        .unwrap();

        writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Nested under block".to_string(),
                parent: block_parent(BLOCK_A),
                folder_id: None,
                first_block_id: BLOCK_D.to_string(),
                after_block_id: Some(BLOCK_B.to_string()),
                properties: None,
            },
        )
        .await
        .unwrap();

        let children = reads::block_children(&pool, BLOCK_A, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        assert_eq!(children_json["results"][0]["id"], BLOCK_B);
        assert_eq!(children_json["results"][1]["id"], PAGE_B);
        assert_eq!(children_json["results"][1]["type"], "child_page");
        assert_eq!(
            children_json["results"][1]["child_page"]["title"],
            "Nested under block"
        );

        let child_page = reads::get_page(&pool, PAGE_B, false).await.unwrap();
        let child_page_json = serde_json::to_value(child_page).unwrap();
        assert_eq!(child_page_json["parent"]["block_id"], BLOCK_A);
    });
}

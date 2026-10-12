use super::helpers::*;

#[test]
fn backlinks_include_visible_child_page_blocks() {
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

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert_eq!(backlinks_json.as_array().unwrap().len(), 1);
        assert_eq!(backlinks_json[0]["object"], "backlink");
        assert_eq!(backlinks_json[0]["source_page"]["id"], PAGE_A);
        assert_eq!(backlinks_json[0]["source_block_id"], PAGE_B);
        assert_eq!(backlinks_json[0]["reference_type"], "child_page");

        writes::archive_page(&pool, PAGE_A, true).await.unwrap();
        assert!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn backlinks_include_local_notes_rich_text_links() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let linked_payload = json!({
            "rich_text": [{
                "type": "text",
                "text": {
                    "content": "See target",
                    "link": {
                        "url": format!(
                            "http://localhost:1420/?view=notes#notes?page={PAGE_B}&block={BLOCK_B}"
                        )
                    }
                },
                "annotations": {
                    "bold": false,
                    "italic": false,
                    "strikethrough": false,
                    "underline": false,
                    "code": false,
                    "color": "default"
                },
                "plain_text": "See target",
                "href": format!(
                    "http://localhost:1420/?view=notes#notes?page={PAGE_B}&block={BLOCK_B}"
                )
            }],
            "color": "default"
        });
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_C, "paragraph", linked_payload)],
            },
        )
        .await
        .unwrap();

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert_eq!(backlinks_json.as_array().unwrap().len(), 1);
        assert_eq!(backlinks_json[0]["source_page"]["id"], PAGE_A);
        assert_eq!(backlinks_json[0]["source_block_id"], BLOCK_C);
        assert_eq!(backlinks_json[0]["reference_type"], "link");
        assert_eq!(backlinks_json[0]["snippet"], "See target");
    });
}

#[test]
fn backlinks_refresh_when_local_notes_rich_text_links_are_edited() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("See target"))],
            },
        )
        .await
        .unwrap();
        assert!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );

        let target_url = format!("#notes?page={PAGE_B}&block={BLOCK_B}");
        writes::update_block(
            &pool,
            BLOCK_C,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [linked_rich_text("See target", &target_url)],
                    "color": "default"
                }),
            ),
        )
        .await
        .unwrap();
        let saved = reads::get_block(&pool, BLOCK_C, false).await.unwrap();
        let saved_json = serde_json::to_value(saved).unwrap();
        assert_eq!(
            saved_json["paragraph"]["rich_text"][0]["text"]["link"]["url"],
            target_url
        );
        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert_eq!(backlinks_json.as_array().unwrap().len(), 1);
        assert_eq!(backlinks_json[0]["source_page"]["id"], PAGE_A);
        assert_eq!(backlinks_json[0]["source_block_id"], BLOCK_C);
        assert_eq!(backlinks_json[0]["reference_type"], "link");

        writes::update_block(
            &pool,
            BLOCK_C,
            block_update(
                "paragraph",
                json!({
                    "rich_text": [linked_rich_text("Email", "mailto:team@example.com")],
                    "color": "default"
                }),
            ),
        )
        .await
        .unwrap();
        assert!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

#[test]
fn backlinks_include_page_mentions() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("See "),
                            page_mention(PAGE_B, "Target page")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert_eq!(backlinks_json.as_array().unwrap().len(), 1);
        assert_eq!(backlinks_json[0]["source_page"]["id"], PAGE_A);
        assert_eq!(backlinks_json[0]["source_block_id"], BLOCK_C);
        assert_eq!(backlinks_json[0]["reference_type"], "page_mention");
        assert_eq!(backlinks_json[0]["snippet"], "See Target page");
    });
}

#[test]
fn backlinks_index_rebuilds_comments_and_local_object_mentions() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let target_url =
            format!("http://localhost:1420/?view=notes#notes?page={PAGE_B}&block={BLOCK_B}");

        comments::create_comment(
            &pool,
            NoteCommentCreate {
                id: COMMENT_A.to_string(),
                parent: Some(page_parent(PAGE_A)),
                discussion_id: None,
                anchor: None,
                rich_text: vec![rich_text("Comment says "), page_mention(PAGE_B, "Target")],
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
                anchor: None,
                rich_text: vec![linked_rich_text("Linked comment", &target_url)],
                attachments: None,
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("Assigned "),
                            project_task_mention(PAGE_C, "Task")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert!(backlinks_json.as_array().unwrap().iter().any(|backlink| {
            backlink["source_block_type"] == "comment"
                && backlink["source_block_id"] == PAGE_A
                && backlink["reference_type"] == "comment_mention"
        }));
        assert!(backlinks_json.as_array().unwrap().iter().any(|backlink| {
            backlink["source_block_type"] == "comment"
                && backlink["source_block_id"] == BLOCK_A
                && backlink["reference_type"] == "comment_link"
        }));

        let local_object_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)
             FROM notes_backlink_index
             WHERE target_type = 'local_object'
               AND target_object_type = 'project_task'
               AND target_id = ?
               AND reference_type = 'local_object_mention'",
        )
        .bind(PAGE_C)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(local_object_count, 1);

        sqlx::query("DELETE FROM notes_backlink_index")
            .execute(&pool)
            .await
            .unwrap();
        let rebuilt = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let rebuilt_json = serde_json::to_value(rebuilt).unwrap();
        assert!(rebuilt_json.as_array().unwrap().iter().any(|backlink| {
            backlink["source_block_type"] == "comment"
                && backlink["reference_type"] == "comment_mention"
        }));

        comments::delete_comment(&pool, COMMENT_A).await.unwrap();
        let after_delete = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let after_delete_json = serde_json::to_value(after_delete).unwrap();
        assert!(
            !after_delete_json
                .as_array()
                .unwrap()
                .iter()
                .any(|backlink| {
                    backlink["source_block_type"] == "comment"
                        && backlink["reference_type"] == "comment_mention"
                })
        );
    });
}

#[test]
fn backlinks_include_data_source_row_page_mentions() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Tasks",
            BLOCK_A,
        )
        .await;
        data_sources::rows::create_data_source_row_page(
            &pool,
            DATA_SOURCE_A,
            NoteDataSourceRowPageCreate {
                id: PAGE_B.to_string(),
                title: "Target row".to_string(),
                first_block_id: BLOCK_B.to_string(),
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [
                            rich_text("See "),
                            page_mention(PAGE_B, "Target row")
                        ],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();

        let backlinks = links::backlinks::list_backlinks(&pool, PAGE_B)
            .await
            .unwrap();
        let backlinks_json = serde_json::to_value(backlinks).unwrap();
        assert_eq!(backlinks_json.as_array().unwrap().len(), 1);
        assert_eq!(backlinks_json[0]["source_page"]["id"], PAGE_A);
        assert_eq!(backlinks_json[0]["source_block_id"], BLOCK_C);
        assert_eq!(backlinks_json[0]["reference_type"], "page_mention");
    });
}

#[test]
fn backlinks_hide_trashed_blocks_and_source_pages() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        let target_url =
            format!("http://localhost:1420/?view=notes#notes?page={PAGE_B}&block={BLOCK_B}");
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(
                    BLOCK_C,
                    "paragraph",
                    json!({
                        "rich_text": [linked_rich_text("See target", &target_url)],
                        "color": "default"
                    }),
                )],
            },
        )
        .await
        .unwrap();
        assert_eq!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .len(),
            1
        );

        writes::trash_block(&pool, BLOCK_C, true).await.unwrap();
        assert!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );

        writes::trash_block(&pool, BLOCK_C, false).await.unwrap();
        assert_eq!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .len(),
            1
        );

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        assert!(
            links::backlinks::list_backlinks(&pool, PAGE_B)
                .await
                .unwrap()
                .is_empty()
        );
    });
}

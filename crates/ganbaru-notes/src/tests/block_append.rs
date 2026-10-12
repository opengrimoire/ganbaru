use super::helpers::*;

#[test]
fn append_children_paginates_and_preserves_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "to_do", todo_payload("Check", true)),
                    block(BLOCK_C, "code", code_payload("let x = 1;", "typescript")),
                ],
            },
        )
        .await
        .unwrap();

        let page_one = reads::block_children(&pool, PAGE_A, None, Some(2))
            .await
            .unwrap();
        assert!(
            serde_json::to_value(&page_one).unwrap()["has_more"]
                .as_bool()
                .unwrap()
        );
        let next_cursor = serde_json::to_value(&page_one).unwrap()["next_cursor"]
            .as_str()
            .unwrap()
            .to_string();
        let page_two = reads::block_children(&pool, PAGE_A, Some(&next_cursor), Some(2))
            .await
            .unwrap();
        let page_two_json = serde_json::to_value(&page_two).unwrap();
        assert_eq!(page_two_json["results"][0]["type"], "code");
        assert_eq!(
            page_two_json["results"][0]["code"]["language"],
            "typescript"
        );
    });
}

#[test]
fn append_children_rejects_non_child_parent_blocks() {
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
                    "code",
                    code_payload("let x = 1;", "typescript"),
                )],
            },
        )
        .await
        .unwrap();

        let result = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("code blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_children_round_trips_toggle_heading_parent_state() {
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
                    "heading_2",
                    heading_payload("Details", true, Some(false)),
                )],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        let heading = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let heading_json = serde_json::to_value(heading).unwrap();
        assert_eq!(heading_json["type"], "heading_2");
        assert_eq!(heading_json["heading_2"]["is_toggleable"], true);
        assert_eq!(heading_json["heading_2"]["ganbaru_open"], false);
        assert_eq!(heading_json["has_children"], true);

        let children = reads::block_children(&pool, BLOCK_B, None, Some(10))
            .await
            .unwrap();
        let children_json = serde_json::to_value(children).unwrap();
        assert_eq!(children_json["results"][0]["id"], BLOCK_C);
    });
}

#[test]
fn append_children_rejects_normal_heading_parent_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "heading_2", paragraph_payload("Details"))],
            },
        )
        .await
        .unwrap();

        let result = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("heading_2 blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_children_rejects_breadcrumb_parent_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "breadcrumb", json!({}))],
            },
        )
        .await
        .unwrap();

        let result = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("breadcrumb blocks cannot have children".to_string())
        );
    });
}

#[test]
fn append_and_update_breadcrumb_blocks_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "breadcrumb", json!({}))],
            },
        )
        .await
        .unwrap();

        writes::update_block(&pool, BLOCK_B, block_update("breadcrumb", json!({})))
            .await
            .unwrap();

        let stored = reads::get_block(&pool, BLOCK_B, false).await.unwrap();
        let stored_json = serde_json::to_value(stored).unwrap();
        assert_eq!(stored_json["type"], "breadcrumb");
        assert_eq!(stored_json["breadcrumb"], json!({}));
    });
}

#[test]
fn append_children_rejects_table_of_contents_parent_blocks() {
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
                    "table_of_contents",
                    json!({ "color": "default" }),
                )],
            },
        )
        .await
        .unwrap();

        let result = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("table_of_contents blocks cannot have children".to_string())
        );
    });
}

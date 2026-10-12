use super::helpers::*;

#[test]
fn move_page_between_workspace_and_parent_page() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;

        let moved = writes::move_page(
            &pool,
            PAGE_B,
            NoteMovePage {
                parent: page_parent(PAGE_A),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        let moved_page = serde_json::to_value(moved).unwrap();
        assert_eq!(moved_page["page"]["parent"]["type"], "page_id");
        assert_eq!(moved_page["page"]["parent"]["page_id"], PAGE_A);

        let parent_blocks = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let parent_blocks_json = serde_json::to_value(parent_blocks).unwrap();
        assert_eq!(parent_blocks_json["results"][1]["id"], PAGE_B);
        assert_eq!(parent_blocks_json["results"][1]["type"], "child_page");
        assert_eq!(
            parent_blocks_json["results"][1]["child_page"]["title"],
            "First page"
        );

        let moved_top_level = writes::move_page(
            &pool,
            PAGE_B,
            NoteMovePage {
                parent: workspace_parent(),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        let moved_top_level_page = serde_json::to_value(moved_top_level).unwrap();
        assert_eq!(moved_top_level_page["page"]["parent"]["type"], "workspace");
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());
    });
}

#[test]
fn move_page_rejects_descendant_parent() {
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

        let result = writes::move_page(
            &pool,
            PAGE_A,
            NoteMovePage {
                parent: page_parent(PAGE_B),
                folder_id: None,
            },
        )
        .await;
        let Err(error) = result else {
            panic!("descendant parent unexpectedly accepted a moved page");
        };
        assert_eq!(
            error,
            "page cannot be moved under its descendant".to_string()
        );
    });
}

#[test]
fn archiving_nested_page_hides_child_page_block() {
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

        writes::archive_page(&pool, PAGE_B, true).await.unwrap();
        let parent_blocks = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let parent_blocks_json = serde_json::to_value(parent_blocks).unwrap();
        assert_eq!(parent_blocks_json["results"].as_array().unwrap().len(), 1);
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());

        writes::archive_page(&pool, PAGE_B, false).await.unwrap();
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_ok());
    });
}

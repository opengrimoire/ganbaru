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
                project_id: None,
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
                project_id: None,
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

async fn create_nested_page(pool: &SqlitePool, page_id: &str, block_id: &str, title: &str) {
    writes::create_page(
        pool,
        NotePageCreate {
            id: page_id.to_string(),
            title: title.to_string(),
            parent: page_parent(PAGE_A),
            folder_id: None,
            project_id: None,
            first_block_id: block_id.to_string(),
            after_block_id: None,
            properties: None,
        },
    )
    .await
    .unwrap();
}

async fn page_parent_json(pool: &SqlitePool, page_id: &str) -> serde_json::Value {
    let page = reads::load_page(pool, page_id).await.unwrap();
    serde_json::to_value(page).unwrap()["page"]["parent"].clone()
}

#[test]
fn page_history_restore_keeps_moved_child_pages_where_they_live() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_nested_page(&pool, PAGE_B, BLOCK_B, "Moved").await;
        create_nested_page(&pool, PAGE_C, BLOCK_C, "Trashed").await;
        writes::move_page(
            &pool,
            PAGE_B,
            NoteMovePage {
                parent: workspace_parent(),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        writes::trash_page(&pool, PAGE_C, true).await.unwrap();
        writes::move_page(
            &pool,
            PAGE_A,
            NoteMovePage {
                parent: page_parent(PAGE_B),
                folder_id: None,
            },
        )
        .await
        .unwrap();

        let mut snapshot_with_both = None;
        for snapshot in page_history::list_page_history_snapshots(&pool, PAGE_A)
            .await
            .unwrap()
        {
            let snapshot_id = serde_json::to_value(snapshot).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let version = page_history::load_page_history_snapshot(&pool, PAGE_A, &snapshot_id)
                .await
                .unwrap();
            let ids = serde_json::to_value(version).unwrap()["blocks"]["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|block| block["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            if ids.iter().any(|id| id == PAGE_B) && ids.iter().any(|id| id == PAGE_C) {
                snapshot_with_both = Some(snapshot_id);
            }
        }
        let snapshot_id = snapshot_with_both.expect("a version lists both child pages");

        let restored = page_history::restore_page_history_snapshot(&pool, PAGE_A, &snapshot_id)
            .await
            .unwrap();
        let blocks = serde_json::to_value(restored).unwrap()["blocks"]["results"].clone();
        let child_pages = blocks
            .as_array()
            .unwrap()
            .iter()
            .filter(|block| block["type"] == "child_page")
            .collect::<Vec<_>>();
        assert_eq!(child_pages.len(), 2);
        assert!(child_pages.iter().any(|block| block["id"] == PAGE_C));
        let moved_stub = child_pages
            .iter()
            .find(|block| block["child_page"]["title"] == "Moved")
            .expect("the moved page keeps a restored link");
        assert_ne!(moved_stub["id"], PAGE_B);

        assert_eq!(page_parent_json(&pool, PAGE_B).await["type"], "workspace");
        assert_eq!(page_parent_json(&pool, PAGE_A).await["page_id"], PAGE_B);
        let trashed_child = page_parent_json(&pool, PAGE_C).await;
        assert_eq!(trashed_child["page_id"], PAGE_A);
        let trashed_child_page =
            serde_json::to_value(reads::load_page(&pool, PAGE_C).await.unwrap()).unwrap();
        assert_eq!(trashed_child_page["page"]["in_trash"], false);
    });
}

async fn append_paragraph(pool: &SqlitePool, page_id: &str, block_id: &str, text: &str) {
    writes::append_block_children(
        pool,
        NoteAppendBlockChildren {
            parent: page_parent(page_id),
            after: None,
            children: vec![block(block_id, "paragraph", paragraph_payload(text))],
        },
    )
    .await
    .unwrap();
}

async fn set_paragraph(pool: &SqlitePool, block_id: &str, text: &str) {
    writes::update_block(
        pool,
        block_id,
        block_update("paragraph", paragraph_payload(text)),
    )
    .await
    .unwrap();
}

async fn block_trash_state(pool: &SqlitePool, block_id: &str) -> Option<(String, i64)> {
    sqlx::query_as("SELECT page_id, in_trash FROM notes_blocks WHERE id = ?")
        .bind(block_id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

#[test]
fn page_history_restore_applies_the_snapshot_as_a_diff() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        set_paragraph(&pool, BLOCK_A, "Kept").await;
        append_paragraph(&pool, PAGE_A, BLOCK_D, "Deleted later").await;
        create_nested_page(&pool, PAGE_B, BLOCK_B, "Nested").await;
        set_paragraph(&pool, BLOCK_B, "Nested body").await;
        let mut tx = pool.begin().await.unwrap();
        let snapshot_id = page_history::record_page_snapshot_tx(&mut tx, PAGE_A, "move_page")
            .await
            .unwrap()
            .expect("the snapshot is recorded");
        tx.commit().await.unwrap();

        set_paragraph(&pool, BLOCK_A, "Edited").await;
        sqlx::query("DELETE FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_D)
            .execute(&pool)
            .await
            .unwrap();
        append_paragraph(&pool, PAGE_A, BLOCK_E, "Added later").await;
        create_nested_page(&pool, PAGE_C, BLOCK_C, "Added page").await;

        let restored = page_history::restore_page_history_snapshot(&pool, PAGE_A, &snapshot_id)
            .await
            .unwrap();
        let blocks = serde_json::to_value(restored).unwrap()["blocks"]["results"].clone();
        let blocks = blocks.as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0]["id"], BLOCK_A);
        assert_eq!(blocks[0]["paragraph"]["rich_text"][0]["plain_text"], "Kept");
        assert_ne!(blocks[1]["id"], BLOCK_D);
        assert_eq!(
            blocks[1]["paragraph"]["rich_text"][0]["plain_text"],
            "Deleted later"
        );
        assert_eq!(blocks[2]["id"], PAGE_B);
        assert_eq!(blocks[2]["child_page"]["title"], "Nested");

        assert_eq!(block_trash_state(&pool, BLOCK_D).await, None);
        assert_eq!(
            block_trash_state(&pool, BLOCK_E).await,
            Some((PAGE_A.to_string(), 1))
        );
        assert_eq!(
            block_trash_state(&pool, PAGE_C).await,
            Some((PAGE_A.to_string(), 1))
        );
        let added_page_trashed: i64 =
            sqlx::query_scalar("SELECT in_trash FROM notes_pages WHERE id = ?")
                .bind(PAGE_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(added_page_trashed, 1);
        assert_eq!(
            block_trash_state(&pool, BLOCK_C).await,
            Some((PAGE_C.to_string(), 0))
        );

        let nested_page =
            serde_json::to_value(reads::load_page(&pool, PAGE_B).await.unwrap()).unwrap();
        assert_eq!(nested_page["page"]["in_trash"], false);
        let nested_blocks = nested_page["blocks"]["results"].as_array().unwrap();
        assert_eq!(nested_blocks.len(), 1);
        assert_eq!(nested_blocks[0]["id"], BLOCK_B);
        assert_eq!(
            nested_blocks[0]["paragraph"]["rich_text"][0]["plain_text"],
            "Nested body"
        );
    });
}

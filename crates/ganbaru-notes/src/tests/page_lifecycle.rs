use super::helpers::*;

#[test]
fn duplicate_page_copies_metadata_and_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::update_page(
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
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Detail"))],
            },
        )
        .await
        .unwrap();

        let duplicated = writes::duplicate_page(
            &pool,
            PAGE_A,
            NoteDuplicatePage {
                title: Some("Copy of First page".to_string()),
            },
        )
        .await
        .unwrap();
        let duplicated_json = serde_json::to_value(duplicated).unwrap();
        let duplicated_id = duplicated_json["page"]["id"].as_str().unwrap();
        assert_ne!(duplicated_id, PAGE_A);
        assert_eq!(duplicated_json["page"]["parent"]["type"], "workspace");
        assert_eq!(
            duplicated_json["page"]["properties"]["title"]["title"][0]["plain_text"],
            "Copy of First page"
        );
        assert_eq!(duplicated_json["page"]["icon"]["emoji"], "📌");
        assert_eq!(
            duplicated_json["page"]["cover"]["external"]["url"],
            "https://example.com/cover.jpg"
        );
        assert_eq!(
            duplicated_json["blocks"]["results"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            duplicated_json["blocks"]["results"][1]["paragraph"]["rich_text"][0]["plain_text"],
            "Detail"
        );
        assert_ne!(duplicated_json["blocks"]["results"][0]["id"], BLOCK_A);
        assert_ne!(duplicated_json["blocks"]["results"][1]["id"], BLOCK_B);
    });
}

#[test]
fn duplicate_page_copies_nested_child_pages() {
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
                after_block_id: Some(BLOCK_A.to_string()),
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
                after_block_id: Some(BLOCK_B.to_string()),
                properties: None,
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_C),
                after: Some(BLOCK_C.to_string()),
                children: vec![block(
                    BLOCK_D,
                    "paragraph",
                    paragraph_payload("Leaf detail"),
                )],
            },
        )
        .await
        .unwrap();

        let duplicated = writes::duplicate_page(
            &pool,
            PAGE_B,
            NoteDuplicatePage {
                title: Some("Copy of Nested".to_string()),
            },
        )
        .await
        .unwrap();
        let duplicated_json = serde_json::to_value(duplicated).unwrap();
        let duplicated_id = duplicated_json["page"]["id"].as_str().unwrap();
        assert_eq!(duplicated_json["page"]["parent"]["page_id"], PAGE_A);
        assert_eq!(
            duplicated_json["page"]["properties"]["title"]["title"][0]["plain_text"],
            "Copy of Nested"
        );

        let parent_blocks = reads::block_children(&pool, PAGE_A, None, Some(10))
            .await
            .unwrap();
        let parent_blocks_json = serde_json::to_value(parent_blocks).unwrap();
        assert_eq!(parent_blocks_json["results"][1]["id"], PAGE_B);
        assert_eq!(parent_blocks_json["results"][2]["id"], duplicated_id);
        assert_eq!(
            parent_blocks_json["results"][2]["child_page"]["title"],
            "Copy of Nested"
        );

        let child_page_id: String =
            sqlx::query_scalar("SELECT id FROM notes_pages WHERE parent_page_id = ?")
                .bind(duplicated_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(child_page_id, PAGE_C);
        let duplicated_blocks = reads::block_children(&pool, duplicated_id, None, Some(10))
            .await
            .unwrap();
        let duplicated_blocks_json = serde_json::to_value(duplicated_blocks).unwrap();
        assert_eq!(duplicated_blocks_json["results"][1]["id"], child_page_id);
        assert_eq!(
            duplicated_blocks_json["results"][1]["child_page"]["title"],
            "Leaf"
        );

        let duplicated_child_blocks = reads::block_children(&pool, &child_page_id, None, Some(10))
            .await
            .unwrap();
        let duplicated_child_blocks_json = serde_json::to_value(duplicated_child_blocks).unwrap();
        assert_eq!(
            duplicated_child_blocks_json["results"][1]["paragraph"]["rich_text"][0]["plain_text"],
            "Leaf detail"
        );
    });
}

#[test]
fn page_rename_and_trash_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::update_page(
            &pool,
            PAGE_A,
            NotePageUpdate {
                title: Some("Renamed".to_string()),
                parent: None,
                properties: None,
                icon: OptionalJsonValue::Unset,
                cover: OptionalJsonValue::Unset,
            },
        )
        .await
        .unwrap();
        let stored_title: String = sqlx::query_scalar("SELECT title FROM notes_pages WHERE id = ?")
            .bind(PAGE_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(stored_title, "Renamed");

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        let trashed_time: Option<String> =
            sqlx::query_scalar("SELECT trashed_time FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(trashed_time.is_some());
        assert!(reads::list_pages(&pool).await.unwrap().is_empty());
        let trashed_pages = reads::list_trashed_pages(&pool).await.unwrap();
        assert_eq!(trashed_pages.len(), 1);

        writes::trash_page(&pool, PAGE_A, false).await.unwrap();
        let restored_trashed_time: Option<String> =
            sqlx::query_scalar("SELECT trashed_time FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(restored_trashed_time.is_none());
        assert_eq!(reads::list_pages(&pool).await.unwrap().len(), 1);
        assert!(reads::list_trashed_pages(&pool).await.unwrap().is_empty());
    });
}

#[test]
fn page_archive_and_unarchive_round_trip() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::archive_page(&pool, PAGE_A, true).await.unwrap();
        assert!(reads::list_pages(&pool).await.unwrap().is_empty());
        assert!(reads::load_page(&pool, PAGE_A).await.is_err());
        let archived_pages = reads::list_archived_pages(&pool).await.unwrap();
        assert_eq!(archived_pages.len(), 1);
        let archived_page = serde_json::to_value(&archived_pages[0]).unwrap();
        assert_eq!(archived_page["archived"], true);
        assert_eq!(archived_page["in_trash"], false);

        let child_result = writes::create_page(
            &pool,
            NotePageCreate {
                id: PAGE_B.to_string(),
                title: "Blocked child".to_string(),
                parent: page_parent(PAGE_A),
                folder_id: None,
                first_block_id: BLOCK_B.to_string(),
                after_block_id: None,
                properties: None,
            },
        )
        .await;
        let Err(child_error) = child_result else {
            panic!("archived parent unexpectedly accepted a child page");
        };
        assert_eq!(child_error, "parent page not found");

        writes::archive_page(&pool, PAGE_A, false).await.unwrap();
        assert_eq!(reads::list_pages(&pool).await.unwrap().len(), 1);
        assert!(reads::list_archived_pages(&pool).await.unwrap().is_empty());
        assert!(reads::load_page(&pool, PAGE_A).await.is_ok());
    });
}

#[test]
fn unarchiving_nested_page_with_archived_parent_promotes_to_workspace() {
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

        writes::archive_page(&pool, PAGE_A, true).await.unwrap();
        writes::archive_page(&pool, PAGE_B, true).await.unwrap();
        let restored = writes::archive_page(&pool, PAGE_B, false).await.unwrap();
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(restored_json["parent"]["type"], "workspace");
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());
    });
}

#[test]
fn trashing_archived_page_clears_archive_state() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        writes::archive_page(&pool, PAGE_A, true).await.unwrap();
        let trashed = writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        let trashed_page = serde_json::to_value(trashed).unwrap();
        assert_eq!(trashed_page["in_trash"], true);
        assert_eq!(trashed_page["archived"], false);
        assert!(reads::list_archived_pages(&pool).await.unwrap().is_empty());
        assert_eq!(reads::list_trashed_pages(&pool).await.unwrap().len(), 1);
    });
}

#[test]
fn trashing_parent_page_updates_descendant_pages() {
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

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        assert!(reads::list_pages(&pool).await.unwrap().is_empty());
        assert_eq!(reads::list_trashed_pages(&pool).await.unwrap().len(), 3);
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());

        writes::trash_page(&pool, PAGE_A, false).await.unwrap();
        assert_eq!(reads::list_pages(&pool).await.unwrap().len(), 3);
        assert!(reads::list_trashed_pages(&pool).await.unwrap().is_empty());
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_ok());
    });
}

#[test]
fn trashing_nested_page_hides_and_restores_child_page_block() {
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

        writes::trash_page(&pool, PAGE_B, true).await.unwrap();
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());
        let trashed_block = reads::get_block(&pool, PAGE_B, true).await.unwrap();
        let trashed_block_json = serde_json::to_value(trashed_block).unwrap();
        assert_eq!(trashed_block_json["in_trash"], true);

        writes::trash_page(&pool, PAGE_B, false).await.unwrap();
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_ok());
    });
}

#[test]
fn restoring_nested_page_with_trashed_parent_promotes_to_workspace() {
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

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        let restored = writes::trash_page(&pool, PAGE_B, false).await.unwrap();
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(restored_json["parent"]["type"], "workspace");
        assert_eq!(reads::list_pages(&pool).await.unwrap().len(), 1);
        assert_eq!(reads::list_trashed_pages(&pool).await.unwrap().len(), 1);
        assert!(reads::get_block(&pool, PAGE_B, false).await.is_err());
    });
}

#[test]
fn restoring_page_with_missing_parent_promotes_to_workspace() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notes_pages
             SET parent_type = 'page_id',
                 parent_page_id = ?,
                 parent_block_id = NULL
             WHERE id = ?",
        )
        .bind(PAGE_B)
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();

        let restored = writes::trash_page(&pool, PAGE_A, false).await.unwrap();
        let restored_json = serde_json::to_value(restored).unwrap();
        assert_eq!(restored_json["parent"]["type"], "workspace");
        assert!(reads::get_page(&pool, PAGE_A, false).await.is_ok());
    });
}

#[test]
fn permanent_page_delete_requires_trash() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;

        let result = writes::permanently_delete_page(&pool, PAGE_A).await;
        assert_eq!(
            result,
            Err("notes page must be in trash before permanent delete".to_string())
        );
        assert_eq!(reads::list_pages(&pool).await.unwrap().len(), 1);
    });
}

#[test]
fn permanent_page_delete_removes_subtree_and_paired_blocks() {
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
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("Leaf"))],
            },
        )
        .await
        .unwrap();
        writes::create_child_page_from_block(
            &pool,
            BLOCK_C,
            NoteChildPageFromBlockCreate {
                first_block_id: BLOCK_D.to_string(),
                title: None,
                properties: None,
            },
        )
        .await
        .unwrap();

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        let deleted_ids = writes::permanently_delete_page(&pool, PAGE_A)
            .await
            .unwrap();
        assert!(deleted_ids.contains(&PAGE_A.to_string()));
        assert!(deleted_ids.contains(&PAGE_B.to_string()));
        assert!(deleted_ids.contains(&BLOCK_C.to_string()));
        assert!(reads::list_pages(&pool).await.unwrap().is_empty());
        assert!(reads::list_trashed_pages(&pool).await.unwrap().is_empty());
        let page_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();
        let block_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(page_count, 0);
        assert_eq!(block_count, 0);
    });
}

#[test]
fn purge_expired_trashed_pages_deletes_after_retention_window() {
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

        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        sqlx::query(
            "UPDATE notes_pages
             SET trashed_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-6 days')
             WHERE id = ?",
        )
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            writes::purge_expired_trashed_pages(&pool)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(reads::list_trashed_pages(&pool).await.unwrap().len(), 2);

        sqlx::query(
            "UPDATE notes_pages
             SET trashed_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-8 days')
             WHERE id = ?",
        )
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        let deleted_ids = writes::purge_expired_trashed_pages(&pool).await.unwrap();
        assert!(deleted_ids.contains(&PAGE_A.to_string()));
        assert!(deleted_ids.contains(&PAGE_B.to_string()));
        assert!(reads::list_trashed_pages(&pool).await.unwrap().is_empty());
    });
}

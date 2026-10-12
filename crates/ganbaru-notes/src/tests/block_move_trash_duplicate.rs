use super::helpers::*;

#[test]
fn move_block_nests_and_outdents_with_parent_state() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "paragraph", paragraph_payload("Parent")),
                    block(BLOCK_C, "paragraph", paragraph_payload("Child")),
                ],
            },
        )
        .await
        .unwrap();

        writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: block_parent(BLOCK_B),
                after: None,
                before: None,
            },
        )
        .await
        .unwrap();
        let nested_parent: String =
            sqlx::query_scalar("SELECT parent_block_id FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(nested_parent, BLOCK_B);
        let has_children: i64 =
            sqlx::query_scalar("SELECT has_children FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(has_children, 1);

        writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_B.to_string()),
                before: None,
            },
        )
        .await
        .unwrap();
        let row = sqlx::query("SELECT parent_type, parent_page_id FROM notes_blocks WHERE id = ?")
            .bind(BLOCK_C)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("parent_type"), "page_id");
        assert_eq!(row.get::<String, _>("parent_page_id"), PAGE_A);
    });
}

#[test]
fn move_block_can_place_before_first_sibling() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "paragraph", paragraph_payload("Second")),
                    block(BLOCK_C, "paragraph", paragraph_payload("Third")),
                    block(BLOCK_D, "paragraph", paragraph_payload("Fourth")),
                ],
            },
        )
        .await
        .unwrap();

        writes::move_block(
            &pool,
            BLOCK_D,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: None,
                before: Some(BLOCK_A.to_string()),
            },
        )
        .await
        .unwrap();

        let ids = sqlx::query_scalar::<_, String>(
            "SELECT id FROM notes_blocks WHERE parent_page_id = ? ORDER BY sort_order ASC, id ASC",
        )
        .bind(PAGE_A)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(ids, vec![BLOCK_D, BLOCK_A, BLOCK_B, BLOCK_C]);
    });
}

#[test]
fn move_block_rejects_ambiguous_after_and_before() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "paragraph", paragraph_payload("Second")),
                    block(BLOCK_C, "paragraph", paragraph_payload("Third")),
                ],
            },
        )
        .await
        .unwrap();

        let result = writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                before: Some(BLOCK_C.to_string()),
            },
        )
        .await;
        assert_eq!(
            result.err(),
            Some("move request cannot include both after and before".to_string())
        );
    });
}

#[test]
fn move_block_rejects_after_anchor_outside_destination_parent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "paragraph", paragraph_payload("Parent")),
                    block(BLOCK_C, "paragraph", paragraph_payload("Nested")),
                ],
            },
        )
        .await
        .unwrap();
        writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: block_parent(BLOCK_B),
                after: None,
                before: None,
            },
        )
        .await
        .unwrap();

        let result = writes::move_block(
            &pool,
            BLOCK_A,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_C.to_string()),
                before: None,
            },
        )
        .await;
        assert_eq!(result.err(), Some("after block not found".to_string()));
    });
}

#[test]
fn move_block_rejects_invalid_structural_parent_shapes() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "column_list", json!({})),
                    block(BLOCK_C, "table", table_payload(2)),
                    block(BLOCK_D, "paragraph", paragraph_payload("Paragraph")),
                ],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_E, "column", column_payload(Some(1.0)))],
            },
        )
        .await
        .unwrap();

        let paragraph_under_columns = writes::move_block(
            &pool,
            BLOCK_D,
            NoteMoveBlock {
                parent: block_parent(BLOCK_B),
                after: None,
                before: None,
            },
        )
        .await;
        assert_eq!(
            paragraph_under_columns.err(),
            Some("paragraph blocks cannot be children of column_list blocks".to_string())
        );

        let paragraph_under_table = writes::move_block(
            &pool,
            BLOCK_D,
            NoteMoveBlock {
                parent: block_parent(BLOCK_C),
                after: None,
                before: None,
            },
        )
        .await;
        assert_eq!(
            paragraph_under_table.err(),
            Some("paragraph blocks cannot be children of table blocks".to_string())
        );

        let column_under_page = writes::move_block(
            &pool,
            BLOCK_E,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_D.to_string()),
                before: None,
            },
        )
        .await;
        assert_eq!(
            column_under_page.err(),
            Some("column blocks must be children of column_list blocks".to_string())
        );
    });
}

#[test]
fn move_block_to_another_page_updates_descendant_page_ids() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_D).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
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

        writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: page_parent(PAGE_B),
                after: None,
                before: None,
            },
        )
        .await
        .unwrap();

        let rows = sqlx::query("SELECT id, page_id, parent_type, parent_page_id, parent_block_id FROM notes_blocks WHERE id IN (?, ?) ORDER BY id ASC")
            .bind(BLOCK_B)
            .bind(BLOCK_C)
            .fetch_all(&pool)
            .await
            .unwrap();
        let moved_parent = rows
            .iter()
            .find(|row| row.get::<String, _>("id") == BLOCK_B)
            .expect("moved parent row should exist");
        assert_eq!(moved_parent.get::<String, _>("page_id"), PAGE_B);
        assert_eq!(moved_parent.get::<String, _>("parent_type"), "page_id");
        assert_eq!(moved_parent.get::<String, _>("parent_page_id"), PAGE_B);
        let moved_child = rows
            .iter()
            .find(|row| row.get::<String, _>("id") == BLOCK_C)
            .expect("moved child row should exist");
        assert_eq!(moved_child.get::<String, _>("page_id"), PAGE_B);
        assert_eq!(moved_child.get::<String, _>("parent_type"), "block_id");
        assert_eq!(moved_child.get::<String, _>("parent_block_id"), BLOCK_B);
    });
}

#[test]
fn move_block_rejects_destination_page_inside_source_subtree() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::create_child_page_from_block(
            &pool,
            BLOCK_B,
            NoteChildPageFromBlockCreate {
                first_block_id: BLOCK_D.to_string(),
                title: Some("Nested".to_string()),
                properties: None,
            },
        )
        .await
        .unwrap();

        let result = writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: page_parent(BLOCK_B),
                after: None,
                before: None,
            },
        )
        .await;
        assert_eq!(
            result.err(),
            Some("block cannot be moved into a page contained by its subtree".to_string())
        );
    });
}

#[test]
fn trash_block_hides_descendants_and_refreshes_parent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_D, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        writes::trash_block(&pool, BLOCK_B, true).await.unwrap();
        let trashed_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes_blocks WHERE in_trash = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(trashed_count, 2);
        let visible_children = reads::block_children(&pool, PAGE_A, None, Some(50))
            .await
            .unwrap();
        let json = serde_json::to_value(visible_children).unwrap();
        assert_eq!(json["results"].as_array().unwrap().len(), 1);
    });
}

#[test]
fn move_block_rejects_descendant_parent() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
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

        let result = writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: block_parent(BLOCK_C),
                after: None,
                before: None,
            },
        )
        .await;
        assert_eq!(
            result.err(),
            Some("block cannot be moved under its descendant".to_string())
        );
    });
}

#[test]
fn duplicate_block_clones_nested_subtree_after_source() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![
                    block(BLOCK_B, "paragraph", paragraph_payload("Parent")),
                    block(BLOCK_C, "paragraph", paragraph_payload("After source")),
                ],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_D, "to_do", todo_payload("Child", true))],
            },
        )
        .await
        .unwrap();

        writes::duplicate_block(
            &pool,
            BLOCK_B,
            NoteDuplicateBlock {
                duplicated_block_ids: vec![
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_B.to_string(),
                        duplicate_id: BLOCK_E.to_string(),
                    },
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_D.to_string(),
                        duplicate_id: BLOCK_F.to_string(),
                    },
                ],
            },
        )
        .await
        .unwrap();

        let duplicate_root = sqlx::query(
            "SELECT parent_type, parent_page_id, has_children, type, payload, sort_order
             FROM notes_blocks
             WHERE id = ?",
        )
        .bind(BLOCK_E)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(duplicate_root.get::<String, _>("parent_type"), "page_id");
        assert_eq!(duplicate_root.get::<String, _>("parent_page_id"), PAGE_A);
        assert_eq!(duplicate_root.get::<i64, _>("has_children"), 1);
        assert_eq!(duplicate_root.get::<String, _>("type"), "paragraph");
        let duplicate_payload: String = duplicate_root.get("payload");
        assert!(duplicate_payload.contains("Parent"));

        let duplicate_child = sqlx::query(
            "SELECT parent_type, parent_block_id, type, payload
             FROM notes_blocks
             WHERE id = ?",
        )
        .bind(BLOCK_F)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(duplicate_child.get::<String, _>("parent_type"), "block_id");
        assert_eq!(duplicate_child.get::<String, _>("parent_block_id"), BLOCK_E);
        assert_eq!(duplicate_child.get::<String, _>("type"), "to_do");
        let child_payload: String = duplicate_child.get("payload");
        assert!(child_payload.contains("\"checked\":true"));

        let source_order: f64 =
            sqlx::query_scalar("SELECT sort_order FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        let duplicate_order: f64 = duplicate_root.get("sort_order");
        let next_order: f64 =
            sqlx::query_scalar("SELECT sort_order FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(source_order < duplicate_order);
        assert!(duplicate_order < next_order);
    });
}

#[test]
fn duplicate_block_rejects_partial_subtree_id_maps() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: page_parent(PAGE_A),
                after: Some(BLOCK_A.to_string()),
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("Parent"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_B),
                after: None,
                children: vec![block(BLOCK_D, "paragraph", paragraph_payload("Child"))],
            },
        )
        .await
        .unwrap();

        let result = writes::duplicate_block(
            &pool,
            BLOCK_B,
            NoteDuplicateBlock {
                duplicated_block_ids: vec![NoteDuplicatedBlockId {
                    source_id: BLOCK_B.to_string(),
                    duplicate_id: BLOCK_E.to_string(),
                }],
            },
        )
        .await;

        assert_eq!(
            result.err(),
            Some("duplicated_block_ids must match the source block subtree".to_string())
        );
    });
}

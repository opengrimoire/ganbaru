use super::helpers::*;
use crate::writes::compound::{NoteCompoundEdit, apply_compound_edit};

#[test]
fn copy_limits_reject_a_large_page_without_creating_a_partial_copy() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        sqlx::query(
            "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM ids WHERE n < 10000)
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, payload, plain_text, sort_order)
             SELECT printf('12121212-1212-4212-8212-%012d', n), ?, 'page_id', ?, 'paragraph',
                '{\"rich_text\":[]}', '', n FROM ids",
        )
        .bind(PAGE_A)
        .bind(PAGE_A)
        .execute(&pool)
        .await
        .unwrap();
        let error = writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
            .await
            .err()
            .unwrap();
        assert!(error.contains("record limit"), "{error}");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
    });
}

#[test]
fn copy_limits_reject_excessive_block_depth_without_truncating_content() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        sqlx::query(
            "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM ids WHERE n < 65)
             INSERT INTO notes_blocks (id, page_id, parent_type, parent_block_id, type, payload, plain_text, sort_order)
             SELECT printf('12121212-1212-4212-8212-%012d', n), ?, 'block_id',
                CASE WHEN n = 1 THEN ? ELSE printf('12121212-1212-4212-8212-%012d', n - 1) END,
                'paragraph', '{\"rich_text\":[]}', '', n FROM ids",
        )
        .bind(PAGE_A)
        .bind(BLOCK_A)
        .execute(&pool)
        .await
        .unwrap();
        let error = writes::duplicate_page(&pool, PAGE_A, NoteDuplicatePage { title: None })
            .await
            .err()
            .unwrap();
        assert!(error.contains("nesting limit"), "{error}");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_blocks")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 66);
    });
}

#[test]
fn copy_limits_reject_a_cyclic_subtree_before_loading_payloads() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_B, "paragraph", paragraph_payload("child"))],
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE notes_blocks SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = ? WHERE id = ?")
            .bind(BLOCK_B).bind(BLOCK_A).execute(&pool).await.unwrap();
        let error = writes::duplicate_block(
            &pool,
            BLOCK_A,
            NoteDuplicateBlock {
                duplicated_block_ids: vec![
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_A.into(),
                        duplicate_id: BLOCK_C.into(),
                    },
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_B.into(),
                        duplicate_id: BLOCK_D.into(),
                    },
                ],
            },
        )
        .await
        .err()
        .unwrap();
        assert!(error.contains("cycle"), "{error}");
        assert!(reads::get_block(&pool, BLOCK_C, true).await.is_err());
    });
}

#[test]
fn copy_limits_accumulate_database_source_bytes_across_one_compound_edit() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Source",
            BLOCK_A,
        )
        .await;
        // Each copy fits individually, but two copies exceed the shared source budget.
        sqlx::query("UPDATE notes_data_sources SET description = json_array(json_object('type', 'text', 'text', json_object('content', replace(hex(zeroblob(17 * 1024 * 1024)), '00', 'x')))) WHERE id = ?")
            .bind(DATA_SOURCE_A).execute(&pool).await.unwrap();
        let revision = reads::get_block_row(&pool, BLOCK_A, false)
            .await
            .unwrap()
            .edit_revision()
            .unwrap();
        let request: NoteCompoundEdit = serde_json::from_value(json!({
            "operation_id": "13131313-1313-4313-8313-131313131313", "page_id": PAGE_A, "kind": "paste",
            "expected_blocks": { BLOCK_A: revision },
            "operations": [
                { "type": "copy_database", "request": { "source_block_id": DATABASE_A, "id": DATABASE_B,
                    "parent": page_parent(PAGE_A), "after_block_id": BLOCK_A } },
                { "type": "copy_database", "request": { "source_block_id": DATABASE_A, "id": LINKED_DATABASE_A,
                    "parent": page_parent(PAGE_A), "after_block_id": DATABASE_B } }
            ]
        })).unwrap();
        let error = apply_compound_edit(&pool, request).await.err().unwrap();
        assert!(error.contains("source byte limit"), "{error}");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_databases")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "the first completed database copy must roll back");
        let receipts: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_edit_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
    });
}

use super::helpers::*;
use crate::transfers::import_writer::{ImportBlock, validate_import_block_depth};

const CHAIN_DEPTH: i64 = writes::MAX_BLOCK_DEPTH - 1;

fn chain_id(level: i64) -> String {
    format!("d0000000-0000-4000-8000-{level:012}")
}

/// Stores one paragraph per level so the deepest block sits one level under the limit.
async fn insert_paragraph_chain(pool: &SqlitePool) {
    sqlx::query(
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM ids WHERE n < ?)
         INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, parent_block_id, type, text, plain_text, order_key)
         SELECT printf('d0000000-0000-4000-8000-%012d', n), ?,
            CASE WHEN n = 1 THEN 'page_id' ELSE 'block_id' END,
            CASE WHEN n = 1 THEN ? END,
            CASE WHEN n = 1 THEN NULL ELSE printf('d0000000-0000-4000-8000-%012d', n - 1) END,
            'paragraph', '{\"rich_text\":[]}', '', 'a0' FROM ids",
    )
    .bind(CHAIN_DEPTH)
    .bind(PAGE_A)
    .bind(PAGE_A)
    .execute(pool)
    .await
    .unwrap();
}

/// Adds a top-level paragraph with one child, a subtree two levels tall.
async fn append_two_level_subtree(pool: &SqlitePool) {
    writes::append_block_children(
        pool,
        NoteAppendBlockChildren {
            parent: page_parent(PAGE_A),
            after: None,
            children: vec![block(BLOCK_B, "paragraph", paragraph_payload("parent"))],
        },
    )
    .await
    .unwrap();
    writes::append_block_children(
        pool,
        NoteAppendBlockChildren {
            parent: block_parent(BLOCK_B),
            after: None,
            children: vec![block(BLOCK_C, "paragraph", paragraph_payload("child"))],
        },
    )
    .await
    .unwrap();
}

async fn block_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notes_blocks")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn parent_block_of(pool: &SqlitePool, block_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT parent_block_id FROM notes_blocks WHERE id = ?")
        .bind(block_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn nested_import_blocks(levels: usize) -> Vec<ImportBlock> {
    let mut blocks = Vec::new();
    for _ in 0..levels {
        blocks = vec![ImportBlock::new(
            "paragraph",
            paragraph_payload("level"),
            None,
            blocks,
        )];
    }
    blocks
}

fn nested_html_toggles(levels: usize) -> String {
    format!(
        "<html><body>{}{}</body></html>",
        "<details><summary>level</summary>".repeat(levels),
        "</details>".repeat(levels)
    )
}

#[test]
fn appends_fill_the_last_allowed_level_and_refuse_one_more() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        insert_paragraph_chain(&pool).await;

        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(&chain_id(CHAIN_DEPTH)),
                after: None,
                children: vec![block(BLOCK_D, "paragraph", paragraph_payload("deepest"))],
            },
        )
        .await
        .unwrap();
        let before = block_count(&pool).await;
        let error = writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_D),
                after: None,
                children: vec![block(BLOCK_E, "paragraph", paragraph_payload("too deep"))],
            },
        )
        .await
        .err()
        .unwrap();

        assert!(error.contains("nesting depth limit"), "{error}");
        assert_eq!(block_count(&pool).await, before);
    });
}

#[test]
fn moves_count_the_moved_subtree_height_against_the_limit() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        insert_paragraph_chain(&pool).await;
        append_two_level_subtree(&pool).await;
        let deepest = chain_id(CHAIN_DEPTH);

        let single = writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: block_parent(&deepest),
                after: None,
                before: None,
            },
        )
        .await
        .err()
        .unwrap();
        let batch = writes::move_blocks(
            &pool,
            NoteMoveBlocks {
                block_ids: vec![BLOCK_B.to_string()],
                parent: block_parent(&deepest),
                after: None,
                before: None,
                include_trashed_sources: None,
            },
        )
        .await
        .err()
        .unwrap();

        assert!(single.contains("nesting depth limit"), "{single}");
        assert!(batch.contains("nesting depth limit"), "{batch}");
        assert_eq!(parent_block_of(&pool, BLOCK_B).await, None);
        writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: block_parent(&deepest),
                after: None,
                before: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(parent_block_of(&pool, BLOCK_C).await, Some(deepest));
    });
}

#[test]
fn pasted_copies_count_the_copied_subtree_height_against_the_limit() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        insert_paragraph_chain(&pool).await;
        append_two_level_subtree(&pool).await;
        let before = block_count(&pool).await;

        let error = writes::duplicate_blocks(
            &pool,
            NoteDuplicateBlocks {
                block_ids: vec![BLOCK_B.to_string()],
                duplicated_block_ids: vec![
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_B.to_string(),
                        duplicate_id: BLOCK_E.to_string(),
                    },
                    NoteDuplicatedBlockId {
                        source_id: BLOCK_C.to_string(),
                        duplicate_id: BLOCK_F.to_string(),
                    },
                ],
                parent: block_parent(&chain_id(CHAIN_DEPTH)),
                after: None,
                before: None,
                include_trashed_sources: None,
            },
        )
        .await
        .err()
        .unwrap();

        assert!(error.contains("nesting depth limit"), "{error}");
        assert_eq!(block_count(&pool).await, before);
    });
}

#[test]
fn moves_refuse_a_cyclic_stored_subtree_without_running_unbounded() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        append_two_level_subtree(&pool).await;
        sqlx::query("UPDATE notes_blocks SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = ? WHERE id = ?")
            .bind(BLOCK_C)
            .bind(BLOCK_B)
            .execute(&pool)
            .await
            .unwrap();

        let error = writes::move_block(
            &pool,
            BLOCK_C,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: None,
                before: None,
            },
        )
        .await
        .err()
        .unwrap();

        assert!(error.contains("nesting depth limit"), "{error}");
    });
}

#[test]
fn imported_block_trees_are_limited_to_the_nesting_depth() {
    let limit = writes::MAX_BLOCK_DEPTH as usize;
    assert!(validate_import_block_depth(&nested_import_blocks(limit)).is_ok());
    let error = validate_import_block_depth(&nested_import_blocks(limit + 1)).unwrap_err();
    assert!(error.contains("nesting depth limit"), "{error}");
}

#[test]
fn html_imports_deeper_than_the_block_limit_fail_before_writing() {
    let limit = writes::MAX_BLOCK_DEPTH as usize;
    assert!(transfers::html_import::parse_html(&nested_html_toggles(limit), false, 5_000).is_ok());
    let error = transfers::html_import::parse_html(&nested_html_toggles(limit + 1), false, 5_000)
        .err()
        .unwrap();
    assert!(error.contains("nesting depth limit"), "{error}");
}

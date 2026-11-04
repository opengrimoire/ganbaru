//! Recursive walks over planted parent cycles, which no write path creates,
//! must end at their depth bound instead of looping.

use super::helpers::*;

async fn append_paragraph(pool: &SqlitePool, parent: NoteParent, block_id: &str) {
    writes::append_block_children(
        pool,
        NoteAppendBlockChildren {
            parent,
            after: None,
            children: vec![block(block_id, "paragraph", paragraph_payload("cycle"))],
        },
    )
    .await
    .unwrap();
}

/// Makes `BLOCK_B` and `BLOCK_C` each other's parent, detached from the page root.
async fn plant_block_cycle(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    append_paragraph(pool, page_parent(PAGE_A), BLOCK_B).await;
    append_paragraph(pool, block_parent(BLOCK_B), BLOCK_C).await;
    sqlx::query(
        "UPDATE notes_blocks
         SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = ?
         WHERE id = ?",
    )
    .bind(BLOCK_C)
    .bind(BLOCK_B)
    .execute(pool)
    .await
    .unwrap();
}

/// Makes `PAGE_A` and `PAGE_B` each other's parent page, each placed by a `child_page` block.
async fn plant_page_cycle(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    writes::create_page(
        pool,
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
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO notes_blocks (
            id, page_id, parent_type, parent_page_id, type, attrs, text, plain_text, order_key
         ) VALUES (?, ?, 'page_id', ?, 'child_page', '{}', '{}', '', 'a9')",
    )
    .bind(PAGE_A)
    .bind(PAGE_B)
    .bind(PAGE_B)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE notes_pages
         SET parent_type = 'page_id', parent_page_id = ?, child_block_id = id
         WHERE id = ?",
    )
    .bind(PAGE_B)
    .bind(PAGE_A)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn block_parent_id(pool: &SqlitePool, block_id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT parent_block_id FROM notes_blocks WHERE id = ?")
        .bind(block_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn block_walks_end_on_a_planted_parent_cycle() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        plant_block_cycle(&pool).await;

        let under_cycle = writes::move_block(
            &pool,
            BLOCK_A,
            NoteMoveBlock {
                parent: block_parent(BLOCK_C),
                after: None,
                before: None,
            },
        )
        .await;
        assert!(
            under_cycle.is_err(),
            "a cyclic ancestry exceeds the depth limit"
        );

        let cyclic_subtree = writes::move_block(
            &pool,
            BLOCK_B,
            NoteMoveBlock {
                parent: page_parent(PAGE_A),
                after: None,
                before: None,
            },
        )
        .await;
        assert!(
            cyclic_subtree.is_err(),
            "a cyclic subtree exceeds the depth limit"
        );
        assert_eq!(
            block_parent_id(&pool, BLOCK_B).await.as_deref(),
            Some(BLOCK_C)
        );
        assert_eq!(
            block_parent_id(&pool, BLOCK_C).await.as_deref(),
            Some(BLOCK_B)
        );
    });
}

#[test]
fn page_walks_end_on_a_planted_parent_cycle() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        plant_page_cycle(&pool).await;

        let location = reads::page_location(&pool, PAGE_A).await.unwrap();
        assert!(location.len() as i64 <= crate::structure::MAX_TREE_DEPTH + 1);

        let under_descendant = writes::move_page(
            &pool,
            PAGE_A,
            NoteMovePage {
                parent: page_parent(PAGE_B),
                folder_id: None,
            },
        )
        .await;
        assert!(under_descendant.is_err());

        sqlx::query("UPDATE notes_pages SET in_trash = 1 WHERE id IN (?, ?)")
            .bind(PAGE_A)
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        let deleted = writes::permanently_delete_page(&pool, PAGE_A)
            .await
            .unwrap();
        assert_eq!(deleted, vec![PAGE_A.to_string(), PAGE_B.to_string()]);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    });
}

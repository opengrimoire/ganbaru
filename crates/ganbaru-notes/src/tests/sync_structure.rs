use super::helpers::*;
use crate::sync_apply::refresh_after_apply;
use crate::sync_repair::repair_structure;
use ganbaru_sync::manifest::vault::notes::{BLOCKS_TABLE, PAGES_TABLE};

fn chain_id(level: i64) -> String {
    format!("e0000000-0000-4000-8000-{level:012}")
}

/// Stores a chain of `levels` paragraphs on `PAGE_A`, the block of level `n` at depth `n`.
async fn insert_chain(pool: &SqlitePool, levels: i64) {
    sqlx::query(
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM ids WHERE n < ?1)
         INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, parent_block_id, type, text, plain_text, order_key)
         SELECT printf('e0000000-0000-4000-8000-%012d', n), ?2,
            CASE WHEN n = 1 THEN 'page_id' ELSE 'block_id' END,
            CASE WHEN n = 1 THEN ?2 END,
            CASE WHEN n = 1 THEN NULL ELSE printf('e0000000-0000-4000-8000-%012d', n - 1) END,
            'paragraph', '{\"rich_text\":[]}', '', 'a5' FROM ids",
    )
    .bind(levels)
    .bind(PAGE_A)
    .execute(pool)
    .await
    .unwrap();
}

async fn placement(pool: &SqlitePool, id: &str) -> (Option<String>, Option<String>, String) {
    sqlx::query_as("SELECT parent_page_id, parent_block_id, page_id FROM notes_blocks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn max_block_depth(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "WITH RECURSIVE tree(id, depth) AS (
             SELECT id, 1 FROM notes_blocks WHERE parent_page_id IS NOT NULL
             UNION ALL
             SELECT child.id, tree.depth + 1 FROM notes_blocks AS child
             JOIN tree ON child.parent_block_id = tree.id
         )
         SELECT MAX(depth) FROM tree",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn repair_breaks_block_cycle_at_smallest_id_after_last_root() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("c"))],
            },
        )
        .await
        .unwrap();
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_C),
                after: None,
                children: vec![block(BLOCK_D, "paragraph", paragraph_payload("d"))],
            },
        )
        .await
        .unwrap();
        // A merge that nests C under D while D stays under C.
        sqlx::query("UPDATE notes_blocks SET parent_block_id = ? WHERE id = ?")
            .bind(BLOCK_D)
            .bind(BLOCK_C)
            .execute(&pool)
            .await
            .unwrap();

        let report = repair_structure(&pool, &[], &[BLOCK_C.to_string()])
            .await
            .unwrap();

        assert_eq!(report.cycle_blocks, vec![BLOCK_C.to_string()]);
        assert!(report.unplaced_blocks.is_empty());
        assert_eq!(
            placement(&pool, BLOCK_C).await,
            (Some(PAGE_A.to_string()), None, PAGE_A.to_string())
        );
        assert_eq!(
            placement(&pool, BLOCK_D).await,
            (None, Some(BLOCK_C.to_string()), PAGE_A.to_string())
        );
        let roots: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM notes_blocks WHERE parent_page_id = ? ORDER BY order_key, id",
        )
        .bind(PAGE_A)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(roots, vec![BLOCK_A.to_string(), BLOCK_C.to_string()]);
        let queued: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_page_history_pending WHERE page_id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(queued, 1);
        // A second pass finds nothing left to repair.
        let again = repair_structure(&pool, &[], &[BLOCK_C.to_string(), BLOCK_D.to_string()])
            .await
            .unwrap();
        assert!(again.is_empty());
    });
}

#[test]
fn repair_leaves_a_cycle_without_a_known_page_unplaced() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        insert_chain(&pool, 2).await;
        sqlx::query(
            "UPDATE notes_blocks
             SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = ?, page_id = ''
             WHERE id = ?",
        )
        .bind(chain_id(2))
        .bind(chain_id(1))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE notes_blocks SET page_id = '' WHERE id = ?")
            .bind(chain_id(2))
            .execute(&pool)
            .await
            .unwrap();

        let report = repair_structure(&pool, &[], &[chain_id(2)]).await.unwrap();

        assert!(report.cycle_blocks.is_empty());
        assert_eq!(report.unplaced_blocks, vec![chain_id(1)]);
        assert_eq!(
            placement(&pool, &chain_id(1)).await,
            (None, Some(chain_id(2)), String::new())
        );
    });
}

#[test]
fn repair_lifts_the_block_past_the_depth_bound_to_its_page_root() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        let levels = writes::MAX_BLOCK_DEPTH + 2;
        insert_chain(&pool, levels).await;

        let report = repair_structure(&pool, &[], &[chain_id(levels)])
            .await
            .unwrap();

        let lifted = chain_id(writes::MAX_BLOCK_DEPTH + 1);
        assert_eq!(report.deep_blocks, vec![lifted.clone()]);
        assert_eq!(
            placement(&pool, &lifted).await,
            (Some(PAGE_A.to_string()), None, PAGE_A.to_string())
        );
        assert_eq!(
            placement(&pool, &chain_id(levels)).await,
            (None, Some(lifted), PAGE_A.to_string())
        );
        assert_eq!(max_block_depth(&pool).await, writes::MAX_BLOCK_DEPTH);
    });
}

#[test]
fn repair_moves_the_smallest_page_of_a_nesting_cycle_to_the_workspace() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        writes::move_page(
            &pool,
            PAGE_B,
            NoteMovePage {
                parent: page_parent(PAGE_A),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        // A merge that also nests A in B through a child page block of its own.
        sqlx::query(
            "INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, type, plain_text, order_key)
             VALUES (?1, ?2, 'page_id', ?2, 'child_page', 'First page', 'az')",
        )
        .bind(PAGE_A)
        .bind(PAGE_B)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE notes_pages SET child_block_id = id WHERE id = ?")
            .bind(PAGE_A)
            .execute(&pool)
            .await
            .unwrap();

        let report = repair_structure(&pool, &[PAGE_A.to_string()], &[])
            .await
            .unwrap();

        assert_eq!(report.detached_pages, vec![PAGE_A.to_string()]);
        let page_a: (String, Option<String>) =
            sqlx::query_as("SELECT parent_type, child_block_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_A)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(page_a, ("workspace".to_string(), None));
        let block_a: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_blocks WHERE id = ?")
            .bind(PAGE_A)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(block_a, 0);
        let page_b: (String, Option<String>) =
            sqlx::query_as("SELECT parent_type, parent_page_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(page_b, ("page_id".to_string(), Some(PAGE_A.to_string())));
    });
}

#[test]
fn sync_hook_recomputes_derived_columns_queues_history_and_drops_index_fingerprints() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        writes::append_block_children(
            &pool,
            NoteAppendBlockChildren {
                parent: block_parent(BLOCK_A),
                after: None,
                children: vec![block(BLOCK_C, "paragraph", paragraph_payload("hello sync"))],
            },
        )
        .await
        .unwrap();
        writes::move_page(
            &pool,
            PAGE_B,
            NoteMovePage {
                parent: page_parent(PAGE_A),
                folder_id: None,
            },
        )
        .await
        .unwrap();
        search::search(&pool, "hello", None, false).await.unwrap();
        let expected_block: (String, String) =
            sqlx::query_as("SELECT page_id, plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        let expected_page: (String, Option<String>) =
            sqlx::query_as("SELECT parent_type, parent_page_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        // A received row carries only replicated columns; hook columns hold their defaults.
        sqlx::query("UPDATE notes_blocks SET page_id = '', plain_text = '' WHERE id = ?")
            .bind(BLOCK_C)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE notes_pages SET parent_type = 'workspace', parent_page_id = NULL WHERE id = ?",
        )
        .bind(PAGE_B)
        .execute(&pool)
        .await
        .unwrap();

        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, BLOCKS_TABLE, &[BLOCK_C.to_string()])
            .await
            .unwrap();
        refresh_after_apply(&mut conn, PAGES_TABLE, &[PAGE_B.to_string()])
            .await
            .unwrap();
        drop(conn);

        let block_row: (String, String) =
            sqlx::query_as("SELECT page_id, plain_text FROM notes_blocks WHERE id = ?")
                .bind(BLOCK_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(block_row, expected_block);
        let page_row: (String, Option<String>) =
            sqlx::query_as("SELECT parent_type, parent_page_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(page_row, expected_page);
        let queued: Vec<String> =
            sqlx::query_scalar("SELECT page_id FROM notes_page_history_pending ORDER BY page_id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(queued, vec![PAGE_A.to_string(), PAGE_B.to_string()]);
        let fingerprints: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_search_index_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fingerprints, 0);

        let flushed = page_history::flush_pending_remote_snapshots(&pool)
            .await
            .unwrap();
        assert_eq!(flushed, 2);
        let reasons: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT reason FROM notes_page_history_snapshots WHERE page_id IN (?, ?)
               AND reason = ?",
        )
        .bind(PAGE_A)
        .bind(PAGE_B)
        .bind(page_history::REMOTE_CHANGE_REASON)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            reasons,
            vec![page_history::REMOTE_CHANGE_REASON.to_string()]
        );
    });
}

#[test]
fn removing_a_database_block_removes_its_row_pages_through_nested_databases() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_database(
            &pool,
            DATABASE_A,
            DATA_SOURCE_A,
            DATABASE_VIEW_A,
            "Outer",
            BLOCK_A,
        )
        .await;
        insert_row_page(&pool, PAGE_B, DATA_SOURCE_A, "Row", json!({})).await;
        databases::create_database(
            &pool,
            NoteDatabaseCreate {
                id: DATABASE_B.to_string(),
                data_source_id: DATA_SOURCE_B.to_string(),
                view_id: DATABASE_VIEW_B.to_string(),
                title: "Inner".to_string(),
                parent: Some(page_parent(PAGE_B)),
                after_block_id: None,
                replace_block_id: None,
                icon: None,
                cover: None,
            },
        )
        .await
        .unwrap();
        insert_row_page(&pool, PAGE_C, DATA_SOURCE_B, "Inner row", json!({})).await;

        // A received block delete purges the block the way a sync apply does.
        sqlx::query("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM notes_blocks WHERE id = ?")
            .bind(DATABASE_A)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
            .execute(&pool)
            .await
            .unwrap();

        let pages: Vec<String> = sqlx::query_scalar("SELECT id FROM notes_pages ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(pages, vec![PAGE_A.to_string()]);
        let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM notes_data_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sources, 0);
    });
}

#[test]
fn a_row_page_for_an_unknown_data_source_is_not_listed_or_counted() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        for (id, in_trash) in [(PAGE_B, 0_i64), (PAGE_C, 1_i64)] {
            sqlx::query(
                "INSERT INTO notes_pages
                     (id, parent_type, parent_data_source_id, title, in_trash, trashed_time)
                 VALUES (?1, 'data_source_id', 'absent-source', 'Unplaced', ?2,
                         CASE WHEN ?2 = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END)",
            )
            .bind(id)
            .bind(in_trash)
            .execute(&pool)
            .await
            .unwrap();
        }

        let listed = serde_json::to_value(reads::list_pages(&pool).await.unwrap()).unwrap();
        assert_eq!(listed.as_array().map(Vec::len), Some(1));
        assert_eq!(listed[0]["id"], PAGE_A);
        let trash = serde_json::to_value(
            reads::list_trashed_page_window(
                &pool,
                crate::models::NotePageSummaryWindowRequest {
                    cursor: None,
                    query: None,
                    page_size: Some(25),
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(trash["total_count"], 0);
        assert_eq!(trash["pages"], json!([]));
        for destination_candidates in [false, true] {
            let request = serde_json::from_value(json!({
                "project_id": null,
                "destination_candidates": destination_candidates,
            }))
            .unwrap();
            let shell = serde_json::to_value(
                workspace_shell::load_workspace_shell(&pool, request)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(shell["total_page_count"], 1);
            let ids: Vec<&str> = shell["pages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|page| page["id"].as_str())
                .collect();
            assert_eq!(ids, vec![PAGE_A]);
        }
    });
}

#[test]
fn purging_a_former_parent_keeps_a_page_moved_out_of_it() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        create_page(&pool, PAGE_A, BLOCK_A).await;
        create_page(&pool, PAGE_B, BLOCK_B).await;
        create_page(&pool, PAGE_C, BLOCK_C).await;
        let to = |parent: NoteParent| NoteMovePage {
            parent,
            folder_id: None,
        };
        writes::move_page(&pool, PAGE_B, to(page_parent(PAGE_A)))
            .await
            .unwrap();
        writes::move_page(&pool, PAGE_B, to(workspace_parent()))
            .await
            .unwrap();
        writes::trash_page(&pool, PAGE_A, true).await.unwrap();
        let deleted = writes::permanently_delete_page(&pool, PAGE_A)
            .await
            .unwrap();

        assert_eq!(deleted, vec![PAGE_A.to_string()]);
        let page_b: i64 = sqlx::query_scalar("SELECT in_trash FROM notes_pages WHERE id = ?")
            .bind(PAGE_B)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(page_b, 0);
        writes::move_page(&pool, PAGE_B, to(page_parent(PAGE_C)))
            .await
            .unwrap();
        let block: (String, i64) =
            sqlx::query_as("SELECT page_id, in_trash FROM notes_blocks WHERE id = ?")
                .bind(PAGE_B)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(block, (PAGE_C.to_string(), 0));
    });
}

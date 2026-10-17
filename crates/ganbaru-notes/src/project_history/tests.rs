use super::{
    checkpoint_is_due, create_checkpoint, create_checkpoint_after_graph_load,
    ensure_project_baseline_for_mutation, flush_due_checkpoints, history_schedule,
    initialize_project_history, mark_project_dirty_tx, mutation_result, restore,
};
use ganbaru_db::run_migrations;
use sqlx::SqlitePool;

const PROJECT_ID: &str = "10101010-1010-4010-8010-101010101010";
const PAGE_ID: &str = "20202020-2020-4020-8020-202020202020";
const BLOCK_ID: &str = "30303030-3030-4030-8030-303030303030";
const LATER_PAGE_ID: &str = "40404040-4040-4040-8040-404040404040";
const LATER_BLOCK_ID: &str = "50505050-5050-4050-8050-505050505050";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}

async fn seed_project(pool: &SqlitePool) {
    sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group-history', 'History')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name) VALUES (?, 'group-history', 'Learning')",
    )
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    insert_project_page(pool, PAGE_ID, BLOCK_ID, "First version").await;
}

async fn seed_empty_project(pool: &SqlitePool) {
    sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group-history', 'History')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name) VALUES (?, 'group-history', 'Learning')",
    )
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_project_page(pool: &SqlitePool, page_id: &str, block_id: &str, text: &str) {
    sqlx::query(
        "INSERT INTO notes_pages (id, parent_type, title, properties)
         VALUES (?, 'workspace', ?, json_object('__ganbaru_project_id', ?, 'title', json_object()))",
    )
    .bind(page_id)
    .bind(text)
    .bind(PROJECT_ID)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notes_blocks (
            id, page_id, parent_type, parent_page_id, type, payload, plain_text, sort_order
         ) VALUES (?, ?, 'page_id', ?, 'paragraph', ?, ?, 1000)",
    )
    .bind(block_id)
    .bind(page_id)
    .bind(page_id)
    .bind(
        serde_json::json!({
            "paragraph": {
                "rich_text": [{
                    "type": "text",
                    "text": { "content": text, "link": null },
                    "annotations": {
                        "bold": false,
                        "italic": false,
                        "strikethrough": false,
                        "underline": false,
                        "code": false,
                        "color": "default"
                    },
                    "plain_text": text,
                    "href": null
                }],
                "color": "default"
            }
        })
        .to_string(),
    )
    .bind(text)
    .execute(pool)
    .await
    .unwrap();
}

mod checkpoint;
mod contracts;
mod read_projection;
mod restore_adapter;
mod retention;
mod row_hierarchy;
mod schedule;

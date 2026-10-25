use super::helpers::*;
use crate::sync_apply::refresh_after_apply;
use ganbaru_sync::manifest::vault::notes::{
    BLOCKS_TABLE, DATABASES_TABLE, PROPERTIES_TABLE, RELATION_EDGES_TABLE, VALUES_TABLE,
};

async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(pool)
        .await
        .unwrap();
}

/// A database on `PAGE_A` after `BLOCK_A`, with one row page `PAGE_B`.
async fn database_with_row(pool: &SqlitePool) {
    create_page(pool, PAGE_A, BLOCK_A).await;
    create_database(
        pool,
        DATABASE_A,
        DATA_SOURCE_A,
        DATABASE_VIEW_A,
        "Tasks",
        BLOCK_A,
    )
    .await;
    insert_row_page(pool, PAGE_B, DATA_SOURCE_A, "Row", json!({})).await;
}

async fn database_parent(pool: &SqlitePool) -> (String, Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT parent_type, parent_page_id, parent_block_id FROM notes_databases WHERE id = ?",
    )
    .bind(DATABASE_A)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn sync_hook_moves_database_parents_with_their_block() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        set_applying(&pool, true).await;
        sqlx::query(
            "UPDATE notes_blocks
             SET parent_type = 'block_id', parent_page_id = NULL, parent_block_id = ?
             WHERE id = ?",
        )
        .bind(BLOCK_A)
        .bind(DATABASE_A)
        .execute(&pool)
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, BLOCKS_TABLE, &[DATABASE_A.to_string()])
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            database_parent(&pool).await,
            ("block_id".to_string(), None, Some(BLOCK_A.to_string()))
        );

        // A received database row starts with the default parent columns.
        sqlx::query(
            "UPDATE notes_databases
             SET parent_type = 'page_id', parent_page_id = NULL, parent_block_id = NULL",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, DATABASES_TABLE, &[DATABASE_A.to_string()])
            .await
            .unwrap();
        drop(conn);
        set_applying(&pool, false).await;
        assert_eq!(
            database_parent(&pool).await,
            ("block_id".to_string(), None, Some(BLOCK_A.to_string()))
        );
    });
}

async fn cache_rollup(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO notes_data_source_rollup_cache
             (source_page_id, source_data_source_id, source_property_id, source_property_name,
              relation_property_id, rollup_property_id, target_data_source_id, value)
         VALUES (?1, ?2, 'rollup', 'Rollup', 'relation', 'rollup', ?2, '{}')",
    )
    .bind(PAGE_B)
    .bind(DATA_SOURCE_A)
    .execute(pool)
    .await
    .unwrap();
}

async fn cached_rollups(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notes_data_source_rollup_cache")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn sync_hook_forgets_rollups_that_read_changed_rows_including_removed_ones() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        let value_key = crate::sync_keys::PROPERTY_VALUES
            .key(&[PAGE_B, "notes"])
            .unwrap();
        sqlx::query(
            "INSERT INTO notes_page_property_values
                 (page_id, property_id, sync_key, value_type, value)
             VALUES (?, 'notes', ?, 'rich_text', '{\"rich_text\":[]}')",
        )
        .bind(PAGE_B)
        .bind(&value_key)
        .execute(&pool)
        .await
        .unwrap();

        cache_rollup(&pool).await;
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, VALUES_TABLE, std::slice::from_ref(&value_key))
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            cached_rollups(&pool).await,
            0,
            "a changed value of the source"
        );

        cache_rollup(&pool).await;
        let gone = crate::sync_keys::RELATION_EDGES
            .key(&[PAGE_B, "relation", PAGE_A])
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, RELATION_EDGES_TABLE, &[gone])
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            cached_rollups(&pool).await,
            0,
            "a removed edge no longer names its source, so every rollup is forgotten"
        );

        cache_rollup(&pool).await;
        let title_key = crate::sync_keys::PROPERTIES
            .key(&[DATA_SOURCE_A, "title"])
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, PROPERTIES_TABLE, &[title_key])
            .await
            .unwrap();
        drop(conn);
        assert_eq!(
            cached_rollups(&pool).await,
            0,
            "a changed property of the source"
        );
    });
}

#[test]
fn sync_hook_refreshes_date_property_reminders_and_settles_past_ones() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                    "Due": { "id": "due", "name": "Due", "type": "date", "date": {} }
                }),
            },
        )
        .await
        .unwrap();
        let value_key = crate::sync_keys::PROPERTY_VALUES
            .key(&[PAGE_B, "due"])
            .unwrap();
        let date = |start: &str| {
            json!({ "start": start, "ganbaru_reminder": { "enabled": true } }).to_string()
        };
        set_applying(&pool, true).await;
        sqlx::query(
            "INSERT INTO notes_page_property_values
                 (page_id, property_id, sync_key, value_type, value)
             VALUES (?, 'due', ?, 'date', ?)",
        )
        .bind(PAGE_B)
        .bind(&value_key)
        .bind(date("2001-01-01T09:00:00.000Z"))
        .execute(&pool)
        .await
        .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, VALUES_TABLE, std::slice::from_ref(&value_key))
            .await
            .unwrap();
        drop(conn);
        let states = || async {
            sqlx::query_scalar::<_, String>(
                "SELECT status FROM notes_mention_notifications
                 WHERE source_type = 'page_property' AND source_id = ?",
            )
            .bind(PAGE_B)
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        assert_eq!(
            states().await,
            vec!["delivered".to_string()],
            "a reminder first seen after its time never notifies"
        );

        sqlx::query("UPDATE notes_page_property_values SET value = ? WHERE sync_key = ?")
            .bind(date("2999-01-01T09:00:00.000Z"))
            .bind(&value_key)
            .execute(&pool)
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, VALUES_TABLE, std::slice::from_ref(&value_key))
            .await
            .unwrap();
        drop(conn);
        assert!(states().await.contains(&"pending".to_string()));

        sqlx::query("DELETE FROM notes_page_property_values WHERE sync_key = ?")
            .bind(&value_key)
            .execute(&pool)
            .await
            .unwrap();
        let mut conn = pool.acquire().await.unwrap();
        refresh_after_apply(&mut conn, VALUES_TABLE, std::slice::from_ref(&value_key))
            .await
            .unwrap();
        drop(conn);
        set_applying(&pool, false).await;
        assert!(
            !states().await.contains(&"pending".to_string()),
            "a removed value clears its pending reminder"
        );
    });
}

#[test]
fn property_names_repeated_by_a_merge_read_apart_and_survive_the_next_write() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        // Two devices each added a property named Owner.
        for property_id in ["owner-b", "owner-a"] {
            sqlx::query(
                "INSERT INTO notes_data_source_properties
                     (data_source_id, property_id, sync_key, name, type, config, order_key)
                 VALUES (?, ?, ?, 'Owner', 'rich_text', '{}', 'a5')",
            )
            .bind(DATA_SOURCE_A)
            .bind(property_id)
            .bind(
                crate::sync_keys::PROPERTIES
                    .key(&[DATA_SOURCE_A, property_id])
                    .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        let mut conn = pool.acquire().await.unwrap();
        let schema = crate::property_storage::load_data_source_properties(&mut conn, DATA_SOURCE_A)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(schema["Owner"]["id"], "owner-a");
        assert_eq!(schema["Owner (owner-b)"]["id"], "owner-b");
        assert_eq!(schema["Owner (owner-b)"]["name"], "Owner (owner-b)");

        crate::property_storage::write_data_source_properties(&mut conn, DATA_SOURCE_A, &schema)
            .await
            .unwrap();
        let stored: Vec<(String, String)> = sqlx::query_as(
            "SELECT property_id, name FROM notes_data_source_properties
             WHERE property_id LIKE 'owner-%' ORDER BY property_id",
        )
        .fetch_all(&mut *conn)
        .await
        .unwrap();
        assert_eq!(
            stored,
            vec![
                ("owner-a".to_string(), "Owner".to_string()),
                ("owner-b".to_string(), "Owner (owner-b)".to_string()),
            ],
            "the write keeps both properties and stores the name reads showed"
        );
    });
}

#[test]
fn options_name_their_property_key_and_go_with_the_property() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        data_sources::schema::update_data_source_schema(
            &pool,
            DATA_SOURCE_A,
            None,
            None,
            NoteDataSourceSchemaUpdate {
                properties: json!({
                    "Name": { "id": "title", "name": "Name", "type": "title", "title": {} },
                    "Stage": {
                        "id": "stage", "name": "Stage", "type": "select",
                        "select": { "options": [{ "id": "todo", "name": "To do", "color": "gray" }] }
                    }
                }),
            },
        )
        .await
        .unwrap();
        let property_key: String = sqlx::query_scalar(
            "SELECT property_key FROM notes_data_source_property_options WHERE option_id = 'todo'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            property_key,
            crate::sync_keys::PROPERTIES
                .key(&[DATA_SOURCE_A, "stage"])
                .unwrap()
        );
        sqlx::query("DELETE FROM notes_data_source_properties WHERE property_id = 'stage'")
            .execute(&pool)
            .await
            .unwrap();
        let options: i64 =
            sqlx::query_scalar("SELECT count(*) FROM notes_data_source_property_options")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(options, 0);
    });
}

#[test]
fn view_reorder_rewrites_only_the_order_keys_that_must_move() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        let mut ids = vec![DATABASE_VIEW_A.to_string()];
        for index in 1..=2 {
            let id = format!("98989898-9898-4989-8989-{index:012}");
            databases::view_management::duplicate_database_view(
                &pool,
                serde_json::from_value(json!({
                    "id": id,
                    "database_id": DATABASE_A,
                    "source_view_id": DATABASE_VIEW_A,
                    "name": format!("Copy {index}"),
                }))
                .unwrap(),
            )
            .await
            .unwrap();
            ids.push(id);
        }
        let keys = || async {
            sqlx::query_as::<_, (String, String)>(
                "SELECT id, order_key FROM notes_database_views ORDER BY order_key, id",
            )
            .fetch_all(&pool)
            .await
            .unwrap()
        };
        let before = keys().await;
        assert_eq!(
            before.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
            ids
        );
        let reordered = vec![ids[0].clone(), ids[2].clone(), ids[1].clone()];
        let views =
            databases::view_management::reorder_database_views(&pool, DATABASE_A, &reordered)
                .await
                .unwrap();
        let listed: Vec<String> = serde_json::to_value(views)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|view| view["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(listed, reordered);
        let after = keys().await;
        let moved = before
            .iter()
            .filter(|(id, key)| after.iter().any(|(other, next)| other == id && next != key))
            .count();
        assert_eq!(moved, 1, "one view moves, the others keep their keys");
    });
}

#[test]
fn template_saves_refuse_blocks_with_missing_parents_or_too_many_blocks() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query("INSERT INTO notes_page_templates (id, name) VALUES (?, 'Template')")
            .bind(TEMPLATE_A)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO notes_page_template_blocks
                 (template_id, ordinal, id, parent_type, parent_block_id, type, payload)
             VALUES (?, 0, 'orphan', 'block_id', 'missing', 'paragraph', '{}')",
        )
        .bind(TEMPLATE_A)
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let refusal = crate::templates::ensure_template_content_size_tx(
            &mut tx,
            crate::templates::TemplateKind::Page,
            TEMPLATE_A,
        )
        .await
        .unwrap_err();
        assert!(refusal.contains("parents inside the template"), "{refusal}");
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?2)
             INSERT OR REPLACE INTO notes_page_template_blocks
                 (template_id, ordinal, id, parent_type, type, payload)
             SELECT ?1, i, 'block-' || i, 'template', 'paragraph', '{}' FROM n",
        )
        .bind(TEMPLATE_A)
        .bind(crate::templates::MAX_TEMPLATE_BLOCKS as i64)
        .execute(&mut *tx)
        .await
        .unwrap();
        let refusal = crate::templates::ensure_template_content_size_tx(
            &mut tx,
            crate::templates::TemplateKind::Page,
            TEMPLATE_A,
        )
        .await
        .unwrap_err();
        assert!(refusal.contains("block limit"), "{refusal}");
    });
}

#[test]
fn repair_clears_a_row_parent_left_in_another_data_source() {
    crate::test_block_on(async {
        let pool = migrated_memory_pool().await;
        database_with_row(&pool).await;
        create_database(
            &pool,
            DATABASE_B,
            DATA_SOURCE_B,
            DATABASE_VIEW_B,
            "Other",
            DATABASE_A,
        )
        .await;
        insert_row_page(&pool, PAGE_C, DATA_SOURCE_A, "Child", json!({})).await;
        sqlx::query("UPDATE notes_pages SET parent_row_page_id = ? WHERE id = ?")
            .bind(PAGE_B)
            .bind(PAGE_C)
            .execute(&pool)
            .await
            .unwrap();
        // A merge moved the parent row to another data source while its child stayed.
        set_applying(&pool, true).await;
        sqlx::query("UPDATE notes_pages SET parent_data_source_id = ? WHERE id = ?")
            .bind(DATA_SOURCE_B)
            .bind(PAGE_B)
            .execute(&pool)
            .await
            .unwrap();
        set_applying(&pool, false).await;

        let report = crate::sync_repair::repair_structure(&pool, &[PAGE_B.to_string()], &[])
            .await
            .unwrap();

        assert_eq!(report.detached_rows, vec![PAGE_C.to_string()]);
        let parent: Option<String> =
            sqlx::query_scalar("SELECT parent_row_page_id FROM notes_pages WHERE id = ?")
                .bind(PAGE_C)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(parent, None);
        let again = crate::sync_repair::repair_structure(
            &pool,
            &[PAGE_B.to_string(), PAGE_C.to_string()],
            &[],
        )
        .await
        .unwrap();
        assert!(again.is_empty(), "the repair reaches a fixed point");
    });
}

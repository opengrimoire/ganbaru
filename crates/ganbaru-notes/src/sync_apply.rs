//! Notes work that runs inside the sync engine transaction after rows of a Notes table
//! materialize.
//!
//! The hook receives only the connection and a change sink, so the desktop service and the
//! headless Android pass run the same code. For the changed keys of an apply it fills the
//! hook-derived columns (a page's `parent_type`, `parent_page_id`, and `parent_block_id`, a
//! block's `page_id` and `plain_text`), refreshes the reminder candidates of changed blocks,
//! invalidates the fingerprints of the search and link indexes so the next read rebuilds them,
//! queues history snapshots of the touched pages in `notes_page_history_pending`, queues the
//! changed page and block keys for structure repair in `notes_sync_repair_pending`, and reports
//! every changed row under its table name. It writes only device-local rows and hook-derived
//! columns, which sync never captures.

use crate::block_storage::compose_block_content;
use crate::validation::plain_text_from_payload;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::notes::{BLOCKS_TABLE, DOMAIN, PAGES_TABLE};
use ganbaru_sync::{ChangeSink, HookError, HookFuture, Hooks, MaterializeHook};
use serde_json::Value;
use sqlx::{Connection, SqliteConnection};
use std::collections::BTreeSet;

/// Index fingerprint tables whose rows the hook deletes so the next read rebuilds the index.
const INDEX_STATE_TABLES: [&str; 4] = [
    "notes_search_index_state",
    "notes_backlink_index_state",
    "notes_unresolved_link_index_state",
    "notes_link_facts_state",
];

/// Block types whose plain text is a title stored outside the block.
const TITLED_BLOCK_TYPES: [&str; 2] = ["child_page", "child_database"];

/// Refreshes derived Notes state after sync materializes Notes rows.
struct NotesHook;

impl MaterializeHook for NotesHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            refresh_after_apply(conn, table.id, changed_keys)
                .await
                .map_err(HookError::from)?;
            for key in changed_keys {
                changes.report(table.name, key.clone());
            }
            Ok(())
        })
    }
}

/// `hooks` with the Notes post-apply hook registered for every replicated Notes table.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(NotesHook))
    })
}

/// Whether a change notice topic names a replicated Notes table.
pub fn is_notes_topic(topic: &str) -> bool {
    DOMAIN.tables.iter().any(|table| table.name == topic)
}

/// Recomputes what Notes derives from the rows of table `table` whose keys are in `keys`.
pub async fn refresh_after_apply(
    conn: &mut SqliteConnection,
    table: ganbaru_sync_contracts::TableId,
    keys: &[String],
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    if table == PAGES_TABLE {
        refresh_pages(conn, keys).await?;
        queue_repair(conn, "page", keys).await?;
    } else if table == BLOCKS_TABLE {
        refresh_blocks(conn, keys).await?;
        queue_repair(conn, "block", keys).await?;
    }
    invalidate_indexes(conn).await
}

/// Records `keys` of `kind` (`page` or `block`) for the Notes structure repair, which clears
/// them in the transaction that repairs them.
async fn queue_repair(
    conn: &mut SqliteConnection,
    kind: &str,
    keys: &[String],
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO notes_sync_repair_pending (kind, row_key)
         SELECT ?1, value FROM json_each(?2) WHERE trim(value) <> ''
         ON CONFLICT (kind, row_key) DO NOTHING",
    )
    .bind(kind)
    .bind(keys_json(keys)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("queue Notes structure repair: {error}"))?;
    Ok(())
}

fn keys_json(keys: &[String]) -> Result<String, String> {
    serde_json::to_string(keys).map_err(|error| format!("encode Notes sync keys: {error}"))
}

/// Sets the parent columns of `page_ids` from replicated placement: a row page sits in its data
/// source, a nested page under the parent of its `child_page` block, and every other page in the
/// workspace. Pages whose columns already agree are left alone.
pub async fn refresh_page_parents(
    conn: &mut SqliteConnection,
    page_ids: &[String],
) -> Result<(), String> {
    sqlx::query(
        "WITH target(id) AS (SELECT value FROM json_each(?1)),
         placed(id, kind, parent_page, parent_block) AS (
             SELECT page.id,
                 CASE WHEN page.parent_data_source_id IS NOT NULL THEN 'data_source_id'
                      WHEN block.id IS NULL THEN 'workspace'
                      WHEN block.parent_page_id IS NOT NULL THEN 'page_id'
                      ELSE 'block_id' END,
                 CASE WHEN page.parent_data_source_id IS NULL THEN block.parent_page_id END,
                 CASE WHEN page.parent_data_source_id IS NULL THEN block.parent_block_id END
             FROM notes_pages AS page
             JOIN target ON target.id = page.id
             LEFT JOIN notes_blocks AS block ON block.id = page.child_block_id
         )
         UPDATE notes_pages
         SET parent_type = placed.kind,
             parent_page_id = placed.parent_page,
             parent_block_id = placed.parent_block
         FROM placed
         WHERE notes_pages.id = placed.id
           AND (notes_pages.parent_type IS NOT placed.kind
                OR notes_pages.parent_page_id IS NOT placed.parent_page
                OR notes_pages.parent_block_id IS NOT placed.parent_block)",
    )
    .bind(keys_json(page_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("refresh synced Notes page parents: {error}"))?;
    Ok(())
}

async fn refresh_pages(conn: &mut SqliteConnection, page_ids: &[String]) -> Result<(), String> {
    refresh_page_parents(conn, page_ids).await?;
    sqlx::query(
        "UPDATE notes_blocks
         SET plain_text = (SELECT title FROM notes_pages WHERE notes_pages.id = notes_blocks.id)
         WHERE type = 'child_page'
           AND id IN (SELECT value FROM json_each(?1))
           AND EXISTS (SELECT 1 FROM notes_pages WHERE notes_pages.id = notes_blocks.id)
           AND plain_text IS NOT (SELECT title FROM notes_pages WHERE notes_pages.id = notes_blocks.id)",
    )
    .bind(keys_json(page_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("refresh synced Notes child page titles: {error}"))?;
    let mut tx = conn
        .begin()
        .await
        .map_err(|error| format!("begin synced Notes page refresh: {error}"))?;
    for page_id in page_ids {
        crate::data_sources::rollups::invalidate_rollup_cache_for_page_tx(&mut tx, page_id).await?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit synced Notes page refresh: {error}"))?;
    queue_page_history(conn, page_ids).await
}

/// Sets `page_id` of `block_ids` and every block nested under them to the page their ancestry
/// reaches. A block whose ancestry reaches no page within the depth bound, because it is part of
/// a cycle or nested too deep, keeps its value until the structure repair places it.
pub async fn refresh_block_pages(
    conn: &mut SqliteConnection,
    block_ids: &[String],
) -> Result<(), String> {
    sqlx::query(
        "WITH RECURSIVE
         changed(id) AS (SELECT value FROM json_each(?1)),
         affected(id, depth) AS (
             SELECT id, 0 FROM notes_blocks WHERE id IN changed
             UNION
             SELECT child.id, affected.depth + 1
             FROM notes_blocks AS child
             JOIN affected ON child.parent_block_id = affected.id
             WHERE affected.depth < ?2
         ),
         up(start, parent_page, parent_block, depth) AS (
             SELECT id, parent_page_id, parent_block_id, 0
             FROM notes_blocks WHERE id IN (SELECT id FROM affected)
             UNION
             SELECT up.start, parent.parent_page_id, parent.parent_block_id, up.depth + 1
             FROM up
             JOIN notes_blocks AS parent ON parent.id = up.parent_block
             WHERE up.depth < ?2
         ),
         resolved(id, page_id) AS (
             SELECT start, MIN(parent_page) FROM up
             WHERE parent_page IS NOT NULL
             GROUP BY start
         )
         UPDATE notes_blocks
         SET page_id = resolved.page_id
         FROM resolved
         WHERE notes_blocks.id = resolved.id AND notes_blocks.page_id <> resolved.page_id",
    )
    .bind(keys_json(block_ids)?)
    .bind(crate::writes::MAX_BLOCK_DEPTH)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("refresh synced Notes block pages: {error}"))?;
    Ok(())
}

/// One changed block's stored content.
#[derive(sqlx::FromRow)]
struct ChangedBlock {
    id: String,
    page_id: String,
    #[sqlx(rename = "type")]
    block_type: String,
    attrs: String,
    text: String,
    plain_text: String,
    title: Option<String>,
}

async fn refresh_blocks(conn: &mut SqliteConnection, block_ids: &[String]) -> Result<(), String> {
    refresh_block_pages(conn, block_ids).await?;
    let blocks: Vec<ChangedBlock> = sqlx::query_as(
        "SELECT block.id, block.page_id, block.type, block.attrs, block.text, block.plain_text,
                page.title
         FROM notes_blocks AS block
         LEFT JOIN notes_pages AS page ON page.id = block.id AND block.type = 'child_page'
         WHERE block.id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(block_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load synced Notes blocks: {error}"))?;
    let present: BTreeSet<&str> = blocks.iter().map(|block| block.id.as_str()).collect();
    let mut tx = conn
        .begin()
        .await
        .map_err(|error| format!("begin synced Notes block refresh: {error}"))?;
    for block_id in block_ids {
        if !present.contains(block_id.as_str()) {
            crate::mention_notifications::clear_source_tx(&mut tx, "block", block_id).await?;
        }
    }
    let known_reminders = reminder_ids(&mut tx, block_ids).await?;
    let mut pages = BTreeSet::new();
    for block in &blocks {
        let mut payload = compose_block_content(&block.block_type, &block.attrs, &block.text)?;
        if let (Some(title), Value::Object(fields)) = (&block.title, &mut payload) {
            fields.insert("title".to_string(), Value::String(title.clone()));
        }
        let titled = TITLED_BLOCK_TYPES.contains(&block.block_type.as_str());
        let plain_text = if titled && block.title.is_none() {
            block.plain_text.clone()
        } else {
            plain_text_from_payload(&block.block_type, &payload)
        };
        if plain_text != block.plain_text {
            sqlx::query("UPDATE notes_blocks SET plain_text = ? WHERE id = ?")
                .bind(&plain_text)
                .bind(&block.id)
                .execute(&mut *tx)
                .await
                .map_err(|error| format!("refresh synced Notes block text: {error}"))?;
        }
        if block.page_id.is_empty() {
            continue;
        }
        crate::mention_notifications::sync_block_tx(
            &mut tx,
            &block.id,
            &block.page_id,
            &block.block_type,
            &payload,
            &plain_text,
        )
        .await?;
        pages.insert(block.page_id.clone());
    }
    settle_past_reminders(&mut tx, block_ids, &known_reminders).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit synced Notes block refresh: {error}"))?;
    let nested_pages: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM notes_pages WHERE child_block_id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(block_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load pages of synced Notes blocks: {error}"))?;
    refresh_page_parents(conn, &nested_pages).await?;
    queue_page_history(conn, &pages.into_iter().collect::<Vec<_>>()).await
}

/// Ids of the reminder candidates of `block_ids` that exist before a refresh.
async fn reminder_ids(
    conn: &mut SqliteConnection,
    block_ids: &[String],
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT id FROM notes_mention_notifications
         WHERE source_type = 'block' AND source_id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(block_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load synced Notes reminder candidates: {error}"))
}

/// Marks delivered the candidates a refresh created whose time has already passed, so a device
/// that first sees a mention or reminder from another device after its moment never notifies.
async fn settle_past_reminders(
    conn: &mut SqliteConnection,
    block_ids: &[String],
    known: &[String],
) -> Result<(), String> {
    let known = serde_json::to_string(known)
        .map_err(|error| format!("encode known Notes reminder candidates: {error}"))?;
    sqlx::query(
        "UPDATE notes_mention_notifications
         SET status = 'delivered',
             delivered_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE source_type = 'block'
           AND source_id IN (SELECT value FROM json_each(?1))
           AND id NOT IN (SELECT value FROM json_each(?2))
           AND status = 'pending'
           AND COALESCE(trigger_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
               <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(keys_json(block_ids)?)
    .bind(known)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("settle synced Notes reminder candidates: {error}"))?;
    Ok(())
}

/// Queues a history snapshot of every existing page among `page_ids`.
pub async fn queue_page_history(
    conn: &mut SqliteConnection,
    page_ids: &[String],
) -> Result<(), String> {
    if page_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notes_page_history_pending (page_id, queued_at)
         SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now') FROM notes_pages
         WHERE id IN (SELECT value FROM json_each(?1))
         ON CONFLICT(page_id) DO NOTHING",
    )
    .bind(keys_json(page_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("queue synced Notes page history: {error}"))?;
    Ok(())
}

/// Deletes the stored fingerprints of the search and link indexes, so the next read rebuilds
/// them from the current rows.
async fn invalidate_indexes(conn: &mut SqliteConnection) -> Result<(), String> {
    for table in INDEX_STATE_TABLES {
        let sql = format!("DELETE FROM {table}");
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .execute(&mut *conn)
            .await
            .map_err(|error| format!("invalidate {table}: {error}"))?;
    }
    Ok(())
}

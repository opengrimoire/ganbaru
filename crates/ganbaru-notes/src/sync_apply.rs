//! Notes work that runs inside the sync engine transaction after rows of a Notes table
//! materialize.
//!
//! The hook receives only the connection and a change sink, so the desktop service and the
//! headless Android pass run the same code. For the changed keys of an apply it fills the
//! hook-derived columns (a page's `parent_type`, `parent_page_id`, and `parent_block_id`, a
//! block's `page_id` and `plain_text`, a database's parent columns), refreshes the reminder
//! candidates of changed blocks and property values, forgets cached rollups that read changed
//! data sources, properties, options, values, or relation edges, invalidates the fingerprints
//! of the search and link indexes so the next read rebuilds them, queues history snapshots of
//! the touched pages in `notes_page_history_pending`, queues the
//! changed page and block keys for structure repair in `notes_sync_repair_pending`, and reports
//! every changed row under its table name. It writes only device-local rows and hook-derived
//! columns, which sync never captures.

use crate::block_storage::compose_block_content;
use crate::validation::plain_text_from_payload;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::notes::{
    BLOCKS_TABLE, DATA_SOURCES_TABLE, DATABASES_TABLE, DOMAIN, OPTIONS_TABLE, PAGES_TABLE,
    PROPERTIES_TABLE, RELATION_EDGES_TABLE, VALUES_TABLE, value_group,
};
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
        refresh_database_parents(conn, keys).await?;
        queue_repair(conn, "block", keys).await?;
    } else if table == DATABASES_TABLE {
        refresh_databases(conn, keys).await?;
    } else if table == DATA_SOURCES_TABLE {
        invalidate_rollups(conn, keys, false).await?;
    } else if table == PROPERTIES_TABLE || table == OPTIONS_TABLE {
        let table_name = if table == PROPERTIES_TABLE {
            "notes_data_source_properties"
        } else {
            "notes_data_source_property_options"
        };
        let (sources, removed) = present(conn, table_name, "data_source_id", keys).await?;
        invalidate_rollups(conn, &sources, removed).await?;
        refresh_property_reminders(conn, &[], true).await?;
    } else if table == VALUES_TABLE {
        let (mut pages, removed) =
            present(conn, "notes_page_property_values", "page_id", keys).await?;
        let sources = data_sources_of(conn, &pages).await?;
        invalidate_rollups(conn, &sources, removed).await?;
        refresh_property_reminders(conn, &pages, removed).await?;
        if removed {
            pages.extend(recorded_value_pages(conn, keys).await?);
        }
        queue_page_history(conn, &pages).await?;
    } else if table == RELATION_EDGES_TABLE {
        let (sources_pages, removed) =
            present(conn, "notes_relation_edges", "source_page_id", keys).await?;
        let (target_pages, _) =
            present(conn, "notes_relation_edges", "target_page_id", keys).await?;
        let mut pages = sources_pages.clone();
        pages.extend(target_pages);
        let sources = data_sources_of(conn, &pages).await?;
        invalidate_rollups(conn, &sources, removed).await?;
        queue_page_history(conn, &sources_pages).await?;
    }
    invalidate_indexes(conn).await
}

/// Sets the parent columns of the databases among `database_ids` to the placement of their
/// `child_database` block. Databases whose columns already agree, or whose block has no valid
/// placement, are left alone.
pub async fn refresh_database_parents(
    conn: &mut SqliteConnection,
    database_ids: &[String],
) -> Result<(), String> {
    sqlx::query(
        "WITH placed(id, kind, parent_page, parent_block) AS (
             SELECT database.id,
                 CASE WHEN block.parent_page_id IS NOT NULL THEN 'page_id' ELSE 'block_id' END,
                 block.parent_page_id,
                 block.parent_block_id
             FROM notes_databases AS database
             JOIN notes_blocks AS block ON block.id = database.block_id
             WHERE database.id IN (SELECT value FROM json_each(?1))
               AND (block.parent_page_id IS NULL) <> (block.parent_block_id IS NULL)
         )
         UPDATE notes_databases
         SET parent_type = placed.kind,
             parent_page_id = placed.parent_page,
             parent_block_id = placed.parent_block
         FROM placed
         WHERE notes_databases.id = placed.id
           AND (notes_databases.parent_type IS NOT placed.kind
                OR notes_databases.parent_page_id IS NOT placed.parent_page
                OR notes_databases.parent_block_id IS NOT placed.parent_block)",
    )
    .bind(keys_json(database_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("refresh synced Notes database parents: {error}"))?;
    Ok(())
}

async fn refresh_databases(
    conn: &mut SqliteConnection,
    database_ids: &[String],
) -> Result<(), String> {
    refresh_database_parents(conn, database_ids).await?;
    let pages: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT page_id FROM notes_blocks
         WHERE id IN (SELECT value FROM json_each(?1)) AND page_id <> ''",
    )
    .bind(keys_json(database_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load pages of synced Notes databases: {error}"))?;
    queue_page_history(conn, &pages).await
}

/// Forgets cached rollups that may read changed rows: those reading or read from
/// `data_source_ids`, or every cached rollup when `all` is set, because a removed row no longer
/// says where it lived.
async fn invalidate_rollups(
    conn: &mut SqliteConnection,
    data_source_ids: &[String],
    all: bool,
) -> Result<(), String> {
    let result = if all {
        sqlx::query("DELETE FROM notes_data_source_rollup_cache")
            .execute(&mut *conn)
            .await
    } else {
        sqlx::query(
            "DELETE FROM notes_data_source_rollup_cache
             WHERE source_data_source_id IN (SELECT value FROM json_each(?1))
                OR target_data_source_id IN (SELECT value FROM json_each(?1))",
        )
        .bind(keys_json(data_source_ids)?)
        .execute(&mut *conn)
        .await
    };
    result.map_err(|error| format!("invalidate synced Notes rollups: {error}"))?;
    Ok(())
}

/// The distinct values of `column` in the rows of the derived-key table `table` whose
/// `sync_key` is among `keys`, and whether some key names no row because it was removed.
async fn present(
    conn: &mut SqliteConnection,
    table: &str,
    column: &str,
    keys: &[String],
) -> Result<(Vec<String>, bool), String> {
    let found: Vec<(String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT sync_key, {column} FROM {table}
         WHERE sync_key IN (SELECT value FROM json_each(?1))"
    )))
    .bind(keys_json(keys)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load synced {table} rows: {error}"))?;
    let unique_keys: BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    let removed = found.len() < unique_keys.len();
    let values: BTreeSet<String> = found.into_iter().map(|(_, value)| value).collect();
    Ok((values.into_iter().collect(), removed))
}

/// The pages that the property values keyed by `keys` belong to, as the sync engine recorded
/// them with each version of the value identity. A removed value row no longer names its page,
/// but the engine keeps the identity versions of absent rows.
async fn recorded_value_pages(
    conn: &mut SqliteConnection,
    keys: &[String],
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT DISTINCT ref_key FROM sync_version_refs
         WHERE table_id = ?2 AND group_id = ?3 AND field = 0
           AND row_key IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(keys)?)
    .bind(i64::from(VALUES_TABLE.0))
    .bind(i64::from(value_group::IDENTITY.get()))
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load pages of removed Notes values: {error}"))
}

/// The data sources whose rows are among `page_ids`.
async fn data_sources_of(
    conn: &mut SqliteConnection,
    page_ids: &[String],
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT DISTINCT parent_data_source_id FROM notes_pages
         WHERE id IN (SELECT value FROM json_each(?1)) AND parent_data_source_id IS NOT NULL",
    )
    .bind(keys_json(page_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load data sources of synced Notes rows: {error}"))
}

/// Refreshes the date property reminders of `page_ids`, and of every page with a pending
/// property reminder when `all` is set, settling as delivered those first seen after their time.
async fn refresh_property_reminders(
    conn: &mut SqliteConnection,
    page_ids: &[String],
    all: bool,
) -> Result<(), String> {
    let mut pages: BTreeSet<String> = page_ids.iter().cloned().collect();
    if all {
        let pending: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT source_id FROM notes_mention_notifications
             WHERE source_type = ?1 AND status = 'pending'",
        )
        .bind(crate::mention_notifications::PAGE_PROPERTY_SOURCE)
        .fetch_all(&mut *conn)
        .await
        .map_err(|error| format!("load pending Notes property reminders: {error}"))?;
        pages.extend(pending);
    }
    if pages.is_empty() {
        return Ok(());
    }
    let pages: Vec<String> = pages.into_iter().collect();
    let mut tx = conn
        .begin()
        .await
        .map_err(|error| format!("begin synced Notes reminder refresh: {error}"))?;
    let source = crate::mention_notifications::PAGE_PROPERTY_SOURCE;
    let known = reminder_ids(&mut tx, source, &pages).await?;
    for page_id in &pages {
        crate::mention_notifications::sync_page_properties_tx(&mut tx, page_id).await?;
    }
    settle_past_reminders(&mut tx, source, &pages, &known).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit synced Notes reminder refresh: {error}"))
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
    invalidate_page_rollups(conn, page_ids).await?;
    queue_page_history(conn, page_ids).await
}

/// Forgets cached rollups that may read the changed pages `page_ids`.
///
/// A page that no longer exists says neither where it lived nor which edges the deletion
/// cascaded away, so every cached rollup is forgotten. Otherwise the rollups of the pages'
/// current data sources go, along with those computed for the pages themselves (which name the
/// data source a moved page left) and those whose relation edges reach the pages.
async fn invalidate_page_rollups(
    conn: &mut SqliteConnection,
    page_ids: &[String],
) -> Result<(), String> {
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM notes_pages WHERE id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(page_ids)?)
    .fetch_one(&mut *conn)
    .await
    .map_err(|error| format!("count synced Notes pages: {error}"))?;
    let unique: BTreeSet<&str> = page_ids.iter().map(String::as_str).collect();
    if usize::try_from(existing).unwrap_or(usize::MAX) < unique.len() {
        return invalidate_rollups(conn, &[], true).await;
    }
    let mut sources: BTreeSet<String> =
        data_sources_of(conn, page_ids).await?.into_iter().collect();
    let previous: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT source_data_source_id FROM notes_data_source_rollup_cache
         WHERE source_page_id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(page_ids)?)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load cached rollups of synced Notes pages: {error}"))?;
    sources.extend(previous);
    invalidate_rollups(conn, &sources.into_iter().collect::<Vec<_>>(), false).await?;
    sqlx::query(
        "DELETE FROM notes_data_source_rollup_cache
         WHERE source_page_id IN (
             SELECT source_page_id FROM notes_relation_edges
             WHERE target_page_id IN (SELECT value FROM json_each(?1))
         )",
    )
    .bind(keys_json(page_ids)?)
    .execute(&mut *conn)
    .await
    .map_err(|error| format!("invalidate rollups reaching synced Notes pages: {error}"))?;
    Ok(())
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
    let known_reminders = reminder_ids(&mut tx, "block", block_ids).await?;
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
    settle_past_reminders(&mut tx, "block", block_ids, &known_reminders).await?;
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

/// Ids of the reminder candidates of the `source_type` sources `source_ids` that exist before a
/// refresh.
async fn reminder_ids(
    conn: &mut SqliteConnection,
    source_type: &str,
    source_ids: &[String],
) -> Result<Vec<String>, String> {
    sqlx::query_scalar(
        "SELECT id FROM notes_mention_notifications
         WHERE source_type = ?2 AND source_id IN (SELECT value FROM json_each(?1))",
    )
    .bind(keys_json(source_ids)?)
    .bind(source_type)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("load synced Notes reminder candidates: {error}"))
}

/// Marks delivered the candidates a refresh created whose time has already passed, so a device
/// that first sees a mention or reminder from another device after its moment never notifies.
async fn settle_past_reminders(
    conn: &mut SqliteConnection,
    source_type: &str,
    source_ids: &[String],
    known: &[String],
) -> Result<(), String> {
    let known = serde_json::to_string(known)
        .map_err(|error| format!("encode known Notes reminder candidates: {error}"))?;
    sqlx::query(
        "UPDATE notes_mention_notifications
         SET status = 'delivered',
             delivered_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE source_type = ?3
           AND source_id IN (SELECT value FROM json_each(?1))
           AND id NOT IN (SELECT value FROM json_each(?2))
           AND status = 'pending'
           AND COALESCE(trigger_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
               <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(keys_json(source_ids)?)
    .bind(known)
    .bind(source_type)
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

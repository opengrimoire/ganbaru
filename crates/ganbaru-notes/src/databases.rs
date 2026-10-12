pub mod editing_lock;
pub mod view_management;

use crate::models::{
    NoteBlockRow, NoteCreatedDatabaseDto, NoteDataSourceRow, NoteDatabaseCreate,
    NoteDatabaseDuplicate, NoteDatabaseReferenceDto, NoteDatabaseRename, NoteDatabaseRow,
    NoteDatabaseViewRow, NoteLinkedDatabaseCreate, NoteParent,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_block_payload, validate_database_create,
    validate_parent, validate_sort_order,
};
use crate::{page_history, project_history, writes};
use serde_json::{Value, json};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{HashMap, HashSet};

const DEFAULT_DATABASE_TITLE: &str = "Untitled database";
const DEFAULT_TITLE_PROPERTY_NAME: &str = "Name";
const DEFAULT_TABLE_VIEW_NAME: &str = "Table";

/// Resolve the owning database without hydrating row pages or view data.
pub async fn database_reference(
    pool: &SqlitePool,
    block_id: &str,
) -> Result<NoteDatabaseReferenceDto, String> {
    require_uuid(block_id, "block_id")?;
    let block = crate::reads::get_block_row(pool, block_id.trim(), true).await?;
    if block.block_type != "child_database" {
        return Err("database block not found".to_string());
    }
    let (source_id, _) = source_database_refs(&block)?;
    let owner: (String, String) = sqlx::query_as(
        "SELECT source.database_id, block.page_id FROM notes_data_sources AS source
         JOIN notes_blocks AS block ON block.id = source.database_id
         WHERE source.id = ?",
    )
    .bind(source_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("resolve Notes database owner: {e}"))?
    .ok_or_else(|| "database source not found".to_string())?;
    let owned_data_source_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notes_data_sources WHERE database_id = ? AND in_trash = 0",
    )
    .bind(&block.id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("count owned Notes database sources: {e}"))?;
    let title: String = sqlx::query_scalar("SELECT title FROM notes_databases WHERE id = ?")
        .bind(&block.id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("read Notes database title: {e}"))?;
    Ok(NoteDatabaseReferenceDto {
        editing_locked: editing_lock::is_editing_locked(
            &serde_json::from_str(&block.payload)
                .map_err(|error| format!("read database editing preference: {error}"))?,
        )?,
        is_linked: owner.0 != block.id,
        source_block_id: block.id,
        page_id: block.page_id,
        canonical_source_block_id: owner.0,
        canonical_source_page_id: owner.1,
        title,
        owned_data_source_count,
    })
}

/// Copy a database and its complete local data graph to a fresh destination.
pub async fn duplicate_database(
    pool: &SqlitePool,
    request: NoteDatabaseDuplicate,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_database_destination(
        &request.id,
        &request.source_block_id,
        request.parent.as_ref(),
        request.after_block_id.as_deref(),
        request.replace_block_id.as_deref(),
    )?;
    ensure_destination_baseline(
        pool,
        &request.source_block_id,
        request.parent.as_ref(),
        request.replace_block_id.as_deref(),
    )
    .await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin Notes database copy: {e}"))?;
    let created = duplicate_database_tx(
        &mut tx,
        request,
        true,
        &mut writes::copy_budget::CopyBudget::default(),
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit Notes database copy: {e}"))?;
    Ok(created)
}

/// Duplicate a database inside the caller's transaction, optionally recording page history.
pub(crate) async fn duplicate_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: NoteDatabaseDuplicate,
    record_history: bool,
    budget: &mut writes::copy_budget::CopyBudget,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_database_destination(
        &request.id,
        &request.source_block_id,
        request.parent.as_ref(),
        request.after_block_id.as_deref(),
        request.replace_block_id.as_deref(),
    )?;
    let source = load_active_block_row_tx(tx, &request.source_block_id).await?;
    if source.block_type != "child_database" {
        return Err("database source must be a local database block".to_string());
    }
    let placement = resolve_database_destination(
        tx,
        &source,
        request.parent.as_ref(),
        request.after_block_id.as_deref(),
        request.replace_block_id.as_deref(),
    )
    .await?;
    let project_id =
        project_history::resolve_project_id_for_page_tx(tx, &placement.parent.page_id).await?;
    let mut reserved = HashSet::from([request.id.clone()]);
    let mut context = writes::copy_budget::CopyContext {
        reserved_ids: &mut reserved,
        budget,
    };
    let copy = writes::database_copy::plan_database_copy(
        tx,
        &source,
        &request.id,
        &mut context,
        project_id.as_deref(),
        false,
    )
    .await?;
    if record_history {
        page_history::record_page_snapshot_tx(tx, &placement.parent.page_id, "duplicate_database")
            .await?;
    }
    place_database_block(tx, &placement, &request.id, &copy.payload).await?;
    writes::database_copy::insert_database_copy(tx, &copy).await?;
    writes::database_copy::finalize_copies(tx, std::slice::from_ref(&copy), &[], &HashMap::new())
        .await?;
    writes::refresh_parent_has_children(tx, &placement.parent).await?;
    writes::touch_page(tx, &placement.parent.page_id).await?;
    let (source_id, view_id) =
        source_database_refs(&load_active_block_row_tx(tx, &request.id).await?)?;
    let created = load_created_linked_database_tx(tx, &request.id, &source_id, &view_id).await?;
    Ok(created)
}

struct DatabasePlacement {
    parent: writes::ParentTarget,
    sort_order: f64,
    replacing: bool,
}

fn validate_database_destination(
    id: &str,
    source_id: &str,
    parent: Option<&NoteParent>,
    after: Option<&str>,
    replacement: Option<&str>,
) -> Result<(), String> {
    require_uuid(id, "id")?;
    require_uuid(source_id, "source_block_id")?;
    if id.trim() == source_id.trim() {
        return Err("database id must differ from source block id".to_string());
    }
    if let Some(replacement) = replacement {
        require_uuid(replacement, "replace_block_id")?;
        if replacement.trim() != id.trim() || parent.is_some() || after.is_some() {
            return Err(
                "replacement must match id and cannot include parent or after_block_id".to_string(),
            );
        }
    }
    if let Some(parent) = parent {
        validate_parent(parent)?;
    }
    if let Some(after) = after {
        require_uuid(after, "after_block_id")?;
    }
    Ok(())
}

async fn ensure_destination_baseline(
    pool: &SqlitePool,
    source: &str,
    parent: Option<&NoteParent>,
    replacement: Option<&str>,
) -> Result<(), String> {
    if let Some(replacement) = replacement {
        project_history::ensure_blocks_baseline_for_mutation(pool, &[replacement.to_string()]).await
    } else if let Some(parent) = parent {
        project_history::ensure_parent_baseline_for_mutation(pool, parent).await
    } else {
        project_history::ensure_blocks_baseline_for_mutation(pool, &[source.to_string()]).await
    }
}

async fn resolve_database_destination(
    tx: &mut Transaction<'_, Sqlite>,
    source: &NoteBlockRow,
    parent: Option<&NoteParent>,
    after: Option<&str>,
    replacement: Option<&str>,
) -> Result<DatabasePlacement, String> {
    if let Some(replacement) = replacement {
        let current = load_active_block_row_tx(tx, replacement).await?;
        validate_replacement_block(&current)?;
        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM notes_blocks WHERE parent_block_id = ?)",
        )
        .bind(&current.id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| format!("check database replacement children: {e}"))?;
        if current.block_type == "child_database" || has_children {
            return Err("database replacement requires a block without owned content".to_string());
        }
        let parent = active_database_parent(tx, &current).await?;
        writes::validate_block_for_parent(&parent, "child_database", &json!({}))?;
        return Ok(DatabasePlacement {
            parent,
            sort_order: current.sort_order,
            replacing: true,
        });
    }
    let destination = match parent {
        Some(parent) => writes::resolve_block_parent(tx, parent).await?,
        None => active_database_parent(tx, source).await?,
    };
    writes::validate_page_parent_exists(
        tx,
        &NoteParent::PageId {
            page_id: destination.page_id.clone(),
        },
    )
    .await?;
    writes::validate_block_for_parent(&destination, "child_database", &json!({}))?;
    let after = after.or_else(|| parent.is_none().then_some(source.id.as_str()));
    let sort_order = writes::next_sort_orders(tx, &destination, after, 1).await?[0];
    validate_sort_order(sort_order)?;
    Ok(DatabasePlacement {
        parent: destination,
        sort_order,
        replacing: false,
    })
}

async fn active_database_parent(
    tx: &mut Transaction<'_, Sqlite>,
    block: &NoteBlockRow,
) -> Result<writes::ParentTarget, String> {
    let parent = match block.parent_block_id.as_ref() {
        Some(block_id) => NoteParent::BlockId {
            block_id: block_id.clone(),
        },
        None => NoteParent::PageId {
            page_id: block.page_id.clone(),
        },
    };
    let target = writes::resolve_block_parent(tx, &parent).await?;
    writes::validate_page_parent_exists(
        tx,
        &NoteParent::PageId {
            page_id: target.page_id.clone(),
        },
    )
    .await?;
    Ok(target)
}

async fn place_database_block(
    tx: &mut Transaction<'_, Sqlite>,
    placement: &DatabasePlacement,
    id: &str,
    payload: &Value,
) -> Result<(), String> {
    validate_block_payload("child_database", payload)?;
    let plain_text = plain_text_from_payload("child_database", payload);
    if placement.replacing {
        sqlx::query("UPDATE notes_blocks SET type = 'child_database', payload = ?, plain_text = ?, has_children = 0,
            last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ? AND in_trash = 0")
            .bind(payload.to_string()).bind(plain_text).bind(id.trim()).execute(&mut **tx).await
            .map_err(|e| format!("replace Notes block with database: {e}"))?;
    } else {
        sqlx::query("INSERT INTO notes_blocks (id, page_id, parent_type, parent_page_id, parent_block_id, type, payload, plain_text, sort_order)
            VALUES (?, ?, ?, ?, ?, 'child_database', ?, ?, ?)")
            .bind(id.trim()).bind(&placement.parent.page_id).bind(placement.parent.parent_type)
            .bind(&placement.parent.parent_page_id).bind(&placement.parent.parent_block_id)
            .bind(payload.to_string()).bind(plain_text).bind(placement.sort_order).execute(&mut **tx).await
            .map_err(|e| format!("insert Notes database destination: {e}"))?;
    }
    crate::assets::sync_block_asset_reference_tx(
        tx,
        id.trim(),
        &placement.parent.page_id,
        "child_database",
        payload,
    )
    .await?;
    Ok(())
}

/// Rename a shell and synchronize its sole owned source, preserving independent source names.
pub async fn rename_database(
    pool: &SqlitePool,
    database_id: &str,
    update: NoteDatabaseRename,
) -> Result<String, String> {
    require_uuid(database_id, "database_id")?;
    let title = update.title.trim();
    if title.chars().count() > 200 || title.chars().any(char::is_control) {
        return Err("database title must contain at most 200 printable characters".to_string());
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Notes database rename: {error}"))?;
    editing_lock::ensure_unlocked_tx(&mut tx, database_id).await?;
    let block = load_active_block_row_tx(&mut tx, database_id).await?;
    if block.block_type != "child_database" {
        return Err("database block not found".to_string());
    }
    let database: NoteDatabaseRow =
        sqlx::query_as("SELECT * FROM notes_databases WHERE id = ? AND in_trash = 0")
            .bind(database_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| format!("load Notes database to rename: {error}"))?
            .ok_or_else(|| "database not found".to_string())?;
    if database.title == title {
        return Ok(title.to_string());
    }
    page_history::record_page_snapshot_tx(&mut tx, &block.page_id, "rename_database").await?;
    project_history::mark_page_dirty_tx(&mut tx, &block.page_id, "Rename database", false).await?;
    let mut payload: Value = serde_json::from_str(&block.payload)
        .map_err(|error| format!("parse Notes database block: {error}"))?;
    payload["title"] = Value::String(title.to_string());
    validate_block_payload("child_database", &payload)?;
    sqlx::query("UPDATE notes_blocks SET payload = ?, plain_text = ?, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(payload.to_string())
        .bind(title)
        .bind(database_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("rename Notes database block: {error}"))?;
    sqlx::query("UPDATE notes_databases SET title = ?, title_rich_text = ?, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?")
        .bind(title)
        .bind(rich_text_array(title).to_string())
        .bind(database_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("rename Notes database: {error}"))?;
    sqlx::query("UPDATE notes_data_sources SET title = ?, title_rich_text = ?, last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE database_id = ? AND in_trash = 0 AND (SELECT COUNT(*) FROM notes_data_sources WHERE database_id = ? AND in_trash = 0) = 1")
        .bind(title)
        .bind(rich_text_array(title).to_string())
        .bind(database_id)
        .bind(database_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("rename Notes database data source: {error}"))?;
    writes::touch_page(&mut tx, &block.page_id).await?;
    tx.commit()
        .await
        .map_err(|error| format!("commit Notes database rename: {error}"))?;
    Ok(title.to_string())
}

/// Create a database using the explicit title, including an intentionally empty title.
pub async fn create_database(
    pool: &SqlitePool,
    request: NoteDatabaseCreate,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_database_create(&request)?;
    if let Some(block_id) = request.replace_block_id.as_ref() {
        project_history::ensure_blocks_baseline_for_mutation(pool, std::slice::from_ref(block_id))
            .await?;
    } else if let Some(parent) = request.parent.as_ref() {
        project_history::ensure_parent_baseline_for_mutation(pool, parent).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes database create: {e}"))?;
    let created = create_database_tx(&mut tx, request, true).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes database create: {e}"))?;
    Ok(created)
}

/// Create a database inside the caller's transaction, optionally recording page history.
pub(crate) async fn create_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: NoteDatabaseCreate,
    record_history: bool,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_database_create(&request)?;
    let (parent, title) = if let Some(replace_block_id) = &request.replace_block_id {
        let current = load_active_block_row_tx(tx, replace_block_id).await?;
        validate_replacement_block(&current)?;
        let parent = writes::parent_target_from_block_row(&current);
        let title = request.title.trim().to_string();
        if record_history {
            page_history::record_page_snapshot_tx(
                tx,
                &current.page_id,
                "create_database_from_block",
            )
            .await?;
        }
        replace_block_with_database(tx, &current, &request, &title).await?;
        (parent, title)
    } else {
        let request_parent = request
            .parent
            .as_ref()
            .ok_or_else(|| "parent is required".to_string())?;
        let parent = writes::resolve_block_parent(tx, request_parent).await?;
        let title = request.title.trim().to_string();
        let sort_order =
            writes::next_sort_orders(tx, &parent, request.after_block_id.as_deref(), 1)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| "database sort order was not prepared".to_string())?;
        validate_sort_order(sort_order)?;
        if record_history {
            page_history::record_page_snapshot_tx(tx, &parent.page_id, "create_database").await?;
        }
        insert_database_block(tx, &parent, &request, &title, sort_order).await?;
        writes::refresh_parent_has_children(tx, &parent).await?;
        (parent, title)
    };
    insert_database_objects(tx, &parent, &request, &title).await?;
    writes::touch_page(tx, &parent.page_id).await?;
    let created = load_created_database_tx(tx, &request.id).await?;
    Ok(created)
}

/// Place a linked shell at the requested destination while sharing its source data.
pub async fn create_linked_database_view(
    pool: &SqlitePool,
    request: NoteLinkedDatabaseCreate,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_linked_database_create(&request)?;
    ensure_destination_baseline(
        pool,
        &request.source_block_id,
        request.parent.as_ref(),
        request.replace_block_id.as_deref(),
    )
    .await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin linked notes database create: {e}"))?;
    let created = create_linked_database_view_tx(&mut tx, request, true).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit linked notes database create: {e}"))?;
    Ok(created)
}

/// Create a linked database inside the caller's transaction, optionally recording page history.
pub(crate) async fn create_linked_database_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    request: NoteLinkedDatabaseCreate,
    record_history: bool,
) -> Result<NoteCreatedDatabaseDto, String> {
    validate_linked_database_create(&request)?;
    let source_block = load_active_block_row_tx(tx, &request.source_block_id).await?;
    if source_block.block_type != "child_database" {
        return Err("linked database source must be a local database block".to_string());
    }
    let (source_data_source_id, source_view_id) = source_database_refs(&source_block)?;
    let source = load_active_data_source_tx(tx, &source_data_source_id).await?;
    let placement = resolve_database_destination(
        tx,
        &source_block,
        request.parent.as_ref(),
        request.after_block_id.as_deref(),
        request.replace_block_id.as_deref(),
    )
    .await?;
    let parent = &placement.parent;
    let title = linked_database_title(&request.title, &source_block, &source);
    if record_history {
        page_history::record_page_snapshot_tx(tx, &parent.page_id, "create_linked_database")
            .await?;
    }
    let payload = linked_child_database_payload(&request, &title, &source_data_source_id);
    place_database_block(tx, &placement, &request.id, &payload).await?;
    insert_linked_database_objects(
        tx,
        parent,
        &request,
        &title,
        &source_data_source_id,
        &source_view_id,
    )
    .await?;
    writes::refresh_parent_has_children(tx, parent).await?;
    writes::touch_page(tx, &parent.page_id).await?;
    let created =
        load_created_linked_database_tx(tx, &request.id, &source_data_source_id, &request.view_id)
            .await?;
    Ok(created)
}

async fn replace_block_with_database(
    tx: &mut Transaction<'_, Sqlite>,
    current: &NoteBlockRow,
    request: &NoteDatabaseCreate,
    title: &str,
) -> Result<(), String> {
    let payload = child_database_payload(request, title);
    validate_block_payload("child_database", &payload)?;
    let plain_text = plain_text_from_payload("child_database", &payload);
    sqlx::query(
        "UPDATE notes_blocks
         SET type = 'child_database',
             payload = ?,
             plain_text = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND in_trash = 0",
    )
    .bind(payload.to_string())
    .bind(plain_text)
    .bind(&current.id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("convert notes block to database: {e}"))?;
    Ok(())
}

async fn insert_database_block(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &writes::ParentTarget,
    request: &NoteDatabaseCreate,
    title: &str,
    sort_order: f64,
) -> Result<(), String> {
    let payload = child_database_payload(request, title);
    validate_block_payload("child_database", &payload)?;
    let plain_text = plain_text_from_payload("child_database", &payload);
    sqlx::query(
        "INSERT INTO notes_blocks (
            id,
            page_id,
            parent_type,
            parent_page_id,
            parent_block_id,
            type,
            payload,
            plain_text,
            sort_order
         )
         VALUES (?, ?, ?, ?, ?, 'child_database', ?, ?, ?)",
    )
    .bind(request.id.trim())
    .bind(&parent.page_id)
    .bind(parent.parent_type)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(payload.to_string())
    .bind(plain_text)
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes database block: {e}"))?;
    Ok(())
}

async fn insert_database_objects(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &writes::ParentTarget,
    request: &NoteDatabaseCreate,
    title: &str,
) -> Result<(), String> {
    let title_rich_text = rich_text_array(title);
    let description = Value::Array(Vec::new());
    let icon = request.icon.as_ref().map(Value::to_string);
    let cover = request.cover.as_ref().map(Value::to_string);
    sqlx::query(
        "INSERT INTO notes_databases (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            title,
            title_rich_text,
            description,
            icon,
            cover,
            is_inline
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(request.id.trim())
    .bind(parent.parent_type)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(title)
    .bind(title_rich_text.to_string())
    .bind(description.to_string())
    .bind(icon.clone())
    .bind(cover)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes database: {e}"))?;

    sqlx::query(
        "INSERT INTO notes_data_sources (
            id,
            database_id,
            title,
            title_rich_text,
            description,
            icon,
            properties
         )
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.data_source_id.trim())
    .bind(request.id.trim())
    .bind(title)
    .bind(title_rich_text.to_string())
    .bind(description.to_string())
    .bind(icon)
    .bind(default_data_source_properties().to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes data source: {e}"))?;

    sqlx::query(
        "INSERT INTO notes_database_views (
            id,
            database_id,
            data_source_id,
            name,
            type,
            sorts,
            configuration
         )
         VALUES (?, ?, ?, ?, 'table', ?, ?)",
    )
    .bind(request.view_id.trim())
    .bind(request.id.trim())
    .bind(request.data_source_id.trim())
    .bind(DEFAULT_TABLE_VIEW_NAME)
    .bind(Value::Array(Vec::new()).to_string())
    .bind(default_table_view_configuration().to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert notes database view: {e}"))?;
    Ok(())
}

async fn load_created_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
) -> Result<NoteCreatedDatabaseDto, String> {
    let database =
        sqlx::query_as::<_, NoteDatabaseRow>("SELECT * FROM notes_databases WHERE id = ?")
            .bind(database_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("load created notes database: {e}"))?;
    let data_source = sqlx::query_as::<_, NoteDataSourceRow>(
        "SELECT *
         FROM notes_data_sources
         WHERE database_id = ?
         ORDER BY created_time ASC, id ASC
         LIMIT 1",
    )
    .bind(database_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load created notes data source: {e}"))?;
    let view = sqlx::query_as::<_, NoteDatabaseViewRow>(
        "SELECT id,
                database_id,
                data_source_id,
                name,
                type AS view_type,
                filter,
                sorts,
                configuration,
                source_provider,
                source_object_id,
                source_workspace_id,
                source_last_edited_time,
                url,
                created_time,
                last_edited_time
         FROM notes_database_views
         WHERE database_id = ?
         ORDER BY sort_order ASC, created_time ASC, id ASC
         LIMIT 1",
    )
    .bind(database_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load created notes database view: {e}"))?;
    let block = load_active_block_row_tx(tx, database_id).await?;
    NoteCreatedDatabaseDto::new(database, data_source, view, block)
}

async fn load_active_block_row_tx(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
) -> Result<NoteBlockRow, String> {
    sqlx::query_as::<_, NoteBlockRow>(
        "SELECT id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                has_children,
                in_trash,
                type AS block_type,
                payload,
                plain_text,
                sort_order,
                source_provider,
                source_object_id,
                source_last_edited_time,
                created_time,
                last_edited_time
         FROM notes_blocks
         WHERE id = ? AND in_trash = 0",
    )
    .bind(block_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes database block: {e}"))?
    .ok_or_else(|| "block not found".to_string())
}

fn validate_replacement_block(current: &NoteBlockRow) -> Result<(), String> {
    if current.block_type == "child_page" {
        return Err("child_page blocks cannot be converted to databases".to_string());
    }
    if matches!(
        current.block_type.as_str(),
        "table" | "table_row" | "column_list" | "column" | "tab"
    ) {
        return Err(format!(
            "{} blocks cannot be converted to databases",
            current.block_type
        ));
    }
    Ok(())
}

fn child_database_payload(request: &NoteDatabaseCreate, title: &str) -> Value {
    json!({
        "title": title,
        "database_id": request.id.trim(),
        "data_source_id": request.data_source_id.trim(),
        "view_id": request.view_id.trim()
    })
}

fn linked_child_database_payload(
    request: &NoteLinkedDatabaseCreate,
    title: &str,
    data_source_id: &str,
) -> Value {
    json!({
        "title": title,
        "database_id": request.id.trim(),
        "data_source_id": data_source_id.trim(),
        "view_id": request.view_id.trim()
    })
}

async fn insert_linked_database_objects(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &writes::ParentTarget,
    request: &NoteLinkedDatabaseCreate,
    title: &str,
    data_source_id: &str,
    source_view_id: &str,
) -> Result<(), String> {
    let title_rich_text = rich_text_array(title);
    let description = Value::Array(Vec::new());
    sqlx::query(
        "INSERT INTO notes_databases (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            title,
            title_rich_text,
            description,
            is_inline
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, 1)",
    )
    .bind(request.id.trim())
    .bind(parent.parent_type)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(title)
    .bind(title_rich_text.to_string())
    .bind(description.to_string())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert linked notes database: {e}"))?;

    let source_view = load_source_view_tx(tx, data_source_id, source_view_id).await?;
    sqlx::query(
        "INSERT INTO notes_database_views (
            id,
            database_id,
            data_source_id,
            name,
            type,
            filter,
            sorts,
            configuration
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.view_id.trim())
    .bind(request.id.trim())
    .bind(data_source_id.trim())
    .bind(source_view.name)
    .bind(source_view.view_type)
    .bind(source_view.filter)
    .bind(source_view.sorts)
    .bind(source_view.configuration)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("insert linked notes database view: {e}"))?;
    Ok(())
}

async fn load_source_view_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    source_view_id: &str,
) -> Result<NoteDatabaseViewRow, String> {
    let exact = sqlx::query_as::<_, NoteDatabaseViewRow>(
        "SELECT id,
                database_id,
                data_source_id,
                name,
                type AS view_type,
                filter,
                sorts,
                configuration,
                source_provider,
                source_object_id,
                source_workspace_id,
                source_last_edited_time,
                url,
                created_time,
                last_edited_time
         FROM notes_database_views
         WHERE id = ? AND data_source_id = ?",
    )
    .bind(source_view_id.trim())
    .bind(data_source_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load exact source table view: {e}"))?;
    if let Some(view) = exact {
        return Ok(view);
    }
    sqlx::query_as::<_, NoteDatabaseViewRow>(
        "SELECT id,
                database_id,
                data_source_id,
                name,
                type AS view_type,
                filter,
                sorts,
                configuration,
                source_provider,
                source_object_id,
                source_workspace_id,
                source_last_edited_time,
                url,
                created_time,
                last_edited_time
         FROM notes_database_views
         WHERE data_source_id = ? AND type = 'table'
         ORDER BY sort_order ASC, created_time ASC, id ASC
         LIMIT 1",
    )
    .bind(data_source_id.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load source table view: {e}"))
}

async fn load_active_data_source_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<NoteDataSourceRow, String> {
    sqlx::query_as::<_, NoteDataSourceRow>(
        "SELECT data_source.*
         FROM notes_data_sources AS data_source
         JOIN notes_databases AS database ON database.id = data_source.database_id
         WHERE data_source.id = ?
           AND data_source.in_trash = 0
           AND database.in_trash = 0",
    )
    .bind(data_source_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load linked source data source: {e}"))?
    .ok_or_else(|| "linked source data source not found".to_string())
}

async fn load_created_linked_database_tx(
    tx: &mut Transaction<'_, Sqlite>,
    database_id: &str,
    data_source_id: &str,
    view_id: &str,
) -> Result<NoteCreatedDatabaseDto, String> {
    let database =
        sqlx::query_as::<_, NoteDatabaseRow>("SELECT * FROM notes_databases WHERE id = ?")
            .bind(database_id.trim())
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("load created linked notes database: {e}"))?;
    let data_source =
        sqlx::query_as::<_, NoteDataSourceRow>("SELECT * FROM notes_data_sources WHERE id = ?")
            .bind(data_source_id.trim())
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| format!("load linked notes data source: {e}"))?;
    let view = sqlx::query_as::<_, NoteDatabaseViewRow>(
        "SELECT id,
                database_id,
                data_source_id,
                name,
                type AS view_type,
                filter,
                sorts,
                configuration,
                source_provider,
                source_object_id,
                source_workspace_id,
                source_last_edited_time,
                url,
                created_time,
                last_edited_time
         FROM notes_database_views
         WHERE id = ?",
    )
    .bind(view_id.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| format!("load created linked notes database view: {e}"))?;
    let block = load_active_block_row_tx(tx, database_id).await?;
    NoteCreatedDatabaseDto::new(database, data_source, view, block)
}

fn validate_linked_database_create(request: &NoteLinkedDatabaseCreate) -> Result<(), String> {
    validate_database_destination(
        &request.id,
        &request.source_block_id,
        request.parent.as_ref(),
        request.after_block_id.as_deref(),
        request.replace_block_id.as_deref(),
    )?;
    require_uuid(&request.id, "id")?;
    require_uuid(&request.view_id, "view_id")?;
    require_uuid(&request.source_block_id, "source_block_id")?;
    if request.id == request.view_id {
        return Err("linked database id and view_id must be unique".to_string());
    }
    if request.id == request.source_block_id {
        return Err("linked database id must differ from source block id".to_string());
    }
    Ok(())
}

fn source_database_refs(source_block: &NoteBlockRow) -> Result<(String, String), String> {
    let payload: Value = serde_json::from_str(&source_block.payload)
        .map_err(|e| format!("parse source child_database payload: {e}"))?;
    let data_source_id = payload
        .get("data_source_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "source database block has no data_source_id".to_string())?;
    let view_id = payload
        .get("view_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "source database block has no view_id".to_string())?;
    require_uuid(data_source_id, "source data_source_id")?;
    require_uuid(view_id, "source view_id")?;
    Ok((data_source_id.to_string(), view_id.to_string()))
}

fn linked_database_title(
    request_title: &Option<String>,
    source_block: &NoteBlockRow,
    source: &NoteDataSourceRow,
) -> String {
    request_title
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string()
        .into_non_empty()
        .or_else(|| source_block.plain_text.trim().to_string().into_non_empty())
        .or_else(|| source.title.trim().to_string().into_non_empty())
        .unwrap_or_else(|| DEFAULT_DATABASE_TITLE.to_string())
}

pub(super) fn default_data_source_properties() -> Value {
    let mut properties = serde_json::Map::new();
    properties.insert(
        DEFAULT_TITLE_PROPERTY_NAME.to_string(),
        json!({
            "id": "title",
            "name": DEFAULT_TITLE_PROPERTY_NAME,
            "type": "title",
            "title": {}
        }),
    );
    Value::Object(properties)
}

fn default_table_view_configuration() -> Value {
    json!({
        "type": "table",
        "table": {
            "property_order": ["title"],
            "hidden_property_ids": [],
            "column_widths": {},
            "row_open_mode": "full_page"
        }
    })
}

pub(super) fn rich_text_array(text: &str) -> Value {
    json!([{
        "type": "text",
        "text": {
            "content": text,
            "link": null
        },
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
    }])
}

trait NonEmptyString {
    fn into_non_empty(self) -> Option<String>;
}

impl NonEmptyString for String {
    fn into_non_empty(self) -> Option<String> {
        if self.is_empty() { None } else { Some(self) }
    }
}

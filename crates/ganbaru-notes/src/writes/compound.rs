//! Atomic editor operations over the canonical Notes graph.

use super::{
    block_commands, block_duplicates, block_moves, block_tree, compound_layouts,
    compound_preimages, copy_budget::CopyBudget, parents,
};
use crate::models::{
    NoteAppendBlockChildren, NoteBlockDto, NoteBlockUpdate, NoteCreatedDatabaseDto,
    NoteDatabaseCreate, NoteDatabaseDuplicate, NoteDuplicateBlocks, NoteLinkedDatabaseCreate,
    NoteMoveBlock, NoteParent,
};
use crate::{databases, page_history, project_history, validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::{BTreeMap, BTreeSet};

const MAX_OPERATIONS: usize = 512;
const MAX_REFERENCES: usize = 4096;
pub(super) const MAX_GRAPH_BLOCKS: i64 = 20_000;
const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
pub(super) const MAX_RESULT_BYTES: usize = 32 * 1024 * 1024;

/// The editor's semantic boundary, retained in its durable receipt and history.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteEditKind {
    Split,
    Merge,
    ReplaceSelection,
    FormatSelection,
    Paste,
    Convert,
    TableColumns,
    ColumnLayout,
    TabLayout,
    Undo,
    Redo,
    DeleteSelection,
    IndentSelection,
    Template,
}

/// Bounded Notes graph instructions, never arbitrary SQL or filesystem operations.
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteEditOperation {
    Append {
        request: NoteAppendBlockChildren,
    },
    Update {
        block_id: String,
        update: Box<NoteBlockUpdate>,
    },
    Move {
        block_id: String,
        request: NoteMoveBlock,
    },
    MoveBetweenPages {
        block_id: String,
        source_page_id: String,
        destination_page_id: String,
        request: NoteMoveBlock,
    },
    Trash {
        block_id: String,
        in_trash: bool,
    },
    Duplicate {
        request: NoteDuplicateBlocks,
    },
    DuplicateChildren {
        source_block_id: String,
        request: NoteDuplicateBlocks,
    },
    CreateDatabase {
        request: NoteDatabaseCreate,
    },
    CopyDatabase {
        request: NoteDatabaseDuplicate,
    },
    LinkDatabase {
        request: NoteLinkedDatabaseCreate,
    },
    MoveChildren {
        source_block_id: String,
        parent: NoteParent,
        after: Option<String>,
    },
    TableColumn {
        table_id: String,
        index: usize,
        insert: bool,
        empty_row_id: String,
    },
}

/// One atomic editor action, immutable per operation ID so it can be retried after an uncertain response or restart.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteCompoundEdit {
    pub operation_id: String,
    pub page_id: String,
    pub kind: NoteEditKind,
    pub expected_blocks: BTreeMap<String, String>,
    pub operations: Vec<NoteEditOperation>,
}

/// Canonical state committed alongside the operation receipt.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteCompoundEditResult {
    pub operation_id: String,
    pub page_id: String,
    pub blocks: Vec<NoteBlockDto>,
    pub databases: Vec<NoteCreatedDatabaseDto>,
    pub placements: Vec<NoteEditPlacement>,
    pub before_blocks: Vec<NoteBlockDto>,
    pub before_placements: Vec<NoteEditPlacement>,
}

/// Canonical sibling anchors for affected active blocks, in sibling order.
#[derive(Deserialize, Serialize)]
pub struct NoteEditPlacement {
    pub block_id: String,
    pub parent: NoteParent,
    pub after: Option<String>,
    pub before: Option<String>,
}

/// Execute an entire accepted editor action and persist its exact response together.
pub async fn apply_compound_edit(
    pool: &SqlitePool,
    request: NoteCompoundEdit,
) -> Result<NoteCompoundEditResult, String> {
    validation::require_uuid(&request.operation_id, "operation_id")?;
    validation::require_uuid(&request.page_id, "page_id")?;
    if request.operations.is_empty()
        || request.operations.len() > MAX_OPERATIONS
        || request.expected_blocks.len() > MAX_REFERENCES
    {
        return Err("Notes edit exceeds its operation or reference limit".to_string());
    }
    let encoded = serde_json::to_vec(&request).map_err(|e| format!("encode Notes edit: {e}"))?;
    if encoded.len() > MAX_REQUEST_BYTES {
        return Err("Notes edit exceeds its byte limit".to_string());
    }
    let request_hash = format!("{:x}", Sha256::digest(&encoded));
    let (pages, foreign_references) = explicit_page_moves(&request)?;
    // A baseline is immutable recovery evidence, established before any graph mutation.
    for page_id in &pages {
        project_history::ensure_page_baseline_for_mutation(pool, page_id).await?;
    }
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin compound Notes edit: {e}"))?;
    // Claim the writer before reading preconditions. A rolled-back claim leaves no receipt.
    sqlx::query("INSERT INTO notes_edit_receipts(operation_id, page_id, request_hash, result_json) VALUES (?, ?, ?, '') ON CONFLICT(operation_id) DO NOTHING")
        .bind(&request.operation_id).bind(&request.page_id).bind(&request_hash)
        .execute(&mut *tx).await.map_err(|e| format!("claim Notes edit: {e}"))?;
    let (stored_hash, stored_result): (String, String) = sqlx::query_as(
        "SELECT request_hash, result_json FROM notes_edit_receipts WHERE operation_id = ?",
    )
    .bind(&request.operation_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| format!("read Notes edit receipt: {e}"))?;
    if stored_hash != request_hash {
        return Err("Notes operation identity was reused with a different request".to_string());
    }
    if !stored_result.is_empty() {
        return serde_json::from_str(&stored_result)
            .map_err(|e| format!("decode Notes edit receipt: {e}"));
    }
    for page_id in &pages {
        parents::resolve_block_parent(
            &mut tx,
            &NoteParent::PageId {
                page_id: page_id.clone(),
            },
        )
        .await?;
    }
    let mut preimages = compound_preimages::Preimages::default();
    let mut column_lists = BTreeSet::new();
    for (id, revision) in &request.expected_blocks {
        validation::require_uuid(id, "expected block")?;
        if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Notes edit contains an invalid revision".to_string());
        }
        preimages
            .capture(&mut tx, std::slice::from_ref(id), &BTreeSet::new())
            .await?;
        let row = block_tree::load_block_row_tx(&mut tx, id, true).await?;
        if row.page_id != request.page_id && foreign_references.get(id) != Some(&row.page_id) {
            return Err("Notes edit preconditions cannot reference another page".to_string());
        }
        if row.edit_revision()? != *revision {
            return Err(format!(
                "Notes edit conflict: block {id} changed; reload before retrying"
            ));
        }
        if matches!(&request.kind, NoteEditKind::ColumnLayout) {
            if row.block_type == "column_list" {
                column_lists.insert(row.id);
            } else if row.block_type == "column" {
                if let Some(parent) = row.parent_block_id {
                    column_lists.insert(parent);
                }
            }
        }
    }
    for list_id in column_lists {
        let columns: Vec<String> = sqlx::query_scalar("SELECT id FROM notes_blocks WHERE parent_block_id = ? AND in_trash = 0 ORDER BY sort_order, id LIMIT ?")
            .bind(list_id).bind((MAX_REFERENCES + 1) as i64).fetch_all(&mut *tx).await
            .map_err(|error| format!("read canonical Notes columns: {error}"))?;
        if columns.len() > MAX_REFERENCES
            || columns
                .iter()
                .any(|id| !request.expected_blocks.contains_key(id))
        {
            return Err(
                "Notes column edit requires all canonical column siblings; reload before retrying"
                    .to_string(),
            );
        }
    }
    for page_id in &pages {
        page_history::record_page_snapshot_tx(&mut tx, page_id, "compound_edit").await?;
    }
    let mut created = BTreeSet::new();
    let mut changed = BTreeSet::new();
    let mut created_databases = Vec::new();
    let mut copy_budget = CopyBudget::default();
    for operation in request.operations {
        let mutation = match &operation {
            NoteEditOperation::Update { block_id, .. }
            | NoteEditOperation::Move { block_id, .. }
            | NoteEditOperation::Trash { block_id, .. } => Some((block_id, &request.page_id)),
            NoteEditOperation::MoveBetweenPages {
                block_id,
                source_page_id,
                ..
            } => Some((block_id, source_page_id)),
            NoteEditOperation::MoveChildren {
                source_block_id, ..
            } => Some((source_block_id, &request.page_id)),
            NoteEditOperation::TableColumn { table_id, .. } => Some((table_id, &request.page_id)),
            NoteEditOperation::CreateDatabase { request: database } => database
                .replace_block_id
                .as_ref()
                .map(|id| (id, &request.page_id)),
            NoteEditOperation::CopyDatabase { request: database } => database
                .replace_block_id
                .as_ref()
                .map(|id| (id, &request.page_id)),
            NoteEditOperation::LinkDatabase { request: database } => database
                .replace_block_id
                .as_ref()
                .map(|id| (id, &request.page_id)),
            _ => None,
        };
        if let Some((id, page_id)) = mutation {
            let affected =
                validate_mutation(&mut tx, page_id, id, &request.expected_blocks, &created).await?;
            preimages.capture(&mut tx, &affected, &created).await?;
            changed.extend(affected);
        }
        match operation {
            NoteEditOperation::MoveBetweenPages {
                block_id,
                source_page_id: _,
                destination_page_id,
                request: movement,
            } => {
                validate_destination(
                    &mut tx,
                    &destination_page_id,
                    &movement.parent,
                    movement.after.as_deref().or(movement.before.as_deref()),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                block_moves::move_block_tx(&mut tx, &block_id, movement, false).await?;
                changed.insert(block_id);
            }
            NoteEditOperation::MoveChildren {
                source_block_id,
                parent,
                after,
            } => {
                validate_destination(
                    &mut tx,
                    &request.page_id,
                    &parent,
                    after.as_deref(),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                changed.extend(
                    compound_layouts::move_children(&mut tx, &source_block_id, parent, after)
                        .await?,
                );
                changed.insert(source_block_id);
            }
            NoteEditOperation::TableColumn {
                table_id,
                index,
                insert,
                empty_row_id,
            } => {
                changed.extend(
                    compound_layouts::table_column(
                        &mut tx,
                        &table_id,
                        index,
                        insert,
                        &empty_row_id,
                    )
                    .await?,
                );
                changed.insert(table_id);
            }
            NoteEditOperation::Append { request: append } => {
                validate_destination(
                    &mut tx,
                    &request.page_id,
                    &append.parent,
                    append.after.as_deref(),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                let ids = block_commands::append_block_children_tx(&mut tx, append, false).await?;
                changed.extend(ids.iter().cloned());
                created.extend(ids);
            }
            NoteEditOperation::Update { block_id, update } => {
                block_commands::update_block_tx(&mut tx, &block_id, *update, false).await?;
                changed.insert(block_id);
            }
            NoteEditOperation::Move {
                block_id,
                request: movement,
            } => {
                validate_destination(
                    &mut tx,
                    &request.page_id,
                    &movement.parent,
                    movement.after.as_deref().or(movement.before.as_deref()),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                block_moves::move_block_tx(&mut tx, &block_id, movement, false).await?;
                changed.insert(block_id);
            }
            NoteEditOperation::Trash { block_id, in_trash } => {
                block_commands::trash_block_tx(&mut tx, &block_id, in_trash, false).await?;
                changed.insert(block_id);
            }
            NoteEditOperation::Duplicate { request: copy } => {
                validate_destination(
                    &mut tx,
                    &request.page_id,
                    &copy.parent,
                    copy.after.as_deref().or(copy.before.as_deref()),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                let ids =
                    block_duplicates::duplicate_blocks_tx(&mut tx, copy, false, &mut copy_budget)
                        .await?;
                changed.extend(ids.iter().cloned());
                created.extend(ids);
            }
            NoteEditOperation::DuplicateChildren {
                source_block_id,
                request: copy,
            } => {
                validate_destination(
                    &mut tx,
                    &request.page_id,
                    &copy.parent,
                    copy.after.as_deref().or(copy.before.as_deref()),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                let copy =
                    compound_layouts::template_children(&mut tx, &source_block_id, copy).await?;
                let ids =
                    block_duplicates::duplicate_blocks_tx(&mut tx, copy, false, &mut copy_budget)
                        .await?;
                changed.extend(ids.iter().cloned());
                created.extend(ids);
            }
            NoteEditOperation::CreateDatabase { request: database } => {
                validate_database_destination(
                    &mut tx,
                    &request.page_id,
                    database.parent.as_ref(),
                    database.after_block_id.as_deref(),
                    database.replace_block_id.as_deref(),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                changed.insert(database.id.clone());
                created.insert(database.id.clone());
                let result = databases::create_database_tx(&mut tx, database, false).await?;
                preimages.charge_result(&result)?;
                created_databases.push(result);
            }
            NoteEditOperation::CopyDatabase { request: database } => {
                validate_database_destination(
                    &mut tx,
                    &request.page_id,
                    database.parent.as_ref(),
                    database.after_block_id.as_deref(),
                    database.replace_block_id.as_deref(),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                changed.insert(database.id.clone());
                created.insert(database.id.clone());
                let result =
                    databases::duplicate_database_tx(&mut tx, database, false, &mut copy_budget)
                        .await?;
                preimages.charge_result(&result)?;
                created_databases.push(result);
            }
            NoteEditOperation::LinkDatabase { request: database } => {
                validate_database_destination(
                    &mut tx,
                    &request.page_id,
                    database.parent.as_ref(),
                    database.after_block_id.as_deref(),
                    database.replace_block_id.as_deref(),
                    &request.expected_blocks,
                    &created,
                )
                .await?;
                changed.insert(database.id.clone());
                created.insert(database.id.clone());
                let result =
                    databases::create_linked_database_view_tx(&mut tx, database, false).await?;
                preimages.charge_result(&result)?;
                created_databases.push(result);
            }
        }
    }
    // Include known parents and anchors whose placement or child-presence revision changed.
    changed.extend(request.expected_blocks.keys().cloned());
    if changed.len() > MAX_GRAPH_BLOCKS as usize {
        return Err("Notes edit result exceeds its block limit".to_string());
    }
    let mut blocks = Vec::with_capacity(changed.len());
    let mut rows = Vec::with_capacity(changed.len());
    let mut result_bytes = preimages.bytes();
    for id in changed {
        result_bytes = compound_preimages::charge_row(&mut tx, &id, result_bytes).await?;
        rows.push(block_tree::load_block_row_tx(&mut tx, &id, true).await?);
    }
    rows.sort_by(|left, right| {
        (&left.parent_page_id, &left.parent_block_id)
            .cmp(&(&right.parent_page_id, &right.parent_block_id))
            .then(left.sort_order.total_cmp(&right.sort_order))
            .then(left.id.cmp(&right.id))
    });
    let mut placements = Vec::new();
    for row in rows {
        placements.extend(compound_preimages::placement(&mut tx, &row).await?);
        blocks.push(NoteBlockDto::new(row)?);
    }
    let (before_blocks, before_placements) = preimages.into_parts()?;
    let result = NoteCompoundEditResult {
        operation_id: request.operation_id.clone(),
        page_id: request.page_id,
        blocks,
        databases: created_databases,
        placements,
        before_blocks,
        before_placements,
    };
    let result_json =
        serde_json::to_string(&result).map_err(|e| format!("encode Notes edit result: {e}"))?;
    if result_json.len() > MAX_RESULT_BYTES {
        return Err("Notes edit result exceeds its byte limit".to_string());
    }
    sqlx::query("UPDATE notes_edit_receipts SET result_json = ? WHERE operation_id = ?")
        .bind(result_json)
        .bind(&request.operation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("save Notes edit receipt: {e}"))?;
    tx.commit()
        .await
        .map_err(|e| format!("commit compound Notes edit: {e}"))?;
    Ok(result)
}

/// Collect affected pages and foreign revisions, limited to the reviewed roots and destinations of explicit page moves.
fn explicit_page_moves(
    request: &NoteCompoundEdit,
) -> Result<(BTreeSet<String>, BTreeMap<String, String>), String> {
    let mut pages = BTreeSet::from([request.page_id.clone()]);
    let mut references = BTreeMap::new();
    for operation in &request.operations {
        let NoteEditOperation::MoveBetweenPages {
            block_id,
            source_page_id,
            destination_page_id,
            request: movement,
        } = operation
        else {
            continue;
        };
        validation::require_uuid(source_page_id, "source_page_id")?;
        validation::require_uuid(destination_page_id, "destination_page_id")?;
        if source_page_id == destination_page_id
            || (source_page_id != &request.page_id && destination_page_id != &request.page_id)
        {
            return Err("Notes page move must explicitly connect the requested page".to_string());
        }
        pages.insert(source_page_id.clone());
        pages.insert(destination_page_id.clone());
        let mut reference = |id: &str, page_id: &str| -> Result<(), String> {
            validation::require_uuid(id, "page move block")?;
            if references
                .insert(id.to_string(), page_id.to_string())
                .is_some_and(|previous| previous != page_id)
            {
                return Err("Notes page moves contain contradictory source ownership".to_string());
            }
            Ok(())
        };
        reference(block_id, source_page_id)?;
        if let NoteParent::BlockId { block_id } = &movement.parent {
            reference(block_id, destination_page_id)?;
        }
        if let Some(anchor) = movement.after.as_deref().or(movement.before.as_deref()) {
            reference(anchor, destination_page_id)?;
        }
    }
    Ok((pages, references))
}

fn require_reference(
    id: &str,
    expected: &BTreeMap<String, String>,
    created: &BTreeSet<String>,
) -> Result<(), String> {
    validation::require_uuid(id, "block reference")?;
    if !expected.contains_key(id) && !created.contains(id) {
        return Err(format!("Notes edit requires the revision of block {id}"));
    }
    Ok(())
}

async fn validate_mutation(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    id: &str,
    expected: &BTreeMap<String, String>,
    created: &BTreeSet<String>,
) -> Result<Vec<String>, String> {
    require_reference(id, expected, created)?;
    let row = block_tree::load_block_row_tx(tx, id, true).await?;
    if row.page_id != page_id {
        return Err("Notes compound edits cannot mutate another page".to_string());
    }
    let ids: Vec<String> = sqlx::query_scalar("WITH RECURSIVE subtree(id) AS (SELECT id FROM notes_blocks WHERE id = ? UNION SELECT b.id FROM notes_blocks b JOIN subtree s ON b.parent_block_id = s.id LIMIT ?) SELECT id FROM subtree")
        .bind(id).bind(MAX_GRAPH_BLOCKS + 1).fetch_all(&mut **tx).await
        .map_err(|e| format!("bound Notes edit graph: {e}"))?;
    if ids.len() > MAX_GRAPH_BLOCKS as usize {
        return Err("Notes edit graph exceeds its block limit".to_string());
    }
    let mut affected = ids;
    if let Some(parent_id) = row.parent_block_id {
        affected.push(parent_id);
    }
    Ok(affected)
}

async fn validate_destination(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    parent: &NoteParent,
    anchor: Option<&str>,
    expected: &BTreeMap<String, String>,
    created: &BTreeSet<String>,
) -> Result<(), String> {
    if let NoteParent::BlockId { block_id } = parent {
        require_reference(block_id, expected, created)?;
    }
    let target = parents::resolve_block_parent(tx, parent).await?;
    if target.page_id != page_id {
        return Err("Notes compound edits cannot write to another page".to_string());
    }
    if let Some(id) = anchor {
        require_reference(id, expected, created)?;
    }
    Ok(())
}

async fn validate_database_destination(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    parent: Option<&NoteParent>,
    anchor: Option<&str>,
    replacement: Option<&str>,
    expected: &BTreeMap<String, String>,
    created: &BTreeSet<String>,
) -> Result<(), String> {
    if let Some(id) = replacement {
        validate_mutation(tx, page_id, id, expected, created).await?;
    }
    if let Some(parent) = parent {
        validate_destination(tx, page_id, parent, anchor, expected, created).await?;
    }
    if parent.is_none() && replacement.is_none() {
        return Err("Notes database edit requires an explicit destination".to_string());
    }
    Ok(())
}

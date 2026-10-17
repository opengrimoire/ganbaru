use super::block_tree::{
    load_child_page_block_row, load_page_block_subtree_rows, load_page_block_subtree_rows_for_copy,
    refresh_duplicated_has_children,
};
use super::copy_budget::{CopyBudget, CopyContext};
use super::database_copy::{DatabaseCopy, insert_database_copy, plan_database_copy};
use super::ids::new_note_id;
use super::pages::load_page_row;
use super::parents::{parent_target_from_block_row, refresh_parent_has_children, touch_page};
use super::payloads::{child_page_payload, page_row_properties_for_title};
use super::sort::next_sort_orders;
use crate::models::{
    NoteBlockRow, NoteDuplicatePage, NoteLoadedPage, NotePageRow, NoteParent, page_parent_columns,
};
use crate::validation::{
    plain_text_from_payload, require_uuid, validate_parent, validate_sort_order,
};
use crate::{assets, page_history, project_history, reads};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet, VecDeque};

pub(super) struct DuplicatePagePlan {
    source_page: NotePageRow,
    duplicate_id: String,
    duplicate_title: String,
    parent: NoteParent,
    blocks: Vec<NoteBlockRow>,
    is_root: bool,
}

/// A complete page graph captured before inserting anything into its possible descendants.
pub(crate) struct DuplicatePageGraph {
    plans: Vec<DuplicatePagePlan>,
    block_ids: HashMap<String, String>,
    databases: Vec<DatabaseCopy>,
}

impl DuplicatePageGraph {
    pub(super) fn collect_identities(
        &self,
        identities: &mut HashMap<String, String>,
        schemas: &mut HashMap<String, HashMap<String, String>>,
    ) {
        identities.extend(self.block_ids.clone());
        for plan in &self.plans {
            identities.insert(plan.source_page.id.clone(), plan.duplicate_id.clone());
        }
        for database in &self.databases {
            database.collect_identities(identities, schemas);
        }
    }

    pub(super) fn extend_identities(&self, identities: &mut HashMap<String, String>) {
        self.collect_identities(identities, &mut HashMap::new());
    }

    pub(super) async fn finalize(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        inherited: &HashMap<String, String>,
        schemas: &HashMap<String, HashMap<String, String>>,
        copied_sources: &mut HashSet<String>,
    ) -> Result<(), String> {
        let mut identities = inherited.clone();
        let mut schemas = schemas.clone();
        self.collect_identities(&mut identities, &mut schemas);
        for plan in &self.plans {
            let mut scoped = identities.clone();
            if let Some(ids) = plan
                .source_page
                .parent_data_source_id
                .as_ref()
                .and_then(|source| schemas.get(source))
            {
                scoped.extend(ids.clone());
            }
            super::database_copy::remap_column(
                tx,
                "notes_pages",
                "properties",
                &plan.duplicate_id,
                &scoped,
            )
            .await?;
            let icon = plan
                .source_page
                .icon
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| format!("parse copied page icon: {e}"))?;
            let cover = plan
                .source_page
                .cover
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| format!("parse copied page cover: {e}"))?;
            assets::sync_page_asset_references_tx(
                tx,
                &plan.duplicate_id,
                true,
                icon.as_ref(),
                true,
                cover.as_ref(),
            )
            .await?;
            for row in &plan.blocks {
                let id = &self.block_ids[&row.id];
                super::database_copy::remap_column(tx, "notes_blocks", "payload", id, &identities)
                    .await?;
                let raw: String =
                    sqlx::query_scalar("SELECT payload FROM notes_blocks WHERE id = ?")
                        .bind(id)
                        .fetch_one(&mut **tx)
                        .await
                        .map_err(|e| format!("read copied block assets: {e}"))?;
                let payload = serde_json::from_str(&raw)
                    .map_err(|e| format!("parse copied block assets: {e}"))?;
                assets::sync_block_asset_reference_tx(
                    tx,
                    id,
                    &plan.duplicate_id,
                    &row.block_type,
                    &payload,
                )
                .await?;
            }
        }
        for database in &self.databases {
            Box::pin(database.finalize(tx, &identities, &schemas, copied_sources)).await?;
        }
        Ok(())
    }
}

/// Plan a child-note copy with fresh identities while retaining the caller's root identity.
pub(crate) async fn plan_child_page_copy(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    duplicate_id: &str,
    parent: NoteParent,
    include_trashed: bool,
    context: &mut CopyContext<'_>,
    destination_project_id: Option<&str>,
) -> Result<DuplicatePageGraph, String> {
    let mut plans = Vec::new();
    let mut block_ids = HashMap::new();
    let mut visited = HashSet::new();
    let mut databases = Vec::new();
    let mut queue = VecDeque::from([(source_id.to_string(), duplicate_id.to_string(), parent)]);
    while let Some((source_page_id, duplicate_id, parent)) = queue.pop_front() {
        if !visited.insert(source_page_id.clone()) {
            return Err("child page graph contains a cycle".to_string());
        }
        context.budget.page(tx, &source_page_id).await?;
        let mut source_page = sqlx::query_as::<_, NotePageRow>(
            "SELECT * FROM notes_pages WHERE id = ? AND (? OR in_trash = 0)",
        )
        .bind(&source_page_id)
        .bind(include_trashed)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load copied child page: {e}"))?
        .ok_or_else(|| "notes child page not found".to_string())?;
        let mut properties: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&source_page.properties)
                .map_err(|e| format!("parse copied note properties: {e}"))?;
        properties.remove("__ganbaru_project_id");
        properties.remove("__ganbaru_trash");
        properties.remove("__ganbaru_trash_owner");
        if let Some(project_id) = destination_project_id {
            properties.insert("__ganbaru_project_id".to_string(), project_id.into());
        }
        source_page.properties = serde_json::Value::Object(properties).to_string();
        let blocks = load_page_block_subtree_rows_for_copy(
            tx,
            &source_page_id,
            include_trashed,
            context.budget,
        )
        .await?;
        for row in &blocks {
            let id = new_note_id(tx, context.reserved_ids).await?;
            block_ids.insert(row.id.clone(), id.clone());
            if row.block_type == "child_page" {
                let parent = duplicate_page_parent_for_child_block(row, &duplicate_id, &block_ids)?;
                queue.push_back((row.id.clone(), id, parent));
            } else if row.block_type == "child_database"
                && super::database_copy::has_database_graph(row)?
            {
                databases.push(
                    Box::pin(plan_database_copy(
                        tx,
                        row,
                        &id,
                        context,
                        destination_project_id,
                        include_trashed,
                    ))
                    .await?,
                );
            }
        }
        plans.push(DuplicatePagePlan {
            duplicate_title: source_page.title.clone(),
            source_page,
            duplicate_id,
            parent,
            blocks,
            is_root: false,
        });
    }
    Ok(DuplicatePageGraph {
        plans,
        block_ids,
        databases,
    })
}

/// Insert a planned child-note graph in the same transaction as its paired block.
pub(crate) async fn insert_child_page_copy(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    graph: &DuplicatePageGraph,
) -> Result<(), String> {
    let titles = graph
        .plans
        .iter()
        .map(|plan| (plan.source_page.id.clone(), plan.duplicate_title.clone()))
        .collect();
    let mut inserted = HashSet::new();
    for plan in &graph.plans {
        insert_duplicated_page(tx, plan).await?;
        let icon = plan
            .source_page
            .icon
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("parse copied note icon: {e}"))?;
        let cover = plan
            .source_page
            .cover
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| format!("parse copied note cover: {e}"))?;
        assets::sync_page_asset_references_tx(
            tx,
            &plan.duplicate_id,
            true,
            icon.as_ref(),
            true,
            cover.as_ref(),
        )
        .await?;
        insert_duplicated_page_blocks(tx, plan, &graph.block_ids, &titles, &mut inserted).await?;
        for row in &plan.blocks {
            let payload = serde_json::from_str(&row.payload)
                .map_err(|e| format!("parse copied note block: {e}"))?;
            assets::sync_block_asset_reference_tx(
                tx,
                &graph.block_ids[&row.id],
                &plan.duplicate_id,
                &row.block_type,
                &payload,
            )
            .await?;
        }
        let block_ids = plan
            .blocks
            .iter()
            .filter_map(|row| {
                graph
                    .block_ids
                    .get(&row.id)
                    .map(|id| (row.id.clone(), id.clone()))
            })
            .collect();
        super::block_comments::duplicate_block_comment_threads(tx, &block_ids, &plan.duplicate_id)
            .await?;
    }
    for database in &graph.databases {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database.payload.to_string())
            .bind(&database.id)
            .execute(&mut **tx)
            .await
            .map_err(|e| format!("set copied database identities: {e}"))?;
        Box::pin(insert_database_copy(tx, database)).await?;
    }
    refresh_duplicated_has_children(tx, &inserted).await
}

pub async fn duplicate_page(
    pool: &SqlitePool,
    page_id: &str,
    request: NoteDuplicatePage,
) -> Result<NoteLoadedPage, String> {
    let page_id = page_id.trim();
    require_uuid(page_id, "page_id")?;
    project_history::ensure_page_baseline_for_mutation(pool, page_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin duplicate notes page: {e}"))?;
    let mut budget = CopyBudget::default();
    budget.page(&mut tx, page_id).await?;
    let root_page = load_page_row(&mut tx, page_id).await?;
    if let Some(source_id) = root_page
        .parent_data_source_id
        .as_ref()
        .filter(|_| root_page.parent_type == "data_source_id")
    {
        crate::data_sources::row_hierarchy::validate_sources_tx(
            &mut tx,
            std::slice::from_ref(source_id),
        )
        .await?;
    }
    if matches!(root_page.parent_type.as_str(), "page_id" | "block_id") {
        let source_block = load_child_page_block_row(&mut tx, page_id).await?;
        page_history::record_page_snapshot_tx(&mut tx, &source_block.page_id, "duplicate_page")
            .await?;
    }
    let root_duplicate_title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(root_page.title.trim())
        .to_string();
    let mut reserved_ids = HashSet::new();
    let mut block_ids = HashMap::new();
    let root_duplicate_id = new_note_id(&mut tx, &mut reserved_ids).await?;
    let mut plans = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::from([(
        page_id.to_string(),
        root_duplicate_id.clone(),
        Some(root_duplicate_title),
        None::<NoteParent>,
        true,
    )]);
    while let Some((source_page_id, duplicate_id, title_override, parent_override, is_root)) =
        queue.pop_front()
    {
        if !visited.insert(source_page_id.clone()) {
            return Err("Notes page copy contains a cycle".to_string());
        }
        if !is_root {
            budget.page(&mut tx, &source_page_id).await?;
        }
        let source_page = load_page_row(&mut tx, &source_page_id).await?;
        let blocks = load_page_block_subtree_rows(&mut tx, &source_page_id, &mut budget).await?;
        if let Some(source_id) = source_page
            .parent_data_source_id
            .as_ref()
            .filter(|_| source_page.parent_type == "data_source_id")
        {
            let children = sqlx::query_scalar::<_, String>(
                "SELECT page.id FROM notes_data_source_row_hierarchy AS hierarchy JOIN notes_pages AS page ON page.id = hierarchy.row_page_id
                 WHERE hierarchy.parent_row_page_id = ? AND hierarchy.data_source_id = ?
                   AND page.parent_type = 'data_source_id' AND page.parent_data_source_id = ? AND page.in_trash = 0 AND page.archived = 0
                 ORDER BY page.id LIMIT ?",
            ).bind(&source_page_id).bind(source_id).bind(source_id)
                .bind((super::database_copy::MAX_COPY_OBJECTS + 1) as i64).fetch_all(&mut *tx).await
                .map_err(|error| format!("load duplicated row sub-items: {error}"))?;
            for child in children {
                if plans.len() + queue.len() >= super::database_copy::MAX_COPY_OBJECTS {
                    return Err("row sub-items exceed the database copy limit".to_string());
                }
                let child_duplicate_id = new_note_id(&mut tx, &mut reserved_ids).await?;
                block_ids.insert(child.clone(), child_duplicate_id.clone());
                queue.push_back((
                    child,
                    child_duplicate_id,
                    None,
                    Some(NoteParent::DataSourceId {
                        data_source_id: source_id.clone(),
                    }),
                    false,
                ));
            }
        }
        for row in &blocks {
            if row.block_type == "child_page" {
                let child_duplicate_id = new_note_id(&mut tx, &mut reserved_ids).await?;
                block_ids.insert(row.id.clone(), child_duplicate_id.clone());
                let duplicate_parent =
                    duplicate_page_parent_for_child_block(row, &duplicate_id, &block_ids)?;
                queue.push_back((
                    row.id.clone(),
                    child_duplicate_id,
                    None,
                    Some(duplicate_parent),
                    false,
                ));
            } else if !block_ids.contains_key(&row.id) {
                let duplicate_block_id = new_note_id(&mut tx, &mut reserved_ids).await?;
                block_ids.insert(row.id.clone(), duplicate_block_id);
            }
        }
        let parent = match parent_override {
            Some(parent) => parent,
            None => page_parent_from_columns(
                &source_page.parent_type,
                source_page.parent_page_id.clone(),
                source_page.parent_block_id.clone(),
                source_page.parent_data_source_id.clone(),
            )?,
        };
        plans.push(DuplicatePagePlan {
            duplicate_title: title_override.unwrap_or_else(|| source_page.title.clone()),
            source_page,
            duplicate_id,
            parent,
            blocks,
            is_root,
        });
    }
    let duplicate_titles = plans
        .iter()
        .map(|plan| (plan.source_page.id.clone(), plan.duplicate_title.clone()))
        .collect::<HashMap<_, _>>();
    let mut inserted_block_ids = HashSet::new();
    let mut databases = Vec::new();
    let destination_project_id =
        project_history::resolve_project_id_for_page_tx(&mut tx, page_id).await?;
    let mut context = CopyContext {
        reserved_ids: &mut reserved_ids,
        budget: &mut budget,
    };
    for plan in &plans {
        for row in plan
            .blocks
            .iter()
            .filter(|row| row.block_type == "child_database")
        {
            if super::database_copy::has_database_graph(row)? {
                databases.push(
                    plan_database_copy(
                        &mut tx,
                        row,
                        &block_ids[&row.id],
                        &mut context,
                        destination_project_id.as_deref(),
                        false,
                    )
                    .await?,
                );
            }
        }
    }
    for plan in &plans {
        insert_duplicated_page(&mut tx, plan).await?;
        if plan.is_root {
            insert_root_duplicate_child_page_block(&mut tx, plan).await?;
        }
        insert_duplicated_page_blocks(
            &mut tx,
            plan,
            &block_ids,
            &duplicate_titles,
            &mut inserted_block_ids,
        )
        .await?;
    }
    for database in &databases {
        sqlx::query("UPDATE notes_blocks SET payload = ? WHERE id = ?")
            .bind(database.payload.to_string())
            .bind(&database.id)
            .execute(&mut *tx)
            .await
            .map_err(|e| format!("set duplicated page database identities: {e}"))?;
        insert_database_copy(&mut tx, database).await?;
    }
    refresh_duplicated_has_children(&mut tx, &inserted_block_ids).await?;
    let graph = DuplicatePageGraph {
        plans,
        block_ids,
        databases,
    };
    super::database_copy::finalize_copies(&mut tx, &[], &[graph], &HashMap::new()).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit duplicate notes page: {e}"))?;
    reads::load_page(pool, &root_duplicate_id).await
}

pub(super) fn page_parent_from_columns(
    parent_type: &str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
    parent_data_source_id: Option<String>,
) -> Result<NoteParent, String> {
    match parent_type {
        "workspace" => Ok(NoteParent::Workspace { workspace: true }),
        "page_id" => parent_page_id
            .map(|page_id| NoteParent::PageId { page_id })
            .ok_or_else(|| "page parent row is missing parent_page_id".to_string()),
        "block_id" => parent_block_id
            .map(|block_id| NoteParent::BlockId { block_id })
            .ok_or_else(|| "page parent row is missing parent_block_id".to_string()),
        "data_source_id" => parent_data_source_id
            .map(|data_source_id| NoteParent::DataSourceId { data_source_id })
            .ok_or_else(|| "page parent row is missing parent_data_source_id".to_string()),
        _ => Err("page parent row has unsupported parent_type".to_string()),
    }
}

pub(super) fn duplicate_page_parent_for_child_block(
    block: &NoteBlockRow,
    duplicate_page_id: &str,
    block_ids: &HashMap<String, String>,
) -> Result<NoteParent, String> {
    match block.parent_type.as_str() {
        "page_id" => Ok(NoteParent::PageId {
            page_id: duplicate_page_id.to_string(),
        }),
        "block_id" => {
            let source_parent_id = block
                .parent_block_id
                .as_ref()
                .ok_or_else(|| "child page block is missing its parent".to_string())?;
            let duplicate_parent_id = block_ids
                .get(source_parent_id)
                .cloned()
                .ok_or_else(|| "child page block parent was not duplicated".to_string())?;
            Ok(NoteParent::BlockId {
                block_id: duplicate_parent_id,
            })
        }
        _ => Err("child page block has unsupported parent_type".to_string()),
    }
}

pub(super) async fn insert_duplicated_page(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &DuplicatePagePlan,
) -> Result<(), String> {
    validate_parent(&plan.parent)?;
    let (parent_type, parent_page_id, parent_block_id, parent_data_source_id) =
        page_parent_columns(&plan.parent);
    let folder_id = if plan.is_root && parent_type == "workspace" {
        plan.source_page.folder_id.as_deref()
    } else {
        None
    };
    let properties = if plan.duplicate_title == plan.source_page.title {
        plan.source_page.properties.clone()
    } else {
        page_row_properties_for_title(&plan.source_page, &plan.duplicate_title)?
    };
    sqlx::query(
        "INSERT INTO notes_pages (
            id,
            parent_type,
            parent_page_id,
            parent_block_id,
            parent_data_source_id,
            folder_id,
            title,
            properties,
            icon,
            cover,
            archived
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&plan.duplicate_id)
    .bind(parent_type)
    .bind(parent_page_id)
    .bind(parent_block_id)
    .bind(parent_data_source_id)
    .bind(folder_id)
    .bind(&plan.duplicate_title)
    .bind(properties)
    .bind(&plan.source_page.icon)
    .bind(&plan.source_page.cover)
    .bind(plan.source_page.archived)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("duplicate notes page: {e}"))?;
    Ok(())
}

pub(super) async fn insert_root_duplicate_child_page_block(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &DuplicatePagePlan,
) -> Result<(), String> {
    if plan.source_page.parent_type == "workspace"
        || plan.source_page.parent_type == "data_source_id"
    {
        return Ok(());
    }
    let source_block = load_child_page_block_row(tx, &plan.source_page.id).await?;
    let parent = parent_target_from_block_row(&source_block);
    let sort_order = next_sort_orders(tx, &parent, Some(source_block.id.as_str()), 1).await?[0];
    validate_sort_order(sort_order)?;
    let payload = child_page_payload(&plan.duplicate_title);
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
         VALUES (?, ?, ?, ?, ?, 'child_page', ?, ?, ?)",
    )
    .bind(&plan.duplicate_id)
    .bind(&source_block.page_id)
    .bind(parent.parent_type)
    .bind(&parent.parent_page_id)
    .bind(&parent.parent_block_id)
    .bind(payload.to_string())
    .bind(plain_text_from_payload("child_page", &payload))
    .bind(sort_order)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("duplicate notes child page block: {e}"))?;
    refresh_parent_has_children(tx, &parent).await?;
    touch_page(tx, &source_block.page_id).await?;
    Ok(())
}

pub(super) async fn insert_duplicated_page_blocks(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &DuplicatePagePlan,
    block_ids: &HashMap<String, String>,
    duplicate_titles: &HashMap<String, String>,
    inserted_block_ids: &mut HashSet<String>,
) -> Result<(), String> {
    for row in &plan.blocks {
        let duplicate_id = block_ids
            .get(&row.id)
            .ok_or_else(|| "duplicated page block id is missing".to_string())?;
        let (parent_type, parent_page_id, parent_block_id) = if row.parent_type == "page_id" {
            ("page_id", Some(plan.duplicate_id.clone()), None)
        } else {
            let source_parent_id = row
                .parent_block_id
                .as_ref()
                .ok_or_else(|| "duplicated block is missing its parent".to_string())?;
            let duplicate_parent_id = block_ids
                .get(source_parent_id)
                .cloned()
                .ok_or_else(|| "duplicated block parent id is missing".to_string())?;
            ("block_id", None, Some(duplicate_parent_id))
        };
        let (payload, plain_text) = if row.block_type == "child_page" {
            let title = duplicate_titles
                .get(&row.id)
                .ok_or_else(|| "duplicated child page title is missing".to_string())?;
            let payload = child_page_payload(title);
            (
                payload.to_string(),
                plain_text_from_payload("child_page", &payload),
            )
        } else {
            let mut payload: serde_json::Value = serde_json::from_str(&row.payload)
                .map_err(|e| format!("parse copied block: {e}"))?;
            super::database_copy::strip_trash_metadata(&mut payload);
            (payload.to_string(), row.plain_text.clone())
        };
        validate_sort_order(row.sort_order)?;
        sqlx::query(
            "INSERT INTO notes_blocks (
                id,
                page_id,
                parent_type,
                parent_page_id,
                parent_block_id,
                has_children,
                type,
                payload,
                plain_text,
                sort_order
             )
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(duplicate_id)
        .bind(&plan.duplicate_id)
        .bind(parent_type)
        .bind(parent_page_id)
        .bind(parent_block_id)
        .bind(row.has_children)
        .bind(&row.block_type)
        .bind(payload)
        .bind(plain_text)
        .bind(row.sort_order)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("duplicate notes page block: {e}"))?;
        inserted_block_ids.insert(duplicate_id.clone());
    }
    Ok(())
}

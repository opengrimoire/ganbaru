use crate::data_sources;
use crate::models::{
    NoteAgentBridgeExportDiagnosticDto, NoteAgentBridgeExportDto, NoteAgentBridgeExportRequest,
    NoteAgentBridgeExportSaveDto, NoteDataSourceCsvExportRequest,
};
use crate::validation::require_uuid;
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

mod render;

const EXPORT_VERSION: i64 = 1;
const SCHEMA_VERSION: &str = "notes-agent-bridge.v1";
const MARKDOWN_CONTENT_TYPE: &str = "text/markdown; charset=utf-8";

#[derive(Clone, Copy)]
struct BridgeOptions {
    include_descendants: bool,
    include_backlinks: bool,
    include_database_views: bool,
    include_task_context: bool,
    include_page_comments: bool,
    include_resolved_comments: bool,
}

impl BridgeOptions {
    fn from_request(request: &NoteAgentBridgeExportRequest) -> Self {
        Self {
            include_descendants: request.include_descendants.unwrap_or(true),
            include_backlinks: request.include_backlinks.unwrap_or(true),
            include_database_views: request.include_database_views.unwrap_or(true),
            include_task_context: request.include_task_context.unwrap_or(true),
            include_page_comments: request.include_page_comments.unwrap_or(false),
            include_resolved_comments: request.include_resolved_comments.unwrap_or(false),
        }
    }
}

struct BridgePage {
    id: String,
    title: String,
}

struct BridgeDatabaseView {
    block_id: String,
    page_title: String,
    title: String,
    data_source_id: String,
    database_id: String,
    view_id: String,
    markdown_table: String,
    exported_row_count: i64,
}

struct BridgeBacklink {
    target_id: String,
    target_title: String,
    source_page_id: String,
    source_page_title: String,
    source_block_id: Option<String>,
    source_comment_id: Option<String>,
    reference_type: String,
    snippet: String,
}

struct BridgeProject {
    id: String,
    group_name: String,
    name: String,
    status: String,
    tasks: Vec<BridgeTask>,
}

struct BridgeTask {
    id: String,
    title: String,
    description: String,
    section_name: String,
    status_name: String,
    priority: String,
    task_type: String,
    parent_title: Option<String>,
    estimate_minutes: Option<i64>,
    start_date: Option<String>,
    due_date: Option<String>,
    target_end_date: Option<String>,
    completed_at: Option<String>,
    blocker_reason: Option<String>,
    checklist: Vec<BridgeChecklistItem>,
    tags: Vec<String>,
}

#[derive(Clone)]
struct BridgeChecklistItem {
    title: String,
    completed: bool,
}

struct BridgeSummary {
    page_count: i64,
    project_count: i64,
    task_count: i64,
    database_view_count: i64,
    backlink_count: i64,
}

pub async fn export_bridge(
    pool: &SqlitePool,
    request: NoteAgentBridgeExportRequest,
) -> Result<NoteAgentBridgeExportDto, String> {
    let options = BridgeOptions::from_request(&request);
    let requested_page_ids = normalize_page_ids(&request.page_ids)?;
    let requested_project_ids = normalize_project_ids(&request.project_ids)?;
    if requested_page_ids.is_empty() && requested_project_ids.is_empty() {
        return Err("agent bridge export requires at least one page or project".to_string());
    }

    let mut diagnostics = Vec::new();
    let pages = load_bridge_pages(
        pool,
        &requested_page_ids,
        options.include_descendants,
        &mut diagnostics,
    )
    .await?;
    let page_ids = pages.iter().map(|page| page.id.clone()).collect::<Vec<_>>();
    let database_views = if options.include_database_views {
        load_database_views(pool, &page_ids, &mut diagnostics).await?
    } else {
        Vec::new()
    };
    let backlinks = if options.include_backlinks {
        load_backlinks(pool, &page_ids).await?
    } else {
        Vec::new()
    };
    let project_ids = if options.include_task_context {
        project_scope_ids(pool, &requested_project_ids, &page_ids).await?
    } else {
        Vec::new()
    };
    let projects = if options.include_task_context {
        load_projects(pool, &project_ids, &mut diagnostics).await?
    } else {
        Vec::new()
    };

    let summary = BridgeSummary {
        page_count: pages.len() as i64,
        project_count: projects.len() as i64,
        task_count: projects
            .iter()
            .map(|project| project.tasks.len() as i64)
            .sum(),
        database_view_count: database_views.len() as i64,
        backlink_count: backlinks.len() as i64,
    };
    let markdown = render::render_bridge_markdown(
        pool,
        &pages,
        &database_views,
        &projects,
        &backlinks,
        options,
        &mut diagnostics,
    )
    .await?;
    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == "warning")
        .count() as i64;
    Ok(NoteAgentBridgeExportDto {
        object: "notes_agent_bridge_export",
        export_version: EXPORT_VERSION,
        schema_version: SCHEMA_VERSION,
        file_name: "ganbaru-agent-bridge.md".to_string(),
        content_type: MARKDOWN_CONTENT_TYPE,
        byte_size: markdown.len() as i64,
        markdown,
        diagnostics,
        exported_page_count: summary.page_count,
        exported_project_count: summary.project_count,
        exported_task_count: summary.task_count,
        exported_database_view_count: summary.database_view_count,
        exported_backlink_count: summary.backlink_count,
        warning_count,
    })
}

pub fn write_bridge(
    path: &Path,
    export: NoteAgentBridgeExportDto,
) -> Result<NoteAgentBridgeExportSaveDto, String> {
    require_markdown_extension(path)?;
    write_markdown_file(path, &export.markdown)?;
    Ok(NoteAgentBridgeExportSaveDto::saved(export))
}

fn normalize_page_ids(values: &[String]) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    for value in values {
        let id = value.trim();
        if id.is_empty() {
            continue;
        }
        require_uuid(id, "page_id")?;
        seen.insert(id.to_string());
    }
    Ok(seen.into_iter().collect())
}

fn normalize_project_ids(values: &[String]) -> Result<Vec<String>, String> {
    let mut seen = BTreeSet::new();
    for value in values {
        let id = value.trim();
        if id.is_empty() {
            continue;
        }
        if id.len() > 200 {
            return Err("project_id is too long".to_string());
        }
        seen.insert(id.to_string());
    }
    Ok(seen.into_iter().collect())
}

async fn load_bridge_pages(
    pool: &SqlitePool,
    requested_page_ids: &[String],
    include_descendants: bool,
    diagnostics: &mut Vec<NoteAgentBridgeExportDiagnosticDto>,
) -> Result<Vec<BridgePage>, String> {
    if requested_page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let pages = if include_descendants {
        load_pages_with_descendants(pool, requested_page_ids).await?
    } else {
        load_exact_pages(pool, requested_page_ids).await?
    };
    let found = pages
        .iter()
        .map(|page| page.id.as_str())
        .collect::<HashSet<_>>();
    for requested in requested_page_ids {
        if !found.contains(requested.as_str()) {
            diagnostics.push(NoteAgentBridgeExportDiagnosticDto::new(
                "agent_bridge_page_not_exported",
                "warning",
                Some("page"),
                Some(requested.clone()),
                "Requested Notes page was not found or is not active.",
            ));
        }
    }
    Ok(pages)
}

async fn load_exact_pages(
    pool: &SqlitePool,
    requested_page_ids: &[String],
) -> Result<Vec<BridgePage>, String> {
    let sql = format!(
        "SELECT id, title
         FROM notes_pages
         WHERE in_trash = 0
           AND archived = 0
           AND id IN ({})
         ORDER BY lower(title) ASC, id ASC",
        placeholders(requested_page_ids.len())
    );
    let mut query = sqlx::query(&sql);
    for id in requested_page_ids {
        query = query.bind(id);
    }
    rows_to_pages(
        query
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load agent bridge pages: {e}"))?,
    )
}

async fn load_pages_with_descendants(
    pool: &SqlitePool,
    requested_page_ids: &[String],
) -> Result<Vec<BridgePage>, String> {
    let values = (0..requested_page_ids.len())
        .map(|_| "(?)")
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "WITH RECURSIVE requested(id) AS (VALUES {values}),
             bridge_pages(id) AS (
                SELECT page.id
                FROM notes_pages page
                JOIN requested ON requested.id = page.id
                WHERE page.in_trash = 0 AND page.archived = 0
                UNION
                SELECT child.id
                FROM notes_pages child
                JOIN bridge_pages parent ON child.parent_type = 'page_id'
                  AND child.parent_page_id = parent.id
                WHERE child.in_trash = 0 AND child.archived = 0
                UNION
                SELECT child.id
                FROM notes_pages child
                JOIN notes_blocks block ON child.parent_type = 'block_id'
                  AND child.parent_block_id = block.id
                JOIN bridge_pages parent ON block.page_id = parent.id
                WHERE child.in_trash = 0
                  AND child.archived = 0
                  AND block.in_trash = 0
             )
         SELECT page.id, page.title
         FROM notes_pages page
         JOIN bridge_pages ON bridge_pages.id = page.id
         ORDER BY lower(page.title) ASC, page.id ASC"
    );
    let mut query = sqlx::query(&sql);
    for id in requested_page_ids {
        query = query.bind(id);
    }
    rows_to_pages(
        query
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load agent bridge page descendants: {e}"))?,
    )
}

fn rows_to_pages(rows: Vec<sqlx::sqlite::SqliteRow>) -> Result<Vec<BridgePage>, String> {
    rows.into_iter()
        .map(|row| {
            Ok(BridgePage {
                id: row
                    .try_get("id")
                    .map_err(|e| format!("read bridge page id: {e}"))?,
                title: row
                    .try_get("title")
                    .map_err(|e| format!("read bridge page title: {e}"))?,
            })
        })
        .collect()
}

async fn load_database_views(
    pool: &SqlitePool,
    page_ids: &[String],
    diagnostics: &mut Vec<NoteAgentBridgeExportDiagnosticDto>,
) -> Result<Vec<BridgeDatabaseView>, String> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT block.id AS block_id,
                page.title AS page_title,
                block.payload AS payload
         FROM notes_blocks block
         JOIN notes_pages page ON page.id = block.page_id
         WHERE block.in_trash = 0
           AND block.type = 'child_database'
           AND block.page_id IN ({})
         ORDER BY lower(page.title) ASC, block.sort_order ASC, block.id ASC",
        placeholders(page_ids.len())
    );
    let mut query = sqlx::query(&sql);
    for id in page_ids {
        query = query.bind(id);
    }
    let rows = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load agent bridge database blocks: {e}"))?;
    let mut views = Vec::new();
    for row in rows {
        let block_id: String = row
            .try_get("block_id")
            .map_err(|e| format!("read database block id: {e}"))?;
        let page_title: String = row
            .try_get("page_title")
            .map_err(|e| format!("read database block page title: {e}"))?;
        let payload_raw: String = row
            .try_get("payload")
            .map_err(|e| format!("read database block payload: {e}"))?;
        let payload: Value = serde_json::from_str(&payload_raw)
            .map_err(|e| format!("parse database block payload: {e}"))?;
        let Some(data_source_id) = payload.get("data_source_id").and_then(Value::as_str) else {
            diagnostics.push(NoteAgentBridgeExportDiagnosticDto::new(
                "agent_bridge_database_missing_source",
                "warning",
                Some("block"),
                Some(block_id.clone()),
                "Child database block has no data source id.",
            ));
            continue;
        };
        let database_id = payload
            .get("database_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let view_id = payload
            .get("view_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let export = match data_sources::csv_export::export_csv(
            pool,
            data_source_id,
            NoteDataSourceCsvExportRequest {
                database_id: database_id.clone(),
                view_id: view_id.clone(),
                scope: Some("view".to_string()),
            },
        )
        .await
        {
            Ok(export) => export,
            Err(error) => {
                diagnostics.push(NoteAgentBridgeExportDiagnosticDto::new(
                    "agent_bridge_database_export_failed",
                    "warning",
                    Some("block"),
                    Some(block_id),
                    format!("Database view was not exported: {error}"),
                ));
                continue;
            }
        };
        for diagnostic in &export.diagnostics {
            diagnostics.push(NoteAgentBridgeExportDiagnosticDto::new(
                diagnostic.code.clone(),
                diagnostic.severity.clone(),
                Some("data_source"),
                Some(data_source_id.to_string()),
                diagnostic.message.clone(),
            ));
        }
        views.push(BridgeDatabaseView {
            block_id,
            page_title,
            title: payload
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Database")
                .to_string(),
            data_source_id: export.data_source_id,
            database_id: export.database_id,
            view_id: export.view_id,
            markdown_table: render::csv_to_markdown_table(&export.csv)?,
            exported_row_count: export.exported_row_count,
        });
    }
    Ok(views)
}

async fn load_backlinks(
    pool: &SqlitePool,
    page_ids: &[String],
) -> Result<Vec<BridgeBacklink>, String> {
    if page_ids.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT backlink.target_id,
                target.title AS target_title,
                backlink.source_page_id,
                source.title AS source_page_title,
                backlink.source_block_id,
                backlink.source_comment_id,
                backlink.reference_type,
                backlink.snippet
         FROM notes_backlink_index backlink
         JOIN notes_pages target ON target.id = backlink.target_id
         JOIN notes_pages source ON source.id = backlink.source_page_id
         WHERE backlink.target_type = 'page'
           AND backlink.target_id IN ({})
         ORDER BY lower(target.title) ASC,
                  lower(source.title) ASC,
                  backlink.reference_type ASC,
                  backlink.id ASC",
        placeholders(page_ids.len())
    );
    let mut query = sqlx::query(&sql);
    for id in page_ids {
        query = query.bind(id);
    }
    query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load agent bridge backlinks: {e}"))?
        .into_iter()
        .map(|row| {
            Ok(BridgeBacklink {
                target_id: row
                    .try_get("target_id")
                    .map_err(|e| format!("read backlink target id: {e}"))?,
                target_title: row
                    .try_get("target_title")
                    .map_err(|e| format!("read backlink target title: {e}"))?,
                source_page_id: row
                    .try_get("source_page_id")
                    .map_err(|e| format!("read backlink source page id: {e}"))?,
                source_page_title: row
                    .try_get("source_page_title")
                    .map_err(|e| format!("read backlink source page title: {e}"))?,
                source_block_id: row
                    .try_get("source_block_id")
                    .map_err(|e| format!("read backlink source block id: {e}"))?,
                source_comment_id: row
                    .try_get("source_comment_id")
                    .map_err(|e| format!("read backlink source comment id: {e}"))?,
                reference_type: row
                    .try_get("reference_type")
                    .map_err(|e| format!("read backlink reference type: {e}"))?,
                snippet: row
                    .try_get("snippet")
                    .map_err(|e| format!("read backlink snippet: {e}"))?,
            })
        })
        .collect()
}

async fn project_scope_ids(
    pool: &SqlitePool,
    requested_project_ids: &[String],
    page_ids: &[String],
) -> Result<Vec<String>, String> {
    let mut ids = requested_project_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if !page_ids.is_empty() {
        let sql = format!(
            "SELECT DISTINCT project.id
             FROM notes_link_facts fact
             JOIN projects project ON project.id = fact.target_object_id
             WHERE fact.source_page_id IN ({})
               AND fact.target_object_type = 'project'
             UNION
             SELECT DISTINCT task.project_id
             FROM notes_link_facts fact
             JOIN project_tasks task ON task.id = fact.target_object_id
             WHERE fact.source_page_id IN ({})
               AND fact.target_object_type = 'project_task'",
            placeholders(page_ids.len()),
            placeholders(page_ids.len())
        );
        let mut query = sqlx::query_scalar::<_, String>(&sql);
        for id in page_ids {
            query = query.bind(id);
        }
        for id in page_ids {
            query = query.bind(id);
        }
        for id in query
            .fetch_all(pool)
            .await
            .map_err(|e| format!("load agent bridge linked projects: {e}"))?
        {
            ids.insert(id);
        }
    }
    Ok(ids.into_iter().collect())
}

async fn load_projects(
    pool: &SqlitePool,
    project_ids: &[String],
    diagnostics: &mut Vec<NoteAgentBridgeExportDiagnosticDto>,
) -> Result<Vec<BridgeProject>, String> {
    if project_ids.is_empty() {
        return Ok(Vec::new());
    }
    let project_sql = format!(
        "SELECT project.id,
                project.name,
                project.status,
                COALESCE(group_row.name, '') AS group_name
         FROM projects project
         LEFT JOIN project_groups group_row ON group_row.id = project.group_id
         WHERE project.id IN ({})
         ORDER BY lower(group_row.name) ASC, lower(project.name) ASC, project.id ASC",
        placeholders(project_ids.len())
    );
    let mut query = sqlx::query(&project_sql);
    for id in project_ids {
        query = query.bind(id);
    }
    let project_rows = query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load agent bridge projects: {e}"))?;
    let found = project_rows
        .iter()
        .map(|row| row.get::<String, _>("id"))
        .collect::<HashSet<_>>();
    for id in project_ids {
        if !found.contains(id) {
            diagnostics.push(NoteAgentBridgeExportDiagnosticDto::new(
                "agent_bridge_project_not_exported",
                "warning",
                Some("project"),
                Some(id.clone()),
                "Requested project was not found.",
            ));
        }
    }
    let mut projects = Vec::new();
    for row in project_rows {
        let id: String = row
            .try_get("id")
            .map_err(|e| format!("read bridge project id: {e}"))?;
        projects.push(BridgeProject {
            tasks: load_project_tasks(pool, &id).await?,
            id,
            name: row
                .try_get("name")
                .map_err(|e| format!("read bridge project name: {e}"))?,
            status: row
                .try_get("status")
                .map_err(|e| format!("read bridge project status: {e}"))?,
            group_name: row
                .try_get("group_name")
                .map_err(|e| format!("read bridge project group: {e}"))?,
        });
    }
    Ok(projects)
}

async fn load_project_tasks(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<Vec<BridgeTask>, String> {
    let rows = sqlx::query(
        "SELECT task.id,
                task.title,
                task.description,
                task.priority,
                task.task_type,
                task.estimate_minutes,
                task.start_date,
                task.due_date,
                task.target_end_date,
                task.completed_at,
                task.blocker_reason,
                section.name AS section_name,
                status.name AS status_name,
                parent.title AS parent_title
         FROM project_tasks task
         JOIN project_sections section ON section.id = task.section_id
         JOIN project_statuses status ON status.id = task.status_id
         LEFT JOIN project_tasks parent ON parent.id = task.parent_task_id
         WHERE task.project_id = ?
           AND task.archived_at IS NULL
         ORDER BY section.sort_order ASC,
                  task.section_sort_order ASC,
                  task.created_at ASC,
                  task.id ASC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("load agent bridge project tasks: {e}"))?;
    let task_ids = rows
        .iter()
        .map(|row| row.get::<String, _>("id"))
        .collect::<Vec<_>>();
    let checklist = load_checklist_items(pool, &task_ids).await?;
    let tags = load_task_tags(pool, &task_ids).await?;
    rows.into_iter()
        .map(|row| {
            let id: String = row
                .try_get("id")
                .map_err(|e| format!("read bridge task id: {e}"))?;
            Ok(BridgeTask {
                checklist: checklist.get(&id).cloned().unwrap_or_default(),
                tags: tags.get(&id).cloned().unwrap_or_default(),
                id,
                title: row
                    .try_get("title")
                    .map_err(|e| format!("read bridge task title: {e}"))?,
                description: row
                    .try_get("description")
                    .map_err(|e| format!("read bridge task description: {e}"))?,
                priority: row
                    .try_get("priority")
                    .map_err(|e| format!("read bridge task priority: {e}"))?,
                task_type: row
                    .try_get("task_type")
                    .map_err(|e| format!("read bridge task type: {e}"))?,
                estimate_minutes: row
                    .try_get("estimate_minutes")
                    .map_err(|e| format!("read bridge task estimate: {e}"))?,
                start_date: row
                    .try_get("start_date")
                    .map_err(|e| format!("read bridge task start date: {e}"))?,
                due_date: row
                    .try_get("due_date")
                    .map_err(|e| format!("read bridge task due date: {e}"))?,
                target_end_date: row
                    .try_get("target_end_date")
                    .map_err(|e| format!("read bridge task target end date: {e}"))?,
                completed_at: row
                    .try_get("completed_at")
                    .map_err(|e| format!("read bridge task completed at: {e}"))?,
                blocker_reason: row
                    .try_get("blocker_reason")
                    .map_err(|e| format!("read bridge task blocker: {e}"))?,
                section_name: row
                    .try_get("section_name")
                    .map_err(|e| format!("read bridge task section: {e}"))?,
                status_name: row
                    .try_get("status_name")
                    .map_err(|e| format!("read bridge task status: {e}"))?,
                parent_title: row
                    .try_get("parent_title")
                    .map_err(|e| format!("read bridge task parent: {e}"))?,
            })
        })
        .collect()
}

async fn load_checklist_items(
    pool: &SqlitePool,
    task_ids: &[String],
) -> Result<HashMap<String, Vec<BridgeChecklistItem>>, String> {
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT task_id, title, completed_at
         FROM project_checklist_items
         WHERE task_id IN ({})
         ORDER BY task_id ASC, sort_order ASC, id ASC",
        placeholders(task_ids.len())
    );
    let mut query = sqlx::query(&sql);
    for id in task_ids {
        query = query.bind(id);
    }
    let mut by_task = HashMap::<String, Vec<BridgeChecklistItem>>::new();
    for row in query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load agent bridge task checklist: {e}"))?
    {
        let task_id: String = row
            .try_get("task_id")
            .map_err(|e| format!("read checklist task id: {e}"))?;
        by_task
            .entry(task_id)
            .or_default()
            .push(BridgeChecklistItem {
                title: row
                    .try_get("title")
                    .map_err(|e| format!("read checklist title: {e}"))?,
                completed: row
                    .try_get::<Option<String>, _>("completed_at")
                    .map_err(|e| format!("read checklist completion: {e}"))?
                    .is_some(),
            });
    }
    Ok(by_task)
}

async fn load_task_tags(
    pool: &SqlitePool,
    task_ids: &[String],
) -> Result<HashMap<String, Vec<String>>, String> {
    if task_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let sql = format!(
        "SELECT link.task_id, tag.name
         FROM project_task_tag_links link
         JOIN project_tags tag ON tag.id = link.tag_id
         WHERE link.task_id IN ({})
         ORDER BY link.task_id ASC, lower(tag.name) ASC, tag.id ASC",
        placeholders(task_ids.len())
    );
    let mut query = sqlx::query(&sql);
    for id in task_ids {
        query = query.bind(id);
    }
    let mut by_task = HashMap::<String, Vec<String>>::new();
    for row in query
        .fetch_all(pool)
        .await
        .map_err(|e| format!("load agent bridge task tags: {e}"))?
    {
        let task_id: String = row
            .try_get("task_id")
            .map_err(|e| format!("read task tag task id: {e}"))?;
        by_task.entry(task_id).or_default().push(
            row.try_get("name")
                .map_err(|e| format!("read task tag name: {e}"))?,
        );
    }
    Ok(by_task)
}

fn placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn require_markdown_extension(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        Ok(())
    } else {
        Err("agent bridge export path must end in .md".to_string())
    }
}

fn write_markdown_file(path: &Path, contents: &str) -> Result<(), String> {
    let temporary_path = temporary_markdown_path(path)?;
    let result = fs::File::create(&temporary_path)
        .map_err(|e| format!("create agent bridge export: {e}"))
        .and_then(|mut file| {
            file.write_all(contents.as_bytes())
                .map_err(|e| format!("write agent bridge export: {e}"))
        })
        .and_then(|()| {
            fs::rename(&temporary_path, path).map_err(|e| format!("save agent bridge export: {e}"))
        });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn temporary_markdown_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "agent bridge export path has no file name".to_string())?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}

use crate::models::{
    NoteJsonGraphExportDiagnosticDto, NoteJsonGraphExportDto, NoteJsonGraphExportRequest,
    NoteJsonGraphExportSaveDto,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const EXPORT_VERSION: i64 = 1;
const GRAPH_SCHEMA_VERSION: &str = "notes-json-graph.v1";
const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";

#[derive(Clone, Copy, Debug, Serialize)]
struct ExportOptions {
    include_indexes: bool,
    include_history: bool,
    include_templates: bool,
    include_local_state: bool,
    pretty: bool,
}

impl ExportOptions {
    fn from_request(request: &NoteJsonGraphExportRequest) -> Self {
        Self {
            include_indexes: request.include_indexes.unwrap_or(true),
            include_history: request.include_history.unwrap_or(true),
            include_templates: request.include_templates.unwrap_or(true),
            include_local_state: request.include_local_state.unwrap_or(true),
            pretty: request.pretty.unwrap_or(true),
        }
    }

    fn allows(self, family: TableFamily) -> bool {
        match family {
            TableFamily::Core => true,
            TableFamily::Index => self.include_indexes,
            TableFamily::History => self.include_history,
            TableFamily::Template => self.include_templates,
            TableFamily::LocalState => self.include_local_state,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableFamily {
    Core,
    Index,
    History,
    Template,
    LocalState,
}

struct ExportTable {
    name: String,
    section: &'static str,
    family: TableFamily,
    columns: Vec<String>,
}

struct ExportedTable {
    table: ExportTable,
    rows: Vec<Value>,
}

pub async fn export_graph(
    pool: &SqlitePool,
    request: NoteJsonGraphExportRequest,
) -> Result<NoteJsonGraphExportDto, String> {
    let options = ExportOptions::from_request(&request);
    let generated_at = current_timestamp(pool).await?;
    let database_schema = database_schema_summary(pool).await?;
    let mut diagnostics = option_diagnostics(options);
    let mut exported_tables = Vec::new();

    for table in discover_notes_tables(pool).await? {
        if !options.allows(table.family) {
            continue;
        }
        let rows = export_table_rows(pool, &table).await?;
        if table.section == "misc" {
            diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
                "json_graph_unknown_notes_table",
                "info",
                Some(table.name.clone()),
                None::<String>,
                "A Notes table without a dedicated graph section was exported under misc.",
            ));
        }
        exported_tables.push(ExportedTable { table, rows });
    }

    diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
        "json_graph_rebuildable_fts_omitted",
        "warning",
        Some("notes_search_fts"),
        None::<String>,
        "SQLite FTS virtual tables and shadow tables are omitted because notes_search_index is exported and can rebuild them.",
    ));

    let counts = build_counts(&exported_tables);
    let summary = build_summary(&exported_tables, &diagnostics);
    if summary.exported_file_count > 0 {
        diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
            "json_graph_asset_bytes_not_embedded",
            "warning",
            Some("notes_assets"),
            None::<String>,
            "Managed asset metadata is exported, but binary file bytes stay in the Ganbaru assets folder.",
        ));
    }
    let summary = build_summary(&exported_tables, &diagnostics);
    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == "warning")
        .count() as i64;
    let file_name = default_file_name(&generated_at);
    let root = json!({
        "object": "notes_json_graph",
        "export_version": EXPORT_VERSION,
        "schema_version": GRAPH_SCHEMA_VERSION,
        "generated_at": generated_at,
        "options": options,
        "database_schema": database_schema,
        "counts": counts,
        "diagnostics": diagnostics,
        "graph": build_graph(exported_tables),
    });
    let json = if options.pretty {
        serde_json::to_string_pretty(&root)
    } else {
        serde_json::to_string(&root)
    }
    .map_err(|e| format!("serialize Notes JSON graph export: {e}"))?;

    Ok(NoteJsonGraphExportDto {
        object: "notes_json_graph_export",
        export_version: EXPORT_VERSION,
        schema_version: GRAPH_SCHEMA_VERSION.to_string(),
        generated_at,
        file_name,
        content_type: JSON_CONTENT_TYPE,
        byte_size: json.len() as i64,
        json,
        counts,
        diagnostics,
        exported_page_count: summary.exported_page_count,
        exported_block_count: summary.exported_block_count,
        exported_comment_count: summary.exported_comment_count,
        exported_data_source_count: summary.exported_data_source_count,
        exported_file_count: summary.exported_file_count,
        exported_index_record_count: summary.exported_index_record_count,
        exported_property_schema_count: summary.exported_property_schema_count,
        exported_table_count: summary.exported_table_count,
        exported_record_count: summary.exported_record_count,
        warning_count,
    })
}

pub async fn project_history_source_rows(
    pool: &SqlitePool,
) -> Result<BTreeMap<String, Vec<Value>>, String> {
    let excluded = [
        "notes_backlink_index",
        "notes_backlink_index_state",
        "notes_collaboration_operations",
        "notes_data_source_relation_links",
        "notes_data_source_rollup_cache",
        "notes_link_facts",
        "notes_link_facts_state",
        "notes_local_users",
        "notes_page_history_settings",
        "notes_page_history_snapshots",
        "notes_page_template_blocks",
        "notes_page_templates",
        "notes_search_index",
        "notes_search_index_state",
        "notes_undo_state",
        "notes_unresolved_link_index",
        "notes_unresolved_link_index_state",
    ];
    let mut rows_by_table = BTreeMap::new();
    for table in discover_notes_tables(pool).await? {
        if table.family == TableFamily::History
            || table.family == TableFamily::Index
            || excluded.contains(&table.name.as_str())
        {
            continue;
        }
        rows_by_table.insert(table.name.clone(), export_table_rows(pool, &table).await?);
    }
    Ok(rows_by_table)
}

pub fn write_graph(
    path: &Path,
    export: NoteJsonGraphExportDto,
) -> Result<NoteJsonGraphExportSaveDto, String> {
    require_json_extension(path)?;
    write_json_file(path, &export.json)?;
    Ok(NoteJsonGraphExportSaveDto::saved(export))
}

async fn discover_notes_tables(pool: &SqlitePool) -> Result<Vec<ExportTable>, String> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_schema
         WHERE type = 'table'
           AND name LIKE 'notes_%'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list Notes graph export tables: {e}"))?;
    let mut tables = Vec::new();
    for row in rows {
        let name: String = row
            .try_get("name")
            .map_err(|e| format!("read Notes graph export table name: {e}"))?;
        if should_skip_table(&name) {
            continue;
        }
        let columns = table_columns(pool, &name).await?;
        tables.push(ExportTable {
            section: table_section(&name),
            family: table_family(&name),
            name,
            columns,
        });
    }
    Ok(tables)
}

async fn table_columns(pool: &SqlitePool, table: &str) -> Result<Vec<String>, String> {
    let sql = format!("PRAGMA table_info({})", quote_identifier(table));
    let rows = sqlx::query(AssertSqlSafe(sql.as_str()))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("inspect Notes graph export table {table}: {e}"))?;
    rows.into_iter()
        .map(|row| {
            row.try_get("name")
                .map_err(|e| format!("read Notes graph export column for {table}: {e}"))
        })
        .collect()
}

async fn export_table_rows(pool: &SqlitePool, table: &ExportTable) -> Result<Vec<Value>, String> {
    if table.columns.is_empty() {
        return Ok(Vec::new());
    }
    let args = table
        .columns
        .iter()
        .flat_map(|column| {
            [
                quote_string_literal(column),
                column_value_expression(&table.name, column),
            ]
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT json_object({args}) AS row_json FROM {} ORDER BY {}",
        quote_identifier(&table.name),
        order_by_columns(&table.name, &table.columns)
    );
    let rows = sqlx::query_scalar::<_, String>(AssertSqlSafe(sql.as_str()))
        .fetch_all(pool)
        .await
        .map_err(|e| format!("export Notes graph table {}: {e}", table.name))?;
    rows.into_iter()
        .map(|row_json| {
            serde_json::from_str(&row_json)
                .map_err(|e| format!("decode Notes graph row from {}: {e}", table.name))
        })
        .collect()
}

fn should_skip_table(name: &str) -> bool {
    name.starts_with("notes_search_fts")
        || name.starts_with("notes_history_")
        || name.starts_with("notes_project_history_")
        || matches!(
            name,
            "notes_blocks_next" | "notes_pages_next" | "notes_page_history_snapshots_next"
        )
}

fn table_family(name: &str) -> TableFamily {
    if name.contains("_index")
        || name == "notes_link_facts"
        || name.ends_with("_index_state")
        || name == "notes_link_facts_state"
    {
        return TableFamily::Index;
    }
    if name.contains("_history_") || name == "notes_page_history_settings" {
        return TableFamily::History;
    }
    if name.contains("_template") {
        return TableFamily::Template;
    }
    if matches!(
        name,
        "notes_undo_state"
            | "notes_local_users"
            | "notes_comment_thread_reads"
            | "notes_comment_thread_mutes"
            | "notes_page_notification_settings"
            | "notes_mention_notifications"
            | "notes_collaboration_operations"
    ) {
        return TableFamily::LocalState;
    }
    TableFamily::Core
}

fn table_section(name: &str) -> &'static str {
    if matches!(name, "notes_pages" | "notes_page_aliases" | "notes_folders") {
        return "pages";
    }
    if name == "notes_blocks" {
        return "blocks";
    }
    if name.starts_with("notes_comment")
        || matches!(name, "notes_suggestions" | "notes_collaboration_operations")
    {
        return "comments";
    }
    if name.starts_with("notes_database")
        || name.starts_with("notes_data_source")
        || name == "notes_databases"
    {
        return "data_sources";
    }
    if matches!(
        name,
        "notes_assets"
            | "notes_asset_references"
            | "notes_page_icon_assets"
            | "notes_page_cover_assets"
    ) {
        return "files";
    }
    if table_family(name) == TableFamily::Index {
        return "indexes";
    }
    if table_family(name) == TableFamily::History {
        return "history";
    }
    if table_family(name) == TableFamily::Template {
        return "templates";
    }
    if table_family(name) == TableFamily::LocalState {
        return "local_state";
    }
    "misc"
}

fn json_column(table: &str, column: &str) -> bool {
    matches!(
        (table, column),
        ("notes_pages", "properties")
            | ("notes_pages", "icon")
            | ("notes_pages", "cover")
            | ("notes_blocks", "payload")
            | ("notes_databases", "title_rich_text")
            | ("notes_databases", "description")
            | ("notes_databases", "icon")
            | ("notes_databases", "cover")
            | ("notes_data_sources", "title_rich_text")
            | ("notes_data_sources", "description")
            | ("notes_data_sources", "icon")
            | ("notes_data_sources", "properties")
            | ("notes_database_views", "filter")
            | ("notes_database_views", "sorts")
            | ("notes_database_views", "configuration")
            | ("notes_page_history_snapshots", "properties")
            | ("notes_page_history_snapshots", "icon")
            | ("notes_page_history_snapshots", "cover")
            | ("notes_page_history_snapshots", "blocks")
            | ("notes_page_templates", "properties")
            | ("notes_page_templates", "icon")
            | ("notes_page_templates", "cover")
            | ("notes_page_template_blocks", "payload")
            | ("notes_data_source_templates", "properties")
            | ("notes_data_source_template_blocks", "payload")
            | ("notes_comments", "rich_text")
            | ("notes_comments", "display_name")
            | ("notes_comments", "attachments")
            | ("notes_comment_reactions", "display_name")
            | ("notes_suggestions", "display_name")
            | ("notes_collaboration_operations", "actor_display_name")
            | ("notes_collaboration_operations", "payload")
            | ("notes_data_source_rollup_cache", "value")
            | ("notes_undo_state", "state_json")
    )
}

fn column_value_expression(table: &str, column: &str) -> String {
    let quoted = quote_identifier(column);
    if json_column(table, column) {
        format!(
            "CASE WHEN {quoted} IS NULL THEN NULL WHEN json_valid({quoted}) THEN json({quoted}) ELSE {quoted} END"
        )
    } else {
        quoted
    }
}

fn order_by_columns(table: &str, columns: &[String]) -> String {
    let preferred = match table {
        "notes_blocks" => vec![
            "page_id",
            "parent_type",
            "parent_page_id",
            "parent_block_id",
            "sort_order",
            "id",
        ],
        "notes_asset_references" => vec!["owner_type", "owner_id", "role", "asset_id"],
        "notes_data_source_relation_links" => {
            vec!["source_page_id", "source_property_id", "target_page_id"]
        }
        "notes_data_source_rollup_cache" => vec!["source_page_id", "source_property_id"],
        "notes_collaboration_operations" => vec!["sequence"],
        _ => vec!["id", "page_id", "created_time", "updated_at"],
    };
    let order_columns = preferred
        .into_iter()
        .filter(|name| columns.iter().any(|column| column == name))
        .map(quote_identifier)
        .collect::<Vec<_>>();
    if order_columns.is_empty() {
        columns
            .iter()
            .map(|column| quote_identifier(column))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        order_columns.join(", ")
    }
}

fn build_graph(tables: Vec<ExportedTable>) -> Value {
    let mut sections = Map::new();
    for exported in tables {
        let section = sections
            .entry(exported.table.section.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(section) = section.as_object_mut() {
            section.insert(exported.table.name, Value::Array(exported.rows));
        }
    }
    Value::Object(sections)
}

fn build_counts(tables: &[ExportedTable]) -> Value {
    let mut table_counts = Map::new();
    let mut section_counts = BTreeMap::<String, i64>::new();
    let mut total_records = 0_i64;
    for exported in tables {
        let count = exported.rows.len() as i64;
        table_counts.insert(exported.table.name.clone(), json!(count));
        *section_counts
            .entry(exported.table.section.to_string())
            .or_insert(0) += count;
        total_records += count;
    }
    json!({
        "tables": table_counts,
        "sections": section_counts,
        "total_tables": tables.len() as i64,
        "total_records": total_records,
    })
}

#[derive(Default)]
struct ExportSummary {
    exported_page_count: i64,
    exported_block_count: i64,
    exported_comment_count: i64,
    exported_data_source_count: i64,
    exported_file_count: i64,
    exported_index_record_count: i64,
    exported_property_schema_count: i64,
    exported_table_count: i64,
    exported_record_count: i64,
}

fn build_summary(
    tables: &[ExportedTable],
    _diagnostics: &[NoteJsonGraphExportDiagnosticDto],
) -> ExportSummary {
    let mut summary = ExportSummary {
        exported_table_count: tables.len() as i64,
        ..ExportSummary::default()
    };
    for exported in tables {
        let count = exported.rows.len() as i64;
        summary.exported_record_count += count;
        match exported.table.name.as_str() {
            "notes_pages" => summary.exported_page_count = count,
            "notes_blocks" => summary.exported_block_count = count,
            "notes_comments" => summary.exported_comment_count = count,
            "notes_data_sources" => {
                summary.exported_data_source_count = count;
                summary.exported_property_schema_count = exported
                    .rows
                    .iter()
                    .filter_map(|row| row.get("properties"))
                    .filter_map(Value::as_object)
                    .map(|properties| properties.len() as i64)
                    .sum();
            }
            "notes_assets" => summary.exported_file_count = count,
            _ => {
                if exported.table.family == TableFamily::Index {
                    summary.exported_index_record_count += count;
                }
            }
        }
    }
    summary
}

fn option_diagnostics(options: ExportOptions) -> Vec<NoteJsonGraphExportDiagnosticDto> {
    let mut diagnostics = Vec::new();
    if !options.include_indexes {
        diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
            "json_graph_indexes_excluded",
            "warning",
            None::<String>,
            None::<String>,
            "Rebuildable Notes search, backlink, unresolved link, and link fact index rows were excluded.",
        ));
    }
    if !options.include_history {
        diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
            "json_graph_history_excluded",
            "warning",
            None::<String>,
            None::<String>,
            "Notes page history snapshots were excluded from this export.",
        ));
    }
    if !options.include_templates {
        diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
            "json_graph_templates_excluded",
            "warning",
            None::<String>,
            None::<String>,
            "Notes page and data source templates were excluded from this export.",
        ));
    }
    if !options.include_local_state {
        diagnostics.push(NoteJsonGraphExportDiagnosticDto::new(
            "json_graph_local_state_excluded",
            "warning",
            None::<String>,
            None::<String>,
            "Local editor, notification, unread, and collaboration queue state was excluded from this export.",
        ));
    }
    diagnostics
}

async fn current_timestamp(pool: &SqlitePool) -> Result<String, String> {
    sqlx::query_scalar("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("generate Notes JSON graph export timestamp: {e}"))
}

async fn database_schema_summary(pool: &SqlitePool) -> Result<Value, String> {
    let row = sqlx::query(
        "SELECT
            COALESCE(MAX(version), 0) AS latest_version,
            COUNT(*) AS applied_migration_count
         FROM _sqlx_migrations
         WHERE success = 1",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| format!("read Notes JSON graph schema version: {e}"))?;
    let latest_version: i64 = row
        .try_get("latest_version")
        .map_err(|e| format!("read latest schema version: {e}"))?;
    let applied_migration_count: i64 = row
        .try_get("applied_migration_count")
        .map_err(|e| format!("read applied migration count: {e}"))?;
    Ok(json!({
        "migration_table": "_sqlx_migrations",
        "latest_version": latest_version.to_string(),
        "applied_migration_count": applied_migration_count,
    }))
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn default_file_name(generated_at: &str) -> String {
    let stamp = generated_at
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("ganbaru-notes-json-graph-{stamp}.json")
}

fn require_json_extension(path: &Path) -> Result<(), String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        Ok(())
    } else {
        Err("Notes JSON graph export path must end in .json".to_string())
    }
}

fn write_json_file(path: &Path, contents: &str) -> Result<(), String> {
    let temporary_path = temporary_json_path(path)?;
    let result = write_json_file_inner(&temporary_path, contents).and_then(|()| {
        fs::rename(&temporary_path, path).map_err(|e| format!("save JSON graph: {e}"))
    });
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn write_json_file_inner(path: &Path, contents: &str) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|e| format!("create JSON graph: {e}"))?;
    file.write_all(contents.as_bytes())
        .map_err(|e| format!("write JSON graph: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("sync JSON graph: {e}"))?;
    Ok(())
}

fn temporary_json_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| "JSON graph path has no file name".to_string())?
        .to_string_lossy();
    Ok(path.with_file_name(format!("{file_name}.tmp")))
}

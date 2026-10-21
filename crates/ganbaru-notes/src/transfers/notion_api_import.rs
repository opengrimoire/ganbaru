pub mod cancellation;
pub mod client;
pub mod convert;
#[cfg(test)]
mod test_server;
pub mod writer;

pub use self::cancellation::NotionImportCancellation;
use self::cancellation::NotionImportStop;
use self::client::{MAX_IMPORT_REQUESTS, NotionApiClient, NotionApiError, canonical_notion_id};
use self::convert::{
    FetchedNotionBlock, NotionConvertOptions, NotionConvertStats, convert_data_source,
    convert_data_source_row, convert_page,
};
use self::writer::{
    create_imported_notion_data_source, create_imported_notion_page, import_comments,
    rebuild_indexes_after_import,
};
use crate::models::{
    NoteLoadedPage, NoteNotionApiImportDiagnosticDto, NoteNotionApiImportDto,
    NoteNotionApiImportRequest, NoteNotionApiImportSummary, NoteNotionApiImportedObjectDto,
    NoteNotionApiImportedUserDto,
};
use crate::transfers::import_writer::{count_import_blocks, normalized_import_project_id};
use crate::validation::{validate_page_size, validate_parent};
use serde_json::Value;
use sqlx::SqlitePool;
use std::future::Future;
use std::pin::Pin;

const DEFAULT_PAGE_SIZE: i64 = 100;
const MAX_SOURCE_IDS: usize = 50;
/// Longest pasted page or data source reference, an ID or a Notion link.
const MAX_SOURCE_ID_INPUT_CHARS: usize = 2_048;
const MAX_IMPORTED_BLOCKS: usize = 5_000;
/// Blocks one import may fetch across every page and row. Data source rows stay in memory until
/// their source is written, so this bounds memory as well as time.
const MAX_TOTAL_IMPORTED_BLOCKS: usize = 100_000;
/// Comments one import may collect; later comments are skipped with a warning.
const MAX_TOTAL_IMPORTED_COMMENTS: usize = 20_000;
/// Longest client-chosen import ID accepted for cancellation.
pub const MAX_IMPORT_ID_CHARS: usize = 128;

/// Imports Notion pages and data sources through the Notion API.
///
/// Each source is written once it is fully fetched. When `cancellation` fires or an aggregate
/// request or block budget runs out, the source in progress and later sources are skipped,
/// sources already written are kept, and the summary carries a warning diagnostic.
pub async fn import_from_api(
    pool: &SqlitePool,
    request: NoteNotionApiImportRequest,
    cancellation: &NotionImportCancellation,
) -> Result<NoteNotionApiImportDto, String> {
    let prepared = PreparedNotionImport::new(request)?;
    let client = NotionApiClient::new(
        prepared.integration_token.clone(),
        prepared.page_size,
        cancellation.clone(),
    )?;
    run_import(pool, &prepared, ImportRun::new(client)).await
}

/// Validates a client-chosen import ID used to address cancellation.
pub fn validate_import_id(value: &str) -> Result<&str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > MAX_IMPORT_ID_CHARS
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err("Notion import ID is invalid".to_string());
    }
    Ok(trimmed)
}

async fn run_import(
    pool: &SqlitePool,
    prepared: &PreparedNotionImport,
    mut run: ImportRun,
) -> Result<NoteNotionApiImportDto, String> {
    if prepared.include_users {
        run.list_users().await;
    }
    if let Err(error) = run.import_sources(pool, prepared).await {
        let Some(stop) = run.stop_reason() else {
            if !run.imported_pages.is_empty() {
                rebuild_indexes_after_import(pool)
                    .await
                    .map_err(|rebuild_error| format!("{error}; {rebuild_error}"))?;
            }
            return Err(error);
        };
        let diagnostic = stop_diagnostic(stop, run.max_blocks, run.current_source.take());
        run.diagnostics.push(diagnostic);
    }
    rebuild_indexes_after_import(pool).await?;
    Ok(run.into_dto())
}

fn stop_diagnostic(
    stop: NotionImportStop,
    max_blocks: usize,
    source_id: Option<String>,
) -> NoteNotionApiImportDiagnosticDto {
    let (code, reason) = match stop {
        NotionImportStop::Canceled => {
            ("import_canceled", "Notion import was canceled.".to_string())
        }
        NotionImportStop::RequestLimit => (
            "import_limit_reached",
            format!("Notion import stopped after {MAX_IMPORT_REQUESTS} API requests."),
        ),
        NotionImportStop::BlockLimit => (
            "import_limit_reached",
            format!("Notion import stopped after {max_blocks} blocks."),
        ),
    };
    NoteNotionApiImportDiagnosticDto::new(
        code,
        "warning",
        source_id,
        format!(
            "{reason} Sources finished before the stop were imported; import this source and later ones again, in smaller batches if a limit was reached."
        ),
    )
}

/// Mutable state of one import: the API client, aggregate budgets, and imported results.
struct ImportRun {
    client: NotionApiClient,
    diagnostics: Vec<NoteNotionApiImportDiagnosticDto>,
    imported_pages: Vec<NoteLoadedPage>,
    imported_data_sources: Vec<NoteNotionApiImportedObjectDto>,
    imported_users: Vec<NoteNotionApiImportedUserDto>,
    stats: NotionConvertStats,
    imported_block_count: i64,
    imported_comment_count: i64,
    fetched_block_count: usize,
    fetched_comment_count: usize,
    max_blocks: usize,
    max_comments: usize,
    comment_limit_reported: bool,
    stop: Option<NotionImportStop>,
    current_source: Option<String>,
}

impl ImportRun {
    fn new(client: NotionApiClient) -> Self {
        Self {
            client,
            diagnostics: Vec::new(),
            imported_pages: Vec::new(),
            imported_data_sources: Vec::new(),
            imported_users: Vec::new(),
            stats: NotionConvertStats::default(),
            imported_block_count: 0,
            imported_comment_count: 0,
            fetched_block_count: 0,
            fetched_comment_count: 0,
            max_blocks: MAX_TOTAL_IMPORTED_BLOCKS,
            max_comments: MAX_TOTAL_IMPORTED_COMMENTS,
            comment_limit_reported: false,
            stop: None,
            current_source: None,
        }
    }

    /// Lowers the aggregate block and comment budgets so tests can reach them.
    #[cfg(test)]
    fn with_limits(mut self, max_blocks: usize, max_comments: usize) -> Self {
        self.max_blocks = max_blocks;
        self.max_comments = max_comments;
        self
    }

    fn stop_reason(&self) -> Option<NotionImportStop> {
        self.stop.or(self.client.stop_reason())
    }

    /// Refuses to write a source once the import stopped, since a stop can end comment
    /// fetches that otherwise only warn.
    fn ensure_running(&self) -> Result<(), String> {
        match self.stop_reason() {
            Some(_) => Err("Notion import stopped".to_string()),
            None => Ok(()),
        }
    }

    async fn import_sources(
        &mut self,
        pool: &SqlitePool,
        prepared: &PreparedNotionImport,
    ) -> Result<(), String> {
        for source_page_id in &prepared.page_ids {
            self.current_source = Some(source_page_id.clone());
            self.import_page(pool, prepared, source_page_id).await?;
        }
        for source_data_source_id in &prepared.data_source_ids {
            self.current_source = Some(source_data_source_id.clone());
            self.import_data_source(pool, prepared, source_data_source_id)
                .await?;
        }
        self.current_source = None;
        Ok(())
    }

    async fn import_page(
        &mut self,
        pool: &SqlitePool,
        prepared: &PreparedNotionImport,
        source_page_id: &str,
    ) -> Result<(), String> {
        let raw_page = self
            .client
            .retrieve_page(source_page_id)
            .await
            .map_err(|e| api_error("retrieve Notion page", source_page_id, e))?;
        let mut comments = Vec::new();
        if prepared.include_comments {
            self.append_comments(source_page_id, &mut comments).await;
        }
        let mut block_count = 0;
        let blocks = self
            .fetch_block_children_recursive(
                source_page_id,
                prepared.include_comments,
                &mut comments,
                &mut block_count,
            )
            .await?;
        self.ensure_running()?;
        let converted = convert_page(
            &raw_page,
            blocks,
            &prepared.options,
            &mut self.diagnostics,
            &mut self.stats,
        );
        self.imported_block_count += count_import_blocks(&converted.blocks) as i64;
        let page_source_id = converted.source_id.clone();
        let page = create_imported_notion_page(
            pool,
            &prepared.parent,
            prepared.source_workspace_id.as_deref(),
            prepared.project_id.as_deref(),
            converted,
        )
        .await?;
        self.imported_pages.push(page);
        if prepared.include_comments {
            self.imported_comment_count += import_comments(
                pool,
                &page_source_id,
                prepared.source_workspace_id.as_deref(),
                &comments,
                prepared.options.keep_external_file_references,
                &mut self.diagnostics,
            )
            .await?;
        }
        Ok(())
    }

    async fn import_data_source(
        &mut self,
        pool: &SqlitePool,
        prepared: &PreparedNotionImport,
        source_data_source_id: &str,
    ) -> Result<(), String> {
        let raw_data_source = self
            .client
            .retrieve_data_source(source_data_source_id)
            .await
            .map_err(|e| api_error("retrieve Notion data source", source_data_source_id, e))?;
        let raw_rows = self
            .client
            .query_data_source(source_data_source_id)
            .await
            .map_err(|e| api_error("query Notion data source", source_data_source_id, e))?;
        let converted_data_source = convert_data_source(&raw_data_source, &mut self.diagnostics);
        let mut converted_rows = Vec::new();
        let mut row_comment_batches = Vec::new();
        for raw_row in raw_rows {
            let Some(source_row_id) = raw_row.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut comments = Vec::new();
            if prepared.include_comments {
                self.append_comments(source_row_id, &mut comments).await;
            }
            let mut block_count = 0;
            let blocks = self
                .fetch_block_children_recursive(
                    source_row_id,
                    prepared.include_comments,
                    &mut comments,
                    &mut block_count,
                )
                .await?;
            let converted = convert_data_source_row(
                &raw_row,
                blocks,
                comments,
                &converted_data_source.properties,
                &prepared.options,
                &mut self.diagnostics,
                &mut self.stats,
            );
            row_comment_batches.push((converted.source_id.clone(), converted.comments.clone()));
            converted_rows.push(converted);
        }
        self.ensure_running()?;
        let (pages, data_source, block_count) = create_imported_notion_data_source(
            pool,
            &prepared.parent,
            "notion",
            prepared.source_workspace_id.as_deref(),
            prepared.project_id.as_deref(),
            converted_data_source,
            converted_rows,
        )
        .await?;
        self.imported_pages.extend(pages);
        self.imported_block_count += block_count;
        self.imported_data_sources.push(data_source);
        if prepared.include_comments {
            for (source_row_id, comments) in row_comment_batches {
                self.imported_comment_count += import_comments(
                    pool,
                    &source_row_id,
                    prepared.source_workspace_id.as_deref(),
                    &comments,
                    prepared.options.keep_external_file_references,
                    &mut self.diagnostics,
                )
                .await?;
            }
        }
        Ok(())
    }

    fn fetch_block_children_recursive<'a>(
        &'a mut self,
        block_id: &'a str,
        include_comments: bool,
        comments: &'a mut Vec<Value>,
        block_count: &'a mut usize,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<FetchedNotionBlock>, String>> + Send + 'a>> {
        Box::pin(async move {
            if *block_count >= MAX_IMPORTED_BLOCKS {
                return Err(format!(
                    "Notion API import supports up to {MAX_IMPORTED_BLOCKS} blocks per page or row"
                ));
            }
            let remaining_blocks = MAX_IMPORTED_BLOCKS - *block_count;
            let children = self
                .client
                .list_block_children(block_id, remaining_blocks)
                .await
                .map_err(|e| api_error("list Notion block children", block_id, e))?;
            self.fetched_block_count += children.len();
            if self.fetched_block_count > self.max_blocks {
                self.stop.get_or_insert(NotionImportStop::BlockLimit);
                return Err(format!(
                    "Notion import reached the limit of {} blocks",
                    self.max_blocks
                ));
            }
            let mut fetched = Vec::with_capacity(children.len());
            for child in children {
                *block_count += 1;
                let child_id = child.get("id").and_then(Value::as_str).unwrap_or_default();
                if include_comments && !child_id.is_empty() {
                    self.append_comments(child_id, comments).await;
                }
                let has_children = child
                    .get("has_children")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let nested = if has_children && !child_id.is_empty() {
                    self.fetch_block_children_recursive(
                        child_id,
                        include_comments,
                        comments,
                        block_count,
                    )
                    .await?
                } else {
                    Vec::new()
                };
                fetched.push(FetchedNotionBlock {
                    raw: child,
                    children: nested,
                });
            }
            Ok(fetched)
        })
    }

    async fn append_comments(&mut self, block_id: &str, comments: &mut Vec<Value>) {
        if self.fetched_comment_count >= self.max_comments {
            self.report_comment_limit();
            return;
        }
        match self.client.list_comments(block_id).await {
            Ok(values) => {
                let room = self.max_comments - self.fetched_comment_count;
                if values.len() > room {
                    self.report_comment_limit();
                }
                let accepted = values.into_iter().take(room);
                let before = comments.len();
                comments.extend(accepted);
                self.fetched_comment_count += comments.len() - before;
            }
            Err(_) if self.client.stop_reason().is_some() => {}
            Err(error) => {
                self.diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
                    "comments_unavailable",
                    "warning",
                    Some(block_id.to_string()),
                    format!(
                        "Notion comments could not be imported for this object: {}",
                        error.message
                    ),
                ));
            }
        }
    }

    fn report_comment_limit(&mut self) {
        if self.comment_limit_reported {
            return;
        }
        self.comment_limit_reported = true;
        self.diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
            "comments_limit_reached",
            "warning",
            None::<String>,
            format!(
                "Notion import collected {} comments; later comments were skipped. Import fewer sources at once to keep them.",
                self.max_comments
            ),
        ));
    }

    async fn list_users(&mut self) {
        match self.client.list_users().await {
            Ok(users) => {
                self.imported_users = users
                    .into_iter()
                    .filter_map(|user| {
                        let id = user.get("id").and_then(Value::as_str)?;
                        let name = user
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Notion user");
                        let user_type = user
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown");
                        Some(NoteNotionApiImportedUserDto::new(id, name, user_type))
                    })
                    .collect();
            }
            Err(_) if self.client.stop_reason().is_some() => {}
            Err(error) => {
                self.diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
                    "users_unavailable",
                    "warning",
                    None::<String>,
                    format!("Notion users could not be listed: {}", error.message),
                ));
            }
        }
    }

    fn into_dto(self) -> NoteNotionApiImportDto {
        let client_stats = self.client.stats();
        NoteNotionApiImportDto::new(NoteNotionApiImportSummary {
            imported_pages: self.imported_pages,
            imported_data_sources: self.imported_data_sources,
            imported_users: self.imported_users,
            diagnostics: self.diagnostics,
            request_count: client_stats.request_count,
            retry_count: client_stats.retry_count,
            rate_limit_count: client_stats.rate_limit_count,
            imported_block_count: self.imported_block_count,
            imported_comment_count: self.imported_comment_count,
            imported_file_count: self.stats.imported_file_count,
            unsupported_block_count: self.stats.unsupported_block_count,
        })
    }
}

struct PreparedNotionImport {
    parent: crate::models::NoteParent,
    integration_token: String,
    source_workspace_id: Option<String>,
    project_id: Option<String>,
    page_ids: Vec<String>,
    data_source_ids: Vec<String>,
    include_comments: bool,
    include_users: bool,
    page_size: i64,
    options: NotionConvertOptions,
}

impl PreparedNotionImport {
    fn new(request: NoteNotionApiImportRequest) -> Result<Self, String> {
        validate_parent(&request.parent)?;
        let integration_token = request.integration_token.trim().to_string();
        if integration_token.is_empty() {
            return Err("Notion integration token is required".to_string());
        }
        let page_ids = validated_source_ids(request.page_ids, "page_ids")?;
        let data_source_ids = validated_source_ids(request.data_source_ids, "data_source_ids")?;
        if page_ids.is_empty() && data_source_ids.is_empty() {
            return Err("provide at least one Notion page ID or data source ID".to_string());
        }
        if page_ids.len() + data_source_ids.len() > MAX_SOURCE_IDS {
            return Err(format!(
                "Notion API import supports up to {MAX_SOURCE_IDS} source IDs at once"
            ));
        }
        let page_size = request.page_size.unwrap_or(DEFAULT_PAGE_SIZE);
        validate_page_size(page_size)?;
        Ok(Self {
            parent: request.parent,
            integration_token,
            source_workspace_id: request
                .source_workspace_id
                .and_then(|value| trimmed_non_empty(&value)),
            project_id: normalized_import_project_id(request.project_id),
            page_ids,
            data_source_ids,
            include_comments: request.include_comments.unwrap_or(true),
            include_users: request.include_users.unwrap_or(true),
            page_size,
            options: NotionConvertOptions {
                keep_external_file_references: request
                    .keep_external_file_references
                    .unwrap_or(false),
            },
        })
    }
}

fn validated_source_ids(values: Vec<String>, field: &str) -> Result<Vec<String>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let id = source_id_from_input(trimmed)
            .ok_or_else(|| format!("{field} contains an invalid Notion object ID"))?;
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Canonical Notion ID from a pasted reference: a bare ID, or a Notion link whose last path
/// segment is the ID or ends with `-` and the 32-digit ID, as in `Page-Title-<id>`.
fn source_id_from_input(value: &str) -> Option<String> {
    if value.chars().count() > MAX_SOURCE_ID_INPUT_CHARS
        || value.chars().any(|character| character.is_control())
    {
        return None;
    }
    if let Some(id) = canonical_notion_id(value) {
        return Some(id);
    }
    let without_suffix = value.split(['?', '#']).next().unwrap_or_default();
    let segment = without_suffix
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if let Some(id) = canonical_notion_id(segment) {
        return Some(id);
    }
    let split = segment.len().checked_sub(32)?;
    let (title, id) = (segment.get(..split)?, segment.get(split..)?);
    if title.ends_with('-') {
        canonical_notion_id(id)
    } else {
        None
    }
}

fn trimmed_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn api_error(action: &str, source_id: &str, error: NotionApiError) -> String {
    match error.status {
        Some(status) => format!("{action} {source_id}: {status}: {}", error.message),
        None => format!("{action} {source_id}: {}", error.message),
    }
}

#[cfg(test)]
mod tests {
    use super::test_server::{ScriptedResponse, start_scripted_server};
    use super::*;
    use crate::models::NoteParent;
    use reqwest::Url;
    use serde_json::json;

    const PAGE_1: &str = "0a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
    const PAGE_2: &str = "1a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
    const BLOCK_1: &str = "2a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
    const BLOCK_2: &str = "3a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";
    const BLOCK_3: &str = "4a1b2c3d-4e5f-4a6b-8c7d-8e9f0a1b2c3d";

    async fn migrated_memory_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::run_migrations(&pool).await.unwrap();
        pool
    }

    fn prepared_page_import(page_ids: &[&str], include_comments: bool) -> PreparedNotionImport {
        PreparedNotionImport::new(NoteNotionApiImportRequest {
            parent: NoteParent::Workspace { workspace: true },
            integration_token: "bearer_secret".to_string(),
            source_workspace_id: None,
            page_ids: page_ids.iter().map(ToString::to_string).collect(),
            data_source_ids: Vec::new(),
            include_comments: Some(include_comments),
            include_users: Some(false),
            keep_external_file_references: None,
            page_size: None,
            project_id: None,
            import_id: None,
        })
        .unwrap()
    }

    fn notion_page(id: &str, title: &str) -> ScriptedResponse {
        ScriptedResponse::json(
            "200 OK",
            json!({
                "object": "page",
                "id": id,
                "properties": {
                    "Name": { "type": "title", "title": [{ "type": "text", "plain_text": title, "text": { "content": title } }] }
                }
            }),
        )
    }

    fn paragraph_children(id: &str, text: &str) -> ScriptedResponse {
        paragraphs_children(&[(id, text)])
    }

    fn paragraphs_children(paragraphs: &[(&str, &str)]) -> ScriptedResponse {
        let results = paragraphs
            .iter()
            .map(|(id, text)| {
                json!({
                    "object": "block",
                    "id": id,
                    "type": "paragraph",
                    "has_children": false,
                    "paragraph": { "rich_text": [{ "type": "text", "plain_text": text, "text": { "content": text } }] }
                })
            })
            .collect::<Vec<_>>();
        ScriptedResponse::json(
            "200 OK",
            json!({ "results": results, "has_more": false, "next_cursor": null }),
        )
    }

    fn comments_on(parent: Value, comments: &[(&str, &str)]) -> ScriptedResponse {
        let results = comments
            .iter()
            .map(|(id, text)| {
                json!({
                    "object": "comment",
                    "id": id,
                    "discussion_id": format!("discussion-{id}"),
                    "parent": parent,
                    "rich_text": [{ "type": "text", "plain_text": text, "text": { "content": text } }]
                })
            })
            .collect::<Vec<_>>();
        ScriptedResponse::json(
            "200 OK",
            json!({ "results": results, "has_more": false, "next_cursor": null }),
        )
    }

    fn diagnostic_codes(summary: &Value) -> Vec<&str> {
        summary["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|diagnostic| diagnostic["code"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn request_budget_stop_keeps_finished_pages_and_reports_the_rest() {
        sqlx::__rt::test_block_on(async {
            let pool = migrated_memory_pool().await;
            let (base_url, requests, server) = start_scripted_server(vec![
                notion_page(PAGE_1, "First import"),
                paragraph_children(BLOCK_1, "kept paragraph"),
            ]);
            let client =
                NotionApiClient::for_test(base_url, 100, NotionImportCancellation::new(), 2);

            let summary = run_import(
                &pool,
                &prepared_page_import(&[PAGE_1, PAGE_2], false),
                ImportRun::new(client),
            )
            .await
            .expect("a budget stop returns a summary");

            let summary = serde_json::to_value(summary).unwrap();
            assert_eq!(summary["imported_page_count"], 1);
            assert_eq!(summary["imported_block_count"], 1);
            assert_eq!(summary["request_count"], 2);
            let diagnostic = summary["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .find(|diagnostic| diagnostic["code"] == "import_limit_reached")
                .expect("the stop is reported");
            assert_eq!(diagnostic["severity"], "warning");
            assert_eq!(diagnostic["source_object_id"], PAGE_2);
            let results = crate::search::search(&pool, "kept paragraph", Some(10), false)
                .await
                .unwrap();
            assert!(!results.is_empty());
            server.join().unwrap();
            assert_eq!(requests.try_iter().count(), 2);
        });
    }

    #[test]
    fn canceled_import_sends_no_requests_and_reports_cancellation() {
        sqlx::__rt::test_block_on(async {
            let pool = migrated_memory_pool().await;
            let cancellation = NotionImportCancellation::new();
            cancellation.cancel();
            let base_url = Url::parse("http://127.0.0.1:9/v1/").unwrap();
            let client = NotionApiClient::for_test(base_url, 100, cancellation, 10);

            let summary = run_import(
                &pool,
                &prepared_page_import(&[PAGE_1], false),
                ImportRun::new(client),
            )
            .await
            .expect("a canceled import returns a summary");

            let summary = serde_json::to_value(summary).unwrap();
            assert_eq!(summary["imported_page_count"], 0);
            assert_eq!(summary["request_count"], 0);
            assert_eq!(summary["diagnostics"][0]["code"], "import_canceled");
            assert_eq!(summary["diagnostics"][0]["source_object_id"], PAGE_1);
        });
    }

    #[test]
    fn stop_during_a_comment_fetch_does_not_write_the_partial_page() {
        sqlx::__rt::test_block_on(async {
            let pool = migrated_memory_pool().await;
            let no_comments = || {
                ScriptedResponse::json(
                    "200 OK",
                    json!({ "results": [], "has_more": false, "next_cursor": null }),
                )
            };
            let (base_url, _, server) = start_scripted_server(vec![
                notion_page(PAGE_1, "Partial import"),
                no_comments(),
                paragraph_children(BLOCK_1, "partial paragraph"),
            ]);
            let client =
                NotionApiClient::for_test(base_url, 100, NotionImportCancellation::new(), 3);

            let summary = run_import(
                &pool,
                &prepared_page_import(&[PAGE_1], true),
                ImportRun::new(client),
            )
            .await
            .expect("a budget stop returns a summary");

            let summary = serde_json::to_value(summary).unwrap();
            assert_eq!(summary["imported_page_count"], 0);
            let codes = summary["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|diagnostic| diagnostic["code"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(codes, vec!["import_limit_reached"]);
            let page_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(page_count, 0);
            server.join().unwrap();
        });
    }

    #[test]
    fn block_budget_stop_keeps_finished_pages_and_skips_the_source_that_crossed_it() {
        sqlx::__rt::test_block_on(async {
            let pool = migrated_memory_pool().await;
            let (base_url, requests, server) = start_scripted_server(vec![
                notion_page(PAGE_1, "Within budget"),
                paragraph_children(BLOCK_1, "kept paragraph"),
                notion_page(PAGE_2, "Over budget"),
                paragraphs_children(&[(BLOCK_2, "first extra"), (BLOCK_3, "second extra")]),
            ]);
            let client =
                NotionApiClient::for_test(base_url, 100, NotionImportCancellation::new(), 100);

            let summary = run_import(
                &pool,
                &prepared_page_import(&[PAGE_1, PAGE_2], false),
                ImportRun::new(client).with_limits(2, 100),
            )
            .await
            .expect("a block budget stop returns a summary");

            let summary = serde_json::to_value(summary).unwrap();
            assert_eq!(summary["imported_page_count"], 1);
            assert_eq!(summary["imported_block_count"], 1);
            assert_eq!(diagnostic_codes(&summary), vec!["import_limit_reached"]);
            let diagnostic = &summary["diagnostics"][0];
            assert_eq!(diagnostic["source_object_id"], PAGE_2);
            assert!(
                diagnostic["message"]
                    .as_str()
                    .unwrap()
                    .contains("stopped after 2 blocks")
            );
            let page_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes_pages")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(page_count, 1);
            server.join().unwrap();
            assert_eq!(requests.try_iter().count(), 4);
        });
    }

    #[test]
    fn comments_past_the_import_budget_are_skipped_without_requests_and_reported_once() {
        sqlx::__rt::test_block_on(async {
            let pool = migrated_memory_pool().await;
            let (base_url, requests, server) = start_scripted_server(vec![
                notion_page(PAGE_1, "Discussed page"),
                comments_on(
                    json!({ "type": "page_id", "page_id": PAGE_1 }),
                    &[("comment-page", "page comment")],
                ),
                paragraphs_children(&[(BLOCK_1, "first"), (BLOCK_2, "second")]),
                comments_on(
                    json!({ "type": "block_id", "block_id": BLOCK_1 }),
                    &[
                        ("comment-kept", "kept block comment"),
                        ("comment-over", "comment over the budget"),
                    ],
                ),
            ]);
            let client =
                NotionApiClient::for_test(base_url, 100, NotionImportCancellation::new(), 100);

            let summary = run_import(
                &pool,
                &prepared_page_import(&[PAGE_1], true),
                ImportRun::new(client).with_limits(100, 2),
            )
            .await
            .expect("a comment budget only warns");

            let summary = serde_json::to_value(summary).unwrap();
            assert_eq!(summary["imported_page_count"], 1);
            assert_eq!(summary["imported_comment_count"], 2);
            assert_eq!(diagnostic_codes(&summary), vec!["comments_limit_reached"]);
            server.join().unwrap();
            assert_eq!(
                requests.try_iter().count(),
                4,
                "the second block's comments are never requested"
            );
            let source_ids: Vec<Option<String>> = sqlx::query_scalar(
                "SELECT source_object_id FROM notes_comments ORDER BY source_object_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                source_ids,
                vec![
                    Some("comment-kept".to_string()),
                    Some("comment-page".to_string())
                ]
            );
        });
    }

    #[test]
    fn source_ids_accept_ids_and_notion_links_in_canonical_form() {
        let ids = validated_source_ids(
            vec![
                " 0A1B2C3D4E5F4A6B8C7D8E9F0A1B2C3D ".to_string(),
                PAGE_1.to_string(),
                "https://www.notion.so/team/Weekly-plan-1a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3d?pvs=4"
                    .to_string(),
                format!("https://www.notion.so/{BLOCK_1}#section"),
            ],
            "page_ids",
        )
        .unwrap();

        assert_eq!(ids, vec![PAGE_1, PAGE_2, BLOCK_1]);
    }

    #[test]
    fn source_ids_refuse_values_that_could_change_the_api_path_or_query() {
        for value in [
            "page-1",
            "../users",
            "0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3d/../../users",
            "0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3d&filter=x",
            "0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3",
            "0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3dd",
            "Weekly0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3d",
            "0a1b2c3d4e5f4a6b8c7d8e9f0a1b2c3g",
        ] {
            assert!(
                validated_source_ids(vec![value.to_string()], "page_ids").is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn import_ids_are_short_ascii_tokens() {
        assert_eq!(validate_import_id(" 0f1e-2d3c ").unwrap(), "0f1e-2d3c");
        assert!(validate_import_id("").is_err());
        assert!(validate_import_id("a\nb").is_err());
        assert!(validate_import_id("a/b").is_err());
        assert!(validate_import_id(&"a".repeat(MAX_IMPORT_ID_CHARS + 1)).is_err());
    }
}

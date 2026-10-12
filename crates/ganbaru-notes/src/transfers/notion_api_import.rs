pub mod client;
pub mod convert;
pub mod writer;

use self::client::{NotionApiClient, NotionApiError};
use self::convert::{
    FetchedNotionBlock, NotionConvertOptions, NotionConvertStats, convert_data_source,
    convert_data_source_row, convert_page,
};
use self::writer::{
    create_imported_notion_data_source, create_imported_notion_page, import_comments,
    rebuild_indexes_after_import,
};
use crate::models::{
    NoteNotionApiImportDiagnosticDto, NoteNotionApiImportDto, NoteNotionApiImportRequest,
    NoteNotionApiImportSummary, NoteNotionApiImportedUserDto,
};
use crate::transfers::import_writer::{count_import_blocks, normalized_import_project_id};
use crate::validation::{validate_page_size, validate_parent};
use serde_json::Value;
use sqlx::SqlitePool;
use std::future::Future;
use std::pin::Pin;

const DEFAULT_PAGE_SIZE: i64 = 100;
const MAX_SOURCE_IDS: usize = 50;
const MAX_IMPORTED_BLOCKS: usize = 5_000;

pub async fn import_from_api(
    pool: &SqlitePool,
    request: NoteNotionApiImportRequest,
) -> Result<NoteNotionApiImportDto, String> {
    let prepared = PreparedNotionImport::new(request)?;
    let mut client = NotionApiClient::new(prepared.integration_token, prepared.page_size)?;
    let mut diagnostics = Vec::new();
    let mut imported_pages = Vec::new();
    let mut imported_data_sources = Vec::new();
    let mut imported_users = Vec::new();
    let mut stats = NotionConvertStats::default();
    let mut imported_block_count = 0;
    let mut imported_comment_count = 0;

    if prepared.include_users {
        imported_users = list_users(&mut client, &mut diagnostics).await;
    }

    for source_page_id in &prepared.page_ids {
        let raw_page = client
            .retrieve_page(source_page_id)
            .await
            .map_err(|e| api_error("retrieve Notion page", source_page_id, e))?;
        let mut comments = Vec::new();
        if prepared.include_comments {
            append_comments(&mut client, source_page_id, &mut comments, &mut diagnostics).await;
        }
        let mut block_count = 0;
        let blocks = fetch_block_children_recursive(
            &mut client,
            source_page_id,
            prepared.include_comments,
            &mut comments,
            &mut diagnostics,
            &mut block_count,
        )
        .await?;
        let converted = convert_page(
            &raw_page,
            blocks,
            &prepared.options,
            &mut diagnostics,
            &mut stats,
        );
        imported_block_count += count_import_blocks(&converted.blocks) as i64;
        let page_source_id = converted.source_id.clone();
        let page = create_imported_notion_page(
            pool,
            &prepared.parent,
            prepared.source_workspace_id.as_deref(),
            prepared.project_id.as_deref(),
            converted,
        )
        .await?;
        imported_pages.push(page);
        if prepared.include_comments {
            imported_comment_count += import_comments(
                pool,
                &page_source_id,
                prepared.source_workspace_id.as_deref(),
                &comments,
                prepared.options.keep_external_file_references,
                &mut diagnostics,
            )
            .await?;
        }
    }

    for source_data_source_id in &prepared.data_source_ids {
        let raw_data_source = client
            .retrieve_data_source(source_data_source_id)
            .await
            .map_err(|e| api_error("retrieve Notion data source", source_data_source_id, e))?;
        let raw_rows = client
            .query_data_source(source_data_source_id)
            .await
            .map_err(|e| api_error("query Notion data source", source_data_source_id, e))?;
        let converted_data_source = convert_data_source(&raw_data_source, &mut diagnostics);
        let mut converted_rows = Vec::new();
        let mut row_comment_batches = Vec::new();
        for raw_row in raw_rows {
            let Some(source_row_id) = raw_row.get("id").and_then(Value::as_str) else {
                continue;
            };
            let mut comments = Vec::new();
            if prepared.include_comments {
                append_comments(&mut client, source_row_id, &mut comments, &mut diagnostics).await;
            }
            let mut block_count = 0;
            let blocks = fetch_block_children_recursive(
                &mut client,
                source_row_id,
                prepared.include_comments,
                &mut comments,
                &mut diagnostics,
                &mut block_count,
            )
            .await?;
            let converted = convert_data_source_row(
                &raw_row,
                blocks,
                comments,
                &converted_data_source.properties,
                &prepared.options,
                &mut diagnostics,
                &mut stats,
            );
            row_comment_batches.push((converted.source_id.clone(), converted.comments.clone()));
            converted_rows.push(converted);
        }
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
        imported_pages.extend(pages);
        imported_block_count += block_count;
        imported_data_sources.push(data_source);
        if prepared.include_comments {
            for (source_row_id, comments) in row_comment_batches {
                imported_comment_count += import_comments(
                    pool,
                    &source_row_id,
                    prepared.source_workspace_id.as_deref(),
                    &comments,
                    prepared.options.keep_external_file_references,
                    &mut diagnostics,
                )
                .await?;
            }
        }
    }

    rebuild_indexes_after_import(pool).await?;
    let client_stats = client.stats();
    Ok(NoteNotionApiImportDto::new(NoteNotionApiImportSummary {
        imported_pages,
        imported_data_sources,
        imported_users,
        diagnostics,
        request_count: client_stats.request_count,
        retry_count: client_stats.retry_count,
        rate_limit_count: client_stats.rate_limit_count,
        imported_block_count,
        imported_comment_count,
        imported_file_count: stats.imported_file_count,
        unsupported_block_count: stats.unsupported_block_count,
    }))
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

fn fetch_block_children_recursive<'a>(
    client: &'a mut NotionApiClient,
    block_id: &'a str,
    include_comments: bool,
    comments: &'a mut Vec<Value>,
    diagnostics: &'a mut Vec<NoteNotionApiImportDiagnosticDto>,
    block_count: &'a mut usize,
) -> Pin<Box<dyn Future<Output = Result<Vec<FetchedNotionBlock>, String>> + Send + 'a>> {
    Box::pin(async move {
        if *block_count >= MAX_IMPORTED_BLOCKS {
            return Err(format!(
                "Notion API import supports up to {MAX_IMPORTED_BLOCKS} blocks per page or row"
            ));
        }
        let remaining_blocks = MAX_IMPORTED_BLOCKS - *block_count;
        let children = client
            .list_block_children(block_id, remaining_blocks)
            .await
            .map_err(|e| api_error("list Notion block children", block_id, e))?;
        let mut fetched = Vec::with_capacity(children.len());
        for child in children {
            *block_count += 1;
            let child_id = child.get("id").and_then(Value::as_str).unwrap_or_default();
            if include_comments && !child_id.is_empty() {
                append_comments(client, child_id, comments, diagnostics).await;
            }
            let has_children = child
                .get("has_children")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let nested = if has_children && !child_id.is_empty() {
                fetch_block_children_recursive(
                    client,
                    child_id,
                    include_comments,
                    comments,
                    diagnostics,
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

async fn append_comments(
    client: &mut NotionApiClient,
    block_id: &str,
    comments: &mut Vec<Value>,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) {
    match client.list_comments(block_id).await {
        Ok(mut values) => {
            comments.append(&mut values);
        }
        Err(error) => {
            diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
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

async fn list_users(
    client: &mut NotionApiClient,
    diagnostics: &mut Vec<NoteNotionApiImportDiagnosticDto>,
) -> Vec<NoteNotionApiImportedUserDto> {
    match client.list_users().await {
        Ok(users) => users
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
            .collect(),
        Err(error) => {
            diagnostics.push(NoteNotionApiImportDiagnosticDto::new(
                "users_unavailable",
                "warning",
                None::<String>,
                format!("Notion users could not be listed: {}", error.message),
            ));
            Vec::new()
        }
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
        if trimmed.len() > 200
            || trimmed.contains('/')
            || trimmed.chars().any(|character| character.is_control())
        {
            return Err(format!("{field} contains an invalid Notion object ID"));
        }
        if seen.insert(trimmed.to_string()) {
            ids.push(trimmed.to_string());
        }
    }
    Ok(ids)
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

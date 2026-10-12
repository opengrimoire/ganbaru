use super::*;

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteDatabaseRow {
    pub id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub title: String,
    pub title_rich_text: String,
    pub description: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub is_inline: i64,
    pub in_trash: i64,
    pub source_provider: Option<String>,
    pub source_object_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub source_last_edited_time: Option<String>,
    pub url: Option<String>,
    pub public_url: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteDataSourceRow {
    pub id: String,
    pub database_id: String,
    pub title: String,
    pub title_rich_text: String,
    pub description: String,
    pub icon: Option<String>,
    pub properties: String,
    pub in_trash: i64,
    pub source_provider: Option<String>,
    pub source_object_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub source_last_edited_time: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
}

impl NoteDataSourceRow {
    pub fn summary(&self) -> NoteDatabaseDataSourceSummaryDto {
        NoteDatabaseDataSourceSummaryDto::new(self.id.clone(), self.title.clone())
    }
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteDatabaseViewRow {
    pub id: String,
    pub database_id: String,
    pub data_source_id: String,
    pub name: String,
    pub view_type: String,
    pub filter: Option<String>,
    pub sorts: String,
    pub configuration: Option<String>,
    pub source_provider: Option<String>,
    pub source_object_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub source_last_edited_time: Option<String>,
    pub url: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageRow {
    pub id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub parent_data_source_id: Option<String>,
    pub folder_id: Option<String>,
    pub title: String,
    pub properties: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub in_trash: i64,
    pub archived: i64,
    pub source_provider: Option<String>,
    pub source_object_id: Option<String>,
    pub source_workspace_id: Option<String>,
    pub source_last_edited_time: Option<String>,
    pub url: Option<String>,
    pub public_url: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteFolderRow {
    pub id: String,
    pub project_id: String,
    pub parent_folder_id: Option<String>,
    pub name: String,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageTemplateRow {
    pub id: String,
    pub name: String,
    pub source_page_id: Option<String>,
    pub properties: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub block_count: i64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteDataSourceTemplateRow {
    pub id: String,
    pub data_source_id: String,
    pub source_page_id: Option<String>,
    pub name: String,
    pub properties: String,
    pub is_default: i64,
    pub block_count: i64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageHistorySnapshotRow {
    pub id: String,
    pub page_id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub parent_data_source_id: Option<String>,
    pub folder_id: Option<String>,
    pub title: String,
    pub properties: String,
    pub icon: Option<String>,
    pub cover: Option<String>,
    pub in_trash: i64,
    pub archived: i64,
    pub blocks: String,
    pub block_bundle_hash: Option<String>,
    pub block_count: i64,
    pub reason: String,
    pub created_by: String,
    pub created_time: String,
    pub page_created_time: String,
    pub page_last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteLocalUserRow {
    pub id: String,
    pub display_name: String,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NotePageHistorySettingsRow {
    pub retention_days: Option<i64>,
    pub updated_at: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NoteMentionNotificationRow {
    pub id: String,
    pub source_type: String,
    pub source_id: String,
    pub page_id: String,
    pub page_title: String,
    pub block_id: Option<String>,
    pub comment_id: Option<String>,
    pub kind: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub trigger_at: Option<String>,
    /// Minutes a reminder fires before `trigger_at`; 0 for mentions and reminders at their date.
    pub remind_minutes_before: i64,
    pub plain_text: String,
    pub source_plain_text: String,
    pub status: String,
    pub delivered_at: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
    /// The current local user muted this page or discussion, so delivery stays silent.
    pub suppressed: bool,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageAliasRow {
    pub id: String,
    pub page_id: String,
    pub alias: String,
    pub normalized_alias: String,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteUnresolvedLinkRow {
    pub id: String,
    pub source_type: String,
    pub source_page_id: String,
    pub source_block_id: Option<String>,
    pub source_comment_id: Option<String>,
    pub raw_url: String,
    pub raw_target: String,
    pub normalized_target: String,
    pub link_text: String,
    pub snippet: String,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Deserialize, Serialize, sqlx::FromRow)]
pub struct NoteBlockRow {
    pub id: String,
    pub page_id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub has_children: i64,
    pub in_trash: i64,
    #[serde(rename = "type")]
    pub block_type: String,
    pub payload: String,
    pub plain_text: String,
    pub sort_order: f64,
    pub source_provider: Option<String>,
    pub source_object_id: Option<String>,
    pub source_last_edited_time: Option<String>,
    pub created_time: String,
    pub last_edited_time: String,
}

impl NoteBlockRow {
    /// Fingerprint the exact canonical block state, including placement and lifecycle.
    pub fn edit_revision(&self) -> Result<String, String> {
        use sha2::{Digest, Sha256};
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("encode Notes block revision: {error}"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageTemplateBlockRow {
    pub template_id: String,
    pub id: String,
    pub parent_type: String,
    pub parent_block_id: Option<String>,
    pub has_children: i64,
    pub block_type: String,
    pub payload: String,
    pub plain_text: String,
    pub sort_order: f64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NoteDataSourceTemplateBlockRow {
    pub template_id: String,
    pub id: String,
    pub parent_type: String,
    pub parent_block_id: Option<String>,
    pub has_children: i64,
    pub block_type: String,
    pub payload: String,
    pub plain_text: String,
    pub sort_order: f64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NoteCommentThreadRow {
    pub id: String,
    pub page_id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub status: String,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    pub sync_version: i64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NoteCommentAnchorRow {
    pub thread_id: String,
    pub page_id: String,
    pub block_id: String,
    pub start_offset: i64,
    pub end_offset: i64,
    pub anchor_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NoteCommentRow {
    pub id: String,
    pub thread_id: String,
    pub rich_text: String,
    pub plain_text: String,
    pub created_by: String,
    pub display_name: String,
    pub attachments: String,
    pub deleted_at: Option<String>,
    pub sync_version: i64,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct NoteSuggestionRow {
    pub id: String,
    pub page_id: String,
    pub block_id: String,
    pub created_by: String,
    pub display_name: String,
    pub status: String,
    pub range_start: i64,
    pub range_end: i64,
    pub original_text: String,
    pub proposed_text: String,
    pub prefix_text: String,
    pub suffix_text: String,
    pub accepted_at: Option<String>,
    pub accepted_by: Option<String>,
    pub rejected_at: Option<String>,
    pub rejected_by: Option<String>,
    pub sync_version: i64,
    pub created_time: String,
    pub last_edited_time: String,
}

pub fn parent_columns(parent: &NoteParent) -> (&'static str, Option<&str>, Option<&str>) {
    match parent {
        NoteParent::Workspace { .. } => ("workspace", None, None),
        NoteParent::PageId { page_id } => ("page_id", Some(page_id.as_str()), None),
        NoteParent::BlockId { block_id } => ("block_id", None, Some(block_id.as_str())),
        NoteParent::DataSourceId { .. } => ("data_source_id", None, None),
    }
}

pub fn page_parent_columns(
    parent: &NoteParent,
) -> (&'static str, Option<&str>, Option<&str>, Option<&str>) {
    match parent {
        NoteParent::Workspace { .. } => ("workspace", None, None, None),
        NoteParent::PageId { page_id } => ("page_id", Some(page_id.as_str()), None, None),
        NoteParent::BlockId { block_id } => ("block_id", None, Some(block_id.as_str()), None),
        NoteParent::DataSourceId { data_source_id } => {
            ("data_source_id", None, None, Some(data_source_id.as_str()))
        }
    }
}

pub fn parent_from_row(
    parent_type: &str,
    parent_page_id: Option<String>,
    parent_block_id: Option<String>,
    parent_data_source_id: Option<String>,
) -> Result<NoteParent, String> {
    match parent_type {
        "workspace" => Ok(NoteParent::Workspace { workspace: true }),
        "page_id" => parent_page_id
            .map(|page_id| NoteParent::PageId { page_id })
            .ok_or_else(|| "stored page parent is missing page_id".to_string()),
        "block_id" => parent_block_id
            .map(|block_id| NoteParent::BlockId { block_id })
            .ok_or_else(|| "stored block parent is missing block_id".to_string()),
        "data_source_id" => parent_data_source_id
            .map(|data_source_id| NoteParent::DataSourceId { data_source_id })
            .ok_or_else(|| "stored page parent is missing data_source_id".to_string()),
        _ => Err(format!("unsupported parent type in storage: {parent_type}")),
    }
}

pub fn block_parent_from_database_row(row: &NoteDatabaseRow) -> Result<NoteParent, String> {
    parent_from_row(
        &row.parent_type,
        row.parent_page_id.clone(),
        row.parent_block_id.clone(),
        None,
    )
}

pub fn parse_json(value: String, label: &str) -> Result<Value, String> {
    serde_json::from_str(&value).map_err(|e| format!("parse {label}: {e}"))
}

pub fn parse_optional_json(value: Option<String>, label: &str) -> Result<Option<Value>, String> {
    value.map(|json| parse_json(json, label)).transpose()
}

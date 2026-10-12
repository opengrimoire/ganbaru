use super::*;

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct ProjectHistoryManifest {
    pub(crate) schema_version: i64,
    pub(crate) project_id: String,
    pub(crate) rows_by_table: BTreeMap<String, Vec<String>>,
    pub(crate) asset_ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesProjectHistoryVersionDto {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) manifest_hash: String,
    pub(crate) reason: String,
    pub(crate) created_by: String,
    pub(crate) display_name: Value,
    pub(crate) changed_note_summary: String,
    pub(crate) page_count: i64,
    pub(crate) active_page_count: i64,
    pub(crate) archived_page_count: i64,
    pub(crate) deleted_page_count: i64,
    pub(crate) created_time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesProjectHistoryVersionListDto {
    pub(crate) versions: Vec<NotesProjectHistoryVersionDto>,
    pub(crate) next_cursor_time: Option<String>,
    pub(crate) next_cursor_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesHistoryRetentionImpactDto {
    pub(crate) version_count: i64,
    pub(crate) stored_bytes: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesProjectHistoryScheduleDto {
    pub(crate) created_count: i64,
    pub(crate) next_checkpoint_at: Option<String>,
    pub(crate) next_maintenance_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesMutationResultDto<T: Serialize> {
    pub(crate) value: T,
    pub(crate) next_history_checkpoint_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesProjectHistoryTreeDto {
    pub(crate) version: NotesProjectHistoryVersionDto,
    pub(crate) pages: Vec<NotesHistoricalPageSummaryDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesHistoricalPageSummaryDto {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) parent_page_id: Option<String>,
    pub(crate) parent_data_source_id: Option<String>,
    pub(crate) in_trash: bool,
    pub(crate) archived: bool,
    pub(crate) icon: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesHistoricalPageDto {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) properties: Value,
    pub(crate) icon: Option<Value>,
    pub(crate) cover: Option<Value>,
    pub(crate) in_trash: bool,
    pub(crate) archived: bool,
    pub(crate) blocks: Vec<Value>,
    pub(crate) databases: Vec<Value>,
    pub(crate) data_sources: Vec<Value>,
    pub(crate) database_views: Vec<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesProjectHistoryRestorePlanDto {
    pub(crate) version_id: String,
    pub(crate) remove_count: i64,
    pub(crate) recreate_count: i64,
    pub(crate) change_count: i64,
    pub(crate) copy_count: i64,
    pub(crate) safety_version_will_be_created: bool,
}

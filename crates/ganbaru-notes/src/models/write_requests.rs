use super::*;

#[derive(Deserialize)]
pub struct NotePageCreate {
    pub id: String,
    pub title: String,
    pub parent: NoteParent,
    #[serde(default)]
    pub folder_id: Option<String>,
    pub first_block_id: String,
    pub after_block_id: Option<String>,
    pub properties: Option<Value>,
}

#[derive(Deserialize)]
pub struct NoteFolderCreate {
    pub id: String,
    pub project_id: String,
    pub parent_folder_id: Option<String>,
    pub name: String,
}

#[derive(Deserialize)]
pub struct NoteFolderUpdate {
    pub parent_folder_id: Option<String>,
    pub name: String,
}

#[derive(Deserialize)]
pub struct NoteChildPageFromBlockCreate {
    pub first_block_id: String,
    pub title: Option<String>,
    pub properties: Option<Value>,
}

#[derive(Deserialize)]
pub struct NoteDuplicatePage {
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceRowPageCreate {
    pub id: String,
    pub title: String,
    pub first_block_id: String,
    pub properties: Option<Value>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceCsvImportRequest {
    pub csv: String,
    pub has_header: Option<bool>,
    pub dry_run: Option<bool>,
}

#[derive(Serialize)]
pub struct NoteDataSourceCsvImportColumnDto {
    pub source_index: i64,
    pub source_name: String,
    pub property_id: Option<String>,
    pub property_name: Option<String>,
    pub property_type: Option<String>,
    pub mapped: bool,
    pub read_only: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct NoteDataSourceCsvImportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub row_number: Option<i64>,
    pub column_index: Option<i64>,
    pub column_name: Option<String>,
    pub property_id: Option<String>,
    pub message: String,
}

#[derive(Serialize)]
pub struct NoteDataSourceCsvImportRowDto {
    pub row_number: i64,
    pub title: String,
    pub valid: bool,
    pub mapped_cell_count: i64,
    pub error_count: i64,
}

#[derive(Serialize)]
pub struct NoteDataSourceCsvImportDto {
    pub object: &'static str,
    pub data_source_id: String,
    pub dry_run: bool,
    pub total_row_count: i64,
    pub valid_row_count: i64,
    pub skipped_row_count: i64,
    pub imported_row_count: i64,
    pub imported_page_ids: Vec<String>,
    pub columns: Vec<NoteDataSourceCsvImportColumnDto>,
    pub rows: Vec<NoteDataSourceCsvImportRowDto>,
    pub diagnostics: Vec<NoteDataSourceCsvImportDiagnosticDto>,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceCsvExportRequest {
    pub database_id: Option<String>,
    pub view_id: Option<String>,
    pub scope: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct NoteDataSourceCsvExportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub property_id: Option<String>,
    pub property_name: Option<String>,
    pub message: String,
}

#[derive(Clone, Serialize)]
pub struct NoteDataSourceCsvExportDto {
    pub object: &'static str,
    pub data_source_id: String,
    pub database_id: String,
    pub view_id: String,
    pub scope: String,
    pub file_name: String,
    pub csv: String,
    pub exported_row_count: i64,
    pub exported_property_count: i64,
    pub diagnostics: Vec<NoteDataSourceCsvExportDiagnosticDto>,
}

#[derive(Serialize)]
pub struct NoteDataSourceCsvExportSaveDto {
    saved: bool,
    export: Option<NoteDataSourceCsvExportDto>,
}

impl NoteDataSourceCsvExportSaveDto {
    pub fn saved(export: NoteDataSourceCsvExportDto) -> Self {
        Self {
            saved: true,
            export: Some(export),
        }
    }

    pub fn canceled() -> Self {
        Self {
            saved: false,
            export: None,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteDataSourceTemplateCreateFromRow {
    pub id: String,
    pub source_page_id: String,
    pub name: String,
    pub is_default: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceTemplateApply {
    pub id: Option<String>,
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceTemplateUpdate {
    pub name: Option<String>,
    pub source_page_id: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceTemplateDuplicate {
    pub id: String,
    pub name: String,
    pub is_default: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceButtonClick {
    pub property_id: String,
    pub confirmed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NoteDataSourceTableFilter {
    Predicate(NoteDataSourceTableFilterPredicate),
    Group(NoteDataSourceTableFilterGroup),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceTableFilterPredicate {
    pub property_id: String,
    pub condition: NoteDataSourceFilterCondition,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceTableFilterGroup {
    #[serde(rename = "type")]
    pub operator: NoteDataSourceFilterOperator,
    pub filters: Vec<NoteDataSourceTableFilter>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteDataSourceFilterOperator {
    And,
    Or,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NoteDataSourceFilterCondition {
    Contains,
    Equals,
    NotEquals,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
    Before,
    OnOrBefore,
    After,
    OnOrAfter,
    IsEmpty,
    IsNotEmpty,
    Checked,
    Unchecked,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct NoteDataSourceTableSort {
    pub property_id: String,
    pub direction: String,
}

#[derive(Clone, Default, Deserialize)]
pub struct NoteDataSourceTableConfigurationUpdate {
    pub property_order: Vec<String>,
    pub hidden_property_ids: Vec<String>,
    pub column_widths: Value,
    pub row_open_mode: String,
    #[serde(default)]
    pub group_property_id: Option<String>,
    #[serde(default)]
    pub group_order: Vec<String>,
    #[serde(default)]
    pub collapsed_group_ids: Vec<String>,
    #[serde(default)]
    pub collapsed_row_ids: Vec<String>,
    #[serde(default)]
    pub hide_empty_groups: bool,
    #[serde(default)]
    pub presentation: crate::data_sources::layouts::table::presentation::TablePresentation,
}

#[derive(Deserialize)]
pub struct NoteDataSourceTableViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceTableConfigurationUpdate,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NoteDataSourceViewWindowRequest {
    pub start_cursor: Option<String>,
    pub page_size: Option<i64>,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
}

pub struct NoteDataSourceRowWindow {
    pub rows: Vec<NotePageRow>,
    pub total_row_count: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub group_counts: std::collections::HashMap<String, i64>,
}

#[derive(Deserialize)]
pub struct NoteDataSourceRowPropertyUpdate {
    pub property_id: String,
    pub value: Value,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceBoardConfigurationUpdate {
    pub group_property_id: Option<String>,
    pub group_order: Vec<String>,
    pub hidden_group_ids: Vec<String>,
    pub visible_property_ids: Vec<String>,
    pub row_open_mode: String,
}

#[derive(Deserialize)]
pub struct NoteDataSourceBoardViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceBoardConfigurationUpdate,
}

#[derive(Deserialize)]
pub struct NoteDataSourceBoardRowMove {
    pub page_id: String,
    pub group_id: String,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceGalleryConfigurationUpdate {
    pub cover_source: String,
    pub cover_property_id: Option<String>,
    pub visible_property_ids: Vec<String>,
    pub card_size: String,
    pub fit_image: bool,
    pub row_open_mode: String,
}

#[derive(Deserialize)]
pub struct NoteDataSourceGalleryViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceGalleryConfigurationUpdate,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceListConfigurationUpdate {
    pub group_property_id: Option<String>,
    pub group_order: Vec<String>,
    pub hidden_group_ids: Vec<String>,
    pub visible_property_ids: Vec<String>,
    pub row_open_mode: String,
}

#[derive(Deserialize)]
pub struct NoteDataSourceListViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceListConfigurationUpdate,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceCalendarConfigurationUpdate {
    pub date_property_id: Option<String>,
    pub range_start: String,
    pub range_end: String,
    pub visible_property_ids: Vec<String>,
    pub row_open_mode: String,
}

#[derive(Deserialize)]
pub struct NoteDataSourceCalendarViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceCalendarConfigurationUpdate,
}

#[derive(Clone, Deserialize)]
pub struct NoteDataSourceTimelineConfigurationUpdate {
    pub date_property_id: Option<String>,
    pub group_property_id: Option<String>,
    pub group_order: Vec<String>,
    pub hidden_group_ids: Vec<String>,
    pub range_start: String,
    pub range_end: String,
    pub visible_property_ids: Vec<String>,
    pub row_open_mode: String,
}

#[derive(Deserialize)]
pub struct NoteDataSourceTimelineViewUpdate {
    pub filter: Vec<NoteDataSourceTableFilter>,
    pub sorts: Vec<NoteDataSourceTableSort>,
    pub configuration: NoteDataSourceTimelineConfigurationUpdate,
}

#[derive(Deserialize)]
pub struct NoteMovePage {
    pub parent: NoteParent,
    #[serde(default)]
    pub folder_id: Option<String>,
}

#[derive(Deserialize)]
pub struct NotePageTemplateCreateFromPage {
    pub id: String,
    pub source_page_id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct NotePageTemplateApply {
    pub parent: NoteParent,
    pub title: Option<String>,
}

#[derive(Deserialize)]
pub struct NotePageTemplateUpdate {
    pub name: Option<String>,
    pub source_page_id: Option<String>,
}

#[derive(Deserialize)]
pub struct NotePageTemplateDuplicate {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct NotePageHistorySettingsUpdate {
    pub retention_days: Option<i64>,
}

#[derive(Deserialize)]
pub struct NotePageHistoryCopyBlocks {
    pub after_block_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum OptionalJsonValue {
    #[default]
    Unset,
    Null,
    Value(Value),
}

impl OptionalJsonValue {
    pub fn is_set(&self) -> bool {
        !matches!(self, Self::Unset)
    }

    pub fn value(&self) -> Option<&Value> {
        match self {
            Self::Value(value) => Some(value),
            Self::Unset | Self::Null => None,
        }
    }

    pub fn storage_value(&self) -> Option<String> {
        match self {
            Self::Value(value) => Some(value.to_string()),
            Self::Unset | Self::Null => None,
        }
    }
}

impl<'de> Deserialize<'de> for OptionalJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() {
            Ok(Self::Null)
        } else {
            Ok(Self::Value(value))
        }
    }
}

#[derive(Deserialize)]
pub struct NotePageUpdate {
    pub title: Option<String>,
    pub parent: Option<NoteParent>,
    pub properties: Option<Value>,
    #[serde(default)]
    pub icon: OptionalJsonValue,
    #[serde(default)]
    pub cover: OptionalJsonValue,
}

#[derive(Deserialize, Serialize)]
pub struct NoteBlockWrite {
    pub id: String,
    #[serde(rename = "type")]
    pub block_type: String,
    pub paragraph: Option<Value>,
    pub heading_1: Option<Value>,
    pub heading_2: Option<Value>,
    pub heading_3: Option<Value>,
    pub heading_4: Option<Value>,
    pub heading_5: Option<Value>,
    pub heading_6: Option<Value>,
    pub bulleted_list_item: Option<Value>,
    pub numbered_list_item: Option<Value>,
    pub to_do: Option<Value>,
    pub toggle: Option<Value>,
    pub callout: Option<Value>,
    pub quote: Option<Value>,
    pub child_page: Option<Value>,
    pub child_database: Option<Value>,
    pub breadcrumb: Option<Value>,
    pub table_of_contents: Option<Value>,
    pub column_list: Option<Value>,
    pub column: Option<Value>,
    pub table: Option<Value>,
    pub table_row: Option<Value>,
    pub tab: Option<Value>,
    pub image: Option<Value>,
    pub video: Option<Value>,
    pub audio: Option<Value>,
    pub file: Option<Value>,
    pub pdf: Option<Value>,
    pub bookmark: Option<Value>,
    pub link_preview: Option<Value>,
    pub synced_block: Option<Value>,
    pub template: Option<Value>,
    pub button: Option<Value>,
    pub embed: Option<Value>,
    pub equation: Option<Value>,
    pub divider: Option<Value>,
    pub code: Option<Value>,
    pub unsupported: Option<Value>,
}

impl NoteBlockWrite {
    pub fn payload(&self) -> Option<&Value> {
        payload_refs::payload_from_write(self)
    }
}

#[derive(Deserialize, Serialize)]
pub struct NoteBlockUpdate {
    #[serde(rename = "type")]
    pub block_type: Option<String>,
    pub paragraph: Option<Value>,
    pub heading_1: Option<Value>,
    pub heading_2: Option<Value>,
    pub heading_3: Option<Value>,
    pub heading_4: Option<Value>,
    pub heading_5: Option<Value>,
    pub heading_6: Option<Value>,
    pub bulleted_list_item: Option<Value>,
    pub numbered_list_item: Option<Value>,
    pub to_do: Option<Value>,
    pub toggle: Option<Value>,
    pub callout: Option<Value>,
    pub quote: Option<Value>,
    pub child_page: Option<Value>,
    pub child_database: Option<Value>,
    pub breadcrumb: Option<Value>,
    pub table_of_contents: Option<Value>,
    pub column_list: Option<Value>,
    pub column: Option<Value>,
    pub table: Option<Value>,
    pub table_row: Option<Value>,
    pub tab: Option<Value>,
    pub image: Option<Value>,
    pub video: Option<Value>,
    pub audio: Option<Value>,
    pub file: Option<Value>,
    pub pdf: Option<Value>,
    pub bookmark: Option<Value>,
    pub link_preview: Option<Value>,
    pub synced_block: Option<Value>,
    pub template: Option<Value>,
    pub button: Option<Value>,
    pub embed: Option<Value>,
    pub equation: Option<Value>,
    pub divider: Option<Value>,
    pub code: Option<Value>,
    pub unsupported: Option<Value>,
}

impl NoteBlockUpdate {
    pub fn payload_for(&self, block_type: &str) -> Option<&Value> {
        payload_refs::payload_from_update(self, block_type)
    }
}

#[derive(Deserialize, Serialize)]
pub struct NoteAppendBlockChildren {
    pub parent: NoteParent,
    pub after: Option<String>,
    pub children: Vec<NoteBlockWrite>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteMoveBlock {
    pub parent: NoteParent,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteMoveBlocks {
    pub block_ids: Vec<String>,
    pub parent: NoteParent,
    pub after: Option<String>,
    pub before: Option<String>,
    pub include_trashed_sources: Option<bool>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteDuplicatedBlockId {
    pub source_id: String,
    pub duplicate_id: String,
}

#[derive(Deserialize)]
pub struct NoteDuplicateBlock {
    pub duplicated_block_ids: Vec<NoteDuplicatedBlockId>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteDuplicateBlocks {
    pub block_ids: Vec<String>,
    pub duplicated_block_ids: Vec<NoteDuplicatedBlockId>,
    pub parent: NoteParent,
    pub after: Option<String>,
    pub before: Option<String>,
    pub include_trashed_sources: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteTrashBlocks {
    pub block_ids: Vec<String>,
    pub in_trash: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteCommentCreate {
    pub id: String,
    pub parent: Option<NoteParent>,
    pub discussion_id: Option<String>,
    pub anchor: Option<NoteCommentAnchorCreate>,
    pub rich_text: Vec<Value>,
    #[serde(default)]
    pub attachments: Option<Vec<Value>>,
}

#[derive(Deserialize)]
pub struct NoteCommentAnchorCreate {
    pub start: i64,
    pub end: i64,
    pub text: String,
    pub prefix: String,
    pub suffix: String,
}

#[derive(Deserialize)]
pub struct NoteCommentUpdate {
    pub rich_text: Vec<Value>,
    #[serde(default)]
    pub attachments: Option<Vec<Value>>,
}

#[derive(Deserialize)]
pub struct NoteCommentThreadReadUpdate {
    pub page_id: String,
    pub discussion_ids: Vec<String>,
    pub include_resolved: Option<bool>,
}

#[derive(Deserialize)]
pub struct NoteMentionNotificationDeliveryUpdate {
    pub ids: Vec<String>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteDatabaseCreate {
    pub id: String,
    pub data_source_id: String,
    pub view_id: String,
    pub title: String,
    pub parent: Option<NoteParent>,
    pub after_block_id: Option<String>,
    pub replace_block_id: Option<String>,
    pub icon: Option<Value>,
    pub cover: Option<Value>,
}

#[derive(Deserialize, Serialize)]
pub struct NoteLinkedDatabaseCreate {
    pub id: String,
    pub view_id: String,
    pub source_block_id: String,
    pub title: Option<String>,
    pub parent: Option<NoteParent>,
    pub after_block_id: Option<String>,
    pub replace_block_id: Option<String>,
}

/// Request to copy a database graph into a caller-reserved block identity.
#[derive(Deserialize, Serialize)]
pub struct NoteDatabaseDuplicate {
    pub source_block_id: String,
    pub id: String,
    pub parent: Option<NoteParent>,
    pub after_block_id: Option<String>,
    pub replace_block_id: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteDatabaseViewDuplicate {
    pub id: String,
    pub database_id: String,
    pub source_view_id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub struct NoteDatabaseViewRename {
    pub name: String,
}

#[derive(Deserialize)]
pub struct NoteDatabaseRename {
    pub title: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceSchemaUpdate {
    pub properties: Value,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoteDataSourcePropertyAction {
    Insert {
        property_id: String,
        side: NoteDataSourcePropertyInsertionSide,
        property: Value,
    },
    Duplicate {
        property_id: String,
        name: String,
    },
    /// Convert a non-title property to another type, carrying row values over where they have a meaning.
    ChangeType {
        property_id: String,
        property_type: String,
        /// Type configuration the new type cannot derive, such as a relation target.
        config: Option<Value>,
    },
    Delete {
        property_id: String,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteDataSourcePropertyInsertionSide {
    Left,
    Right,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceCreate {
    pub id: String,
    pub database_id: String,
    pub view_id: String,
    pub title: String,
    pub view_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteDataSourceAttach {
    pub data_source_id: String,
    pub database_id: String,
    pub view_id: String,
    pub view_name: String,
}

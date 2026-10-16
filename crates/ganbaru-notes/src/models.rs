use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests;

mod payload_refs;
mod transfers;
pub use transfers::{
    NoteAgentBridgeExportDiagnosticDto, NoteAgentBridgeExportDto, NoteAgentBridgeExportRequest,
    NoteAgentBridgeExportSaveDto, NoteHtmlArchiveSaveDto, NoteHtmlExportAssetDto,
    NoteHtmlExportDiagnosticDto, NoteHtmlExportDto, NoteHtmlExportFileDto, NoteHtmlExportRequest,
    NoteHtmlImportDiagnosticDto, NoteHtmlImportDto, NoteHtmlImportRequest,
    NoteJsonGraphExportDiagnosticDto, NoteJsonGraphExportDto, NoteJsonGraphExportRequest,
    NoteJsonGraphExportSaveDto, NoteMarkdownExportDiagnosticDto, NoteMarkdownExportDto,
    NoteMarkdownExportRequest, NoteMarkdownImportDiagnosticDto, NoteMarkdownImportDto,
    NoteMarkdownImportRequest, NoteNotionApiImportDiagnosticDto, NoteNotionApiImportDto,
    NoteNotionApiImportRequest, NoteNotionApiImportedObjectDto, NoteNotionApiImportedUserDto,
    NoteNotionExportImportDiagnosticDto, NoteNotionExportImportDto, NoteNotionExportImportRequest,
};
mod workspace;
pub use workspace::{
    NoteDataSourceTemplateDto, NoteNavigationDatabaseDto, NotePageBreadcrumbItemDto,
    NotePageSummaryWindowDto, NotePageSummaryWindowRequest, NotePageTemplateDto,
    NoteSidebarPageList, NoteSidebarPagesRequest, NoteWorkspaceShellDto, NoteWorkspaceShellRequest,
};
mod history_knowledge;
pub use history_knowledge::{
    NoteBacklinkDto, NoteMentionNotificationDto, NotePageAliasCreate, NotePageAliasDto,
    NotePageHistorySettingsDto, NotePageHistorySnapshotDto, NoteSearchResultDto,
    NoteSearchWindowDto, NoteUnresolvedLinkDto, NoteUnresolvedLinkResolve,
};
mod collaboration;
pub use collaboration::{
    NoteCommentAnchorDto, NoteCommentDto, NoteCommentReactionDto, NoteCommentThreadDto,
    NoteCommentThreadViewerState, NoteLocalUserDto, NotePageNotificationSettingsDto,
    NotePartialUserDto, NoteSuggestionCreate, NoteSuggestionDto,
};
mod write_requests;
pub use write_requests::{
    NoteAppendBlockChildren, NoteBlockUpdate, NoteBlockWrite, NoteChildPageFromBlockCreate,
    NoteCommentAnchorCreate, NoteCommentCreate, NoteCommentThreadReadUpdate, NoteCommentUpdate,
    NoteDataSourceAttach, NoteDataSourceBoardConfigurationUpdate, NoteDataSourceBoardRowMove,
    NoteDataSourceBoardViewUpdate, NoteDataSourceButtonClick,
    NoteDataSourceCalendarConfigurationUpdate, NoteDataSourceCalendarViewUpdate,
    NoteDataSourceCreate, NoteDataSourceCsvExportDiagnosticDto, NoteDataSourceCsvExportDto,
    NoteDataSourceCsvExportRequest, NoteDataSourceCsvExportSaveDto,
    NoteDataSourceCsvImportColumnDto, NoteDataSourceCsvImportDiagnosticDto,
    NoteDataSourceCsvImportDto, NoteDataSourceCsvImportRequest, NoteDataSourceCsvImportRowDto,
    NoteDataSourceFilterCondition, NoteDataSourceFilterOperator,
    NoteDataSourceGalleryConfigurationUpdate, NoteDataSourceGalleryViewUpdate,
    NoteDataSourceListConfigurationUpdate, NoteDataSourceListViewUpdate,
    NoteDataSourcePropertyAction, NoteDataSourcePropertyInsertionSide, NoteDataSourceRowPageCreate,
    NoteDataSourceRowPropertyUpdate, NoteDataSourceSchemaUpdate,
    NoteDataSourceTableConfigurationUpdate, NoteDataSourceTableFilter,
    NoteDataSourceTableFilterGroup, NoteDataSourceTableFilterPredicate, NoteDataSourceTableSort,
    NoteDataSourceTableViewUpdate, NoteDataSourceTemplateApply,
    NoteDataSourceTemplateCreateFromRow, NoteDataSourceTemplateDuplicate,
    NoteDataSourceTemplateUpdate, NoteDataSourceTimelineConfigurationUpdate,
    NoteDataSourceTimelineViewUpdate, NoteDataSourceViewWindowRequest, NoteDatabaseCreate,
    NoteDatabaseDuplicate, NoteDatabaseRename, NoteDatabaseViewDuplicate, NoteDatabaseViewRename,
    NoteDuplicateBlock, NoteDuplicateBlocks, NoteDuplicatePage, NoteDuplicatedBlockId,
    NoteFolderCreate, NoteFolderUpdate, NoteLinkedDatabaseCreate, NoteLocalUserUpdate,
    NoteMentionNotificationDeliveryUpdate, NoteMoveBlock, NoteMoveBlocks, NoteMovePage,
    NotePageCreate, NotePageHistoryCopyBlocks, NotePageHistorySettingsUpdate,
    NotePageTemplateApply, NotePageTemplateCreateFromPage, NotePageTemplateDuplicate,
    NotePageTemplateUpdate, NotePageUpdate, NoteTrashBlocks,
};
mod database;
#[allow(unused_imports)]
pub use database::{
    NoteCreatedDatabaseDto, NoteDataSourceBoardGroupDto, NoteDataSourceBoardViewDto,
    NoteDataSourceCalendarViewDto, NoteDataSourceDto, NoteDataSourceGalleryViewDto,
    NoteDataSourceListItemDto, NoteDataSourceListViewDto, NoteDataSourceParentDto,
    NoteDataSourceSchemaDto, NoteDataSourceTableViewDto, NoteDataSourceTimelineViewDto,
    NoteDatabaseDataSourceSummaryDto, NoteDatabaseDto, NoteDatabaseReferenceDto,
    NoteDatabaseViewDto, NoteDatabaseViewParentDto,
};
mod rows;
pub use history_knowledge::NoteBacklinkIndexedInput;
pub use rows::{
    NoteBlockRow, NoteCommentAnchorRow, NoteCommentRow, NoteCommentThreadRow, NoteDataSourceRow,
    NoteDataSourceTemplateBlockRow, NoteDataSourceTemplateRow, NoteDatabaseRow,
    NoteDatabaseViewRow, NoteFolderRow, NoteLocalUserRow, NoteMentionNotificationRow,
    NotePageAliasRow, NotePageHistorySettingsRow, NotePageHistorySnapshotRow, NotePageRow,
    NotePageTemplateBlockRow, NotePageTemplateRow, NoteSuggestionRow, NoteUnresolvedLinkRow,
    block_parent_from_database_row, page_parent_columns, parent_columns, parent_from_row,
    parse_json, parse_optional_json,
};
pub use transfers::{NoteNotionApiImportSummary, NoteNotionExportImportSummary};
pub use workspace::NotePageSummaryDto;
pub use write_requests::{NoteDataSourceRowWindow, OptionalJsonValue};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum NoteParent {
    #[serde(rename = "workspace")]
    Workspace { workspace: bool },
    #[serde(rename = "page_id")]
    PageId { page_id: String },
    #[serde(rename = "block_id")]
    BlockId { block_id: String },
    #[serde(rename = "data_source_id")]
    DataSourceId { data_source_id: String },
}

#[derive(Serialize)]
pub struct NotePageDto {
    object: &'static str,
    id: String,
    created_time: String,
    last_edited_time: String,
    parent: NoteParent,
    folder_id: Option<String>,
    in_trash: bool,
    archived: bool,
    icon: Option<Value>,
    cover: Option<Value>,
    properties: Value,
    url: Option<String>,
    public_url: Option<String>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_workspace_id: Option<String>,
    source_last_edited_time: Option<String>,
}

impl NotePageDto {
    pub fn new(row: NotePageRow) -> Result<Self, String> {
        let in_trash = row.in_trash != 0;
        let archived = row.archived != 0;
        Ok(Self {
            object: "page",
            id: row.id,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
            parent: parent_from_row(
                &row.parent_type,
                row.parent_page_id,
                row.parent_block_id,
                row.parent_data_source_id,
            )?,
            folder_id: row.folder_id,
            in_trash,
            archived,
            icon: parse_optional_json(row.icon, "page icon")?,
            cover: parse_optional_json(row.cover, "page cover")?,
            properties: parse_json(row.properties, "page properties")?,
            url: row.url,
            public_url: row.public_url,
            source_provider: row.source_provider,
            source_object_id: row.source_object_id,
            source_workspace_id: row.source_workspace_id,
            source_last_edited_time: row.source_last_edited_time,
        })
    }
}

#[derive(Serialize)]
pub struct NoteFolderDto {
    object: &'static str,
    id: String,
    project_id: String,
    parent_folder_id: Option<String>,
    name: String,
    created_time: String,
    last_edited_time: String,
}

impl NoteFolderDto {
    pub fn new(row: NoteFolderRow) -> Self {
        Self {
            object: "folder",
            id: row.id,
            project_id: row.project_id,
            parent_folder_id: row.parent_folder_id,
            name: row.name,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoteBlockDto {
    object: String,
    id: String,
    edit_revision: String,
    content_revision: String,
    parent: NoteParent,
    created_time: String,
    last_edited_time: String,
    has_children: bool,
    in_trash: bool,
    archived: bool,
    #[serde(rename = "type")]
    block_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    paragraph: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_1: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_2: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_3: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_4: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_5: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heading_6: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bulleted_list_item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    numbered_list_item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    to_do: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toggle: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    callout: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_page: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    child_database: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    breadcrumb: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_of_contents: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column_list: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    table_row: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tab: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    video: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pdf: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    link_preview: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    synced_block: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    template: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    button: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embed: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    equation: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    divider: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsupported: Option<Value>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_last_edited_time: Option<String>,
}

impl NoteBlockDto {
    pub fn new(row: NoteBlockRow) -> Result<Self, String> {
        let edit_revision = row.edit_revision()?;
        let content_revision = row.content_revision()?;
        let payload = parse_json(row.payload, "block payload")?;
        let in_trash = row.in_trash != 0;
        let mut block = Self {
            object: "block".to_string(),
            id: row.id,
            edit_revision,
            content_revision,
            parent: parent_from_row(
                &row.parent_type,
                row.parent_page_id,
                row.parent_block_id,
                None,
            )?,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
            has_children: row.has_children != 0,
            in_trash,
            archived: in_trash,
            block_type: row.block_type.clone(),
            paragraph: None,
            heading_1: None,
            heading_2: None,
            heading_3: None,
            heading_4: None,
            heading_5: None,
            heading_6: None,
            bulleted_list_item: None,
            numbered_list_item: None,
            to_do: None,
            toggle: None,
            callout: None,
            quote: None,
            child_page: None,
            child_database: None,
            breadcrumb: None,
            table_of_contents: None,
            column_list: None,
            column: None,
            table: None,
            table_row: None,
            tab: None,
            image: None,
            video: None,
            audio: None,
            file: None,
            pdf: None,
            bookmark: None,
            link_preview: None,
            synced_block: None,
            template: None,
            button: None,
            embed: None,
            equation: None,
            divider: None,
            code: None,
            unsupported: None,
            source_provider: row.source_provider,
            source_object_id: row.source_object_id,
            source_last_edited_time: row.source_last_edited_time,
        };
        match row.block_type.as_str() {
            "paragraph" => block.paragraph = Some(payload),
            "heading_1" => block.heading_1 = Some(payload),
            "heading_2" => block.heading_2 = Some(payload),
            "heading_3" => block.heading_3 = Some(payload),
            "heading_4" => block.heading_4 = Some(payload),
            "heading_5" => block.heading_5 = Some(payload),
            "heading_6" => block.heading_6 = Some(payload),
            "bulleted_list_item" => block.bulleted_list_item = Some(payload),
            "numbered_list_item" => block.numbered_list_item = Some(payload),
            "to_do" => block.to_do = Some(payload),
            "toggle" => block.toggle = Some(payload),
            "callout" => block.callout = Some(payload),
            "quote" => block.quote = Some(payload),
            "child_page" => block.child_page = Some(payload),
            "child_database" => block.child_database = Some(payload),
            "breadcrumb" => block.breadcrumb = Some(payload),
            "table_of_contents" => block.table_of_contents = Some(payload),
            "column_list" => block.column_list = Some(payload),
            "column" => block.column = Some(payload),
            "table" => block.table = Some(payload),
            "table_row" => block.table_row = Some(payload),
            "tab" => block.tab = Some(payload),
            "image" => block.image = Some(payload),
            "video" => block.video = Some(payload),
            "audio" => block.audio = Some(payload),
            "file" => block.file = Some(payload),
            "pdf" => block.pdf = Some(payload),
            "bookmark" => block.bookmark = Some(payload),
            "link_preview" => block.link_preview = Some(payload),
            "synced_block" => block.synced_block = Some(payload),
            "template" => block.template = Some(payload),
            "button" => block.button = Some(payload),
            "embed" => block.embed = Some(payload),
            "equation" => block.equation = Some(payload),
            "divider" => block.divider = Some(payload),
            "code" => block.code = Some(payload),
            "unsupported" => block.unsupported = Some(payload),
            other => return Err(format!("unsupported block type in storage: {other}")),
        }
        Ok(block)
    }
}

#[derive(Serialize)]
pub struct NotePaginatedBlockList {
    object: &'static str,
    #[serde(rename = "type")]
    list_type: &'static str,
    block: Value,
    results: Vec<NoteBlockDto>,
    next_cursor: Option<String>,
    has_more: bool,
}

impl NotePaginatedBlockList {
    pub fn new(results: Vec<NoteBlockDto>, next_cursor: Option<String>, has_more: bool) -> Self {
        Self {
            object: "list",
            list_type: "block",
            block: Value::Object(serde_json::Map::new()),
            results,
            next_cursor,
            has_more,
        }
    }
}

#[derive(Serialize)]
pub struct NoteLoadedPage {
    page: NotePageDto,
    blocks: NotePaginatedBlockList,
}

#[derive(Serialize)]
pub struct NotePageOpenDto {
    page: NotePageDto,
    breadcrumb: Vec<NotePageBreadcrumbItemDto>,
    blocks: NotePaginatedBlockList,
    outlines: Vec<NoteBlockOutlineDto>,
}

impl NotePageOpenDto {
    pub fn new(
        page: NotePageDto,
        breadcrumb: Vec<NotePageBreadcrumbItemDto>,
        blocks: NotePaginatedBlockList,
        outlines: Vec<NoteBlockOutlineDto>,
    ) -> Self {
        Self {
            page,
            breadcrumb,
            blocks,
            outlines,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NoteBlockOutlineDto {
    /// Additional indentation relative to the structural parent.
    pub ganbaru_indent: i64,
    id: String,
    page_id: String,
    parent: NoteParent,
    #[serde(rename = "type")]
    block_type: String,
    sort_order: f64,
    has_children: bool,
    retained_height: i64,
}

impl NoteBlockOutlineDto {
    pub fn new(
        id: String,
        page_id: String,
        parent: NoteParent,
        block_type: String,
        sort_order: f64,
        has_children: bool,
        retained_height: i64,
    ) -> Self {
        Self {
            id,
            page_id,
            parent,
            block_type,
            sort_order,
            has_children,
            retained_height,
            ganbaru_indent: 0,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteBlockHydrationRequest {
    pub page_id: String,
    pub block_ids: Vec<String>,
}

#[derive(Serialize)]
pub struct NoteBlockFrontierDto {
    blocks: Vec<NoteBlockDto>,
}

impl NoteBlockFrontierDto {
    pub fn new(blocks: Vec<NoteBlockDto>) -> Self {
        Self { blocks }
    }
}

impl NoteLoadedPage {
    pub fn new(page: NotePageDto, blocks: NotePaginatedBlockList) -> Self {
        Self { page, blocks }
    }
}

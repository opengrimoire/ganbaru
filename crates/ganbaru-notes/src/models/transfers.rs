use super::*;

#[derive(Deserialize)]
pub struct NoteMarkdownImportRequest {
    pub parent: NoteParent,
    pub markdown: String,
    pub title: Option<String>,
    pub source_name: Option<String>,
    pub after_block_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteMarkdownImportDiagnosticDto {
    code: String,
    severity: String,
    line: Option<i64>,
    message: String,
}

impl NoteMarkdownImportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        line: Option<i64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            line,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
pub struct NoteMarkdownImportDto {
    page: NoteLoadedPage,
    diagnostics: Vec<NoteMarkdownImportDiagnosticDto>,
    imported_block_count: i64,
}

impl NoteMarkdownImportDto {
    pub fn new(
        page: NoteLoadedPage,
        diagnostics: Vec<NoteMarkdownImportDiagnosticDto>,
        imported_block_count: i64,
    ) -> Self {
        Self {
            page,
            diagnostics,
            imported_block_count,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteHtmlImportRequest {
    pub parent: NoteParent,
    pub html: String,
    pub title: Option<String>,
    pub source_name: Option<String>,
    pub after_block_id: Option<String>,
    pub keep_external_file_references: Option<bool>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteHtmlImportDiagnosticDto {
    code: String,
    severity: String,
    line: Option<i64>,
    message: String,
}

impl NoteHtmlImportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        line: Option<i64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            line,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
pub struct NoteHtmlImportDto {
    page: NoteLoadedPage,
    diagnostics: Vec<NoteHtmlImportDiagnosticDto>,
    imported_block_count: i64,
}

impl NoteHtmlImportDto {
    pub fn new(
        page: NoteLoadedPage,
        diagnostics: Vec<NoteHtmlImportDiagnosticDto>,
        imported_block_count: i64,
    ) -> Self {
        Self {
            page,
            diagnostics,
            imported_block_count,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteNotionApiImportRequest {
    pub parent: NoteParent,
    pub integration_token: String,
    pub source_workspace_id: Option<String>,
    #[serde(default)]
    pub page_ids: Vec<String>,
    #[serde(default)]
    pub data_source_ids: Vec<String>,
    pub include_comments: Option<bool>,
    pub include_users: Option<bool>,
    pub keep_external_file_references: Option<bool>,
    pub page_size: Option<i64>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteNotionApiImportDiagnosticDto {
    code: String,
    severity: String,
    source_object_id: Option<String>,
    message: String,
}

impl NoteNotionApiImportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        source_object_id: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            source_object_id: source_object_id.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteNotionApiImportedObjectDto {
    object_type: String,
    source_object_id: String,
    local_id: String,
    title: String,
}

impl NoteNotionApiImportedObjectDto {
    pub fn new(
        object_type: impl Into<String>,
        source_object_id: impl Into<String>,
        local_id: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            object_type: object_type.into(),
            source_object_id: source_object_id.into(),
            local_id: local_id.into(),
            title: title.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteNotionApiImportedUserDto {
    source_user_id: String,
    name: String,
    user_type: String,
}

impl NoteNotionApiImportedUserDto {
    pub fn new(
        source_user_id: impl Into<String>,
        name: impl Into<String>,
        user_type: impl Into<String>,
    ) -> Self {
        Self {
            source_user_id: source_user_id.into(),
            name: name.into(),
            user_type: user_type.into(),
        }
    }
}

#[derive(Serialize)]
pub struct NoteNotionApiImportDto {
    object: &'static str,
    imported_pages: Vec<NoteLoadedPage>,
    imported_data_sources: Vec<NoteNotionApiImportedObjectDto>,
    imported_users: Vec<NoteNotionApiImportedUserDto>,
    diagnostics: Vec<NoteNotionApiImportDiagnosticDto>,
    request_count: i64,
    retry_count: i64,
    rate_limit_count: i64,
    imported_page_count: i64,
    imported_block_count: i64,
    imported_data_source_count: i64,
    imported_comment_count: i64,
    imported_user_count: i64,
    imported_file_count: i64,
    unsupported_block_count: i64,
}

pub struct NoteNotionApiImportSummary {
    pub imported_pages: Vec<NoteLoadedPage>,
    pub imported_data_sources: Vec<NoteNotionApiImportedObjectDto>,
    pub imported_users: Vec<NoteNotionApiImportedUserDto>,
    pub diagnostics: Vec<NoteNotionApiImportDiagnosticDto>,
    pub request_count: i64,
    pub retry_count: i64,
    pub rate_limit_count: i64,
    pub imported_block_count: i64,
    pub imported_comment_count: i64,
    pub imported_file_count: i64,
    pub unsupported_block_count: i64,
}

impl NoteNotionApiImportDto {
    pub fn new(summary: NoteNotionApiImportSummary) -> Self {
        let imported_page_count = summary.imported_pages.len() as i64;
        let imported_data_source_count = summary.imported_data_sources.len() as i64;
        let imported_user_count = summary.imported_users.len() as i64;
        Self {
            object: "notes_notion_api_import",
            imported_pages: summary.imported_pages,
            imported_data_sources: summary.imported_data_sources,
            imported_users: summary.imported_users,
            diagnostics: summary.diagnostics,
            request_count: summary.request_count,
            retry_count: summary.retry_count,
            rate_limit_count: summary.rate_limit_count,
            imported_page_count,
            imported_block_count: summary.imported_block_count,
            imported_data_source_count,
            imported_comment_count: summary.imported_comment_count,
            imported_user_count,
            imported_file_count: summary.imported_file_count,
            unsupported_block_count: summary.unsupported_block_count,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteNotionExportImportRequest {
    pub parent: NoteParent,
    pub export_root_path: String,
    pub source_workspace_id: Option<String>,
    pub keep_external_file_references: Option<bool>,
    pub copy_local_file_references: Option<bool>,
    pub import_markdown: Option<bool>,
    pub import_html: Option<bool>,
    pub import_csv: Option<bool>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NoteNotionExportImportDiagnosticDto {
    code: String,
    severity: String,
    source_path: Option<String>,
    message: String,
}

impl NoteNotionExportImportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        source_path: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            source_path: source_path.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
pub struct NoteNotionExportImportDto {
    object: &'static str,
    imported_pages: Vec<NoteLoadedPage>,
    imported_data_sources: Vec<NoteNotionApiImportedObjectDto>,
    diagnostics: Vec<NoteNotionExportImportDiagnosticDto>,
    imported_page_count: i64,
    imported_block_count: i64,
    imported_data_source_count: i64,
    imported_file_count: i64,
    skipped_file_count: i64,
    unsupported_block_count: i64,
}

pub struct NoteNotionExportImportSummary {
    pub imported_pages: Vec<NoteLoadedPage>,
    pub imported_data_sources: Vec<NoteNotionApiImportedObjectDto>,
    pub diagnostics: Vec<NoteNotionExportImportDiagnosticDto>,
    pub imported_block_count: i64,
    pub imported_file_count: i64,
    pub skipped_file_count: i64,
    pub unsupported_block_count: i64,
}

impl NoteNotionExportImportDto {
    pub fn new(summary: NoteNotionExportImportSummary) -> Self {
        let imported_page_count = summary.imported_pages.len() as i64;
        let imported_data_source_count = summary.imported_data_sources.len() as i64;
        Self {
            object: "notes_notion_export_import",
            imported_pages: summary.imported_pages,
            imported_data_sources: summary.imported_data_sources,
            diagnostics: summary.diagnostics,
            imported_page_count,
            imported_block_count: summary.imported_block_count,
            imported_data_source_count,
            imported_file_count: summary.imported_file_count,
            skipped_file_count: summary.skipped_file_count,
            unsupported_block_count: summary.unsupported_block_count,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteMarkdownExportRequest {
    pub page_id: String,
    pub include_page_title: Option<bool>,
    pub include_comments: Option<bool>,
    pub include_resolved_comments: Option<bool>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteMarkdownExportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub block_id: Option<String>,
    pub comment_id: Option<String>,
    pub message: String,
}

impl NoteMarkdownExportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        block_id: Option<impl Into<String>>,
        comment_id: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            block_id: block_id.map(Into::into),
            comment_id: comment_id.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
pub struct NoteMarkdownExportDto {
    pub object: &'static str,
    pub page_id: String,
    pub markdown: String,
    pub diagnostics: Vec<NoteMarkdownExportDiagnosticDto>,
    pub exported_block_count: i64,
    pub exported_comment_count: i64,
}

impl NoteMarkdownExportDto {
    pub fn new(
        page_id: String,
        markdown: String,
        diagnostics: Vec<NoteMarkdownExportDiagnosticDto>,
        exported_block_count: i64,
        exported_comment_count: i64,
    ) -> Self {
        Self {
            object: "notes_markdown_export",
            page_id,
            markdown,
            diagnostics,
            exported_block_count,
            exported_comment_count,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct NoteHtmlExportRequest {
    pub page_id: String,
    pub include_page_tree: Option<bool>,
    pub include_comments: Option<bool>,
    pub include_resolved_comments: Option<bool>,
    pub include_assets: Option<bool>,
    pub include_database_views: Option<bool>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteHtmlExportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub page_id: Option<String>,
    pub block_id: Option<String>,
    pub asset_id: Option<String>,
    pub comment_id: Option<String>,
    pub message: String,
}

impl NoteHtmlExportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        page_id: Option<impl Into<String>>,
        block_id: Option<impl Into<String>>,
        asset_id: Option<impl Into<String>>,
        comment_id: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            page_id: page_id.map(Into::into),
            block_id: block_id.map(Into::into),
            asset_id: asset_id.map(Into::into),
            comment_id: comment_id.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteHtmlExportFileDto {
    pub path: String,
    pub content_type: String,
    pub contents: String,
    pub byte_size: i64,
}

impl NoteHtmlExportFileDto {
    pub fn new(
        path: impl Into<String>,
        content_type: impl Into<String>,
        contents: impl Into<String>,
    ) -> Self {
        let contents = contents.into();
        Self {
            path: path.into(),
            content_type: content_type.into(),
            byte_size: contents.len() as i64,
            contents,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteHtmlExportAssetDto {
    pub id: String,
    pub archive_path: String,
    pub source_path: String,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub storage_state: String,
    pub exported: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct NoteHtmlExportDto {
    pub object: &'static str,
    pub root_page_id: String,
    pub files: Vec<NoteHtmlExportFileDto>,
    pub assets: Vec<NoteHtmlExportAssetDto>,
    pub diagnostics: Vec<NoteHtmlExportDiagnosticDto>,
    pub manifest_json: String,
    pub exported_page_count: i64,
    pub exported_block_count: i64,
    pub exported_asset_count: i64,
    pub exported_comment_count: i64,
    pub exported_database_view_count: i64,
}

#[derive(Serialize)]
pub struct NoteHtmlArchiveSaveDto {
    object: &'static str,
    saved: bool,
    export: Option<NoteHtmlExportDto>,
}

impl NoteHtmlArchiveSaveDto {
    pub fn canceled() -> Self {
        Self {
            object: "notes_html_archive_save",
            saved: false,
            export: None,
        }
    }

    pub fn saved(export: NoteHtmlExportDto) -> Self {
        Self {
            object: "notes_html_archive_save",
            saved: true,
            export: Some(export),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NoteJsonGraphExportRequest {
    pub include_indexes: Option<bool>,
    pub include_history: Option<bool>,
    pub include_templates: Option<bool>,
    pub include_local_state: Option<bool>,
    pub pretty: Option<bool>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteJsonGraphExportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub table_name: Option<String>,
    pub row_id: Option<String>,
    pub message: String,
}

impl NoteJsonGraphExportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        table_name: Option<impl Into<String>>,
        row_id: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            table_name: table_name.map(Into::into),
            row_id: row_id.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NoteJsonGraphExportDto {
    pub object: &'static str,
    pub export_version: i64,
    pub schema_version: String,
    pub generated_at: String,
    pub file_name: String,
    pub content_type: &'static str,
    pub json: String,
    pub byte_size: i64,
    pub counts: Value,
    pub diagnostics: Vec<NoteJsonGraphExportDiagnosticDto>,
    pub exported_page_count: i64,
    pub exported_block_count: i64,
    pub exported_comment_count: i64,
    pub exported_data_source_count: i64,
    pub exported_file_count: i64,
    pub exported_index_record_count: i64,
    pub exported_property_schema_count: i64,
    pub exported_table_count: i64,
    pub exported_record_count: i64,
    pub warning_count: i64,
}

#[derive(Serialize)]
pub struct NoteJsonGraphExportSaveDto {
    object: &'static str,
    saved: bool,
    export: Option<NoteJsonGraphExportDto>,
}

impl NoteJsonGraphExportSaveDto {
    pub fn canceled() -> Self {
        Self {
            object: "notes_json_graph_export_save",
            saved: false,
            export: None,
        }
    }

    pub fn saved(export: NoteJsonGraphExportDto) -> Self {
        Self {
            object: "notes_json_graph_export_save",
            saved: true,
            export: Some(export),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct NoteAgentBridgeExportRequest {
    #[serde(default)]
    pub page_ids: Vec<String>,
    #[serde(default)]
    pub project_ids: Vec<String>,
    pub include_descendants: Option<bool>,
    pub include_backlinks: Option<bool>,
    pub include_database_views: Option<bool>,
    pub include_task_context: Option<bool>,
    pub include_page_comments: Option<bool>,
    pub include_resolved_comments: Option<bool>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteAgentBridgeExportDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub message: String,
}

impl NoteAgentBridgeExportDiagnosticDto {
    pub fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        source_type: Option<impl Into<String>>,
        source_id: Option<impl Into<String>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity: severity.into(),
            source_type: source_type.map(Into::into),
            source_id: source_id.map(Into::into),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct NoteAgentBridgeExportDto {
    pub object: &'static str,
    pub export_version: i64,
    pub schema_version: &'static str,
    pub file_name: String,
    pub content_type: &'static str,
    pub markdown: String,
    pub byte_size: i64,
    pub diagnostics: Vec<NoteAgentBridgeExportDiagnosticDto>,
    pub exported_page_count: i64,
    pub exported_project_count: i64,
    pub exported_task_count: i64,
    pub exported_database_view_count: i64,
    pub exported_backlink_count: i64,
    pub warning_count: i64,
}

#[derive(Serialize)]
pub struct NoteAgentBridgeExportSaveDto {
    object: &'static str,
    saved: bool,
    export: Option<NoteAgentBridgeExportDto>,
}

impl NoteAgentBridgeExportSaveDto {
    pub fn canceled() -> Self {
        Self {
            object: "notes_agent_bridge_export_save",
            saved: false,
            export: None,
        }
    }

    pub fn saved(export: NoteAgentBridgeExportDto) -> Self {
        Self {
            object: "notes_agent_bridge_export_save",
            saved: true,
            export: Some(export),
        }
    }
}

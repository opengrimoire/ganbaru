use super::*;

#[derive(Deserialize)]
pub struct NoteSidebarPagesRequest {
    #[serde(default)]
    pub expanded_page_ids: Vec<String>,
    #[serde(default)]
    pub seed_page_ids: Vec<String>,
    #[serde(default)]
    pub selected_page_id: Option<String>,
}

#[derive(Deserialize)]
pub struct NoteWorkspaceShellRequest {
    pub project_id: Option<String>,
    #[serde(default)]
    pub expanded_page_ids: Vec<String>,
    #[serde(default)]
    pub seed_page_ids: Vec<String>,
    #[serde(default)]
    pub selected_page_id: Option<String>,
    #[serde(default)]
    pub page_cursor: Option<String>,
    #[serde(default)]
    pub folder_cursor: Option<String>,
    #[serde(default)]
    pub destination_candidates: bool,
    #[serde(default)]
    pub page_query: Option<String>,
    #[serde(default)]
    pub include_navigation_index: bool,
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct NotePageSummaryDto {
    pub id: String,
    pub parent_type: String,
    pub parent_page_id: Option<String>,
    pub parent_block_id: Option<String>,
    pub parent_data_source_id: Option<String>,
    pub folder_id: Option<String>,
    pub title: String,
    pub project_id: Option<String>,
    pub icon: Option<String>,
    pub in_trash: bool,
    pub archived: bool,
    pub created_time: String,
    pub last_edited_time: String,
}

#[derive(Default, Deserialize)]
pub struct NotePageSummaryWindowRequest {
    pub cursor: Option<String>,
    pub query: Option<String>,
    pub page_size: Option<i64>,
}

#[derive(Serialize)]
pub struct NotePageSummaryWindowDto {
    pages: Vec<NotePageSummaryDto>,
    total_count: i64,
    next_cursor: Option<String>,
}

impl NotePageSummaryWindowDto {
    pub fn new(
        pages: Vec<NotePageSummaryDto>,
        total_count: i64,
        next_cursor: Option<String>,
    ) -> Self {
        Self {
            pages,
            total_count,
            next_cursor,
        }
    }
}
#[derive(Serialize)]
pub struct NoteWorkspaceShellDto {
    pages: Vec<NotePageSummaryDto>,
    folders: Vec<NoteFolderDto>,
    navigation_pages: Vec<NotePageSummaryDto>,
    navigation_folders: Vec<NoteFolderDto>,
    navigation_page_ids_with_children: Vec<String>,
    navigation_databases: Vec<NoteNavigationDatabaseDto>,
    page_ids_with_children: Vec<String>,
    missing_parent_page_ids: Vec<String>,
    trashed_parent_page_ids: Vec<String>,
    resolved_selected_page_id: Option<String>,
    total_page_count: i64,
    total_folder_count: i64,
    next_page_cursor: Option<String>,
    next_folder_cursor: Option<String>,
}

impl NoteWorkspaceShellDto {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pages: Vec<NotePageSummaryDto>,
        folders: Vec<NoteFolderDto>,
        navigation_pages: Vec<NotePageSummaryDto>,
        navigation_folders: Vec<NoteFolderDto>,
        navigation_page_ids_with_children: Vec<String>,
        navigation_databases: Vec<NoteNavigationDatabaseDto>,
        page_ids_with_children: Vec<String>,
        missing_parent_page_ids: Vec<String>,
        trashed_parent_page_ids: Vec<String>,
        resolved_selected_page_id: Option<String>,
        total_page_count: i64,
        total_folder_count: i64,
        next_page_cursor: Option<String>,
        next_folder_cursor: Option<String>,
    ) -> Self {
        Self {
            pages,
            folders,
            navigation_pages,
            navigation_folders,
            navigation_page_ids_with_children,
            navigation_databases,
            page_ids_with_children,
            missing_parent_page_ids,
            trashed_parent_page_ids,
            resolved_selected_page_id,
            total_page_count,
            total_folder_count,
            next_page_cursor,
            next_folder_cursor,
        }
    }
}

/// Compact database identities for Notes hierarchy navigation, without row content.
#[derive(Serialize, sqlx::FromRow)]
pub struct NoteNavigationDatabaseDto {
    pub id: String,
    pub page_id: String,
    pub title: String,
    pub data_source_id: String,
}

#[derive(Serialize)]
pub struct NoteSidebarPageList {
    pages: Vec<NotePageSummaryDto>,
    page_ids_with_children: Vec<String>,
    missing_parent_page_ids: Vec<String>,
    trashed_parent_page_ids: Vec<String>,
}

impl NoteSidebarPageList {
    pub fn new(
        pages: Vec<NotePageSummaryDto>,
        page_ids_with_children: Vec<String>,
        missing_parent_page_ids: Vec<String>,
        trashed_parent_page_ids: Vec<String>,
    ) -> Self {
        Self {
            pages,
            page_ids_with_children,
            missing_parent_page_ids,
            trashed_parent_page_ids,
        }
    }
}

#[derive(Serialize)]
pub struct NotePageBreadcrumbItemDto {
    id: Option<String>,
    title: String,
    current: bool,
    status: &'static str,
}

impl NotePageBreadcrumbItemDto {
    pub fn active(row: NotePageRow, current: bool) -> Self {
        Self {
            id: Some(row.id),
            title: row.title,
            current,
            status: "active",
        }
    }

    pub fn unavailable(row: NotePageRow, status: &'static str) -> Self {
        Self {
            id: Some(row.id),
            title: row.title,
            current: false,
            status,
        }
    }

    pub fn missing(page_id: String) -> Self {
        Self {
            id: Some(page_id),
            title: String::new(),
            current: false,
            status: "missing",
        }
    }
}

#[derive(Serialize)]
pub struct NotePageTemplateDto {
    object: &'static str,
    id: String,
    name: String,
    source_page_id: Option<String>,
    properties: Value,
    icon: Option<Value>,
    cover: Option<Value>,
    block_count: i64,
    created_time: String,
    last_edited_time: String,
}

impl NotePageTemplateDto {
    pub fn new(row: NotePageTemplateRow) -> Result<Self, String> {
        Ok(Self {
            object: "page_template",
            id: row.id,
            name: row.name,
            source_page_id: row.source_page_id,
            properties: parse_json(row.properties, "page template properties")?,
            icon: parse_optional_json(row.icon, "page template icon")?,
            cover: parse_optional_json(row.cover, "page template cover")?,
            block_count: row.block_count,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceTemplateDto {
    object: &'static str,
    id: String,
    data_source_id: String,
    source_page_id: Option<String>,
    name: String,
    properties: Value,
    is_default: bool,
    block_count: i64,
    created_time: String,
    last_edited_time: String,
}

impl NoteDataSourceTemplateDto {
    pub fn new(row: NoteDataSourceTemplateRow) -> Result<Self, String> {
        Ok(Self {
            object: "data_source_template",
            id: row.id,
            data_source_id: row.data_source_id,
            source_page_id: row.source_page_id,
            name: row.name,
            properties: parse_json(row.properties, "data source template properties")?,
            is_default: row.is_default != 0,
            block_count: row.block_count,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

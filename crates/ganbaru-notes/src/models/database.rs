use super::*;

/// Destination and canonical ownership metadata for a database block.
#[derive(Serialize)]
pub struct NoteDatabaseReferenceDto {
    pub editing_locked: bool,
    pub source_block_id: String,
    pub page_id: String,
    pub canonical_source_block_id: String,
    pub canonical_source_page_id: String,
    pub title: String,
    pub owned_data_source_count: i64,
    pub is_linked: bool,
}

#[derive(Serialize, Deserialize)]
pub struct NoteCreatedDatabaseDto {
    database: NoteDatabaseDto,
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    block: NoteBlockDto,
}

impl NoteCreatedDatabaseDto {
    pub fn new(
        database: NoteDatabaseRow,
        data_source: NoteDataSourceRow,
        view: NoteDatabaseViewRow,
        block: NoteBlockRow,
    ) -> Result<Self, String> {
        let database_parent = block_parent_from_database_row(&database)?;
        Ok(Self {
            database: NoteDatabaseDto::new(database, vec![data_source.summary()])?,
            data_source: NoteDataSourceDto::new(data_source, database_parent)?,
            view: NoteDatabaseViewDto::new(view)?,
            block: NoteBlockDto::new(block)?,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceSchemaDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
}

impl NoteDataSourceSchemaDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceTableViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    rows: Vec<NotePageDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
    group_counts: std::collections::HashMap<String, i64>,
    calculations: Value,
    row_hierarchy: std::collections::HashMap<
        String,
        crate::data_sources::row_hierarchy::NoteDataSourceRowHierarchyMetadata,
    >,
}

impl NoteDataSourceTableViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        window: NoteDataSourceRowWindow,
        calculations: Value,
        row_hierarchy: std::collections::HashMap<
            String,
            crate::data_sources::row_hierarchy::NoteDataSourceRowHierarchyMetadata,
        >,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            rows: window
                .rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
            group_counts: window.group_counts,
            calculations,
            row_hierarchy,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceBoardGroupDto {
    id: String,
    name: String,
    color: String,
    hidden: bool,
    rows: Vec<NotePageDto>,
}

impl NoteDataSourceBoardGroupDto {
    pub fn new(
        id: String,
        name: String,
        color: String,
        hidden: bool,
        rows: Vec<NotePageRow>,
    ) -> Result<Self, String> {
        Ok(Self {
            id,
            name,
            color,
            hidden,
            rows: rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceBoardViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    groups: Vec<NoteDataSourceBoardGroupDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
    group_counts: std::collections::HashMap<String, i64>,
}

impl NoteDataSourceBoardViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        groups: Vec<NoteDataSourceBoardGroupDto>,
        window: NoteDataSourceRowWindow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            groups,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
            group_counts: window.group_counts,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceGalleryViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    rows: Vec<NotePageDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
}

impl NoteDataSourceGalleryViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        window: NoteDataSourceRowWindow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            rows: window
                .rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceListViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    rows: Vec<NotePageDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
    group_counts: std::collections::HashMap<String, i64>,
}

impl NoteDataSourceListViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        window: NoteDataSourceRowWindow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            rows: window
                .rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
            group_counts: window.group_counts,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceCalendarViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    rows: Vec<NotePageDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
}

impl NoteDataSourceCalendarViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        window: NoteDataSourceRowWindow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            rows: window
                .rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
        })
    }
}

#[derive(Serialize)]
pub struct NoteDataSourceTimelineViewDto {
    data_source: NoteDataSourceDto,
    view: NoteDatabaseViewDto,
    rows: Vec<NotePageDto>,
    total_row_count: i64,
    next_cursor: Option<String>,
    has_more: bool,
    group_counts: std::collections::HashMap<String, i64>,
}

impl NoteDataSourceTimelineViewDto {
    pub fn new(
        data_source: NoteDataSourceRow,
        database: NoteDatabaseRow,
        view: NoteDatabaseViewRow,
        window: NoteDataSourceRowWindow,
    ) -> Result<Self, String> {
        Ok(Self {
            data_source: NoteDataSourceDto::new(
                data_source,
                block_parent_from_database_row(&database)?,
            )?,
            view: NoteDatabaseViewDto::new(view)?,
            rows: window
                .rows
                .into_iter()
                .map(NotePageDto::new)
                .collect::<Result<Vec<_>, _>>()?,
            total_row_count: window.total_row_count,
            next_cursor: window.next_cursor,
            has_more: window.has_more,
            group_counts: window.group_counts,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoteDatabaseDataSourceSummaryDto {
    id: String,
    name: String,
}

impl NoteDatabaseDataSourceSummaryDto {
    pub fn new(id: String, name: String) -> Self {
        Self { id, name }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoteDatabaseDto {
    object: String,
    id: String,
    parent: NoteParent,
    title: String,
    title_rich_text: Value,
    description: Value,
    icon: Option<Value>,
    cover: Option<Value>,
    in_trash: bool,
    is_inline: bool,
    data_sources: Vec<NoteDatabaseDataSourceSummaryDto>,
    url: Option<String>,
    public_url: Option<String>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_workspace_id: Option<String>,
    source_last_edited_time: Option<String>,
    created_time: String,
    last_edited_time: String,
}

impl NoteDatabaseDto {
    pub fn new(
        row: NoteDatabaseRow,
        data_sources: Vec<NoteDatabaseDataSourceSummaryDto>,
    ) -> Result<Self, String> {
        let parent = block_parent_from_database_row(&row)?;
        Ok(Self {
            object: "database".to_string(),
            id: row.id,
            parent,
            title: row.title,
            title_rich_text: parse_json(row.title_rich_text, "database title rich text")?,
            description: parse_json(row.description, "database description")?,
            icon: parse_optional_json(row.icon, "database icon")?,
            cover: parse_optional_json(row.cover, "database cover")?,
            in_trash: row.in_trash != 0,
            is_inline: row.is_inline != 0,
            data_sources,
            url: row.url,
            public_url: row.public_url,
            source_provider: row.source_provider,
            source_object_id: row.source_object_id,
            source_workspace_id: row.source_workspace_id,
            source_last_edited_time: row.source_last_edited_time,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoteDataSourceParentDto {
    #[serde(rename = "type")]
    parent_type: String,
    database_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct NoteDataSourceDto {
    object: String,
    id: String,
    parent: NoteDataSourceParentDto,
    database_parent: NoteParent,
    title: String,
    title_rich_text: Value,
    description: Value,
    icon: Option<Value>,
    properties: Value,
    in_trash: bool,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_workspace_id: Option<String>,
    source_last_edited_time: Option<String>,
    created_time: String,
    last_edited_time: String,
}

impl NoteDataSourceDto {
    pub fn new(row: NoteDataSourceRow, database_parent: NoteParent) -> Result<Self, String> {
        Ok(Self {
            object: "data_source".to_string(),
            id: row.id,
            parent: NoteDataSourceParentDto {
                parent_type: "database_id".to_string(),
                database_id: row.database_id,
            },
            database_parent,
            title: row.title,
            title_rich_text: parse_json(row.title_rich_text, "data source title rich text")?,
            description: parse_json(row.description, "data source description")?,
            icon: parse_optional_json(row.icon, "data source icon")?,
            properties: parse_json(row.properties, "data source properties")?,
            in_trash: row.in_trash != 0,
            source_provider: row.source_provider,
            source_object_id: row.source_object_id,
            source_workspace_id: row.source_workspace_id,
            source_last_edited_time: row.source_last_edited_time,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

/// A data source in the vault-wide list, with the titles of the pages that contain its database.
#[derive(Serialize)]
pub struct NoteDataSourceListItemDto {
    #[serde(flatten)]
    data_source: NoteDataSourceDto,
    /// Outermost first: project group and project names, folder names, then the titles of the pages (and row databases) leading to the page that holds the database block.
    location: Vec<String>,
}

impl NoteDataSourceListItemDto {
    pub fn new(data_source: NoteDataSourceDto, location: Vec<String>) -> Self {
        Self {
            data_source,
            location,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct NoteDatabaseViewParentDto {
    #[serde(rename = "type")]
    parent_type: String,
    database_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct NoteDatabaseViewDto {
    object: String,
    id: String,
    parent: NoteDatabaseViewParentDto,
    data_source_id: String,
    name: String,
    #[serde(rename = "type")]
    view_type: String,
    filter: Option<Value>,
    sorts: Value,
    configuration: Option<Value>,
    url: Option<String>,
    source_provider: Option<String>,
    source_object_id: Option<String>,
    source_workspace_id: Option<String>,
    source_last_edited_time: Option<String>,
    created_time: String,
    last_edited_time: String,
}

impl NoteDatabaseViewDto {
    pub fn new(row: NoteDatabaseViewRow) -> Result<Self, String> {
        Ok(Self {
            object: "view".to_string(),
            id: row.id,
            parent: NoteDatabaseViewParentDto {
                parent_type: "database_id".to_string(),
                database_id: row.database_id,
            },
            data_source_id: row.data_source_id,
            name: row.name,
            view_type: row.view_type,
            filter: parse_optional_json(row.filter, "database view filter")?,
            sorts: parse_json(row.sorts, "database view sorts")?,
            configuration: parse_optional_json(row.configuration, "database view configuration")?,
            url: row.url,
            source_provider: row.source_provider,
            source_object_id: row.source_object_id,
            source_workspace_id: row.source_workspace_id,
            source_last_edited_time: row.source_last_edited_time,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

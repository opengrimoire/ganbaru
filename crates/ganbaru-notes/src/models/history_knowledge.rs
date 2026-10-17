use super::*;

#[derive(Serialize)]
pub struct NotePageHistorySnapshotDto {
    object: &'static str,
    id: String,
    page_id: String,
    title: String,
    icon: Option<Value>,
    cover: Option<Value>,
    block_count: i64,
    reason: String,
    created_by: NotePartialUserDto,
    created_time: String,
    page_last_edited_time: String,
}

impl NotePageHistorySnapshotDto {
    pub fn new(row: NotePageHistorySnapshotRow) -> Result<Self, String> {
        Ok(Self {
            object: "page_history_snapshot",
            id: row.id,
            page_id: row.page_id,
            title: row.title,
            icon: parse_optional_json(row.icon, "page history icon")?,
            cover: parse_optional_json(row.cover, "page history cover")?,
            block_count: row.block_count,
            reason: row.reason,
            created_by: NotePartialUserDto::new(row.created_by),
            created_time: row.created_time,
            page_last_edited_time: row.page_last_edited_time,
        })
    }
}

#[derive(Serialize)]
pub struct NotePageHistorySettingsDto {
    object: &'static str,
    retention_days: Option<i64>,
    updated_at: String,
}

impl NotePageHistorySettingsDto {
    pub fn new(row: NotePageHistorySettingsRow) -> Self {
        Self {
            object: "page_history_settings",
            retention_days: row.retention_days,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Serialize)]
pub struct NoteMentionNotificationDto {
    object: &'static str,
    id: String,
    source_type: String,
    source_id: String,
    page_id: String,
    page_title: String,
    block_id: Option<String>,
    comment_id: Option<String>,
    kind: String,
    target_type: String,
    target_id: Option<String>,
    trigger_at: Option<String>,
    remind_minutes_before: i64,
    plain_text: String,
    source_plain_text: String,
    status: String,
    delivered_at: Option<String>,
    created_time: String,
    last_edited_time: String,
    suppressed: bool,
}

impl NoteMentionNotificationDto {
    pub fn new(row: NoteMentionNotificationRow) -> Self {
        Self {
            object: "mention_notification",
            id: row.id,
            source_type: row.source_type,
            source_id: row.source_id,
            page_id: row.page_id,
            page_title: row.page_title,
            block_id: row.block_id,
            comment_id: row.comment_id,
            kind: row.kind,
            target_type: row.target_type,
            target_id: row.target_id,
            trigger_at: row.trigger_at,
            remind_minutes_before: row.remind_minutes_before,
            plain_text: row.plain_text,
            source_plain_text: row.source_plain_text,
            status: row.status,
            delivered_at: row.delivered_at,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
            suppressed: row.suppressed,
        }
    }
}

#[derive(Serialize)]
pub struct NoteBacklinkDto {
    object: &'static str,
    id: String,
    source_page: NotePageDto,
    source_block_id: String,
    source_block_type: String,
    reference_type: String,
    snippet: String,
    created_time: String,
    last_edited_time: String,
}

pub struct NoteBacklinkIndexedInput {
    pub source_page: NotePageDto,
    pub id: String,
    pub source_block_id: String,
    pub source_block_type: String,
    pub reference_type: String,
    pub snippet: String,
    pub created_time: String,
    pub last_edited_time: String,
}

impl NoteBacklinkDto {
    pub fn indexed(input: NoteBacklinkIndexedInput) -> Self {
        Self {
            object: "backlink",
            id: input.id,
            source_page: input.source_page,
            source_block_id: input.source_block_id,
            source_block_type: input.source_block_type,
            reference_type: input.reference_type,
            snippet: input.snippet,
            created_time: input.created_time,
            last_edited_time: input.last_edited_time,
        }
    }
}

#[derive(Deserialize)]
pub struct NotePageAliasCreate {
    pub id: String,
    pub alias: String,
}

#[derive(Serialize)]
pub struct NotePageAliasDto {
    object: &'static str,
    id: String,
    page_id: String,
    alias: String,
    normalized_alias: String,
    created_time: String,
    last_edited_time: String,
}

impl NotePageAliasDto {
    pub fn new(row: NotePageAliasRow) -> Self {
        Self {
            object: "page_alias",
            id: row.id,
            page_id: row.page_id,
            alias: row.alias,
            normalized_alias: row.normalized_alias,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        }
    }
}

#[derive(Serialize)]
pub struct NoteUnresolvedLinkDto {
    object: &'static str,
    id: String,
    source_type: String,
    source_page_id: String,
    source_block_id: Option<String>,
    source_comment_id: Option<String>,
    raw_url: String,
    raw_target: String,
    normalized_target: String,
    link_text: String,
    snippet: String,
    created_time: String,
    last_edited_time: String,
}

impl NoteUnresolvedLinkDto {
    pub fn new(row: NoteUnresolvedLinkRow) -> Self {
        Self {
            object: "unresolved_link",
            id: row.id,
            source_type: row.source_type,
            source_page_id: row.source_page_id,
            source_block_id: row.source_block_id,
            source_comment_id: row.source_comment_id,
            raw_url: row.raw_url,
            raw_target: row.raw_target,
            normalized_target: row.normalized_target,
            link_text: row.link_text,
            snippet: row.snippet,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        }
    }
}

#[derive(Deserialize)]
pub struct NoteUnresolvedLinkResolve {
    pub target_page_id: String,
}

#[derive(Serialize)]
pub struct NoteSearchResultDto {
    object: &'static str,
    id: String,
    #[serde(rename = "type")]
    result_type: String,
    page: NotePageSummaryDto,
    block_id: Option<String>,
    block_type: Option<String>,
    comment_id: Option<String>,
    discussion_id: Option<String>,
    comment_status: Option<String>,
    comment_author: Option<Value>,
    comment_anchor: Option<NoteCommentAnchorDto>,
    snippet: String,
    last_edited_time: String,
}

impl NoteSearchResultDto {
    pub fn page(page: NotePageSummaryDto, snippet: String, last_edited_time: String) -> Self {
        let id = format!("page:{}", page.id);
        Self {
            object: "search_result",
            id,
            result_type: "page".to_string(),
            page,
            block_id: None,
            block_type: None,
            comment_id: None,
            discussion_id: None,
            comment_status: None,
            comment_author: None,
            comment_anchor: None,
            snippet,
            last_edited_time,
        }
    }

    pub fn block(page: NotePageSummaryDto, block: NoteBlockRow, snippet: String) -> Self {
        Self {
            object: "search_result",
            id: format!("block:{}", block.id),
            result_type: "block".to_string(),
            page,
            block_id: Some(block.id),
            block_type: Some(block.block_type),
            comment_id: None,
            discussion_id: None,
            comment_status: None,
            comment_author: None,
            comment_anchor: None,
            snippet,
            last_edited_time: block.last_edited_time,
        }
    }

    pub fn comment(
        page: NotePageSummaryDto,
        comment: NoteCommentRow,
        block_id: Option<String>,
        status: String,
        anchor: Option<NoteCommentAnchorRow>,
        snippet: String,
    ) -> Result<Self, String> {
        let display_name = parse_json(comment.display_name.clone(), "comment display name")?;
        Ok(Self {
            object: "search_result",
            id: format!("comment:{}", comment.id),
            result_type: "comment".to_string(),
            page,
            block_id,
            block_type: None,
            comment_id: Some(comment.id),
            discussion_id: Some(comment.thread_id),
            comment_status: Some(status),
            comment_author: Some(display_name),
            comment_anchor: anchor.map(NoteCommentAnchorDto::new),
            snippet,
            last_edited_time: comment.last_edited_time,
        })
    }
}

#[derive(Serialize)]
pub struct NoteSearchWindowDto {
    results: Vec<NoteSearchResultDto>,
    next_cursor: Option<String>,
}

impl NoteSearchWindowDto {
    pub fn new(results: Vec<NoteSearchResultDto>, next_cursor: Option<String>) -> Self {
        Self {
            results,
            next_cursor,
        }
    }

    #[cfg(test)]
    pub fn into_results(self) -> Vec<NoteSearchResultDto> {
        self.results
    }
}

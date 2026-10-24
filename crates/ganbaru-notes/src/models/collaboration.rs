use super::*;

#[derive(Serialize)]
pub struct NotePartialUserDto {
    object: &'static str,
    id: String,
}

impl NotePartialUserDto {
    pub fn new(id: String) -> Self {
        Self { object: "user", id }
    }
}

#[derive(Serialize)]
pub struct NoteLocalUserDto {
    object: &'static str,
    id: String,
    display_name: String,
    created_time: String,
}

impl NoteLocalUserDto {
    /// Builds the local user with the name read from the profile preference.
    pub fn new(row: NoteLocalUserRow, display_name: String) -> Self {
        Self {
            object: "user",
            id: row.id,
            display_name,
            created_time: row.created_time,
        }
    }
}

#[derive(Serialize)]
pub struct NoteCommentDto {
    object: &'static str,
    id: String,
    parent: NoteParent,
    discussion_id: String,
    created_time: String,
    last_edited_time: String,
    created_by: NotePartialUserDto,
    rich_text: Value,
    attachments: Value,
    display_name: Value,
    deleted_at: Option<String>,
    reactions: Vec<NoteCommentReactionDto>,
}

/// One emoji on a comment with everyone who chose it, in reaction order.
#[derive(Serialize)]
pub struct NoteCommentReactionDto {
    emoji: String,
    count: usize,
    reacted_by_me: bool,
    names: Vec<String>,
}

impl NoteCommentReactionDto {
    pub fn new(emoji: String) -> Self {
        Self {
            emoji,
            count: 0,
            reacted_by_me: false,
            names: Vec::new(),
        }
    }

    pub fn emoji(&self) -> &str {
        &self.emoji
    }

    /// Adds one person's reaction, remembering whether it belongs to the current local user.
    pub fn add(&mut self, name: String, is_current_user: bool) {
        self.count += 1;
        self.reacted_by_me |= is_current_user;
        self.names.push(name);
    }
}

impl NoteCommentDto {
    pub fn new(
        thread: &NoteCommentThreadRow,
        row: NoteCommentRow,
        reactions: Vec<NoteCommentReactionDto>,
        local_author_name: &str,
    ) -> Result<Self, String> {
        let display_name = crate::local_user::resolved_display_name_json(
            row.display_name.as_deref(),
            local_author_name,
        );
        Ok(Self {
            object: "comment",
            id: row.id,
            parent: parent_from_row(
                &thread.parent_type,
                thread.parent_page_id.clone(),
                thread.parent_block_id.clone(),
                None,
            )?,
            discussion_id: row.thread_id,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
            created_by: NotePartialUserDto::new(row.created_by),
            rich_text: parse_json(row.rich_text, "comment rich_text")?,
            attachments: parse_json(row.attachments, "comment attachments")?,
            display_name: parse_json(display_name, "comment display name")?,
            deleted_at: row.deleted_at,
            reactions,
        })
    }
}

#[derive(Serialize)]
pub struct NoteCommentAnchorDto {
    object: &'static str,
    #[serde(rename = "type")]
    anchor_type: &'static str,
    block_id: String,
    start: i64,
    end: i64,
    text: String,
    prefix: String,
    suffix: String,
    created_time: String,
    last_edited_time: String,
}

impl NoteCommentAnchorDto {
    pub fn new(row: NoteCommentAnchorRow) -> Self {
        Self {
            object: "comment_anchor",
            anchor_type: "text_range",
            block_id: row.block_id,
            start: row.start_offset,
            end: row.end_offset,
            text: row.anchor_text,
            prefix: row.prefix_text,
            suffix: row.suffix_text,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        }
    }
}

#[derive(Serialize)]
pub struct NoteCommentThreadDto {
    object: &'static str,
    id: String,
    parent: NoteParent,
    page_id: String,
    block_id: Option<String>,
    status: String,
    resolved_at: Option<String>,
    resolved_by: Option<NotePartialUserDto>,
    anchor: Option<NoteCommentAnchorDto>,
    created_time: String,
    last_edited_time: String,
    unread: bool,
    muted: bool,
    comments: Vec<NoteCommentDto>,
}

/// Per-person state of one discussion for the current local user.
pub struct NoteCommentThreadViewerState {
    pub unread: bool,
    pub muted: bool,
}

impl NoteCommentThreadDto {
    pub fn new(
        row: NoteCommentThreadRow,
        comments: Vec<NoteCommentDto>,
        anchor: Option<NoteCommentAnchorDto>,
        viewer: NoteCommentThreadViewerState,
    ) -> Result<Self, String> {
        let resolved_by = row.resolved_by.clone().map(NotePartialUserDto::new);
        Ok(Self {
            object: "comment_thread",
            id: row.id,
            parent: parent_from_row(
                &row.parent_type,
                row.parent_page_id.clone(),
                row.parent_block_id.clone(),
                None,
            )?,
            page_id: row.page_id,
            block_id: row.parent_block_id,
            status: row.status,
            resolved_at: row.resolved_at,
            resolved_by,
            anchor,
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
            unread: viewer.unread,
            muted: viewer.muted,
            comments,
        })
    }
}

/// Which notifications from one page reach the current local user: `all`, `mentions`, or `none`.
#[derive(Serialize)]
pub struct NotePageNotificationSettingsDto {
    object: &'static str,
    page_id: String,
    level: String,
}

impl NotePageNotificationSettingsDto {
    pub fn new(page_id: String, level: String) -> Self {
        Self {
            object: "page_notification_settings",
            page_id,
            level,
        }
    }
}

#[derive(Serialize)]
pub struct NoteSuggestionDto {
    object: &'static str,
    id: String,
    page_id: String,
    block_id: String,
    created_by: NotePartialUserDto,
    display_name: Value,
    status: String,
    range_start: i64,
    range_end: i64,
    original_text: String,
    proposed_text: String,
    prefix: String,
    suffix: String,
    accepted_at: Option<String>,
    accepted_by: Option<NotePartialUserDto>,
    rejected_at: Option<String>,
    rejected_by: Option<NotePartialUserDto>,
    created_time: String,
    last_edited_time: String,
}

impl NoteSuggestionDto {
    pub fn new(row: NoteSuggestionRow, local_author_name: &str) -> Result<Self, String> {
        let display_name = crate::local_user::resolved_display_name_json(
            row.display_name.as_deref(),
            local_author_name,
        );
        Ok(Self {
            object: "suggestion",
            id: row.id,
            page_id: row.page_id,
            block_id: row.block_id,
            created_by: NotePartialUserDto::new(row.created_by),
            display_name: parse_json(display_name, "suggestion display name")?,
            status: row.status,
            range_start: row.range_start,
            range_end: row.range_end,
            original_text: row.original_text,
            proposed_text: row.proposed_text,
            prefix: row.prefix_text,
            suffix: row.suffix_text,
            accepted_at: row.accepted_at,
            accepted_by: row.accepted_by.map(NotePartialUserDto::new),
            rejected_at: row.rejected_at,
            rejected_by: row.rejected_by.map(NotePartialUserDto::new),
            created_time: row.created_time,
            last_edited_time: row.last_edited_time,
        })
    }
}

#[derive(Deserialize)]
pub struct NoteSuggestionCreate {
    pub id: String,
    pub block_id: String,
    pub range_start: i64,
    pub range_end: i64,
    pub original_text: String,
    pub proposed_text: String,
    pub prefix: String,
    pub suffix: String,
}

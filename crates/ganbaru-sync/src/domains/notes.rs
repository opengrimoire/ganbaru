//! Notes adapter: the value checks that mirror the Notes structure table checks, the block
//! parent kind derived from block placement, and the lifecycle priority that lets a trashed page
//! or block win over a concurrent restore.

use ganbaru_sync_contracts::bounds::MAX_TEXT_FIELD_BYTES;
use ganbaru_sync_contracts::{Field, GroupId, GroupValue, TableId, Value, derived_key};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::codec;
use crate::manifest::OwnedTable;
use crate::manifest::vault::notes::{
    ADAPTER, ALIASES_TABLE, ANCHORS_TABLE, BLOCKS_TABLE, COMMENTS_TABLE,
    DATA_SOURCE_TEMPLATE_BLOCKS, DATA_SOURCE_TEMPLATES_TABLE, DATA_SOURCES_TABLE, DATABASES_TABLE,
    FOLDERS_TABLE, OPTIONS_TABLE, PAGE_TEMPLATE_BLOCKS, PAGE_TEMPLATES_TABLE, PAGES_TABLE,
    PROPERTIES_TABLE, PROPERTY_KEY_PREFIX, REACTIONS_TABLE, SUGGESTIONS_TABLE, THREADS_TABLE,
    VALUES_TABLE, VIEWS_TABLE, alias_group, anchor_group, block_group, comment_group,
    data_source_group, data_source_template_group, database_group, folder_group, option_group,
    page_group, page_template_group, property_group, reaction_group, suggestion_group,
    thread_group, value_group, view_group,
};

/// Characters of a free text column without a narrower Notes limit.
pub const MAX_TEXT_CHARS: usize = MAX_TEXT_FIELD_BYTES;
/// Characters of a folder name, a page alias, or a block type.
pub const MAX_NAME_CHARS: usize = 200;
/// Bytes of a block's rich text JSON.
pub const MAX_BLOCK_TEXT_BYTES: usize = 256 * 1024;
/// Bytes of a block's attributes JSON.
pub const MAX_BLOCK_ATTRS_BYTES: usize = 64 * 1024;
/// Bytes of one template's row and blocks together, as Notes measures them on save.
pub const MAX_TEMPLATE_CONTENT_BYTES: usize = 256 * 1024;
/// Attachments of one comment.
pub const MAX_COMMENT_ATTACHMENTS: usize = 100;
/// Bytes of one comment's attachments JSON.
pub const MAX_COMMENT_ATTACHMENTS_BYTES: usize = 50 * 1024;
/// Bytes of one reaction emoji.
pub const MAX_REACTION_EMOJI_BYTES: usize = 64;
/// Bytes of one page icon image.
pub const MAX_PAGE_ICON_BYTES: u32 = 3 * 1024 * 1024;
/// Bytes of one page cover image.
pub const MAX_PAGE_COVER_BYTES: u32 = 8 * 1024 * 1024;
/// Bytes of one file attached to a block, a property value, or a comment.
pub const MAX_FILE_BYTES: u32 = 50 * 1024 * 1024;

/// View types the `notes_database_views.type` check allows.
pub const VIEW_TYPES: [&str; 10] = [
    "table",
    "board",
    "list",
    "calendar",
    "timeline",
    "gallery",
    "form",
    "chart",
    "map",
    "dashboard",
];

/// Parent kind of a template block at the root of its template.
pub const TEMPLATE_PARENT_ROOT: &str = "template";

/// Block types the `notes_blocks.type` check allows.
pub const BLOCK_TYPES: [&str; 37] = [
    "paragraph",
    "heading_1",
    "heading_2",
    "heading_3",
    "heading_4",
    "heading_5",
    "heading_6",
    "bulleted_list_item",
    "numbered_list_item",
    "to_do",
    "toggle",
    "callout",
    "quote",
    "child_page",
    "child_database",
    "breadcrumb",
    "table_of_contents",
    "column_list",
    "column",
    "table",
    "table_row",
    "tab",
    "image",
    "video",
    "audio",
    "file",
    "pdf",
    "bookmark",
    "link_preview",
    "synced_block",
    "template",
    "button",
    "embed",
    "equation",
    "divider",
    "code",
    "unsupported",
];

/// Parent kind of a block placed directly on a page.
pub const BLOCK_PARENT_PAGE: &str = "page_id";
/// Parent kind of a block nested under another block.
pub const BLOCK_PARENT_BLOCK: &str = "block_id";

fn text(field: &Field) -> Result<&str, AdapterError> {
    match field {
        Field::Text(text) => Ok(text),
        _ => Err(AdapterError("notes text field")),
    }
}

fn is_null(field: &Field) -> bool {
    matches!(field, Field::Null)
}

/// Text whose space-trimmed form is not empty, as the Notes checks require.
fn not_blank(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    if text(field)?.trim_matches(' ').is_empty() {
        Err(AdapterError(what))
    } else {
        Ok(())
    }
}

fn optional_not_blank(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    if is_null(field) {
        Ok(())
    } else {
        not_blank(field, what)
    }
}

/// Valid JSON of any shape, or null.
fn optional_json(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    if is_null(field) {
        return Ok(());
    }
    serde_json::from_str::<serde_json::Value>(text(field)?)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

/// A JSON object of at most `max_bytes`.
fn json_object(field: &Field, max_bytes: usize, what: &'static str) -> Result<(), AdapterError> {
    let json = text(field)?;
    if json.len() > max_bytes {
        return Err(AdapterError(what));
    }
    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

/// The parent kind of a block placement: a page parent, a block parent, or neither when the
/// placement is invalid.
pub fn block_parent_kind(parent_page: &Field, parent_block: &Field) -> Option<&'static str> {
    match (is_null(parent_page), is_null(parent_block)) {
        (false, true) => Some(BLOCK_PARENT_PAGE),
        (true, false) => Some(BLOCK_PARENT_BLOCK),
        _ => None,
    }
}

fn validate_folder(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (folder_group::NAME, [name]) => not_blank(name, "notes folder name"),
        _ => Ok(()),
    }
}

fn validate_page(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (page_group::PLACEMENT, [child_block, data_source, folder]) => {
            if !is_null(child_block) && !is_null(data_source) {
                return Err(AdapterError("notes page with two parents"));
            }
            if !is_null(folder) && !(is_null(child_block) && is_null(data_source)) {
                return Err(AdapterError("notes nested page in a folder"));
            }
            Ok(())
        }
        (page_group::APPEARANCE, [icon, cover]) => {
            optional_json(icon, "notes page icon")?;
            optional_json(cover, "notes page cover")
        }
        (page_group::LIFECYCLE, [_, _, owner]) => optional_not_blank(owner, "notes trash owner"),
        _ => Ok(()),
    }
}

fn validate_alias(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (alias_group::ALIAS, [alias, normalized]) => {
            not_blank(alias, "notes page alias")?;
            not_blank(normalized, "notes normalized page alias")
        }
        _ => Ok(()),
    }
}

fn validate_block(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (block_group::PLACEMENT, [parent_page, parent_block]) => {
            block_parent_kind(parent_page, parent_block)
                .map(|_| ())
                .ok_or(AdapterError("notes block placement"))
        }
        (block_group::KIND, [kind, attrs]) => {
            if !BLOCK_TYPES.contains(&text(kind)?) {
                return Err(AdapterError("notes block type"));
            }
            json_object(attrs, MAX_BLOCK_ATTRS_BYTES, "notes block attributes")
        }
        (block_group::TEXT, [rich_text]) => {
            json_object(rich_text, MAX_BLOCK_TEXT_BYTES, "notes block text")
        }
        (block_group::LIFECYCLE, [_, owner]) => optional_not_blank(owner, "notes trash owner"),
        _ => Ok(()),
    }
}

/// Valid JSON of any shape.
fn json(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    serde_json::from_str::<serde_json::Value>(text(field)?)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

/// A JSON array.
fn json_array(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    serde_json::from_str::<Vec<serde_json::Value>>(text(field)?)
        .map(|_| ())
        .map_err(|_| AdapterError(what))
}

fn validate_database(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (database_group::TITLE, [_, rich_text]) => json(rich_text, "notes database title"),
        (database_group::DESCRIPTION, [description]) => {
            json(description, "notes database description")
        }
        (database_group::APPEARANCE, [icon, cover]) => {
            optional_json(icon, "notes database icon")?;
            optional_json(cover, "notes database cover")
        }
        _ => Ok(()),
    }
}

fn validate_data_source(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (data_source_group::TITLE, [_, rich_text]) => json(rich_text, "notes data source title"),
        (data_source_group::DESCRIPTION, [description]) => {
            json(description, "notes data source description")
        }
        (data_source_group::ICON, [icon]) => optional_json(icon, "notes data source icon"),
        _ => Ok(()),
    }
}

fn validate_property(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (property_group::CONFIG, [_, config]) => {
            json_object(config, MAX_TEXT_CHARS, "notes property config")
        }
        _ => Ok(()),
    }
}

fn validate_value(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (value_group::VALUE, [_, value]) => json(value, "notes property value"),
        _ => Ok(()),
    }
}

fn validate_view(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (view_group::LAYOUT, [kind, filter, sorts, configuration]) => {
            if !VIEW_TYPES.contains(&text(kind)?) {
                return Err(AdapterError("notes view type"));
            }
            optional_json(filter, "notes view filter")?;
            json(sorts, "notes view sorts")?;
            optional_json(configuration, "notes view configuration")
        }
        _ => Ok(()),
    }
}

/// The template blocks encoded in `blob`: types and payloads valid, ids unique, and every block
/// either at the root with no parent or under another block of the same template.
fn validate_template_blocks(owned: &OwnedTable, blob: &Field) -> Result<(), AdapterError> {
    const WHAT: &str = "notes template blocks";
    let Field::Blob(bytes) = blob else {
        return Err(AdapterError(WHAT));
    };
    let rows = codec::decode(owned, bytes).map_err(|_| AdapterError(WHAT))?;
    let mut ids = std::collections::BTreeSet::new();
    for row in &rows {
        let [Field::Text(id), ..] = row.fields.as_slice() else {
            return Err(AdapterError(WHAT));
        };
        if !ids.insert(id.as_str()) {
            return Err(AdapterError("notes template block ids"));
        }
    }
    for row in &rows {
        let [_, parent_type, parent, _, kind, payload, ..] = row.fields.as_slice() else {
            return Err(AdapterError(WHAT));
        };
        match (text(parent_type)?, parent) {
            (TEMPLATE_PARENT_ROOT, Field::Null) => {}
            (BLOCK_PARENT_BLOCK, Field::Text(parent)) if ids.contains(parent.as_str()) => {}
            _ => return Err(AdapterError("notes template block parent")),
        }
        if !BLOCK_TYPES.contains(&text(kind)?) {
            return Err(AdapterError("notes template block type"));
        }
        json(payload, "notes template block payload")?;
    }
    Ok(())
}

fn validate_page_template(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (page_template_group::CONTENT, [properties, icon, cover, blocks]) => {
            json(properties, "notes template properties")?;
            optional_json(icon, "notes template icon")?;
            optional_json(cover, "notes template cover")?;
            validate_template_blocks(&PAGE_TEMPLATE_BLOCKS, blocks)
        }
        _ => Ok(()),
    }
}

fn validate_data_source_template(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (data_source_template_group::CONTENT, [properties, blocks]) => {
            json(properties, "notes template properties")?;
            validate_template_blocks(&DATA_SOURCE_TEMPLATE_BLOCKS, blocks)
        }
        _ => Ok(()),
    }
}

/// Thread status values, as the Notes check allows them.
pub const THREAD_OPEN: &str = "open";
/// A resolved thread.
pub const THREAD_RESOLVED: &str = "resolved";
/// Suggestion status values, as the Notes check allows them.
pub const SUGGESTION_OPEN: &str = "open";
/// An accepted suggestion.
pub const SUGGESTION_ACCEPTED: &str = "accepted";
/// A rejected suggestion.
pub const SUGGESTION_REJECTED: &str = "rejected";

fn validate_thread(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (thread_group::CREATED, [kind, parent_page, parent_block, _]) => {
            if block_parent_kind(parent_page, parent_block) == Some(text(kind)?) {
                Ok(())
            } else {
                Err(AdapterError("notes comment thread parent"))
            }
        }
        (thread_group::STATUS, [status, resolved_at, resolved_by]) => {
            let open = is_null(resolved_at) && is_null(resolved_by);
            let resolved = !is_null(resolved_at) && !is_null(resolved_by);
            match text(status)? {
                THREAD_OPEN if open => Ok(()),
                THREAD_RESOLVED if resolved => Ok(()),
                _ => Err(AdapterError("notes comment thread status")),
            }
        }
        _ => Ok(()),
    }
}

/// An offset pair whose end lies after its start.
fn ordered_range(start: &Field, end: &Field, what: &'static str) -> Result<(), AdapterError> {
    match (start, end) {
        (Field::Integer(start), Field::Integer(end)) if end > start => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn validate_anchor(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (anchor_group::RANGE, [start, end, quote, _, _]) => {
            ordered_range(start, end, "notes comment anchor range")?;
            not_blank(quote, "notes comment anchor text")
        }
        _ => Ok(()),
    }
}

fn validate_comment(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (comment_group::CREATED, [_, author, display_name, _]) => {
            not_blank(author, "notes comment author")?;
            optional_json(display_name, "notes comment author name")
        }
        (comment_group::BODY, [rich_text, _, attachments]) => {
            json_array(rich_text, "notes comment text")?;
            comment_attachments(attachments)
        }
        _ => Ok(()),
    }
}

/// A comment's attachments: a JSON array of at most [`MAX_COMMENT_ATTACHMENTS`] objects within
/// [`MAX_COMMENT_ATTACHMENTS_BYTES`], as Notes checks them on a local write.
fn comment_attachments(field: &Field) -> Result<(), AdapterError> {
    const WHAT: &str = "notes comment attachments";
    let json = text(field)?;
    if json.len() > MAX_COMMENT_ATTACHMENTS_BYTES {
        return Err(AdapterError(WHAT));
    }
    let attachments =
        serde_json::from_str::<Vec<serde_json::Value>>(json).map_err(|_| AdapterError(WHAT))?;
    if attachments.len() > MAX_COMMENT_ATTACHMENTS || !attachments.iter().all(|a| a.is_object()) {
        return Err(AdapterError(WHAT));
    }
    Ok(())
}

fn validate_reaction(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (reaction_group::IDENTITY, [_, author, emoji]) => {
            not_blank(author, "notes reaction author")?;
            not_blank(emoji, "notes reaction emoji")?;
            if text(emoji)?.len() > MAX_REACTION_EMOJI_BYTES {
                return Err(AdapterError("notes reaction emoji"));
            }
            Ok(())
        }
        (reaction_group::CREATED, [_, display_name]) => {
            optional_json(display_name, "notes reaction author name")
        }
        _ => Ok(()),
    }
}

fn validate_suggestion(group: GroupId, fields: &[Field]) -> Result<(), AdapterError> {
    match (group, fields) {
        (suggestion_group::CONTENT, [_, _, display_name, start, end, original, ..]) => {
            optional_json(display_name, "notes suggestion author name")?;
            ordered_range(start, end, "notes suggestion range")?;
            not_blank(original, "notes suggestion original text")
        }
        (
            suggestion_group::STATUS,
            [status, accepted_at, accepted_by, rejected_at, rejected_by],
        ) => {
            let undecided = is_null(accepted_at)
                && is_null(accepted_by)
                && is_null(rejected_at)
                && is_null(rejected_by);
            let accepted = !is_null(accepted_at) && is_null(rejected_at) && is_null(rejected_by);
            let rejected = !is_null(rejected_at) && is_null(accepted_at) && is_null(accepted_by);
            match text(status)? {
                SUGGESTION_OPEN if undecided => Ok(()),
                SUGGESTION_ACCEPTED if accepted => Ok(()),
                SUGGESTION_REJECTED if rejected => Ok(()),
                _ => Err(AdapterError("notes suggestion status")),
            }
        }
        _ => Ok(()),
    }
}

/// The rank of a coupled decision state: a deleted comment over a live one, and an accepted
/// suggestion over a rejected one over an open one. Thread status has no rank, so concurrent
/// resolves and reopens settle by version order.
fn decision_priority(table: TableId, group: GroupId, value: &Value) -> u8 {
    match value.fields() {
        [deleted_at] if table == COMMENTS_TABLE && group == comment_group::DELETED => {
            u8::from(!is_null(deleted_at))
        }
        [Field::Text(status), ..]
            if table == SUGGESTIONS_TABLE && group == suggestion_group::STATUS =>
        {
            match status.as_str() {
                SUGGESTION_ACCEPTED => 2,
                SUGGESTION_REJECTED => 1,
                _ => 0,
            }
        }
        _ => 0,
    }
}

/// The text field `field` of the first value of `group` among `values`.
fn group_text(values: &[(GroupId, Value)], group: GroupId, field: usize) -> Option<&str> {
    values
        .iter()
        .filter(|(id, _)| *id == group)
        .find_map(|(_, value)| match value.fields().get(field) {
            Some(Field::Text(text)) => Some(text.as_str()),
            _ => None,
        })
}

/// The preview of a comment's plain text or a suggestion's proposed text, falling back to the
/// text it replaces when it proposes a removal.
fn preview(table: TableId, values: &[(GroupId, Value)]) -> String {
    let text = if table == COMMENTS_TABLE {
        group_text(values, comment_group::BODY, 1)
    } else if table == SUGGESTIONS_TABLE {
        group_text(values, suggestion_group::CONTENT, 6)
            .filter(|proposed| !proposed.is_empty())
            .or_else(|| group_text(values, suggestion_group::CONTENT, 5))
    } else if table == ANCHORS_TABLE {
        group_text(values, anchor_group::RANGE, 2)
    } else {
        None
    };
    text.unwrap_or_default().to_owned()
}

/// Checks that an option names the property its identity derives, so an option never attaches
/// to another property.
fn validate_option_row(groups: &[GroupValue]) -> Result<(), AdapterError> {
    const WHAT: &str = "notes option property key";
    let fields = |group: GroupId| {
        groups
            .iter()
            .find(|value| value.group == group)
            .map(|value| value.value.fields())
    };
    let (Some([data_source, property, _]), Some([property_key])) = (
        fields(option_group::IDENTITY),
        fields(option_group::PROPERTY),
    ) else {
        return Err(AdapterError(WHAT));
    };
    let expected = derived_key(PROPERTY_KEY_PREFIX, &[text(data_source)?, text(property)?])
        .map_err(|_| AdapterError(WHAT))?;
    if text(property_key)? == expected.as_str() {
        Ok(())
    } else {
        Err(AdapterError(WHAT))
    }
}

/// Whether `group` of `table` is a trash state where trashing wins.
fn is_lifecycle(table: TableId, group: GroupId) -> bool {
    (table == PAGES_TABLE && group == page_group::LIFECYCLE)
        || (table == BLOCKS_TABLE && group == block_group::LIFECYCLE)
        || (table == DATABASES_TABLE && group == database_group::LIFECYCLE)
        || (table == DATA_SOURCES_TABLE && group == data_source_group::LIFECYCLE)
}

/// The group holding the name a table presents, if any.
fn title_group(table: TableId) -> Option<GroupId> {
    match table {
        PAGES_TABLE => Some(page_group::TITLE),
        DATABASES_TABLE => Some(database_group::TITLE),
        DATA_SOURCES_TABLE => Some(data_source_group::TITLE),
        PROPERTIES_TABLE => Some(property_group::NAME),
        VIEWS_TABLE => Some(view_group::NAME),
        PAGE_TEMPLATES_TABLE => Some(page_template_group::NAME),
        DATA_SOURCE_TEMPLATES_TABLE => Some(data_source_template_group::NAME),
        _ => None,
    }
}

/// Validation, derived block parent kinds, and lifecycle priorities for Notes tables.
pub struct NotesAdapter;

impl DomainAdapter for NotesAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    /// A block's `parent_type` follows its placement.
    fn derived(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        if table != BLOCKS_TABLE || group != block_group::PLACEMENT {
            return Ok(Vec::new());
        }
        match value.fields() {
            [parent_page, parent_block] => block_parent_kind(parent_page, parent_block)
                .map(|kind| vec![("parent_type", Field::Text(kind.to_owned()))])
                .ok_or(AdapterError("notes block placement")),
            _ => Err(AdapterError("notes block placement")),
        }
    }

    /// A trashed page, block, database, or data source wins over a concurrent restore, so
    /// trashing a subtree is never undone halfway by an edit that did not see it. A deleted
    /// comment wins over a concurrent restore, and an accepted suggestion over a rejected one.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        if !is_lifecycle(table, group) {
            return decision_priority(table, group, value);
        }
        match value.fields() {
            [Field::Integer(1), ..] => 1,
            _ => 0,
        }
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == FOLDERS_TABLE {
            validate_folder(group, fields)
        } else if table == PAGES_TABLE {
            validate_page(group, fields)
        } else if table == ALIASES_TABLE {
            validate_alias(group, fields)
        } else if table == BLOCKS_TABLE {
            validate_block(group, fields)
        } else if table == DATABASES_TABLE {
            validate_database(group, fields)
        } else if table == DATA_SOURCES_TABLE {
            validate_data_source(group, fields)
        } else if table == PROPERTIES_TABLE {
            validate_property(group, fields)
        } else if table == VALUES_TABLE {
            validate_value(group, fields)
        } else if table == VIEWS_TABLE {
            validate_view(group, fields)
        } else if table == PAGE_TEMPLATES_TABLE {
            validate_page_template(group, fields)
        } else if table == DATA_SOURCE_TEMPLATES_TABLE {
            validate_data_source_template(group, fields)
        } else if table == THREADS_TABLE {
            validate_thread(group, fields)
        } else if table == ANCHORS_TABLE {
            validate_anchor(group, fields)
        } else if table == COMMENTS_TABLE {
            validate_comment(group, fields)
        } else if table == REACTIONS_TABLE {
            validate_reaction(group, fields)
        } else if table == SUGGESTIONS_TABLE {
            validate_suggestion(group, fields)
        } else {
            Ok(())
        }
    }

    fn validate_row(&self, table: TableId, groups: &[GroupValue]) -> Result<(), AdapterError> {
        if table == OPTIONS_TABLE {
            validate_option_row(groups)
        } else {
            Ok(())
        }
    }

    /// A page, database, data source, property, view, or template presents its title or name.
    /// A comment previews its plain text, a suggestion its proposed text, and an anchor its
    /// quoted text; a thread presents nothing of its own.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let title = title_group(table)
            .and_then(|title_group| {
                values
                    .iter()
                    .filter(|(group, _)| *group == title_group)
                    .find_map(|(_, value)| match value.fields().first() {
                        Some(Field::Text(title)) => Some(title.clone()),
                        _ => None,
                    })
            })
            .unwrap_or_default();
        Presentation {
            title,
            preview: preview(table, values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn t(text: &str) -> Field {
        Field::Text(text.to_owned())
    }

    #[test]
    fn block_placement_needs_exactly_one_parent_and_derives_its_kind() {
        let adapter = NotesAdapter;
        let on_page = value(vec![t("page-1"), Field::Null]);
        let nested = value(vec![Field::Null, t("block-1")]);
        for (placement, kind) in [(&on_page, "page_id"), (&nested, "block_id")] {
            adapter
                .validate(BLOCKS_TABLE, block_group::PLACEMENT, placement)
                .unwrap();
            assert_eq!(
                adapter
                    .derived(BLOCKS_TABLE, block_group::PLACEMENT, placement)
                    .unwrap(),
                vec![("parent_type", t(kind))]
            );
        }
        for invalid in [
            value(vec![Field::Null, Field::Null]),
            value(vec![t("page-1"), t("block-1")]),
        ] {
            assert!(
                adapter
                    .validate(BLOCKS_TABLE, block_group::PLACEMENT, &invalid)
                    .is_err()
            );
            assert!(
                adapter
                    .derived(BLOCKS_TABLE, block_group::PLACEMENT, &invalid)
                    .is_err()
            );
        }
    }

    #[test]
    fn block_content_checks_mirror_the_table_checks() {
        let adapter = NotesAdapter;
        let kind = |kind: &str, attrs: &str| value(vec![t(kind), t(attrs)]);
        adapter
            .validate(
                BLOCKS_TABLE,
                block_group::KIND,
                &kind("to_do", "{\"checked\":true}"),
            )
            .unwrap();
        for invalid in [
            kind("sticker", "{}"),
            kind("paragraph", "[]"),
            kind("paragraph", "{"),
            kind(
                "paragraph",
                &format!("{{\"x\":\"{}\"}}", "a".repeat(MAX_BLOCK_ATTRS_BYTES)),
            ),
        ] {
            assert!(
                adapter
                    .validate(BLOCKS_TABLE, block_group::KIND, &invalid)
                    .is_err()
            );
        }
        assert!(
            adapter
                .validate(
                    BLOCKS_TABLE,
                    block_group::TEXT,
                    &value(vec![t("\"plain\"")])
                )
                .is_err()
        );
        assert!(
            adapter
                .validate(
                    BLOCKS_TABLE,
                    block_group::LIFECYCLE,
                    &value(vec![Field::Integer(1), t("  ")])
                )
                .is_err()
        );
    }

    #[test]
    fn page_placement_keeps_nested_pages_out_of_folders_and_data_sources() {
        let adapter = NotesAdapter;
        let placement = |child: Field, source: Field, folder: Field| {
            adapter.validate(
                PAGES_TABLE,
                page_group::PLACEMENT,
                &value(vec![child, source, folder]),
            )
        };
        placement(t("page-1"), Field::Null, Field::Null).unwrap();
        placement(Field::Null, t("source-1"), Field::Null).unwrap();
        placement(Field::Null, Field::Null, t("folder-1")).unwrap();
        placement(Field::Null, Field::Null, Field::Null).unwrap();
        assert!(placement(t("page-1"), t("source-1"), Field::Null).is_err());
        assert!(placement(t("page-1"), Field::Null, t("folder-1")).is_err());
        assert!(placement(Field::Null, t("source-1"), t("folder-1")).is_err());
        assert!(
            adapter
                .validate(
                    PAGES_TABLE,
                    page_group::APPEARANCE,
                    &value(vec![t("{\"type\":\"emoji\"}"), t("not json")])
                )
                .is_err()
        );
    }

    /// A template content value of `properties` and the template blocks `rows`, each
    /// `(id, parent_type, parent, type, payload)`.
    fn template_content(rows: &[(&str, &str, Option<&str>, &str, &str)]) -> Value {
        let rows = rows
            .iter()
            .map(|(id, parent_type, parent, kind, payload)| {
                crate::codec::OwnedRow::flat(vec![
                    t(id),
                    t(parent_type),
                    parent.map_or(Field::Null, t),
                    Field::Integer(0),
                    t(kind),
                    t(payload),
                    t(""),
                    t("2027-01-01T00:00:00.000Z"),
                    t("2027-01-01T00:00:00.000Z"),
                ])
            })
            .collect::<Vec<_>>();
        let blob = codec::encode(&DATA_SOURCE_TEMPLATE_BLOCKS, &rows).unwrap();
        value(vec![t("{}"), Field::Blob(blob)])
    }

    #[test]
    fn template_blocks_need_unique_ids_parents_inside_the_template_and_valid_content() {
        let adapter = NotesAdapter;
        let validate = |rows: &[(&str, &str, Option<&str>, &str, &str)]| {
            adapter.validate(
                DATA_SOURCE_TEMPLATES_TABLE,
                data_source_template_group::CONTENT,
                &template_content(rows),
            )
        };
        validate(&[
            ("root", "template", None, "toggle", "{}"),
            ("child", "block_id", Some("root"), "paragraph", "{}"),
        ])
        .unwrap();
        for invalid in [
            vec![("root", "template", Some("root"), "paragraph", "{}")],
            vec![("child", "block_id", Some("missing"), "paragraph", "{}")],
            vec![
                ("same", "template", None, "paragraph", "{}"),
                ("same", "template", None, "paragraph", "{}"),
            ],
            vec![("root", "template", None, "sticker", "{}")],
            vec![("root", "template", None, "paragraph", "{")],
        ] {
            assert!(validate(&invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn database_values_views_and_configs_mirror_the_table_checks() {
        let adapter = NotesAdapter;
        let layout = |kind: &str, sorts: &str| {
            adapter.validate(
                VIEWS_TABLE,
                view_group::LAYOUT,
                &value(vec![t(kind), Field::Null, t(sorts), Field::Null]),
            )
        };
        layout("board", "[]").unwrap();
        assert!(layout("kanbanish", "[]").is_err());
        assert!(layout("table", "not json").is_err());
        assert!(
            adapter
                .validate(
                    PROPERTIES_TABLE,
                    property_group::CONFIG,
                    &value(vec![t("select"), t("[]")])
                )
                .is_err()
        );
        assert!(
            adapter
                .validate(
                    VALUES_TABLE,
                    value_group::VALUE,
                    &value(vec![t("number"), t("{")])
                )
                .is_err()
        );
        let trashed = |table, group| {
            adapter.priority(table, group, &value(vec![Field::Integer(1)]))
                > adapter.priority(table, group, &value(vec![Field::Integer(0)]))
        };
        assert!(trashed(DATABASES_TABLE, database_group::LIFECYCLE));
        assert!(trashed(DATA_SOURCES_TABLE, data_source_group::LIFECYCLE));
    }

    #[test]
    fn an_option_names_the_property_its_identity_derives() {
        let adapter = NotesAdapter;
        let row = |property_key: &str| {
            adapter.validate_row(
                OPTIONS_TABLE,
                &[
                    GroupValue::new(
                        OPTIONS_TABLE,
                        option_group::IDENTITY,
                        value(vec![t("source-1"), t("status"), t("done")]),
                    ),
                    GroupValue::new(
                        OPTIONS_TABLE,
                        option_group::PROPERTY,
                        value(vec![t(property_key)]),
                    ),
                ],
            )
        };
        let own = derived_key(PROPERTY_KEY_PREFIX, &["source-1", "status"]).unwrap();
        let other = derived_key(PROPERTY_KEY_PREFIX, &["source-2", "status"]).unwrap();
        assert_eq!(row(own.as_str()), Ok(()));
        assert!(row(other.as_str()).is_err());
        assert!(row("not-a-key").is_err());
    }

    #[test]
    fn a_trashed_page_or_block_outranks_a_restored_one() {
        let adapter = NotesAdapter;
        let lifecycle = |in_trash: i64| {
            adapter.priority(
                PAGES_TABLE,
                page_group::LIFECYCLE,
                &value(vec![Field::Integer(in_trash), Field::Null, Field::Null]),
            )
        };
        assert!(lifecycle(1) > lifecycle(0));
        let block = |in_trash: i64| {
            adapter.priority(
                BLOCKS_TABLE,
                block_group::LIFECYCLE,
                &value(vec![Field::Integer(in_trash), Field::Null]),
            )
        };
        assert!(block(1) > block(0));
        assert_eq!(
            adapter.priority(
                BLOCKS_TABLE,
                block_group::TEXT,
                &value(vec![Field::Integer(1)])
            ),
            0
        );
    }

    #[test]
    fn comment_threads_anchors_and_suggestions_mirror_the_table_checks() {
        let adapter = NotesAdapter;
        let check =
            |table, group, fields: Vec<Field>| adapter.validate(table, group, &value(fields));
        let created = |kind: &str, page: Field, block: Field| {
            check(
                THREADS_TABLE,
                thread_group::CREATED,
                vec![t(kind), page, block, t("2027-01-01T00:00:00.000Z")],
            )
        };
        created(BLOCK_PARENT_PAGE, t("page"), Field::Null).unwrap();
        created(BLOCK_PARENT_BLOCK, Field::Null, t("block")).unwrap();
        assert!(created(BLOCK_PARENT_PAGE, Field::Null, t("block")).is_err());
        assert!(created(BLOCK_PARENT_BLOCK, t("page"), t("block")).is_err());

        let status = |status: &str, at: Field, by: Field| {
            check(THREADS_TABLE, thread_group::STATUS, vec![t(status), at, by])
        };
        status(THREAD_OPEN, Field::Null, Field::Null).unwrap();
        status(THREAD_RESOLVED, t("now"), t("person")).unwrap();
        assert!(status(THREAD_RESOLVED, t("now"), Field::Null).is_err());
        assert!(status(THREAD_OPEN, t("now"), t("person")).is_err());

        let range = |start: i64, end: i64, quote: &str| {
            check(
                ANCHORS_TABLE,
                anchor_group::RANGE,
                vec![
                    Field::Integer(start),
                    Field::Integer(end),
                    t(quote),
                    t(""),
                    t(""),
                ],
            )
        };
        range(0, 4, "word").unwrap();
        assert!(range(4, 4, "word").is_err());
        assert!(range(0, 4, "  ").is_err());

        let body = |rich_text: &str, attachments: &str| {
            check(
                COMMENTS_TABLE,
                comment_group::BODY,
                vec![t(rich_text), t(""), t(attachments)],
            )
        };
        body("[]", "[]").unwrap();
        assert!(body("{}", "[]").is_err());
        assert!(body("[]", "{").is_err());
        assert!(body("[]", "[1]").is_err());
        let attachments = |count: usize, name: &str| {
            serde_json::Value::Array(vec![serde_json::json!({ "name": name }); count]).to_string()
        };
        body("[]", &attachments(MAX_COMMENT_ATTACHMENTS, "a")).unwrap();
        assert!(body("[]", &attachments(MAX_COMMENT_ATTACHMENTS + 1, "a")).is_err());
        let wide = "a".repeat(MAX_COMMENT_ATTACHMENTS_BYTES / 4);
        assert!(body("[]", &attachments(4, &wide)).is_err());
        let reaction = |emoji: &str| {
            check(
                REACTIONS_TABLE,
                reaction_group::IDENTITY,
                vec![t("comment"), t("person"), t(emoji)],
            )
        };
        reaction("👍").unwrap();
        assert!(reaction(" ").is_err());
        assert!(reaction(&"👍".repeat(MAX_REACTION_EMOJI_BYTES / 4 + 1)).is_err());

        let decision = |status: &str, accepted: Field, rejected: Field| {
            check(
                SUGGESTIONS_TABLE,
                suggestion_group::STATUS,
                vec![
                    t(status),
                    accepted.clone(),
                    accepted,
                    rejected.clone(),
                    rejected,
                ],
            )
        };
        decision(SUGGESTION_OPEN, Field::Null, Field::Null).unwrap();
        decision(SUGGESTION_ACCEPTED, t("now"), Field::Null).unwrap();
        decision(SUGGESTION_REJECTED, Field::Null, t("now")).unwrap();
        assert!(decision(SUGGESTION_ACCEPTED, t("now"), t("now")).is_err());
        assert!(decision(SUGGESTION_OPEN, Field::Null, t("now")).is_err());
    }

    #[test]
    fn a_deleted_comment_and_an_accepted_suggestion_outrank_their_alternatives() {
        let adapter = NotesAdapter;
        let deleted =
            |at: Field| adapter.priority(COMMENTS_TABLE, comment_group::DELETED, &value(vec![at]));
        assert!(deleted(t("now")) > deleted(Field::Null));
        let decided = |status: &str| {
            adapter.priority(
                SUGGESTIONS_TABLE,
                suggestion_group::STATUS,
                &value(vec![
                    t(status),
                    Field::Null,
                    Field::Null,
                    Field::Null,
                    Field::Null,
                ]),
            )
        };
        assert!(decided(SUGGESTION_ACCEPTED) > decided(SUGGESTION_REJECTED));
        assert!(decided(SUGGESTION_REJECTED) > decided(SUGGESTION_OPEN));
        assert_eq!(
            adapter.priority(
                THREADS_TABLE,
                thread_group::STATUS,
                &value(vec![t(THREAD_RESOLVED), t("now"), t("person")]),
            ),
            0
        );
    }

    #[test]
    fn comments_and_suggestions_preview_their_text() {
        let adapter = NotesAdapter;
        let comment = adapter.presentation(
            COMMENTS_TABLE,
            &[(
                comment_group::BODY,
                value(vec![t("[]"), t("Looks good"), t("[]")]),
            )],
        );
        assert_eq!(comment.preview, "Looks good");
        let suggestion = |proposed: &str| {
            adapter
                .presentation(
                    SUGGESTIONS_TABLE,
                    &[(
                        suggestion_group::CONTENT,
                        value(vec![
                            t("block"),
                            t("person"),
                            Field::Null,
                            Field::Integer(0),
                            Field::Integer(3),
                            t("old"),
                            t(proposed),
                            t(""),
                            t(""),
                            t("2027-01-01T00:00:00.000Z"),
                        ]),
                    )],
                )
                .preview
        };
        assert_eq!(suggestion("new"), "new");
        assert_eq!(suggestion(""), "old");
    }
}

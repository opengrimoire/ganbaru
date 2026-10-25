//! Notes adapter: the value checks that mirror the Notes structure table checks, the block
//! parent kind derived from block placement, and the lifecycle priority that lets a trashed page
//! or block win over a concurrent restore.

use ganbaru_sync_contracts::bounds::MAX_TEXT_FIELD_BYTES;
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::manifest::vault::notes::{
    ADAPTER, ALIASES_TABLE, BLOCKS_TABLE, FOLDERS_TABLE, PAGES_TABLE, alias_group, block_group,
    folder_group, page_group,
};

/// Characters of a free text column without a narrower Notes limit.
pub const MAX_TEXT_CHARS: usize = MAX_TEXT_FIELD_BYTES;
/// Characters of a folder name, a page alias, or a block type.
pub const MAX_NAME_CHARS: usize = 200;
/// Bytes of a block's rich text JSON.
pub const MAX_BLOCK_TEXT_BYTES: usize = 256 * 1024;
/// Bytes of a block's attributes JSON.
pub const MAX_BLOCK_ATTRS_BYTES: usize = 64 * 1024;

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

    /// A trashed page or block wins over a concurrent restore, so trashing a subtree is never
    /// undone halfway by an edit that did not see it.
    fn priority(&self, table: TableId, group: GroupId, value: &Value) -> u8 {
        let lifecycle = (table == PAGES_TABLE && group == page_group::LIFECYCLE)
            || (table == BLOCKS_TABLE && group == block_group::LIFECYCLE);
        if !lifecycle {
            return 0;
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
        } else {
            Ok(())
        }
    }

    /// A page presents its title; Notes is owner-only, so other rows present nothing yet.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let title = values
            .iter()
            .filter(|(group, _)| table == PAGES_TABLE && *group == page_group::TITLE)
            .find_map(|(_, value)| match value.fields() {
                [Field::Text(title)] => Some(title.clone()),
                _ => None,
            })
            .unwrap_or_default();
        Presentation {
            title,
            preview: String::new(),
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
}

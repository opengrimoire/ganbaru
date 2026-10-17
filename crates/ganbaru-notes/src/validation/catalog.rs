use super::*;

pub const NOTE_BLOCK_TYPES: &[&str] = &[
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

pub const NOTE_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES: &[&str] = &["meeting_notes", "transcription"];
pub const NOTE_BLOCK_CATALOG_GATE_REQUIREMENTS: &[&str] = &[
    "rich editor P0 completion",
    "current block quality completion",
    "honest docs",
    "usable editing UI",
    "persistence",
    "focused tests",
    "pnpm -w run validate",
];

pub fn validate_block_type(block_type: &str) -> Result<(), String> {
    if NOTE_BLOCK_TYPES.contains(&block_type) {
        Ok(())
    } else {
        Err(block_catalog_gate_error(block_type))
    }
}

pub fn block_type_supports_children(block_type: &str) -> bool {
    matches!(
        block_type,
        "paragraph"
            | "bulleted_list_item"
            | "numbered_list_item"
            | "to_do"
            | "toggle"
            | "callout"
            | "quote"
            | "child_database"
            | "column_list"
            | "column"
            | "table"
            | "tab"
            | "template"
            | "button"
    )
}

pub fn block_payload_supports_children(block_type: &str, payload: &Value) -> bool {
    if matches!(
        block_type,
        "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6"
    ) {
        return payload
            .get("is_toggleable")
            .and_then(Value::as_bool)
            .unwrap_or(false);
    }
    if block_type == "synced_block" {
        return matches!(payload.get("synced_from"), Some(Value::Null));
    }
    block_type_supports_children(block_type)
}

pub fn block_catalog_gate_error(block_type: &str) -> String {
    if NOTE_BLOCK_CATALOG_BLOCKED_FUTURE_TYPES.contains(&block_type) {
        return format!(
            "{block_type} is blocked by the Notes block catalog gate until {} are complete",
            NOTE_BLOCK_CATALOG_GATE_REQUIREMENTS.join(", ")
        );
    }
    format!("unsupported block type: {block_type}")
}

use super::*;

const TEXT_BLOCK_TYPES: &[&str] = &[
    "paragraph",
    "heading_1",
    "heading_2",
    "heading_3",
    "heading_4",
    "heading_5",
    "heading_6",
    "bulleted_list_item",
    "numbered_list_item",
    "toggle",
    "callout",
    "quote",
];
pub fn validate_block_payload(block_type: &str, payload: &Value) -> Result<(), String> {
    validate_json_object(payload, block_type)?;
    if let Some(indent) = payload.get("ganbaru_indent") {
        if !matches!(indent.as_u64(), Some(value) if value <= 9_007_199_254_740_991) {
            return Err("ganbaru_indent must be a nonnegative safe integer".to_string());
        }
    }
    if TEXT_BLOCK_TYPES.contains(&block_type) {
        validate_rich_text_payload(payload, block_type)?;
        return Ok(());
    }
    match block_type {
        "to_do" => {
            validate_rich_text_payload(payload, block_type)?;
            match payload.get("checked") {
                Some(Value::Bool(_)) => Ok(()),
                _ => Err("to_do.checked must be a boolean".to_string()),
            }
        }
        "code" => {
            validate_rich_text_payload(payload, block_type)?;
            match payload.get("caption") {
                Some(Value::Array(caption)) => {
                    for item in caption {
                        validate_rich_text_item(item)?;
                    }
                }
                _ => return Err("code.caption must be a rich text array".to_string()),
            }
            match payload.get("language") {
                Some(Value::String(value)) if !value.trim().is_empty() => Ok(()),
                _ => Err("code.language is required".to_string()),
            }
        }
        "table_of_contents" => validate_table_of_contents_payload(payload),
        "column_list" => Ok(()),
        "column" => validate_column_payload(payload),
        "table" => validate_table_payload(payload),
        "table_row" => validate_table_row_payload(payload),
        "tab" => {
            let mut content = payload.clone();
            content.as_object_mut().unwrap().remove("ganbaru_indent");
            validate_empty_object_payload(&content, "tab")
        }
        "image" | "video" | "audio" | "file" | "pdf" => validate_media_payload(block_type, payload),
        "child_page" => validate_child_page_payload(payload),
        "child_database" => validate_child_database_payload(payload),
        "bookmark" => validate_bookmark_payload(payload),
        "link_preview" => validate_link_preview_payload(payload),
        "synced_block" => validate_synced_block_payload(payload),
        "template" => validate_template_payload(payload),
        "button" => validate_button_payload(payload),
        "embed" => validate_embed_payload(payload),
        "equation" => validate_equation_payload(payload),
        "breadcrumb" | "divider" => Ok(()),
        "unsupported" => validate_unsupported_payload(payload),
        _ => Err(block_catalog_gate_error(block_type)),
    }
}

pub fn validate_json_object(value: &Value, field: &str) -> Result<(), String> {
    if value.is_object() {
        Ok(())
    } else {
        Err(format!("{field} must be an object"))
    }
}

pub fn validate_empty_object_payload(value: &Value, field: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{field} must be an object"))?;
    if object.is_empty() {
        Ok(())
    } else {
        Err(format!("{field} must be an empty object"))
    }
}

pub fn validate_table_of_contents_payload(payload: &Value) -> Result<(), String> {
    validate_optional_color(payload, "table_of_contents.color")
}

pub fn validate_child_page_payload(payload: &Value) -> Result<(), String> {
    match payload.get("title") {
        Some(Value::String(title)) if !contains_control_characters(title) => Ok(()),
        Some(Value::String(_)) => {
            Err("child_page.title must not contain control characters".to_string())
        }
        _ => Err("child_page.title must be a string".to_string()),
    }
}

pub fn validate_child_database_payload(payload: &Value) -> Result<(), String> {
    if payload
        .get("editing_locked")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err("child_database.editing_locked must be a boolean".to_string());
    }
    match payload.get("title") {
        Some(Value::String(title)) if !contains_control_characters(title) => {}
        Some(Value::String(_)) => {
            return Err("child_database.title must not contain control characters".to_string());
        }
        _ => return Err("child_database.title must be a string".to_string()),
    }
    validate_optional_payload_uuid(payload, "database_id", "child_database.database_id")?;
    validate_optional_payload_uuid(payload, "data_source_id", "child_database.data_source_id")?;
    validate_optional_payload_uuid(payload, "view_id", "child_database.view_id")
}

pub fn validate_optional_payload_uuid(
    payload: &Value,
    key: &str,
    field: &str,
) -> Result<(), String> {
    match payload.get(key) {
        None => Ok(()),
        Some(Value::String(id)) => require_uuid(id, field),
        _ => Err(format!("{field} must be a string")),
    }
}

pub fn validate_column_payload(payload: &Value) -> Result<(), String> {
    match payload.get("width_ratio") {
        None => Ok(()),
        Some(Value::Number(width)) => match width.as_f64() {
            Some(value) if value > 0.0 && value <= 1.0 => Ok(()),
            _ => Err("column.width_ratio must be greater than 0 and no more than 1".to_string()),
        },
        _ => Err("column.width_ratio must be a number".to_string()),
    }
}

pub fn validate_table_payload(payload: &Value) -> Result<(), String> {
    match payload.get("table_width") {
        Some(Value::Number(width)) => match width.as_i64() {
            Some(value) if (1..=100).contains(&value) => {}
            _ => return Err("table.table_width must be between 1 and 100".to_string()),
        },
        _ => return Err("table.table_width must be an integer".to_string()),
    }
    match payload.get("has_column_header") {
        Some(Value::Bool(_)) => {}
        _ => return Err("table.has_column_header must be a boolean".to_string()),
    }
    match payload.get("has_row_header") {
        Some(Value::Bool(_)) => {}
        _ => return Err("table.has_row_header must be a boolean".to_string()),
    }
    Ok(())
}

pub fn validate_table_row_payload(payload: &Value) -> Result<(), String> {
    let cells = match payload.get("cells") {
        Some(Value::Array(cells)) if (1..=100).contains(&cells.len()) => cells,
        Some(Value::Array(_)) => {
            return Err("table_row.cells must include between 1 and 100 cells".to_string());
        }
        _ => return Err("table_row.cells must be an array".to_string()),
    };
    for cell in cells {
        let Value::Array(items) = cell else {
            return Err("table_row.cells entries must be rich text arrays".to_string());
        };
        for item in items {
            validate_rich_text_item(item)?;
        }
    }
    Ok(())
}

pub fn validate_synced_block_payload(payload: &Value) -> Result<(), String> {
    match payload.get("synced_from") {
        Some(Value::Null) => Ok(()),
        Some(Value::Object(synced_from)) => {
            match synced_from.get("type") {
                Some(Value::String(source_type)) if source_type == "block_id" => {}
                Some(Value::String(_)) => {
                    return Err("synced_block.synced_from.type must be block_id".to_string());
                }
                _ => return Err("synced_block.synced_from.type must be a string".to_string()),
            }
            match synced_from.get("block_id") {
                Some(Value::String(block_id)) => {
                    require_uuid(block_id, "synced_block.synced_from.block_id")
                }
                _ => Err("synced_block.synced_from.block_id must be a string".to_string()),
            }
        }
        Some(_) => Err("synced_block.synced_from must be null or an object".to_string()),
        None => Err("synced_block.synced_from is required".to_string()),
    }
}

pub fn validate_equation_payload(payload: &Value) -> Result<(), String> {
    match payload.get("expression") {
        Some(Value::String(expression)) if !contains_control_characters(expression) => Ok(()),
        Some(Value::String(_)) => {
            Err("equation.expression must not contain control characters".to_string())
        }
        _ => Err("equation.expression must be a string".to_string()),
    }
}

pub fn validate_unsupported_payload(payload: &Value) -> Result<(), String> {
    validate_optional_display_string(payload.get("block_type"), "unsupported.block_type")?;
    validate_optional_display_string(payload.get("source_type"), "unsupported.source_type")?;
    match payload.get("raw") {
        None | Some(Value::Object(_)) => {}
        _ => return Err("unsupported.raw must be an object".to_string()),
    }
    match payload.get("warnings") {
        None => {}
        Some(Value::Array(warnings)) => {
            for (index, warning) in warnings.iter().enumerate() {
                validate_optional_display_string(
                    Some(warning),
                    &format!("unsupported.warnings[{index}]"),
                )?;
            }
        }
        _ => return Err("unsupported.warnings must be an array".to_string()),
    }
    Ok(())
}

pub fn validate_optional_display_string(value: Option<&Value>, field: &str) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(Value::String(text)) if text.trim().is_empty() => {
            Err(format!("{field} must not be empty"))
        }
        Some(Value::String(text)) if contains_control_characters(text) => {
            Err(format!("{field} must not contain control characters"))
        }
        Some(Value::String(_)) => Ok(()),
        _ => Err(format!("{field} must be a string")),
    }
}

pub fn contains_control_characters(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
}

pub fn validate_rich_text_payload(payload: &Value, field: &str) -> Result<(), String> {
    validate_optional_color(payload, &format!("{field}.color"))?;
    validate_optional_toggle_open(payload, field)?;
    validate_optional_heading_toggle_fields(payload, field)?;
    validate_callout_icon(payload, field)?;
    validate_optional_paragraph_icon(payload, field)?;
    match payload.get("rich_text") {
        Some(Value::Array(items)) => {
            for item in items {
                validate_rich_text_item(item)?;
            }
            Ok(())
        }
        _ => Err(format!("{field}.rich_text must be an array")),
    }
}

pub fn validate_optional_paragraph_icon(payload: &Value, field: &str) -> Result<(), String> {
    if field != "paragraph" {
        return Ok(());
    }
    match payload.get("icon") {
        None | Some(Value::Null) => Ok(()),
        Some(icon) => validate_icon_value(icon, "paragraph.icon"),
    }
}

pub fn validate_template_payload(payload: &Value) -> Result<(), String> {
    if payload.get("color").is_some() {
        return Err("template.color is not supported".to_string());
    }
    if payload.get("children").is_some() {
        return Err("template.children must be stored as child blocks".to_string());
    }
    match payload.get("rich_text") {
        Some(Value::Array(items)) => {
            for item in items {
                validate_rich_text_item(item)?;
            }
            Ok(())
        }
        _ => Err("template.rich_text must be an array".to_string()),
    }
}

pub fn validate_button_payload(payload: &Value) -> Result<(), String> {
    if payload.get("children").is_some() {
        return Err("button.children must be stored as child blocks".to_string());
    }
    match payload.get("rich_text") {
        Some(Value::Array(items)) => {
            for item in items {
                validate_rich_text_item(item)?;
            }
        }
        _ => return Err("button.rich_text must be an array".to_string()),
    }
    match payload.get("icon") {
        Some(Value::Null) => {}
        Some(icon) => validate_icon_value(icon, "button.icon")?,
        None => return Err("button.icon is required".to_string()),
    }
    match payload.get("actions") {
        Some(Value::Array(actions)) if (1..=10).contains(&actions.len()) => {
            for (index, action) in actions.iter().enumerate() {
                validate_button_action(action, index)?;
            }
            Ok(())
        }
        Some(Value::Array(_)) => {
            Err("button.actions must include between 1 and 10 actions".to_string())
        }
        _ => Err("button.actions must be an array".to_string()),
    }
}

pub fn validate_button_action(action: &Value, index: usize) -> Result<(), String> {
    validate_json_object(action, &format!("button.actions[{index}]"))?;
    match action.get("type") {
        Some(Value::String(value)) if value == "insert_blocks" => {}
        _ => {
            return Err(format!(
                "button.actions[{index}].type must be insert_blocks"
            ));
        }
    }
    match action.get("source") {
        Some(Value::String(value)) if value == "children" => {}
        _ => return Err(format!("button.actions[{index}].source must be children")),
    }
    match action.get("position") {
        Some(Value::String(value))
            if matches!(
                value.as_str(),
                "below_button" | "above_button" | "top_of_page" | "bottom_of_page"
            ) =>
        {
            Ok(())
        }
        _ => Err(format!(
            "button.actions[{index}].position must be a supported button insert position"
        )),
    }
}

pub fn validate_callout_icon(payload: &Value, field: &str) -> Result<(), String> {
    if field != "callout" {
        return Ok(());
    }
    match payload.get("icon") {
        Some(Value::Null) => Ok(()),
        Some(Value::Object(icon)) => validate_icon_object(icon, "callout.icon"),
        _ => Err("callout.icon must be an icon object or null".to_string()),
    }
}

pub fn validate_optional_toggle_open(payload: &Value, field: &str) -> Result<(), String> {
    if field != "toggle" {
        return Ok(());
    }
    match payload.get("ganbaru_open") {
        None | Some(Value::Bool(_)) => Ok(()),
        _ => Err("toggle.ganbaru_open must be a boolean".to_string()),
    }
}

pub fn validate_optional_heading_toggle_fields(payload: &Value, field: &str) -> Result<(), String> {
    if !matches!(
        field,
        "heading_1" | "heading_2" | "heading_3" | "heading_4" | "heading_5" | "heading_6"
    ) {
        return Ok(());
    }
    match payload.get("is_toggleable") {
        None | Some(Value::Bool(_)) => {}
        _ => return Err(format!("{field}.is_toggleable must be a boolean")),
    }
    match payload.get("ganbaru_open") {
        None | Some(Value::Bool(_)) => Ok(()),
        _ => Err(format!("{field}.ganbaru_open must be a boolean")),
    }
}

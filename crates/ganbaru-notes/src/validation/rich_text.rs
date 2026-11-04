use super::*;

const NOTE_COLORS: &[&str] = &[
    "default",
    "gray",
    "brown",
    "orange",
    "yellow",
    "green",
    "blue",
    "purple",
    "pink",
    "red",
    "gray_background",
    "brown_background",
    "orange_background",
    "yellow_background",
    "green_background",
    "blue_background",
    "purple_background",
    "pink_background",
    "red_background",
];
pub fn validate_comment_rich_text(rich_text: &[Value]) -> Result<(), String> {
    validate_comment_body_rich_text(rich_text, false)
}

/// Validate comment text, allowing it to be empty only when the comment carries attachments.
pub fn validate_comment_body_rich_text(
    rich_text: &[Value],
    has_attachments: bool,
) -> Result<(), String> {
    let min_items = usize::from(!has_attachments);
    if !(min_items..=100).contains(&rich_text.len()) {
        return Err("comment rich_text must include between 1 and 100 items".to_string());
    }
    for item in rich_text {
        validate_rich_text_item(item)?;
    }
    if !has_attachments && rich_text_items_plain_text(rich_text).trim().is_empty() {
        return Err("comment rich_text must not be empty".to_string());
    }
    validate_comment_text_size(rich_text)
}

/// Prefix of the refusal for a comment whose stored text exceeds the sync text bound; the
/// frontend recognizes it to show a localized message.
const COMMENT_TOO_LARGE_CODE: &str = "notes-comment-too-large";

/// Refuses a comment whose stored rich text JSON could not replicate. The stored plain text is
/// part of that JSON, so it is never larger.
fn validate_comment_text_size(rich_text: &[Value]) -> Result<(), String> {
    let bytes = Value::Array(rich_text.to_vec()).to_string().len();
    if bytes > ganbaru_sync_contracts::bounds::MAX_TEXT_FIELD_BYTES {
        return Err(format!(
            "{COMMENT_TOO_LARGE_CODE}: comment text is {bytes} bytes, above the {} byte limit",
            ganbaru_sync_contracts::bounds::MAX_TEXT_FIELD_BYTES
        ));
    }
    Ok(())
}
pub fn validate_rich_text_item(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "rich text item must be an object".to_string())?;
    match object.get("type") {
        Some(Value::String(kind)) if kind == "text" => validate_text_rich_text_item(object)?,
        Some(Value::String(kind)) if kind == "mention" => validate_mention_rich_text_item(object)?,
        Some(Value::String(kind)) if kind == "equation" => {
            validate_equation_rich_text_item(object)?;
        }
        Some(Value::String(_)) => {
            return Err("rich text item type must be text, mention, or equation".to_string());
        }
        _ => return Err("rich text item type must be a string".to_string()),
    }
    validate_rich_text_common_fields(object)?;
    Ok(())
}

pub fn validate_text_rich_text_item(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    let text = object
        .get("text")
        .and_then(Value::as_object)
        .ok_or_else(|| "rich text text payload is required".to_string())?;
    match text.get("content") {
        Some(Value::String(_)) => {}
        _ => return Err("rich text text.content must be a string".to_string()),
    }
    match text.get("link") {
        None | Some(Value::Null) => {}
        Some(Value::Object(link)) => {
            let url = link
                .get("url")
                .and_then(Value::as_str)
                .ok_or_else(|| "rich text text.link.url must be a string".to_string())?;
            validate_rich_text_url(url, "rich text text.link.url")?;
        }
        _ => return Err("rich text text.link must be null or an object".to_string()),
    }
    Ok(())
}

pub fn validate_equation_rich_text_item(
    object: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let expression = object
        .get("equation")
        .and_then(|equation| equation.get("expression"))
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text equation.expression must be a string".to_string())?;
    validate_inline_equation_expression(expression, "rich text equation.expression")
}

pub fn validate_mention_rich_text_item(
    object: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let mention = object
        .get("mention")
        .and_then(Value::as_object)
        .ok_or_else(|| "rich text mention payload is required".to_string())?;
    match mention.get("type") {
        Some(Value::String(kind)) if kind == "page" => validate_page_mention(mention),
        Some(Value::String(kind)) if kind == "date" => validate_date_mention(mention),
        Some(Value::String(kind)) if kind == "user" => validate_user_mention(mention),
        Some(Value::String(kind)) if kind == "database" => validate_database_mention(mention),
        Some(Value::String(kind)) if kind == "ganbaru_object" => {
            validate_ganbaru_object_mention(mention)
        }
        Some(Value::String(_)) => Err("rich text mention.type is unsupported".to_string()),
        _ => Err("rich text mention.type must be a string".to_string()),
    }
}

pub fn validate_page_mention(mention: &serde_json::Map<String, Value>) -> Result<(), String> {
    let page_id = mention
        .get("page")
        .and_then(|page| page.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.page.id must be a string".to_string())?;
    require_uuid(page_id, "rich text mention.page.id")
}

pub fn validate_user_mention(mention: &serde_json::Map<String, Value>) -> Result<(), String> {
    let user_id = mention
        .get("user")
        .and_then(|user| user.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.user.id must be a string".to_string())?;
    require_uuid(user_id, "rich text mention.user.id")
}

pub fn validate_database_mention(mention: &serde_json::Map<String, Value>) -> Result<(), String> {
    let database_id = mention
        .get("database")
        .and_then(|database| database.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.database.id must be a string".to_string())?;
    require_uuid(database_id, "rich text mention.database.id")
}

pub fn validate_ganbaru_object_mention(
    mention: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let object = mention
        .get("ganbaru_object")
        .and_then(Value::as_object)
        .ok_or_else(|| "rich text mention.ganbaru_object must be an object".to_string())?;
    let object_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.ganbaru_object.type must be a string".to_string())?;
    if !matches!(object_type, "project" | "project_task" | "calendar_event") {
        return Err("rich text mention.ganbaru_object.type is unsupported".to_string());
    }
    let object_id = object
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.ganbaru_object.id must be a string".to_string())?;
    validate_ganbaru_object_id(object_id, "rich text mention.ganbaru_object.id")
}

/// Validates an app object identifier as an opaque bounded string. The owning domain defines the format; seeded
/// projects, for example, use readable ids rather than UUIDs.
pub fn validate_ganbaru_object_id(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must not be empty or contain control characters"
        ));
    }
    if value.len() > 2048 {
        return Err(format!("{field} is too long"));
    }
    Ok(())
}

pub fn validate_date_mention(mention: &serde_json::Map<String, Value>) -> Result<(), String> {
    let date = mention
        .get("date")
        .and_then(Value::as_object)
        .ok_or_else(|| "rich text mention.date must be an object".to_string())?;
    let start = date
        .get("start")
        .and_then(Value::as_str)
        .ok_or_else(|| "rich text mention.date.start must be a string".to_string())?;
    validate_date_mention_boundary(start, "rich text mention.date.start")?;
    if let Some(end) = date.get("end") {
        if let Some(end_value) = end.as_str() {
            validate_date_mention_boundary(end_value, "rich text mention.date.end")?;
        } else if !end.is_null() {
            return Err("rich text mention.date.end must be null or a string".to_string());
        }
    }
    if let Some(time_zone) = date.get("time_zone") {
        if let Some(time_zone_value) = time_zone.as_str() {
            if time_zone_value.trim().is_empty() || contains_control_characters(time_zone_value) {
                return Err(
                    "rich text mention.date.time_zone must not be empty or contain control characters"
                        .to_string(),
                );
            }
            if time_zone_value.len() > 100 {
                return Err("rich text mention.date.time_zone is too long".to_string());
            }
        } else if !time_zone.is_null() {
            return Err("rich text mention.date.time_zone must be null or a string".to_string());
        }
    }
    if let Some(reminder) = date.get("ganbaru_reminder") {
        validate_date_mention_reminder(reminder)?;
    }
    if let Some(format) = date.get("ganbaru_format") {
        validate_date_mention_format(format)?;
    }
    Ok(())
}

/// Spellings the date editor can store for a date mention.
pub const DATE_MENTION_FORMATS: [&str; 4] = ["full", "short", "numeric", "iso"];

/// Accept a missing spelling or one of [`DATE_MENTION_FORMATS`].
pub fn validate_date_mention_format(value: &Value) -> Result<(), String> {
    match value {
        Value::Null => Ok(()),
        Value::String(format) if DATE_MENTION_FORMATS.contains(&format.as_str()) => Ok(()),
        _ => Err(format!(
            "rich text mention.date.ganbaru_format must be null or one of {}",
            DATE_MENTION_FORMATS.join(", ")
        )),
    }
}

/// The longest a date reminder can fire before its date: four weeks. The SQLite column check matches it.
pub const MAX_DATE_REMINDER_MINUTES_BEFORE: i64 = 40_320;

pub fn validate_date_mention_reminder(value: &Value) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let reminder = value
        .as_object()
        .ok_or_else(|| "rich text mention.date.ganbaru_reminder must be an object".to_string())?;
    if !matches!(reminder.get("enabled"), Some(Value::Bool(_))) {
        return Err(
            "rich text mention.date.ganbaru_reminder.enabled must be a boolean".to_string(),
        );
    }
    match reminder.get("minutes_before") {
        None | Some(Value::Null) => Ok(()),
        Some(minutes)
            if minutes.as_i64().is_some_and(|minutes| {
                (0..=MAX_DATE_REMINDER_MINUTES_BEFORE).contains(&minutes)
            }) =>
        {
            Ok(())
        }
        Some(_) => Err(format!(
            "rich text mention.date.ganbaru_reminder.minutes_before must be null or a whole number from 0 to {MAX_DATE_REMINDER_MINUTES_BEFORE}"
        )),
    }
}

/// The minutes a valid date reminder fires before its date; a missing offset fires at the date.
pub fn date_mention_reminder_minutes_before(reminder: &serde_json::Map<String, Value>) -> i64 {
    reminder
        .get("minutes_before")
        .and_then(Value::as_i64)
        .filter(|minutes| (0..=MAX_DATE_REMINDER_MINUTES_BEFORE).contains(minutes))
        .unwrap_or(0)
}

pub fn validate_date_mention_boundary(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || contains_control_characters(value) {
        return Err(format!(
            "{field} must not be empty or contain control characters"
        ));
    }
    if value.len() > 80 {
        return Err(format!("{field} is too long"));
    }
    if !date_mention_boundary_looks_iso(value) {
        return Err(format!("{field} must be an ISO date or date-time"));
    }
    Ok(())
}

pub fn validate_inline_equation_expression(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || contains_control_characters(value) {
        return Err(format!(
            "{field} must not be empty or contain control characters"
        ));
    }
    if value.len() > 2048 {
        return Err(format!("{field} is too long"));
    }
    Ok(())
}

pub fn date_mention_boundary_looks_iso(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    for (index, byte) in bytes.iter().take(10).enumerate() {
        match index {
            4 | 7 if *byte == b'-' => {}
            4 | 7 => return false,
            _ if byte.is_ascii_digit() => {}
            _ => return false,
        }
    }
    let month = two_digit_number(bytes[5], bytes[6]);
    let day = two_digit_number(bytes[8], bytes[9]);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return false;
    }
    bytes.len() == 10 || bytes.get(10).is_some_and(|byte| *byte == b'T')
}

pub fn two_digit_number(tens: u8, ones: u8) -> u8 {
    (tens - b'0') * 10 + (ones - b'0')
}

pub fn validate_rich_text_common_fields(
    object: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    let annotations = object
        .get("annotations")
        .and_then(Value::as_object)
        .ok_or_else(|| "rich text annotations are required".to_string())?;
    for field in ["bold", "italic", "strikethrough", "underline", "code"] {
        if !matches!(annotations.get(field), Some(Value::Bool(_))) {
            return Err(format!("rich text annotations.{field} must be a boolean"));
        }
    }
    validate_required_color(annotations.get("color"), "rich text annotations.color")?;
    match object.get("plain_text") {
        Some(Value::String(_)) => {}
        _ => return Err("rich text plain_text must be a string".to_string()),
    }
    if !matches!(
        object.get("href"),
        None | Some(Value::Null) | Some(Value::String(_))
    ) {
        return Err("rich text href must be null or a string".to_string());
    }
    if let Some(Value::String(href)) = object.get("href") {
        validate_rich_text_url(href, "rich text href")?;
    }
    Ok(())
}

/// Validate web, email, and vault-local Notes references without admitting arbitrary URI schemes.
pub fn validate_rich_text_url(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must not be empty or contain control characters"
        ));
    }
    if value.len() > 2048 {
        return Err(format!("{field} is too long"));
    }
    if let Some(query) = value.strip_prefix("#notes?") {
        let invalid =
            || format!("{field} must contain a valid local Notes page and optional block UUID");
        let parsed = reqwest::Url::parse(&format!("notes:?{query}")).map_err(|_| invalid())?;
        if parsed.fragment().is_some() {
            return Err(invalid());
        }
        let params = parsed.query_pairs().collect::<Vec<_>>();
        let page_id = params
            .iter()
            .find(|(key, _)| key == "page")
            .map(|(_, value)| value);
        let block_id = params
            .iter()
            .find(|(key, _)| key == "block")
            .map(|(_, value)| value);
        require_uuid(page_id.ok_or_else(invalid)?, "page").map_err(|_| invalid())?;
        if let Some(block_id) = block_id {
            require_uuid(block_id, "block").map_err(|_| invalid())?;
        }
        return Ok(());
    }
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| format!("{field} must be a valid HTTP, HTTPS, or email URL"))?;
    match parsed.scheme() {
        "http" | "https" if parsed.host_str().is_some() => Ok(()),
        "mailto" if is_email_address_like(parsed.path()) => Ok(()),
        _ => Err(format!("{field} must be a valid HTTP, HTTPS, or email URL")),
    }
}

pub fn is_email_address_like(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = trimmed.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && domain.split('.').all(|segment| !segment.is_empty())
        && !domain.contains('@')
}

pub fn validate_optional_color(payload: &Value, field: &str) -> Result<(), String> {
    match payload.get("color") {
        None => Ok(()),
        Some(value) => validate_required_color(Some(value), field),
    }
}

pub fn validate_required_color(value: Option<&Value>, field: &str) -> Result<(), String> {
    match value {
        Some(Value::String(color)) if NOTE_COLORS.contains(&color.as_str()) => Ok(()),
        Some(Value::String(_)) => Err(format!("{field} must be a supported Notion color")),
        _ => Err(format!("{field} must be a string")),
    }
}

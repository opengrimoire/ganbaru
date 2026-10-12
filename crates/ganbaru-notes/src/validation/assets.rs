use super::*;

const NOTE_ICON_COLORS: &[&str] = &[
    "gray",
    "lightgray",
    "brown",
    "yellow",
    "orange",
    "green",
    "blue",
    "purple",
    "pink",
    "red",
];
const NOTE_MANAGED_IMAGE_EXTENSIONS: &[&str] = &[".gif", ".jpeg", ".jpg", ".png", ".webp"];
const NOTE_LOCAL_IMAGE_CONTENT_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

/// Validate editable designs, managed image references, and normalized image focal points.
pub fn validate_page_cover_value(value: &Value) -> Result<(), String> {
    validate_json_object(value, "cover")?;
    let Some(Value::String(source_type)) = value.get("type") else {
        return Err("cover.type must be a string".to_string());
    };
    if let Some(focal) = value.get("focal_point") {
        if source_type == "design" {
            return Err("designed covers cannot have an image focal point".to_string());
        }
        for axis in ["x", "y"] {
            if !focal
                .get(axis)
                .and_then(Value::as_f64)
                .is_some_and(|coordinate| {
                    coordinate.is_finite() && (0.0..=1.0).contains(&coordinate)
                })
            {
                return Err("cover.focal_point coordinates must be between 0 and 1".to_string());
            }
        }
    }
    match source_type.as_str() {
        "design" => {
            const PATTERNS: &[&str] = &[
                "contours",
                "studio",
                "botanical",
                "orbit",
                "atlas",
                "study",
                "mathematics",
                "programming",
                "finance",
                "solid",
                "gradient",
                "mosaic",
                "dots",
                "grid",
            ];
            const THEME_PALETTE_SIZE: u64 = 32;
            let design = value
                .get("design")
                .and_then(Value::as_object)
                .ok_or_else(|| "cover.design must be an object".to_string())?;
            if !design
                .get("pattern")
                .and_then(Value::as_str)
                .is_some_and(|pattern| PATTERNS.contains(&pattern))
            {
                return Err("cover.design.pattern is unsupported".to_string());
            }
            if design.get("color").and_then(Value::as_str) != Some("default")
                && !design
                    .get("color")
                    .and_then(Value::as_u64)
                    .is_some_and(|color| color < THEME_PALETTE_SIZE)
            {
                return Err("cover.design.color must reference a theme palette slot".to_string());
            }
            Ok(())
        }

        "external" => {
            let external = value
                .get("external")
                .and_then(Value::as_object)
                .ok_or_else(|| "cover.external must be an object".to_string())?;
            let Some(Value::String(url)) = external.get("url") else {
                return Err("cover.external.url must be a string".to_string());
            };
            validate_media_url("image", url, false, "cover.external.url")
        }
        "file" => {
            let file = value
                .get("file")
                .and_then(Value::as_object)
                .ok_or_else(|| "cover.file must be an object".to_string())?;
            let Some(Value::String(url)) = file.get("url") else {
                return Err("cover.file.url must be a string".to_string());
            };
            validate_cover_file_object(file, url.trim())
        }
        "file_upload" => {
            let file_upload = value
                .get("file_upload")
                .and_then(Value::as_object)
                .ok_or_else(|| "cover.file_upload must be an object".to_string())?;
            let Some(Value::String(id)) = file_upload.get("id") else {
                return Err("cover.file_upload.id must be a string".to_string());
            };
            require_uuid(id, "cover.file_upload.id")
        }
        _ => Err("cover.type must be design, file, external, or file_upload".to_string()),
    }
}

pub fn validate_cover_file_object(
    file: &serde_json::Map<String, Value>,
    file_url: &str,
) -> Result<(), String> {
    if file_url.is_empty() {
        return Err("cover.file.url must be a non-empty string".to_string());
    }
    validate_optional_non_empty_string(file.get("name"), "cover.file.name")?;
    let local_asset_path = file
        .get("ganbaru_asset_path")
        .and_then(Value::as_str)
        .map(str::trim);
    validate_optional_asset_path(
        file.get("ganbaru_asset_path"),
        "cover.file.ganbaru_asset_path",
        &["notes/page-covers/"],
    )?;
    if let Some(asset_path) = local_asset_path {
        if file_url != format!("ganbaru-asset:{asset_path}") {
            return Err("cover.file.url must reference the managed cover asset path".to_string());
        }
        validate_local_image_metadata(file, "cover.file")?;
        return Ok(());
    }
    if file_url.starts_with("ganbaru-asset:") {
        return Err("cover.file.url must include managed asset metadata".to_string());
    }
    validate_media_url("image", file_url, false, "cover.file.url")?;
    match file.get("expiry_time") {
        Some(Value::String(expiry_time)) if !contains_control_characters(expiry_time) => Ok(()),
        Some(Value::String(_)) => {
            Err("cover.file.expiry_time must not contain control characters".to_string())
        }
        _ => Err("cover.file.expiry_time must be a string".to_string()),
    }
}

pub fn validate_icon_value(value: &Value, field: &str) -> Result<(), String> {
    match value {
        Value::Object(icon) => validate_icon_object(icon, field),
        _ => Err(format!("{field} must be an icon object")),
    }
}

pub fn validate_icon_object(
    icon: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), String> {
    let Some(Value::String(icon_type)) = icon.get("type") else {
        return Err(format!("{field}.type must be a string"));
    };
    match icon_type.as_str() {
        "emoji" => match icon.get("emoji") {
            Some(Value::String(value)) if !value.trim().is_empty() => Ok(()),
            _ => Err(format!("{field}.emoji must be a non-empty string")),
        },
        "custom_emoji" => match icon.get("custom_emoji").and_then(Value::as_object) {
            Some(custom_emoji) => validate_custom_emoji_icon(custom_emoji, field),
            _ => Err(format!("{field}.custom_emoji must be an object")),
        },
        "icon" => validate_native_icon(icon, field),
        "external" => match icon.get("external").and_then(Value::as_object) {
            Some(external) => match external.get("url") {
                Some(Value::String(url)) => validate_external_icon_url(url, field),
                _ => Err(format!("{field}.external.url must be a non-empty string")),
            },
            None => Err(format!("{field}.external must be an object")),
        },
        "file" => match icon.get("file").and_then(Value::as_object) {
            Some(file) => validate_file_icon(file, field),
            _ => Err(format!("{field}.file must be an object")),
        },
        _ => Err(format!("{field}.type must be a supported Notion icon type")),
    }
}

pub fn validate_custom_emoji_icon(
    custom_emoji: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), String> {
    match custom_emoji.get("id") {
        Some(Value::String(id)) if !id.trim().is_empty() => {}
        _ => {
            return Err(format!(
                "{field}.custom_emoji.id must be a non-empty string"
            ));
        }
    }
    validate_optional_non_empty_string(
        custom_emoji.get("name"),
        &format!("{field}.custom_emoji.name"),
    )?;
    let local_asset_path = custom_emoji
        .get("ganbaru_asset_path")
        .and_then(Value::as_str)
        .map(str::trim);
    validate_optional_icon_url(
        custom_emoji.get("url"),
        &format!("{field}.custom_emoji.url"),
        &["project-icons/"],
    )?;
    validate_optional_asset_path(
        custom_emoji.get("ganbaru_asset_path"),
        &format!("{field}.custom_emoji.ganbaru_asset_path"),
        &["project-icons/"],
    )?;
    if let Some(Value::String(url)) = custom_emoji.get("url") {
        if let Some(url_asset_path) = url.trim().strip_prefix("ganbaru-asset:") {
            if local_asset_path != Some(url_asset_path) {
                return Err(format!(
                    "{field}.custom_emoji.url must reference the managed icon asset path"
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_external_icon_url(url: &str, field: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(format!("{field}.external.url must be a non-empty string"));
    }
    let parsed = Url::parse(trimmed)
        .map_err(|_| format!("{field}.external.url must be a supported HTTPS image URL"))?;
    if parsed.scheme() != "https" || !has_supported_image_extension(parsed.path()) {
        return Err(format!(
            "{field}.external.url must be a supported HTTPS image URL"
        ));
    }
    Ok(())
}

pub fn validate_file_icon(
    file: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), String> {
    let file_url = match file.get("url") {
        Some(Value::String(url)) if !url.trim().is_empty() => url.trim(),
        _ => return Err(format!("{field}.file.url must be a non-empty string")),
    };
    validate_optional_non_empty_string(
        file.get("expiry_time"),
        &format!("{field}.file.expiry_time"),
    )?;
    validate_optional_non_empty_string(file.get("name"), &format!("{field}.file.name"))?;
    let local_asset_path = file
        .get("ganbaru_asset_path")
        .and_then(Value::as_str)
        .map(str::trim);
    validate_optional_asset_path(
        file.get("ganbaru_asset_path"),
        &format!("{field}.file.ganbaru_asset_path"),
        &["notes/page-icons/"],
    )?;
    if let Some(asset_path) = local_asset_path {
        if file_url != format!("ganbaru-asset:{asset_path}") {
            return Err(format!(
                "{field}.file.url must reference the managed icon asset path"
            ));
        }
        validate_local_image_metadata(file, &format!("{field}.file"))?;
    } else if file_url.starts_with("ganbaru-asset:") {
        return Err(format!(
            "{field}.file.url must include managed asset metadata"
        ));
    } else {
        validate_remote_icon_file_url(file_url, &format!("{field}.file.url"))?;
    }
    Ok(())
}

pub fn validate_native_icon(
    icon: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), String> {
    let native = icon
        .get("icon")
        .and_then(Value::as_object)
        .ok_or_else(|| format!("{field}.icon must be an object"))?;
    match native.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => {}
        _ => return Err(format!("{field}.icon.name must be a non-empty string")),
    }
    match native.get("color") {
        None => Ok(()),
        Some(Value::String(color)) if NOTE_ICON_COLORS.contains(&color.as_str()) => Ok(()),
        Some(Value::String(_)) => Err(format!(
            "{field}.icon.color must be a supported Notion icon color"
        )),
        _ => Err(format!("{field}.icon.color must be a string")),
    }
}

pub fn validate_local_image_metadata(
    file: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<(), String> {
    match file.get("content_type") {
        Some(Value::String(content_type))
            if NOTE_LOCAL_IMAGE_CONTENT_TYPES.contains(&content_type.as_str()) => {}
        _ => {
            return Err(format!(
                "{field}.content_type must be a supported local image type"
            ));
        }
    }
    match file.get("byte_size").and_then(Value::as_i64) {
        Some(size) if size > 0 => {}
        _ => return Err(format!("{field}.byte_size must be positive")),
    }
    match file.get("sha256") {
        Some(Value::String(hash)) if is_sha256_hex(hash) => {}
        _ => {
            return Err(format!(
                "{field}.sha256 must be a lowercase SHA-256 hex digest"
            ));
        }
    }
    Ok(())
}

pub fn validate_optional_non_empty_string(
    value: Option<&Value>,
    field: &str,
) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(()),
        Some(Value::String(_)) => Err(format!("{field} must be a non-empty string")),
        Some(_) => Err(format!("{field} must be a string")),
    }
}

pub fn validate_optional_icon_url(
    value: Option<&Value>,
    field: &str,
    asset_prefixes: &[&str],
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let Value::String(url) = value else {
        return Err(format!("{field} must be a string"));
    };
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must be a non-empty string"));
    }
    if let Some(relative_path) = trimmed.strip_prefix("ganbaru-asset:") {
        return validate_managed_image_asset_path(relative_path, field, asset_prefixes);
    }
    let parsed =
        Url::parse(trimmed).map_err(|_| format!("{field} must be a supported HTTPS image URL"))?;
    if parsed.scheme() != "https" || !has_supported_image_extension(parsed.path()) {
        return Err(format!("{field} must be a supported HTTPS image URL"));
    }
    Ok(())
}

pub fn validate_optional_asset_path(
    value: Option<&Value>,
    field: &str,
    prefixes: &[&str],
) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let Value::String(path) = value else {
        return Err(format!("{field} must be a string"));
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must be a non-empty string"));
    }
    validate_managed_image_asset_path(trimmed, field, prefixes)
}

pub fn validate_managed_image_asset_path(
    path: &str,
    field: &str,
    prefixes: &[&str],
) -> Result<(), String> {
    let valid_prefix = prefixes.iter().any(|prefix| path.starts_with(prefix));
    if !valid_prefix
        || path.contains("..")
        || path.contains('\\')
        || !has_supported_image_extension(path)
    {
        return Err(format!(
            "{field} must stay under a managed image asset directory"
        ));
    }
    let remainder = prefixes
        .iter()
        .find_map(|prefix| path.strip_prefix(prefix))
        .unwrap_or_default();
    if remainder.is_empty() || remainder.contains('/') {
        return Err(format!(
            "{field} must stay under a managed image asset directory"
        ));
    }
    Ok(())
}

pub fn validate_remote_icon_file_url(url: &str, field: &str) -> Result<(), String> {
    let parsed =
        Url::parse(url).map_err(|_| format!("{field} must be a supported HTTPS image URL"))?;
    if parsed.scheme() != "https" || !has_supported_image_extension(parsed.path()) {
        return Err(format!("{field} must be a supported HTTPS image URL"));
    }
    Ok(())
}

pub fn has_supported_image_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    NOTE_MANAGED_IMAGE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

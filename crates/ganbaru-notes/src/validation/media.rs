use super::*;

const IMAGE_EXTENSIONS: &[&str] = &[
    ".bmp", ".gif", ".heic", ".jpeg", ".jpg", ".png", ".svg", ".tif", ".tiff", ".webp",
];
const AUDIO_EXTENSIONS: &[&str] = &[".mp3", ".wav", ".ogg", ".oga", ".m4a"];
const VIDEO_EXTENSIONS: &[&str] = &[
    ".amv", ".asf", ".avi", ".f4v", ".flv", ".gifv", ".mkv", ".mov", ".mpg", ".mpeg", ".mpv",
    ".mp4", ".m4v", ".qt", ".wmv",
];
pub fn validate_media_payload(block_type: &str, payload: &Value) -> Result<(), String> {
    match payload.get("caption") {
        Some(Value::Array(caption)) => {
            for item in caption {
                validate_rich_text_item(item)?;
            }
        }
        None if block_type != "file" => {}
        _ => return Err(format!("{block_type}.caption must be a rich text array")),
    }
    match payload.get("name") {
        None => {}
        Some(Value::String(name)) if !contains_control_characters(name) => {}
        Some(Value::String(_)) => {
            return Err(format!(
                "{block_type}.name must not contain control characters"
            ));
        }
        _ => return Err(format!("{block_type}.name must be a string")),
    }
    let Some(Value::String(source_type)) = payload.get("type") else {
        return Err(format!("{block_type}.type must be a string"));
    };
    match source_type.as_str() {
        "external" => {
            let external = payload
                .get("external")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{block_type}.external must be an object"))?;
            let Some(Value::String(url)) = external.get("url") else {
                return Err(format!("{block_type}.external.url must be a string"));
            };
            validate_media_url(block_type, url, true, &format!("{block_type}.external.url"))
        }
        "file" => {
            let file = payload
                .get("file")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{block_type}.file must be an object"))?;
            let Some(Value::String(url)) = file.get("url") else {
                return Err(format!("{block_type}.file.url must be a string"));
            };
            let local_asset_path = file
                .get("ganbaru_asset_path")
                .and_then(Value::as_str)
                .map(str::trim);
            validate_optional_non_empty_string(
                file.get("name"),
                &format!("{block_type}.file.name"),
            )?;
            if let Some(asset_path) = local_asset_path {
                if url.trim() != format!("ganbaru-asset:{asset_path}") {
                    return Err(format!(
                        "{block_type}.file.url must reference the managed file asset path"
                    ));
                }
                validate_local_media_metadata(file, block_type)?;
                Ok(())
            } else if url.trim().starts_with("ganbaru-asset:") {
                Err(format!(
                    "{block_type}.file.url must include managed asset metadata"
                ))
            } else {
                validate_media_url(block_type, url, false, &format!("{block_type}.file.url"))?;
                match file.get("expiry_time") {
                    Some(Value::String(expiry_time))
                        if !contains_control_characters(expiry_time) =>
                    {
                        Ok(())
                    }
                    Some(Value::String(_)) => Err(format!(
                        "{block_type}.file.expiry_time must not contain control characters"
                    )),
                    _ => Err(format!("{block_type}.file.expiry_time must be a string")),
                }
            }
        }
        "file_upload" => {
            let file_upload = payload
                .get("file_upload")
                .and_then(Value::as_object)
                .ok_or_else(|| format!("{block_type}.file_upload must be an object"))?;
            let Some(Value::String(id)) = file_upload.get("id") else {
                return Err(format!("{block_type}.file_upload.id must be a string"));
            };
            require_uuid(id, &format!("{block_type}.file_upload.id"))
        }
        _ => Err(format!(
            "{block_type}.type must be file, external, or file_upload"
        )),
    }
}

pub fn validate_local_media_metadata(
    file: &serde_json::Map<String, Value>,
    block_type: &str,
) -> Result<(), String> {
    let Some(Value::String(asset_path)) = file.get("ganbaru_asset_path") else {
        return Err(format!(
            "{block_type}.file.ganbaru_asset_path must be a string"
        ));
    };
    validate_managed_file_asset_path(
        asset_path.trim(),
        &format!("{block_type}.file.ganbaru_asset_path"),
    )?;
    let content_type = match file.get("content_type") {
        Some(Value::String(content_type)) if !contains_control_characters(content_type) => {
            content_type.trim()
        }
        _ => {
            return Err(format!(
                "{block_type}.file.content_type must be a MIME type"
            ));
        }
    };
    if !local_media_content_type_matches_block(block_type, content_type) {
        return Err(format!(
            "{block_type}.file.content_type must match the local media block type"
        ));
    }
    match file.get("byte_size").and_then(Value::as_i64) {
        Some(size) if size > 0 => {}
        _ => return Err(format!("{block_type}.file.byte_size must be positive")),
    }
    match file.get("sha256") {
        Some(Value::String(hash)) if is_sha256_hex(hash) => {}
        _ => {
            return Err(format!(
                "{block_type}.file.sha256 must be a lowercase SHA-256 hex digest"
            ));
        }
    }
    Ok(())
}

pub fn validate_managed_file_asset_path(path: &str, field: &str) -> Result<(), String> {
    let Some(remainder) = path.strip_prefix("notes/files/") else {
        return Err(format!(
            "{field} must stay under the managed Notes file directory"
        ));
    };
    if remainder.is_empty() || remainder.contains('/') || path.contains("..") || path.contains('\\')
    {
        return Err(format!(
            "{field} must stay under the managed Notes file directory"
        ));
    }
    Ok(())
}

pub fn local_media_content_type_matches_block(block_type: &str, content_type: &str) -> bool {
    match block_type {
        "image" => matches!(content_type, "image/png" | "image/jpeg" | "image/webp"),
        "video" => content_type.starts_with("video/"),
        "audio" => content_type.starts_with("audio/"),
        "pdf" => content_type == "application/pdf",
        "file" => {
            content_type.contains('/')
                && !content_type.contains(' ')
                && !content_type.contains(';')
                && content_type != "image/svg+xml"
        }
        _ => false,
    }
}

pub fn validate_media_url(
    block_type: &str,
    url: &str,
    allow_empty: bool,
    field: &str,
) -> Result<(), String> {
    if url.trim().is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(format!("{field} must not be empty"))
        };
    }
    if contains_control_characters(url) {
        return Err(format!("{field} must not contain control characters"));
    }
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| format!("{field} must be a supported HTTPS {block_type} URL"))?;
    if parsed.scheme() != "https" || !media_url_matches_block_type(block_type, &parsed) {
        return Err(format!(
            "{field} must be a supported HTTPS {block_type} URL"
        ));
    }
    Ok(())
}

pub fn media_url_matches_block_type(block_type: &str, url: &reqwest::Url) -> bool {
    match block_type {
        "file" => true,
        "pdf" => path_has_extension(url, &[".pdf"]),
        "image" => path_has_extension(url, IMAGE_EXTENSIONS),
        "audio" => path_has_extension(url, AUDIO_EXTENSIONS),
        "video" => path_has_extension(url, VIDEO_EXTENSIONS) || is_youtube_video_url(url),
        _ => false,
    }
}

pub fn path_has_extension(url: &reqwest::Url, extensions: &[&str]) -> bool {
    let path = url.path().to_ascii_lowercase();
    extensions.iter().any(|extension| path.ends_with(extension))
}

pub fn is_youtube_video_url(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host != "www.youtube.com" && host != "youtube.com" {
        return false;
    }
    if url.path() == "/watch" {
        return url
            .query_pairs()
            .any(|(key, value)| key == "v" && !value.is_empty());
    }
    url.path().starts_with("/embed/")
}

pub fn validate_bookmark_payload(payload: &Value) -> Result<(), String> {
    match payload.get("caption") {
        Some(Value::Array(caption)) => {
            for item in caption {
                validate_rich_text_item(item)?;
            }
        }
        _ => return Err("bookmark.caption must be a rich text array".to_string()),
    }
    match payload.get("url") {
        Some(Value::String(url)) if is_bookmark_url_string(url) => Ok(()),
        Some(Value::String(_)) => {
            Err("bookmark.url must not contain control characters".to_string())
        }
        _ => Err("bookmark.url must be a string".to_string()),
    }
}

pub fn is_bookmark_url_string(url: &str) -> bool {
    !url.chars().any(char::is_control)
}

pub fn validate_embed_payload(payload: &Value) -> Result<(), String> {
    match payload.get("url") {
        Some(Value::String(url)) if !url.chars().any(char::is_control) => Ok(()),
        Some(Value::String(_)) => Err("embed.url must not contain control characters".to_string()),
        _ => Err("embed.url must be a string".to_string()),
    }
}

pub fn validate_link_preview_payload(payload: &Value) -> Result<(), String> {
    match payload.get("url") {
        Some(Value::String(url)) if !url.chars().any(char::is_control) => Ok(()),
        Some(Value::String(_)) => {
            Err("link_preview.url must not contain control characters".to_string())
        }
        _ => Err("link_preview.url must be a string".to_string()),
    }
}

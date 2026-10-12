pub mod files;
pub mod image_metadata;
pub mod page_covers;
pub mod page_icons;

use serde_json::{Map, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::collections::HashSet;
use std::path::{Component, Path};

pub const ASSET_SOURCE_LOCAL_UPLOAD: &str = "local_upload";
pub const ASSET_SOURCE_IMPORTED: &str = "imported";
pub const ASSET_STATE_AVAILABLE: &str = "available";
pub const ASSET_STATE_MISSING: &str = "missing";

const ASSET_PAGE_ICON_PREFIX: &str = "notes/page-icons/";
const ASSET_PAGE_COVER_PREFIX: &str = "notes/page-covers/";
const ASSET_FILE_PREFIX: &str = "notes/files/";
const ASSET_ALLOWED_IMAGE_CONTENT_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp"];
const ASSET_ALLOWED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const ASSET_SOURCE_TYPES: &[&str] = &[
    "local_upload",
    "generated",
    "imported",
    "external_reference",
];
const ASSET_STORAGE_STATES: &[&str] = &["available", "missing"];

pub struct ManagedAssetWrite<'a> {
    pub relative_path: &'a str,
    pub original_name: Option<&'a str>,
    pub content_type: &'a str,
    pub byte_size: i64,
    pub sha256: &'a str,
    pub source_type: &'a str,
    pub storage_state: &'a str,
    pub missing_at: Option<&'a str>,
}

pub async fn upsert_managed_asset_tx(
    tx: &mut Transaction<'_, Sqlite>,
    asset: ManagedAssetWrite<'_>,
) -> Result<(), String> {
    let kind = validate_managed_asset(&asset)?;
    let original_name = asset
        .original_name
        .map(str::trim)
        .filter(|name| !name.is_empty());
    sqlx::query(
        "INSERT INTO notes_assets (
            id,
            asset_path,
            kind,
            source_type,
            original_name,
            content_type,
            byte_size,
            sha256,
            storage_state,
            missing_at
         )
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(asset_path)
         DO UPDATE SET
            kind = excluded.kind,
            source_type = excluded.source_type,
            original_name = COALESCE(excluded.original_name, notes_assets.original_name),
            content_type = excluded.content_type,
            byte_size = excluded.byte_size,
            sha256 = excluded.sha256,
            storage_state = excluded.storage_state,
            missing_at = excluded.missing_at,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(asset.relative_path.trim())
    .bind(asset.relative_path.trim())
    .bind(kind)
    .bind(asset.source_type.trim())
    .bind(original_name)
    .bind(asset.content_type.trim())
    .bind(asset.byte_size)
    .bind(asset.sha256.trim())
    .bind(asset.storage_state.trim())
    .bind(asset.missing_at.map(str::trim))
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("record notes asset: {e}"))?;
    Ok(())
}

pub async fn mark_managed_asset_storage_state(
    pool: &SqlitePool,
    relative_path: &str,
    missing: bool,
    context: &str,
) -> Result<(), String> {
    let storage_state = if missing {
        ASSET_STATE_MISSING
    } else {
        ASSET_STATE_AVAILABLE
    };
    sqlx::query(
        "UPDATE notes_assets
         SET storage_state = ?,
             missing_at = CASE
                WHEN ? = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                ELSE NULL
             END,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE asset_path = ?",
    )
    .bind(storage_state)
    .bind(if missing { 1_i64 } else { 0_i64 })
    .bind(relative_path.trim())
    .execute(pool)
    .await
    .map_err(|e| format!("update {context} asset storage state: {e}"))?;
    Ok(())
}

pub async fn sync_page_asset_references_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    icon_is_set: bool,
    icon: Option<&Value>,
    cover_is_set: bool,
    cover: Option<&Value>,
) -> Result<(), String> {
    if icon_is_set {
        replace_page_asset_reference_tx(
            tx,
            page_id,
            "page_icon",
            icon,
            "icon",
            ASSET_PAGE_ICON_PREFIX,
        )
        .await?;
    }
    if cover_is_set {
        replace_page_asset_reference_tx(
            tx,
            page_id,
            "page_cover",
            cover,
            "cover",
            ASSET_PAGE_COVER_PREFIX,
        )
        .await?;
    }
    Ok(())
}

pub async fn sync_block_asset_reference_tx(
    tx: &mut Transaction<'_, Sqlite>,
    block_id: &str,
    page_id: &str,
    block_type: &str,
    payload: &Value,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_asset_references
         WHERE owner_type = 'block' AND owner_id = ? AND role IN ('block_file', 'callout_icon')",
    )
    .bind(block_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear notes block asset reference: {e}"))?;

    let (role, asset) = if block_type == "callout" {
        (
            "callout_icon",
            page_local_file_asset(payload.get("icon"), "callout.icon", ASSET_PAGE_ICON_PREFIX)?,
        )
    } else if matches!(block_type, "image" | "video" | "audio" | "file" | "pdf") {
        ("block_file", media_local_file_asset(payload, block_type)?)
    } else {
        ("block_file", None)
    };
    let Some(asset) = asset else {
        return Ok(());
    };
    let asset_path = asset.relative_path.trim().to_string();
    upsert_managed_asset_tx(tx, asset).await?;
    sqlx::query(
        "INSERT INTO notes_asset_references (
            asset_id,
            owner_type,
            owner_id,
            page_id,
            block_id,
            role
         )
         VALUES (?, 'block', ?, ?, ?, ?)
         ON CONFLICT(asset_id, owner_type, owner_id, role)
         DO UPDATE SET
            page_id = excluded.page_id,
            block_id = excluded.block_id,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(asset_path)
    .bind(block_id)
    .bind(page_id)
    .bind(block_id)
    .bind(role)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("record notes block asset reference: {e}"))?;
    Ok(())
}

pub async fn sync_current_data_source_property_asset_references_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
) -> Result<(), String> {
    let properties: String = sqlx::query_scalar(
        "SELECT properties
         FROM notes_data_sources
         WHERE id = ? AND in_trash = 0",
    )
    .bind(data_source_id.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| format!("load notes data source properties for asset references: {e}"))?
    .ok_or_else(|| "data source not found".to_string())?;
    let schema_properties: Value = serde_json::from_str(&properties)
        .map_err(|e| format!("parse data source properties for asset references: {e}"))?;
    sync_data_source_property_asset_references_tx(tx, data_source_id, &schema_properties).await
}

pub async fn sync_data_source_property_asset_references_tx(
    tx: &mut Transaction<'_, Sqlite>,
    data_source_id: &str,
    schema_properties: &Value,
) -> Result<(), String> {
    let data_source_id = data_source_id.trim();
    if data_source_id.is_empty() {
        return Err("data_source_id is required".to_string());
    }
    let property_ids = file_property_ids(schema_properties)?;
    sqlx::query(
        "DELETE FROM notes_asset_references
         WHERE owner_type = 'data_source_property'
           AND owner_id = ?
           AND role = 'property_file'",
    )
    .bind(data_source_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear notes data source property asset references: {e}"))?;

    if property_ids.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query_as::<_, (String,)>(
        "SELECT properties
         FROM notes_pages
         WHERE parent_type = 'data_source_id'
           AND parent_data_source_id = ?",
    )
    .bind(data_source_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| format!("load notes row properties for asset references: {e}"))?;
    let mut seen_asset_paths = HashSet::new();
    for (row_properties,) in rows {
        let properties: Value = serde_json::from_str(&row_properties)
            .map_err(|e| format!("parse row properties for asset references: {e}"))?;
        for property_id in &property_ids {
            for asset in data_source_property_local_file_assets(&properties, property_id)? {
                let asset_path = asset.relative_path.trim().to_string();
                if !seen_asset_paths.insert(asset_path.clone()) {
                    continue;
                }
                upsert_managed_asset_tx(tx, asset).await?;
                sqlx::query(
                    "INSERT INTO notes_asset_references (
                        asset_id,
                        owner_type,
                        owner_id,
                        data_source_id,
                        property_id,
                        role
                     )
                     VALUES (?, 'data_source_property', ?, ?, ?, 'property_file')",
                )
                .bind(asset_path)
                .bind(data_source_id)
                .bind(data_source_id)
                .bind(property_id)
                .execute(&mut **tx)
                .await
                .map_err(|e| format!("record notes data source property asset reference: {e}"))?;
            }
        }
    }
    Ok(())
}

pub async fn sync_comment_asset_references_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    comment_id: &str,
    attachments: &[Value],
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_asset_references
         WHERE owner_type = 'comment' AND owner_id = ? AND role = 'comment_attachment'",
    )
    .bind(comment_id.trim())
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear notes comment asset references: {e}"))?;

    let mut seen_asset_paths = HashSet::new();
    for (index, attachment) in attachments.iter().enumerate() {
        let Some(asset) = comment_attachment_local_file_asset(attachment, index)? else {
            continue;
        };
        let asset_path = asset.relative_path.trim().to_string();
        if !seen_asset_paths.insert(asset_path.clone()) {
            continue;
        }
        upsert_managed_asset_tx(tx, asset).await?;
        sqlx::query(
            "INSERT INTO notes_asset_references (
                asset_id,
                owner_type,
                owner_id,
                page_id,
                comment_id,
                role
             )
             VALUES (?, 'comment', ?, ?, ?, 'comment_attachment')",
        )
        .bind(asset_path)
        .bind(comment_id.trim())
        .bind(page_id.trim())
        .bind(comment_id.trim())
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("record notes comment asset reference: {e}"))?;
    }
    Ok(())
}

async fn replace_page_asset_reference_tx(
    tx: &mut Transaction<'_, Sqlite>,
    page_id: &str,
    role: &str,
    value: Option<&Value>,
    field: &str,
    expected_prefix: &str,
) -> Result<(), String> {
    sqlx::query(
        "DELETE FROM notes_asset_references
         WHERE owner_type = 'page' AND owner_id = ? AND role = ?",
    )
    .bind(page_id)
    .bind(role)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("clear notes page asset reference: {e}"))?;

    let Some(asset) = page_local_file_asset(value, field, expected_prefix)? else {
        return Ok(());
    };
    let asset_path = asset.relative_path.trim().to_string();
    upsert_managed_asset_tx(tx, asset).await?;
    sqlx::query(
        "INSERT INTO notes_asset_references (
            asset_id,
            owner_type,
            owner_id,
            page_id,
            role
         )
         VALUES (?, 'page', ?, ?, ?)
         ON CONFLICT(asset_id, owner_type, owner_id, role)
         DO UPDATE SET
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(asset_path)
    .bind(page_id)
    .bind(page_id)
    .bind(role)
    .execute(&mut **tx)
    .await
    .map_err(|e| format!("record notes page asset reference: {e}"))?;
    Ok(())
}

fn page_local_file_asset<'a>(
    value: Option<&'a Value>,
    field: &str,
    expected_prefix: &str,
) -> Result<Option<ManagedAssetWrite<'a>>, String> {
    let Some(Value::Object(object)) = value else {
        return Ok(None);
    };
    if object.get("type").and_then(Value::as_str) != Some("file") {
        return Ok(None);
    }
    let Some(file) = object.get("file").and_then(Value::as_object) else {
        return Ok(None);
    };
    managed_file_asset_from_file_object(
        file,
        &format!("{field}.file"),
        expected_prefix,
        file.get("name").and_then(Value::as_str),
    )
}

fn media_local_file_asset<'a>(
    payload: &'a Value,
    block_type: &str,
) -> Result<Option<ManagedAssetWrite<'a>>, String> {
    let Value::Object(object) = payload else {
        return Ok(None);
    };
    if object.get("type").and_then(Value::as_str) != Some("file") {
        return Ok(None);
    }
    let Some(file) = object.get("file").and_then(Value::as_object) else {
        return Ok(None);
    };
    managed_file_asset_from_file_object(
        file,
        &format!("{block_type}.file"),
        ASSET_FILE_PREFIX,
        file.get("name").and_then(Value::as_str),
    )
}

fn file_property_ids(schema_properties: &Value) -> Result<Vec<String>, String> {
    let object = schema_properties
        .as_object()
        .ok_or_else(|| "data source properties must be an object".to_string())?;
    let mut property_ids = Vec::new();
    for property in object.values() {
        let property = property
            .as_object()
            .ok_or_else(|| "data source property must be an object".to_string())?;
        if property.get("type").and_then(Value::as_str) != Some("files") {
            continue;
        }
        let property_id = property
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "data source files property id must be a string".to_string())?
            .trim();
        if property_id.is_empty() {
            return Err("data source files property id is required".to_string());
        }
        property_ids.push(property_id.to_string());
    }
    Ok(property_ids)
}

fn data_source_property_local_file_assets<'a>(
    properties: &'a Value,
    property_id: &str,
) -> Result<Vec<ManagedAssetWrite<'a>>, String> {
    let object = properties
        .as_object()
        .ok_or_else(|| "row properties must be an object".to_string())?;
    let Some(property) = object.values().find(|value| {
        value.get("id").and_then(Value::as_str) == Some(property_id)
            && value.get("type").and_then(Value::as_str) == Some("files")
    }) else {
        return Ok(Vec::new());
    };
    let files = property
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| "row files property value must be an array".to_string())?;
    let mut assets = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if let Some(asset) = property_file_local_asset(file, property_id, index)? {
            assets.push(asset);
        }
    }
    Ok(assets)
}

fn property_file_local_asset<'a>(
    value: &'a Value,
    property_id: &str,
    index: usize,
) -> Result<Option<ManagedAssetWrite<'a>>, String> {
    let Some(object) = value.as_object() else {
        return Err("row files property items must be objects".to_string());
    };
    if object.get("type").and_then(Value::as_str) != Some("file") {
        return Ok(None);
    }
    let Some(file) = object.get("file").and_then(Value::as_object) else {
        return Ok(None);
    };
    let fallback_name = object.get("name").and_then(Value::as_str);
    managed_file_asset_from_file_object(
        file,
        &format!("row property {property_id}.files[{index}].file"),
        ASSET_FILE_PREFIX,
        file.get("name").and_then(Value::as_str).or(fallback_name),
    )
}

fn comment_attachment_local_file_asset<'a>(
    value: &'a Value,
    index: usize,
) -> Result<Option<ManagedAssetWrite<'a>>, String> {
    let Some(object) = value.as_object() else {
        return Err("comment attachments must be objects".to_string());
    };
    if object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|attachment_type| attachment_type != "file")
    {
        return Ok(None);
    }
    let Some(file) = object.get("file").and_then(Value::as_object) else {
        return Ok(None);
    };
    let fallback_name = object.get("name").and_then(Value::as_str);
    managed_file_asset_from_file_object(
        file,
        &format!("comment.attachments[{index}].file"),
        ASSET_FILE_PREFIX,
        file.get("name").and_then(Value::as_str).or(fallback_name),
    )
}

fn managed_file_asset_from_file_object<'a>(
    file: &'a Map<String, Value>,
    field: &str,
    expected_prefix: &str,
    original_name: Option<&'a str>,
) -> Result<Option<ManagedAssetWrite<'a>>, String> {
    let Some(relative_path) = file.get("ganbaru_asset_path").and_then(Value::as_str) else {
        if file
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|url| url.trim().starts_with("ganbaru-asset:"))
        {
            return Err(format!("{field}.ganbaru_asset_path must be a string"));
        }
        return Ok(None);
    };
    let relative_path = relative_path.trim();
    if !relative_path.starts_with(expected_prefix) {
        return Err(format!(
            "{field}.ganbaru_asset_path must stay under the expected managed asset directory"
        ));
    }
    let url = file
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field}.url must be a string"))?;
    if url.trim() != format!("ganbaru-asset:{relative_path}") {
        return Err(format!("{field}.url must reference the managed asset path"));
    }
    let content_type = file
        .get("content_type")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field}.content_type must be a string"))?;
    let byte_size = file
        .get("byte_size")
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("{field}.byte_size must be an integer"))?;
    let sha256 = file
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{field}.sha256 must be a string"))?;
    Ok(Some(ManagedAssetWrite {
        relative_path,
        original_name,
        content_type,
        byte_size,
        sha256,
        source_type: ASSET_SOURCE_LOCAL_UPLOAD,
        storage_state: ASSET_STATE_AVAILABLE,
        missing_at: None,
    }))
}

fn validate_managed_asset(asset: &ManagedAssetWrite<'_>) -> Result<&'static str, String> {
    let relative_path = asset.relative_path.trim();
    validate_managed_asset_path(relative_path)?;
    let kind = asset_kind_for_content_type(asset.content_type.trim())?;
    let is_page_media_asset = relative_path.starts_with(ASSET_PAGE_ICON_PREFIX)
        || relative_path.starts_with(ASSET_PAGE_COVER_PREFIX);
    if is_page_media_asset && kind != "image" {
        return Err("page icon and cover assets must be images".to_string());
    }
    validate_asset_size(asset.byte_size)?;
    validate_sha256(asset.sha256.trim())?;
    validate_allowed_value(
        asset.source_type.trim(),
        ASSET_SOURCE_TYPES,
        "notes asset source_type",
    )?;
    validate_allowed_value(
        asset.storage_state.trim(),
        ASSET_STORAGE_STATES,
        "notes asset storage_state",
    )?;
    match (asset.storage_state.trim(), asset.missing_at.map(str::trim)) {
        ("available", None) => {}
        ("missing", Some(missing_at)) if !missing_at.is_empty() => {
            validate_no_control_characters(missing_at, "notes asset missing_at")?;
        }
        ("available", Some(_)) => {
            return Err("available notes assets must not have missing_at".to_string());
        }
        ("missing", _) => return Err("missing notes assets must have missing_at".to_string()),
        _ => {}
    }
    if let Some(original_name) = asset.original_name.map(str::trim) {
        validate_no_control_characters(original_name, "notes asset original_name")?;
    }
    Ok(kind)
}

fn validate_managed_asset_path(relative_path: &str) -> Result<(), String> {
    let prefix = [
        ASSET_PAGE_ICON_PREFIX,
        ASSET_PAGE_COVER_PREFIX,
        ASSET_FILE_PREFIX,
    ]
    .iter()
    .find(|prefix| relative_path.starts_with(**prefix))
    .ok_or_else(|| {
        "notes asset path must stay under a managed Notes asset directory".to_string()
    })?;
    let file_name = relative_path.strip_prefix(*prefix).ok_or_else(|| {
        "notes asset path must stay under a managed Notes asset directory".to_string()
    })?;
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains("..")
    {
        return Err("notes asset path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("notes asset path cannot contain nested or parent paths".to_string());
    }
    if (*prefix == ASSET_PAGE_ICON_PREFIX || *prefix == ASSET_PAGE_COVER_PREFIX)
        && !has_allowed_image_extension(path)
    {
        return Err("page icon and cover assets must be PNG, JPG, or WebP".to_string());
    }
    Ok(())
}

fn asset_kind_for_content_type(content_type: &str) -> Result<&'static str, String> {
    validate_mime_type(content_type)?;
    if ASSET_ALLOWED_IMAGE_CONTENT_TYPES.contains(&content_type) {
        return Ok("image");
    }
    if content_type == "image/svg+xml" {
        return Err("SVG assets are blocked because they can contain active content".to_string());
    }
    if content_type.starts_with("video/") {
        return Ok("video");
    }
    if content_type.starts_with("audio/") {
        return Ok("audio");
    }
    if content_type == "application/pdf" {
        return Ok("pdf");
    }
    Ok("file")
}

fn validate_mime_type(content_type: &str) -> Result<(), String> {
    if content_type.is_empty()
        || content_type.contains(' ')
        || content_type.contains(';')
        || content_type.matches('/').count() != 1
        || content_type.chars().any(char::is_control)
    {
        return Err("notes asset content_type must be a plain MIME type".to_string());
    }
    let Some((kind, subtype)) = content_type.split_once('/') else {
        return Err("notes asset content_type must be a plain MIME type".to_string());
    };
    if kind.is_empty() || subtype.is_empty() {
        return Err("notes asset content_type must be a plain MIME type".to_string());
    }
    Ok(())
}

fn validate_asset_size(byte_size: i64) -> Result<(), String> {
    if byte_size <= 0 {
        return Err("notes asset byte_size must be positive".to_string());
    }
    Ok(())
}

fn validate_sha256(sha256: &str) -> Result<(), String> {
    if sha256.len() != 64 || !sha256.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err("notes asset sha256 must be a SHA-256 hex digest".to_string());
    }
    if sha256.chars().any(|value| value.is_ascii_uppercase()) {
        return Err("notes asset sha256 must be lowercase".to_string());
    }
    Ok(())
}

fn validate_allowed_value(value: &str, allowed: &[&str], field: &str) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("{field} is unsupported"))
    }
}

fn validate_no_control_characters(value: &str, field: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        Err(format!("{field} must not contain control characters"))
    } else {
        Ok(())
    }
}

fn has_allowed_image_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            ASSET_ALLOWED_IMAGE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_asset(relative_path: &str) -> ManagedAssetWrite<'_> {
        ManagedAssetWrite {
            relative_path,
            original_name: Some("focus.png"),
            content_type: "image/png",
            byte_size: 42,
            sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            source_type: ASSET_SOURCE_LOCAL_UPLOAD,
            storage_state: ASSET_STATE_AVAILABLE,
            missing_at: None,
        }
    }

    #[test]
    fn validate_managed_asset_accepts_supported_paths_and_types() {
        assert_eq!(
            validate_managed_asset(&valid_asset(
                "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            ))
            .unwrap(),
            "image",
        );
        let asset = ManagedAssetWrite {
            relative_path: "notes/files/report.pdf",
            original_name: Some("report.pdf"),
            content_type: "application/pdf",
            byte_size: 12,
            sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            source_type: "imported",
            storage_state: "missing",
            missing_at: Some("2026-07-02T12:00:00.000Z"),
        };
        assert_eq!(validate_managed_asset(&asset).unwrap(), "pdf");
    }

    #[test]
    fn validate_managed_asset_rejects_unsafe_asset_shapes() {
        for (asset, expected) in [
            (
                ManagedAssetWrite {
                    relative_path: "notes/files/../secret.pdf",
                    ..valid_asset("notes/files/../secret.pdf")
                },
                "notes asset path cannot contain nested or parent paths",
            ),
            (
                ManagedAssetWrite {
                    relative_path: "notes/page-icons/icon.svg",
                    ..valid_asset("notes/page-icons/icon.svg")
                },
                "page icon and cover assets must be PNG, JPG, or WebP",
            ),
            (
                ManagedAssetWrite {
                    content_type: "image/svg+xml",
                    ..valid_asset("notes/files/icon.svg")
                },
                "SVG assets are blocked because they can contain active content",
            ),
            (
                ManagedAssetWrite {
                    byte_size: 0,
                    ..valid_asset("notes/files/empty.txt")
                },
                "notes asset byte_size must be positive",
            ),
            (
                ManagedAssetWrite {
                    sha256: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                    ..valid_asset("notes/files/file.bin")
                },
                "notes asset sha256 must be lowercase",
            ),
            (
                ManagedAssetWrite {
                    storage_state: "missing",
                    missing_at: None,
                    ..valid_asset("notes/files/file.bin")
                },
                "missing notes assets must have missing_at",
            ),
        ] {
            assert_eq!(
                validate_managed_asset(&asset).err().as_deref(),
                Some(expected)
            );
        }
    }
}

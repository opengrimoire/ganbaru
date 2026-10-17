use base64::{Engine as _, engine::general_purpose};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use crate::assets::image_metadata::{
    ManagedImageDimensionError, ManagedImageKind, ManagedImageMetadata, ManagedImageMetadataError,
    parse_managed_image_metadata, validate_managed_image_dimensions,
};

use crate::assets::{self, ASSET_SOURCE_LOCAL_UPLOAD, ASSET_STATE_AVAILABLE, ManagedAssetWrite};

const PAGE_ICON_MAX_DISPLAY_MEGABYTES: usize = 3;
const PAGE_ICON_MAX_BYTES: usize = PAGE_ICON_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const PAGE_ICON_MAX_BASE64_CHARS: usize = PAGE_ICON_MAX_BYTES.div_ceil(3) * 4;
const PAGE_ICON_DIR: &str = "notes/page-icons";
pub const PAGE_ICON_ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePageIconAssetDto {
    pub relative_path: String,
    pub original_name: Option<String>,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
}

fn active_page_icon_dir(vault_root: &Path) -> PathBuf {
    vault_root.join("assets").join(PAGE_ICON_DIR)
}

fn asset_path_for_relative(vault_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let file_name = validate_page_icon_relative_path(relative_path)?;
    Ok(active_page_icon_dir(vault_root).join(file_name))
}

fn validate_page_icon_relative_path(relative_path: &str) -> Result<&str, String> {
    let relative_path = relative_path.trim();
    let prefix = format!("{PAGE_ICON_DIR}/");
    let file_name = relative_path
        .strip_prefix(&prefix)
        .ok_or_else(|| "page icon path must stay under notes/page-icons".to_string())?;
    if file_name.is_empty() {
        return Err("page icon path is missing a file name".to_string());
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("page icon path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("page icon path cannot contain nested or parent paths".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !PAGE_ICON_ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        return Err(page_icon_unsupported_type_error());
    }
    Ok(file_name)
}

fn page_icon_unsupported_type_error() -> String {
    "Use PNG, JPG, or WebP. SVG is blocked for security because it can contain interactive or external content.".to_string()
}

fn ensure_page_icon_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("page icon image is empty".to_string());
    }
    if bytes.len() > PAGE_ICON_MAX_BYTES {
        return Err(format!(
            "page icon image exceeds the {PAGE_ICON_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    Ok(())
}

fn page_icon_metadata_error(error: ManagedImageMetadataError) -> String {
    match error {
        ManagedImageMetadataError::UnsupportedFormat => page_icon_unsupported_type_error(),
        ManagedImageMetadataError::MalformedHeader(reason) => {
            format!("page icon image header is malformed: {reason}")
        }
    }
}

fn page_icon_dimension_error(error: ManagedImageDimensionError) -> String {
    format!("page icon image {error}")
}

fn validate_page_icon_image(bytes: &[u8]) -> Result<ManagedImageMetadata, String> {
    let metadata = parse_managed_image_metadata(bytes).map_err(page_icon_metadata_error)?;
    validate_managed_image_dimensions(metadata).map_err(page_icon_dimension_error)?;
    Ok(metadata)
}

fn hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn write_binary_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "page icon target has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("create page icon directory: {e}"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "page icon target has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let temporary_path = parent.join(format!("{file_name}.tmp"));
    {
        let mut file =
            fs::File::create(&temporary_path).map_err(|e| format!("write page icon: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("write page icon: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync page icon: {e}"))?;
    }
    fs::rename(&temporary_path, path).map_err(|e| format!("save page icon: {e}"))
}

pub fn read_file_capped(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("inspect page icon image: {e}"))?;
    if !metadata.is_file() {
        return Err("page icon image path must be a file".to_string());
    }
    if metadata.len() > PAGE_ICON_MAX_BYTES as u64 {
        return Err(format!(
            "page icon image exceeds the {PAGE_ICON_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("read page icon image: {e}"))?;
    ensure_page_icon_size(&bytes)?;
    Ok(bytes)
}

pub fn decode_page_icon_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let trimmed = data_url.trim();
    let Some((data_url_metadata, payload)) = trimmed.split_once(',') else {
        return Err("page icon data URL is malformed".to_string());
    };
    let Some(declared_mime_type) = data_url_metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.strip_suffix(";base64"))
    else {
        return Err("page icon data URL must be a base64 image".to_string());
    };
    if ManagedImageKind::from_mime_type(declared_mime_type).is_none() {
        return Err("page icon data URL must be a PNG, JPEG, or WebP image".to_string());
    }
    if payload.len() > PAGE_ICON_MAX_BASE64_CHARS {
        return Err(format!(
            "page icon image exceeds the {PAGE_ICON_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("decode page icon data URL: {e}"))?;
    ensure_page_icon_size(&bytes)?;
    let metadata = validate_page_icon_image(&bytes)?;
    if !metadata.kind.matches_mime_type(declared_mime_type) {
        return Err("page icon data URL MIME type does not match its image contents".to_string());
    }
    Ok(bytes)
}

fn page_icon_data_url(bytes: &[u8]) -> Result<String, String> {
    ensure_page_icon_size(bytes)?;
    let kind = validate_page_icon_image(bytes)?.kind;
    Ok(format!(
        "data:{};base64,{}",
        kind.mime_type(),
        general_purpose::STANDARD.encode(bytes)
    ))
}

pub async fn save_page_icon_bytes(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    bytes: Vec<u8>,
    original_name: Option<String>,
) -> Result<NotePageIconAssetDto, String> {
    ensure_page_icon_size(&bytes)?;
    let kind = validate_page_icon_image(&bytes)?.kind;
    let sha256 = hex_hash(&bytes);
    let file_name = format!("{}.{}", sha256, kind.extension());
    let relative_path = format!("{PAGE_ICON_DIR}/{file_name}");
    let path = active_page_icon_dir(vault_root).join(file_name);
    if !path.exists() {
        write_binary_file_atomically(&path, &bytes)?;
    }
    let original_name = original_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin page icon asset record: {e}"))?;
    sqlx::query(
        "INSERT INTO notes_page_icon_assets
            (id, asset_path, original_name, content_type, byte_size, sha256)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(asset_path)
         DO UPDATE SET
            original_name = COALESCE(excluded.original_name, notes_page_icon_assets.original_name),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(&sha256)
    .bind(&relative_path)
    .bind(&original_name)
    .bind(kind.mime_type())
    .bind(bytes.len() as i64)
    .bind(&sha256)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("record page icon asset: {e}"))?;
    assets::upsert_managed_asset_tx(
        &mut tx,
        ManagedAssetWrite {
            relative_path: &relative_path,
            original_name: original_name.as_deref(),
            content_type: kind.mime_type(),
            byte_size: bytes.len() as i64,
            sha256: &sha256,
            source_type: ASSET_SOURCE_LOCAL_UPLOAD,
            storage_state: ASSET_STATE_AVAILABLE,
            missing_at: None,
        },
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit page icon asset record: {e}"))?;
    Ok(NotePageIconAssetDto {
        relative_path,
        original_name,
        content_type: kind.mime_type().to_string(),
        byte_size: bytes.len() as i64,
        sha256,
    })
}

pub async fn page_icon_asset_data_url(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    relative_path: String,
) -> Result<String, String> {
    let path = asset_path_for_relative(vault_root, &relative_path)?;
    let bytes = match read_file_capped(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ =
                assets::mark_managed_asset_storage_state(pool, &relative_path, true, "page icon")
                    .await;
            return Err(error);
        }
    };
    assets::mark_managed_asset_storage_state(pool, &relative_path, false, "page icon").await?;
    page_icon_data_url(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0; 33];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[8..12].copy_from_slice(&13u32.to_be_bytes());
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn page_icon_validation_accepts_bounded_image_metadata() {
        assert_eq!(
            validate_page_icon_image(&png(4032, 3024)).unwrap().kind,
            ManagedImageKind::Png
        );
        assert!(validate_page_icon_image(&png(5000, 4000)).is_err());
    }

    #[test]
    fn page_icon_data_url_rejects_mime_signature_mismatch() {
        let payload = general_purpose::STANDARD.encode(png(100, 50));
        assert!(decode_page_icon_data_url(&format!("data:image/png;base64,{payload}")).is_ok());
        assert_eq!(
            decode_page_icon_data_url(&format!("data:image/webp;base64,{payload}")).unwrap_err(),
            "page icon data URL MIME type does not match its image contents"
        );
    }

    #[test]
    fn validate_page_icon_relative_path_rejects_escape_paths() {
        assert!(validate_page_icon_relative_path(
            "notes/page-icons/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        )
        .is_ok());
        assert!(validate_page_icon_relative_path("project-icons/a.png").is_err());
        assert!(validate_page_icon_relative_path("notes/page-icons/../a.png").is_err());
        assert!(validate_page_icon_relative_path("notes/page-icons/nested/a.png").is_err());
        assert!(validate_page_icon_relative_path("notes/page-icons/a.svg").is_err());
    }
}

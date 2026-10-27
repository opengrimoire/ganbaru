use base64::{Engine as _, engine::general_purpose};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Component, Path, PathBuf},
};

use crate::assets::image_metadata::{
    ManagedImageDimensionError, ManagedImageKind, ManagedImageMetadata, ManagedImageMetadataError,
    parse_managed_image_metadata, validate_managed_image_dimensions,
};

use crate::assets::{self, ASSET_SOURCE_LOCAL_UPLOAD, ASSET_STATE_AVAILABLE, ManagedAssetWrite};
use crate::blocking::run_blocking;
use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped};

const PAGE_COVER_MAX_BYTES: usize = ganbaru_sync::domains::notes::MAX_PAGE_COVER_BYTES as usize;
const PAGE_COVER_MAX_DISPLAY_MEGABYTES: usize = PAGE_COVER_MAX_BYTES / (1024 * 1024);
const PAGE_COVER_MAX_BASE64_CHARS: usize = PAGE_COVER_MAX_BYTES.div_ceil(3) * 4;
const PAGE_COVER_DIR: &str = "notes/page-covers";
pub const PAGE_COVER_ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePageCoverAssetDto {
    pub relative_path: String,
    pub original_name: Option<String>,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
}

fn active_page_cover_dir(vault_root: &Path) -> PathBuf {
    vault_root.join("assets").join(PAGE_COVER_DIR)
}

fn asset_path_for_relative(vault_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let file_name = validate_page_cover_relative_path(relative_path)?;
    Ok(active_page_cover_dir(vault_root).join(file_name))
}

fn validate_page_cover_relative_path(relative_path: &str) -> Result<&str, String> {
    let relative_path = relative_path.trim();
    let prefix = format!("{PAGE_COVER_DIR}/");
    let file_name = relative_path
        .strip_prefix(&prefix)
        .ok_or_else(|| "page cover path must stay under notes/page-covers".to_string())?;
    if file_name.is_empty() {
        return Err("page cover path is missing a file name".to_string());
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("page cover path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("page cover path cannot contain nested or parent paths".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !PAGE_COVER_ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        return Err(page_cover_unsupported_type_error());
    }
    Ok(file_name)
}

fn page_cover_unsupported_type_error() -> String {
    "Use PNG, JPG, or WebP. SVG is blocked for security because it can contain interactive or external content.".to_string()
}

fn ensure_page_cover_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("page cover image is empty".to_string());
    }
    if bytes.len() > PAGE_COVER_MAX_BYTES {
        return Err(format!(
            "page cover image exceeds the {PAGE_COVER_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    Ok(())
}

fn page_cover_metadata_error(error: ManagedImageMetadataError) -> String {
    match error {
        ManagedImageMetadataError::UnsupportedFormat => page_cover_unsupported_type_error(),
        ManagedImageMetadataError::MalformedHeader(reason) => {
            format!("page cover image header is malformed: {reason}")
        }
    }
}

fn page_cover_dimension_error(error: ManagedImageDimensionError) -> String {
    format!("page cover image {error}")
}

fn validate_page_cover_image(bytes: &[u8]) -> Result<ManagedImageMetadata, String> {
    let metadata = parse_managed_image_metadata(bytes).map_err(page_cover_metadata_error)?;
    validate_managed_image_dimensions(metadata).map_err(page_cover_dimension_error)?;
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

fn write_binary_file_atomically(
    vault_root: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<(), String> {
    ganbaru_atomic_files::write_file_atomically_within(
        vault_root,
        path,
        bytes,
        ganbaru_atomic_files::FileAccess::Default,
    )
    .map_err(|e| format!("save page cover: {e}"))
}

/// Reads a page cover image the user chose in the native picker, following a final link.
pub fn read_picked_file_capped(path: &Path) -> Result<Vec<u8>, String> {
    read_file_capped(path, FinalSymlink::Follow)
}

fn read_file_capped(path: &Path, final_symlink: FinalSymlink) -> Result<Vec<u8>, String> {
    let bytes = read_regular_file_capped(path, PAGE_COVER_MAX_BYTES as u64, final_symlink)
        .map_err(|error| match error.kind() {
            io::ErrorKind::FileTooLarge => {
                format!("page cover image exceeds the {PAGE_COVER_MAX_DISPLAY_MEGABYTES} MB limit")
            }
            io::ErrorKind::InvalidInput => {
                "page cover image path must be a regular file".to_string()
            }
            _ => format!("read page cover image: {error}"),
        })?;
    ensure_page_cover_size(&bytes)?;
    Ok(bytes)
}

pub fn decode_page_cover_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let trimmed = data_url.trim();
    let Some((data_url_metadata, payload)) = trimmed.split_once(',') else {
        return Err("page cover data URL is malformed".to_string());
    };
    let Some(declared_mime_type) = data_url_metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.strip_suffix(";base64"))
    else {
        return Err("page cover data URL must be a base64 image".to_string());
    };
    if ManagedImageKind::from_mime_type(declared_mime_type).is_none() {
        return Err("page cover data URL must be a PNG, JPEG, or WebP image".to_string());
    }
    if payload.len() > PAGE_COVER_MAX_BASE64_CHARS {
        return Err(format!(
            "page cover image exceeds the {PAGE_COVER_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| format!("decode page cover data URL: {e}"))?;
    ensure_page_cover_size(&bytes)?;
    let metadata = validate_page_cover_image(&bytes)?;
    if !metadata.kind.matches_mime_type(declared_mime_type) {
        return Err("page cover data URL MIME type does not match its image contents".to_string());
    }
    Ok(bytes)
}

fn page_cover_data_url(bytes: &[u8]) -> Result<String, String> {
    ensure_page_cover_size(bytes)?;
    let kind = validate_page_cover_image(bytes)?.kind;
    Ok(format!(
        "data:{};base64,{}",
        kind.mime_type(),
        general_purpose::STANDARD.encode(bytes)
    ))
}

pub async fn save_page_cover_bytes(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    bytes: Vec<u8>,
    original_name: Option<String>,
) -> Result<NotePageCoverAssetDto, String> {
    let byte_size = bytes.len() as i64;
    let vault_root = vault_root.to_path_buf();
    let (kind, sha256, relative_path) = run_blocking(move || {
        ensure_page_cover_size(&bytes)?;
        let kind = validate_page_cover_image(&bytes)?.kind;
        let sha256 = hex_hash(&bytes);
        let file_name = ganbaru_sync::assets::file_name(&sha256, kind.mime_type())
            .map_err(|error| format!("name page cover file: {error}"))?;
        let relative_path = format!("{PAGE_COVER_DIR}/{file_name}");
        let path = active_page_cover_dir(&vault_root).join(file_name);
        if !ganbaru_atomic_files::is_regular_file(&path) {
            write_binary_file_atomically(&vault_root, &path, &bytes)?;
        }
        Ok((kind, sha256, relative_path))
    })
    .await?;
    let original_name = original_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin page cover asset record: {e}"))?;
    assets::upsert_managed_asset_tx(
        &mut tx,
        ManagedAssetWrite {
            relative_path: &relative_path,
            original_name: original_name.as_deref(),
            content_type: kind.mime_type(),
            byte_size,
            sha256: &sha256,
            source_type: ASSET_SOURCE_LOCAL_UPLOAD,
            storage_state: ASSET_STATE_AVAILABLE,
            missing_at: None,
        },
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit page cover asset record: {e}"))?;
    Ok(NotePageCoverAssetDto {
        relative_path,
        original_name,
        content_type: kind.mime_type().to_string(),
        byte_size,
        sha256,
    })
}

pub async fn page_cover_asset_data_url(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    relative_path: String,
) -> Result<String, String> {
    let path = asset_path_for_relative(vault_root, &relative_path)?;
    let read = run_blocking(move || {
        Ok(read_file_capped(&path, FinalSymlink::Refuse).map(|bytes| page_cover_data_url(&bytes)))
    })
    .await?;
    let data_url = match read {
        Ok(data_url) => data_url,
        Err(error) => {
            let _ =
                assets::mark_managed_asset_storage_state(pool, &relative_path, true, "page cover")
                    .await;
            return Err(error);
        }
    };
    assets::mark_managed_asset_storage_state(pool, &relative_path, false, "page cover").await?;
    data_url
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
    fn page_cover_validation_accepts_bounded_image_metadata() {
        assert_eq!(
            validate_page_cover_image(&png(4032, 3024)).unwrap().kind,
            ManagedImageKind::Png
        );
        assert!(validate_page_cover_image(&png(5000, 4000)).is_err());
    }

    #[test]
    fn page_cover_data_url_rejects_mime_signature_mismatch() {
        let payload = general_purpose::STANDARD.encode(png(100, 50));
        assert!(decode_page_cover_data_url(&format!("data:image/png;base64,{payload}")).is_ok());
        assert_eq!(
            decode_page_cover_data_url(&format!("data:image/jpeg;base64,{payload}")).unwrap_err(),
            "page cover data URL MIME type does not match its image contents"
        );
    }

    #[test]
    fn validate_page_cover_relative_path_rejects_escape_paths() {
        assert!(validate_page_cover_relative_path(
            "notes/page-covers/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png",
        )
        .is_ok());
        assert!(validate_page_cover_relative_path("notes/page-icons/a.png").is_err());
        assert!(validate_page_cover_relative_path("notes/page-covers/../a.png").is_err());
        assert!(validate_page_cover_relative_path("notes/page-covers/nested/a.png").is_err());
        assert!(validate_page_cover_relative_path("notes/page-covers/a.svg").is_err());
    }
}

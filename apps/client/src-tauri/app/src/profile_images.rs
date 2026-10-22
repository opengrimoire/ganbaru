use base64::{Engine as _, engine::general_purpose};
use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped};
use ganbaru_notes::assets::image_metadata::{
    ManagedImageDimensionError, ManagedImageMetadata, ManagedImageMetadataError,
    parse_managed_image_metadata, validate_managed_image_dimensions,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};
#[cfg(desktop)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

#[cfg(desktop)]
use crate::native_dialogs::on_dialog_thread;
use crate::vault;

const PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES: usize = 3;
const PROFILE_IMAGE_MAX_BYTES: usize = PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const PROFILE_IMAGE_DIR: &str = "profile";
const PROFILE_IMAGE_ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const PROFILE_CONFIG_KEY: &str = "profile";
const PROFILE_IMAGE_PATH_CONFIG_KEY: &str = "imagePath";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileImageAsset {
    pub relative_path: String,
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))
}

#[cfg(desktop)]
fn profile_image_start_directory<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path().picture_dir().ok().filter(|path| path.is_dir())
}

fn active_profile_image_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(vault::active_vault_path(app)?
        .join("assets")
        .join(PROFILE_IMAGE_DIR))
}

fn validate_profile_image_relative_path(relative_path: &str) -> Result<&str, String> {
    let relative_path = relative_path.trim();
    let prefix = format!("{PROFILE_IMAGE_DIR}/");
    let file_name = relative_path
        .strip_prefix(&prefix)
        .ok_or_else(|| "profile image path must stay under profile".to_string())?;
    if file_name.is_empty() {
        return Err("profile image path is missing a file name".to_string());
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("profile image path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("profile image path cannot contain nested or parent paths".to_string());
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !PROFILE_IMAGE_ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    {
        return Err(profile_image_unsupported_type_error());
    }
    Ok(file_name)
}

fn profile_image_unsupported_type_error() -> String {
    "Use PNG, JPG, or WebP. SVG is blocked for security because it can contain interactive or external content.".to_string()
}

fn ensure_profile_image_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("profile image is empty".to_string());
    }
    if bytes.len() > PROFILE_IMAGE_MAX_BYTES {
        return Err(format!(
            "profile image exceeds the {PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    Ok(())
}

fn profile_image_metadata_error(error: ManagedImageMetadataError) -> String {
    match error {
        ManagedImageMetadataError::UnsupportedFormat => profile_image_unsupported_type_error(),
        ManagedImageMetadataError::MalformedHeader(reason) => {
            format!("profile image header is malformed: {reason}")
        }
    }
}

fn profile_image_dimension_error(error: ManagedImageDimensionError) -> String {
    format!("profile image {error}")
}

fn validate_profile_image(bytes: &[u8]) -> Result<ManagedImageMetadata, String> {
    let metadata = parse_managed_image_metadata(bytes).map_err(profile_image_metadata_error)?;
    validate_managed_image_dimensions(metadata).map_err(profile_image_dimension_error)?;
    Ok(metadata)
}

fn read_file_capped(path: &Path, final_symlink: FinalSymlink) -> Result<Vec<u8>, String> {
    let bytes = read_regular_file_capped(path, PROFILE_IMAGE_MAX_BYTES as u64, final_symlink)
        .map_err(|error| match error.kind() {
            io::ErrorKind::FileTooLarge => {
                format!("profile image exceeds the {PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES} MB limit")
            }
            io::ErrorKind::InvalidInput => "profile image path must be a regular file".to_string(),
            _ => format!("read profile image: {error}"),
        })?;
    ensure_profile_image_size(&bytes)?;
    Ok(bytes)
}

fn content_hash(bytes: &[u8]) -> String {
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
    .map_err(|error| format!("save profile image: {error}"))
}

fn save_profile_image_bytes<R: Runtime>(
    app: &AppHandle<R>,
    bytes: Vec<u8>,
) -> Result<ProfileImageAsset, String> {
    ensure_profile_image_size(&bytes)?;
    let kind = validate_profile_image(&bytes)?.kind;
    let file_name = format!("{}.{}", content_hash(&bytes), kind.extension());
    let relative_path = format!("{PROFILE_IMAGE_DIR}/{file_name}");
    let writable_root = vault::active_writable_vault_path(app)?;
    let vault_root = writable_root.to_path_buf();
    let path = writable_root
        .join("assets")
        .join(PROFILE_IMAGE_DIR)
        .join(file_name);
    if !ganbaru_atomic_files::is_regular_file(&path) {
        write_binary_file_atomically(&vault_root, &path, &bytes)?;
    }
    Ok(ProfileImageAsset { relative_path })
}

fn decode_profile_image_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let encoded_max_bytes = PROFILE_IMAGE_MAX_BYTES.div_ceil(3) * 4;
    let (metadata, encoded) = data_url
        .split_once(',')
        .ok_or_else(|| "profile image must be a base64 data URL".to_string())?;
    let mime_type = metadata
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .ok_or_else(|| "profile image must be a base64 data URL".to_string())?;
    if !matches!(mime_type, "image/png" | "image/jpeg" | "image/webp") {
        return Err(profile_image_unsupported_type_error());
    }
    if encoded.len() > encoded_max_bytes {
        return Err(format!(
            "profile image exceeds the {PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| format!("decode profile image: {error}"))?;
    ensure_profile_image_size(&bytes)?;
    let kind = validate_profile_image(&bytes)?.kind;
    if kind.mime_type() != mime_type {
        return Err("profile image MIME type does not match its content".to_string());
    }
    Ok(bytes)
}

fn profile_image_asset_path<R: Runtime>(
    app: &AppHandle<R>,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let file_name = validate_profile_image_relative_path(relative_path)?;
    Ok(active_profile_image_dir(app)?.join(file_name))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn profile_image_pick_file<R: Runtime>(
    app: AppHandle<R>,
    title: String,
) -> Result<Option<ProfileImageAsset>, String> {
    let mut picker = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter("Image", PROFILE_IMAGE_ALLOWED_EXTENSIONS);
    if let Some(directory) = profile_image_start_directory(&app) {
        picker = picker.set_directory(directory);
    }
    let picked = on_dialog_thread(move || picker.blocking_pick_file()).await?;
    let Some(path) = picked.map(dialog_path).transpose()? else {
        return Ok(None);
    };
    let bytes = read_file_capped(&path, FinalSymlink::Follow)?;
    save_profile_image_bytes(&app, bytes).map(Some)
}

#[tauri::command]
pub fn profile_image_save_data_url<R: Runtime>(
    app: AppHandle<R>,
    data_url: String,
) -> Result<ProfileImageAsset, String> {
    save_profile_image_bytes(&app, decode_profile_image_data_url(&data_url)?)
}

#[tauri::command]
pub fn profile_image_asset_data_url<R: Runtime>(
    app: AppHandle<R>,
    relative_path: String,
) -> Result<String, String> {
    let path = profile_image_asset_path(&app, &relative_path)?;
    let bytes = read_file_capped(&path, FinalSymlink::Refuse)?;
    let kind = validate_profile_image(&bytes)?.kind;
    Ok(format!(
        "data:{};base64,{}",
        kind.mime_type(),
        general_purpose::STANDARD.encode(bytes)
    ))
}

/// Whether a vault configuration document names the profile image file `file_name` as the
/// saved picture. Names compare without case because Windows and macOS volumes usually
/// resolve them that way.
fn is_configured_profile_image(view: &serde_json::Value, file_name: &str) -> bool {
    let configured = view
        .get(PROFILE_CONFIG_KEY)
        .and_then(|profile| profile.get(PROFILE_IMAGE_PATH_CONFIG_KEY))
        .and_then(serde_json::Value::as_str)
        .and_then(|path| validate_profile_image_relative_path(path).ok());
    configured.is_some_and(|configured| configured.eq_ignore_ascii_case(file_name))
}

/// Deletes a replaced or discarded profile image. The image the vault preferences still name is
/// refused, so the webview can delete only pictures the profile no longer uses.
#[tauri::command]
pub async fn profile_image_delete_file<R: Runtime>(
    app: AppHandle<R>,
    relative_path: String,
) -> Result<(), String> {
    let _write_permit = vault::active_writable_vault_path(&app)?;
    let file_name = validate_profile_image_relative_path(&relative_path)?;
    let view = vault::preferences::effective_view(&app).await?;
    if is_configured_profile_image(&view, file_name) {
        return Err("the saved profile image cannot be deleted".to_string());
    }
    let path = profile_image_asset_path(&app, &relative_path)?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("delete profile image: {error}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_notes::assets::image_metadata::ManagedImageKind;

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
    fn profile_image_metadata_accepts_bounded_images() {
        assert_eq!(
            validate_profile_image(&png(4032, 3024)).unwrap().kind,
            ManagedImageKind::Png
        );
        assert!(validate_profile_image(&png(5000, 4000)).is_err());
    }

    #[test]
    fn profile_image_kind_rejects_unsafe_or_unknown_images() {
        assert!(
            validate_profile_image(br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#).is_err()
        );
        assert!(validate_profile_image(b"GIF89amore").is_err());
        assert!(validate_profile_image(b"not-image").is_err());
    }

    #[test]
    fn profile_image_relative_path_stays_in_managed_directory() {
        assert_eq!(
            validate_profile_image_relative_path(
                "profile/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
            )
            .unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.png"
        );
        assert!(validate_profile_image_relative_path("../outside.png").is_err());
        assert!(validate_profile_image_relative_path("profile/../outside.png").is_err());
        assert!(validate_profile_image_relative_path("profile/nested/image.png").is_err());
        assert!(validate_profile_image_relative_path("profile/image.svg").is_err());
    }

    #[test]
    fn saved_profile_image_is_recognized_from_the_vault_preferences() {
        let view = serde_json::json!({ "profile": { "imagePath": " profile/Saved.png " } });
        assert!(is_configured_profile_image(&view, "Saved.png"));
        assert!(is_configured_profile_image(&view, "saved.PNG"));
        assert!(!is_configured_profile_image(&view, "replaced.png"));

        for view in [
            serde_json::json!({}),
            serde_json::json!({ "profile": {} }),
            serde_json::json!({ "profile": { "imagePath": null } }),
            serde_json::json!({ "profile": { "imagePath": "../Saved.png" } }),
        ] {
            assert!(!is_configured_profile_image(&view, "Saved.png"), "{view}");
        }
    }

    #[test]
    fn profile_image_data_url_accepts_validated_content() {
        let bytes = png(64, 64);
        let data_url = format!(
            "data:image/png;base64,{}",
            general_purpose::STANDARD.encode(&bytes)
        );
        assert_eq!(decode_profile_image_data_url(&data_url).unwrap(), bytes);
    }

    #[test]
    fn profile_image_data_url_rejects_mismatched_or_unbounded_content() {
        let jpeg_labeled_png = format!(
            "data:image/jpeg;base64,{}",
            general_purpose::STANDARD.encode(png(64, 64))
        );
        assert!(decode_profile_image_data_url(&jpeg_labeled_png).is_err());

        let oversized = format!(
            "data:image/png;base64,{}",
            "A".repeat(PROFILE_IMAGE_MAX_BYTES.div_ceil(3) * 4 + 1)
        );
        assert!(decode_profile_image_data_url(&oversized).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn vault_profile_image_reads_refuse_symlinks_and_oversized_files() {
        let root = std::env::temp_dir().join(format!(
            "ganbaru-profile-read-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let picture = root.join("picture.png");
        fs::write(&picture, png(64, 64)).unwrap();
        let link = root.join("linked.png");
        std::os::unix::fs::symlink(&picture, &link).unwrap();
        let oversized = root.join("oversized.png");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(PROFILE_IMAGE_MAX_BYTES as u64 + 1)
            .unwrap();

        assert_eq!(
            read_file_capped(&link, FinalSymlink::Follow).unwrap(),
            png(64, 64)
        );
        assert_eq!(
            read_file_capped(&link, FinalSymlink::Refuse).unwrap_err(),
            "profile image path must be a regular file"
        );
        assert!(
            read_file_capped(&oversized, FinalSymlink::Refuse)
                .unwrap_err()
                .contains("MB limit")
        );
        fs::remove_dir_all(&root).unwrap();
    }
}

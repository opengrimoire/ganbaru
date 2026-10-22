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
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::domains::preferences::{
    MAX_PROFILE_IMAGE_BYTES, PROFILE_IMAGE_PATH_FIELD, PROFILE_IMAGE_SIZE_FIELD, ProfileImage,
};
use ganbaru_sync::manifest::vault::preferences::{
    DOMAIN_NAME as PREFERENCES_DOMAIN, PROFILE_CLASS,
};

const PROFILE_IMAGE_MAX_BYTES: usize = MAX_PROFILE_IMAGE_BYTES as usize;
const PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES: usize = PROFILE_IMAGE_MAX_BYTES / (1024 * 1024);
const PROFILE_IMAGE_DIR: &str = PROFILE_CLASS.name;
const PROFILE_IMAGE_ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const PROFILE_CONFIG_KEY: &str = "profile";
const PROFILE_IMAGE_CONFIG_KEY: &str = "image";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileImageAsset {
    pub relative_path: String,
    /// Exact file size, which the `profile.image` preference records for sync.
    pub byte_size: u32,
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
    Ok(PROFILE_CLASS.directory(&vault::active_vault_path(app)?))
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
        return Err(profile_image_too_large_error());
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
            io::ErrorKind::FileTooLarge => profile_image_too_large_error(),
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
    let asset = ProfileImageAsset {
        relative_path: format!("{PROFILE_IMAGE_DIR}/{file_name}"),
        byte_size: u32::try_from(bytes.len()).map_err(|_| profile_image_too_large_error())?,
    };
    if ProfileImage::from_json(&asset.preference_value()).is_none() {
        return Err(profile_image_unsupported_type_error());
    }
    let writable_root = vault::active_replicated_asset_path(app, PREFERENCES_DOMAIN)?;
    let vault_root = writable_root.to_path_buf();
    let path = PROFILE_CLASS.directory(&vault_root).join(file_name);
    if !ganbaru_atomic_files::is_regular_file(&path) {
        write_binary_file_atomically(&vault_root, &path, &bytes)?;
    }
    Ok(asset)
}

impl ProfileImageAsset {
    /// The `profile.image` preference value that names this file.
    fn preference_value(&self) -> serde_json::Value {
        serde_json::json!({
            PROFILE_IMAGE_PATH_FIELD: self.relative_path,
            PROFILE_IMAGE_SIZE_FIELD: self.byte_size,
        })
    }
}

fn profile_image_too_large_error() -> String {
    format!("profile image exceeds the {PROFILE_IMAGE_MAX_DISPLAY_MEGABYTES} MB limit")
}

/// Checks a profile picture that sync received, given the file that holds its bytes after their
/// size and digest matched the reference: the bytes must be a supported image within the limits,
/// of the content type the reference names. Sync registers it as the check of [`PROFILE_CLASS`].
pub(crate) fn check_synced_profile_image(asset: &AssetRef, path: &Path) -> Result<(), String> {
    let bytes = read_file_capped(path, FinalSymlink::Refuse)?;
    let kind = validate_profile_image(&bytes)?.kind;
    if kind.mime_type() != asset.content_type() {
        return Err(format!(
            "the bytes are {} but the reference names {}",
            kind.mime_type(),
            asset.content_type()
        ));
    }
    Ok(())
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
        .and_then(|profile| profile.get(PROFILE_IMAGE_CONFIG_KEY))
        .filter(|image| ProfileImage::from_json(image).is_some())
        .and_then(|image| image.get(PROFILE_IMAGE_PATH_FIELD))
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
    let _write_permit = vault::active_replicated_asset_path(&app, PREFERENCES_DOMAIN)?;
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
        let digest = "a".repeat(64);
        let saved = format!("{digest}.png");
        let view = serde_json::json!({
            "profile": { "image": { "path": format!("profile/{saved}"), "byteSize": 2048 } }
        });
        assert!(is_configured_profile_image(&view, &saved));
        assert!(is_configured_profile_image(
            &view,
            &saved.to_ascii_uppercase()
        ));
        assert!(!is_configured_profile_image(&view, "replaced.png"));

        for view in [
            serde_json::json!({}),
            serde_json::json!({ "profile": {} }),
            serde_json::json!({ "profile": { "image": null } }),
            serde_json::json!({ "profile": { "image": format!("profile/{saved}") } }),
            serde_json::json!({ "profile": { "image": { "path": format!("profile/{saved}") } } }),
            serde_json::json!({
                "profile": { "image": { "path": format!("../{saved}"), "byteSize": 2048 } }
            }),
        ] {
            assert!(!is_configured_profile_image(&view, &saved), "{view}");
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

        let bytes = png(64, 64);
        let reference = |content_type: &str| {
            AssetRef::new(
                &PROFILE_CLASS,
                content_hash(&bytes),
                content_type.to_string(),
                bytes.len() as u32,
            )
            .unwrap()
        };
        assert_eq!(
            check_synced_profile_image(&reference("image/png"), &picture),
            Ok(())
        );
        assert!(check_synced_profile_image(&reference("image/jpeg"), &picture).is_err());
        assert!(check_synced_profile_image(&reference("image/png"), &link).is_err());
        let svg = root.join("picture.svg");
        fs::write(&svg, br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#).unwrap();
        assert!(check_synced_profile_image(&reference("image/png"), &svg).is_err());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn saved_profile_images_name_a_value_sync_reads() {
        let asset = ProfileImageAsset {
            relative_path: format!("{PROFILE_IMAGE_DIR}/{}.png", "b".repeat(64)),
            byte_size: 2048,
        };
        let image = ProfileImage::from_json(&asset.preference_value()).unwrap();
        assert_eq!(image.byte_size, 2048);
        assert_eq!(image.content_type, "image/png");
    }
}

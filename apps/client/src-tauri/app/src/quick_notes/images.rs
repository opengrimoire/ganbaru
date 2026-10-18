//! Quick note image file commands. Images move as raw IPC bytes instead of data URLs; Android
//! cannot send raw request bodies, so its imports carry base64 text instead.
//!
//! The owner and linked stable replicas may write image files, since both write Quick notes.
//! Files are content-addressed and verified, so a replica only ever adds images that match
//! their names.

use base64::{Engine as _, engine::general_purpose};
use ganbaru_quick_notes::images::{self, MAX_IMAGE_BYTES};
use ganbaru_quick_notes::{QuickNoteImage, QuickNoteImageError, QuickNoteImageFile};
#[cfg(desktop)]
use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Runtime};

use crate::db::connect_sqlite;
use crate::vault;

/// Base64 text of the largest accepted image, with padding.
const MAX_BASE64_CHARS: usize = (MAX_IMAGE_BYTES as usize).div_ceil(3) * 4;

/// Stores image bytes in the active vault and returns the reference a note saves.
///
/// Desktop sends the bytes as the raw request body. Android sends `{ "base64": "..." }`.
#[tauri::command]
pub async fn quick_notes_import_image<R: Runtime>(
    app: AppHandle<R>,
    request: Request<'_>,
) -> Result<QuickNoteImage, QuickNoteImageError> {
    let bytes = request_bytes(request.body())?;
    let vault_root = vault::active_replicated_asset_path(&app)?;
    blocking(move || images::import_image(&vault_root, &bytes)).await
}

/// Reads a verified image file of the active vault as raw bytes.
#[tauri::command]
pub async fn quick_notes_read_image<R: Runtime>(
    app: AppHandle<R>,
    file: QuickNoteImageFile,
) -> Result<Response, QuickNoteImageError> {
    let vault_root = vault::active_vault_path(&app)?;
    let bytes = blocking(move || images::read_image(&vault_root, &file)).await?;
    Ok(Response::new(bytes))
}

/// Deletes image files that nothing references any more. Devices that cannot write Quick
/// notes skip the pass.
#[tauri::command]
pub async fn quick_notes_collect_images<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<u64, String> {
    let status = vault::ownership::active_status(&app)?;
    if !status.can_write && !status.replicated_writes {
        return Ok(0);
    }
    let pool = connect_sqlite(app.clone(), db_url).await?;
    let vault_root = vault::active_replicated_asset_path(&app)?;
    images::collect_unreferenced_images(&pool, &vault_root).await
}

/// Images a picker imported, and how many picked files were left out.
#[cfg(desktop)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteImagePick {
    images: Vec<QuickNoteImage>,
    rejected: Vec<QuickNoteImageRejection>,
    skipped: usize,
}

/// A picked file that could not be imported.
#[cfg(desktop)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteImageRejection {
    file_name: String,
    error: QuickNoteImageError,
}

/// Lets the user pick image files and imports up to `limit` of them, in picking order.
/// Canceling returns an empty pick.
#[cfg(desktop)]
#[tauri::command]
pub async fn quick_notes_pick_images<R: Runtime>(
    app: AppHandle<R>,
    limit: usize,
) -> Result<QuickNoteImagePick, QuickNoteImageError> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let mut picker = app
        .dialog()
        .file()
        .set_title("Add images")
        .add_filter("Image", &images::IMAGE_EXTENSIONS);
    if let Some(directory) = app.path().picture_dir().ok().filter(|path| path.is_dir()) {
        picker = picker.set_directory(directory);
    }
    let picked = blocking(move || Ok(picker.blocking_pick_files())).await?;
    let paths = picked
        .unwrap_or_default()
        .into_iter()
        .map(|path| {
            path.into_path()
                .map_err(|error| format!("selected path is not a local file: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let limit = limit.min(images::MAX_IMAGES);
    let skipped = paths.len().saturating_sub(limit);
    let vault_root = vault::active_replicated_asset_path(&app)?;
    blocking(move || {
        let mut pick = QuickNoteImagePick {
            images: Vec::new(),
            rejected: Vec::new(),
            skipped,
        };
        for path in paths.iter().take(limit) {
            match images::read_image_source(path)
                .and_then(|bytes| images::import_image(&vault_root, &bytes))
            {
                Ok(image) => pick.images.push(image),
                Err(QuickNoteImageError::Failed(message)) => {
                    return Err(QuickNoteImageError::Failed(message));
                }
                Err(error) => pick.rejected.push(QuickNoteImageRejection {
                    file_name: display_name(path),
                    error,
                }),
            }
        }
        Ok(pick)
    })
    .await
}

/// Saves a copy of an image wherever the user chooses. Returns `false` when canceled.
#[cfg(desktop)]
#[tauri::command]
pub async fn quick_notes_download_image<R: Runtime>(
    app: AppHandle<R>,
    file: QuickNoteImageFile,
) -> Result<bool, QuickNoteImageError> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    let vault_root = vault::active_vault_path(&app)?;
    let (name, extension) = download_name(&file)?;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Download image")
        .set_file_name(&name)
        .add_filter("Image", &[extension.as_str()]);
    if let Some(directory) = app.path().download_dir().ok().filter(|path| path.is_dir()) {
        dialog = dialog.set_directory(directory);
    }
    let Some(target) = blocking(move || Ok(dialog.blocking_save_file())).await? else {
        return Ok(false);
    };
    let target = target
        .into_path()
        .map_err(|error| format!("selected path is not a local file: {error}"))?;
    blocking(move || {
        let bytes = images::read_image(&vault_root, &file)?;
        std::fs::write(&target, bytes)
            .map_err(|error| QuickNoteImageError::Failed(format!("save image: {error}")))?;
        Ok(true)
    })
    .await
}

/// Saves a copy of an image to Downloads. Returns `true` once saved.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn quick_notes_download_image(
    app: AppHandle,
    file: QuickNoteImageFile,
) -> Result<bool, QuickNoteImageError> {
    use ganbaru_mobile_documents::MobileDocumentsExt;

    let vault_root = vault::active_vault_path(&app)?;
    let (name, extension) = download_name(&file)?;
    let path = images::image_path(&vault_root, &file)?;
    let content_type = file.content_type().to_string();
    // The plugin streams the file; reading it first proves it matches its name.
    blocking(move || images::read_image(&vault_root, &file).map(drop)).await?;
    let source = path
        .to_str()
        .ok_or_else(|| "image path is not valid UTF-8".to_string())?;
    app.mobile_documents().save_file_download(
        source,
        &name,
        u64::from(MAX_IMAGE_BYTES),
        &[extension.as_str()],
        &content_type,
        "image",
    )?;
    Ok(true)
}

/// The suggested download name, `image-{12 digest digits}.{ext}`, and its extension.
fn download_name(file: &QuickNoteImageFile) -> Result<(String, String), QuickNoteImageError> {
    let file_name = file.file_name()?;
    let (digest, extension) = file_name
        .split_once('.')
        .ok_or_else(|| "image file name has no extension".to_string())?;
    let short = digest.get(..12).unwrap_or(digest);
    Ok((format!("image-{short}.{extension}"), extension.to_string()))
}

#[cfg(desktop)]
fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The image bytes of an import request, refusing oversized bodies before decoding.
fn request_bytes(body: &InvokeBody) -> Result<Vec<u8>, QuickNoteImageError> {
    match body {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(value) => {
            let encoded = value
                .get("base64")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "image import has no bytes".to_string())?;
            if encoded.len() > MAX_BASE64_CHARS {
                return Err(QuickNoteImageError::TooLarge);
            }
            general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| QuickNoteImageError::Unsupported)
        }
    }
}

/// Runs file work off the async executor.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, QuickNoteImageError> + Send + 'static,
) -> Result<T, QuickNoteImageError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| QuickNoteImageError::Failed(format!("image task failed: {error}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn digest() -> String {
        "ab".repeat(32)
    }

    #[test]
    fn raw_and_base64_bodies_carry_the_same_bytes() {
        let bytes = vec![1_u8, 2, 3, 250];
        assert_eq!(
            request_bytes(&InvokeBody::Raw(bytes.clone())).unwrap(),
            bytes
        );
        let encoded = general_purpose::STANDARD.encode(&bytes);
        assert_eq!(
            request_bytes(&InvokeBody::Json(json!({ "base64": encoded }))).unwrap(),
            bytes
        );
    }

    #[test]
    fn base64_bodies_are_bounded_and_validated() {
        let oversized = "A".repeat(MAX_BASE64_CHARS + 4);
        assert_eq!(
            request_bytes(&InvokeBody::Json(json!({ "base64": oversized }))),
            Err(QuickNoteImageError::TooLarge)
        );
        assert_eq!(
            request_bytes(&InvokeBody::Json(json!({ "base64": "not base64!" }))),
            Err(QuickNoteImageError::Unsupported)
        );
        assert!(matches!(
            request_bytes(&InvokeBody::Json(json!({ "bytes": [1, 2] }))),
            Err(QuickNoteImageError::Failed(_))
        ));
    }

    #[test]
    fn download_names_keep_the_type_extension() {
        let file = QuickNoteImageFile::new(digest(), "image/jpeg".into());
        assert_eq!(
            download_name(&file).unwrap(),
            ("image-abababababab.jpg".to_string(), "jpg".to_string())
        );
        let unsafe_name = QuickNoteImageFile::new("../escape".into(), "image/png".into());
        assert!(download_name(&unsafe_name).is_err());
    }
}

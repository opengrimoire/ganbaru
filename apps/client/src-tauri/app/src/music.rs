#[cfg(desktop)]
use base64::{Engine as _, engine::general_purpose};
#[cfg(desktop)]
use path_authority::{MusicPathKind, authorize, grant_path};
#[cfg(not(target_os = "ios"))]
use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
};
#[cfg(desktop)]
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(desktop)]
use tauri::Manager;
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, FilePath};

#[cfg(not(target_os = "ios"))]
pub(crate) mod library;
#[cfg(desktop)]
pub(crate) mod media_controls;
#[cfg(not(target_os = "ios"))]
pub(crate) mod media_server;
pub(crate) mod path_authority;
pub(crate) mod player;
#[cfg(not(target_os = "ios"))]
pub(crate) mod root_bindings;
#[cfg(not(target_os = "ios"))]
pub(crate) mod session;
#[cfg(desktop)]
pub(crate) mod soundscape;
#[cfg(desktop)]
pub(crate) mod soundscape_paths;
#[cfg(not(target_os = "ios"))]
pub(crate) mod youtube;

#[cfg(not(target_os = "ios"))]
pub(crate) use media_server::setup_youtube_host;

#[cfg(desktop)]
use ganbaru_music_library::artwork::{
    extract_embedded_artwork, find_track_artwork, sniff_image_content_type as artwork_content_type,
};
#[cfg(not(target_os = "ios"))]
use ganbaru_music_library::managed_artwork;
#[cfg(desktop)]
use ganbaru_music_library::media::{MEDIA_EXTENSIONS, is_supported_media_path};

#[cfg(not(target_os = "ios"))]
const MAX_MEDIA_FOLDER_FILES: usize = 5_000;
#[cfg(not(target_os = "ios"))]
const MAX_ARTWORK_BYTES: u64 = 12 * 1024 * 1024;
#[cfg(not(target_os = "ios"))]
const MAX_INTERCHANGE_BYTES: u64 = 8 * 1024 * 1024;
#[cfg(desktop)]
static MEDIA_FOLDER_SCAN_GENERATION: AtomicU64 = AtomicU64::new(0);
#[cfg(desktop)]
const SOUNDSCAPE_AUDIO_EXTENSIONS: &[&str] = &["flac", "m4a", "mp3", "mp4", "oga", "ogg", "wav"];

/// Reports whether a background audio path has an extension the soundscape engine accepts.
#[cfg(desktop)]
pub(crate) fn is_soundscape_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SOUNDSCAPE_AUDIO_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

#[cfg(desktop)]
fn replace_music_export_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "music export path has no parent".to_string())?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("music-export");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is invalid: {error}"))?
        .as_nanos();
    let temporary = parent.join(format!(".{file_name}.{nonce}.tmp"));
    let backup = parent.join(format!(".{file_name}.{nonce}.bak"));
    fs::write(&temporary, contents)
        .map_err(|error| format!("failed to write music export: {error}"))?;
    let had_existing = path.exists();
    if had_existing {
        if let Err(error) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("failed to prepare existing music export: {error}"));
        }
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("failed to finish music export: {error}"));
    }
    if had_existing {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFolderTrack {
    pub path: String,
    pub title: String,
    pub artwork_path: Option<String>,
}

#[cfg(not(target_os = "ios"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaFolderSelection {
    pub folder_path: String,
    pub display_name: Option<String>,
    pub tracks: Vec<MediaFolderTrack>,
    pub truncated: bool,
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_media_folder(
    app: tauri::AppHandle,
) -> Result<Option<MediaFolderSelection>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app.dialog().file().set_title("Select music folder");
        if let Some(directory) = music_folder_start_directory(&app) {
            picker = picker.set_directory(directory);
        }

        let Some(folder) = picker.blocking_pick_folder().map(dialog_path).transpose()? else {
            return Ok(None);
        };
        grant_path(&app, MusicPathKind::Folder, &folder)?;

        let generation = MEDIA_FOLDER_SCAN_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
        scan_media_folder_with_cancel(&folder, || {
            MEDIA_FOLDER_SCAN_GENERATION.load(Ordering::Acquire) != generation
        })
        .map(Some)
    })
    .await
    .map_err(|e| format!("media folder picker failed: {e}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_pick_media_folder(
    app: tauri::AppHandle,
) -> Result<Option<MediaFolderSelection>, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    app.mobile_media()
        .pick_media_tree(MAX_MEDIA_FOLDER_FILES as u32, 48)
        .await
        .map(|selection| {
            selection.map(|tree| MediaFolderSelection {
                folder_path: tree.tree_uri,
                display_name: Some(tree.display_name),
                tracks: tree
                    .tracks
                    .into_iter()
                    .map(|track| MediaFolderTrack {
                        path: track.uri,
                        title: track.title,
                        artwork_path: track.artwork_uri,
                    })
                    .collect(),
                truncated: tree.truncated,
            })
        })
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_detect_default_folder(
    app: tauri::AppHandle,
) -> Result<Option<MediaFolderSelection>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(folder) = music_folder_start_directory(&app) else {
            return Ok(None);
        };
        let selection = detect_non_empty_media_folder(&folder)?;
        // The operating system audio folder is offered as a source, so Rust grants it here.
        if selection.is_some() {
            grant_path(&app, MusicPathKind::Folder, &folder)?;
        }
        Ok(selection)
    })
    .await
    .map_err(|error| format!("default music folder scan failed: {error}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_detect_default_folder() -> Result<Option<MediaFolderSelection>, String> {
    Ok(None)
}

#[cfg(desktop)]
fn detect_non_empty_media_folder(folder: &Path) -> Result<Option<MediaFolderSelection>, String> {
    require_absolute_directory(folder)?;
    let mut queue = VecDeque::from([folder.to_path_buf()]);
    while let Some(directory) = queue.pop_front() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!(
                "failed to read media folder '{}': {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry =
                entry.map_err(|error| format!("failed to read media folder entry: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("failed to inspect '{}': {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                queue.push_back(path);
            } else if file_type.is_file() && is_supported_media_path(&path) {
                return Ok(Some(MediaFolderSelection {
                    folder_path: folder.to_string_lossy().into_owned(),
                    display_name: folder
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_string),
                    tracks: vec![MediaFolderTrack {
                        title: media_title_from_path(&path),
                        path: path.to_string_lossy().into_owned(),
                        artwork_path: None,
                    }],
                    truncated: true,
                }));
            }
        }
    }
    Ok(None)
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_root_binding_folder(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app.dialog().file().set_title("Map imported music folder");
        if let Some(directory) = music_folder_start_directory(&app) {
            picker = picker.set_directory(directory);
        }
        let Some(folder) = picker.blocking_pick_folder().map(dialog_path).transpose()? else {
            return Ok(None);
        };
        grant_path(&app, MusicPathKind::Folder, &folder)?;
        Ok(Some(folder.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|error| format!("music folder mapping picker failed: {error}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_pick_root_binding_folder(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    app.mobile_media()
        .pick_media_tree(1, 48)
        .await
        .map(|selection| selection.map(|tree| tree.tree_uri))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_media_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app
            .dialog()
            .file()
            .set_title("Select replacement media file")
            .add_filter("Supported media", MEDIA_EXTENSIONS);
        if let Some(directory) = music_folder_start_directory(&app) {
            picker = picker.set_directory(directory);
        }
        pick_granted_file(&app, picker)
    })
    .await
    .map_err(|error| format!("media file picker failed: {error}"))??;
    Ok(selected.map(|path| path.to_string_lossy().into_owned()))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_soundscape_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        let mut picker = app
            .dialog()
            .file()
            .set_title("Select background audio loop")
            .add_filter("Supported audio", SOUNDSCAPE_AUDIO_EXTENSIONS);
        if let Some(directory) = music_folder_start_directory(&app) {
            picker = picker.set_directory(directory);
        }
        pick_granted_file(&app, picker)
    })
    .await
    .map_err(|error| format!("background audio picker failed: {error}"))??;
    Ok(selected.map(|path| path.to_string_lossy().into_owned()))
}

/// Lets the user pick an image and copies it into the active vault as managed artwork.
/// Returns the managed reference an artwork override stores, or `None` when canceled.
#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_artwork_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Select playlist artwork")
            .add_filter("Images", &managed_artwork::ARTWORK_EXTENSIONS)
            .blocking_pick_file()
            .map(dialog_path)
            .transpose()?
        else {
            return Ok(None);
        };
        let bytes = read_bounded_artwork(&path, u64::from(managed_artwork::MAX_ARTWORK_BYTES))?;
        import_managed_artwork(&app, &bytes).map(Some)
    })
    .await
    .map_err(|error| format!("artwork file picker failed: {error}"))?
}

/// Lets the user pick an image and copies it into the active vault as managed artwork.
/// Returns the managed reference an artwork override stores, or `None` when canceled.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_pick_artwork_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    let Some(uri) = app.mobile_media().pick_artwork_file().await? else {
        return Ok(None);
    };
    let data_url = app
        .mobile_media()
        .artwork_data_url(&uri, false, u64::from(managed_artwork::MAX_ARTWORK_BYTES))
        .await?
        .ok_or_else(|| "selected artwork is unavailable".to_string())?;
    let bytes = decode_image_data_url(&data_url)?;
    tauri::async_runtime::spawn_blocking(move || import_managed_artwork(&app, &bytes).map(Some))
        .await
        .map_err(|error| format!("artwork import task failed: {error}"))?
}

/// Decodes the base64 payload of an image data URL.
#[cfg(target_os = "android")]
fn decode_image_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    use base64::{Engine as _, engine::general_purpose};

    let (header, payload) = data_url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(|| "selected artwork is not an image".to_string())?;
    if !header.ends_with(";base64") {
        return Err("selected artwork is not an image".to_string());
    }
    general_purpose::STANDARD
        .decode(payload)
        .map_err(|error| format!("selected artwork could not be decoded: {error}"))
}

/// Copies verified image bytes into the Music artwork class of the active vault and returns
/// the managed reference.
#[cfg(not(target_os = "ios"))]
fn import_managed_artwork(app: &tauri::AppHandle, bytes: &[u8]) -> Result<String, String> {
    let vault_root = crate::vault::active_replicated_asset_path(app, managed_artwork::DOMAIN_NAME)?;
    managed_artwork::import_artwork(&vault_root, bytes)
        .map(|artwork| artwork.reference)
        .map_err(|error| error.to_string())
}

/// Reads managed artwork of the active vault as a data URL after checking that the file
/// matches the digest and type its reference names.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
pub async fn music_artwork_asset_data_url(
    app: tauri::AppHandle,
    reference: String,
) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    let vault_root = crate::vault::active_vault_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let artwork = managed_artwork::read_artwork(&vault_root, &reference)
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "data:{};base64,{}",
            artwork.content_type,
            general_purpose::STANDARD.encode(artwork.bytes)
        ))
    })
    .await
    .map_err(|error| format!("artwork loading task failed: {error}"))?
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_pick_and_read_interchange_file(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Import music playlists")
            .add_filter("Music playlists", &["json", "m3u8", "m3u"])
            .blocking_pick_file()
            .map(dialog_path)
            .transpose()?
        else {
            return Ok(None);
        };
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase);
        if !matches!(extension.as_deref(), Some("json" | "m3u8" | "m3u")) {
            return Err("music import must use .json, .m3u8, or .m3u".to_string());
        }
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("failed to inspect music import: {error}"))?;
        if metadata.len() > MAX_INTERCHANGE_BYTES {
            return Err("music import exceeds the 8 MB safety limit".to_string());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .and_then(|file| file.take(MAX_INTERCHANGE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|error| format!("failed to read music import: {error}"))?;
        if bytes.len() as u64 > MAX_INTERCHANGE_BYTES {
            return Err("music import exceeds the 8 MB safety limit".to_string());
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|error| format!("failed to read UTF-8 music import: {error}"))
    })
    .await
    .map_err(|error| format!("music import picker failed: {error}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_pick_and_read_interchange_file(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    use ganbaru_mobile_documents::MobileDocumentsExt;

    app.mobile_documents().pick_utf8_document_matching(
        MAX_INTERCHANGE_BYTES,
        &["json", "m3u8", "m3u"],
        &[
            "application/json",
            "application/vnd.apple.mpegurl",
            "audio/x-mpegurl",
            "text/plain",
        ],
        "music playlist",
    )
}

#[cfg(desktop)]
/// Writes a completed native Music snapshot after all SQLite reads have finished.
pub async fn music_pick_and_write_interchange_file(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
    format: String,
) -> Result<bool, String> {
    if contents.len() as u64 > MAX_INTERCHANGE_BYTES {
        return Err("music export exceeds the 8 MB safety limit".to_string());
    }
    let (extension, label) = match format.as_str() {
        "json" => ("json", "Ganbaru music JSON"),
        "m3u8" => ("m3u8", "UTF-8 M3U playlist"),
        _ => return Err("unsupported music export format".to_string()),
    };
    let file_name = music_interchange_file_name(&default_name, extension)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Export music playlists")
            .set_file_name(file_name)
            .add_filter(label, &[extension])
            .blocking_save_file()
            .map(dialog_path)
            .transpose()?
        else {
            return Ok(false);
        };
        if path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
            != Some(extension)
        {
            return Err(format!("music export path must end in .{extension}"));
        }
        replace_music_export_file(&path, contents.as_bytes())?;
        Ok(true)
    })
    .await
    .map_err(|error| format!("music export picker failed: {error}"))?
}

#[cfg(target_os = "android")]
/// Exports a completed native Music snapshot through the bounded Android adapter.
pub async fn music_pick_and_write_interchange_file(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
    format: String,
) -> Result<bool, String> {
    use ganbaru_mobile_documents::MobileDocumentsExt;

    if contents.len() as u64 > MAX_INTERCHANGE_BYTES {
        return Err("music export exceeds the 8 MB safety limit".to_string());
    }
    let (extension, mime_type) = match format.as_str() {
        "json" => ("json", "application/json"),
        "m3u8" => ("m3u8", "application/vnd.apple.mpegurl"),
        _ => return Err("unsupported music export format".to_string()),
    };
    let file_name = music_interchange_file_name(&default_name, extension)?;
    app.mobile_documents().save_utf8_download_with_type(
        &file_name,
        &contents,
        MAX_INTERCHANGE_BYTES,
        &[extension],
        mime_type,
        "music playlist",
    )?;
    Ok(true)
}

#[cfg(not(target_os = "ios"))]
fn music_interchange_file_name(default_name: &str, extension: &str) -> Result<String, String> {
    let safe_stem = default_name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let safe_stem = safe_stem.trim().trim_end_matches('.');
    if safe_stem.is_empty() {
        return Err("music export file name is required".to_string());
    }
    Ok(format!("{safe_stem}.{extension}"))
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_artwork_data_url(app: tauri::AppHandle, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = authorize(&app, &path, MusicPathKind::File)?;
        let bytes = read_bounded_artwork(&path, MAX_ARTWORK_BYTES)?;
        let content_type = artwork_content_type(&bytes)
            .ok_or_else(|| "selected artwork is not a supported image".to_string())?;
        Ok(format!(
            "data:{content_type};base64,{}",
            general_purpose::STANDARD.encode(bytes)
        ))
    })
    .await
    .map_err(|error| format!("artwork loading task failed: {error}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_artwork_data_url(app: tauri::AppHandle, path: String) -> Result<String, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    app.mobile_media()
        .artwork_data_url(&path, false, MAX_ARTWORK_BYTES)
        .await?
        .ok_or_else(|| "selected artwork is unavailable".to_string())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_embedded_artwork_data_url(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = authorize(&app, &path, MusicPathKind::File)?;
        let Some(artwork) = extract_embedded_artwork(&path)? else {
            return Ok(None);
        };
        if artwork.bytes.len() as u64 > MAX_ARTWORK_BYTES {
            return Err("embedded artwork exceeds the 12 MB display limit".to_string());
        }
        Ok(Some(format!(
            "data:{};base64,{}",
            artwork.content_type,
            general_purpose::STANDARD.encode(artwork.bytes)
        )))
    })
    .await
    .map_err(|error| format!("embedded artwork loading task failed: {error}"))?
}

#[cfg(target_os = "android")]
#[tauri::command]
pub async fn music_embedded_artwork_data_url(
    app: tauri::AppHandle,
    path: String,
) -> Result<Option<String>, String> {
    use ganbaru_mobile_media::MobileMediaExt;

    app.mobile_media()
        .artwork_data_url(&path, true, MAX_ARTWORK_BYTES)
        .await
}

/// Reads at most `limit` bytes from one open handle, so a file that grows or is swapped after
/// authorization cannot force an unbounded read.
#[cfg(desktop)]
fn read_bounded_artwork(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|error| format!("failed to read artwork: {error}"))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read artwork: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "artwork file exceeds the {} MB display limit",
            limit / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

#[cfg(desktop)]
fn music_folder_start_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    existing_music_start_directory(app.path().audio_dir().ok())
}

#[cfg(desktop)]
fn existing_music_start_directory(candidate: Option<PathBuf>) -> Option<PathBuf> {
    candidate.filter(|path| path.is_dir())
}

#[cfg(desktop)]
#[tauri::command]
pub async fn music_reveal_local_file(app: tauri::AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = authorize(&app, &path, MusicPathKind::File)?;
        reveal_local_file(&path)
    })
    .await
    .map_err(|error| format!("media file location task failed: {error}"))?
}

#[cfg(test)]
fn scan_media_folder(folder: &Path) -> Result<MediaFolderSelection, String> {
    scan_media_folder_with_cancel(folder, || false)
}

#[cfg(desktop)]
fn scan_media_folder_with_cancel(
    folder: &Path,
    is_cancelled: impl Fn() -> bool,
) -> Result<MediaFolderSelection, String> {
    require_absolute_directory(folder)?;
    let mut queue = VecDeque::from([folder.to_path_buf()]);
    let mut tracks = Vec::new();
    let mut truncated = false;
    let mut artwork_cache: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();

    while let Some(directory) = queue.pop_front() {
        if is_cancelled() {
            truncated = true;
            break;
        }
        let mut entries = fs::read_dir(&directory)
            .map_err(|e| format!("failed to read media folder '{}': {e}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("failed to read media folder entry: {e}"))?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            if is_cancelled() {
                truncated = true;
                break;
            }
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| format!("failed to inspect '{}': {e}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                queue.push_back(path);
                continue;
            }
            if !file_type.is_file() || !is_supported_media_path(&path) {
                continue;
            }
            let artwork_path = find_track_artwork(&path, folder, &mut artwork_cache);
            tracks.push(MediaFolderTrack {
                title: media_title_from_path(&path),
                path: path.to_string_lossy().to_string(),
                artwork_path: artwork_path.map(|path| path.to_string_lossy().to_string()),
            });
            if tracks.len() >= MAX_MEDIA_FOLDER_FILES {
                truncated = true;
                break;
            }
        }
        if truncated {
            break;
        }
    }

    tracks.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(MediaFolderSelection {
        folder_path: folder.to_string_lossy().to_string(),
        display_name: folder
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
        tracks,
        truncated,
    })
}

/// Shows a native file picker and grants the chosen file on this device.
#[cfg(desktop)]
fn pick_granted_file(
    app: &tauri::AppHandle,
    picker: tauri_plugin_dialog::FileDialogBuilder<tauri::Wry>,
) -> Result<Option<PathBuf>, String> {
    let Some(file) = picker.blocking_pick_file().map(dialog_path).transpose()? else {
        return Ok(None);
    };
    grant_path(app, MusicPathKind::File, &file)?;
    Ok(Some(file))
}

#[cfg(desktop)]
fn dialog_path(path: FilePath) -> Result<PathBuf, String> {
    path.into_path()
        .map_err(|e| format!("selected path is not a local folder: {e}"))
}

#[cfg(desktop)]
fn require_absolute_directory(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("media folder path must be absolute".to_string());
    }
    let metadata =
        fs::metadata(path).map_err(|e| format!("failed to inspect media folder: {e}"))?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err("selected media path must be a folder".to_string())
    }
}

#[cfg(target_os = "linux")]
fn reveal_local_file(path: &Path) -> Result<(), String> {
    let folder = path
        .parent()
        .ok_or_else(|| "media file has no containing folder".to_string())?;
    spawn_file_manager_command("xdg-open", [folder.as_os_str()])
}

#[cfg(target_os = "macos")]
fn reveal_local_file(path: &Path) -> Result<(), String> {
    spawn_file_manager_command("open", [std::ffi::OsStr::new("-R"), path.as_os_str()])
}

/// Selects the file through the shell item ID API, which takes the path as data rather than as
/// part of an `explorer.exe` command line, where commas and quotes act as separators.
#[cfg(windows)]
fn reveal_local_file(path: &Path) -> Result<(), String> {
    tauri_plugin_opener::reveal_item_in_dir(path)
        .map_err(|error| format!("open media file location: {error}"))
}

#[cfg(all(desktop, not(any(target_os = "linux", target_os = "macos", windows))))]
fn reveal_local_file(_path: &Path) -> Result<(), String> {
    Err("opening media file locations is not implemented for this platform".to_string())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn spawn_file_manager_command<I, S>(program: &str, args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("open media file location: {e}"))
}

#[cfg(desktop)]
fn media_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Untitled media")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::media_server::{ByteRange, media_content_type, parse_byte_range};
    use super::youtube::player_page::youtube_host_html;
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Returns the source of the first function named `name`, from its header line to its
    /// closing brace at column zero.
    fn function_source<'a>(source: &'a str, name: &str) -> &'a str {
        let header = format!("fn {name}(");
        let found = source
            .find(&header)
            .unwrap_or_else(|| panic!("{name} is defined"));
        let start = source[..found].rfind('\n').map_or(0, |index| index + 1);
        let end = source[start..]
            .find("\n}\n")
            .map_or(source.len(), |index| start + index + 3);
        &source[start..end]
    }

    #[test]
    fn music_file_commands_inspect_media_off_the_main_thread() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/music");
        for (file, command) in [
            ("media_server.rs", "music_register_media_file"),
            ("media_server.rs", "music_register_embedded_artwork"),
            ("media_server.rs", "music_register_artwork_asset"),
            ("player.rs", "media_player_probe"),
        ] {
            let source =
                std::fs::read_to_string(root.join(file)).expect("read Music command source");
            let function = function_source(&source, command);
            assert!(
                function
                    .lines()
                    .next()
                    .is_some_and(|line| line.contains("async fn")),
                "{command} must be async so Tauri does not run it on the main thread"
            );
            assert!(
                function.contains("spawn_blocking("),
                "{command} must move file and parser work to a blocking thread"
            );
        }
    }

    #[test]
    fn youtube_host_keeps_its_token_from_youtube_and_targets_the_app_origin() {
        let host = youtube_host_html();

        assert!(host.contains("widget_referrer: location.origin"));
        assert!(!host.contains("location.href"));
        assert!(host.contains("parent.postMessage(payload, parentOrigin)"));
        assert!(!host.contains("parent.postMessage(payload, \"*\")"));
        assert!(host.contains("location.ancestorOrigins"));
    }

    #[test]
    fn youtube_host_uses_supported_minimal_chrome_parameters() {
        let host = youtube_host_html();

        assert!(host.contains("controls: 0"));
        assert!(host.contains("disablekb: 1"));
        assert!(host.contains("fs: 0"));
        assert!(host.contains("iv_load_policy: 3"));
        assert!(host.contains("rel: 0"));
        assert!(host.contains("function videoMetadata"));
        assert!(host.contains("data.title"));
        assert!(host.contains("function applyVolume"));
        assert!(host.contains("player.unMute()"));
        assert!(host.contains("function resetPlayerElement"));
        assert!(host.contains("root.replaceChildren()"));
        assert!(host.contains("function initialPayloadFromParams"));
        assert!(host.contains("params.get(\"sourceKind\")"));
        assert!(host.contains("params.get(\"videoId\")"));
        assert!(host.contains("params.get(\"playlistId\")"));
        assert!(host.contains("const initialLoad = initialPayloadFromParams()"));
        assert!(host.contains("load: loadId"));
        assert!(host.contains("ganbaru-youtube-playlist-error"));
        assert!(host.contains("event.source !== parent"));
        assert!(host.contains("activeSource.kind !== \"youtube-playlist\""));
        assert!(host.contains("playbackActive = event.data === 1"));
        assert!(host.contains("if (player && playbackActive) snapshot()"));
        assert!(host.contains("if (source.kind === \"youtube-video\" || source.videoId)"));
        assert!(!host.contains("videoId: source.kind"));
        assert!(!host.contains("modestbranding"));
        assert!(!host.contains("showinfo"));
        assert!(!host.contains("autohide"));
        assert!(!host.contains("theme"));
    }

    #[test]
    fn byte_range_parser_accepts_open_and_suffix_ranges() {
        assert_eq!(
            parse_byte_range("bytes=10-", 100),
            Some(ByteRange { start: 10, end: 99 })
        );
        assert_eq!(
            parse_byte_range("bytes=10-20", 100),
            Some(ByteRange { start: 10, end: 20 })
        );
        assert_eq!(
            parse_byte_range("bytes=-25", 100),
            Some(ByteRange { start: 75, end: 99 })
        );
    }

    #[test]
    fn byte_range_parser_rejects_unsatisfiable_ranges() {
        assert_eq!(parse_byte_range("bytes=100-200", 100), None);
        assert_eq!(parse_byte_range("bytes=20-10", 100), None);
        assert_eq!(parse_byte_range("items=0-10", 100), None);
    }

    #[test]
    fn media_content_type_maps_common_audio_formats() {
        assert_eq!(
            media_content_type(Path::new("/music/focus.mp3")),
            "audio/mpeg"
        );
        assert_eq!(
            media_content_type(Path::new("/music/focus.flac")),
            "audio/flac"
        );
    }

    #[test]
    fn media_title_omits_file_extension() {
        assert_eq!(
            media_title_from_path(Path::new("/music/focus.flac")),
            "focus"
        );
    }

    #[test]
    fn music_folder_start_directory_uses_only_existing_directories() {
        let root = unique_temporary_dir("ganbaru-music-start-dir");
        let music_dir = root.join("localized-audio");
        let file_path = root.join("not-a-directory");
        fs::create_dir_all(&music_dir).unwrap();
        fs::write(&file_path, []).unwrap();

        assert_eq!(
            existing_music_start_directory(Some(music_dir.clone())),
            Some(music_dir)
        );
        assert_eq!(existing_music_start_directory(Some(file_path)), None);
        assert_eq!(existing_music_start_directory(None), None);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn automatic_music_folder_detection_stops_after_the_first_supported_file() {
        let root = unique_temporary_dir("ganbaru-music-detection");
        fs::create_dir_all(root.join("album")).unwrap();
        fs::write(root.join("notes.txt"), []).unwrap();
        assert!(detect_non_empty_media_folder(&root).unwrap().is_none());
        fs::write(root.join("album/track.flac"), []).unwrap();
        fs::write(root.join("album/second.mp3"), []).unwrap();

        let detected = detect_non_empty_media_folder(&root).unwrap().unwrap();
        assert_eq!(detected.tracks.len(), 1);
        assert!(detected.truncated);
        assert!(detected.tracks[0].artwork_path.is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn media_folder_scan_returns_deterministic_partial_results_on_cancellation() {
        use std::cell::Cell;

        let root = unique_temporary_dir("ganbaru-music-cancel");
        fs::create_dir_all(&root).unwrap();
        for name in ["c.mp3", "a.mp3", "b.mp3"] {
            fs::write(root.join(name), []).unwrap();
        }
        let checks = Cell::new(0usize);
        let result = scan_media_folder_with_cancel(&root, || {
            let next = checks.get() + 1;
            checks.set(next);
            next > 3
        })
        .unwrap();

        assert!(result.truncated);
        assert_eq!(result.tracks.len(), 2);
        assert!(result.tracks[0].path.ends_with("a.mp3"));
        assert!(result.tracks[1].path.ends_with("b.mp3"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn media_folder_scan_handles_deep_trees_without_recursion() {
        let root = unique_temporary_dir("ganbaru-music-deep");
        let mut directory = root.clone();
        for index in 0..128 {
            directory = directory.join(format!("d{index}"));
            fs::create_dir_all(&directory).unwrap();
        }
        fs::write(directory.join("deep.mp3"), []).unwrap();

        let result = scan_media_folder(&root).unwrap();
        assert_eq!(result.tracks.len(), 1);
        assert!(!result.truncated);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn media_folder_scan_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;

        let root = unique_temporary_dir("ganbaru-music-symlink");
        let outside = unique_temporary_dir("ganbaru-music-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("outside.mp3"), []).unwrap();
        symlink(&outside, root.join("linked")).unwrap();

        let result = scan_media_folder(&root).unwrap();
        assert!(result.tracks.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn media_folder_scan_stops_at_five_thousand_tracks() {
        let root = unique_temporary_dir("ganbaru-music-cap");
        fs::create_dir_all(&root).unwrap();
        for index in 0..=MAX_MEDIA_FOLDER_FILES {
            fs::write(root.join(format!("{index:05}.mp3")), []).unwrap();
        }

        let result = scan_media_folder(&root).unwrap();
        assert_eq!(result.tracks.len(), MAX_MEDIA_FOLDER_FILES);
        assert!(result.truncated);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn media_content_type_maps_common_image_formats() {
        assert_eq!(
            media_content_type(Path::new("/music/cover.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            media_content_type(Path::new("/music/folder.webp")),
            "image/webp"
        );
    }

    #[test]
    fn artwork_data_url_sniffing_rejects_extension_only_files() {
        assert_eq!(
            artwork_content_type(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            artwork_content_type(&[0xff, 0xd8, 0xff, 0xdb]),
            Some("image/jpeg")
        );
        assert_eq!(artwork_content_type(b"not an image"), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_reveal_resolves_the_path_as_data_instead_of_a_command_line() {
        let missing = unique_temporary_dir("ganbaru-reveal").join(r#"a,b",c.mp3"#);
        assert!(reveal_local_file(&missing).is_err());
    }

    #[test]
    fn artwork_data_url_reads_at_most_the_limit_from_one_handle() {
        let directory = unique_temporary_dir("ganbaru-artwork-bound");
        fs::create_dir_all(&directory).unwrap();
        let fits = directory.join("fits.png");
        let grown = directory.join("grown.png");
        fs::write(&fits, [7_u8; 16]).unwrap();
        fs::write(&grown, [7_u8; 17]).unwrap();

        assert_eq!(read_bounded_artwork(&fits, 16).unwrap().len(), 16);
        assert!(read_bounded_artwork(&grown, 16).is_err());
        #[cfg(unix)]
        {
            let endless = directory.join("endless.png");
            std::os::unix::fs::symlink("/dev/zero", &endless).unwrap();
            assert!(read_bounded_artwork(&endless, 16).is_err());
        }
        fs::remove_dir_all(&directory).unwrap();
    }

    fn unique_temporary_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{name}-{nanos}"))
    }
}

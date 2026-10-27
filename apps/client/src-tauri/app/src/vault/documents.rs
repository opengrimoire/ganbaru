//! Native Calendar and theme document transfer, independent of vault selection.

#[cfg(desktop)]
use super::{FileAccess, dialog_path, require_absolute_path, write_text_file_atomically};
#[cfg(desktop)]
use crate::native_dialogs::on_dialog_thread;
#[cfg(target_os = "android")]
use ganbaru_mobile_documents::MobileDocumentsExt;
#[cfg(desktop)]
use std::fs;
#[cfg(not(target_os = "android"))]
use std::io::Read;
#[cfg(target_os = "ios")]
use std::io::Write;
use std::path::Path;
#[cfg(desktop)]
use std::path::PathBuf;
#[cfg(desktop)]
use tauri::Manager;
#[cfg(target_os = "ios")]
use tauri::Runtime;
#[cfg(not(target_os = "android"))]
use tauri_plugin_dialog::DialogExt;
#[cfg(target_os = "ios")]
use tauri_plugin_dialog::FilePath;
#[cfg(target_os = "ios")]
use tauri_plugin_fs::{FsExt, OpenOptions};

#[cfg(desktop)]
fn require_extension(path: &Path, allowed: &[&str], label: &str) -> Result<(), String> {
    require_absolute_path(path)?;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if allowed
        .iter()
        .any(|allowed_ext| ext.eq_ignore_ascii_case(allowed_ext))
    {
        Ok(())
    } else {
        Err(format!(
            "{label} file must use one of: {}",
            allowed.join(", ")
        ))
    }
}

fn default_file_name(input: &str, fallback_stem: &str, extension: &str) -> String {
    let trimmed = input.trim();
    let from_input = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback_stem);
    if Path::new(from_input)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
    {
        from_input.to_string()
    } else {
        format!("{from_input}.{extension}")
    }
}

#[cfg(not(target_os = "android"))]
fn read_utf8_capped(reader: &mut impl Read, max_bytes: u64, label: &str) -> Result<String, String> {
    let mut contents = String::new();
    let mut limited = reader.take(max_bytes + 1);
    limited
        .read_to_string(&mut contents)
        .map_err(|e| format!("failed to read {label} as UTF-8: {e}"))?;
    if contents.len() as u64 > max_bytes {
        return Err(format!("{label} exceeds the limit of {max_bytes} bytes"));
    }
    Ok(contents)
}

fn require_text_within_limit(contents: &str, max_bytes: u64, label: &str) -> Result<(), String> {
    let byte_count = contents.len() as u64;
    if byte_count > max_bytes {
        return Err(format!(
            "{label} is {byte_count} bytes, exceeding the limit of {max_bytes} bytes"
        ));
    }
    Ok(())
}

#[cfg(desktop)]
fn read_text_file_capped(path: &Path, max_bytes: u64, label: &str) -> Result<String, String> {
    require_absolute_path(path)?;
    let metadata = fs::metadata(path).map_err(|e| format!("failed to inspect {label}: {e}"))?;
    if metadata.len() > max_bytes {
        return Err(format!(
            "{label} is {} bytes, exceeding the limit of {max_bytes} bytes",
            metadata.len()
        ));
    }

    let mut file = fs::File::open(path).map_err(|e| format!("failed to open {label}: {e}"))?;
    read_utf8_capped(&mut file, max_bytes, label)
}

#[cfg(desktop)]
fn pick_open_path(
    app: &tauri::AppHandle,
    title: &str,
    filter_name: &str,
    extensions: &[&str],
    start_directory: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let mut picker = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter(filter_name, extensions);
    if let Some(directory) = start_directory {
        picker = picker.set_directory(directory);
    }
    picker.blocking_pick_file().map(dialog_path).transpose()
}

#[cfg(desktop)]
pub(crate) fn pick_save_path(
    app: &tauri::AppHandle,
    title: &str,
    default_name: &str,
    filter_name: &str,
    extensions: &[&str],
    start_directory: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    let mut picker = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(default_name)
        .add_filter(filter_name, extensions);
    if let Some(directory) = start_directory {
        picker = picker.set_directory(directory);
    }
    picker.blocking_save_file().map(dialog_path).transpose()
}

#[cfg(desktop)]
pub(crate) fn existing_downloads_directory(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().download_dir().ok().filter(|path| path.is_dir())
}

/// Size and count limits for one ICS zip import.
#[cfg(desktop)]
#[derive(Clone, Copy, Debug)]
struct IcsZipLimits {
    /// Entries inspected inside one zip, far above the one `.ics` per calendar
    /// that Google and Apple exports contain.
    max_entries: usize,
    /// Decompressed bytes of one entry, enough for a multi-decade calendar with
    /// thousands of events (text-only iCalendar averages about 1 KiB per event).
    max_entry_bytes: u64,
    /// Decompressed bytes across all entries, counted from bytes actually read,
    /// and sized for what the import comfortably sends over IPC.
    max_total_bytes: u64,
}

#[cfg(desktop)]
const ICS_ZIP_LIMITS: IcsZipLimits = IcsZipLimits {
    max_entries: 1024,
    max_entry_bytes: 25 * 1024 * 1024,
    max_total_bytes: 64 * 1024 * 1024,
};

/// Plain `.ics` imports share the zip per-entry cap so the import flow has
/// one clear maximum payload size regardless of container.
const ICS_PLAIN_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Theme JSON is small configuration data. One MiB leaves room for custom
/// comments and future tokens while rejecting accidental large-file picks.
const THEME_JSON_MAX_BYTES: u64 = 1024 * 1024;

/// A recovery kit text file holds one code and a short localized explanation.
#[cfg(desktop)]
const RECOVERY_KIT_TEXT_MAX_BYTES: u64 = 16 * 1024;

#[cfg(target_os = "ios")]
const THEME_JSON_DOCUMENT_FILTERS: &[&str] = &["json"];

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeJsonWriteOutcome {
    saved: bool,
    destination: Option<&'static str>,
    file_name: Option<String>,
}

impl ThemeJsonWriteOutcome {
    #[cfg(not(target_os = "android"))]
    fn cancelled() -> Self {
        Self {
            saved: false,
            destination: None,
            file_name: None,
        }
    }

    #[cfg(not(target_os = "android"))]
    fn saved_to_selected_file() -> Self {
        Self {
            saved: true,
            destination: None,
            file_name: None,
        }
    }

    #[cfg(target_os = "android")]
    fn saved_to_downloads(file_name: String) -> Self {
        Self {
            saved: true,
            destination: Some("downloads"),
            file_name: Some(file_name),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct IcsZipEntry {
    /// File-name-only basename (no directory components) so an entry path
    /// like `personal/work.ics` is exposed as `work.ics` to the frontend.
    pub name: String,
    /// UTF-8-decoded entry contents. RFC 5545 mandates UTF-8, so a decode
    /// failure is reported as an error rather than silently lossy-replaced.
    pub contents: String,
}

/// Read every `.ics` entry inside the zip at `path` and return their decoded
/// contents. Used by the calendar import flow so a Google or Apple export
/// (which always arrives as a `.ics.zip` bundle) can be imported in one
/// step instead of forcing the user to unzip first.
///
/// Safety contract:
///
/// - `path` comes from a native picker, never from IPC, and must have a
///   `.zip` extension.
/// - Entry paths are validated through `enclosed_name`, which rejects any
///   entry that tries to escape (zip-slip via `..` or absolute paths).
/// - Encrypted entries are refused; only the deflate feature is enabled.
/// - Declared sizes are an early reject only. The zip reader does not enforce
///   them, so every read goes through a `Take` adapter bounded by the per-entry
///   cap and the bytes left in the aggregate budget, counted from real bytes.
/// - Only entries whose extension matches `.ics` are returned. Anything
///   else (`__MACOSX/`, `.DS_Store`, signature files, archived metadata)
///   is silently skipped so the importer never sees them.
#[cfg(desktop)]
fn read_ics_zip_entries_from_path(path: &Path) -> Result<Vec<IcsZipEntry>, String> {
    read_ics_zip_entries_with_limits(path, ICS_ZIP_LIMITS)
}

#[cfg(desktop)]
fn read_ics_zip_entries_with_limits(
    path: &Path,
    limits: IcsZipLimits,
) -> Result<Vec<IcsZipEntry>, String> {
    require_extension(path, &["zip"], "ICS zip import")?;

    let file = fs::File::open(path).map_err(|e| format!("failed to open zip: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip archive: {e}"))?;

    if archive.len() > limits.max_entries {
        return Err(format!(
            "zip has {} entries, exceeding the limit of {}",
            archive.len(),
            limits.max_entries
        ));
    }

    let mut entries: Vec<IcsZipEntry> = Vec::new();
    let mut total_declared: u64 = 0;
    let mut total_read: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("failed to read zip entry {i}: {e}"))?;

        if entry.is_dir() {
            continue;
        }

        // `enclosed_name` returns `None` for any entry whose path escapes
        // the archive root (zip-slip protection). Treat that as fatal so a
        // tampered bundle is never silently imported.
        let enclosed = match entry.enclosed_name() {
            Some(name) => name,
            None => {
                return Err("zip contains an entry with an unsafe path".to_string());
            }
        };

        let ext_is_ics = enclosed
            .extension()
            .map(|e| e.eq_ignore_ascii_case("ics"))
            .unwrap_or(false);
        if !ext_is_ics {
            continue;
        }

        if entry.encrypted() {
            return Err(format!(
                "zip entry '{}' is encrypted; encrypted .ics imports are not supported",
                enclosed.display()
            ));
        }

        let reported_size = entry.size();
        if reported_size > limits.max_entry_bytes {
            return Err(format!(
                "zip entry '{}' uncompressed size ({reported_size} bytes) exceeds the per-entry limit of {} bytes",
                enclosed.display(),
                limits.max_entry_bytes
            ));
        }
        total_declared = total_declared.saturating_add(reported_size);
        if total_declared > limits.max_total_bytes || total_read >= limits.max_total_bytes {
            return Err(aggregate_limit_error(limits));
        }

        let display_name = enclosed
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("entry-{i}.ics"));

        // Capture only the basename for the frontend before borrowing the
        // entry mutably for the read.
        drop(enclosed);

        // Header sizes can lie, so the read stops one byte past whichever
        // budget is smaller and real byte counts decide which limit failed.
        let remaining_total = limits.max_total_bytes - total_read;
        let read_limit = limits.max_entry_bytes.min(remaining_total);
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(read_limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("failed to read zip entry '{display_name}': {e}"))?;
        let read = bytes.len() as u64;
        if read > limits.max_entry_bytes {
            return Err(format!(
                "zip entry '{display_name}' exceeds the per-entry size limit during decompression"
            ));
        }
        if read > remaining_total {
            return Err(aggregate_limit_error(limits));
        }
        total_read += read;
        let contents = String::from_utf8(bytes)
            .map_err(|e| format!("failed to read zip entry '{display_name}' as UTF-8: {e}"))?;

        entries.push(IcsZipEntry {
            name: display_name,
            contents,
        });
    }

    Ok(entries)
}

#[cfg(desktop)]
fn aggregate_limit_error(limits: IcsZipLimits) -> String {
    format!(
        "zip total uncompressed size exceeds the limit of {} bytes",
        limits.max_total_bytes
    )
}

#[cfg(desktop)]
fn read_plain_ics_entry_from_path(path: &Path) -> Result<IcsZipEntry, String> {
    require_extension(path, &["ics"], "ICS import")?;
    let contents = read_text_file_capped(path, ICS_PLAIN_MAX_BYTES, "ICS import")?;
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "calendar.ics".to_string());
    Ok(IcsZipEntry { name, contents })
}

/// Open a native file picker and read one `.ics` file or every `.ics` entry
/// inside one `.zip` bundle. The selected path never crosses the IPC
/// boundary, and Rust re-validates the extension before reading.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_and_read_ics_import(
    app: tauri::AppHandle,
) -> Result<Option<Vec<IcsZipEntry>>, String> {
    let start_directory = existing_downloads_directory(&app);
    let Some(path) = on_dialog_thread(move || {
        pick_open_path(
            &app,
            "Import calendar",
            "iCalendar (.ics or .zip)",
            &["ics", "zip"],
            start_directory,
        )
    })
    .await??
    else {
        return Ok(None);
    };

    require_extension(&path, &["ics", "zip"], "ICS import")?;
    let is_zip = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"));
    if is_zip {
        read_ics_zip_entries_from_path(&path).map(Some)
    } else {
        read_plain_ics_entry_from_path(&path).map(|entry| Some(vec![entry]))
    }
}

/// Open a native save dialog and write a calendar `.ics` export. The
/// selected path stays in Rust and must still have a `.ics` extension.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_and_write_ics_export(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<bool, String> {
    require_text_within_limit(&contents, ICS_PLAIN_MAX_BYTES, "ICS export")?;
    let default_name = default_file_name(&default_name, "calendar", "ics");
    let Some(path) = on_dialog_thread(move || {
        pick_save_path(
            &app,
            "Export calendar",
            &default_name,
            "iCalendar",
            &["ics"],
            None,
        )
    })
    .await??
    else {
        return Ok(false);
    };
    require_extension(&path, &["ics"], "ICS export")?;
    write_text_file_atomically(&path, &contents, FileAccess::Default)?;
    Ok(true)
}

/// Ask Android to select and read one bounded iCalendar document.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_pick_and_read_ics_import(
    app: tauri::AppHandle,
) -> Result<Option<Vec<IcsZipEntry>>, String> {
    app.mobile_documents()
        .pick_utf8_document_matching(
            ICS_PLAIN_MAX_BYTES,
            &["ics"],
            &["text/calendar", "application/ics", "text/plain"],
            "calendar",
        )
        .map(|selected| {
            selected.map(|contents| {
                vec![IcsZipEntry {
                    name: "calendar.ics".to_string(),
                    contents,
                }]
            })
        })
}

/// Save an iCalendar export to Android's public Downloads collection.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_pick_and_write_ics_export(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<bool, String> {
    let default_name = default_file_name(&default_name, "calendar", "ics");
    app.mobile_documents().save_utf8_download_with_type(
        &default_name,
        &contents,
        ICS_PLAIN_MAX_BYTES,
        &["ics"],
        "text/calendar",
        "calendar",
    )?;
    Ok(true)
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn vault_pick_and_read_ics_import(
    _app: tauri::AppHandle,
) -> Result<Option<Vec<IcsZipEntry>>, String> {
    Err("calendar document import is not available on iOS yet".to_string())
}

#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn vault_pick_and_write_ics_export(
    _app: tauri::AppHandle,
    _default_name: String,
    _contents: String,
) -> Result<bool, String> {
    Err("calendar document export is not available on iOS yet".to_string())
}

/// Open a native file picker and read a theme `.json` file with a small cap.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_and_read_theme_json(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let Some(path) = on_dialog_thread(move || {
        pick_open_path(&app, "Import theme", "Theme JSON", &["json"], None)
    })
    .await??
    else {
        return Ok(None);
    };
    require_extension(&path, &["json"], "theme import")?;
    read_text_file_capped(&path, THEME_JSON_MAX_BYTES, "theme import").map(Some)
}

/// Open a native save dialog and write a theme `.json` export.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_and_write_theme_json(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<ThemeJsonWriteOutcome, String> {
    require_text_within_limit(&contents, THEME_JSON_MAX_BYTES, "theme export")?;
    let default_name = default_file_name(&default_name, "theme", "json");
    let start_directory = existing_downloads_directory(&app);
    let Some(path) = on_dialog_thread(move || {
        pick_save_path(
            &app,
            "Export theme",
            &default_name,
            "Theme JSON",
            &["json"],
            start_directory,
        )
    })
    .await??
    else {
        return Ok(ThemeJsonWriteOutcome::cancelled());
    };
    require_extension(&path, &["json"], "theme export")?;
    write_text_file_atomically(&path, &contents, FileAccess::Default)?;
    Ok(ThemeJsonWriteOutcome::saved_to_selected_file())
}

/// Open a native save dialog and write the vault's recovery kit as a plain text file. Returns
/// whether the user picked a file.
#[cfg(desktop)]
#[tauri::command]
pub async fn vault_pick_and_write_recovery_kit(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<bool, String> {
    require_text_within_limit(&contents, RECOVERY_KIT_TEXT_MAX_BYTES, "recovery kit")?;
    let default_name = default_file_name(&default_name, "recovery-kit", "txt");
    let start_directory = existing_downloads_directory(&app);
    let Some(path) = on_dialog_thread(move || {
        pick_save_path(
            &app,
            "Save recovery kit",
            &default_name,
            "Text",
            &["txt"],
            start_directory,
        )
    })
    .await??
    else {
        return Ok(false);
    };
    require_extension(&path, &["txt"], "recovery kit")?;
    write_text_file_atomically(&path, &contents, FileAccess::OwnerOnly)?;
    Ok(true)
}

#[cfg(target_os = "ios")]
fn finish_mobile_document_access<R: Runtime, T>(
    app: &tauri::AppHandle<R>,
    path: FilePath,
    result: Result<T, String>,
) -> Result<T, String> {
    let cleanup = app
        .fs()
        .stop_accessing_security_scoped_resource(path)
        .map_err(|e| format!("release selected theme document: {e}"));
    match result {
        Err(error) => Err(error),
        Ok(value) => cleanup.map(|()| value),
    }
}

#[cfg(target_os = "ios")]
fn read_mobile_theme_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    path: FilePath,
) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    let result = (|| {
        let mut file = app
            .fs()
            .open(path.clone(), options)
            .map_err(|e| format!("failed to open theme import: {e}"))?;
        read_utf8_capped(&mut file, THEME_JSON_MAX_BYTES, "theme import")
    })();
    finish_mobile_document_access(app, path, result)
}

#[cfg(target_os = "ios")]
fn write_mobile_theme_document<R: Runtime>(
    app: &tauri::AppHandle<R>,
    path: FilePath,
    contents: &str,
) -> Result<(), String> {
    require_text_within_limit(contents, THEME_JSON_MAX_BYTES, "theme export")?;
    let mut options = OpenOptions::new();
    options.write(true).truncate(true);
    let result = (|| {
        let mut file = app
            .fs()
            .open(path.clone(), options)
            .map_err(|e| format!("failed to open theme export: {e}"))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("failed to write theme export: {e}"))?;
        file.flush()
            .map_err(|e| format!("failed to flush theme export: {e}"))?;
        Ok(())
    })();
    finish_mobile_document_access(app, path, result)
}

/// Open iOS document storage and read one bounded theme JSON file.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn vault_pick_and_read_theme_json(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .add_filter("Theme JSON", THEME_JSON_DOCUMENT_FILTERS)
        .blocking_pick_file();
    selected
        .map(|path| read_mobile_theme_document(&app, path))
        .transpose()
}

/// Create a theme JSON document through iOS document storage.
#[cfg(target_os = "ios")]
#[tauri::command]
pub async fn vault_pick_and_write_theme_json(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<ThemeJsonWriteOutcome, String> {
    require_text_within_limit(&contents, THEME_JSON_MAX_BYTES, "theme export")?;
    let default_name = default_file_name(&default_name, "theme", "json");
    let selected = app
        .dialog()
        .file()
        .set_file_name(default_name)
        .add_filter("Theme JSON", THEME_JSON_DOCUMENT_FILTERS)
        .blocking_save_file();
    let Some(path) = selected else {
        return Ok(ThemeJsonWriteOutcome::cancelled());
    };
    write_mobile_theme_document(&app, path, &contents)?;
    Ok(ThemeJsonWriteOutcome::saved_to_selected_file())
}

/// Ask Android to select and read one bounded UTF-8 theme document.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_pick_and_read_theme_json(
    app: tauri::AppHandle,
) -> Result<Option<String>, String> {
    app.mobile_documents()
        .pick_utf8_document(THEME_JSON_MAX_BYTES)
}

/// Save a theme JSON export to Android's public Downloads collection.
#[cfg(target_os = "android")]
#[tauri::command]
pub async fn vault_pick_and_write_theme_json(
    app: tauri::AppHandle,
    default_name: String,
    contents: String,
) -> Result<ThemeJsonWriteOutcome, String> {
    require_text_within_limit(&contents, THEME_JSON_MAX_BYTES, "theme export")?;
    let default_name = default_file_name(&default_name, "theme", "json");
    let file_name = app.mobile_documents().save_utf8_download(
        &default_name,
        &contents,
        THEME_JSON_MAX_BYTES,
    )?;
    Ok(ThemeJsonWriteOutcome::saved_to_downloads(file_name))
}

#[cfg(all(test, desktop))]
mod tests {
    use super::*;
    use crate::vault::tests::unique_path;
    use std::io::Write;

    #[test]
    fn read_text_file_capped_rejects_relative_paths() {
        let err = read_text_file_capped(Path::new("relative/path.txt"), 1024, "test").unwrap_err();
        assert_eq!(err, "path must be absolute");
    }

    #[test]
    fn read_text_file_capped_returns_file_contents_for_absolute_path() {
        let path = unique_path("read.txt");
        fs::write(&path, "hello vault").expect("seed file");
        let result = read_text_file_capped(&path, 1024, "test");
        let _ = fs::remove_file(&path);
        assert_eq!(result.unwrap(), "hello vault");
    }

    #[test]
    fn read_utf8_capped_enforces_the_limit_while_streaming() {
        let mut reader = std::io::Cursor::new(b"12345");
        let error = read_utf8_capped(&mut reader, 4, "theme import").unwrap_err();
        assert_eq!(error, "theme import exceeds the limit of 4 bytes");
    }

    #[test]
    fn theme_export_rejects_oversized_text_before_opening_a_document() {
        let contents = "x".repeat((THEME_JSON_MAX_BYTES + 1) as usize);
        let error =
            require_text_within_limit(&contents, THEME_JSON_MAX_BYTES, "theme export").unwrap_err();
        assert!(error.contains("exceeding the limit"));
    }

    #[test]
    fn default_file_name_drops_path_components_and_preserves_json_extension() {
        assert_eq!(
            default_file_name("../../midnight", "theme", "json"),
            "midnight.json"
        );
        assert_eq!(
            default_file_name("midnight.JSON", "theme", "json"),
            "midnight.JSON"
        );
    }

    #[cfg(desktop)]
    #[test]
    fn recovery_kit_text_is_capped_and_saved_as_txt() {
        let within = "A".repeat(RECOVERY_KIT_TEXT_MAX_BYTES as usize);
        assert!(require_text_within_limit(&within, RECOVERY_KIT_TEXT_MAX_BYTES, "kit").is_ok());
        let over = format!("{within}A");
        assert!(require_text_within_limit(&over, RECOVERY_KIT_TEXT_MAX_BYTES, "kit").is_err());
        assert_eq!(
            default_file_name("../Ganbaru recovery kit", "recovery-kit", "txt"),
            "Ganbaru recovery kit.txt"
        );
    }

    #[test]
    fn read_plain_ics_entry_rejects_wrong_extensions() {
        let path = unique_path("calendar.txt");
        fs::write(&path, "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").expect("seed file");
        let result = read_plain_ics_entry_from_path(&path);
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.contains("ICS import file must use one of: ics"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn read_plain_ics_entry_rejects_oversized_files() {
        let path = unique_path("oversized.ics");
        let file = fs::File::create(&path).expect("create file");
        file.set_len(ICS_PLAIN_MAX_BYTES + 1)
            .expect("set oversized length");
        let result = read_plain_ics_entry_from_path(&path);
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.contains("exceeding the limit"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn read_theme_json_rejects_oversized_files() {
        let path = unique_path("oversized.json");
        let file = fs::File::create(&path).expect("create file");
        file.set_len(THEME_JSON_MAX_BYTES + 1)
            .expect("set oversized length");
        let result = read_text_file_capped(&path, THEME_JSON_MAX_BYTES, "theme import");
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.contains("exceeding the limit"),
            "unexpected error: {err}"
        );
    }

    /// Build a zip file at `path` containing the given `(name, contents)`
    /// pairs. Uses the Stored compression method so the test does not depend
    /// on the deflate path being exercised correctly.
    fn write_zip(path: &PathBuf, entries: &[(&str, &[u8])]) {
        use zip::CompressionMethod;
        use zip::write::SimpleFileOptions;
        use zip::write::ZipWriter;

        let file = fs::File::create(path).expect("create zip file");
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, data) in entries {
            writer.start_file(*name, options).expect("start_file");
            writer.write_all(data).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }

    #[test]
    fn read_ics_zip_entries_rejects_relative_paths() {
        let err = read_ics_zip_entries_from_path(Path::new("relative/path.zip")).unwrap_err();
        assert_eq!(err, "path must be absolute");
    }

    #[test]
    fn read_ics_zip_entries_rejects_wrong_extensions() {
        let path = unique_path("archive.txt");
        fs::write(&path, b"this is not a zip archive").expect("seed file");
        let result = read_ics_zip_entries_from_path(&path);
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.contains("ICS zip import file must use one of: zip"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn read_ics_zip_entries_rejects_non_zip_files() {
        let path = unique_path("not-a-zip.zip");
        fs::write(&path, b"this is not a zip archive").expect("seed file");
        let result = read_ics_zip_entries_from_path(&path);
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.starts_with("not a valid zip archive"),
            "expected not-a-zip error, got: {err}"
        );
    }

    #[test]
    fn read_ics_zip_entries_returns_only_ics_entries() {
        let path = unique_path("mixed.zip");
        let ics_a = b"BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n";
        let ics_b = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";
        write_zip(
            &path,
            &[
                ("calendar.ics", ics_a),
                ("readme.txt", b"ignore me"),
                ("nested/holidays.ICS", ics_b),
                ("__MACOSX/.DS_Store", b"junk"),
            ],
        );

        let result = read_ics_zip_entries_from_path(&path);
        let _ = fs::remove_file(&path);
        let entries = result.expect("should succeed");
        assert_eq!(entries.len(), 2, "should keep only the .ics entries");

        // Names are basenames, not full archive paths.
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"calendar.ics"), "got names: {names:?}");
        assert!(names.contains(&"holidays.ICS"), "got names: {names:?}");

        let calendar = entries.iter().find(|e| e.name == "calendar.ics").unwrap();
        assert_eq!(calendar.contents.as_bytes(), ics_a);
    }

    #[test]
    fn read_ics_zip_entries_returns_empty_when_no_ics_present() {
        let path = unique_path("no-ics.zip");
        write_zip(&path, &[("readme.txt", b"hello"), ("notes.md", b"# title")]);
        let result = read_ics_zip_entries_from_path(&path);
        let _ = fs::remove_file(&path);
        assert!(result.unwrap().is_empty());
    }

    /// Rewrites every local and central directory uncompressed size to zero so
    /// declared sizes stop matching the decompressed bytes.
    fn zero_declared_sizes(path: &Path) {
        const LOCAL_HEADER: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
        const CENTRAL_HEADER: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
        const LOCAL_SIZE_OFFSET: usize = 22;
        const CENTRAL_SIZE_OFFSET: usize = 24;
        let mut bytes = fs::read(path).expect("read zip");
        let mut patched = 0;
        for index in 0..bytes.len().saturating_sub(4) {
            let header = &bytes[index..index + 4];
            let offset = if header == LOCAL_HEADER {
                LOCAL_SIZE_OFFSET
            } else if header == CENTRAL_HEADER {
                CENTRAL_SIZE_OFFSET
            } else {
                continue;
            };
            bytes[index + offset..index + offset + 4].fill(0);
            patched += 1;
        }
        assert!(patched >= 2, "expected zip headers to patch");
        fs::write(path, bytes).expect("write patched zip");
    }

    #[test]
    fn read_ics_zip_entries_rejects_archives_over_the_aggregate_limit() {
        let path = unique_path("aggregate.zip");
        let calendar = vec![b'A'; 800];
        write_zip(
            &path,
            &[
                ("one.ics", calendar.as_slice()),
                ("two.ics", calendar.as_slice()),
                ("three.ics", calendar.as_slice()),
            ],
        );
        let limits = IcsZipLimits {
            max_entries: 16,
            max_entry_bytes: 1_000,
            max_total_bytes: 1_500,
        };
        let result = read_ics_zip_entries_with_limits(&path, limits);
        let generous = read_ics_zip_entries_with_limits(
            &path,
            IcsZipLimits {
                max_total_bytes: 10_000,
                ..limits
            },
        );
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(
            err.contains("total uncompressed size exceeds the limit of 1500 bytes"),
            "unexpected error: {err}"
        );
        let entries = generous.expect("sizes within the limits must import");
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|entry| entry.contents.len() == 800));
    }

    #[test]
    fn read_ics_zip_entries_rejects_entries_larger_than_their_declared_size() {
        let path = unique_path("lying-entry.zip");
        let big = vec![b'A'; 2_000];
        write_zip(&path, &[("big.ics", big.as_slice())]);
        zero_declared_sizes(&path);
        let result = read_ics_zip_entries_with_limits(
            &path,
            IcsZipLimits {
                max_entries: 16,
                max_entry_bytes: 1_000,
                max_total_bytes: 10_000,
            },
        );
        let _ = fs::remove_file(&path);
        let err = result.unwrap_err();
        assert!(err.contains("big.ics"), "unexpected error: {err}");
    }
}

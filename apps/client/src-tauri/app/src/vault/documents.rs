//! Native Calendar and theme document transfer, independent of vault selection.

#[cfg(desktop)]
use super::{dialog_path, require_absolute_path, write_text_file_atomically};
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

/// Hard cap on the number of entries inspected inside a single zip.
/// 1024 is well above the realistic count for an export from Google
/// Calendar (one .ics per calendar a user owns or subscribes to is usually
/// < 50) and protects against pathological inputs that could DoS the read
/// loop.
#[cfg(desktop)]
const ICS_ZIP_MAX_ENTRIES: usize = 1024;

/// Hard cap on the uncompressed size of a single entry, in bytes. 25 MiB
/// is large enough for a multi-decade calendar with thousands of events
/// (text-only iCalendar averages ~1 KiB per event) while clearly rejecting
/// decompression bombs.
#[cfg(desktop)]
const ICS_ZIP_MAX_ENTRY_BYTES: u64 = 25 * 1024 * 1024;

/// Hard cap on the aggregate uncompressed size across every entry. Keeps a
/// zip with many oversized entries from defeating the per-entry guard.
#[cfg(desktop)]
const ICS_ZIP_MAX_TOTAL_BYTES: u64 = 250 * 1024 * 1024;

/// Plain `.ics` imports share the zip per-entry cap so the import flow has
/// one clear maximum payload size regardless of container.
const ICS_PLAIN_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Theme JSON is small configuration data. One MiB leaves room for custom
/// comments and future tokens while rejecting accidental large-file picks.
const THEME_JSON_MAX_BYTES: u64 = 1024 * 1024;

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
/// - Per-entry and aggregate decompressed-size caps reject decompression
///   bombs even if the zip header lies about uncompressed size (the read
///   itself is wrapped in a `Take` adapter so the cap holds at I/O level).
/// - Only entries whose extension matches `.ics` are returned. Anything
///   else (`__MACOSX/`, `.DS_Store`, signature files, archived metadata)
///   is silently skipped so the importer never sees them.
#[cfg(desktop)]
fn read_ics_zip_entries_from_path(path: &Path) -> Result<Vec<IcsZipEntry>, String> {
    require_extension(path, &["zip"], "ICS zip import")?;

    let file = fs::File::open(path).map_err(|e| format!("failed to open zip: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip archive: {e}"))?;

    if archive.len() > ICS_ZIP_MAX_ENTRIES {
        return Err(format!(
            "zip has {} entries, exceeding the limit of {ICS_ZIP_MAX_ENTRIES}",
            archive.len()
        ));
    }

    let mut entries: Vec<IcsZipEntry> = Vec::new();
    let mut total_uncompressed: u64 = 0;

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
        if reported_size > ICS_ZIP_MAX_ENTRY_BYTES {
            return Err(format!(
                "zip entry '{}' uncompressed size ({reported_size} bytes) exceeds the per-entry limit of {ICS_ZIP_MAX_ENTRY_BYTES} bytes",
                enclosed.display()
            ));
        }
        total_uncompressed = total_uncompressed.saturating_add(reported_size);
        if total_uncompressed > ICS_ZIP_MAX_TOTAL_BYTES {
            return Err(format!(
                "zip total uncompressed size exceeds the limit of {ICS_ZIP_MAX_TOTAL_BYTES} bytes"
            ));
        }

        let display_name = enclosed
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("entry-{i}.ics"));

        // Capture only the basename for the frontend before borrowing the
        // entry mutably for the read.
        drop(enclosed);

        // The `Take` adapter is the real defense against a lying header:
        // even if `entry.size()` claims a small value, reading stops one
        // byte past the cap and the entry is rejected.
        let mut contents = String::new();
        let mut limited = entry.by_ref().take(ICS_ZIP_MAX_ENTRY_BYTES + 1);
        limited
            .read_to_string(&mut contents)
            .map_err(|e| format!("failed to read zip entry '{display_name}' as UTF-8: {e}"))?;
        if contents.len() as u64 > ICS_ZIP_MAX_ENTRY_BYTES {
            return Err(format!(
                "zip entry '{display_name}' exceeds the per-entry size limit during decompression"
            ));
        }

        entries.push(IcsZipEntry {
            name: display_name,
            contents,
        });
    }

    Ok(entries)
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
    let Some(path) = pick_open_path(
        &app,
        "Import calendar",
        "iCalendar (.ics or .zip)",
        &["ics", "zip"],
        existing_downloads_directory(&app),
    )?
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
    let default_name = default_file_name(&default_name, "calendar", "ics");
    let Some(path) = pick_save_path(
        &app,
        "Export calendar",
        &default_name,
        "iCalendar",
        &["ics"],
        None,
    )?
    else {
        return Ok(false);
    };
    require_extension(&path, &["ics"], "ICS export")?;
    write_text_file_atomically(&path, &contents)?;
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
    let Some(path) = pick_open_path(&app, "Import theme", "Theme JSON", &["json"], None)? else {
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
    let Some(path) = pick_save_path(
        &app,
        "Export theme",
        &default_name,
        "Theme JSON",
        &["json"],
        existing_downloads_directory(&app),
    )?
    else {
        return Ok(ThemeJsonWriteOutcome::cancelled());
    };
    require_extension(&path, &["json"], "theme export")?;
    write_text_file_atomically(&path, &contents)?;
    Ok(ThemeJsonWriteOutcome::saved_to_selected_file())
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
}

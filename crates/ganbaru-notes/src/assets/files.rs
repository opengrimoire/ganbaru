use base64::{Engine as _, engine::general_purpose};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use crate::assets::{
    self, ASSET_SOURCE_IMPORTED, ASSET_SOURCE_LOCAL_UPLOAD, ASSET_STATE_AVAILABLE,
    ManagedAssetWrite,
};

const NOTES_FILE_MAX_DISPLAY_MEGABYTES: usize = 50;
const NOTES_FILE_MAX_BYTES: usize = NOTES_FILE_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES: i64 = 25;
const NOTES_FILE_PREVIEW_MAX_BYTES: i64 = NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const NOTES_FILE_DIR: &str = "notes/files";
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const EXTERNAL_IMAGE_EXTENSIONS: &[&str] = &[
    "bmp", "gif", "heic", "jpeg", "jpg", "png", "svg", "tif", "tiff", "webp",
];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "oga", "m4a"];
const VIDEO_EXTENSIONS: &[&str] = &[
    "amv", "asf", "avi", "f4v", "flv", "gifv", "mkv", "mov", "mpg", "mpeg", "mpv", "mp4", "m4v",
    "qt", "wmv",
];
const NOTES_IMPORT_FILE_CONTEXTS: &[&str] = &["notion_export", "html_import", "markdown_import"];
const NOTES_IMPORT_FILE_CHOICES: &[&str] = &["copy_local_file", "keep_external_reference", "skip"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesFileAssetDto {
    pub relative_path: String,
    pub original_name: Option<String>,
    pub content_type: String,
    pub byte_size: i64,
    pub sha256: String,
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesImportFileReferenceRequest {
    pub import_context: String,
    pub choice: String,
    pub reference: String,
    pub import_root: Option<String>,
    pub block_type: Option<String>,
    pub original_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotesImportFileDiagnosticDto {
    pub code: String,
    pub severity: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotesImportFileReferenceDto {
    pub action: String,
    pub asset: Option<NotesFileAssetDto>,
    pub external_url: Option<String>,
    pub diagnostics: Vec<NotesImportFileDiagnosticDto>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotesFileAssetKind {
    Image,
    Video,
    Audio,
    Pdf,
    File,
}

impl NotesFileAssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Pdf => "pdf",
            Self::File => "file",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotesImageKind {
    Png,
    Jpeg,
    Webp,
}

impl NotesImageKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
        }
    }

    fn content_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

fn active_notes_file_dir(vault_root: &Path) -> PathBuf {
    vault_root.join("assets").join(NOTES_FILE_DIR)
}

fn asset_path_for_relative(vault_root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let file_name = validate_notes_file_relative_path(relative_path)?;
    Ok(active_notes_file_dir(vault_root).join(file_name))
}

fn validate_notes_file_relative_path(relative_path: &str) -> Result<&str, String> {
    let relative_path = relative_path.trim();
    let prefix = format!("{NOTES_FILE_DIR}/");
    let file_name = relative_path
        .strip_prefix(&prefix)
        .ok_or_else(|| "notes file path must stay under notes/files".to_string())?;
    if file_name.is_empty() {
        return Err("notes file path is missing a file name".to_string());
    }
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("notes file path cannot contain nested or parent paths".to_string());
    }
    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("notes file path cannot contain nested or parent paths".to_string());
    }
    Ok(file_name)
}

fn ensure_notes_file_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() {
        return Err("notes file is empty".to_string());
    }
    if bytes.len() > NOTES_FILE_MAX_BYTES {
        return Err(format!(
            "notes file exceeds the {NOTES_FILE_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    Ok(())
}

fn read_file_capped(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("inspect notes file: {e}"))?;
    if !metadata.is_file() {
        return Err("notes file path must be a file".to_string());
    }
    if metadata.len() > NOTES_FILE_MAX_BYTES as u64 {
        return Err(format!(
            "notes file exceeds the {NOTES_FILE_MAX_DISPLAY_MEGABYTES} MB limit"
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("read notes file: {e}"))?;
    ensure_notes_file_size(&bytes)?;
    Ok(bytes)
}

fn read_preview_file_capped(path: &Path, byte_size: i64) -> Result<Vec<u8>, String> {
    if byte_size > NOTES_FILE_PREVIEW_MAX_BYTES {
        return Err(format!(
            "notes file exceeds the {NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES} MB preview limit"
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("read notes file preview: {e}"))?;
    if bytes.len() as i64 != byte_size {
        return Err("notes file size no longer matches its recorded metadata".to_string());
    }
    Ok(bytes)
}

fn write_binary_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "notes file target has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("create notes file directory: {e}"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| "notes file target has no file name".to_string())?
        .to_string_lossy()
        .into_owned();
    let temporary_path = parent.join(format!("{file_name}.tmp"));
    {
        let mut file =
            fs::File::create(&temporary_path).map_err(|e| format!("write notes file: {e}"))?;
        file.write_all(bytes)
            .map_err(|e| format!("write notes file: {e}"))?;
        file.sync_all()
            .map_err(|e| format!("sync notes file: {e}"))?;
    }
    fs::rename(&temporary_path, path).map_err(|e| format!("save notes file: {e}"))
}

fn hex_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn block_type_kind(block_type: &str) -> Result<NotesFileAssetKind, String> {
    match block_type.trim() {
        "image" => Ok(NotesFileAssetKind::Image),
        "video" => Ok(NotesFileAssetKind::Video),
        "audio" => Ok(NotesFileAssetKind::Audio),
        "pdf" => Ok(NotesFileAssetKind::Pdf),
        "file" => Ok(NotesFileAssetKind::File),
        _ => Err("notes file assets are only supported for media and file blocks".to_string()),
    }
}

fn import_diagnostic(
    code: &str,
    severity: &str,
    message: impl Into<String>,
) -> NotesImportFileDiagnosticDto {
    NotesImportFileDiagnosticDto {
        code: code.to_string(),
        severity: severity.to_string(),
        message: message.into(),
    }
}

fn import_blocked(code: &str, message: impl Into<String>) -> NotesImportFileReferenceDto {
    NotesImportFileReferenceDto {
        action: "blocked".to_string(),
        asset: None,
        external_url: None,
        diagnostics: vec![import_diagnostic(code, "error", message)],
    }
}

fn import_skipped() -> NotesImportFileReferenceDto {
    NotesImportFileReferenceDto {
        action: "skipped".to_string(),
        asset: None,
        external_url: None,
        diagnostics: vec![import_diagnostic(
            "import_reference_skipped",
            "info",
            "The import file reference was skipped by user choice.",
        )],
    }
}

fn import_context_is_supported(context: &str) -> bool {
    NOTES_IMPORT_FILE_CONTEXTS.contains(&context.trim())
}

fn import_choice_is_supported(choice: &str) -> bool {
    NOTES_IMPORT_FILE_CHOICES.contains(&choice.trim())
}

fn is_url_like_import_reference(reference: &str) -> bool {
    let trimmed = reference.trim().to_ascii_lowercase();
    trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("ganbaru-asset:")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HttpsUrlParts<'a> {
    host: &'a str,
    path_and_query: &'a str,
}

fn parse_https_url_parts(
    reference: &str,
) -> Result<HttpsUrlParts<'_>, NotesImportFileDiagnosticDto> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return Err(import_diagnostic(
            "import_reference_missing",
            "error",
            "Choose a file reference before importing it.",
        ));
    }
    if trimmed
        .chars()
        .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err(import_diagnostic(
            "import_reference_external_invalid",
            "error",
            "External file references must be a single HTTPS URL without spaces.",
        ));
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        return Err(import_diagnostic(
            "import_reference_external_requires_https",
            "error",
            "External file references must use HTTPS.",
        ));
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() {
        return Err(import_diagnostic(
            "import_reference_external_invalid",
            "error",
            "External file references must include a host.",
        ));
    }
    Ok(HttpsUrlParts {
        host,
        path_and_query: &rest[host_end..],
    })
}

fn lower_url_path_without_query(reference: &str) -> String {
    let trimmed = reference.trim();
    let without_scheme = trimmed.strip_prefix("https://").unwrap_or(trimmed);
    let path_start = without_scheme.find('/').unwrap_or(without_scheme.len());
    let path_and_query = &without_scheme[path_start..];
    let path_end = path_and_query
        .find(['?', '#'])
        .unwrap_or(path_and_query.len());
    path_and_query[..path_end].to_ascii_lowercase()
}

fn url_has_supported_extension(reference: &str, allowed: &[&str]) -> bool {
    let path = lower_url_path_without_query(reference);
    allowed
        .iter()
        .any(|extension| path.ends_with(&format!(".{extension}")))
}

fn is_supported_youtube_url(reference: &str) -> bool {
    let Ok(parts) = parse_https_url_parts(reference) else {
        return false;
    };
    let host = parts
        .host
        .split(':')
        .next()
        .unwrap_or(parts.host)
        .to_ascii_lowercase();
    if host != "youtube.com" && host != "www.youtube.com" {
        return false;
    }
    parts.path_and_query.starts_with("/embed/")
        || (parts.path_and_query.starts_with("/watch?") && parts.path_and_query.contains("v="))
}

fn validate_external_reference_for_block_type(
    block_type: Option<&str>,
    reference: &str,
) -> Result<(), NotesImportFileDiagnosticDto> {
    parse_https_url_parts(reference)?;
    let Some(block_type) = block_type.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let requested_kind = block_type_kind(block_type).map_err(|error| {
        import_diagnostic("import_reference_block_type_unsupported", "error", error)
    })?;
    let is_supported = match requested_kind {
        NotesFileAssetKind::File => true,
        NotesFileAssetKind::Image => {
            url_has_supported_extension(reference, EXTERNAL_IMAGE_EXTENSIONS)
        }
        NotesFileAssetKind::Pdf => url_has_supported_extension(reference, &["pdf"]),
        NotesFileAssetKind::Audio => url_has_supported_extension(reference, AUDIO_EXTENSIONS),
        NotesFileAssetKind::Video => {
            url_has_supported_extension(reference, VIDEO_EXTENSIONS)
                || is_supported_youtube_url(reference)
        }
    };
    if is_supported {
        Ok(())
    } else {
        Err(import_diagnostic(
            "import_reference_external_unsupported_type",
            "error",
            "The external file reference does not match the target block type.",
        ))
    }
}

fn prepare_external_import_reference(
    reference: &str,
    block_type: Option<&str>,
) -> NotesImportFileReferenceDto {
    match validate_external_reference_for_block_type(block_type, reference) {
        Ok(()) => NotesImportFileReferenceDto {
            action: "external_reference".to_string(),
            asset: None,
            external_url: Some(reference.trim().to_string()),
            diagnostics: vec![import_diagnostic(
                "import_reference_external_kept",
                "info",
                "The import will keep this as an explicit external HTTPS reference.",
            )],
        },
        Err(diagnostic) => NotesImportFileReferenceDto {
            action: "blocked".to_string(),
            asset: None,
            external_url: None,
            diagnostics: vec![diagnostic],
        },
    }
}

pub async fn copy_local_import_file_for_block(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    import_root: &Path,
    reference: &str,
    block_type: &str,
    original_name: Option<String>,
) -> NotesImportFileReferenceDto {
    let requested_kind = match block_type_kind(block_type) {
        Ok(kind) => kind,
        Err(error) => {
            return import_blocked("import_reference_block_type_unsupported", error);
        }
    };
    let path = match resolve_import_candidate_path(&import_root.to_string_lossy(), reference) {
        Ok(path) => path,
        Err(diagnostic) => {
            return NotesImportFileReferenceDto {
                action: "blocked".to_string(),
                asset: None,
                external_url: None,
                diagnostics: vec![diagnostic],
            };
        }
    };
    let original_name = original_name.or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    });
    let bytes = match read_file_capped(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return import_blocked(
                "import_reference_copy_failed",
                format!("Could not read the import file: {error}"),
            );
        }
    };
    match save_notes_file_bytes(
        pool,
        vault_root,
        requested_kind,
        &path,
        bytes,
        original_name,
        ASSET_SOURCE_IMPORTED,
    )
    .await
    {
        Ok(asset) => NotesImportFileReferenceDto {
            action: "copied_asset".to_string(),
            asset: Some(asset),
            external_url: None,
            diagnostics: vec![import_diagnostic(
                "import_reference_copied",
                "info",
                "The import file was copied into managed local Notes assets.",
            )],
        },
        Err(error) => import_blocked(
            "import_reference_copy_failed",
            format!("Could not copy the import file: {error}"),
        ),
    }
}

fn resolve_import_candidate_path(
    import_root: &str,
    reference: &str,
) -> Result<PathBuf, NotesImportFileDiagnosticDto> {
    let root_text = import_root.trim();
    if root_text.is_empty() {
        return Err(import_diagnostic(
            "import_reference_root_required",
            "error",
            "Select an import folder before copying import files.",
        ));
    }
    let root_input = Path::new(root_text);
    if !root_input.is_absolute() {
        return Err(import_diagnostic(
            "import_reference_root_required",
            "error",
            "The import folder must be an absolute local path.",
        ));
    }
    let root = fs::canonicalize(root_input).map_err(|e| {
        import_diagnostic(
            "import_reference_root_not_directory",
            "error",
            format!("Could not inspect the import folder: {e}"),
        )
    })?;
    if !root.is_dir() {
        return Err(import_diagnostic(
            "import_reference_root_not_directory",
            "error",
            "The import root must be a local folder.",
        ));
    }
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(import_diagnostic(
            "import_reference_missing",
            "error",
            "Choose a file reference before importing it.",
        ));
    }
    if reference.chars().any(char::is_control) {
        return Err(import_diagnostic(
            "import_reference_invalid_local_path",
            "error",
            "Local import file references must not contain control characters.",
        ));
    }
    let reference_path = Path::new(reference);
    let candidate_input = if reference_path.is_absolute() {
        reference_path.to_path_buf()
    } else {
        root.join(reference_path)
    };
    let candidate = fs::canonicalize(&candidate_input).map_err(|e| {
        import_diagnostic(
            "import_reference_not_file",
            "error",
            format!("Could not inspect the import file: {e}"),
        )
    })?;
    if !candidate.starts_with(&root) {
        return Err(import_diagnostic(
            "import_reference_path_escape",
            "error",
            "Import files must stay inside the selected import folder.",
        ));
    }
    let metadata = fs::metadata(&candidate).map_err(|e| {
        import_diagnostic(
            "import_reference_not_file",
            "error",
            format!("Could not inspect the import file: {e}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(import_diagnostic(
            "import_reference_not_file",
            "error",
            "The import reference must point to a local file.",
        ));
    }
    if metadata.len() > NOTES_FILE_MAX_BYTES as u64 {
        return Err(import_diagnostic(
            "import_reference_too_large",
            "error",
            format!("Import files cannot exceed {NOTES_FILE_MAX_DISPLAY_MEGABYTES} MB."),
        ));
    }
    Ok(candidate)
}

fn sniff_image_kind(bytes: &[u8]) -> Option<NotesImageKind> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(NotesImageKind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(NotesImageKind::Jpeg);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(NotesImageKind::Webp);
    }
    None
}

fn path_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim().to_ascii_lowercase())
        .filter(|extension| {
            !extension.is_empty()
                && extension.len() <= 16
                && extension.chars().all(|value| value.is_ascii_alphanumeric())
        })
}

fn extension_is(path: &Path, allowed: &[&str]) -> bool {
    path_extension(path).is_some_and(|extension| {
        allowed
            .iter()
            .any(|allowed_extension| extension.eq_ignore_ascii_case(allowed_extension))
    })
}

fn classify_selected_file(
    requested_kind: NotesFileAssetKind,
    path: &Path,
    bytes: &[u8],
) -> Result<(NotesFileAssetKind, &'static str, String), String> {
    match requested_kind {
        NotesFileAssetKind::Image => {
            let Some(image_kind) = sniff_image_kind(bytes) else {
                return Err("local image blocks support PNG, JPG, and WebP files".to_string());
            };
            Ok((
                NotesFileAssetKind::Image,
                image_kind.content_type(),
                image_kind.extension().to_string(),
            ))
        }
        NotesFileAssetKind::Pdf => {
            if !bytes.starts_with(b"%PDF-") || !extension_is(path, &["pdf"]) {
                return Err("local PDF blocks require a PDF file".to_string());
            }
            Ok((
                NotesFileAssetKind::Pdf,
                "application/pdf",
                "pdf".to_string(),
            ))
        }
        NotesFileAssetKind::Audio => {
            let Some(extension) = path_extension(path) else {
                return Err("local audio blocks require a supported audio extension".to_string());
            };
            if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
                return Err(
                    "local audio blocks support MP3, WAV, OGG, OGA, and M4A files".to_string(),
                );
            }
            Ok((
                NotesFileAssetKind::Audio,
                audio_content_type(&extension),
                extension,
            ))
        }
        NotesFileAssetKind::Video => {
            let Some(extension) = path_extension(path) else {
                return Err("local video blocks require a supported video extension".to_string());
            };
            if !VIDEO_EXTENSIONS.contains(&extension.as_str()) {
                return Err("local video blocks require a supported video file".to_string());
            }
            Ok((
                NotesFileAssetKind::Video,
                video_content_type(&extension),
                extension,
            ))
        }
        NotesFileAssetKind::File => {
            let extension = path_extension(path).unwrap_or_else(|| "bin".to_string());
            let content_type = generic_content_type(&extension);
            Ok((kind_for_content_type(content_type), content_type, extension))
        }
    }
}

fn audio_content_type(extension: &str) -> &'static str {
    match extension {
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}

fn video_content_type(extension: &str) -> &'static str {
    match extension {
        "avi" => "video/x-msvideo",
        "flv" | "f4v" => "video/x-flv",
        "mkv" => "video/x-matroska",
        "mov" | "qt" => "video/quicktime",
        "mpeg" | "mpg" => "video/mpeg",
        "wmv" => "video/x-ms-wmv",
        _ => "video/mp4",
    }
}

fn generic_content_type(extension: &str) -> &'static str {
    match extension {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        extension if VIDEO_EXTENSIONS.contains(&extension) => video_content_type(extension),
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        _ => "application/octet-stream",
    }
}

fn kind_for_content_type(content_type: &str) -> NotesFileAssetKind {
    if matches!(content_type, "image/png" | "image/jpeg" | "image/webp") {
        NotesFileAssetKind::Image
    } else if content_type.starts_with("video/") {
        NotesFileAssetKind::Video
    } else if content_type.starts_with("audio/") {
        NotesFileAssetKind::Audio
    } else if content_type == "application/pdf" {
        NotesFileAssetKind::Pdf
    } else {
        NotesFileAssetKind::File
    }
}

fn picker_extensions_for_kind(
    kind: NotesFileAssetKind,
) -> Option<(&'static str, &'static [&'static str])> {
    match kind {
        NotesFileAssetKind::Image => Some(("Image", IMAGE_EXTENSIONS)),
        NotesFileAssetKind::Video => Some(("Video", VIDEO_EXTENSIONS)),
        NotesFileAssetKind::Audio => Some(("Audio", AUDIO_EXTENSIONS)),
        NotesFileAssetKind::Pdf => Some(("PDF", &["pdf"])),
        NotesFileAssetKind::File => None,
    }
}

pub fn picker_extensions_for_block_type(
    block_type: &str,
) -> Result<Option<(&'static str, &'static [&'static str])>, String> {
    block_type_kind(block_type).map(picker_extensions_for_kind)
}

async fn save_notes_file_bytes(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    requested_kind: NotesFileAssetKind,
    source_path: &Path,
    bytes: Vec<u8>,
    original_name: Option<String>,
    source_type: &'static str,
) -> Result<NotesFileAssetDto, String> {
    ensure_notes_file_size(&bytes)?;
    let (kind, content_type, extension) =
        classify_selected_file(requested_kind, source_path, &bytes)?;
    let sha256 = hex_hash(&bytes);
    let file_name = format!("{sha256}.{extension}");
    let relative_path = format!("{NOTES_FILE_DIR}/{file_name}");
    let path = active_notes_file_dir(vault_root).join(file_name);
    if !path.exists() {
        write_binary_file_atomically(&path, &bytes)?;
    }
    let original_name = original_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes file asset record: {e}"))?;
    assets::upsert_managed_asset_tx(
        &mut tx,
        ManagedAssetWrite {
            relative_path: &relative_path,
            original_name: original_name.as_deref(),
            content_type,
            byte_size: bytes.len() as i64,
            sha256: &sha256,
            source_type,
            storage_state: ASSET_STATE_AVAILABLE,
            missing_at: None,
        },
    )
    .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes file asset record: {e}"))?;
    Ok(NotesFileAssetDto {
        relative_path,
        original_name,
        content_type: content_type.to_string(),
        byte_size: bytes.len() as i64,
        sha256,
        kind: kind.as_str().to_string(),
    })
}

pub async fn save_selected_file(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    block_type: String,
    path: &Path,
) -> Result<NotesFileAssetDto, String> {
    let requested_kind = block_type_kind(&block_type)?;
    let original_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned);
    let bytes = read_file_capped(path)?;
    save_notes_file_bytes(
        pool,
        vault_root,
        requested_kind,
        path,
        bytes,
        original_name,
        ASSET_SOURCE_LOCAL_UPLOAD,
    )
    .await
}

pub async fn prepare_import_file_reference(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    request: NotesImportFileReferenceRequest,
) -> Result<NotesImportFileReferenceDto, String> {
    let import_context = request.import_context.trim();
    if !import_context_is_supported(import_context) {
        return Ok(import_blocked(
            "import_reference_context_unsupported",
            "Import file references must come from a supported Notes import context.",
        ));
    }
    let choice = request.choice.trim();
    if !import_choice_is_supported(choice) {
        return Ok(import_blocked(
            "import_reference_requires_explicit_choice",
            "Choose whether to copy, keep, or skip the import file reference.",
        ));
    }
    if choice == "skip" {
        return Ok(import_skipped());
    }
    let reference = request.reference.trim();
    if choice == "keep_external_reference" {
        return Ok(prepare_external_import_reference(
            reference,
            request.block_type.as_deref(),
        ));
    }
    if is_url_like_import_reference(reference) {
        return Ok(import_blocked(
            "import_reference_external_not_copied",
            "External and managed asset references are never copied during import. Keep them as explicit external references or skip them.",
        ));
    }
    let Some(import_root) = request.import_root.as_deref() else {
        return Ok(import_blocked(
            "import_reference_root_required",
            "Select an import folder before copying import files.",
        ));
    };
    let path = match resolve_import_candidate_path(import_root, reference) {
        Ok(path) => path,
        Err(diagnostic) => {
            return Ok(NotesImportFileReferenceDto {
                action: "blocked".to_string(),
                asset: None,
                external_url: None,
                diagnostics: vec![diagnostic],
            });
        }
    };
    let requested_kind = match request
        .block_type
        .as_deref()
        .map(str::trim)
        .filter(|block_type| !block_type.is_empty())
    {
        Some(block_type) => match block_type_kind(block_type) {
            Ok(kind) => kind,
            Err(error) => {
                return Ok(import_blocked(
                    "import_reference_block_type_unsupported",
                    error,
                ));
            }
        },
        None => NotesFileAssetKind::File,
    };
    let original_name = request.original_name.or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned)
    });
    let bytes = match read_file_capped(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(import_blocked(
                "import_reference_copy_failed",
                format!("Could not read the import file: {error}"),
            ));
        }
    };
    match save_notes_file_bytes(
        pool,
        vault_root,
        requested_kind,
        &path,
        bytes,
        original_name,
        ASSET_SOURCE_IMPORTED,
    )
    .await
    {
        Ok(asset) => Ok(NotesImportFileReferenceDto {
            action: "copied_asset".to_string(),
            asset: Some(asset),
            external_url: None,
            diagnostics: vec![import_diagnostic(
                "import_reference_copied",
                "info",
                "The import file was copied into managed local Notes assets.",
            )],
        }),
        Err(error) => Ok(import_blocked(
            "import_reference_copy_failed",
            format!("Could not copy the import file: {error}"),
        )),
    }
}

pub async fn file_asset_data_url(
    pool: &sqlx::SqlitePool,
    vault_root: &Path,
    relative_path: String,
) -> Result<String, String> {
    let path = asset_path_for_relative(vault_root, &relative_path)?;
    let asset: Option<(String, i64)> = sqlx::query_as(
        "SELECT content_type, byte_size
         FROM notes_assets
         WHERE asset_path = ? AND asset_path GLOB 'notes/files/*'",
    )
    .bind(relative_path.trim())
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("load notes file asset metadata: {e}"))?;
    let Some((content_type, byte_size)) = asset else {
        return Err("notes file asset not found".to_string());
    };
    let bytes = match read_preview_file_capped(&path, byte_size) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ =
                assets::mark_managed_asset_storage_state(pool, &relative_path, true, "notes file")
                    .await;
            return Err(error);
        }
    };
    assets::mark_managed_asset_storage_state(pool, &relative_path, false, "notes file").await?;
    Ok(format!(
        "data:{content_type};base64,{}",
        general_purpose::STANDARD.encode(bytes)
    ))
}

#[cfg(test)]
mod tests;

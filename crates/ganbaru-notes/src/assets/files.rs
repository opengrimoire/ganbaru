use base64::{Engine as _, engine::general_purpose};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};

use ganbaru_atomic_files::{FinalSymlink, read_regular_file_capped};

use crate::assets::{
    self, ASSET_SOURCE_IMPORTED, ASSET_SOURCE_LOCAL_UPLOAD, ASSET_STATE_AVAILABLE,
    ManagedAssetWrite,
};
use crate::blocking::run_blocking;

pub(crate) const NOTES_FILE_MAX_BYTES: usize =
    ganbaru_sync::domains::notes::MAX_FILE_BYTES as usize;
const NOTES_FILE_MAX_DISPLAY_MEGABYTES: usize = NOTES_FILE_MAX_BYTES / (1024 * 1024);
const NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES: i64 = 25;
const NOTES_FILE_PREVIEW_MAX_BYTES: i64 = NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES * 1024 * 1024;
const NOTES_FILE_DIR: &str = "notes/files";
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "oga", "m4a"];
const VIDEO_EXTENSIONS: &[&str] = &[
    "amv", "asf", "avi", "f4v", "flv", "gifv", "mkv", "mov", "mpg", "mpeg", "mpv", "mp4", "m4v",
    "qt", "wmv",
];

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

/// Reads a Notes source file through one handle. Picker choices follow a final link; import
/// references refuse one.
fn read_notes_file(path: &Path, final_symlink: FinalSymlink) -> io::Result<Vec<u8>> {
    read_regular_file_capped(path, NOTES_FILE_MAX_BYTES as u64, final_symlink)
}

fn notes_file_read_message(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::FileTooLarge => {
            format!("notes file exceeds the {NOTES_FILE_MAX_DISPLAY_MEGABYTES} MB limit")
        }
        io::ErrorKind::InvalidInput => "notes file path must be a regular file".to_string(),
        _ => format!("read notes file: {error}"),
    }
}

fn read_preview_file_capped(path: &Path, byte_size: i64) -> Result<Vec<u8>, String> {
    if byte_size > NOTES_FILE_PREVIEW_MAX_BYTES {
        return Err(format!(
            "notes file exceeds the {NOTES_FILE_PREVIEW_MAX_DISPLAY_MEGABYTES} MB preview limit"
        ));
    }
    let recorded_size = u64::try_from(byte_size)
        .map_err(|_| "notes file size no longer matches its recorded metadata".to_string())?;
    let bytes =
        read_regular_file_capped(path, recorded_size, FinalSymlink::Refuse).map_err(|error| {
            match error.kind() {
                io::ErrorKind::FileTooLarge => {
                    "notes file size no longer matches its recorded metadata".to_string()
                }
                io::ErrorKind::InvalidInput => "notes file path must be a regular file".to_string(),
                _ => format!("read notes file preview: {error}"),
            }
        })?;
    if bytes.len() as i64 != byte_size {
        return Err("notes file size no longer matches its recorded metadata".to_string());
    }
    Ok(bytes)
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
    .map_err(|e| format!("save notes file: {e}"))
}

pub(crate) fn hex_hash(bytes: &[u8]) -> String {
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
    let vault_root = vault_root.to_path_buf();
    let import_root = import_root.to_string_lossy().into_owned();
    let reference = reference.to_owned();
    let copied = run_blocking(move || {
        Ok(copy_import_file(
            &vault_root,
            &import_root,
            &reference,
            requested_kind,
        ))
    })
    .await;
    let (stored, file_name) = match copied {
        Ok(Ok(copied)) => copied,
        Ok(Err(blocked)) => return *blocked,
        Err(error) => {
            return import_blocked(
                "import_reference_copy_failed",
                format!("Could not copy the import file: {error}"),
            );
        }
    };
    match record_notes_file(
        pool,
        stored,
        original_name.or(file_name),
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

/// Resolve, read, and store one import file reference. Runs on a blocking worker and returns the
/// stored file with the source file name, or the blocked outcome to report.
fn copy_import_file(
    vault_root: &Path,
    import_root: &str,
    reference: &str,
    requested_kind: NotesFileAssetKind,
) -> Result<(StoredNotesFile, Option<String>), Box<NotesImportFileReferenceDto>> {
    let path = resolve_import_candidate_path(import_root, reference).map_err(|diagnostic| {
        Box::new(NotesImportFileReferenceDto {
            action: "blocked".to_string(),
            asset: None,
            external_url: None,
            diagnostics: vec![diagnostic],
        })
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned);
    let bytes = read_notes_file(&path, FinalSymlink::Refuse).map_err(|error| {
        Box::new(match error.kind() {
            io::ErrorKind::FileTooLarge => import_blocked(
                "import_reference_too_large",
                format!("Import files cannot exceed {NOTES_FILE_MAX_DISPLAY_MEGABYTES} MB."),
            ),
            io::ErrorKind::InvalidInput => import_blocked(
                "import_reference_not_file",
                "The import reference must point to a local file.",
            ),
            _ => import_blocked(
                "import_reference_copy_failed",
                format!("Could not read the import file: {error}"),
            ),
        })
    })?;
    let stored =
        store_notes_file_bytes(vault_root, requested_kind, &path, &bytes).map_err(|error| {
            Box::new(import_blocked(
                "import_reference_copy_failed",
                format!("Could not copy the import file: {error}"),
            ))
        })?;
    Ok((stored, file_name))
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
            let Some(content_type) = sniff_media_container(bytes).and_then(audio_content_type)
            else {
                return Err("local audio blocks require an audio file".to_string());
            };
            Ok((NotesFileAssetKind::Audio, content_type, extension))
        }
        NotesFileAssetKind::Video => {
            let Some(extension) = path_extension(path) else {
                return Err("local video blocks require a supported video extension".to_string());
            };
            if !VIDEO_EXTENSIONS.contains(&extension.as_str()) {
                return Err("local video blocks require a supported video file".to_string());
            }
            let Some(content_type) = sniff_media_container(bytes)
                .and_then(|container| video_content_type(container, &extension))
            else {
                return Err("local video blocks require a supported video file".to_string());
            };
            Ok((NotesFileAssetKind::Video, content_type, extension))
        }
        NotesFileAssetKind::File => {
            let extension = path_extension(path).unwrap_or_else(|| "bin".to_string());
            let (kind, content_type) = classify_generic_file(&extension, bytes);
            Ok((kind, content_type, extension))
        }
    }
}

/// Classifies a file block's bytes. A previewable kind needs both a matching extension and a
/// matching signature; other media and PDF extensions stay opaque downloads.
fn classify_generic_file(extension: &str, bytes: &[u8]) -> (NotesFileAssetKind, &'static str) {
    if IMAGE_EXTENSIONS.contains(&extension)
        && let Some(image_kind) = sniff_image_kind(bytes)
    {
        return (NotesFileAssetKind::Image, image_kind.content_type());
    }
    if extension == "pdf" && bytes.starts_with(b"%PDF-") {
        return (NotesFileAssetKind::Pdf, "application/pdf");
    }
    let container = sniff_media_container(bytes);
    if AUDIO_EXTENSIONS.contains(&extension)
        && let Some(content_type) = container.and_then(audio_content_type)
    {
        return (NotesFileAssetKind::Audio, content_type);
    }
    if VIDEO_EXTENSIONS.contains(&extension)
        && let Some(content_type) =
            container.and_then(|container| video_content_type(container, extension))
    {
        return (NotesFileAssetKind::Video, content_type);
    }
    (NotesFileAssetKind::File, generic_content_type(extension))
}

/// Audio and video container families recognized by their leading bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MediaContainer {
    MpegAudio,
    Wave,
    Ogg,
    IsoMedia,
    QuickTime,
    Avi,
    Amv,
    Matroska,
    Asf,
    Flv,
    MpegProgram,
    MpegTransport,
}

/// Leading zero bytes skipped before MPEG audio frame sync, which some encoders pad with.
const MPEG_AUDIO_MAX_LEADING_PADDING: usize = 4096;
const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9, 0x00, 0xaa, 0x00, 0x62, 0xce, 0x6c,
];
const QUICKTIME_LEADING_ATOMS: &[&[u8; 4]] =
    &[b"moov", b"mdat", b"wide", b"free", b"skip", b"pnot"];
const MPEG_TRANSPORT_PACKET_BYTES: usize = 188;

fn sniff_media_container(bytes: &[u8]) -> Option<MediaContainer> {
    let at = |offset: usize, signature: &[u8]| {
        bytes
            .get(offset..offset + signature.len())
            .is_some_and(|slice| slice == signature)
    };
    if at(0, b"ID3") || starts_with_mpeg_audio_frame(bytes) {
        return Some(MediaContainer::MpegAudio);
    }
    if (at(0, b"RIFF") || at(0, b"RF64")) && at(8, b"WAVE") {
        return Some(MediaContainer::Wave);
    }
    if at(0, b"RIFF") && at(8, b"AVI ") {
        return Some(MediaContainer::Avi);
    }
    if at(0, b"RIFF") && at(8, b"AMV ") {
        return Some(MediaContainer::Amv);
    }
    if at(0, b"OggS") {
        return Some(MediaContainer::Ogg);
    }
    if at(4, b"ftyp") {
        return Some(MediaContainer::IsoMedia);
    }
    if QUICKTIME_LEADING_ATOMS.iter().any(|atom| at(4, *atom)) {
        return Some(MediaContainer::QuickTime);
    }
    if at(0, &[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(MediaContainer::Matroska);
    }
    if at(0, &ASF_HEADER_GUID) {
        return Some(MediaContainer::Asf);
    }
    if at(0, b"FLV\x01") {
        return Some(MediaContainer::Flv);
    }
    if at(0, &[0x00, 0x00, 0x01, 0xba]) || at(0, &[0x00, 0x00, 0x01, 0xb3]) {
        return Some(MediaContainer::MpegProgram);
    }
    let transport_sync =
        |start: usize| at(start, &[0x47]) && at(start + MPEG_TRANSPORT_PACKET_BYTES, &[0x47]);
    if transport_sync(0) || transport_sync(4) {
        return Some(MediaContainer::MpegTransport);
    }
    None
}

/// Matches an MPEG audio or ADTS frame sync, with a valid version, after optional zero padding.
fn starts_with_mpeg_audio_frame(bytes: &[u8]) -> bool {
    let start = bytes
        .iter()
        .take(MPEG_AUDIO_MAX_LEADING_PADDING)
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    match bytes.get(start..start + 2) {
        Some([0xff, second]) => second & 0xe0 == 0xe0 && (second >> 3) & 0x03 != 0x01,
        _ => false,
    }
}

fn audio_content_type(container: MediaContainer) -> Option<&'static str> {
    match container {
        MediaContainer::MpegAudio => Some("audio/mpeg"),
        MediaContainer::Wave => Some("audio/wav"),
        MediaContainer::Ogg => Some("audio/ogg"),
        MediaContainer::IsoMedia | MediaContainer::QuickTime => Some("audio/mp4"),
        _ => None,
    }
}

fn video_content_type(container: MediaContainer, extension: &str) -> Option<&'static str> {
    match container {
        MediaContainer::IsoMedia if matches!(extension, "mov" | "qt") => Some("video/quicktime"),
        MediaContainer::IsoMedia => Some("video/mp4"),
        MediaContainer::QuickTime => Some("video/quicktime"),
        MediaContainer::Avi => Some("video/x-msvideo"),
        MediaContainer::Amv => Some("video/x-amv"),
        MediaContainer::Matroska => Some("video/x-matroska"),
        MediaContainer::Asf if extension == "wmv" => Some("video/x-ms-wmv"),
        MediaContainer::Asf => Some("video/x-ms-asf"),
        MediaContainer::Flv => Some("video/x-flv"),
        MediaContainer::MpegProgram => Some("video/mpeg"),
        MediaContainer::MpegTransport => Some("video/mp2t"),
        MediaContainer::MpegAudio | MediaContainer::Wave | MediaContainer::Ogg => None,
    }
}

/// Content type for a file block, which is never previewed. Document extensions keep their type;
/// SVG stays opaque because managed assets refuse active image types, and media and PDF
/// extensions without a matching signature stay opaque.
fn generic_content_type(extension: &str) -> &'static str {
    match extension {
        "json" => "application/json",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        _ => "application/octet-stream",
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

/// A Notes file written to the managed folder and ready to record.
#[derive(Debug)]
struct StoredNotesFile {
    kind: NotesFileAssetKind,
    content_type: &'static str,
    relative_path: String,
    byte_size: i64,
    sha256: String,
}

/// Classify, hash, and write file bytes into the managed Notes folder. Runs on a blocking worker.
///
/// The file is named `{sha256}.{ext}` with the extension sync derives from the content type, so
/// every device stores the same bytes under the same name.
fn store_notes_file_bytes(
    vault_root: &Path,
    requested_kind: NotesFileAssetKind,
    source_path: &Path,
    bytes: &[u8],
) -> Result<StoredNotesFile, String> {
    ensure_notes_file_size(bytes)?;
    let (kind, content_type, _) = classify_selected_file(requested_kind, source_path, bytes)?;
    let sha256 = hex_hash(bytes);
    let file_name = ganbaru_sync::assets::file_name(&sha256, content_type)
        .map_err(|error| format!("name notes file: {error}"))?;
    let relative_path = format!("{NOTES_FILE_DIR}/{file_name}");
    let path = active_notes_file_dir(vault_root).join(file_name);
    if !ganbaru_atomic_files::is_regular_file(&path) {
        write_binary_file_atomically(vault_root, &path, bytes)?;
    }
    Ok(StoredNotesFile {
        kind,
        content_type,
        relative_path,
        byte_size: bytes.len() as i64,
        sha256,
    })
}

async fn record_notes_file(
    pool: &sqlx::SqlitePool,
    stored: StoredNotesFile,
    original_name: Option<String>,
    source_type: &'static str,
) -> Result<NotesFileAssetDto, String> {
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
            relative_path: &stored.relative_path,
            original_name: original_name.as_deref(),
            content_type: stored.content_type,
            byte_size: stored.byte_size,
            sha256: &stored.sha256,
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
        relative_path: stored.relative_path,
        original_name,
        content_type: stored.content_type.to_string(),
        byte_size: stored.byte_size,
        sha256: stored.sha256,
        kind: stored.kind.as_str().to_string(),
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
    let vault_root = vault_root.to_path_buf();
    let path = path.to_path_buf();
    let stored = run_blocking(move || {
        let bytes = read_notes_file(&path, FinalSymlink::Follow)
            .map_err(|error| notes_file_read_message(&error))?;
        store_notes_file_bytes(&vault_root, requested_kind, &path, &bytes)
    })
    .await?;
    record_notes_file(pool, stored, original_name, ASSET_SOURCE_LOCAL_UPLOAD).await
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
    let encoded = run_blocking(move || {
        read_preview_file_capped(&path, byte_size)
            .map(|bytes| general_purpose::STANDARD.encode(bytes))
    })
    .await;
    let encoded = match encoded {
        Ok(encoded) => encoded,
        Err(error) => {
            let _ =
                assets::mark_managed_asset_storage_state(pool, &relative_path, true, "notes file")
                    .await;
            return Err(error);
        }
    };
    assets::mark_managed_asset_storage_state(pool, &relative_path, false, "notes file").await?;
    Ok(format!("data:{content_type};base64,{encoded}"))
}

#[cfg(test)]
mod tests;

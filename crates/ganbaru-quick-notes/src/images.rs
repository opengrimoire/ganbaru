//! Note images: ordered rows that reference content-addressed files under
//! [`IMAGES_DIR`], importing and reading those files, and removing files that nothing
//! references any more.
//!
//! A file is named by the lowercase hex SHA-256 of its bytes and the extension of its type, so
//! notes on every device share one copy of identical images and a name proves its contents.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ganbaru_image_metadata::{
    ManagedImageKind, ManagedImageMetadataError, parse_display_image_metadata,
    validate_image_dimensions,
};
use ganbaru_sync::Engine;
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::domains::quick_notes::{
    ImageError, NoteImage, decode_images, images_blob, validate_image, validate_images,
};
pub use ganbaru_sync::domains::quick_notes::{
    MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS, MAX_IMAGES,
};
use ganbaru_sync::local::retained_group_values;
use ganbaru_sync::manifest::vault::quick_notes::{NOTES_TABLE, note_group};
use ganbaru_sync_contracts::Value;
use ring::digest::{SHA256, digest};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool, Transaction};

/// File extensions an image picker offers.
pub const IMAGE_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "gif"];

/// Vault-relative directory of note image files.
pub const IMAGES_DIR: &str = "assets/quick-notes/images";

/// How long an unreferenced file survives. A file imported for a note that has not saved yet,
/// or one that a sync exchange just delivered, is unreferenced for a moment; the grace period
/// keeps cleanup from racing those writes.
const UNREFERENCED_GRACE: Duration = Duration::from_secs(24 * 60 * 60);

/// A note's reference to an image file, with the displayed dimensions for layout.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteImage {
    sha256: String,
    content_type: String,
    byte_size: u32,
    width: u32,
    height: u32,
}

impl From<NoteImage> for QuickNoteImage {
    fn from(image: NoteImage) -> Self {
        Self {
            sha256: image.sha256,
            content_type: image.content_type,
            byte_size: image.byte_size,
            width: image.width,
            height: image.height,
        }
    }
}

impl From<&QuickNoteImage> for NoteImage {
    fn from(image: &QuickNoteImage) -> Self {
        Self {
            sha256: image.sha256.clone(),
            content_type: image.content_type.clone(),
            byte_size: image.byte_size,
            width: image.width,
            height: image.height,
        }
    }
}

/// The file an image request names.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuickNoteImageFile {
    sha256: String,
    content_type: String,
}

impl QuickNoteImageFile {
    /// Builds a file reference from its digest and content type.
    pub fn new(sha256: String, content_type: String) -> Self {
        Self {
            sha256,
            content_type,
        }
    }

    /// Lowercase hex SHA-256 of the file.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Media type of the file, such as `image/png`.
    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    /// File name with the extension of the content type, such as `{sha256}.png`.
    pub fn file_name(&self) -> Result<String, QuickNoteImageError> {
        let kind = ManagedImageKind::from_mime_type(&self.content_type)
            .ok_or(QuickNoteImageError::Unsupported)?;
        if !is_digest(&self.sha256) {
            return Err(QuickNoteImageError::Failed(
                "quick note image digest is invalid".to_string(),
            ));
        }
        Ok(format!("{}.{}", self.sha256, kind.extension()))
    }
}

/// Stable image file outcomes, independent of diagnostic wording.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum QuickNoteImageError {
    /// The bytes are not a PNG, JPEG, WebP, or GIF image.
    Unsupported,
    /// The file exceeds the size, dimension, or pixel limits.
    TooLarge,
    /// The file is not on this device, or its bytes do not match its name.
    Unavailable,
    Failed(String),
}

impl From<String> for QuickNoteImageError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hex_digest(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The directory of image files in a vault.
pub fn images_dir(vault_root: &Path) -> PathBuf {
    vault_root.join(IMAGES_DIR)
}

/// The path of an image file in a vault, after checking the name it derives.
pub fn image_path(
    vault_root: &Path,
    file: &QuickNoteImageFile,
) -> Result<PathBuf, QuickNoteImageError> {
    Ok(images_dir(vault_root).join(file.file_name()?))
}

/// Checks an image list a note is saved with.
pub(crate) fn validate_note_images(images: &[QuickNoteImage]) -> Result<(), String> {
    let images = images.iter().map(NoteImage::from).collect::<Vec<_>>();
    validate_images(&images).map_err(|error| error.to_string())
}

/// Images of an images group value.
pub(crate) fn images_from_value(value: &Value) -> Result<Vec<QuickNoteImage>, String> {
    let bytes = images_blob(value).map_err(|error| error.to_string())?;
    Ok(decode_images(bytes)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(QuickNoteImage::from)
        .collect())
}

/// Replaces the image rows of a note.
pub(crate) async fn replace_images(
    tx: &mut Transaction<'_, Sqlite>,
    note_id: &str,
    images: &[QuickNoteImage],
) -> Result<(), String> {
    sqlx::query("DELETE FROM quick_note_images WHERE note_id = ?")
        .bind(note_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("clear quick note images: {error}"))?;
    for (sort_order, image) in images.iter().enumerate() {
        sqlx::query(
            "INSERT INTO quick_note_images (
                note_id, sort_order, sha256, content_type, byte_size, width, height
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(note_id)
        .bind(sort_order as i64)
        .bind(&image.sha256)
        .bind(&image.content_type)
        .bind(i64::from(image.byte_size))
        .bind(i64::from(image.width))
        .bind(i64::from(image.height))
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("insert quick note image: {error}"))?;
    }
    Ok(())
}

#[derive(Debug, FromRow)]
struct QuickNoteImageRow {
    note_id: String,
    sha256: String,
    content_type: String,
    byte_size: i64,
    width: i64,
    height: i64,
}

fn stored_u32(value: i64, label: &str) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| format!("stored quick note image {label} is invalid"))
}

fn image_from_row(row: QuickNoteImageRow) -> Result<(String, QuickNoteImage), String> {
    Ok((
        row.note_id,
        QuickNoteImage {
            sha256: row.sha256,
            content_type: row.content_type,
            byte_size: stored_u32(row.byte_size, "size")?,
            width: stored_u32(row.width, "width")?,
            height: stored_u32(row.height, "height")?,
        },
    ))
}

/// Image rows of the given notes, in note order.
pub(crate) async fn load_images(
    pool: &SqlitePool,
    note_ids: &[String],
) -> Result<HashMap<String, Vec<QuickNoteImage>>, String> {
    let mut by_note: HashMap<String, Vec<QuickNoteImage>> = HashMap::new();
    if note_ids.is_empty() {
        return Ok(by_note);
    }
    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT note_id, sha256, content_type, byte_size, width, height
         FROM quick_note_images WHERE note_id IN (",
    );
    {
        let mut separated = query.separated(", ");
        for id in note_ids {
            separated.push_bind(id);
        }
    }
    query.push(") ORDER BY note_id, sort_order");
    let rows = query
        .build_query_as::<QuickNoteImageRow>()
        .fetch_all(pool)
        .await
        .map_err(|error| format!("load quick note images: {error}"))?;
    for row in rows {
        let (note_id, image) = image_from_row(row)?;
        by_note.entry(note_id).or_default().push(image);
    }
    Ok(by_note)
}

/// Image rows of one note inside a transaction.
pub(crate) async fn load_note_images(
    tx: &mut Transaction<'_, Sqlite>,
    note_id: &str,
) -> Result<Vec<QuickNoteImage>, String> {
    sqlx::query_as::<_, QuickNoteImageRow>(
        "SELECT note_id, sha256, content_type, byte_size, width, height
         FROM quick_note_images WHERE note_id = ? ORDER BY sort_order",
    )
    .bind(note_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("load quick note images: {error}"))?
    .into_iter()
    .map(|row| image_from_row(row).map(|(_, image)| image))
    .collect()
}

fn metadata_error(error: ManagedImageMetadataError) -> QuickNoteImageError {
    match error {
        ManagedImageMetadataError::UnsupportedFormat => QuickNoteImageError::Unsupported,
        ManagedImageMetadataError::MalformedHeader(_) => QuickNoteImageError::Unsupported,
    }
}

fn too_large(length: usize) -> bool {
    u32::try_from(length).map_or(true, |length| length > MAX_IMAGE_BYTES)
}

/// Describes image bytes as a note reference after checking their type and limits.
pub fn describe_image(bytes: &[u8]) -> Result<QuickNoteImage, QuickNoteImageError> {
    if bytes.is_empty() {
        return Err(QuickNoteImageError::Unsupported);
    }
    if too_large(bytes.len()) {
        return Err(QuickNoteImageError::TooLarge);
    }
    let metadata = parse_display_image_metadata(bytes).map_err(metadata_error)?;
    validate_image_dimensions(metadata, MAX_IMAGE_DIMENSION, MAX_IMAGE_PIXELS)
        .map_err(|_| QuickNoteImageError::TooLarge)?;
    let image = NoteImage {
        sha256: hex_digest(bytes),
        content_type: metadata.kind.mime_type().to_string(),
        byte_size: bytes.len() as u32,
        width: metadata.width,
        height: metadata.height,
    };
    validate_image(&image).map_err(|error| match error {
        ImageError::Size | ImageError::Dimensions => QuickNoteImageError::TooLarge,
        other => QuickNoteImageError::Failed(other.to_string()),
    })?;
    Ok(image.into())
}

/// Stores image bytes in the vault and returns the reference a note saves.
///
/// An identical file that already exists is kept and its modification time refreshed, so
/// cleanup treats it as new for the grace period.
pub fn import_image(
    vault_root: &Path,
    bytes: &[u8],
) -> Result<QuickNoteImage, QuickNoteImageError> {
    let image = describe_image(bytes)?;
    let file = QuickNoteImageFile::new(image.sha256.clone(), image.content_type.clone());
    write_image_file(&images_dir(vault_root), &file.file_name()?, bytes)?;
    Ok(image)
}

fn io_failure(context: &str, error: std::io::Error) -> QuickNoteImageError {
    QuickNoteImageError::Failed(format!("{context}: {error}"))
}

fn temporary_suffix() -> Result<String, QuickNoteImageError> {
    let mut nonce = [0_u8; 8];
    SystemRandom::new().fill(&mut nonce).map_err(|_| {
        QuickNoteImageError::Failed("secure random generator is unavailable".into())
    })?;
    Ok(nonce.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Writes a content-addressed file through a synced temporary file and a rename, so a crash
/// never leaves a partial file under the final name.
fn write_image_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), QuickNoteImageError> {
    let path = dir.join(name);
    match fs::symlink_metadata(&path) {
        // A damaged file with the right length is replaced below like any other mismatch.
        Ok(existing)
            if existing.is_file()
                && existing.len() == bytes.len() as u64
                && fs::read(&path).is_ok_and(|current| current == bytes) =>
        {
            let file = fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .map_err(|error| io_failure("open quick note image", error))?;
            return file
                .set_modified(SystemTime::now())
                .map_err(|error| io_failure("refresh quick note image", error));
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(io_failure("inspect quick note image", error)),
    }
    fs::create_dir_all(dir).map_err(|error| io_failure("create quick note images", error))?;
    let temporary = dir.join(format!(".{name}.{}.tmp", temporary_suffix()?));
    let written = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)
    })();
    if let Err(error) = written {
        // The temporary file holds nothing a later write needs; cleanup also removes it.
        let _ = fs::remove_file(&temporary);
        return Err(io_failure("write quick note image", error));
    }
    sync_dir(dir)
}

/// Persists the rename. Windows cannot open a directory for syncing and orders the rename
/// itself, so only Unix syncs.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> Result<(), QuickNoteImageError> {
    fs::File::open(dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_failure("sync quick note images", error))
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> Result<(), QuickNoteImageError> {
    Ok(())
}

/// Reads at most the image size limit from a file the user picked.
pub fn read_image_source(path: &Path) -> Result<Vec<u8>, QuickNoteImageError> {
    let metadata = fs::metadata(path).map_err(|error| io_failure("inspect image", error))?;
    if !metadata.is_file() {
        return Err(QuickNoteImageError::Unsupported);
    }
    read_capped(path).map_err(|error| io_failure("read image", error))?
}

fn read_capped(path: &Path) -> std::io::Result<Result<Vec<u8>, QuickNoteImageError>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(u64::from(MAX_IMAGE_BYTES) + 1)
        .read_to_end(&mut bytes)?;
    Ok(if too_large(bytes.len()) {
        Err(QuickNoteImageError::TooLarge)
    } else {
        Ok(bytes)
    })
}

/// Reads an image file of the vault and checks that its bytes match its name.
pub fn read_image(
    vault_root: &Path,
    file: &QuickNoteImageFile,
) -> Result<Vec<u8>, QuickNoteImageError> {
    let path = image_path(vault_root, file)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(QuickNoteImageError::Unavailable),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(QuickNoteImageError::Unavailable);
        }
        Err(error) => return Err(io_failure("inspect quick note image", error)),
    }
    let bytes =
        match read_capped(&path).map_err(|error| io_failure("read quick note image", error))? {
            Ok(bytes) => bytes,
            Err(_) => return Err(QuickNoteImageError::Unavailable),
        };
    if hex_digest(&bytes) != file.sha256 {
        return Err(QuickNoteImageError::Unavailable);
    }
    Ok(bytes)
}

/// Checks a Quick note image that sync received, given the file that holds its bytes after
/// their size and digest matched the reference: the bytes must be a supported image within the
/// limits, of the content type the reference names. Sync registers it as the check of
/// [`ganbaru_sync::manifest::vault::quick_notes::IMAGES_CLASS`].
pub fn check_synced_image(asset: &AssetRef, path: &Path) -> Result<(), String> {
    let bytes = read_capped(path)
        .map_err(|error| format!("read synced quick note image: {error}"))?
        .map_err(|_| "the image exceeds the image size limit".to_string())?;
    let described = describe_image(&bytes)
        .map_err(|error| format!("the bytes are not a supported image: {error:?}"))?;
    if described.content_type != asset.content_type() {
        return Err(format!(
            "the bytes are {} but the reference names {}",
            described.content_type,
            asset.content_type()
        ));
    }
    Ok(())
}

/// Digests that a note, an open conflict, or a recovery offer still references.
async fn referenced_digests(pool: &SqlitePool) -> Result<HashSet<String>, String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("open quick note image references: {error}"))?;
    let mut referenced: HashSet<String> =
        sqlx::query_scalar("SELECT DISTINCT sha256 FROM quick_note_images")
            .fetch_all(&mut *conn)
            .await
            .map_err(|error| format!("load quick note image references: {error}"))?
            .into_iter()
            .collect();
    let retained =
        retained_group_values(&Engine::vault(), &mut conn, NOTES_TABLE, note_group::IMAGES)
            .await
            .map_err(|error| format!("load retained quick note images: {error}"))?;
    for value in retained {
        referenced.extend(
            images_from_value(&value)?
                .into_iter()
                .map(|image| image.sha256),
        );
    }
    Ok(referenced)
}

/// Deletes image files that nothing references and that are older than the grace period.
/// Returns how many files were deleted.
pub async fn collect_unreferenced_images(
    pool: &SqlitePool,
    vault_root: &Path,
) -> Result<u64, String> {
    collect_unreferenced_images_before(pool, vault_root, SystemTime::now()).await
}

pub(crate) async fn collect_unreferenced_images_before(
    pool: &SqlitePool,
    vault_root: &Path,
    now: SystemTime,
) -> Result<u64, String> {
    let referenced = referenced_digests(pool).await?;
    let dir = images_dir(vault_root);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(format!("list quick note images: {error}")),
    };
    let mut deleted = 0;
    for entry in entries {
        let entry = entry.map_err(|error| format!("list quick note images: {error}"))?;
        let name = entry.file_name();
        let digest = name
            .to_str()
            .and_then(|name| name.split_once('.'))
            .map(|(stem, _)| stem);
        if digest.is_some_and(|digest| referenced.contains(digest)) {
            continue;
        }
        // Only regular files are removed; links and directories are not the app's to delete.
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("inspect quick note image: {error}"))?;
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .map_err(|error| format!("inspect quick note image: {error}"))?;
        let age = now.duration_since(modified).unwrap_or_default();
        if age < UNREFERENCED_GRACE {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => deleted += 1,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("delete quick note image: {error}")),
        }
    }
    Ok(deleted)
}

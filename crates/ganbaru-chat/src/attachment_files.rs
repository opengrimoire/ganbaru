//! Content-addressed Chat attachment files: the `chat/attachments` asset class names every file
//! `{sha256}.{ext}` below `assets/chat/attachments`, so attachments with equal bytes share one
//! file and the sync asset store serves it unchanged.
//!
//! Chat never deletes an attachment file. A file stays while any attachment row or reference
//! names its digest, and the sync asset sweep removes it afterwards. Opening a vault renames
//! files written under older names to their digest name through [`convert`].

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_directories_within, read_regular_file_capped,
    remove_file_within, write_file_atomically,
};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Largest attachment in bytes, matching the asset class and the table checks.
pub const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Vault-relative directory of the attachment asset class.
pub const ATTACHMENT_DIRECTORY: &str = "assets/chat/attachments";

/// Extension of an attachment whose content type has no entry in [`ATTACHMENT_EXTENSIONS`],
/// matching the sync asset naming.
pub const UNKNOWN_EXTENSION: &str = "bin";

/// File extensions by content type, matching the sync asset naming for every type an
/// attachment can have.
pub const ATTACHMENT_EXTENSIONS: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/gif", "gif"),
    ("text/plain", "txt"),
];

/// Characters of a lowercase hex SHA-256 digest.
const SHA256_HEX_CHARS: usize = 64;

/// The file extension of an attachment of `content_type`.
pub fn extension(content_type: &str) -> &'static str {
    ATTACHMENT_EXTENSIONS
        .iter()
        .find(|(known, _)| *known == content_type)
        .map_or(UNKNOWN_EXTENSION, |(_, extension)| extension)
}

/// Image content types an attachment of kind `image` may have, in the order imports test them.
pub const IMAGE_CONTENT_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Content type of every attachment of kind `text_snippet`.
pub const TEXT_CONTENT_TYPE: &str = "text/plain";

/// Whether `bytes` carry the file signature of `content_type`: the image magic number of an
/// image type, or UTF-8 without NUL characters for text. An unsupported type never matches.
pub fn content_type_matches(content_type: &str, bytes: &[u8]) -> bool {
    match content_type {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        TEXT_CONTENT_TYPE => !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok(),
        _ => false,
    }
}

/// Checks a received attachment file of `content_type` at `path` before sync stores it: the
/// bytes must fit the attachment limit and carry the signature of their content type.
///
/// # Errors
///
/// Returns a description when the file cannot be read, is too large, or does not match.
pub fn check_received_file(content_type: &str, path: &Path) -> Result<(), String> {
    let bytes = read_regular_file_capped(path, MAX_ATTACHMENT_BYTES, FinalSymlink::Refuse)
        .map_err(|error| format!("read received Chat attachment: {error}"))?;
    if content_type_matches(content_type, &bytes) {
        Ok(())
    } else {
        Err(format!(
            "the received Chat attachment bytes are not {content_type}"
        ))
    }
}

/// The signature kind an import records for an attachment of `content_type`, which is the one
/// a matching file signature implies. An unknown type has no signature and records `unknown`.
pub fn signature_kind(content_type: &str) -> &'static str {
    match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpeg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "text/plain" => "utf8",
        _ => "unknown",
    }
}

/// The vault-relative path of the attachment file with `sha256` and `content_type`.
pub fn relative_path(sha256: &str, content_type: &str) -> String {
    format!(
        "{ATTACHMENT_DIRECTORY}/{sha256}.{}",
        extension(content_type)
    )
}

/// The lowercase hex SHA-256 digest of `bytes`.
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Whether `value` is a lowercase hex SHA-256 digest.
pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == SHA256_HEX_CHARS
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The absolute path of an attachment file, refusing a path outside the attachment directory
/// or one with anything but plain segments.
pub fn resolve(vault_root: &Path, relative_path: &str) -> io::Result<PathBuf> {
    let relative = Path::new(relative_path);
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    if !plain || !relative_path.starts_with(&format!("{ATTACHMENT_DIRECTORY}/")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Chat attachment path is outside the attachment directory",
        ));
    }
    Ok(vault_root.join(relative))
}

/// Whether the file at `path` holds exactly `byte_size` bytes with digest `sha256`. A missing,
/// oversized, linked, or unreadable file does not match.
pub fn file_matches(path: &Path, sha256: &str, byte_size: u64) -> bool {
    read_regular_file_capped(
        path,
        byte_size.min(MAX_ATTACHMENT_BYTES),
        FinalSymlink::Refuse,
    )
    .is_ok_and(|bytes| bytes.len() as u64 == byte_size && digest(&bytes) == sha256)
}

/// Stores `bytes` as the attachment file with `sha256` and `content_type` and returns its
/// vault-relative path. An existing file is kept when its bytes match the digest; otherwise the
/// file is replaced atomically. Empty bytes need no file, so nothing is written for them.
///
/// # Errors
///
/// Returns an error when the digest does not match the bytes, a directory below the vault root
/// is a symlink, or the write fails.
pub fn store(
    vault_root: &Path,
    sha256: &str,
    content_type: &str,
    bytes: &[u8],
) -> io::Result<String> {
    if digest(bytes) != sha256 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Chat attachment bytes do not match their digest",
        ));
    }
    let relative = relative_path(sha256, content_type);
    if bytes.is_empty() {
        return Ok(relative);
    }
    let path = resolve(vault_root, &relative)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Chat attachment has no folder")
    })?;
    create_directories_within(vault_root, parent)?;
    if !file_matches(&path, sha256, bytes.len() as u64) {
        write_file_atomically(&path, bytes, FileAccess::OwnerOnly)?;
    }
    Ok(relative)
}

/// One attachment row whose file name is not its digest name.
struct Unconverted {
    id: String,
    sha256: String,
    mime_type: String,
    byte_size: u64,
    managed_relative_path: String,
}

/// Renames every attachment file still under an older name to its digest name and rewrites the
/// `managed_relative_path` of its attachment and resource rows, returning the rows rewritten.
///
/// Each row is converted on its own and every step can repeat, so a crash at any point leaves
/// a state the next run finishes: the digest file is put in place first (renaming the old file
/// over it, or removing the old file when the digest file already matches), and the rows move
/// to the digest path after. A row whose file is missing or damaged still moves to the digest
/// path, so the file can arrive later; a damaged old file is left where it is. A vault that is
/// already converted is left unchanged.
///
/// # Errors
///
/// Returns an error when the rows cannot be read or written, or a file cannot be moved.
pub async fn convert(pool: &SqlitePool, vault_root: &Path) -> Result<u64, String> {
    let rows = sqlx::query(
        "SELECT id, sha256, mime_type, byte_size, managed_relative_path FROM chat_attachments
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| format!("read Chat attachments: {error}"))?;
    let mut converted = 0;
    for row in rows {
        let read = |column: &str| {
            row.try_get::<String, _>(column)
                .map_err(|error| format!("read Chat attachment {column}: {error}"))
        };
        let attachment = Unconverted {
            id: read("id")?,
            sha256: read("sha256")?,
            mime_type: read("mime_type")?,
            byte_size: row
                .try_get::<i64, _>("byte_size")
                .ok()
                .and_then(|size| u64::try_from(size).ok())
                .ok_or_else(|| "read Chat attachment byte_size".to_string())?,
            managed_relative_path: read("managed_relative_path")?,
        };
        let target = relative_path(&attachment.sha256, &attachment.mime_type);
        if attachment.managed_relative_path == target {
            continue;
        }
        move_file(vault_root, &attachment, &target)
            .map_err(|error| format!("convert Chat attachment {}: {error}", attachment.id))?;
        let mut transaction = pool
            .begin()
            .await
            .map_err(|error| format!("convert Chat attachment: {error}"))?;
        for statement in [
            "UPDATE chat_attachments SET managed_relative_path = ? WHERE id = ?",
            "UPDATE chat_resources SET managed_relative_path = ? WHERE attachment_id = ?",
        ] {
            sqlx::query(statement)
                .bind(&target)
                .bind(&attachment.id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| format!("convert Chat attachment {}: {error}", attachment.id))?;
        }
        transaction
            .commit()
            .await
            .map_err(|error| format!("convert Chat attachment: {error}"))?;
        converted += 1;
    }
    Ok(converted)
}

/// Puts the bytes of `attachment` at `target`, the digest path, from its old file.
fn move_file(vault_root: &Path, attachment: &Unconverted, target: &str) -> io::Result<()> {
    let target = resolve(vault_root, target)?;
    let Ok(old) = resolve(vault_root, &attachment.managed_relative_path) else {
        return Ok(());
    };
    let target_matches = attachment.byte_size == 0
        || file_matches(&target, &attachment.sha256, attachment.byte_size);
    let old_matches = file_matches(&old, &attachment.sha256, attachment.byte_size);
    if target_matches {
        if old_matches {
            remove_file_within(vault_root, &old)?;
        }
        return Ok(());
    }
    if !old_matches {
        return Ok(());
    }
    let parent = target.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Chat attachment has no folder")
    })?;
    create_directories_within(vault_root, parent)?;
    rename_to_digest(&old, &target, attachment)
}

/// Renames `old` to `target`. A missing `old` succeeds when `target` already holds the digest,
/// since a concurrent conversion moved the same file first.
fn rename_to_digest(old: &Path, target: &Path, attachment: &Unconverted) -> io::Result<()> {
    match fs::rename(old, target) {
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && file_matches(target, &attachment.sha256, attachment.byte_size) =>
        {
            Ok(())
        }
        result => result,
    }
}

/// Checks that every attachment row names its digest path.
///
/// # Errors
///
/// Returns an error naming the count of attachments that [`convert`] has not renamed.
pub async fn assert_converted(pool: &SqlitePool) -> Result<(), String> {
    let rows = sqlx::query("SELECT sha256, mime_type, managed_relative_path FROM chat_attachments")
        .fetch_all(pool)
        .await
        .map_err(|error| format!("read Chat attachments: {error}"))?;
    let mut remaining = 0_u64;
    for row in rows {
        let read = |column: &str| {
            row.try_get::<String, _>(column)
                .map_err(|error| format!("read Chat attachment {column}: {error}"))
        };
        if read("managed_relative_path")? != relative_path(&read("sha256")?, &read("mime_type")?) {
            remaining += 1;
        }
    }
    if remaining == 0 {
        Ok(())
    } else {
        Err(format!(
            "{remaining} Chat attachments still name a file that is not content addressed"
        ))
    }
}

#[cfg(test)]
mod tests;

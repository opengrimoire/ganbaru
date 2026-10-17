use crate::models::{
    ChatAttachmentId, ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, UtcTimestamp,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;
const CHAT_ATTACHMENT_DIRECTORY: &str = "assets/chat/attachments";
const CHAT_BROWSER_ARTIFACT_DIRECTORY: &str = "assets/chat/browser-artifacts";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatAttachmentKind {
    Image,
    TextSnippet,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachmentRead {
    pub id: ChatAttachmentId,
    pub working_folder_id: ProjectWorkingFolderId,
    pub kind: ChatAttachmentKind,
    pub original_display_name: String,
    pub mime_type: String,
    pub byte_size: u64,
    pub sha256: String,
    pub managed_relative_path: String,
    pub signature_kind: String,
    pub created_at: UtcTimestamp,
}

pub struct AttachmentBytesImport<'a> {
    pub working_folder_id: &'a ProjectWorkingFolderId,
    pub attachment_id: ChatAttachmentId,
    pub display_name: String,
    pub bytes: &'a [u8],
    pub requested_kind: ChatAttachmentKind,
    pub now: &'a UtcTimestamp,
}

pub async fn import_attachment(
    pool: &SqlitePool,
    vault_root: &Path,
    working_folder_id: &ProjectWorkingFolderId,
    attachment_id: ChatAttachmentId,
    source_path: &Path,
    requested_kind: ChatAttachmentKind,
    now: &UtcTimestamp,
) -> ChatResult<ChatAttachmentRead> {
    let metadata = fs::symlink_metadata(source_path).map_err(io_error)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_ATTACHMENT_BYTES
    {
        return Err(ChatError::validation(
            "attachment",
            "Attachment is not a supported bounded file",
        ));
    }
    let display_name = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty() && value.len() <= 1000)
        .ok_or_else(|| ChatError::validation("attachment.name", "Attachment name is invalid"))?
        .to_string();
    let bytes = fs::read(source_path).map_err(io_error)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(ChatError::validation(
            "attachment",
            "Attachment changed during import",
        ));
    }
    persist_attachment_bytes(
        pool,
        vault_root,
        AttachmentBytesImport {
            working_folder_id,
            attachment_id,
            display_name,
            bytes: &bytes,
            requested_kind,
            now,
        },
    )
    .await
}

pub async fn import_attachment_bytes(
    pool: &SqlitePool,
    vault_root: &Path,
    request: AttachmentBytesImport<'_>,
) -> ChatResult<ChatAttachmentRead> {
    if request.bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(invalid_attachment_error());
    }
    let display_name = request.display_name.trim();
    if display_name.is_empty()
        || display_name.len() > 1_000
        || display_name.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "attachment.name",
            "Attachment name is invalid",
        ));
    }
    persist_attachment_bytes(
        pool,
        vault_root,
        AttachmentBytesImport {
            display_name: display_name.to_string(),
            ..request
        },
    )
    .await
}

async fn persist_attachment_bytes(
    pool: &SqlitePool,
    vault_root: &Path,
    request: AttachmentBytesImport<'_>,
) -> ChatResult<ChatAttachmentRead> {
    let (mime_type, signature_kind, extension) =
        inspect_signature(request.bytes, request.requested_kind)?;
    let sha256 = format!("{:x}", Sha256::digest(request.bytes));
    let relative_path = format!(
        "{CHAT_ATTACHMENT_DIRECTORY}/{}-{}.{}",
        &sha256[..16],
        safe_file_segment(request.attachment_id.as_str()),
        extension
    );
    let destination = resolve_managed_path(vault_root, &relative_path)?;
    let parent = destination.parent().ok_or_else(invalid_path_error)?;
    fs::create_dir_all(parent).map_err(io_error)?;
    write_restrictive(&destination, request.bytes)?;
    let inserted = sqlx::query(
        "INSERT INTO chat_attachments
            (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
             sha256, managed_relative_path, signature_kind, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(request.attachment_id.as_str())
    .bind(request.working_folder_id.as_str())
    .bind(wire_kind(request.requested_kind))
    .bind(&request.display_name)
    .bind(mime_type)
    .bind(i64::try_from(request.bytes.len()).map_err(|_| invalid_attachment_error())?)
    .bind(&sha256)
    .bind(&relative_path)
    .bind(signature_kind)
    .bind(request.now.as_str())
    .execute(pool)
    .await;
    if let Err(error) = inserted {
        let _ = fs::remove_file(&destination);
        return Err(persistence_error(error));
    }
    read_attachment(pool, &request.attachment_id)
        .await?
        .ok_or_else(corrupt_data_error)
}

pub async fn read_attachment(
    pool: &SqlitePool,
    attachment_id: &ChatAttachmentId,
) -> ChatResult<Option<ChatAttachmentRead>> {
    let row = sqlx::query(
        "SELECT id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                sha256, managed_relative_path, signature_kind, created_at
         FROM chat_attachments WHERE id = ? AND deletion_state != 'deleted'",
    )
    .bind(attachment_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    row.map(|row| {
        Ok(ChatAttachmentRead {
            id: ChatAttachmentId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
                .map_err(|_| corrupt_data_error())?,
            working_folder_id: ProjectWorkingFolderId::new(
                row.try_get::<String, _>("working_folder_id")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            kind: parse_kind(
                &row.try_get::<String, _>("kind")
                    .map_err(persistence_error)?,
            )?,
            original_display_name: row
                .try_get("original_display_name")
                .map_err(persistence_error)?,
            mime_type: row.try_get("mime_type").map_err(persistence_error)?,
            byte_size: u64::try_from(
                row.try_get::<i64, _>("byte_size")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
            sha256: row.try_get("sha256").map_err(persistence_error)?,
            managed_relative_path: row
                .try_get("managed_relative_path")
                .map_err(persistence_error)?,
            signature_kind: row.try_get("signature_kind").map_err(persistence_error)?,
            created_at: UtcTimestamp::new(
                row.try_get::<String, _>("created_at")
                    .map_err(persistence_error)?,
            )
            .map_err(|_| corrupt_data_error())?,
        })
    })
    .transpose()
}

pub fn read_managed_attachment_bytes(
    vault_root: &Path,
    attachment: &ChatAttachmentRead,
) -> ChatResult<(PathBuf, Vec<u8>)> {
    let path = resolve_managed_path(vault_root, &attachment.managed_relative_path)?;
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != attachment.byte_size
        || metadata.len() > MAX_ATTACHMENT_BYTES
    {
        return Err(invalid_attachment_error());
    }
    let bytes = fs::read(&path).map_err(io_error)?;
    if format!("{:x}", Sha256::digest(&bytes)) != attachment.sha256 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Managed Chat attachment changed after import",
            true,
        ));
    }
    Ok((path, bytes))
}

pub async fn run_due_attachment_cleanup(
    pool: &SqlitePool,
    vault_root: &Path,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let rows = sqlx::query(
        "SELECT id, exact_target FROM chat_cleanup_jobs
         WHERE cleanup_kind = 'attachment_file'
           AND state IN ('pending', 'failed') AND available_at <= ?
         ORDER BY available_at, id LIMIT 100",
    )
    .bind(now.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut completed = 0;
    for row in rows {
        let cleanup_id: String = row.try_get("id").map_err(persistence_error)?;
        let relative_path: String = row.try_get("exact_target").map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_cleanup_jobs SET state = 'running', attempt_count = attempt_count + 1,
                    updated_at = ? WHERE id = ? AND state IN ('pending', 'failed')",
        )
        .bind(now.as_str())
        .bind(&cleanup_id)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
        let still_referenced: bool = sqlx::query_scalar(
            "SELECT EXISTS(
                SELECT 1 FROM chat_attachments a
                WHERE a.managed_relative_path = ? AND (
                    EXISTS (SELECT 1 FROM chat_attachment_references r WHERE r.attachment_id = a.id)
                    OR EXISTS (SELECT 1 FROM chat_queued_attachment_references q WHERE q.attachment_id = a.id)
                    OR EXISTS (SELECT 1 FROM chat_communication_attachment_references c WHERE c.attachment_id = a.id)
                    OR EXISTS (SELECT 1 FROM chat_scheduled_message_attachment_references s WHERE s.attachment_id = a.id)
                )
                UNION ALL
                SELECT 1 FROM chat_resources resource
                WHERE resource.managed_relative_path = ?
                  AND EXISTS (
                      SELECT 1 FROM chat_resource_thread_references reference
                      WHERE reference.resource_id = resource.id
                  )
             )",
        )
        .bind(&relative_path)
        .bind(&relative_path)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        if still_referenced {
            sqlx::query(
                "UPDATE chat_cleanup_jobs SET state = 'completed', last_error_code = NULL,
                        updated_at = ? WHERE id = ?",
            )
            .bind(now.as_str())
            .bind(&cleanup_id)
            .execute(pool)
            .await
            .map_err(persistence_error)?;
            completed += 1;
            continue;
        }
        let result = resolve_managed_chat_path(vault_root, &relative_path).and_then(|path| {
            match fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(io_error(error)),
            }
        });
        match result {
            Ok(()) => {
                let mut transaction = pool.begin().await.map_err(persistence_error)?;
                sqlx::query(
                    "UPDATE chat_attachments SET deletion_state = 'deleted', deleted_at = ?
                     WHERE managed_relative_path = ?
                       AND NOT EXISTS (SELECT 1 FROM chat_attachment_references r WHERE r.attachment_id = chat_attachments.id)
                       AND NOT EXISTS (SELECT 1 FROM chat_queued_attachment_references q WHERE q.attachment_id = chat_attachments.id)
                       AND NOT EXISTS (SELECT 1 FROM chat_communication_attachment_references c WHERE c.attachment_id = chat_attachments.id)
                       AND NOT EXISTS (SELECT 1 FROM chat_scheduled_message_attachment_references s WHERE s.attachment_id = chat_attachments.id)",
                )
                .bind(now.as_str())
                .bind(&relative_path)
                .execute(&mut *transaction)
                .await
                .map_err(persistence_error)?;
                sqlx::query(
                    "UPDATE chat_resources SET integrity_state = 'deleted', deleted_at = ?
                     WHERE managed_relative_path = ?
                       AND NOT EXISTS (
                           SELECT 1 FROM chat_resource_thread_references reference
                           WHERE reference.resource_id = chat_resources.id
                       )",
                )
                .bind(now.as_str())
                .bind(&relative_path)
                .execute(&mut *transaction)
                .await
                .map_err(persistence_error)?;
                sqlx::query(
                    "UPDATE chat_cleanup_jobs SET state = 'completed', last_error_code = NULL,
                            updated_at = ? WHERE id = ?",
                )
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(&mut *transaction)
                .await
                .map_err(persistence_error)?;
                transaction.commit().await.map_err(persistence_error)?;
                completed += 1;
            }
            Err(_) => {
                sqlx::query(
                    "UPDATE chat_cleanup_jobs SET state = 'failed', last_error_code = 'io',
                            updated_at = ? WHERE id = ?",
                )
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(pool)
                .await
                .map_err(persistence_error)?;
            }
        }
    }
    Ok(completed)
}

fn inspect_signature(
    bytes: &[u8],
    kind: ChatAttachmentKind,
) -> ChatResult<(&'static str, &'static str, &'static str)> {
    match kind {
        ChatAttachmentKind::Image if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => {
            Ok(("image/png", "png", "png"))
        }
        ChatAttachmentKind::Image if bytes.starts_with(&[0xff, 0xd8, 0xff]) => {
            Ok(("image/jpeg", "jpeg", "jpg"))
        }
        ChatAttachmentKind::Image
            if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") =>
        {
            Ok(("image/gif", "gif", "gif"))
        }
        ChatAttachmentKind::Image
            if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" =>
        {
            Ok(("image/webp", "webp", "webp"))
        }
        ChatAttachmentKind::TextSnippet
            if bytes.len() <= MAX_ATTACHMENT_BYTES as usize
                && !bytes.contains(&0)
                && std::str::from_utf8(bytes).is_ok() =>
        {
            Ok(("text/plain", "utf8", "txt"))
        }
        _ => Err(ChatError::validation(
            "attachment.signature",
            "Attachment signature does not match a supported type",
        )),
    }
}

pub(super) fn resolve_managed_path(vault_root: &Path, relative_path: &str) -> ChatResult<PathBuf> {
    if !relative_path.starts_with(&format!("{CHAT_ATTACHMENT_DIRECTORY}/")) {
        return Err(invalid_path_error());
    }
    resolve_managed_chat_path(vault_root, relative_path)
}

pub(super) fn resolve_managed_chat_path(
    vault_root: &Path,
    relative_path: &str,
) -> ChatResult<PathBuf> {
    let relative = Path::new(relative_path);
    let managed_directory = relative_path.starts_with(&format!("{CHAT_ATTACHMENT_DIRECTORY}/"))
        || relative_path.starts_with(&format!("{CHAT_BROWSER_ARTIFACT_DIRECTORY}/"));
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || !managed_directory
    {
        return Err(invalid_path_error());
    }
    Ok(vault_root.join(relative))
}

fn safe_file_segment(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

fn write_restrictive(path: &Path, bytes: &[u8]) -> ChatResult<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn wire_kind(value: ChatAttachmentKind) -> &'static str {
    match value {
        ChatAttachmentKind::Image => "image",
        ChatAttachmentKind::TextSnippet => "text_snippet",
    }
}
fn parse_kind(value: &str) -> ChatResult<ChatAttachmentKind> {
    match value {
        "image" => Ok(ChatAttachmentKind::Image),
        "text_snippet" => Ok(ChatAttachmentKind::TextSnippet),
        _ => Err(corrupt_data_error()),
    }
}
fn invalid_attachment_error() -> ChatError {
    ChatError::validation("attachment", "Attachment is too large")
}
fn invalid_path_error() -> ChatError {
    ChatError::validation("attachment.path", "Managed attachment path is invalid")
}
fn io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Managed Chat attachment file operation failed",
        true,
    )
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat attachment persistence failed",
        true,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat attachment is invalid",
        false,
    )
}

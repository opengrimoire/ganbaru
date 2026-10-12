//! Scratch scope projections, bounded directory browsing, and artifact revisions.

use super::{
    ScratchIdentity, parse_generation_lifecycle, parse_id, parse_scope_lifecycle,
    read_scratch_identity, require_local_owner, require_scratch_inspection_authority,
};
use crate::chat::channels::{chat_pool, persistence_error, timestamp, u64_value};
use crate::chat::scratch;
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::path::{Component, Path};

const MAX_RELATIVE_PATH_BYTES: usize = 4_096;
const DEFAULT_DIRECTORY_PAGE_SIZE: u32 = 20;
const MAX_DIRECTORY_PAGE_SIZE: u32 = 50;
const MAX_PROMOTION_BYTES: u64 = 50 * 1024 * 1024;
const MAX_PAGE_REVISION_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SIZE_ENTRIES: u64 = 10_000;
const MAX_SIZE_BYTES: u64 = 1024 * 1024 * 1024;

#[tauri::command]
pub async fn chat_list_scratch_scopes(
    app: tauri::AppHandle,
    db_url: String,
    teammate_id: Option<ChatParticipantId>,
    reply_thread_id: Option<ChatReplyThreadId>,
) -> ChatResult<Vec<ChatScratchScopeRead>> {
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let rows = sqlx::query(
        "SELECT scope.id, scope.reply_thread_id, scope.teammate_id,
                teammate.display_name AS teammate_name,
                channel.id AS channel_id, channel.name AS channel_name,
                project.id AS project_id, project.name AS project_name,
                project_group.id AS group_id, project_group.name AS group_name,
                scope.lifecycle_state, scope.revision, scope.created_at, scope.updated_at
         FROM chat_scratch_scopes scope
         JOIN chat_participants teammate ON teammate.id = scope.teammate_id
         JOIN chat_reply_threads thread ON thread.id = scope.reply_thread_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         JOIN projects project ON project.id = channel.project_id
         JOIN project_groups project_group ON project_group.id = project.group_id
         WHERE scope.removed_at IS NULL
           AND (? IS NULL OR scope.teammate_id = ?)
           AND (? IS NULL OR scope.reply_thread_id = ?)
         ORDER BY scope.updated_at DESC, scope.id",
    )
    .bind(teammate_id.as_ref().map(ChatParticipantId::as_str))
    .bind(teammate_id.as_ref().map(ChatParticipantId::as_str))
    .bind(reply_thread_id.as_ref().map(ChatReplyThreadId::as_str))
    .bind(reply_thread_id.as_ref().map(ChatReplyThreadId::as_str))
    .fetch_all(&pool)
    .await
    .map_err(persistence_error)?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let scope_id =
            parse_id::<ChatScratchScopeId>(row.try_get("id").map_err(persistence_error)?)?;
        result.push(ChatScratchScopeRead {
            generations: read_scope_generations(&app, &pool, &scope_id).await?,
            id: scope_id,
            reply_thread_id: parse_id(row.try_get("reply_thread_id").map_err(persistence_error)?)?,
            teammate_id: parse_id(row.try_get("teammate_id").map_err(persistence_error)?)?,
            teammate_name: row.try_get("teammate_name").map_err(persistence_error)?,
            channel_id: parse_id(row.try_get("channel_id").map_err(persistence_error)?)?,
            channel_name: row.try_get("channel_name").map_err(persistence_error)?,
            project_id: row.try_get("project_id").map_err(persistence_error)?,
            project_name: row.try_get("project_name").map_err(persistence_error)?,
            group_id: row.try_get("group_id").map_err(persistence_error)?,
            group_name: row.try_get("group_name").map_err(persistence_error)?,
            lifecycle_state: parse_scope_lifecycle(
                &row.try_get::<String, _>("lifecycle_state")
                    .map_err(persistence_error)?,
            )?,
            revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
            created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
            updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn chat_browse_scratch_generation(
    app: tauri::AppHandle,
    db_url: String,
    request: BrowseChatScratchGenerationCommand,
) -> ChatResult<ChatScratchDirectoryPageRead> {
    validate_optional_relative_path(&request.relative_path)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let identity = read_scratch_identity(&pool, &request.scratch_generation_id).await?;
    require_scratch_inspection_authority(&pool, &identity, request.allow_restricted_inspection)
        .await?;
    let root = scratch::resolve_managed_scratch_path_for_inspection(
        &app,
        &pool,
        identity.generation_id.as_str(),
        identity.execution_environment_id.as_str(),
    )
    .await?;
    let mut entries = read_directory_entries(&root, &request.relative_path)?;
    entries.sort_by(|left, right| {
        entry_kind_rank(left.kind)
            .cmp(&entry_kind_rank(right.kind))
            .then_with(|| {
                left.display_name
                    .to_lowercase()
                    .cmp(&right.display_name.to_lowercase())
            })
            .then_with(|| left.display_name.cmp(&right.display_name))
    });
    let start = cursor_start(&entries, request.cursor.as_deref())?;
    let requested_limit = if request.limit == 0 {
        DEFAULT_DIRECTORY_PAGE_SIZE
    } else {
        request.limit.min(MAX_DIRECTORY_PAGE_SIZE)
    };
    let limit = usize::try_from(requested_limit).unwrap_or(DEFAULT_DIRECTORY_PAGE_SIZE as usize);
    let end = start.saturating_add(limit).min(entries.len());
    let next_cursor =
        (end < entries.len()).then(|| directory_cursor(&entries[end - 1].relative_path));
    let mut page_entries: Vec<_> = entries.drain(start..end).collect();
    add_page_content_revisions(&root, &mut page_entries)?;
    Ok(ChatScratchDirectoryPageRead {
        scratch_generation_id: request.scratch_generation_id,
        relative_path: request.relative_path,
        entries: page_entries,
        next_cursor,
    })
}

async fn read_scope_generations(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    scope_id: &ChatScratchScopeId,
) -> ChatResult<Vec<ChatScratchGenerationRead>> {
    let rows = sqlx::query(
        "SELECT generation.id, generation.generation, generation.lifecycle_state,
                generation.byte_size, generation.created_at, generation.updated_at,
                environment.id AS execution_environment_id
         FROM chat_scratch_generations generation
         JOIN chat_execution_environments environment
           ON environment.scratch_generation_id = generation.id
          AND environment.kind = 'scratch'
         WHERE generation.scratch_scope_id = ? AND generation.removed_at IS NULL
         ORDER BY generation.generation DESC",
    )
    .bind(scope_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let generation_id: ChatScratchGenerationId =
            parse_id(row.try_get("id").map_err(persistence_error)?)?;
        let execution_environment_id: ChatExecutionEnvironmentId = parse_id(
            row.try_get("execution_environment_id")
                .map_err(persistence_error)?,
        )?;
        let stored_bytes = u64_value(row.try_get("byte_size").map_err(persistence_error)?)?;
        let stored_size = scratch::BoundedScratchSize {
            bytes: stored_bytes.min(MAX_SIZE_BYTES),
            entries: 0,
            truncated: stored_bytes > MAX_SIZE_BYTES,
        };
        let lifecycle_state = parse_generation_lifecycle(
            &row.try_get::<String, _>("lifecycle_state")
                .map_err(persistence_error)?,
        )?;
        let (device_availability, size) = if matches!(
            lifecycle_state,
            ChatScratchGenerationLifecycleState::Active
                | ChatScratchGenerationLifecycleState::Quarantined
                | ChatScratchGenerationLifecycleState::CleanupFailed
        ) {
            match scratch::resolve_managed_scratch_path_for_inspection(
                app,
                pool,
                generation_id.as_str(),
                execution_environment_id.as_str(),
            )
            .await
            {
                Ok(path) => {
                    match scratch::bounded_scratch_size(&path, MAX_SIZE_ENTRIES, MAX_SIZE_BYTES) {
                        Ok(size) => (ChatScratchDeviceAvailability::Available, size),
                        Err(_) => (
                            ChatScratchDeviceAvailability::UnavailableOnThisDevice,
                            stored_size,
                        ),
                    }
                }
                Err(_) => (
                    ChatScratchDeviceAvailability::UnavailableOnThisDevice,
                    stored_size,
                ),
            }
        } else {
            (
                ChatScratchDeviceAvailability::UnavailableOnThisDevice,
                stored_size,
            )
        };
        result.push(ChatScratchGenerationRead {
            id: generation_id.clone(),
            execution_environment_id,
            generation: u64_value(row.try_get("generation").map_err(persistence_error)?)?,
            lifecycle_state,
            byte_size: size.bytes,
            entry_count: size.entries,
            size_truncated: size.truncated,
            device_availability,
            retained_sources: read_generation_sources(pool, &generation_id).await?,
            created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
            updated_at: timestamp(row.try_get("updated_at").map_err(persistence_error)?)?,
        });
    }
    Ok(result)
}

async fn read_generation_sources(
    pool: &SqlitePool,
    generation_id: &ChatScratchGenerationId,
) -> ChatResult<Vec<ChatScratchSourceSummaryRead>> {
    let rows = sqlx::query(
        "SELECT channel.id AS channel_id, channel.name AS channel_name,
                source.lower_ordinal, source.high_ordinal, source.audience_revision
         FROM chat_scratch_generation_sources source
         JOIN chat_channels channel ON channel.conversation_id = source.conversation_id
         WHERE source.scratch_generation_id = ?
         ORDER BY channel.name COLLATE NOCASE, channel.id",
    )
    .bind(generation_id.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(ChatScratchSourceSummaryRead {
                channel_id: parse_id(row.try_get("channel_id").map_err(persistence_error)?)?,
                channel_name: row.try_get("channel_name").map_err(persistence_error)?,
                lower_ordinal: u64_value(row.try_get("lower_ordinal").map_err(persistence_error)?)?,
                high_ordinal: u64_value(row.try_get("high_ordinal").map_err(persistence_error)?)?,
                audience_revision: u64_value(
                    row.try_get("audience_revision")
                        .map_err(persistence_error)?,
                )?,
            })
        })
        .collect()
}

fn read_directory_entries(
    root: &Path,
    relative_path: &str,
) -> ChatResult<Vec<ChatScratchDirectoryEntryRead>> {
    let (items, truncated) =
        crate::chat::workspace::files::list_managed_artifact_directory(root, relative_path)?;
    if truncated {
        return Err(ChatError::validation(
            "relativePath",
            "Scratch directory is too large to browse safely",
        ));
    }
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let display_name = item.display_name;
        if display_name.is_empty() || display_name.chars().any(char::is_control) {
            return Err(scratch_path_error());
        }
        let child_relative = if relative_path.is_empty() {
            display_name.clone()
        } else {
            format!("{relative_path}/{display_name}")
        };
        validate_required_relative_path(&child_relative)?;
        if item.directory {
            entries.push(ChatScratchDirectoryEntryRead {
                relative_path: child_relative,
                display_name,
                kind: ChatScratchDirectoryEntryKind::Directory,
                byte_size: None,
                content_revision: None,
                promotable: false,
            });
        } else if let Some(byte_size) = item.byte_size {
            entries.push(ChatScratchDirectoryEntryRead {
                relative_path: child_relative,
                display_name,
                kind: ChatScratchDirectoryEntryKind::File,
                byte_size: Some(byte_size),
                promotable: false,
                content_revision: None,
            });
        }
    }
    Ok(entries)
}

fn add_page_content_revisions(
    root: &Path,
    entries: &mut [ChatScratchDirectoryEntryRead],
) -> ChatResult<()> {
    let mut revision_budget = MAX_PAGE_REVISION_BYTES;
    for entry in entries {
        if entry.kind != ChatScratchDirectoryEntryKind::File {
            continue;
        }
        let byte_size = entry.byte_size.unwrap_or(u64::MAX);
        if byte_size > MAX_PROMOTION_BYTES || byte_size > revision_budget {
            continue;
        }
        let file = read_scratch_file(root, &entry.relative_path, None)?;
        revision_budget = revision_budget.saturating_sub(byte_size);
        entry.content_revision = Some(file.content_revision);
        entry.promotable = true;
    }
    Ok(())
}

pub(super) fn read_scratch_file(
    root: &Path,
    relative_path: &str,
    expected_revision: Option<&str>,
) -> ChatResult<ScratchFile> {
    validate_required_relative_path(relative_path)?;
    let bytes = crate::chat::workspace::files::read_managed_artifact_bytes(root, relative_path)?;
    let content_revision = scratch_file_revision(relative_path, &bytes);
    if expected_revision.is_some_and(|expected| expected != content_revision) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Scratch artifact changed before promotion",
            true,
        ));
    }
    Ok(ScratchFile {
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        content_revision,
        bytes,
    })
}

fn cursor_start(
    entries: &[ChatScratchDirectoryEntryRead],
    cursor: Option<&str>,
) -> ChatResult<usize> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    entries
        .iter()
        .position(|entry| directory_cursor(&entry.relative_path) == cursor)
        .map(|index| index + 1)
        .ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::StaleRevision,
                "Scratch directory changed before the next page was read",
                true,
            )
        })
}

pub(super) async fn read_generation_size(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    identity: &ScratchIdentity,
) -> ChatResult<(ChatScratchDeviceAvailability, scratch::BoundedScratchSize)> {
    let stored_bytes = u64_value(
        sqlx::query_scalar("SELECT byte_size FROM chat_scratch_generations WHERE id = ?")
            .bind(identity.generation_id.as_str())
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?,
    )?;
    let unavailable = || {
        (
            ChatScratchDeviceAvailability::UnavailableOnThisDevice,
            scratch::BoundedScratchSize {
                bytes: stored_bytes.min(MAX_SIZE_BYTES),
                entries: 0,
                truncated: stored_bytes > MAX_SIZE_BYTES,
            },
        )
    };
    let Ok(path) = scratch::resolve_managed_scratch_path_for_inspection(
        app,
        pool,
        identity.generation_id.as_str(),
        identity.execution_environment_id.as_str(),
    )
    .await
    else {
        return Ok(unavailable());
    };
    Ok(
        match scratch::bounded_scratch_size(&path, MAX_SIZE_ENTRIES, MAX_SIZE_BYTES) {
            Ok(size) => (ChatScratchDeviceAvailability::Available, size),
            Err(_) => unavailable(),
        },
    )
}

fn validate_optional_relative_path(value: &str) -> ChatResult<()> {
    if value.is_empty() {
        Ok(())
    } else {
        validate_required_relative_path(value)
    }
}

pub(super) fn validate_required_relative_path(value: &str) -> ChatResult<()> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > MAX_RELATIVE_PATH_BYTES
        || path.is_absolute()
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(ChatError::validation(
            "relativePath",
            "Scratch paths must be normalized relative paths",
        ));
    }
    Ok(())
}

fn scratch_file_revision(relative_path: &str, bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"scratch-file-v1\0");
    digest.update(relative_path.as_bytes());
    digest.update(b"\0");
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

fn directory_cursor(relative_path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"scratch-directory-cursor-v1\0");
    digest.update(relative_path.as_bytes());
    format!("{:x}", digest.finalize())
}

const fn entry_kind_rank(kind: ChatScratchDirectoryEntryKind) -> u8 {
    match kind {
        ChatScratchDirectoryEntryKind::Directory => 0,
        ChatScratchDirectoryEntryKind::File => 1,
    }
}

fn scratch_path_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Private scratch path is unavailable or symbolic",
        false,
    )
}

pub(super) struct ScratchFile {
    pub(super) bytes: Vec<u8>,
    pub(super) content_revision: String,
    pub(super) sha256: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_cursors_resume_after_the_entry_and_reject_missing_entries() {
        let entries: Vec<_> = ["first.txt", "last.txt"]
            .into_iter()
            .map(|name| ChatScratchDirectoryEntryRead {
                relative_path: name.to_string(),
                display_name: name.to_string(),
                kind: ChatScratchDirectoryEntryKind::File,
                byte_size: Some(0),
                content_revision: None,
                promotable: false,
            })
            .collect();
        assert_eq!(cursor_start(&entries, None).unwrap(), 0);
        assert_eq!(
            cursor_start(&entries, Some(&directory_cursor("first.txt"))).unwrap(),
            1
        );
        assert_eq!(
            cursor_start(&entries, Some(&directory_cursor("last.txt"))).unwrap(),
            entries.len()
        );
        let missing = cursor_start(&entries, Some(&directory_cursor("removed.txt"))).unwrap_err();
        assert_eq!(missing.code, ChatErrorCode::StaleRevision);
        assert!(cursor_start(&[], Some(&directory_cursor("first.txt"))).is_err());
    }

    #[test]
    fn scratch_relative_paths_reject_traversal_and_platform_separators() {
        assert!(validate_required_relative_path("artifacts/report.txt").is_ok());
        assert!(validate_required_relative_path("../outside").is_err());
        assert!(validate_required_relative_path("artifacts/../outside").is_err());
        assert!(validate_required_relative_path("artifacts\\outside").is_err());
        assert!(validate_required_relative_path("/outside").is_err());
    }

    #[test]
    fn revisions_bind_the_relative_path_and_bytes() {
        let first = scratch_file_revision("one.txt", b"same");
        let second = scratch_file_revision("two.txt", b"same");
        let changed = scratch_file_revision("one.txt", b"changed");
        assert_ne!(first, second);
        assert_ne!(first, changed);
        assert_eq!(first.len(), 64);
    }
}

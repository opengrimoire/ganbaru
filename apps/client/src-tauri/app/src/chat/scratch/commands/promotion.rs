//! Explicit artifact promotion with durable request identity and destination checks.

use super::inspection::{read_scratch_file, validate_required_relative_path};
use super::{
    ScratchIdentity, corrupt_scratch_error, parse_id, read_scratch_identity,
    require_current_scratch_authority, require_local_owner,
};
use crate::chat::channels::{chat_pool, now_timestamp, persistence_error, timestamp};
use crate::chat::scratch;
use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use crate::vault;
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat::repository::attachments::{self, AttachmentBytesImport, ChatAttachmentKind};
use ganbaru_chat_contracts::models::*;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};

#[tauri::command]
pub async fn chat_promote_scratch_file(
    app: tauri::AppHandle,
    db_url: String,
    request: PromoteChatScratchFileCommand,
) -> ChatResult<ChatScratchPromotionResultRead> {
    validate_required_relative_path(&request.source_relative_path)?;
    validate_content_revision(&request.expected_content_revision)?;
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let identity = read_scratch_identity(&pool, &request.scratch_generation_id).await?;
    if identity.generation_lifecycle != ChatScratchGenerationLifecycleState::Active {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Only an active scratch generation can promote artifacts",
            false,
        ));
    }
    require_current_scratch_authority(&pool, &identity).await?;
    let channel_id = match &request.destination {
        ChatScratchPromotionDestinationInput::WorkingFolder { channel_id, .. }
        | ChatScratchPromotionDestinationInput::ManagedAttachment { channel_id, .. } => channel_id,
    };
    let attachment_storage_folder =
        require_destination_channel(&pool, &identity, channel_id).await?;
    let (audited_working_folder_id, destination_kind, destination_path, attachment_id) =
        match &request.destination {
            ChatScratchPromotionDestinationInput::WorkingFolder {
                working_folder_id,
                relative_path,
                ..
            } => {
                validate_required_relative_path(relative_path)?;
                require_edit_folder_grant(&pool, &identity, channel_id, working_folder_id).await?;
                (
                    Some(working_folder_id),
                    "working_folder",
                    Some(relative_path.as_str()),
                    None,
                )
            }
            ChatScratchPromotionDestinationInput::ManagedAttachment {
                attachment_id,
                display_name,
                ..
            } => {
                validate_attachment_display_name(display_name)?;
                (
                    None,
                    "managed_attachment",
                    None,
                    Some(attachment_id.as_str()),
                )
            }
        };
    let request_digest = scratch_promotion_request_digest(&request);
    if scratch_promotion_exists(&pool, &request.promotion_id).await? {
        return read_idempotent_promotion(&pool, &request.promotion_id, &request_digest).await;
    }
    let root = scratch::resolve_managed_scratch_path_for_inspection(
        &app,
        &pool,
        identity.generation_id.as_str(),
        identity.execution_environment_id.as_str(),
    )
    .await?;
    let source = read_scratch_file(
        &root,
        &request.source_relative_path,
        Some(&request.expected_content_revision),
    )?;
    let attachment_kind = match &request.destination {
        ChatScratchPromotionDestinationInput::ManagedAttachment { .. } => {
            Some(supported_attachment_kind(&source.bytes)?)
        }
        ChatScratchPromotionDestinationInput::WorkingFolder { .. } => None,
    };
    let authorized = match &request.destination {
        ChatScratchPromotionDestinationInput::WorkingFolder {
            working_folder_id, ..
        } => Some(
            crate::chat::workspace::commands::authorize_working_folder(
                &app,
                &pool,
                working_folder_id,
                WorkingFolderAuthorizationOperation::FileWrite,
            )
            .await?,
        ),
        ChatScratchPromotionDestinationInput::ManagedAttachment { .. } => None,
    };
    let now = now_timestamp()?;
    let inserted = sqlx::query(
        "INSERT INTO chat_scratch_promotions
            (id, scratch_generation_id, source_relative_path, source_content_revision,
             request_digest, destination_kind, destination_channel_id,
             destination_working_folder_id, destination_relative_path, attachment_id,
             state, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(request.promotion_id.as_str())
    .bind(identity.generation_id.as_str())
    .bind(&request.source_relative_path)
    .bind(&source.content_revision)
    .bind(&request_digest)
    .bind(destination_kind)
    .bind(channel_id.as_str())
    .bind(
        audited_working_folder_id
            .as_ref()
            .map(|working_folder_id| working_folder_id.as_str()),
    )
    .bind(destination_path)
    .bind(attachment_id)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if inserted.rows_affected() == 0 {
        return read_idempotent_promotion(&pool, &request.promotion_id, &request_digest).await;
    }
    if let Err(error) = require_current_scratch_authority(&pool, &identity).await {
        mark_promotion_failed(&pool, &request.promotion_id, "authorization", &now).await?;
        return Err(error);
    }
    if let Err(error) = require_destination_channel(&pool, &identity, channel_id).await {
        mark_promotion_failed(&pool, &request.promotion_id, "authorization", &now).await?;
        return Err(error);
    }
    let destination = match &request.destination {
        ChatScratchPromotionDestinationInput::WorkingFolder {
            working_folder_id,
            relative_path,
            ..
        } => {
            if let Err(error) =
                require_edit_folder_grant(&pool, &identity, channel_id, working_folder_id).await
            {
                mark_promotion_failed(&pool, &request.promotion_id, "authorization", &now).await?;
                return Err(error);
            }
            let authorized = authorized.as_ref().ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Internal,
                    "Scratch folder promotion lost its authorized destination",
                    false,
                )
            })?;
            if let Err(error) = crate::chat::workspace::files::create_workspace_artifact(
                authorized,
                relative_path,
                &source.bytes,
            ) {
                mark_promotion_failed(&pool, &request.promotion_id, "destination_write", &now)
                    .await?;
                return Err(error);
            }
            ChatScratchPromotionDestinationRead::WorkingFolder {
                working_folder_id: working_folder_id.clone(),
                relative_path: relative_path.clone(),
            }
        }
        ChatScratchPromotionDestinationInput::ManagedAttachment {
            attachment_id,
            display_name,
            ..
        } => {
            let kind = attachment_kind.ok_or_else(|| {
                ChatError::new(
                    ChatErrorCode::Internal,
                    "Scratch attachment promotion lost its validated artifact kind",
                    false,
                )
            })?;
            let vault_root = vault::active_writable_vault_path(&app).map_err(vault_error)?;
            if let Err(error) = attachments::import_attachment_bytes(
                &pool,
                &vault_root,
                AttachmentBytesImport {
                    working_folder_id: &attachment_storage_folder,
                    attachment_id: attachment_id.clone(),
                    display_name: display_name.clone(),
                    bytes: &source.bytes,
                    requested_kind: kind,
                    now: &now,
                },
            )
            .await
            {
                mark_promotion_failed(&pool, &request.promotion_id, "destination_write", &now)
                    .await?;
                return Err(error);
            }
            ChatScratchPromotionDestinationRead::ManagedAttachment {
                channel_id: channel_id.clone(),
                attachment_id: attachment_id.clone(),
            }
        }
    };
    let completed = now_timestamp()?;
    let completion = sqlx::query(
        "UPDATE chat_scratch_promotions
         SET source_sha256 = ?, state = 'completed', last_error_code = NULL,
             updated_at = ?, completed_at = ?
         WHERE id = ? AND state = 'pending'",
    )
    .bind(&source.sha256)
    .bind(completed.as_str())
    .bind(completed.as_str())
    .bind(request.promotion_id.as_str())
    .execute(&pool)
    .await
    .map_err(persistence_error)?;
    if completion.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Scratch promotion state changed before completion",
            true,
        ));
    }
    Ok(ChatScratchPromotionResultRead {
        id: request.promotion_id,
        scratch_generation_id: request.scratch_generation_id,
        source_relative_path: request.source_relative_path,
        source_sha256: source.sha256,
        destination,
        created_at: now,
    })
}

/// Resolves the destination channel and its project-managed attachment index.
///
/// The returned working folder is only a storage association for the existing
/// attachment table. It is not a folder grant and must never be used to
/// authorize project file access.
async fn require_destination_channel(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
    channel_id: &ChatChannelId,
) -> ChatResult<ProjectWorkingFolderId> {
    let storage_folder_id: String = sqlx::query_scalar(
        "SELECT folder.id
         FROM chat_channels channel
         JOIN project_working_folders folder
           ON folder.project_id = channel.project_id
          AND folder.kind = 'managed'
          AND folder.archived_at IS NULL
         WHERE channel.id = ? AND channel.conversation_id = ?
           AND channel.archived_at IS NULL",
    )
    .bind(channel_id.as_str())
    .bind(identity.conversation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Permission,
            "The scratch destination channel is no longer available",
            false,
        )
    })?;
    parse_id(storage_folder_id)
}

async fn require_edit_folder_grant(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
    channel_id: &ChatChannelId,
    working_folder_id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT grant_row.capability, revision.maximum_folder_capability,
                grant_row.capability_inherits_profile
         FROM chat_channels channel
         JOIN chat_conversation_memberships membership
           ON membership.conversation_id = channel.conversation_id
          AND membership.participant_id = ?
         JOIN chat_teammate_channel_memberships access
           ON access.conversation_id = membership.conversation_id
          AND access.teammate_id = membership.participant_id
         JOIN chat_access_profiles profile ON profile.id = access.access_profile_id
         JOIN chat_access_profile_revisions revision
           ON revision.access_profile_id = profile.id
          AND revision.revision = profile.latest_revision
         JOIN chat_teammate_working_folder_grants grant_row
           ON grant_row.conversation_id = membership.conversation_id
          AND grant_row.teammate_id = membership.participant_id
          AND grant_row.working_folder_id = ?
         JOIN project_working_folders folder ON folder.id = grant_row.working_folder_id
         WHERE channel.id = ? AND channel.conversation_id = ?
           AND channel.archived_at IS NULL AND membership.removed_at IS NULL
           AND grant_row.revoked_at IS NULL AND folder.archived_at IS NULL
           AND folder.project_id = channel.project_id",
    )
    .bind(identity.teammate_id.as_str())
    .bind(working_folder_id.as_str())
    .bind(channel_id.as_str())
    .bind(identity.conversation_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(|| {
        ChatError::new(
            ChatErrorCode::Permission,
            "The destination folder is not granted in this channel",
            false,
        )
    })?;
    let grant_rank = folder_capability_rank(
        &row.try_get::<String, _>("capability")
            .map_err(persistence_error)?,
    )?;
    let profile_rank = folder_capability_rank(
        &row.try_get::<String, _>("maximum_folder_capability")
            .map_err(persistence_error)?,
    )?;
    let effective_rank = if row
        .try_get::<i64, _>("capability_inherits_profile")
        .map_err(persistence_error)?
        != 0
    {
        profile_rank
    } else {
        grant_rank.min(profile_rank)
    };
    if effective_rank < 2 {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Scratch promotion requires current edit access to the destination folder",
            false,
        ));
    }
    Ok(())
}

async fn mark_promotion_failed(
    pool: &SqlitePool,
    promotion_id: &ChatScratchPromotionId,
    error_code: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "UPDATE chat_scratch_promotions
         SET state = 'failed', last_error_code = ?, updated_at = ?
         WHERE id = ? AND state = 'pending'",
    )
    .bind(error_code)
    .bind(now.as_str())
    .bind(promotion_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn read_idempotent_promotion(
    pool: &SqlitePool,
    promotion_id: &ChatScratchPromotionId,
    request_digest: &str,
) -> ChatResult<ChatScratchPromotionResultRead> {
    let row = sqlx::query(
        "SELECT scratch_generation_id, source_relative_path, source_sha256,
                request_digest, destination_kind, destination_channel_id,
                destination_working_folder_id, destination_relative_path,
                attachment_id, state, created_at
         FROM chat_scratch_promotions WHERE id = ?",
    )
    .bind(promotion_id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(corrupt_scratch_error)?;
    let stored_digest: Option<String> = row.try_get("request_digest").map_err(persistence_error)?;
    if stored_digest.as_deref() != Some(request_digest) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Scratch promotion ID is already bound to another request",
            false,
        ));
    }
    let state: String = row.try_get("state").map_err(persistence_error)?;
    if state != "completed" {
        let (message, retryable) = if state == "pending" {
            ("Scratch promotion is still in progress", true)
        } else {
            ("Scratch promotion already failed", false)
        };
        return Err(ChatError::new(ChatErrorCode::Conflict, message, retryable));
    }
    let destination_kind: String = row.try_get("destination_kind").map_err(persistence_error)?;
    let destination = match destination_kind.as_str() {
        "working_folder" => ChatScratchPromotionDestinationRead::WorkingFolder {
            working_folder_id: parse_id(
                row.try_get("destination_working_folder_id")
                    .map_err(persistence_error)?,
            )?,
            relative_path: row
                .try_get("destination_relative_path")
                .map_err(persistence_error)?,
        },
        "managed_attachment" => ChatScratchPromotionDestinationRead::ManagedAttachment {
            channel_id: parse_id(
                row.try_get("destination_channel_id")
                    .map_err(persistence_error)?,
            )?,
            attachment_id: parse_id(row.try_get("attachment_id").map_err(persistence_error)?)?,
        },
        _ => return Err(corrupt_scratch_error()),
    };
    Ok(ChatScratchPromotionResultRead {
        id: promotion_id.clone(),
        scratch_generation_id: parse_id(
            row.try_get("scratch_generation_id")
                .map_err(persistence_error)?,
        )?,
        source_relative_path: row
            .try_get("source_relative_path")
            .map_err(persistence_error)?,
        source_sha256: row
            .try_get::<Option<String>, _>("source_sha256")
            .map_err(persistence_error)?
            .ok_or_else(corrupt_scratch_error)?,
        destination,
        created_at: timestamp(row.try_get("created_at").map_err(persistence_error)?)?,
    })
}

async fn scratch_promotion_exists(
    pool: &SqlitePool,
    promotion_id: &ChatScratchPromotionId,
) -> ChatResult<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_scratch_promotions WHERE id = ?)")
        .bind(promotion_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)
}

fn scratch_promotion_request_digest(request: &PromoteChatScratchFileCommand) -> String {
    let mut digest = Sha256::new();
    digest.update(b"scratch-promotion-v1\0");
    update_digest_field(&mut digest, request.scratch_generation_id.as_str());
    update_digest_field(&mut digest, &request.source_relative_path);
    update_digest_field(&mut digest, &request.expected_content_revision);
    match &request.destination {
        ChatScratchPromotionDestinationInput::WorkingFolder {
            channel_id,
            working_folder_id,
            relative_path,
        } => {
            update_digest_field(&mut digest, "working_folder");
            update_digest_field(&mut digest, channel_id.as_str());
            update_digest_field(&mut digest, working_folder_id.as_str());
            update_digest_field(&mut digest, relative_path);
        }
        ChatScratchPromotionDestinationInput::ManagedAttachment {
            channel_id,
            attachment_id,
            display_name,
        } => {
            update_digest_field(&mut digest, "managed_attachment");
            update_digest_field(&mut digest, channel_id.as_str());
            update_digest_field(&mut digest, attachment_id.as_str());
            update_digest_field(&mut digest, display_name.trim());
        }
    }
    format!("{:x}", digest.finalize())
}

fn update_digest_field(digest: &mut Sha256, value: &str) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value.as_bytes());
}

fn supported_attachment_kind(bytes: &[u8]) -> ChatResult<ChatAttachmentKind> {
    if !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        return Ok(ChatAttachmentKind::TextSnippet);
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
    {
        return Ok(ChatAttachmentKind::Image);
    }
    Err(ChatError::validation(
        "destination",
        "Managed Chat attachments currently accept images and UTF-8 text artifacts",
    ))
}

fn validate_attachment_display_name(value: &str) -> ChatResult<()> {
    let value = value.trim();
    if value.is_empty() || value.len() > 1_000 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "displayName",
            "Attachment display name is invalid",
        ));
    }
    Ok(())
}

fn validate_content_revision(value: &str) -> ChatResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ChatError::validation(
            "expectedContentRevision",
            "Scratch content revision is invalid",
        ));
    }
    Ok(())
}

fn folder_capability_rank(value: &str) -> ChatResult<u8> {
    match value {
        "none" => Ok(0),
        "read" => Ok(1),
        "edit" => Ok(2),
        "execute" => Ok(3),
        "publish" => Ok(4),
        _ => Err(corrupt_scratch_error()),
    }
}

fn vault_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "The active vault path is unavailable",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_attachment_detection_is_closed_world() {
        assert!(matches!(
            supported_attachment_kind(b"text"),
            Ok(ChatAttachmentKind::TextSnippet)
        ));
        assert!(matches!(
            supported_attachment_kind(b"\x89PNG\r\n\x1a\nrest"),
            Ok(ChatAttachmentKind::Image)
        ));
        assert!(supported_attachment_kind(&[0, 1, 2, 3]).is_err());
    }

    #[test]
    fn promotion_idempotency_digest_binds_the_channel_and_exact_destination() {
        let request = PromoteChatScratchFileCommand {
            promotion_id: ChatScratchPromotionId::new("promotion:test").unwrap(),
            scratch_generation_id: ChatScratchGenerationId::new("generation:test").unwrap(),
            source_relative_path: "reports/result.md".to_string(),
            expected_content_revision: "a".repeat(64),
            destination: ChatScratchPromotionDestinationInput::ManagedAttachment {
                channel_id: ChatChannelId::new("channel:general").unwrap(),
                attachment_id: ChatAttachmentId::new("attachment:result").unwrap(),
                display_name: "result.md".to_string(),
            },
        };
        let other_channel = PromoteChatScratchFileCommand {
            promotion_id: ChatScratchPromotionId::new("promotion:test").unwrap(),
            scratch_generation_id: ChatScratchGenerationId::new("generation:test").unwrap(),
            source_relative_path: "reports/result.md".to_string(),
            expected_content_revision: "a".repeat(64),
            destination: ChatScratchPromotionDestinationInput::ManagedAttachment {
                channel_id: ChatChannelId::new("channel:private").unwrap(),
                attachment_id: ChatAttachmentId::new("attachment:result").unwrap(),
                display_name: "result.md".to_string(),
            },
        };
        let other_name = PromoteChatScratchFileCommand {
            promotion_id: ChatScratchPromotionId::new("promotion:test").unwrap(),
            scratch_generation_id: ChatScratchGenerationId::new("generation:test").unwrap(),
            source_relative_path: "reports/result.md".to_string(),
            expected_content_revision: "a".repeat(64),
            destination: ChatScratchPromotionDestinationInput::ManagedAttachment {
                channel_id: ChatChannelId::new("channel:general").unwrap(),
                attachment_id: ChatAttachmentId::new("attachment:result").unwrap(),
                display_name: "renamed.md".to_string(),
            },
        };

        let digest = scratch_promotion_request_digest(&request);
        assert_eq!(digest.len(), 64);
        assert_ne!(digest, scratch_promotion_request_digest(&other_channel));
        assert_ne!(digest, scratch_promotion_request_digest(&other_name));
    }
}

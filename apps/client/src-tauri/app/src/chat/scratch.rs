//! Device-local private scratch targets for organizational assignments.

pub(crate) mod commands;

use super::device_state::{read_active_device_scope, update_active_device_scope};
use super::workspace::AuthorizedWorkingFolder;
use crate::vault;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId, RepositoryKind,
};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;

const SCRATCH_DIRECTORY: &str = "chat-scratch";
const CONVERSATION_RUNTIME_DIRECTORY: &str = "chat-conversation-runtime";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BoundedScratchSize {
    pub(crate) bytes: u64,
    pub(crate) entries: u64,
    pub(crate) truncated: bool,
}

/// Authorizes a neutral, application-managed provider workspace for a
/// conversation assignment that has no organizational execution target.
pub(crate) fn authorize_conversation_runtime(
    app: &tauri::AppHandle,
    thread_id: &ChatThreadId,
) -> ChatResult<AuthorizedWorkingFolder> {
    let local_root = app
        .path()
        .app_local_data_dir()
        .map_err(device_state_error)?;
    let vault_id = vault::active_vault_id(app).map_err(device_state_error)?;
    let runtime_root = local_root
        .join(CONVERSATION_RUNTIME_DIRECTORY)
        .join(hex_digest(&vault_id, 24))
        .join(hex_digest(thread_id.as_str(), 32));
    prepare_managed_directory(&local_root, CONVERSATION_RUNTIME_DIRECTORY, &runtime_root)?;
    let synthetic_id = ProjectWorkingFolderId::new(format!(
        "conversation-runtime:{}",
        hex_digest(thread_id.as_str(), 32)
    ))
    .map_err(|_| scratch_unavailable_error())?;
    Ok(AuthorizedWorkingFolder {
        working_folder_id: synthetic_id,
        canonical_path: runtime_root,
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        repository_storage_identity: None,
    })
}

pub(crate) async fn authorize_scratch_target(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    scratch_generation_id: &str,
    execution_environment_id: &str,
) -> ChatResult<AuthorizedWorkingFolder> {
    require_active_scratch_target(pool, scratch_generation_id, execution_environment_id).await?;

    let expected = scratch_path(app, scratch_generation_id)?;
    let paths = read_active_device_scope(app)
        .map_err(device_state_error)?
        .execution_environment_paths;
    let path = match paths.get(execution_environment_id) {
        Some(stored) => validate_stored_scratch_path(&expected, stored)?,
        None => {
            prepare_scratch_directory(app, &expected)?;
            let text = expected
                .to_str()
                .ok_or_else(scratch_unavailable_error)?
                .to_string();
            update_active_device_scope(app, |scope| {
                scope
                    .execution_environment_paths
                    .insert(execution_environment_id.to_string(), text);
                Ok(())
            })
            .map_err(device_state_error)?;
            expected
        }
    };
    let synthetic_id = ProjectWorkingFolderId::new(format!(
        "scratch-target:{}",
        hex_digest(scratch_generation_id, 32)
    ))
    .map_err(|_| scratch_unavailable_error())?;
    Ok(AuthorizedWorkingFolder {
        working_folder_id: synthetic_id,
        canonical_path: path,
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        repository_storage_identity: None,
    })
}

pub(crate) async fn resolve_managed_scratch_path_for_inspection(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    scratch_generation_id: &str,
    execution_environment_id: &str,
) -> ChatResult<PathBuf> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_scratch_generations generation
           JOIN chat_execution_environments environment
             ON environment.scratch_generation_id = generation.id
            AND environment.kind = 'scratch'
            AND environment.archived_at IS NULL
           WHERE generation.id = ?
             AND environment.id = ?
             AND generation.lifecycle_state IN ('active', 'quarantined', 'cleanup_failed')
             AND generation.removed_at IS NULL
             AND environment.lifecycle_state IN ('available', 'missing', 'cleanup_failed')
         )",
    )
    .bind(scratch_generation_id)
    .bind(execution_environment_id)
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    if !valid {
        return Err(scratch_unavailable_error());
    }
    let expected = scratch_path(app, scratch_generation_id)?;
    let paths = read_active_device_scope(app)
        .map_err(device_state_error)?
        .execution_environment_paths;
    let stored = paths
        .get(execution_environment_id)
        .ok_or_else(scratch_unavailable_error)?;
    validate_stored_scratch_path(&expected, stored)
}

pub(crate) fn bounded_scratch_size(
    root: &Path,
    maximum_entries: u64,
    maximum_bytes: u64,
) -> ChatResult<BoundedScratchSize> {
    ensure_plain_directory(root)?;
    let mut pending = vec![root.to_path_buf()];
    let mut entries = 0_u64;
    let mut bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(io_error)?;
            if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
                return Err(scratch_unavailable_error());
            }
            entries = entries.saturating_add(1);
            if entries > maximum_entries {
                return Ok(BoundedScratchSize {
                    bytes,
                    entries: maximum_entries,
                    truncated: true,
                });
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                if bytes > maximum_bytes {
                    return Ok(BoundedScratchSize {
                        bytes: maximum_bytes,
                        entries,
                        truncated: true,
                    });
                }
            } else {
                return Err(scratch_unavailable_error());
            }
        }
    }
    Ok(BoundedScratchSize {
        bytes,
        entries,
        truncated: false,
    })
}

pub(crate) fn remove_managed_scratch_generation(root: &Path) -> ChatResult<u64> {
    ensure_plain_directory(root)?;
    let mut pending = vec![root.to_path_buf()];
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut bytes = 0_u64;
    while let Some(directory) = pending.pop() {
        directories.push(directory.clone());
        for entry in fs::read_dir(&directory).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
                return Err(scratch_unavailable_error());
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
                files.push(path);
            } else {
                return Err(scratch_unavailable_error());
            }
        }
    }
    for file in files {
        let metadata = fs::symlink_metadata(&file).map_err(io_error)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata_is_reparse_point(&metadata)
        {
            return Err(scratch_unavailable_error());
        }
        fs::remove_file(file).map_err(io_error)?;
    }
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for directory in directories {
        ensure_plain_directory(&directory)?;
        fs::remove_dir(directory).map_err(io_error)?;
    }
    Ok(bytes)
}

pub(crate) async fn require_reusable_generation(
    pool: &SqlitePool,
    scratch_generation_id: &str,
    authorization_revision_id: &str,
) -> ChatResult<()> {
    let authorization = sqlx::query(
        "SELECT authorization.destination_conversation_id,
                authorization.requester_participant_id,
                assignment.teammate_id
         FROM chat_scratch_generations generation
         JOIN chat_scratch_scopes scope ON scope.id = generation.scratch_scope_id
         JOIN chat_agent_runs run ON run.scratch_generation_id = generation.id
         JOIN chat_work_assignments assignment ON assignment.id = run.assignment_id
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = run.authorization_revision_id
          AND authorization.assignment_id = run.assignment_id
          AND authorization.scope_digest = run.authorization_scope_digest
         WHERE generation.id = ?
           AND run.authorization_revision_id = ?
           AND generation.lifecycle_state = 'active'
           AND generation.removed_at IS NULL
           AND scope.lifecycle_state IN ('active', 'archived')
           AND scope.removed_at IS NULL
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL",
    )
    .bind(scratch_generation_id)
    .bind(authorization_revision_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scratch_authority_unavailable_error)?;
    let destination_conversation_id: String = authorization
        .try_get("destination_conversation_id")
        .map_err(persistence_error)?;
    let requester_participant_id: String = authorization
        .try_get("requester_participant_id")
        .map_err(persistence_error)?;
    let teammate_id: String = authorization
        .try_get("teammate_id")
        .map_err(persistence_error)?;
    if generation_constraints_hold(
        pool,
        scratch_generation_id,
        &destination_conversation_id,
        &teammate_id,
        &requester_participant_id,
    )
    .await?
    {
        Ok(())
    } else {
        Err(scratch_authority_unavailable_error())
    }
}

pub(crate) async fn generation_constraints_hold(
    pool: &SqlitePool,
    scratch_generation_id: &str,
    destination_conversation_id: &str,
    teammate_id: &str,
    requester_participant_id: &str,
) -> ChatResult<bool> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    generation_constraints_hold_in_connection(
        &mut connection,
        scratch_generation_id,
        destination_conversation_id,
        teammate_id,
        requester_participant_id,
    )
    .await
}

pub(crate) async fn generation_sources_readable_by(
    pool: &SqlitePool,
    scratch_generation_id: &str,
    participant_id: &str,
) -> ChatResult<bool> {
    let mut connection = pool.acquire().await.map_err(persistence_error)?;
    let sources = sqlx::query(
        "SELECT conversation_id, lower_ordinal
         FROM chat_scratch_generation_sources
         WHERE scratch_generation_id = ?",
    )
    .bind(scratch_generation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for source in sources {
        let conversation_id: String = source
            .try_get("conversation_id")
            .map_err(persistence_error)?;
        let lower_ordinal: i64 = source.try_get("lower_ordinal").map_err(persistence_error)?;
        if !participant_can_read_source(
            &mut connection,
            participant_id,
            &conversation_id,
            lower_ordinal,
        )
        .await?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) async fn generation_constraints_hold_in_connection(
    connection: &mut SqliteConnection,
    scratch_generation_id: &str,
    destination_conversation_id: &str,
    teammate_id: &str,
    requester_participant_id: &str,
) -> ChatResult<bool> {
    let owns_destination: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_scratch_generations generation
           JOIN chat_scratch_scopes scope ON scope.id = generation.scratch_scope_id
           JOIN chat_reply_threads thread ON thread.id = scope.reply_thread_id
           WHERE generation.id = ?
             AND generation.lifecycle_state = 'active'
             AND generation.removed_at IS NULL
             AND scope.teammate_id = ?
             AND scope.lifecycle_state IN ('active', 'archived')
             AND scope.removed_at IS NULL
             AND thread.conversation_id = ?
         )",
    )
    .bind(scratch_generation_id)
    .bind(teammate_id)
    .bind(destination_conversation_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if !owns_destination {
        return Ok(false);
    }
    let sources = sqlx::query(
        "SELECT conversation_id, lower_ordinal, audience_revision
         FROM chat_scratch_generation_sources
         WHERE scratch_generation_id = ?",
    )
    .bind(scratch_generation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if sources.is_empty() {
        return Ok(true);
    }
    let current_audience_revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM chat_conversation_audience_state WHERE conversation_id = ?",
    )
    .bind(destination_conversation_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(persistence_error)?;
    let Some(current_audience_revision) = current_audience_revision else {
        return Ok(false);
    };
    let destination_readers = sqlx::query(
        "SELECT participant.id, participant.participant_kind
         FROM chat_conversation_memberships membership
         JOIN chat_participants participant ON participant.id = membership.participant_id
         LEFT JOIN chat_teammate_channel_memberships channel_access
           ON channel_access.conversation_id = membership.conversation_id
          AND channel_access.teammate_id = membership.participant_id
         LEFT JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
         LEFT JOIN chat_access_profile_revisions profile_revision
           ON profile_revision.access_profile_id = profile.id
          AND profile_revision.revision = profile.latest_revision
         WHERE membership.conversation_id = ?
           AND membership.removed_at IS NULL
           AND (
             participant.participant_kind != 'ai_teammate'
             OR CASE
                  WHEN channel_access.read_history_inherits_profile = 1
                    THEN profile_revision.default_read_history
                  ELSE channel_access.read_history AND profile_revision.default_read_history
                END = 1
           )",
    )
    .bind(destination_conversation_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for source in sources {
        let conversation_id: String = source
            .try_get("conversation_id")
            .map_err(persistence_error)?;
        let lower_ordinal: i64 = source.try_get("lower_ordinal").map_err(persistence_error)?;
        let audience_revision: i64 = source
            .try_get("audience_revision")
            .map_err(persistence_error)?;
        if current_audience_revision < audience_revision
            || !participant_can_read_source(
                &mut *connection,
                teammate_id,
                &conversation_id,
                lower_ordinal,
            )
            .await?
            || !participant_can_read_source(
                &mut *connection,
                requester_participant_id,
                &conversation_id,
                lower_ordinal,
            )
            .await?
        {
            return Ok(false);
        }
        for reader in &destination_readers {
            let participant_id: String = reader.try_get("id").map_err(persistence_error)?;
            if !participant_can_read_source(
                &mut *connection,
                &participant_id,
                &conversation_id,
                lower_ordinal,
            )
            .await?
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

async fn participant_can_read_source(
    connection: &mut SqliteConnection,
    participant_id: &str,
    conversation_id: &str,
    lower_ordinal: i64,
) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1
           FROM chat_conversation_memberships membership
           JOIN chat_participants participant ON participant.id = membership.participant_id
           LEFT JOIN chat_teammate_channel_memberships channel_access
             ON channel_access.conversation_id = membership.conversation_id
            AND channel_access.teammate_id = membership.participant_id
           LEFT JOIN chat_access_profiles profile ON profile.id = channel_access.access_profile_id
           LEFT JOIN chat_access_profile_revisions profile_revision
             ON profile_revision.access_profile_id = profile.id
            AND profile_revision.revision = profile.latest_revision
           WHERE membership.conversation_id = ?
             AND membership.participant_id = ?
             AND membership.removed_at IS NULL
             AND (
               participant.participant_kind != 'ai_teammate'
               OR (
                 CASE
                   WHEN channel_access.read_history_inherits_profile = 1
                     THEN profile_revision.default_read_history
                   ELSE channel_access.read_history AND profile_revision.default_read_history
                 END = 1
                 AND (
                   CASE
                     WHEN channel_access.history_boundary_inherits_profile = 1
                       THEN profile_revision.default_history_boundary
                     WHEN channel_access.history_boundary = 'from_grant'
                       OR profile_revision.default_history_boundary = 'from_grant'
                       THEN 'from_grant'
                     ELSE 'entire'
                   END = 'entire'
                   OR (
                     channel_access.history_from_ordinal IS NOT NULL
                     AND channel_access.history_from_ordinal <= ?
                   )
                 )
               )
             )
         )",
    )
    .bind(conversation_id)
    .bind(participant_id)
    .bind(lower_ordinal)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)
}

async fn require_active_scratch_target(
    pool: &SqlitePool,
    scratch_generation_id: &str,
    execution_environment_id: &str,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT generation.lifecycle_state AS generation_state,
                environment.lifecycle_state AS environment_state
         FROM chat_scratch_generations generation
         JOIN chat_execution_environments environment
           ON environment.scratch_generation_id = generation.id
          AND environment.kind = 'scratch'
          AND environment.archived_at IS NULL
         WHERE generation.id = ? AND environment.id = ?
           AND generation.removed_at IS NULL",
    )
    .bind(scratch_generation_id)
    .bind(execution_environment_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(scratch_unavailable_error)?;
    if row
        .try_get::<String, _>("generation_state")
        .map_err(persistence_error)?
        != "active"
        || row
            .try_get::<String, _>("environment_state")
            .map_err(persistence_error)?
            != "available"
    {
        return Err(scratch_unavailable_error());
    }
    Ok(())
}

fn validate_stored_scratch_path(expected: &Path, stored: &str) -> ChatResult<PathBuf> {
    let stored = PathBuf::from(stored);
    if stored != expected || !managed_directory_is_available(&stored) {
        return Err(scratch_unavailable_error());
    }
    Ok(stored)
}

fn scratch_path(app: &tauri::AppHandle, scratch_generation_id: &str) -> ChatResult<PathBuf> {
    let local_root = app
        .path()
        .app_local_data_dir()
        .map_err(device_state_error)?;
    let vault_id = vault::active_vault_id(app).map_err(device_state_error)?;
    Ok(local_root
        .join(SCRATCH_DIRECTORY)
        .join(hex_digest(&vault_id, 24))
        .join(hex_digest(scratch_generation_id, 32)))
}

fn prepare_scratch_directory(app: &tauri::AppHandle, target: &Path) -> ChatResult<()> {
    let local_root = app
        .path()
        .app_local_data_dir()
        .map_err(device_state_error)?;
    prepare_managed_directory(&local_root, SCRATCH_DIRECTORY, target)
}

fn prepare_managed_directory(local_root: &Path, category: &str, target: &Path) -> ChatResult<()> {
    fs::create_dir_all(local_root).map_err(io_error)?;
    ensure_plain_directory(local_root)?;
    let root = local_root.join(category);
    create_plain_directory(&root)?;
    let vault_root = target.parent().ok_or_else(scratch_unavailable_error)?;
    if vault_root.parent() != Some(root.as_path()) {
        return Err(scratch_unavailable_error());
    }
    create_plain_directory(vault_root)?;
    create_plain_directory(target)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}

fn create_plain_directory(path: &Path) -> ChatResult<()> {
    match fs::create_dir(path) {
        Ok(()) => ensure_plain_directory(path),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure_plain_directory(path)
        }
        Err(error) => Err(io_error(error)),
    }
}

fn ensure_plain_directory(path: &Path) -> ChatResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink()
        || metadata_is_reparse_point(&metadata)
        || !metadata.is_dir()
    {
        return Err(scratch_unavailable_error());
    }
    Ok(())
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT.0
        != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn managed_directory_is_available(path: &Path) -> bool {
    ensure_plain_directory(path).is_ok()
}

fn hex_digest(value: &str, length: usize) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    digest[..length.min(digest.len())].to_string()
}

fn scratch_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Private scratch is unavailable on this device",
        true,
    )
}

fn scratch_authority_unavailable_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Private scratch authorization is no longer active",
        false,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Private scratch state could not be verified",
        true,
    )
}

fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Private scratch device state is unavailable",
        true,
    )
}

fn io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Private scratch directory could not be prepared",
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-09-18T12:00:00.000Z";
    const GENERATION: &str = "generation:reuse";
    const AUTHORIZATION: &str = "authorization:reuse";
    const TEAMMATE: &str = "participant:scratch-agent";

    /// Builds a real-schema scratch assignment without disabling foreign keys or triggers.
    async fn pool_with_scratch_assignment(scope_owner: &str) -> SqlitePool {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        sqlx::query(
            "UPDATE chat_conversations SET id = 'conversation:scratch'
             WHERE project_id = 'project-chat' AND conversation_kind = 'channel'",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, name) in [
            (TEAMMATE, "Scratch agent"),
            ("participant:other-agent", "Other agent"),
        ] {
            sqlx::query(
                "INSERT INTO chat_participants
                    (id, participant_kind, display_name, created_at, updated_at)
                 VALUES (?, 'ai_teammate', ?, ?, ?)",
            )
            .bind(id)
            .bind(name)
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO chat_teammates
                    (participant_id, role, created_at, updated_at)
                 VALUES (?, 'Test', ?, ?)",
            )
            .bind(id)
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::raw_sql(&format!(
            "INSERT INTO chat_teammate_policy_revisions
                (id, teammate_id, revision, provider_instance_id, model_selection_data, created_at)
             VALUES ('policy:scratch', '{TEAMMATE}', 1, 'codex-personal', '{{}}', '{NOW}');
             INSERT INTO chat_conversation_memberships
                (conversation_id, participant_id, membership_role, created_at, updated_at)
             VALUES ('conversation:scratch', '{TEAMMATE}', 'member', '{NOW}', '{NOW}');
             INSERT INTO chat_teammate_channel_memberships
                (conversation_id, teammate_id, access_profile_id,
                 read_history, participate, history_boundary, created_at, updated_at)
             VALUES ('conversation:scratch', '{TEAMMATE}', 'access-profile:conversation-only',
                     1, 1, 'entire', '{NOW}', '{NOW}');
             INSERT INTO chat_conversation_items
                (id, conversation_id, item_kind, ordinal, created_at)
             VALUES ('item:scratch', 'conversation:scratch', 'message', 1, '{NOW}');
             INSERT INTO chat_communication_messages
                (item_id, author_participant_id, created_at)
             VALUES ('item:scratch', 'participant:local-owner', '{NOW}');
             INSERT INTO chat_reply_threads
                (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
             VALUES ('reply:scratch', 'conversation:scratch', 'item:scratch', '{NOW}', '{NOW}', '{NOW}');
             INSERT INTO chat_work_assignments
                (id, reply_thread_id, teammate_id, triggering_message_item_id, state, created_at, updated_at)
             VALUES ('assignment:scratch', 'reply:scratch', '{TEAMMATE}', 'item:scratch',
                     'working', '{NOW}', '{NOW}');"
        ))
        .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO chat_scratch_scopes
                (id, reply_thread_id, teammate_id, created_at, updated_at)
             VALUES ('scope:scratch', 'reply:scratch', ?, ?, ?)",
        )
        .bind(scope_owner)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(&format!(
            "INSERT INTO chat_scratch_generations
                (id, scratch_scope_id, generation, created_at, updated_at)
             VALUES ('{GENERATION}', 'scope:scratch', 1, '{NOW}', '{NOW}');
             INSERT INTO chat_execution_environments
                (id, scratch_generation_id, kind, display_name, created_at, updated_at)
             VALUES ('environment:scratch', '{GENERATION}', 'scratch', 'Private scratch', '{NOW}', '{NOW}');
             UPDATE chat_threads SET working_folder_id = NULL,
                 execution_environment_id = 'environment:scratch', scratch_generation_id = '{GENERATION}'
             WHERE id = 'thread-1';
             INSERT INTO chat_assignment_authorization_revisions
                (id, assignment_id, revision, requester_participant_id, teammate_policy_revision_id,
                 teammate_access_revision, destination_conversation_id, access_profile_revision_id,
                 execution_environment_id, resolved_runtime_approval_policy, scope_digest,
                 decision_state, created_at)
             VALUES ('{AUTHORIZATION}', 'assignment:scratch', 1, 'participant:local-owner',
                     'policy:scratch', 1, 'conversation:scratch', 'access-profile-revision:conversation-only:1',
                     'environment:scratch', 'ask', printf('%064d', 0), 'allowed', '{NOW}');
             INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
             VALUES ('turn:scratch', 'thread-1', 1, 'active', 'ask_for_approval', 'build', '{NOW}', '{NOW}');
             INSERT INTO chat_agent_runs
                (id, assignment_id, project_id, execution_environment_id, scratch_generation_id,
                 teammate_policy_revision_id, authorization_revision_id, authorization_scope_digest,
                 provider_turn_id, provider_thread_id, state, run_ordinal, created_at, updated_at)
             VALUES ('run:scratch', 'assignment:scratch', 'project-chat', 'environment:scratch',
                     '{GENERATION}', 'policy:scratch', '{AUTHORIZATION}', printf('%064d', 0),
                     'turn:scratch', 'thread-1', 'working', 1, '{NOW}', '{NOW}');"
        ))
        .execute(&pool).await.unwrap();
        pool
    }

    async fn retain_destination_source(pool: &SqlitePool) {
        let inserted = sqlx::query(
            "INSERT INTO chat_scratch_generation_sources
                (scratch_generation_id, conversation_id, lower_ordinal, high_ordinal,
                 audience_revision, created_at)
             SELECT ?, conversation_id, 1, 1, revision, ?
             FROM chat_conversation_audience_state WHERE conversation_id = 'conversation:scratch'",
        )
        .bind(GENERATION)
        .bind(NOW)
        .execute(pool)
        .await
        .unwrap();
        assert_eq!(inserted.rows_affected(), 1);
    }

    #[tokio::test]
    async fn scratch_reuse_resolves_the_assignment_teammate_and_retained_sources() {
        let pool = pool_with_scratch_assignment(TEAMMATE).await;
        require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap();
        retain_destination_source(&pool).await;
        require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn scratch_reuse_rejects_a_different_scope_owner() {
        let pool = pool_with_scratch_assignment("participant:other-agent").await;
        let error = require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
    }

    #[tokio::test]
    async fn scratch_reuse_rejects_revoked_authorization() {
        let pool = pool_with_scratch_assignment(TEAMMATE).await;
        require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap();
        sqlx::query("UPDATE chat_assignment_authorization_revisions SET decision_state = 'revoked', revoked_at = ? WHERE id = ?")
            .bind(NOW).bind(AUTHORIZATION).execute(&pool).await.unwrap();
        let error = require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
    }

    #[tokio::test]
    async fn scratch_reuse_rejects_unknown_identifiers_and_inactive_generations() {
        let pool = pool_with_scratch_assignment(TEAMMATE).await;
        for (generation, authorization) in [
            ("generation:unknown", AUTHORIZATION),
            (GENERATION, "authorization:unknown"),
        ] {
            let error = require_reusable_generation(&pool, generation, authorization)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Permission);
        }
        for state in [
            "quarantined",
            "cleanup_pending",
            "cleanup_failed",
            "removed",
        ] {
            sqlx::query("UPDATE chat_scratch_generations SET lifecycle_state = ? WHERE id = ?")
                .bind(state)
                .bind(GENERATION)
                .execute(&pool)
                .await
                .unwrap();
            let error = require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Permission);
        }
    }

    #[tokio::test]
    async fn scratch_reuse_rechecks_retained_source_access() {
        let pool = pool_with_scratch_assignment(TEAMMATE).await;
        retain_destination_source(&pool).await;
        require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_teammate_channel_memberships SET read_history = 0 WHERE teammate_id = ?",
        )
        .bind(TEAMMATE)
        .execute(&pool)
        .await
        .unwrap();
        let error = require_reusable_generation(&pool, GENERATION, AUTHORIZATION)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
    }

    #[test]
    fn scratch_paths_use_only_bounded_digest_segments() {
        assert_eq!(hex_digest("vault/../../outside", 24).len(), 24);
        assert_eq!(hex_digest("generation\\outside", 32).len(), 32);
        assert!(!hex_digest("generation\\outside", 32).contains('/'));
    }
}

//! Lazily created Chat rows every project needs: the default channel and one current-folder
//! execution environment per working folder.
//!
//! Projects and working folders replicate between devices, so inserting them never creates Chat
//! rows. Chat creates these rows on demand where it is writable. The default conversation and
//! channel ids derive from the project id, so every device that creates them agrees on the ids.

use crate::models::{ChatError, ChatErrorCode, ChatResult, UtcTimestamp};
use ganbaru_sync_contracts::derived_key;
use sqlx::SqliteConnection;

/// Derived key prefix of a project's default channel conversation id.
pub const DEFAULT_CONVERSATION_KEY_PREFIX: &str = "conversation-default";
/// Derived key prefix of a project's default channel id.
pub const DEFAULT_CHANNEL_KEY_PREFIX: &str = "channel-default";
/// Name of every project's default channel.
pub const DEFAULT_CHANNEL_NAME: &str = "general";
/// Participant that owns every default channel.
pub const LOCAL_OWNER_PARTICIPANT_ID: &str = "participant:local-owner";
/// Prefix of the current-folder execution environment id of a working folder.
pub const CURRENT_FOLDER_ENVIRONMENT_PREFIX: &str = "current-folder:";

/// Returns the current-folder execution environment id of `working_folder_id`.
pub fn current_folder_environment_id(working_folder_id: &str) -> String {
    format!("{CURRENT_FOLDER_ENVIRONMENT_PREFIX}{working_folder_id}")
}

/// Returns the derived default conversation and channel ids of `project_id`.
pub fn default_channel_ids(project_id: &str) -> ChatResult<(String, String)> {
    let conversation = derived_key(DEFAULT_CONVERSATION_KEY_PREFIX, &[project_id])
        .map_err(|_| invalid_project_error())?;
    let channel = derived_key(DEFAULT_CHANNEL_KEY_PREFIX, &[project_id])
        .map_err(|_| invalid_project_error())?;
    Ok((conversation.into_string(), channel.into_string()))
}

/// Creates the default channel of `project_id` when it has none, and a current-folder execution
/// environment for each of its working folders that is not archived and lacks one.
///
/// An existing default channel keeps its id. Running it again changes nothing. Callers run it
/// inside their own transaction when the rows must agree with later reads in that transaction.
pub async fn ensure_project_chat_defaults(
    connection: &mut SqliteConnection,
    project_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let project_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?)")
            .bind(project_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(persistence_error)?;
    if !project_exists {
        return Err(ChatError::new(
            ChatErrorCode::NotFound,
            "The Chat project was not found",
            true,
        ));
    }
    ensure_default_channel(connection, project_id, now).await?;
    ensure_folder_environments(connection, project_id).await
}

/// Runs [`ensure_project_chat_defaults`] for the project that owns `working_folder_id`.
pub async fn ensure_working_folder_chat_defaults(
    connection: &mut SqliteConnection,
    working_folder_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let project_id: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM project_working_folders WHERE id = ?")
            .bind(working_folder_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(persistence_error)?;
    match project_id {
        Some(project_id) => ensure_project_chat_defaults(connection, &project_id, now).await,
        None => Err(ChatError::new(
            ChatErrorCode::NotFound,
            "Project working folder was not found",
            true,
        )),
    }
}

/// Runs [`ensure_project_chat_defaults`] for every project that lacks a default channel or a
/// current-folder execution environment for one of its working folders that is not archived.
pub async fn ensure_all_project_chat_defaults(
    connection: &mut SqliteConnection,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let project_ids: Vec<String> = sqlx::query_scalar(
        "SELECT project.id FROM projects project
         WHERE NOT EXISTS (
                 SELECT 1 FROM chat_channels channel
                 WHERE channel.project_id = project.id AND channel.is_default = 1
               )
            OR EXISTS (
                 SELECT 1 FROM project_working_folders folder
                 WHERE folder.project_id = project.id AND folder.archived_at IS NULL
                   AND NOT EXISTS (
                       SELECT 1 FROM chat_execution_environments environment
                       WHERE environment.id = ? || folder.id
                          OR (environment.working_folder_id = folder.id
                              AND environment.kind = 'current_folder'
                              AND environment.archived_at IS NULL)
                   )
               )
         ORDER BY project.id",
    )
    .bind(CURRENT_FOLDER_ENVIRONMENT_PREFIX)
    .fetch_all(&mut *connection)
    .await
    .map_err(persistence_error)?;
    for project_id in project_ids {
        ensure_project_chat_defaults(connection, &project_id, now).await?;
    }
    Ok(())
}

async fn ensure_default_channel(
    connection: &mut SqliteConnection,
    project_id: &str,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let has_default: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM chat_channels WHERE project_id = ? AND is_default = 1)",
    )
    .bind(project_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(persistence_error)?;
    if has_default {
        return Ok(());
    }
    let (conversation_id, channel_id) = default_channel_ids(project_id)?;
    sqlx::query(
        "INSERT INTO chat_conversations
            (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
         SELECT ?, ?, 'channel', ?, ?, ?
         WHERE NOT EXISTS (SELECT 1 FROM chat_conversations WHERE id = ?)",
    )
    .bind(&conversation_id)
    .bind(project_id)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&conversation_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_channels
            (id, project_id, conversation_id, name, topic, is_default, created_at, updated_at)
         SELECT ?, ?, ?, ?, '', 1, ?, ?
         WHERE NOT EXISTS (
             SELECT 1 FROM chat_channels WHERE id = ? OR (project_id = ? AND is_default = 1)
         )",
    )
    .bind(&channel_id)
    .bind(project_id)
    .bind(&conversation_id)
    .bind(DEFAULT_CHANNEL_NAME)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&channel_id)
    .bind(project_id)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_conversation_memberships
            (conversation_id, participant_id, membership_role, created_at, updated_at)
         SELECT ?, ?, 'owner', ?, ?
         WHERE NOT EXISTS (
             SELECT 1 FROM chat_conversation_memberships
             WHERE conversation_id = ? AND participant_id = ?
         )",
    )
    .bind(&conversation_id)
    .bind(LOCAL_OWNER_PARTICIPANT_ID)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(&conversation_id)
    .bind(LOCAL_OWNER_PARTICIPANT_ID)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

/// Inserts `current-folder:{folder id}` for each folder of the project that is not archived and
/// has neither an active current-folder environment nor an archived row with that id.
async fn ensure_folder_environments(
    connection: &mut SqliteConnection,
    project_id: &str,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_execution_environments
            (id, working_folder_id, kind, display_name, repository_identity,
             lifecycle_state, created_at, updated_at)
         SELECT ? || folder.id, folder.id, 'current_folder', folder.display_name,
                folder.repository_identity, 'available', folder.created_at, folder.updated_at
         FROM project_working_folders folder
         WHERE folder.project_id = ? AND folder.archived_at IS NULL
           AND NOT EXISTS (
               SELECT 1 FROM chat_execution_environments environment
               WHERE environment.id = ? || folder.id
                  OR (environment.working_folder_id = folder.id
                      AND environment.kind = 'current_folder'
                      AND environment.archived_at IS NULL)
           )",
    )
    .bind(CURRENT_FOLDER_ENVIRONMENT_PREFIX)
    .bind(project_id)
    .bind(CURRENT_FOLDER_ENVIRONMENT_PREFIX)
    .execute(&mut *connection)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn invalid_project_error() -> ChatError {
    ChatError::validation("projectId", "Chat project id is invalid")
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Project Chat defaults could not be saved",
        true,
    )
}

#[cfg(test)]
mod tests;

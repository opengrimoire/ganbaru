//! Effective primary working folder of a project.
//!
//! The stored primary folder row and the folder rows replicate independently, so the stored folder
//! can be missing, belong to another project, or be archived. Reads then resolve the project's
//! managed folder. Commands keep the rule that a newly chosen primary belongs to the project and
//! is active.

use super::super::channels::{i64_value, identifier_error, persistence_error, u64_value};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};

/// Reads the effective primary working folder of `project_id`.
///
/// The revision is that of the stored row, or 0 when the project has no stored row yet, so a
/// later set with that revision creates it.
pub(super) async fn read_effective_primary_working_folder(
    pool: &SqlitePool,
    project_id: &str,
) -> ChatResult<ChatProjectPrimaryWorkingFolderRead> {
    let row = sqlx::query(
        "SELECT project.id AS project_id,
                coalesce(stored_folder.id, managed_folder.id) AS working_folder_id,
                coalesce(primary_folder.revision, 0) AS revision
         FROM projects project
         LEFT JOIN project_primary_working_folders primary_folder
           ON primary_folder.project_id = project.id
         LEFT JOIN project_working_folders stored_folder
           ON stored_folder.id = primary_folder.working_folder_id
          AND stored_folder.project_id = project.id
          AND stored_folder.archived_at IS NULL
         LEFT JOIN project_working_folders managed_folder
           ON managed_folder.project_id = project.id
          AND managed_folder.kind = 'managed'
         WHERE project.id = ?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(not_found_error)?;
    let working_folder_id: Option<String> = row
        .try_get("working_folder_id")
        .map_err(persistence_error)?;
    Ok(ChatProjectPrimaryWorkingFolderRead {
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        working_folder_id: ProjectWorkingFolderId::new(
            working_folder_id.ok_or_else(not_found_error)?,
        )
        .map_err(identifier_error)?,
        revision: u64_value(row.try_get("revision").map_err(persistence_error)?)?,
    })
}

/// Stores `working_folder_id` as the primary folder of `project_id` when the stored row still has
/// `expected_revision`, where 0 means the project has no stored row.
pub(super) async fn set_primary_working_folder(
    pool: &SqlitePool,
    project_id: &str,
    working_folder_id: &ProjectWorkingFolderId,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<ChatProjectPrimaryWorkingFolderRead> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let eligible: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM project_working_folders
           WHERE id = ? AND project_id = ? AND archived_at IS NULL
         )",
    )
    .bind(working_folder_id.as_str())
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if !eligible {
        return Err(ChatError::validation(
            "workingFolderId",
            "The primary working folder must be an active folder of this project",
        ));
    }
    let updated = if expected_revision == 0 {
        sqlx::query(
            "INSERT INTO project_primary_working_folders
                (project_id, working_folder_id, created_at, updated_at)
             SELECT ?, ?, ?, ?
             WHERE NOT EXISTS (
                 SELECT 1 FROM project_primary_working_folders WHERE project_id = ?
             )",
        )
        .bind(project_id)
        .bind(working_folder_id.as_str())
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(project_id)
        .execute(&mut *transaction)
        .await
    } else {
        sqlx::query(
            "UPDATE project_primary_working_folders
             SET working_folder_id = ?, revision = revision + 1, updated_at = ?
             WHERE project_id = ? AND revision = ?",
        )
        .bind(working_folder_id.as_str())
        .bind(now.as_str())
        .bind(project_id)
        .bind(i64_value(expected_revision)?)
        .execute(&mut *transaction)
        .await
    }
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "The primary working folder changed before the update",
            true,
        ));
    }
    transaction.commit().await.map_err(persistence_error)?;
    read_effective_primary_working_folder(pool, project_id).await
}

fn not_found_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Primary working folder was not found",
        true,
    )
}

#[cfg(test)]
mod tests;

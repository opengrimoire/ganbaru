use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, RepositoryKind, UtcTimestamp,
};
use crate::workspace::{
    CreateProjectWorkingFolderRequest, ProjectWorkingFolder, WorkingFolderKind,
};
use sqlx::{Row, SqlitePool};

pub async fn list_workspaces(pool: &SqlitePool) -> ChatResult<Vec<ProjectWorkingFolder>> {
    let rows = sqlx::query(
        "SELECT id, project_id, display_name, kind, managed_relative_path, sort_order,
                repository_kind, repository_identity, created_at, updated_at, archived_at, revision
         FROM project_working_folders
         ORDER BY project_id, archived_at IS NOT NULL, sort_order, display_name COLLATE NOCASE, id",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(row_to_workspace).collect()
}

pub async fn read_workspace(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
) -> ChatResult<ProjectWorkingFolder> {
    sqlx::query(
        "SELECT id, project_id, display_name, kind, managed_relative_path, sort_order,
                repository_kind, repository_identity, created_at, updated_at, archived_at, revision
         FROM project_working_folders WHERE id = ?",
    )
    .bind(id.as_str())
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .map(row_to_workspace)
    .transpose()?
    .ok_or_else(not_found_error)
}

pub async fn create_workspace(
    pool: &SqlitePool,
    request: &CreateProjectWorkingFolderRequest,
    now: &UtcTimestamp,
) -> ChatResult<ProjectWorkingFolder> {
    validate_name(&request.display_name)?;
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, sort_order, repository_kind, created_at, updated_at)
         VALUES (?, ?, ?, 'external', 10, 'none', ?, ?)",
    )
    .bind(request.id.as_str())
    .bind(&request.project_id)
    .bind(request.display_name.trim())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    read_workspace(pool, &request.id).await
}

pub async fn rename_workspace(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
    display_name: &str,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<ProjectWorkingFolder> {
    validate_name(display_name)?;
    let result = sqlx::query(
        "UPDATE project_working_folders SET display_name = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(display_name.trim())
    .bind(now.as_str())
    .bind(id.as_str())
    .bind(i64::try_from(expected_revision).unwrap_or(i64::MAX))
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    require_updated(pool, id, result.rows_affected()).await
}

pub async fn set_workspace_repository(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
    kind: RepositoryKind,
    identity: Option<&str>,
    now: &UtcTimestamp,
) -> ChatResult<ProjectWorkingFolder> {
    if (kind == RepositoryKind::Git) != identity.is_some() {
        return Err(ChatError::validation(
            "repositoryIdentity",
            "Chat repository identity is inconsistent",
        ));
    }
    let result = sqlx::query(
        "UPDATE project_working_folders SET repository_kind = ?, repository_identity = ?,
                revision = revision + 1, updated_at = ? WHERE id = ?
           AND (repository_identity IS NULL OR repository_identity IS ?)",
    )
    .bind(match kind {
        RepositoryKind::Git => "git",
        RepositoryKind::None => "none",
    })
    .bind(identity)
    .bind(now.as_str())
    .bind(id.as_str())
    .bind(identity)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    require_updated(pool, id, result.rows_affected()).await
}

pub async fn set_workspace_archived(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
    archived: bool,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<ProjectWorkingFolder> {
    let result = sqlx::query(
        "UPDATE project_working_folders SET archived_at = ?, revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ?",
    )
    .bind(archived.then_some(now.as_str()))
    .bind(now.as_str())
    .bind(id.as_str())
    .bind(i64::try_from(expected_revision).unwrap_or(i64::MAX))
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    require_updated(pool, id, result.rows_affected()).await
}

pub async fn remove_external_working_folder(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
) -> ChatResult<()> {
    let result =
        sqlx::query("DELETE FROM project_working_folders WHERE id = ? AND kind = 'external'")
            .bind(id.as_str())
            .execute(pool)
            .await
            .map_err(persistence_error)?;
    if result.rows_affected() == 1 {
        return Ok(());
    }
    let folder = read_workspace(pool, id).await?;
    Err(if folder.kind == WorkingFolderKind::Managed {
        ChatError::validation(
            "workingFolderId",
            "Managed project working folders cannot be removed",
        )
    } else {
        not_found_error()
    })
}

async fn require_updated(
    pool: &SqlitePool,
    id: &ProjectWorkingFolderId,
    rows: u64,
) -> ChatResult<ProjectWorkingFolder> {
    if rows == 1 {
        return read_workspace(pool, id).await;
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM project_working_folders WHERE id = ?)")
            .bind(id.as_str())
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
    Err(if exists {
        ChatError::new(
            ChatErrorCode::StaleRevision,
            "Project working-folder revision is stale",
            true,
        )
    } else {
        not_found_error()
    })
}

fn row_to_workspace(row: sqlx::sqlite::SqliteRow) -> ChatResult<ProjectWorkingFolder> {
    let folder_kind: String = row.try_get("kind").map_err(persistence_error)?;
    let repository_kind: String = row.try_get("repository_kind").map_err(persistence_error)?;
    Ok(ProjectWorkingFolder {
        id: ProjectWorkingFolderId::new(row.try_get::<String, _>("id").map_err(persistence_error)?)
            .map_err(|_| corrupt_data_error())?,
        project_id: row.try_get("project_id").map_err(persistence_error)?,
        display_name: row.try_get("display_name").map_err(persistence_error)?,
        kind: match folder_kind.as_str() {
            "managed" => WorkingFolderKind::Managed,
            "external" => WorkingFolderKind::External,
            _ => return Err(corrupt_data_error()),
        },
        managed_relative_path: row
            .try_get("managed_relative_path")
            .map_err(persistence_error)?,
        sort_order: u64::try_from(
            row.try_get::<i64, _>("sort_order")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        repository_kind: match repository_kind.as_str() {
            "git" => RepositoryKind::Git,
            "none" => RepositoryKind::None,
            _ => return Err(corrupt_data_error()),
        },
        repository_identity: row
            .try_get("repository_identity")
            .map_err(persistence_error)?,
        created_at: timestamp(
            row.try_get::<String, _>("created_at")
                .map_err(persistence_error)?,
        )?,
        updated_at: timestamp(
            row.try_get::<String, _>("updated_at")
                .map_err(persistence_error)?,
        )?,
        archived_at: row
            .try_get::<Option<String>, _>("archived_at")
            .map_err(persistence_error)?
            .map(timestamp)
            .transpose()?,
        revision: u64::try_from(
            row.try_get::<i64, _>("revision")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
    })
}

fn validate_name(value: &str) -> ChatResult<()> {
    if value.trim().is_empty() || value.trim().len() > 240 {
        Err(ChatError::validation(
            "displayName",
            "Project working-folder name is invalid",
        ))
    } else {
        Ok(())
    }
}
fn timestamp(value: String) -> ChatResult<UtcTimestamp> {
    UtcTimestamp::new(value).map_err(|_| corrupt_data_error())
}
fn not_found_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Project working folder was not found",
        true,
    )
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Project working-folder persistence failed",
        true,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored project working folder is invalid",
        false,
    )
}

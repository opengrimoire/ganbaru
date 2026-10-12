use crate::models::{NoteFolderCreate, NoteFolderDto, NoteFolderRow, NoteFolderUpdate};
use crate::project_history;
use crate::validation::{
    require_uuid, validate_folder_create, validate_folder_project_id, validate_folder_update,
};
use sqlx::SqlitePool;

pub async fn list_folders(pool: &SqlitePool) -> Result<Vec<NoteFolderDto>, String> {
    let rows = sqlx::query_as::<_, NoteFolderRow>(
        "SELECT *
         FROM notes_folders
         ORDER BY project_id ASC, name COLLATE NOCASE ASC, id ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("list notes folders: {e}"))?;
    Ok(rows.into_iter().map(NoteFolderDto::new).collect())
}

pub async fn create_folder(
    pool: &SqlitePool,
    folder: NoteFolderCreate,
) -> Result<NoteFolderDto, String> {
    validate_folder_create(&folder)?;
    let id = folder.id.trim();
    let project_id = folder.project_id.trim();
    let parent_folder_id = folder.parent_folder_id.as_deref().map(str::trim);
    let name = folder.name.trim();
    project_history::ensure_project_baseline_for_mutation(pool, project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes folder create: {e}"))?;
    require_project(&mut tx, project_id).await?;
    validate_folder_parent(&mut tx, project_id, parent_folder_id).await?;
    sqlx::query(
        "INSERT INTO notes_folders (id, project_id, parent_folder_id, name)
         VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(project_id)
    .bind(parent_folder_id)
    .bind(name)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("create notes folder: {e}"))?;
    project_history::mark_project_dirty_tx(&mut tx, project_id, name, false).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes folder create: {e}"))?;
    get_folder(pool, id).await
}

pub async fn update_folder(
    pool: &SqlitePool,
    folder_id: &str,
    update: NoteFolderUpdate,
) -> Result<NoteFolderDto, String> {
    let folder_id = folder_id.trim();
    require_uuid(folder_id, "folder_id")?;
    validate_folder_update(&update)?;
    let parent_folder_id = update.parent_folder_id.as_deref().map(str::trim);
    if parent_folder_id == Some(folder_id) {
        return Err("folder cannot be moved under itself".to_string());
    }
    let name = update.name.trim();
    let current_project_id: String =
        sqlx::query_scalar("SELECT project_id FROM notes_folders WHERE id = ?")
            .bind(folder_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes folder project: {e}"))?
            .ok_or_else(|| "notes folder not found".to_string())?;
    project_history::ensure_project_baseline_for_mutation(pool, &current_project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes folder update: {e}"))?;
    let current = get_folder_tx(&mut tx, folder_id).await?;
    validate_folder_parent(&mut tx, &current.project_id, parent_folder_id).await?;
    let result = sqlx::query(
        "UPDATE notes_folders
         SET parent_folder_id = ?,
             name = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?",
    )
    .bind(parent_folder_id)
    .bind(name)
    .bind(folder_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("update notes folder: {e}"))?;
    if result.rows_affected() == 0 {
        return Err("notes folder not found".to_string());
    }
    project_history::mark_project_dirty_tx(&mut tx, &current.project_id, name, false).await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes folder update: {e}"))?;
    get_folder(pool, folder_id).await
}

pub async fn delete_folder(pool: &SqlitePool, folder_id: &str) -> Result<String, String> {
    let folder_id = folder_id.trim();
    require_uuid(folder_id, "folder_id")?;
    let project_id: String =
        sqlx::query_scalar("SELECT project_id FROM notes_folders WHERE id = ?")
            .bind(folder_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| format!("load notes folder project: {e}"))?
            .ok_or_else(|| "notes folder not found".to_string())?;
    project_history::ensure_project_baseline_for_mutation(pool, &project_id).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("begin notes folder delete: {e}"))?;
    let folder = get_folder_tx(&mut tx, folder_id).await?;
    sqlx::query(
        "UPDATE notes_folders
         SET parent_folder_id = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE parent_folder_id = ?",
    )
    .bind(&folder.parent_folder_id)
    .bind(folder_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("promote notes child folders: {e}"))?;
    sqlx::query(
        "UPDATE notes_pages
         SET folder_id = ?,
             last_edited_time = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE folder_id = ?",
    )
    .bind(&folder.parent_folder_id)
    .bind(folder_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("promote notes folder pages: {e}"))?;
    sqlx::query("DELETE FROM notes_folders WHERE id = ?")
        .bind(folder_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("delete notes folder: {e}"))?;
    project_history::mark_project_dirty_tx(&mut tx, &folder.project_id, &folder.name, false)
        .await?;
    tx.commit()
        .await
        .map_err(|e| format!("commit notes folder delete: {e}"))?;
    Ok(folder_id.to_string())
}

async fn get_folder(pool: &SqlitePool, folder_id: &str) -> Result<NoteFolderDto, String> {
    let row = sqlx::query_as::<_, NoteFolderRow>("SELECT * FROM notes_folders WHERE id = ?")
        .bind(folder_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("load notes folder: {e}"))?
        .ok_or_else(|| "notes folder not found".to_string())?;
    Ok(NoteFolderDto::new(row))
}

async fn get_folder_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    folder_id: &str,
) -> Result<NoteFolderRow, String> {
    sqlx::query_as::<_, NoteFolderRow>("SELECT * FROM notes_folders WHERE id = ?")
        .bind(folder_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes folder: {e}"))?
        .ok_or_else(|| "notes folder not found".to_string())
}

async fn require_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
) -> Result<(), String> {
    validate_folder_project_id(project_id)?;
    let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| format!("load notes folder project: {e}"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| "project not found".to_string())
}

async fn validate_folder_parent(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    project_id: &str,
    parent_folder_id: Option<&str>,
) -> Result<(), String> {
    let Some(parent_folder_id) = parent_folder_id else {
        return Ok(());
    };
    let parent_project_id: Option<String> =
        sqlx::query_scalar("SELECT project_id FROM notes_folders WHERE id = ?")
            .bind(parent_folder_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|e| format!("load notes parent folder: {e}"))?;
    match parent_project_id.as_deref() {
        None => Err("parent folder not found".to_string()),
        Some(parent_project_id) if parent_project_id != project_id => {
            Err("parent folder must belong to the same project".to_string())
        }
        Some(_) => Ok(()),
    }
}

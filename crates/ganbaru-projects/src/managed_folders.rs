//! Where this vault keeps each project's managed working folder.
//!
//! A managed working folder row replicates the folder name its creating device chose, as
//! `projects/{name}`. Each vault copy claims its own folder for the row in
//! `project_managed_folder_locations`, so two projects that concurrent devices named alike never
//! share a folder: the second claim gets a numeric suffix. A claim never moves afterwards, so a
//! renamed project keeps the folder its tools remember.

use std::{collections::HashSet, path::Path};

use ganbaru_working_folders::{
    existing_folder_keys, managed_folder_key, managed_folder_name, managed_relative_path,
    portable_folder_name, unique_folder_name,
};
use sqlx::{SqliteConnection, SqlitePool};

/// Inserts the managed working folder of a newly inserted project, claims its folder name in this
/// vault, stores it as the project's primary folder, and returns the claimed folder name. The
/// folder directory itself is created by the caller or when the folder is resolved.
///
/// With a `vault_root`, a name already used by any entry inside `projects/` is skipped, so a new
/// project never takes over a folder a deleted project left behind. Without one only claimed names
/// are skipped, which lets a restored project take back the folder it left under its name.
pub(crate) async fn insert_managed_working_folder(
    conn: &mut SqliteConnection,
    vault_root: Option<&Path>,
    project_id: &str,
    display_name: &str,
) -> Result<String, String> {
    let managed_folder_id = ganbaru_working_folders::managed_folder_id(project_id)
        .map_err(|e| format!("create managed project working folder: {e}"))?
        .into_inner();
    let folder_name =
        free_folder_name(conn, vault_root, &portable_folder_name(display_name)).await?;
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, managed_relative_path, sort_order)
         VALUES (?, ?, ?, 'managed', ?, 0)",
    )
    .bind(&managed_folder_id)
    .bind(project_id)
    .bind(display_name)
    .bind(managed_relative_path(&folder_name)?)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("create managed project working folder: {e}"))?;
    insert_location(conn, &managed_folder_id, &folder_name).await?;
    sqlx::query(
        "INSERT INTO project_primary_working_folders
            (project_id, working_folder_id, created_at, updated_at)
         SELECT id, ?, created_at, created_at FROM projects WHERE id = ?",
    )
    .bind(&managed_folder_id)
    .bind(project_id)
    .execute(&mut *conn)
    .await
    .map_err(|e| format!("create primary project working folder: {e}"))?;
    Ok(folder_name)
}

/// Returns the folder name this vault keeps the managed working folder `working_folder_id` under,
/// claiming one first when the row arrived from another device or predates its claim.
///
/// A new claim prefers the name the row stores and skips names other folders claimed and entries
/// already inside `projects/` under `vault_root`.
///
/// # Errors
///
/// Returns a message when the row is missing or not managed, when `projects/` cannot be listed,
/// or when the database refuses the claim.
pub async fn claim_managed_folder(
    pool: &SqlitePool,
    vault_root: &Path,
    working_folder_id: &str,
) -> Result<String, String> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|e| format!("begin managed folder claim: {e}"))?;
    if let Some(folder_name) = claimed_folder_name(&mut transaction, working_folder_id).await? {
        return Ok(folder_name);
    }
    let row: Option<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT kind, managed_relative_path, display_name
         FROM project_working_folders WHERE id = ?",
    )
    .bind(working_folder_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|e| format!("load managed working folder: {e}"))?;
    let Some((kind, stored_path, display_name)) = row else {
        return Err("project working folder not found".to_string());
    };
    if kind != "managed" {
        return Err("project working folder is not managed".to_string());
    }
    let preferred = stored_path
        .as_deref()
        .and_then(managed_folder_name)
        .map_or_else(|| portable_folder_name(&display_name), str::to_owned);
    let folder_name = free_folder_name(&mut transaction, Some(vault_root), &preferred).await?;
    insert_location(&mut transaction, working_folder_id, &folder_name).await?;
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit managed folder claim: {e}"))?;
    Ok(folder_name)
}

/// Returns the folder name this vault already claimed for `working_folder_id`, if any.
///
/// # Errors
///
/// Returns a message when the claim cannot be read.
pub async fn claimed_folder_name(
    conn: &mut SqliteConnection,
    working_folder_id: &str,
) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT folder_name FROM project_managed_folder_locations WHERE working_folder_id = ?",
    )
    .bind(working_folder_id)
    .fetch_optional(conn)
    .await
    .map_err(|e| format!("load managed folder location: {e}"))
}

/// Returns `preferred`, or the first suffixed variant of it, that no claimed folder and, with a
/// `vault_root`, no entry inside `projects/` already uses.
async fn free_folder_name(
    conn: &mut SqliteConnection,
    vault_root: Option<&Path>,
    preferred: &str,
) -> Result<String, String> {
    let mut taken: HashSet<String> =
        sqlx::query_scalar::<_, String>("SELECT folder_key FROM project_managed_folder_locations")
            .fetch_all(&mut *conn)
            .await
            .map_err(|e| format!("load managed folder locations: {e}"))?
            .into_iter()
            .collect();
    if let Some(vault_root) = vault_root {
        taken.extend(
            existing_folder_keys(vault_root)
                .map_err(|e| format!("list managed project folders: {e}"))?,
        );
    }
    unique_folder_name(preferred, |name| taken.contains(&managed_folder_key(name)))
}

async fn insert_location(
    conn: &mut SqliteConnection,
    working_folder_id: &str,
    folder_name: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO project_managed_folder_locations (working_folder_id, folder_name, folder_key)
         VALUES (?, ?, ?)",
    )
    .bind(working_folder_id)
    .bind(folder_name)
    .bind(managed_folder_key(folder_name))
    .execute(conn)
    .await
    .map_err(|e| format!("claim managed project folder: {e}"))?;
    Ok(())
}

//! Managed Chat files of projects that are being deleted.
//!
//! Deleting a project cascades into the attachment and resource rows of its working folders,
//! but the files those rows name live under the vault's managed Chat directories. The caller
//! collects the paths before the deleting transaction and removes the files after it commits.

use super::attachments::resolve_managed_chat_path;
use crate::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_atomic_files::remove_file_within;
use sqlx::SqlitePool;
use std::collections::BTreeSet;
use std::path::Path;

/// A managed Chat file that could not be removed after its project was deleted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedFileRemovalFailure {
    pub managed_relative_path: String,
    pub reason: String,
}

/// Returns the managed attachment and resource paths of the working folders of every project
/// in `project_ids`, sorted and without duplicates.
pub async fn managed_file_paths_of_projects(
    pool: &SqlitePool,
    project_ids: &[String],
) -> ChatResult<Vec<String>> {
    let mut paths = BTreeSet::new();
    for project_id in project_ids {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT attachment.managed_relative_path
             FROM chat_attachments attachment
             JOIN project_working_folders folder ON folder.id = attachment.working_folder_id
             WHERE folder.project_id = ?1
             UNION
             SELECT resource.managed_relative_path
             FROM chat_resources resource
             JOIN project_working_folders folder ON folder.id = resource.working_folder_id
             WHERE folder.project_id = ?1",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await
        .map_err(persistence_error)?;
        paths.extend(rows);
    }
    Ok(paths.into_iter().collect())
}

/// Removes each managed Chat file in `paths` that no attachment or resource row names any
/// more, and returns the files whose removal failed.
///
/// A path still named by a row is kept, so a delete that did not commit, or a row that shares
/// the file, never loses its bytes. A missing file counts as removed.
pub async fn remove_unreferenced_managed_files(
    pool: &SqlitePool,
    vault_root: &Path,
    paths: &[String],
) -> ChatResult<Vec<ManagedFileRemovalFailure>> {
    let mut failures = Vec::new();
    for relative_path in paths {
        let referenced: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chat_attachments WHERE managed_relative_path = ?1)
                 OR EXISTS(SELECT 1 FROM chat_resources WHERE managed_relative_path = ?1)",
        )
        .bind(relative_path)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        if referenced {
            continue;
        }
        let removal = resolve_managed_chat_path(vault_root, relative_path)
            .map_err(|error| error.message)
            .and_then(|path| {
                remove_file_within(vault_root, &path).map_err(|error| error.to_string())
            });
        if let Err(reason) = removal {
            failures.push(ManagedFileRemovalFailure {
                managed_relative_path: relative_path.clone(),
                reason,
            });
        }
    }
    Ok(failures)
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat project files could not be read",
        true,
    )
}

#[cfg(test)]
mod tests;

//! Effective primary working folder of a project, resolved without the native runtime.

use crate::models::{ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId};

/// Source, filter, and order of the candidates for a project's effective primary folder, best first:
/// the stored primary folder when it belongs to the project and is not archived, then the project's
/// managed folder. It follows a select list over `folder` and binds the project id once.
macro_rules! primary_folder_candidates {
    () => {
        "FROM project_working_folders folder
     LEFT JOIN project_primary_working_folders primary_folder
       ON primary_folder.project_id = folder.project_id
      AND primary_folder.working_folder_id = folder.id
      AND folder.archived_at IS NULL
     WHERE folder.project_id = ?
       AND (primary_folder.project_id IS NOT NULL OR folder.kind = 'managed')
     ORDER BY primary_folder.project_id IS NULL, folder.sort_order, folder.id"
    };
}
#[cfg(feature = "native-runtime")]
pub(crate) use primary_folder_candidates;

/// Returns the id of the effective primary working folder of `project_id`, taking the first
/// candidate whose id parses, or `None` when the project has no candidate on this replica.
///
/// Every agent run of a project's channels starts in this folder by default, so an attachment
/// imported for it reaches those runs.
///
/// # Errors
///
/// Returns a persistence error when the query fails.
pub async fn resolve_primary_working_folder_id<'e, E>(
    executor: E,
    project_id: &str,
) -> ChatResult<Option<ProjectWorkingFolderId>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let ids: Vec<String> =
        sqlx::query_scalar(concat!("SELECT folder.id ", primary_folder_candidates!()))
            .bind(project_id)
            .fetch_all(executor)
            .await
            .map_err(|_| {
                ChatError::new(
                    ChatErrorCode::Persistence,
                    "read the primary working folder",
                    true,
                )
            })?;
    Ok(ids
        .into_iter()
        .find_map(|id| ProjectWorkingFolderId::new(id).ok()))
}

//! Durable cleanup service for exact checkpoint refs.

use crate::chat::workspace::WorkingFolderAuthorizationOperation;
use ganbaru_chat::checkpoints::delete_exact_ref;
use ganbaru_chat::repository::lifecycle::release_stale_cleanup_jobs;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ProjectWorkingFolderId, UtcTimestamp,
};
use sqlx::{Row, SqlitePool};

const CHECKPOINT_REF_CLEANUP_KIND: &str = "checkpoint_ref";

/// One claimed checkpoint ref deletion.
struct CheckpointCleanupJob {
    working_folder_id: Option<String>,
    reference: String,
    repository_identity: Option<String>,
    expected_object_id: Option<String>,
}

/// Deletes due checkpoint refs. Every claimed job ends completed or failed, and
/// jobs an interrupted run left running are released for retry first.
pub(crate) async fn run_checkpoint_cleanup(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    run_checkpoint_cleanup_with(pool, now, |job| delete_checkpoint_ref(app, pool, job)).await
}

/// Runs due checkpoint cleanup jobs with `delete_ref` performing each deletion.
async fn run_checkpoint_cleanup_with<F, Fut>(
    pool: &SqlitePool,
    now: &UtcTimestamp,
    mut delete_ref: F,
) -> ChatResult<u64>
where
    F: FnMut(CheckpointCleanupJob) -> Fut,
    Fut: std::future::Future<Output = ChatResult<()>>,
{
    release_stale_cleanup_jobs(pool, CHECKPOINT_REF_CLEANUP_KIND, now).await?;
    let rows = sqlx::query(
        "SELECT id, working_folder_id, exact_target, repository_identity, expected_object_id
         FROM chat_cleanup_jobs
         WHERE cleanup_kind = ? AND state IN ('pending', 'failed')
           AND available_at <= ?
         ORDER BY available_at, id LIMIT 100",
    )
    .bind(CHECKPOINT_REF_CLEANUP_KIND)
    .bind(now.as_str())
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut completed = 0;
    for row in rows {
        let cleanup_id: String = row.try_get("id").map_err(persistence_error)?;
        let job = CheckpointCleanupJob {
            working_folder_id: row
                .try_get("working_folder_id")
                .map_err(persistence_error)?,
            reference: row.try_get("exact_target").map_err(persistence_error)?,
            repository_identity: row
                .try_get("repository_identity")
                .map_err(persistence_error)?,
            expected_object_id: row
                .try_get("expected_object_id")
                .map_err(persistence_error)?,
        };
        let claimed = sqlx::query(
            "UPDATE chat_cleanup_jobs
             SET state = 'running', attempt_count = attempt_count + 1, updated_at = ?
             WHERE id = ? AND state IN ('pending', 'failed')",
        )
        .bind(now.as_str())
        .bind(&cleanup_id)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        match delete_ref(job).await {
            Ok(()) => {
                sqlx::query(
                    "UPDATE chat_cleanup_jobs
                     SET state = 'completed', last_error_code = NULL, updated_at = ? WHERE id = ?",
                )
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(pool)
                .await
                .map_err(persistence_error)?;
                completed += 1;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE chat_cleanup_jobs
                     SET state = 'failed', last_error_code = ?, updated_at = ? WHERE id = ?",
                )
                .bind(format!("{:?}", error.code).to_lowercase())
                .bind(now.as_str())
                .bind(&cleanup_id)
                .execute(pool)
                .await
                .map_err(persistence_error)?;
            }
        }
    }
    Ok(completed)
}

async fn delete_checkpoint_ref(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    job: CheckpointCleanupJob,
) -> ChatResult<()> {
    let (Some(working_folder_id), Some(repository_identity), Some(expected_object_id)) = (
        job.working_folder_id,
        job.repository_identity,
        job.expected_object_id,
    ) else {
        return Err(corrupt_data_error());
    };
    let working_folder_id =
        ProjectWorkingFolderId::new(working_folder_id).map_err(|_| corrupt_data_error())?;
    let authorized = super::super::workspace::commands::authorize_working_folder(
        app,
        pool,
        &working_folder_id,
        WorkingFolderAuthorizationOperation::Restore,
    )
    .await?;
    if authorized.repository_identity.as_deref() != Some(&repository_identity) {
        return Err(ChatError::new(
            ChatErrorCode::ConfigurationInvalid,
            "Checkpoint cleanup repository identity changed",
            true,
        ));
    }
    let reference = job.reference;
    tauri::async_runtime::spawn_blocking(move || {
        delete_exact_ref(authorized.git_folder(), &reference, &expected_object_id)
    })
    .await
    .map_err(|_| checkpoint_command_error())?
}

fn checkpoint_command_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Checkpoint cleanup worker stopped",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat checkpoint cleanup failed",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat checkpoint cleanup data is invalid",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tests::repository::pool_with_thread;

    const NOW: &str = "2026-07-20T12:00:00.000Z";
    const OBJECT_ID: &str = "0123456789abcdef0123456789abcdef01234567";

    async fn insert_job(pool: &SqlitePool, id: &str, state: &str, updated_at: &str) {
        sqlx::query(
            "INSERT INTO chat_cleanup_jobs
                (id, working_folder_id, cleanup_kind, exact_target, repository_identity,
                 expected_object_id, state, available_at, created_at, updated_at)
             VALUES (?, 'workspace-1', 'checkpoint_ref', ?, 'repository-1', ?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(format!("refs/ganbaru/checkpoints/{id}"))
        .bind(OBJECT_ID)
        .bind(state)
        .bind("2026-07-20T11:00:00.000Z")
        .bind("2026-07-20T11:00:00.000Z")
        .bind(updated_at)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn job_state(pool: &SqlitePool, id: &str) -> (String, Option<String>) {
        let row = sqlx::query("SELECT state, last_error_code FROM chat_cleanup_jobs WHERE id = ?")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap();
        (row.get("state"), row.get("last_error_code"))
    }

    #[test]
    fn failed_and_interrupted_jobs_never_stay_running() {
        tauri::async_runtime::block_on(async {
            let pool = pool_with_thread().await;
            let now = UtcTimestamp::new(NOW).unwrap();
            insert_job(&pool, "a-unauthorized", "pending", NOW).await;
            insert_job(&pool, "b-pending", "pending", NOW).await;
            insert_job(
                &pool,
                "c-interrupted",
                "running",
                "2026-07-20T11:30:00.000Z",
            )
            .await;
            insert_job(
                &pool,
                "d-in-progress",
                "running",
                "2026-07-20T11:59:00.000Z",
            )
            .await;

            let mut attempted = Vec::new();
            let completed = run_checkpoint_cleanup_with(&pool, &now, |job| {
                attempted.push(job.reference.clone());
                std::future::ready(if job.reference.ends_with("a-unauthorized") {
                    Err(ChatError::new(
                        ChatErrorCode::ConfigurationInvalid,
                        "This device has no binding for the project working folder",
                        true,
                    ))
                } else {
                    Ok(())
                })
            })
            .await
            .unwrap();

            assert_eq!(completed, 2);
            assert_eq!(
                attempted,
                [
                    "refs/ganbaru/checkpoints/a-unauthorized",
                    "refs/ganbaru/checkpoints/b-pending",
                    "refs/ganbaru/checkpoints/c-interrupted",
                ]
            );
            assert_eq!(
                job_state(&pool, "a-unauthorized").await,
                (
                    "failed".to_string(),
                    Some("configurationinvalid".to_string())
                )
            );
            assert_eq!(job_state(&pool, "b-pending").await.0, "completed");
            assert_eq!(job_state(&pool, "c-interrupted").await.0, "completed");
            assert_eq!(job_state(&pool, "d-in-progress").await.0, "running");
        });
    }
}

use crate::models::{ChatError, ChatErrorCode, ChatResult, ChatThreadId, UtcTimestamp};
use sqlx::SqlitePool;

pub async fn rename_thread(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    title: &str,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let title = title.trim();
    if title.is_empty() || title.len() > 1000 {
        return Err(ChatError::validation(
            "title",
            "Chat thread title is invalid",
        ));
    }
    update_revision(
        pool,
        "UPDATE chat_threads SET title = ?, title_source = 'user', revision = revision + 1,
                updated_at = ? WHERE id = ? AND revision = ?",
        |query| {
            query
                .bind(title)
                .bind(now.as_str())
                .bind(thread_id.as_str())
                .bind(i64_value(expected_revision))
        },
        thread_id,
    )
    .await
}

pub async fn set_thread_read(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    read: bool,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let updated = if read {
        sqlx::query(
            "UPDATE chat_threads SET read_revision = revision + 1, unread_at = NULL,
                    revision = revision + 1, updated_at = ?
             WHERE id = ? AND revision = ?",
        )
        .bind(now.as_str())
        .bind(thread_id.as_str())
        .bind(i64_value(expected_revision))
        .execute(pool)
        .await
        .map_err(persistence_error)?
    } else {
        sqlx::query(
            "UPDATE chat_threads SET unread_at = ?, revision = revision + 1, updated_at = ?
             WHERE id = ? AND revision = ?",
        )
        .bind(now.as_str())
        .bind(now.as_str())
        .bind(thread_id.as_str())
        .bind(i64_value(expected_revision))
        .execute(pool)
        .await
        .map_err(persistence_error)?
    };
    require_updated(pool, thread_id, updated.rows_affected()).await
}

pub async fn set_thread_archived(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    archived: bool,
    expected_revision: u64,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let archived_at = archived.then_some(now.as_str());
    let state = if archived { "archived" } else { "idle" };
    let updated = sqlx::query(
        "UPDATE chat_threads SET archived_at = ?, state = ?, revision = revision + 1,
                updated_at = ? WHERE id = ? AND revision = ?",
    )
    .bind(archived_at)
    .bind(state)
    .bind(now.as_str())
    .bind(thread_id.as_str())
    .bind(i64_value(expected_revision))
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    require_updated(pool, thread_id, updated.rows_affected()).await
}

pub async fn permanently_delete_thread(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    expected_revision: u64,
    cleanup_available_at: &UtcTimestamp,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    match revision {
        None => return Err(not_found_error()),
        Some(value) if value != i64_value(expected_revision) => return Err(stale_error()),
        Some(_) => {}
    }
    let working_folder_id: String =
        sqlx::query_scalar("SELECT working_folder_id FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(persistence_error)?;
    let checkpoints = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT id, hidden_ref_name, repository_identity, git_object_id
         FROM chat_checkpoints WHERE thread_id = ?",
    )
    .bind(thread_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for (checkpoint_id, hidden_ref, repository_identity, object_id) in checkpoints {
        enqueue_cleanup(
            &mut transaction,
            CleanupRequest {
                id: &format!("cleanup:checkpoint:{checkpoint_id}"),
                thread_id: Some(thread_id.as_str()),
                kind: "checkpoint_ref",
                target: &hidden_ref,
                repository_identity: Some(&repository_identity),
                working_folder_id: Some(&working_folder_id),
                expected_object_id: Some(&object_id),
                available_at: now,
                now,
            },
        )
        .await?;
    }
    let attachments: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT a.id, a.managed_relative_path
         FROM chat_attachments a
         JOIN chat_attachment_references r ON r.attachment_id = a.id
         LEFT JOIN chat_messages m ON m.id = r.message_id
         LEFT JOIN chat_drafts d ON d.id = r.draft_id
         WHERE m.thread_id = ? OR d.thread_id = ?",
    )
    .bind(thread_id.as_str())
    .bind(thread_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let browser_artifacts: Vec<(String, String)> = sqlx::query_as(
        "SELECT resource.id, resource.managed_relative_path
         FROM chat_resources resource
         JOIN chat_resource_thread_references reference ON reference.resource_id = resource.id
         WHERE reference.thread_id = ? AND resource.attachment_id IS NULL
           AND resource.resource_kind IN ('browser_screenshot', 'browser_recording')",
    )
    .bind(thread_id.as_str())
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query("DELETE FROM chat_threads WHERE id = ?")
        .bind(thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    for (attachment_id, relative_path) in attachments {
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_attachment_references WHERE attachment_id = ?",
        )
        .bind(&attachment_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        if remaining == 0 {
            sqlx::query(
                "UPDATE chat_attachments SET deletion_state = 'pending_delete', unreferenced_at = ?
                 WHERE id = ?",
            )
            .bind(now.as_str())
            .bind(&attachment_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            enqueue_cleanup(
                &mut transaction,
                CleanupRequest {
                    id: &format!("cleanup:attachment:{attachment_id}"),
                    thread_id: Some(thread_id.as_str()),
                    kind: "attachment_file",
                    target: &relative_path,
                    repository_identity: None,
                    working_folder_id: Some(&working_folder_id),
                    expected_object_id: None,
                    available_at: cleanup_available_at,
                    now,
                },
            )
            .await?;
        }
    }
    for (resource_id, relative_path) in browser_artifacts {
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_resource_thread_references WHERE resource_id = ?",
        )
        .bind(&resource_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        if remaining == 0 {
            sqlx::query(
                "UPDATE chat_resources SET integrity_state = 'deleted', deleted_at = ?
                 WHERE id = ?",
            )
            .bind(now.as_str())
            .bind(&resource_id)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
            enqueue_cleanup(
                &mut transaction,
                CleanupRequest {
                    id: &format!("cleanup:browser-artifact:{resource_id}"),
                    thread_id: Some(thread_id.as_str()),
                    kind: "attachment_file",
                    target: &relative_path,
                    repository_identity: None,
                    working_folder_id: Some(&working_folder_id),
                    expected_object_id: None,
                    available_at: cleanup_available_at,
                    now,
                },
            )
            .await?;
        }
    }
    transaction.commit().await.map_err(persistence_error)
}

pub async fn resolve_project_deletion(
    pool: &SqlitePool,
    project_id: &str,
    cleanup_available_at: &UtcTimestamp,
    now: &UtcTimestamp,
) -> ChatResult<u64> {
    let thread_rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, revision FROM chat_threads WHERE project_id = ? ORDER BY id")
            .bind(project_id)
            .fetch_all(pool)
            .await
            .map_err(persistence_error)?;
    for (thread_id, revision) in &thread_rows {
        permanently_delete_thread(
            pool,
            &ChatThreadId::new(thread_id.clone()).map_err(|_| corrupt_data_error())?,
            u64::try_from(*revision).map_err(|_| corrupt_data_error())?,
            cleanup_available_at,
            now,
        )
        .await?;
    }
    Ok(thread_rows.len() as u64)
}

struct CleanupRequest<'a> {
    id: &'a str,
    thread_id: Option<&'a str>,
    kind: &'a str,
    target: &'a str,
    repository_identity: Option<&'a str>,
    working_folder_id: Option<&'a str>,
    expected_object_id: Option<&'a str>,
    available_at: &'a UtcTimestamp,
    now: &'a UtcTimestamp,
}

async fn enqueue_cleanup(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    request: CleanupRequest<'_>,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_cleanup_jobs
            (id, source_thread_id, cleanup_kind, exact_target, repository_identity,
             available_at, created_at, updated_at, working_folder_id, expected_object_id)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(cleanup_kind, exact_target) DO UPDATE SET
            state = CASE WHEN chat_cleanup_jobs.state = 'completed' THEN 'completed' ELSE 'pending' END,
            available_at = MIN(chat_cleanup_jobs.available_at, excluded.available_at),
            updated_at = excluded.updated_at",
    )
    .bind(request.id)
    .bind(request.thread_id)
    .bind(request.kind)
    .bind(request.target)
    .bind(request.repository_identity)
    .bind(request.available_at.as_str())
    .bind(request.now.as_str())
    .bind(request.now.as_str())
    .bind(request.working_folder_id)
    .bind(request.expected_object_id)
    .execute(&mut **transaction)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn update_revision<'q, F>(
    pool: &SqlitePool,
    sql: &'q str,
    bind: F,
    thread_id: &ChatThreadId,
) -> ChatResult<u64>
where
    F: FnOnce(
        sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
{
    let result = bind(sqlx::query(sql))
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    require_updated(pool, thread_id, result.rows_affected()).await
}

async fn require_updated(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    rows: u64,
) -> ChatResult<u64> {
    if rows == 1 {
        return sqlx::query_scalar::<_, i64>("SELECT revision FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_one(pool)
            .await
            .map_err(persistence_error)
            .and_then(|value| u64::try_from(value).map_err(|_| corrupt_data_error()));
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM chat_threads WHERE id = ?)")
        .bind(thread_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    Err(if exists {
        stale_error()
    } else {
        not_found_error()
    })
}

fn i64_value(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
fn not_found_error() -> ChatError {
    ChatError::new(ChatErrorCode::NotFound, "Chat thread was not found", true)
}
fn stale_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::StaleRevision,
        "Chat thread revision is stale",
        true,
    )
}
fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat lifecycle persistence failed",
        true,
    )
}
fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat lifecycle data is invalid",
        false,
    )
}

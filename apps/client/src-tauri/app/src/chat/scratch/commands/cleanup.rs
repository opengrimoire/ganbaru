//! Confirmed scratch cleanup, including unavailable-device and retryable failure records.

use super::inspection::read_generation_size;
use super::{ScratchIdentity, read_scratch_identity, require_local_owner};
use crate::chat::channels::{
    chat_pool, i64_value, identifier_error, now_timestamp, persistence_error, u64_value,
};
use crate::chat::device_state::update_active_device_scope;
use crate::chat::scratch;
use ganbaru_chat::coordination::contracts::*;
use ganbaru_chat_contracts::models::*;
use sqlx::SqlitePool;

#[tauri::command]
pub async fn chat_preview_scratch_cleanup(
    app: tauri::AppHandle,
    db_url: String,
    request: PreviewChatScratchCleanupCommand,
) -> ChatResult<ChatScratchCleanupPreviewRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let identity = read_scratch_identity(&pool, &request.scratch_generation_id).await?;
    let (device_availability, size) = read_generation_size(&app, &pool, &identity).await?;
    let active_run_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_agent_runs
         WHERE scratch_generation_id = ?
           AND state IN ('queued', 'starting', 'working', 'waiting')",
    )
    .bind(identity.generation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    let other_generation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_scratch_generations
         WHERE scratch_scope_id = ? AND id != ?
           AND removed_at IS NULL AND lifecycle_state != 'removed'",
    )
    .bind(identity.scope_id.as_str())
    .bind(identity.generation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    Ok(ChatScratchCleanupPreviewRead {
        scratch_scope_id: identity.scope_id,
        scratch_generation_id: identity.generation_id,
        expected_scope_revision: identity.scope_revision,
        lifecycle_state: identity.generation_lifecycle,
        byte_size: size.bytes,
        entry_count: size.entries,
        size_truncated: size.truncated,
        device_availability,
        active_run_count: u64_value(active_run_count)?,
        will_remove_scope: other_generation_count == 0,
    })
}

#[tauri::command]
pub async fn chat_cleanup_scratch(
    app: tauri::AppHandle,
    db_url: String,
    request: CleanupChatScratchCommand,
) -> ChatResult<ChatScratchCleanupResultRead> {
    if !request.confirmed {
        return Err(ChatError::new(
            ChatErrorCode::Permission,
            "Private scratch cleanup requires explicit confirmation",
            true,
        ));
    }
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let identity = read_scratch_identity(&pool, &request.scratch_generation_id).await?;
    if identity.scope_revision != request.expected_scope_revision {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Private scratch changed before cleanup",
            true,
        ));
    }
    if !matches!(
        identity.scope_lifecycle,
        ChatScratchScopeLifecycleState::Active
            | ChatScratchScopeLifecycleState::Archived
            | ChatScratchScopeLifecycleState::CleanupFailed
    ) || !matches!(
        identity.generation_lifecycle,
        ChatScratchGenerationLifecycleState::Active
            | ChatScratchGenerationLifecycleState::Quarantined
            | ChatScratchGenerationLifecycleState::CleanupFailed
    ) {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Private scratch is already being cleaned",
            true,
        ));
    }
    let active_run_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_agent_runs
         WHERE scratch_generation_id = ?
           AND state IN ('queued', 'starting', 'working', 'waiting')",
    )
    .bind(identity.generation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    if active_run_count != 0 {
        return Err(ChatError::new(
            ChatErrorCode::Conflict,
            "Active work must stop before private scratch can be cleaned",
            true,
        ));
    }
    let existing_job_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM chat_scratch_cleanup_jobs WHERE scratch_generation_id = ?",
    )
    .bind(identity.generation_id.as_str())
    .fetch_optional(&pool)
    .await
    .map_err(persistence_error)?;
    let job_id = match existing_job_id {
        Some(value) => ChatScratchCleanupJobId::new(value).map_err(identifier_error)?,
        None => {
            ChatScratchCleanupJobId::new(new_id("scratch-cleanup")).map_err(identifier_error)?
        }
    };
    let now = now_timestamp()?;
    let path = scratch::resolve_managed_scratch_path_for_inspection(
        &app,
        &pool,
        identity.generation_id.as_str(),
        identity.execution_environment_id.as_str(),
    )
    .await;
    let path = match path {
        Ok(path) => path,
        Err(_) => {
            persist_unavailable_cleanup_job(&pool, &identity, &job_id, &now).await?;
            return Ok(ChatScratchCleanupResultRead {
                job_id,
                scratch_scope_id: identity.scope_id,
                scratch_generation_id: identity.generation_id,
                scope_revision: identity.scope_revision,
                state: ChatScratchCleanupJobState::UnavailableOnThisDevice,
                removed_bytes: 0,
                device_availability: ChatScratchDeviceAvailability::UnavailableOnThisDevice,
                completed_at: None,
            });
        }
    };
    let other_generation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chat_scratch_generations
         WHERE scratch_scope_id = ? AND id != ?
           AND removed_at IS NULL AND lifecycle_state != 'removed'",
    )
    .bind(identity.scope_id.as_str())
    .bind(identity.generation_id.as_str())
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    let next_scope_revision = identity.scope_revision.saturating_add(1);
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "INSERT INTO chat_scratch_cleanup_jobs
            (id, scratch_scope_id, scratch_generation_id, expected_scope_revision,
             state, attempt_count, confirmed_at, available_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'pending', 0, ?, ?, ?, ?)
         ON CONFLICT(scratch_generation_id) DO UPDATE SET
             expected_scope_revision = excluded.expected_scope_revision,
             state = 'pending', last_error_code = NULL,
             confirmed_at = excluded.confirmed_at,
             available_at = excluded.available_at,
             updated_at = excluded.updated_at",
    )
    .bind(job_id.as_str())
    .bind(identity.scope_id.as_str())
    .bind(identity.generation_id.as_str())
    .bind(i64_value(identity.scope_revision)?)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let scope_updated = sqlx::query(
        "UPDATE chat_scratch_scopes
         SET lifecycle_state = CASE WHEN ? = 0 THEN 'cleanup_pending' ELSE lifecycle_state END,
             revision = revision + 1, updated_at = ?
         WHERE id = ? AND revision = ? AND removed_at IS NULL",
    )
    .bind(other_generation_count)
    .bind(now.as_str())
    .bind(identity.scope_id.as_str())
    .bind(i64_value(identity.scope_revision)?)
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if scope_updated.rows_affected() != 1 {
        return Err(ChatError::new(
            ChatErrorCode::StaleRevision,
            "Private scratch changed before cleanup",
            true,
        ));
    }
    sqlx::query(
        "UPDATE chat_scratch_generations
         SET lifecycle_state = 'cleanup_pending', updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(identity.generation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_execution_environments
         SET lifecycle_state = 'cleanup_pending', updated_at = ?
         WHERE id = ? AND scratch_generation_id = ?",
    )
    .bind(now.as_str())
    .bind(identity.execution_environment_id.as_str())
    .bind(identity.generation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_scratch_cleanup_jobs
         SET state = 'running', attempt_count = attempt_count + 1, updated_at = ?
         WHERE id = ? AND state = 'pending'",
    )
    .bind(now.as_str())
    .bind(job_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;

    let removed_bytes = match scratch::remove_managed_scratch_generation(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            let failed_at = now_timestamp()?;
            mark_cleanup_failed(
                &pool,
                &identity,
                &job_id,
                other_generation_count == 0,
                &failed_at,
            )
            .await?;
            return Err(error);
        }
    };
    update_active_device_scope(&app, |scope| {
        scope
            .execution_environment_paths
            .remove(identity.execution_environment_id.as_str());
        Ok(())
    })
    .map_err(device_state_error)?;
    let completed = now_timestamp()?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_scratch_generations
         SET lifecycle_state = 'removed', byte_size = 0,
             updated_at = ?, removed_at = ? WHERE id = ?",
    )
    .bind(completed.as_str())
    .bind(completed.as_str())
    .bind(identity.generation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_execution_environments
         SET lifecycle_state = 'removed', updated_at = ?, archived_at = ?
         WHERE id = ? AND scratch_generation_id = ?",
    )
    .bind(completed.as_str())
    .bind(completed.as_str())
    .bind(identity.execution_environment_id.as_str())
    .bind(identity.generation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if other_generation_count == 0 {
        sqlx::query(
            "UPDATE chat_scratch_scopes
             SET lifecycle_state = 'removed', updated_at = ?, removed_at = ? WHERE id = ?",
        )
        .bind(completed.as_str())
        .bind(completed.as_str())
        .bind(identity.scope_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    sqlx::query(
        "UPDATE chat_scratch_cleanup_jobs
         SET state = 'completed', removed_bytes = ?, last_error_code = NULL,
             updated_at = ?, completed_at = ? WHERE id = ?",
    )
    .bind(i64::try_from(removed_bytes).unwrap_or(i64::MAX))
    .bind(completed.as_str())
    .bind(completed.as_str())
    .bind(job_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(ChatScratchCleanupResultRead {
        job_id,
        scratch_scope_id: identity.scope_id,
        scratch_generation_id: identity.generation_id,
        scope_revision: next_scope_revision,
        state: ChatScratchCleanupJobState::Completed,
        removed_bytes,
        device_availability: ChatScratchDeviceAvailability::UnavailableOnThisDevice,
        completed_at: Some(completed),
    })
}

async fn persist_unavailable_cleanup_job(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
    job_id: &ChatScratchCleanupJobId,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    sqlx::query(
        "INSERT INTO chat_scratch_cleanup_jobs
            (id, scratch_scope_id, scratch_generation_id, expected_scope_revision,
             state, attempt_count, last_error_code, confirmed_at, available_at,
             created_at, updated_at)
         VALUES (?, ?, ?, ?, 'unavailable_on_device', 1, 'missing_on_device', ?, ?, ?, ?)
         ON CONFLICT(scratch_generation_id) DO UPDATE SET
             expected_scope_revision = excluded.expected_scope_revision,
             state = 'unavailable_on_device', attempt_count = attempt_count + 1,
             last_error_code = 'missing_on_device', confirmed_at = excluded.confirmed_at,
             available_at = excluded.available_at, updated_at = excluded.updated_at",
    )
    .bind(job_id.as_str())
    .bind(identity.scope_id.as_str())
    .bind(identity.generation_id.as_str())
    .bind(i64_value(identity.scope_revision)?)
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

async fn mark_cleanup_failed(
    pool: &SqlitePool,
    identity: &ScratchIdentity,
    job_id: &ChatScratchCleanupJobId,
    scope_was_pending: bool,
    now: &UtcTimestamp,
) -> ChatResult<()> {
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_scratch_cleanup_jobs
         SET state = 'failed', last_error_code = 'io', updated_at = ?, available_at = ?
         WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(now.as_str())
    .bind(job_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_scratch_generations
         SET lifecycle_state = 'cleanup_failed', updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(identity.generation_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    sqlx::query(
        "UPDATE chat_execution_environments
         SET lifecycle_state = 'cleanup_failed', updated_at = ? WHERE id = ?",
    )
    .bind(now.as_str())
    .bind(identity.execution_environment_id.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if scope_was_pending {
        sqlx::query(
            "UPDATE chat_scratch_scopes
             SET lifecycle_state = 'cleanup_failed', updated_at = ? WHERE id = ?",
        )
        .bind(now.as_str())
        .bind(identity.scope_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)
}

fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}:{:x}:{sequence:x}", nanos)
}

fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Private scratch device state could not be updated",
        true,
    )
}

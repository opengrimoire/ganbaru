//! Immediate runtime cleanup for durable organizational access revocations.

use super::internal_mcp::generate_opaque_handle;
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatAgentRunId, ChatAuthorizationRevisionId, ChatError, ChatErrorCode, ChatResult,
    ChatThreadId, ChatTurnId,
};
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use tauri::Manager;

const REVOCATION_CLAIM_TIMEOUT_SECONDS: i64 = 5 * 60;
const REVOCATION_DRAIN_BATCH_SIZE: i64 = 32;
const REVOCATION_RETRY_BASE_SECONDS: i64 = 5;
const REVOCATION_RETRY_MAX_SECONDS: i64 = 5 * 60;
const REVOCATION_RETRY_MAX_POLLS: u32 = 60;
const REVOCATION_RETRY_POLL: Duration = Duration::from_secs(5);
const REVOCATION_STOP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct RevokedRun {
    thread_id: ChatThreadId,
    run_id: ChatAgentRunId,
    turn_id: ChatTurnId,
    authorization_revision_id: ChatAuthorizationRevisionId,
}

pub(crate) async fn drain_access_revocation_jobs(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) -> ChatResult<u64> {
    recover_stale_claims(pool).await?;
    let completed = drain_ready_jobs(app, pool).await?;
    if has_pending_jobs(pool).await? {
        let retry_app = app.clone();
        let retry_pool = pool.clone();
        tauri::async_runtime::spawn(async move {
            retry_pending_jobs(retry_app, retry_pool).await;
        });
    }
    Ok(completed)
}

pub(crate) fn start_startup_recovery(app: &tauri::AppHandle) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
        let pool = match crate::db::connect_sqlite(app.clone(), db_url).await {
            Ok(pool) => pool,
            Err(error) => {
                eprintln!("Chat revocation startup recovery could not open the database: {error}");
                return;
            }
        };
        if let Err(error) = drain_access_revocation_jobs(&app, &pool).await {
            eprintln!(
                "Chat revocation startup recovery failed with code {:?}",
                error.code
            );
        }
    });
}

async fn drain_ready_jobs(app: &tauri::AppHandle, pool: &SqlitePool) -> ChatResult<u64> {
    let now = now_timestamp();
    let job_ids = sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_access_revocation_jobs
         WHERE state IN ('queued', 'failed') AND available_at <= ?
         ORDER BY available_at, created_at, id
         LIMIT ?",
    )
    .bind(&now)
    .bind(REVOCATION_DRAIN_BATCH_SIZE)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut completed = 0_u64;
    for job_id in job_ids {
        let claim_token = generate_opaque_handle("revocation-claim")?;
        let claim_time = now_timestamp();
        let claimed = sqlx::query(
            "UPDATE chat_access_revocation_jobs
             SET state = 'claimed', claimed_at = ?, claim_token = ?,
                 attempt_count = attempt_count + 1, updated_at = ?
             WHERE id = ? AND state IN ('queued', 'failed') AND available_at <= ?",
        )
        .bind(&claim_time)
        .bind(&claim_token)
        .bind(&claim_time)
        .bind(&job_id)
        .bind(&claim_time)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
        if claimed.rows_affected() != 1 {
            continue;
        }
        if let Err(error) = drain_claimed_job(app, pool, &job_id, &claim_token).await {
            mark_claim_failed(pool, &job_id, &claim_token, &error).await?;
            continue;
        }
        completed = completed.saturating_add(1);
    }
    Ok(completed)
}

async fn retry_pending_jobs(app: tauri::AppHandle, pool: SqlitePool) {
    for _ in 0..REVOCATION_RETRY_MAX_POLLS {
        tokio::time::sleep(REVOCATION_RETRY_POLL).await;
        if recover_stale_claims(&pool).await.is_err()
            || drain_ready_jobs(&app, &pool).await.is_err()
        {
            return;
        }
        match has_pending_jobs(&pool).await {
            Ok(true) => {}
            Ok(false) | Err(_) => return,
        }
    }
}

async fn has_pending_jobs(pool: &SqlitePool) -> ChatResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_access_revocation_jobs
           WHERE state IN ('queued', 'failed', 'claimed')
         )",
    )
    .fetch_one(pool)
    .await
    .map_err(persistence_error)
}

async fn recover_stale_claims(pool: &SqlitePool) -> ChatResult<()> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(REVOCATION_CLAIM_TIMEOUT_SECONDS))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let stale = sqlx::query(
        "SELECT id, attempt_count FROM chat_access_revocation_jobs
         WHERE state = 'claimed' AND claimed_at <= ?
         ORDER BY claimed_at, id",
    )
    .bind(&cutoff)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    for row in stale {
        let id: String = row.try_get("id").map_err(persistence_error)?;
        let attempt_count: i64 = row.try_get("attempt_count").map_err(persistence_error)?;
        let available_at = retry_available_at(attempt_count);
        let now = now_timestamp();
        sqlx::query(
            "UPDATE chat_access_revocation_jobs
             SET state = 'failed', claim_token = NULL, claimed_at = NULL,
                 last_error = 'Revocation cleanup claim expired',
                 available_at = ?, updated_at = ?
             WHERE id = ? AND state = 'claimed' AND claimed_at <= ?",
        )
        .bind(available_at)
        .bind(now)
        .bind(id)
        .bind(&cutoff)
        .execute(pool)
        .await
        .map_err(persistence_error)?;
    }
    Ok(())
}

async fn drain_claimed_job(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    job_id: &str,
    claim_token: &str,
) -> ChatResult<()> {
    let row = sqlx::query(
        "SELECT job.authorization_revision_id, job.reason
         FROM chat_access_revocation_jobs job
         WHERE job.id = ? AND job.state = 'claimed' AND job.claim_token = ?",
    )
    .bind(job_id)
    .bind(claim_token)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(revocation_error)?;
    let authorization_revision_id: Option<String> = row
        .try_get("authorization_revision_id")
        .map_err(persistence_error)?;
    let reason: String = row.try_get("reason").map_err(persistence_error)?;
    let runs = read_revoked_runs(pool, authorization_revision_id.as_deref()).await?;

    persist_revocation_before_cleanup(
        pool,
        job_id,
        claim_token,
        authorization_revision_id.as_deref(),
        &reason,
        &runs,
    )
    .await?;

    let mcp = app.state::<super::internal_mcp::InternalMcpRegistry>();
    let runtime = app.state::<ChatRuntimeRegistry>();
    let mutations = app.state::<super::workspace::mutation::ChatWorkspaceMutationRegistry>();
    for run in &runs {
        let exact_scope = mcp
            .revoke_matching_run_scope(
                &run.thread_id,
                &run.run_id,
                &run.turn_id,
                &run.authorization_revision_id,
            )
            .await;
        let has_runtime_owner = runtime
            .owners()?
            .into_iter()
            .find(|owner| owner.thread_id() == &run.thread_id)
            .is_some();
        let newer_active_run: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM chat_agent_runs newer
               WHERE newer.provider_thread_id = ?
                 AND newer.authorization_revision_id != ?
                 AND newer.state IN ('starting', 'working', 'waiting')
             )",
        )
        .bind(run.thread_id.as_str())
        .bind(run.authorization_revision_id.as_str())
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        if !newer_active_run && (exact_scope || has_runtime_owner) {
            runtime
                .shutdown_thread_and_remove(&run.thread_id, REVOCATION_STOP_TIMEOUT, &mutations)
                .await?;
        }
    }
    mark_claim_completed(pool, job_id, claim_token).await
}

async fn read_revoked_runs(
    pool: &SqlitePool,
    authorization_revision_id: Option<&str>,
) -> ChatResult<Vec<RevokedRun>> {
    let Some(authorization_revision_id) = authorization_revision_id else {
        return Ok(Vec::new());
    };
    let rows = sqlx::query(
        "SELECT id, provider_thread_id, provider_turn_id, authorization_revision_id
         FROM chat_agent_runs
         WHERE authorization_revision_id = ? AND provider_thread_id IS NOT NULL",
    )
    .bind(authorization_revision_id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(RevokedRun {
                thread_id: ChatThreadId::new(
                    row.try_get::<String, _>("provider_thread_id")
                        .map_err(persistence_error)?,
                )
                .map_err(|_| revocation_error())?,
                run_id: ChatAgentRunId::new(
                    row.try_get::<String, _>("id").map_err(persistence_error)?,
                )
                .map_err(|_| revocation_error())?,
                turn_id: ChatTurnId::new(
                    row.try_get::<String, _>("provider_turn_id")
                        .map_err(persistence_error)?,
                )
                .map_err(|_| revocation_error())?,
                authorization_revision_id: ChatAuthorizationRevisionId::new(
                    row.try_get::<String, _>("authorization_revision_id")
                        .map_err(persistence_error)?,
                )
                .map_err(|_| revocation_error())?,
            })
        })
        .collect()
}

async fn persist_revocation_before_cleanup(
    pool: &SqlitePool,
    job_id: &str,
    claim_token: &str,
    authorization_revision_id: Option<&str>,
    reason: &str,
    runs: &[RevokedRun],
) -> ChatResult<()> {
    let settled_at = now_timestamp();
    let turn_reason = reason.chars().take(1_000).collect::<String>();
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM chat_access_revocation_jobs
           WHERE id = ? AND state = 'claimed' AND claim_token = ?
         )",
    )
    .bind(job_id)
    .bind(claim_token)
    .fetch_one(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    if !owned {
        return Err(revocation_error());
    }
    if let Some(authorization_revision_id) = authorization_revision_id {
        sqlx::query(
            "UPDATE chat_agent_runs
             SET state = 'cancelled', settled_at = coalesce(settled_at, ?), updated_at = ?
             WHERE authorization_revision_id = ?
               AND state IN ('queued', 'starting', 'working', 'waiting')",
        )
        .bind(&settled_at)
        .bind(&settled_at)
        .bind(authorization_revision_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'cancelled', state_reason = ?, settled_at = coalesce(settled_at, ?),
                 revision = revision + 1, updated_at = ?
             WHERE id IN (
               SELECT assignment_id FROM chat_assignment_authorization_revisions
               WHERE id = ?
             ) AND state IN (
               'queued', 'working', 'waiting_for_answer',
               'waiting_for_approval', 'ready_for_review'
             )",
        )
        .bind(reason)
        .bind(&settled_at)
        .bind(&settled_at)
        .bind(authorization_revision_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_assignment_dispatch_jobs
             SET state = 'cancelled', last_error = ?, updated_at = ?
             WHERE assignment_id IN (
               SELECT assignment_id FROM chat_assignment_authorization_revisions
               WHERE id = ?
             ) AND state IN ('queued', 'claimed')",
        )
        .bind(reason)
        .bind(&settled_at)
        .bind(authorization_revision_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    for run in runs {
        sqlx::query(
            "UPDATE chat_turns
             SET state = 'interrupted', completed_at = coalesce(completed_at, ?),
                 stop_reason = ?, updated_at = ?
             WHERE id = ? AND thread_id = ?
               AND state IN ('pending', 'dispatching', 'active',
                             'waiting_for_approval', 'waiting_for_user_input')",
        )
        .bind(&settled_at)
        .bind(&turn_reason)
        .bind(&settled_at)
        .bind(run.turn_id.as_str())
        .bind(run.thread_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_threads
             SET state = 'closed', latest_turn_state = 'interrupted',
                 provider_thread_id = NULL, resume_cursor_schema_version = NULL,
                 resume_cursor_data = NULL, revision = revision + 1, updated_at = ?
             WHERE id = ? AND NOT EXISTS (
               SELECT 1 FROM chat_agent_runs newer
               WHERE newer.provider_thread_id = chat_threads.id
                 AND newer.authorization_revision_id != ?
                 AND newer.state IN ('starting', 'working', 'waiting')
             )",
        )
        .bind(&settled_at)
        .bind(run.thread_id.as_str())
        .bind(run.authorization_revision_id.as_str())
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)
}

async fn mark_claim_completed(
    pool: &SqlitePool,
    job_id: &str,
    claim_token: &str,
) -> ChatResult<()> {
    let completed = sqlx::query(
        "UPDATE chat_access_revocation_jobs
         SET state = 'completed', claim_token = NULL, claimed_at = NULL,
             last_error = NULL, updated_at = ?
         WHERE id = ? AND state = 'claimed' AND claim_token = ?",
    )
    .bind(now_timestamp())
    .bind(job_id)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if completed.rows_affected() == 1 {
        Ok(())
    } else {
        Err(revocation_error())
    }
}

async fn mark_claim_failed(
    pool: &SqlitePool,
    job_id: &str,
    claim_token: &str,
    error: &ChatError,
) -> ChatResult<()> {
    let attempt_count: i64 = sqlx::query_scalar(
        "SELECT attempt_count FROM chat_access_revocation_jobs
         WHERE id = ? AND state = 'claimed' AND claim_token = ?",
    )
    .bind(job_id)
    .bind(claim_token)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?
    .ok_or_else(revocation_error)?;
    sqlx::query(
        "UPDATE chat_access_revocation_jobs
         SET state = 'failed', claim_token = NULL, claimed_at = NULL,
             last_error = ?, available_at = ?, updated_at = ?
         WHERE id = ? AND state = 'claimed' AND claim_token = ?",
    )
    .bind(error.message.chars().take(4_000).collect::<String>())
    .bind(retry_available_at(attempt_count))
    .bind(now_timestamp())
    .bind(job_id)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

fn retry_available_at(attempt_count: i64) -> String {
    let seconds = retry_delay_seconds(attempt_count);
    (chrono::Utc::now() + chrono::Duration::seconds(seconds))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn retry_delay_seconds(attempt_count: i64) -> i64 {
    let exponent = u32::try_from(attempt_count.clamp(0, 6)).unwrap_or(6);
    REVOCATION_RETRY_BASE_SECONDS
        .saturating_mul(1_i64.checked_shl(exponent).unwrap_or(i64::MAX))
        .min(REVOCATION_RETRY_MAX_SECONDS)
}

fn now_timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn revocation_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Organizational revocation cleanup could not be completed",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    revocation_error()
}

#[cfg(test)]
mod tests {
    use super::{
        REVOCATION_DRAIN_BATCH_SIZE, REVOCATION_RETRY_MAX_POLLS, REVOCATION_RETRY_MAX_SECONDS,
        retry_delay_seconds,
    };

    #[test]
    fn revocation_recovery_batches_and_caps_retry_backoff() {
        assert_eq!(REVOCATION_DRAIN_BATCH_SIZE, 32);
        assert_eq!(REVOCATION_RETRY_MAX_POLLS, 60);
        assert_eq!(retry_delay_seconds(0), 5);
        assert_eq!(retry_delay_seconds(1), 10);
        assert_eq!(retry_delay_seconds(100), REVOCATION_RETRY_MAX_SECONDS);
    }
}

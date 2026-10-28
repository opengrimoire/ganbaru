//! Immediate runtime cleanup for durable organizational access revocations.

use super::internal_mcp::{InternalMcpRunScope, generate_opaque_handle};
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatAgentRunId, ChatAuthorizationRevisionId, ChatError, ChatErrorCode, ChatResult,
    ChatThreadId, ChatTurnId, ProjectWorkingFolderId,
};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;
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

/// Selects the active organizational runs whose authority a folder or live-check change ended.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ActiveRunSelector<'a> {
    /// Runs that execute in the working folder or hold it as an authorized folder source.
    WorkingFolder(&'a ProjectWorkingFolderId),
    /// Runs on one provider thread, narrowed to a provider turn when one is known.
    ProviderTurn {
        thread_id: &'a ChatThreadId,
        turn_id: Option<&'a ChatTurnId>,
    },
}

/// Revokes the allowed authorizations behind the active runs `selector` matches and queues
/// their cleanup jobs, so the next drain cancels the runs and stops their providers.
///
/// Queued runs are left alone because they start only after the live checks pass again.
/// Returns whether any authorization was revoked.
pub(crate) async fn revoke_active_run_authorizations(
    pool: &SqlitePool,
    selector: ActiveRunSelector<'_>,
    reason: &str,
) -> ChatResult<bool> {
    let (working_folder_id, thread_id, turn_id) = match selector {
        ActiveRunSelector::WorkingFolder(folder) => (Some(folder.as_str()), None, None),
        ActiveRunSelector::ProviderTurn { thread_id, turn_id } => (
            None,
            Some(thread_id.as_str()),
            turn_id.map(ChatTurnId::as_str),
        ),
    };
    let now = now_timestamp();
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    let rows = sqlx::query(
        "SELECT DISTINCT authorization.id, assignment.teammate_id
         FROM chat_agent_runs run
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = run.authorization_revision_id
         JOIN chat_work_assignments assignment ON assignment.id = authorization.assignment_id
         WHERE run.state IN ('starting', 'working', 'waiting')
           AND authorization.decision_state = 'allowed'
           AND authorization.revoked_at IS NULL
           AND (?1 IS NULL OR run.working_folder_id = ?1 OR EXISTS (
                 SELECT 1 FROM chat_assignment_authorized_folder_sources source
                 WHERE source.authorization_revision_id = authorization.id
                   AND source.working_folder_id = ?1
               ))
           AND (?2 IS NULL OR run.provider_thread_id = ?2)
           AND (?3 IS NULL OR run.provider_turn_id = ?3)
         ORDER BY authorization.id",
    )
    .bind(working_folder_id)
    .bind(thread_id)
    .bind(turn_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    for row in &rows {
        let authorization_id: String = row.try_get("id").map_err(persistence_error)?;
        let teammate_id: String = row.try_get("teammate_id").map_err(persistence_error)?;
        sqlx::query(
            "UPDATE chat_assignment_authorization_revisions
             SET decision_state = 'revoked', reason = ?, revoked_at = ?
             WHERE id = ? AND decision_state = 'allowed' AND revoked_at IS NULL",
        )
        .bind(reason)
        .bind(&now)
        .bind(&authorization_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        sqlx::query(
            "INSERT INTO chat_access_revocation_jobs
                (id, teammate_id, authorization_revision_id, reason,
                 available_at, created_at, updated_at)
             SELECT 'access-revocation:' || lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?
             WHERE NOT EXISTS (
                 SELECT 1 FROM chat_access_revocation_jobs existing
                 WHERE existing.authorization_revision_id = ?
                   AND existing.state IN ('queued', 'claimed')
             )",
        )
        .bind(&teammate_id)
        .bind(&authorization_id)
        .bind(reason)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .bind(&authorization_id)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
    }
    transaction.commit().await.map_err(persistence_error)?;
    Ok(!rows.is_empty())
}

/// Revokes the runs `selector` matches and, when any were revoked, stops them now.
pub(crate) async fn revoke_and_stop_active_runs(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    selector: ActiveRunSelector<'_>,
    reason: &str,
) -> ChatResult<()> {
    if revoke_active_run_authorizations(pool, selector, reason).await? {
        drain_access_revocation_jobs(app, pool).await?;
    }
    Ok(())
}

/// Stops orphaned runs and drains ready revocation jobs, scheduling a retry when anything is
/// left or any part failed. Returns the jobs completed, or the first failure of the pass.
pub(crate) async fn drain_access_revocation_jobs(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
) -> ChatResult<u64> {
    let pass = run_drain_pass(app, pool).await;
    if pass.retry {
        let retry_app = app.clone();
        let retry_pool = pool.clone();
        tauri::async_runtime::spawn(async move {
            retry_pending_jobs(retry_app, retry_pool).await;
        });
    }
    pass.completed
}

/// What one pass over every part of a revocation drain found.
#[derive(Debug)]
struct DrainPass {
    /// Jobs completed, or the first failure of the pass.
    completed: ChatResult<u64>,
    /// Whether a later pass is needed: an orphaned run is left, a part failed, or jobs remain.
    retry: bool,
}

/// Runs every part of a drain, so a failure of one part never skips the others.
async fn run_drain_pass(app: &tauri::AppHandle, pool: &SqlitePool) -> DrainPass {
    let orphans_left = stop_orphaned_runs_logged(&AppOrphanedRunHost(app), pool).await;
    let recovered = recover_stale_claims(pool).await;
    let drained = drain_ready_jobs(app, pool).await;
    let pending = has_pending_jobs(pool).await;
    drain_pass(orphans_left, recovered, drained, pending)
}

/// Combines the results of the parts of one drain pass. Any failure, an orphaned run left, or a
/// pending job schedules a retry; the pass reports the first failure, else the jobs completed.
fn drain_pass(
    orphans_left: bool,
    recovered: ChatResult<()>,
    drained: ChatResult<u64>,
    pending: ChatResult<bool>,
) -> DrainPass {
    let retry =
        orphans_left || recovered.is_err() || drained.is_err() || !matches!(pending, Ok(false));
    let completed = recovered
        .and(drained)
        .and_then(|count| pending.map(|_| count));
    DrainPass { completed, retry }
}

/// Runtime services the orphaned run sweep acts on.
pub(crate) trait OrphanedRunHost {
    /// Every active organizational run scope, with the thread it serves.
    async fn active_run_scopes(&self) -> Vec<(ChatThreadId, InternalMcpRunScope)>;
    /// Revokes `scope` on `thread_id` if it is still the active one.
    async fn revoke_run_scope(&self, thread_id: &ChatThreadId, scope: &InternalMcpRunScope);
    /// Whether a provider runtime still owns `thread_id`.
    fn has_runtime_owner(&self, thread_id: &ChatThreadId) -> ChatResult<bool>;
    /// Shuts down the provider runtime of `thread_id`.
    async fn shutdown_thread(&self, thread_id: &ChatThreadId) -> ChatResult<()>;
    /// Clears terminal access granted in `folder`.
    fn clear_folder_terminal_access(&self, folder: &ProjectWorkingFolderId) -> ChatResult<()>;
}

/// [`OrphanedRunHost`] over the running app's registries.
struct AppOrphanedRunHost<'a>(&'a tauri::AppHandle);

impl OrphanedRunHost for AppOrphanedRunHost<'_> {
    async fn active_run_scopes(&self) -> Vec<(ChatThreadId, InternalMcpRunScope)> {
        self.0
            .state::<super::internal_mcp::InternalMcpRegistry>()
            .active_run_scopes()
            .await
    }

    async fn revoke_run_scope(&self, thread_id: &ChatThreadId, scope: &InternalMcpRunScope) {
        self.0
            .state::<super::internal_mcp::InternalMcpRegistry>()
            .revoke_matching_run_scope(
                thread_id,
                &scope.run_id,
                &scope.provider_turn_id,
                &scope.authorization_revision_id,
            )
            .await;
    }

    fn has_runtime_owner(&self, thread_id: &ChatThreadId) -> ChatResult<bool> {
        Ok(self
            .0
            .state::<ChatRuntimeRegistry>()
            .owners()?
            .into_iter()
            .any(|owner| owner.thread_id() == thread_id))
    }

    async fn shutdown_thread(&self, thread_id: &ChatThreadId) -> ChatResult<()> {
        let runtime = self.0.state::<ChatRuntimeRegistry>();
        let mutations = self
            .0
            .state::<super::workspace::mutation::ChatWorkspaceMutationRegistry>();
        runtime
            .shutdown_thread_and_remove(thread_id, REVOCATION_STOP_TIMEOUT, &mutations)
            .await
    }

    fn clear_folder_terminal_access(&self, folder: &ProjectWorkingFolderId) -> ChatResult<()> {
        self.0
            .state::<super::terminal_access::ChatTerminalAccess>()
            .clear_folder(folder)
    }
}

/// [`stop_orphaned_run_scopes`] with its failures logged, so one failure never skips the rest
/// of a drain. Returns whether an orphaned run is left for the next drain to retry.
async fn stop_orphaned_runs_logged(host: &impl OrphanedRunHost, pool: &SqlitePool) -> bool {
    match stop_orphaned_run_scopes(host, pool).await {
        Ok(left) => left > 0,
        Err(error) => {
            eprintln!(
                "Chat revocation drain could not check orphaned runs: code {:?}",
                error.code
            );
            true
        }
    }
}

/// Stops organizational runs whose authorization no longer exists, and returns how many are
/// left for the next drain.
///
/// Deleting a conversation, locally or by sync, cascades away the authorizations that target it
/// together with their runs, so no revocation job can name those runs while the provider
/// process and its host-tool scope keep running. The scope remembers its authorization, so the
/// thread of each active scope whose authorization row is gone is shut down. The scope is
/// revoked only once the shutdown succeeded (or no runtime owns the thread), so a failed
/// shutdown leaves the scope for the next drain to find and retry; the host tools of that scope
/// already refuse every call, because they check the missing authorization.
pub(crate) async fn stop_orphaned_run_scopes(
    host: &impl OrphanedRunHost,
    pool: &SqlitePool,
) -> ChatResult<usize> {
    let scopes = host.active_run_scopes().await;
    if scopes.is_empty() {
        return Ok(0);
    }
    let authorization_ids: Vec<&str> = scopes
        .iter()
        .map(|(_, scope)| scope.authorization_revision_id.as_str())
        .collect();
    let missing = missing_authorizations(pool, &authorization_ids).await?;
    let mut left = 0;
    for (thread_id, scope) in &scopes {
        if !missing.contains(scope.authorization_revision_id.as_str()) {
            continue;
        }
        let stopped = match host.has_runtime_owner(thread_id) {
            Ok(false) => Ok(()),
            Ok(true) => host.shutdown_thread(thread_id).await,
            Err(error) => Err(error),
        };
        if let Err(error) = stopped {
            eprintln!(
                "Chat revocation drain could not stop an orphaned run: code {:?}",
                error.code
            );
            left += 1;
            continue;
        }
        host.revoke_run_scope(thread_id, scope).await;
        if let Err(error) = clear_thread_terminal_access(host, pool, thread_id).await {
            eprintln!(
                "Chat revocation drain could not clear terminal access of an orphaned run: code \
                 {:?}",
                error.code
            );
        }
    }
    Ok(left)
}

/// Clears the terminal access granted in the working folder of `thread_id`, if it has one.
async fn clear_thread_terminal_access(
    host: &impl OrphanedRunHost,
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
) -> ChatResult<()> {
    let folder: Option<String> =
        sqlx::query_scalar("SELECT working_folder_id FROM chat_threads WHERE id = ?")
            .bind(thread_id.as_str())
            .fetch_optional(pool)
            .await
            .map_err(persistence_error)?;
    if let Some(folder) = folder {
        let folder = ProjectWorkingFolderId::new(folder).map_err(|_| revocation_error())?;
        host.clear_folder_terminal_access(&folder)?;
    }
    Ok(())
}

/// The ids among `authorization_ids` that name no authorization row.
async fn missing_authorizations(
    pool: &SqlitePool,
    authorization_ids: &[&str],
) -> ChatResult<BTreeSet<String>> {
    let mut missing = BTreeSet::new();
    for id in authorization_ids.iter().copied() {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM chat_assignment_authorization_revisions WHERE id = ?)",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
        if !exists {
            missing.insert(id.to_string());
        }
    }
    Ok(missing)
}

/// Drains queued access revocation jobs in the background, at startup and whenever sync
/// revoked authorizations or deleted conversations on this device. Each drain first stops runs
/// whose authorization a conversation delete removed.
pub(crate) fn start_job_drain(app: &tauri::AppHandle) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
        let pool = match crate::db::connect_sqlite(app.clone(), db_url).await {
            Ok(pool) => pool,
            Err(error) => {
                eprintln!("Chat revocation drain could not open the database: {error}");
                return;
            }
        };
        if let Err(error) = drain_access_revocation_jobs(&app, &pool).await {
            eprintln!("Chat revocation drain failed with code {:?}", error.code);
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

/// Polls until a full drain pass leaves no job, no orphaned run, and no failed part, retrying
/// orphaned runs whose shutdown failed, failed parts, and pending jobs.
async fn retry_pending_jobs(app: tauri::AppHandle, pool: SqlitePool) {
    for _ in 0..REVOCATION_RETRY_MAX_POLLS {
        tokio::time::sleep(REVOCATION_RETRY_POLL).await;
        let pass = run_drain_pass(&app, &pool).await;
        if let Err(error) = &pass.completed {
            eprintln!("Chat revocation retry failed with code {:?}", error.code);
        }
        if !pass.retry {
            return;
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
    clear_revoked_terminal_grants(app, pool, authorization_revision_id.as_deref()).await?;
    mark_claim_completed(pool, job_id, claim_token).await
}

/// Drops terminal start grants for the folders a revoked authorization ran in,
/// so the next terminal there asks again.
async fn clear_revoked_terminal_grants(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    authorization_revision_id: Option<&str>,
) -> ChatResult<()> {
    let Some(authorization_revision_id) = authorization_revision_id else {
        return Ok(());
    };
    let folders: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT thread.working_folder_id
         FROM chat_agent_runs run
         JOIN chat_threads thread ON thread.id = run.provider_thread_id
         WHERE run.authorization_revision_id = ?",
    )
    .bind(authorization_revision_id)
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let access = app.state::<super::terminal_access::ChatTerminalAccess>();
    for folder in folders {
        let folder = ProjectWorkingFolderId::new(folder).map_err(|_| revocation_error())?;
        access.clear_folder(&folder)?;
    }
    Ok(())
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
pub(crate) mod tests {
    use super::{
        ActiveRunSelector, InternalMcpRunScope, OrphanedRunHost, REVOCATION_DRAIN_BATCH_SIZE,
        REVOCATION_RETRY_MAX_POLLS, REVOCATION_RETRY_MAX_SECONDS, drain_pass,
        missing_authorizations, retry_delay_seconds, revocation_error,
        revoke_active_run_authorizations, stop_orphaned_run_scopes, stop_orphaned_runs_logged,
    };
    use crate::chat::sync::RevocationWake;
    use ganbaru_chat_contracts::models::{
        ChatAgentRunId, ChatAuthorizationRevisionId, ChatConversationId, ChatError, ChatErrorCode,
        ChatResult, ChatRuntimeApprovalPolicy, ChatThreadId, ChatTurnId, ChatWorkAssignmentId,
        ProjectWorkingFolderId,
    };
    use sqlx::SqlitePool;
    use std::collections::BTreeSet;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const NOW: &str = "2026-10-20T12:00:00.000Z";
    const TEAMMATE: &str = "participant:folder-agent";

    /// Seeds one working run in `workspace-1` that also holds `workspace-2` as a read source,
    /// and a queued follow-up run for the same assignment.
    pub(crate) async fn pool_with_folder_runs() -> SqlitePool {
        let pool = crate::chat::tests::repository::pool_with_thread().await;
        let (conversation, _) =
            ganbaru_chat::project_defaults::default_channel_ids("project-chat").unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO project_working_folders
                (id, project_id, display_name, kind, managed_relative_path,
                 repository_kind, sort_order, created_at, updated_at)
             VALUES
                ('workspace-2', 'project-chat', 'Reference', 'external', NULL,
                 'none', 1, '{NOW}', '{NOW}'),
                ('workspace-3', 'project-chat', 'Unrelated', 'external', NULL,
                 'none', 2, '{NOW}', '{NOW}');
             INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('{TEAMMATE}', 'ai_teammate', 'Folder agent', '{NOW}', '{NOW}');
             INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
             VALUES ('{TEAMMATE}', 'Test', '{NOW}', '{NOW}');
             INSERT INTO chat_teammate_policy_revisions
                (id, teammate_id, provider_instance_id, model_selection_data, created_at)
             VALUES ('policy:folders', '{TEAMMATE}', 'codex-personal', '{{}}', '{NOW}');
             INSERT INTO chat_conversation_memberships
                (conversation_id, participant_id, sync_key, membership_role, created_at,
                 updated_at)
             VALUES ('{conversation}', '{TEAMMATE}', 'membership:folders', 'member',
                     '{NOW}', '{NOW}');
             INSERT INTO chat_teammate_channel_memberships
                (conversation_id, teammate_id, sync_key, access_profile_id,
                 read_history, participate, history_boundary, created_at, updated_at)
             VALUES ('{conversation}', '{TEAMMATE}', 'channel-membership:folders',
                     'access-profile:build-and-test', 1, 1, 'entire', '{NOW}', '{NOW}');
             INSERT INTO chat_teammate_working_folder_grants
                (conversation_id, teammate_id, project_id, working_folder_id, sync_key,
                 capability, capability_inherits_profile, is_default, created_at)
             VALUES
                ('{conversation}', '{TEAMMATE}', 'project-chat', 'workspace-1',
                 'grant:folders-1', 'execute', 1, 1, '{NOW}'),
                ('{conversation}', '{TEAMMATE}', 'project-chat', 'workspace-2',
                 'grant:folders-2', 'read', 0, 0, '{NOW}');
             INSERT INTO chat_conversation_items
                (id, conversation_id, item_kind, order_key, created_at)
             VALUES ('item:folders', '{conversation}', 'message', 'a1', '{NOW}');
             INSERT INTO chat_communication_messages
                (item_id, author_participant_id, created_at)
             VALUES ('item:folders', 'participant:local-owner', '{NOW}');
             INSERT INTO chat_reply_threads
                (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
             VALUES ('reply:folders', '{conversation}', 'item:folders',
                     '{NOW}', '{NOW}', '{NOW}');
             INSERT INTO chat_work_assignments
                (id, reply_thread_id, teammate_id, triggering_message_item_id, state,
                 created_at, updated_at)
             VALUES ('assignment:folders', 'reply:folders', '{TEAMMATE}', 'item:folders',
                     'working', '{NOW}', '{NOW}');
             INSERT INTO chat_assignment_authorization_revisions
                (id, assignment_id, revision, requester_participant_id,
                 teammate_policy_revision_id, teammate_access_revision,
                 destination_conversation_id, access_profile_revision_id,
                 execution_environment_id, working_folder_id,
                 resolved_runtime_approval_policy, scope_digest, decision_state, created_at)
             VALUES
                ('authorization:working', 'assignment:folders', 1, 'participant:local-owner',
                 'policy:folders', 1, '{conversation}',
                 'access-profile-revision:build-and-test:1', (SELECT id FROM chat_execution_environments WHERE working_folder_id = 'workspace-1'),
                 'workspace-1', 'ask', printf('%064d', 0), 'allowed', '{NOW}'),
                ('authorization:queued', 'assignment:folders', 2, 'participant:local-owner',
                 'policy:folders', 1, '{conversation}',
                 'access-profile-revision:build-and-test:1', (SELECT id FROM chat_execution_environments WHERE working_folder_id = 'workspace-1'),
                 'workspace-1', 'ask', printf('%064d', 0), 'allowed', '{NOW}');
             INSERT INTO chat_assignment_authorized_folder_sources
                (authorization_revision_id, root_handle, working_folder_id, capability,
                 is_execution_target, created_at, resolved_runtime_approval_policy)
             VALUES
                ('authorization:working', printf('%032d', 1), 'workspace-1', 'execute', 1, '{NOW}', 'ask'),
                ('authorization:working', printf('%032d', 2), 'workspace-2', 'read', 0, '{NOW}', 'ask');
             INSERT INTO chat_turns
                (id, thread_id, ordinal, state, safety_mode, interaction_mode,
                 created_at, updated_at)
             VALUES
                ('turn:working', 'thread-1', 1, 'active', 'ask_for_approval', 'build',
                 '{NOW}', '{NOW}'),
                ('turn:queued', 'thread-1', 2, 'pending', 'ask_for_approval', 'build',
                 '{NOW}', '{NOW}');
             INSERT INTO chat_agent_runs
                (id, assignment_id, project_id, working_folder_id, execution_environment_id,
                 teammate_policy_revision_id, authorization_revision_id,
                 authorization_scope_digest, provider_turn_id, provider_thread_id, state,
                 run_ordinal, created_at, updated_at)
             VALUES
                ('run:working', 'assignment:folders', 'project-chat', 'workspace-1',
                 (SELECT id FROM chat_execution_environments WHERE working_folder_id = 'workspace-1'), 'policy:folders', 'authorization:working',
                 printf('%064d', 0), 'turn:working', 'thread-1', 'working', 1, '{NOW}', '{NOW}'),
                ('run:queued', 'assignment:folders', 'project-chat', 'workspace-1',
                 (SELECT id FROM chat_execution_environments WHERE working_folder_id = 'workspace-1'), 'policy:folders', 'authorization:queued',
                 printf('%064d', 0), 'turn:queued', 'thread-1', 'queued', 2, '{NOW}', '{NOW}');"
        )))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn authorization_states(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT id, decision_state FROM chat_assignment_authorization_revisions ORDER BY id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn queued_job_authorizations(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT authorization_revision_id FROM chat_access_revocation_jobs
             WHERE state = 'queued' ORDER BY authorization_revision_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn states(working: &str, queued: &str) -> Vec<(String, String)> {
        vec![
            ("authorization:queued".to_string(), queued.to_string()),
            ("authorization:working".to_string(), working.to_string()),
        ]
    }

    #[tokio::test]
    async fn losing_a_folder_revokes_active_runs_that_execute_in_or_read_it() {
        for folder in ["workspace-1", "workspace-2"] {
            let pool = pool_with_folder_runs().await;
            let folder = ProjectWorkingFolderId::new(folder).unwrap();
            let revoked = revoke_active_run_authorizations(
                &pool,
                ActiveRunSelector::WorkingFolder(&folder),
                "The working folder was unbound",
            )
            .await
            .unwrap();

            assert!(revoked);
            assert_eq!(
                authorization_states(&pool).await,
                states("revoked", "allowed")
            );
            assert_eq!(
                queued_job_authorizations(&pool).await,
                vec!["authorization:working".to_string()]
            );
            let repeated = revoke_active_run_authorizations(
                &pool,
                ActiveRunSelector::WorkingFolder(&folder),
                "The working folder was unbound",
            )
            .await
            .unwrap();
            assert!(!repeated);
            assert_eq!(queued_job_authorizations(&pool).await.len(), 1);
        }
    }

    #[tokio::test]
    async fn losing_an_unrelated_folder_keeps_runs_authorized() {
        let pool = pool_with_folder_runs().await;
        let folder = ProjectWorkingFolderId::new("workspace-3").unwrap();
        let revoked = revoke_active_run_authorizations(
            &pool,
            ActiveRunSelector::WorkingFolder(&folder),
            "The working folder was archived",
        )
        .await
        .unwrap();

        assert!(!revoked);
        assert_eq!(
            authorization_states(&pool).await,
            states("allowed", "allowed")
        );
        assert!(queued_job_authorizations(&pool).await.is_empty());
    }

    #[tokio::test]
    async fn a_failed_live_check_revokes_only_the_active_run_for_its_turn() {
        let pool = pool_with_folder_runs().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let queued_turn = ChatTurnId::new("turn:queued").unwrap();
        let revoked = revoke_active_run_authorizations(
            &pool,
            ActiveRunSelector::ProviderTurn {
                thread_id: &thread_id,
                turn_id: Some(&queued_turn),
            },
            "Organizational run authority was lost",
        )
        .await
        .unwrap();
        assert!(
            !revoked,
            "queued runs pass the live checks again before they start"
        );

        let working_turn = ChatTurnId::new("turn:working").unwrap();
        let revoked = revoke_active_run_authorizations(
            &pool,
            ActiveRunSelector::ProviderTurn {
                thread_id: &thread_id,
                turn_id: Some(&working_turn),
            },
            "Organizational run authority was lost",
        )
        .await
        .unwrap();
        assert!(revoked);
        assert_eq!(
            authorization_states(&pool).await,
            states("revoked", "allowed")
        );
        assert_eq!(
            queued_job_authorizations(&pool).await,
            vec!["authorization:working".to_string()]
        );
    }

    /// Deletes the seeded runs' conversation, with sync applying when `applying` holds.
    async fn delete_run_conversation(pool: &SqlitePool, applying: bool) -> String {
        let (conversation, _) =
            ganbaru_chat::project_defaults::default_channel_ids("project-chat").unwrap();
        sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
            .bind(applying)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_conversations WHERE id = ?")
            .bind(&conversation)
            .execute(pool)
            .await
            .unwrap();
        conversation
    }

    /// Runtime registries as the orphaned run sweep sees them: active scopes, the threads a
    /// provider runtime owns, and a number of shutdowns that fail before one succeeds.
    #[derive(Default)]
    struct FakeRunHost {
        scopes: Mutex<Vec<(ChatThreadId, InternalMcpRunScope)>>,
        owned_threads: Mutex<BTreeSet<String>>,
        failing_shutdowns: AtomicUsize,
        shutdowns: Mutex<Vec<String>>,
    }

    impl FakeRunHost {
        /// A host whose runtime owns `thread-1`, running under `authorization:working`.
        fn with_working_run() -> Self {
            let host = Self::default();
            host.scopes.lock().unwrap().push((
                ChatThreadId::new("thread-1").unwrap(),
                InternalMcpRunScope {
                    run_id: ChatAgentRunId::new("run:working").unwrap(),
                    provider_turn_id: ChatTurnId::new("turn:working").unwrap(),
                    assignment_id: ChatWorkAssignmentId::new("assignment:folders").unwrap(),
                    authorization_revision_id: ChatAuthorizationRevisionId::new(
                        "authorization:working",
                    )
                    .unwrap(),
                    destination_conversation_id: ChatConversationId::new(
                        ganbaru_chat::project_defaults::default_channel_ids("project-chat")
                            .unwrap()
                            .0,
                    )
                    .unwrap(),
                    authorization_scope_digest: "0".repeat(64),
                    execution_environment_id: None,
                    scratch_generation_id: None,
                    runtime_approval_policy: ChatRuntimeApprovalPolicy::Ask,
                    channel_sources: Vec::new(),
                    folder_sources: Vec::new(),
                },
            ));
            host.owned_threads
                .lock()
                .unwrap()
                .insert("thread-1".to_string());
            host
        }

        fn scope_count(&self) -> usize {
            self.scopes.lock().unwrap().len()
        }

        fn shutdowns(&self) -> Vec<String> {
            self.shutdowns.lock().unwrap().clone()
        }
    }

    impl OrphanedRunHost for FakeRunHost {
        async fn active_run_scopes(&self) -> Vec<(ChatThreadId, InternalMcpRunScope)> {
            self.scopes.lock().unwrap().clone()
        }

        async fn revoke_run_scope(&self, thread_id: &ChatThreadId, scope: &InternalMcpRunScope) {
            self.scopes
                .lock()
                .unwrap()
                .retain(|(thread, active)| !(thread == thread_id && active == scope));
        }

        fn has_runtime_owner(&self, thread_id: &ChatThreadId) -> ChatResult<bool> {
            Ok(self
                .owned_threads
                .lock()
                .unwrap()
                .contains(thread_id.as_str()))
        }

        async fn shutdown_thread(&self, thread_id: &ChatThreadId) -> ChatResult<()> {
            let failing = self.failing_shutdowns.load(Ordering::SeqCst);
            if failing > 0 {
                self.failing_shutdowns.store(failing - 1, Ordering::SeqCst);
                return Err(revocation_error());
            }
            self.owned_threads
                .lock()
                .unwrap()
                .remove(thread_id.as_str());
            self.shutdowns
                .lock()
                .unwrap()
                .push(thread_id.as_str().to_string());
            Ok(())
        }

        fn clear_folder_terminal_access(&self, _folder: &ProjectWorkingFolderId) -> ChatResult<()> {
            Ok(())
        }
    }

    /// Records whether the local delete path woke the revocation drain.
    #[derive(Default)]
    struct RecordingWake(AtomicBool);

    impl RevocationWake for RecordingWake {
        fn wake_revocation_drain(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    /// Deleting a group cascades away the conversation, its authorizations, and its runs, so no
    /// row names the run any more: the command wakes the drain, whose sweep finds the run
    /// through the active scope's remembered authorization and stops it.
    #[tokio::test]
    async fn a_local_group_delete_wakes_the_drain_and_the_sweep_stops_the_orphaned_run() {
        let pool = pool_with_folder_runs().await;
        let host = FakeRunHost::with_working_run();
        assert_eq!(stop_orphaned_run_scopes(&host, &pool).await.unwrap(), 0);
        assert_eq!(host.scope_count(), 1);
        assert!(host.shutdowns().is_empty());

        let wake = RecordingWake::default();
        crate::projects::structure_commands::delete_group_and_wake(
            &pool,
            "group-chat".to_string(),
            &wake,
        )
        .await
        .unwrap();

        assert!(wake.0.load(Ordering::SeqCst));
        assert!(authorization_states(&pool).await.is_empty());
        assert_eq!(stop_orphaned_run_scopes(&host, &pool).await.unwrap(), 0);
        assert_eq!(host.scope_count(), 0);
        assert_eq!(host.shutdowns(), vec!["thread-1".to_string()]);
    }

    /// A failed shutdown keeps the scope, so the next drain finds the orphaned run again and
    /// retries until the provider stops.
    #[tokio::test]
    async fn a_failed_orphan_shutdown_is_retried_by_the_next_drain() {
        let pool = pool_with_folder_runs().await;
        let host = FakeRunHost::with_working_run();
        host.failing_shutdowns.store(1, Ordering::SeqCst);
        delete_run_conversation(&pool, false).await;

        assert!(stop_orphaned_runs_logged(&host, &pool).await);
        assert_eq!(host.scope_count(), 1);
        assert!(host.shutdowns().is_empty());

        assert!(!stop_orphaned_runs_logged(&host, &pool).await);
        assert_eq!(host.scope_count(), 0);
        assert_eq!(host.shutdowns(), vec!["thread-1".to_string()]);
        assert_eq!(stop_orphaned_run_scopes(&host, &pool).await.unwrap(), 0);
        assert_eq!(host.shutdowns().len(), 1);
    }

    #[tokio::test]
    async fn an_applied_conversation_delete_reports_the_conversation_and_its_missing_runs() {
        let pool = pool_with_folder_runs().await;
        let host = FakeRunHost::with_working_run();
        let conversation = delete_run_conversation(&pool, true).await;
        let mut connection = pool.acquire().await.unwrap();
        let effects = ganbaru_chat::sync_effects::after_materialize(
            &mut connection,
            ganbaru_chat::sync_effects::CONVERSATIONS_TABLE,
            std::slice::from_ref(&conversation),
        )
        .await
        .unwrap();
        drop(connection);

        assert_eq!(
            effects.removed_conversations,
            BTreeSet::from([conversation])
        );
        assert_eq!(
            missing_authorizations(&pool, &["authorization:working", "authorization:queued"])
                .await
                .unwrap(),
            BTreeSet::from([
                "authorization:queued".to_string(),
                "authorization:working".to_string()
            ])
        );
        assert!(effects.revoked_teammates.is_empty());
        assert_eq!(stop_orphaned_run_scopes(&host, &pool).await.unwrap(), 0);
        assert_eq!(host.shutdowns(), vec!["thread-1".to_string()]);
    }

    #[test]
    fn revocation_recovery_batches_and_caps_retry_backoff() {
        assert_eq!(REVOCATION_DRAIN_BATCH_SIZE, 32);
        assert_eq!(REVOCATION_RETRY_MAX_POLLS, 60);
        assert_eq!(retry_delay_seconds(0), 5);
        assert_eq!(retry_delay_seconds(1), 10);
        assert_eq!(retry_delay_seconds(100), REVOCATION_RETRY_MAX_SECONDS);
    }

    /// A failure of the drain part named `part`.
    fn part_failure(part: &str) -> ChatError {
        ChatError::new(ChatErrorCode::Persistence, part, true)
    }

    #[test]
    fn a_drain_pass_retries_when_any_part_failed_or_left_work() {
        let clean = drain_pass(false, Ok(()), Ok(3), Ok(false));
        assert!(!clean.retry);
        assert_eq!(clean.completed.unwrap(), 3);

        // Each reason alone schedules the retry.
        assert!(drain_pass(true, Ok(()), Ok(0), Ok(false)).retry);
        assert!(drain_pass(false, Ok(()), Ok(0), Ok(true)).retry);
        assert!(drain_pass(false, Err(part_failure("recover")), Ok(0), Ok(false)).retry);
        assert!(drain_pass(false, Ok(()), Err(part_failure("drain")), Ok(false)).retry);
        assert!(drain_pass(false, Ok(()), Ok(0), Err(part_failure("pending"))).retry);

        // An orphan shutdown that failed together with a failed recovery still retries, and the
        // pass reports the first failure.
        let failed = drain_pass(
            true,
            Err(part_failure("recover")),
            Err(part_failure("drain")),
            Err(part_failure("pending")),
        );
        assert!(failed.retry);
        assert_eq!(failed.completed.unwrap_err().message, "recover");
        let drain_failed = drain_pass(
            false,
            Ok(()),
            Err(part_failure("drain")),
            Err(part_failure("pending")),
        );
        assert_eq!(drain_failed.completed.unwrap_err().message, "drain");
        let pending_failed = drain_pass(false, Ok(()), Ok(2), Err(part_failure("pending")));
        assert_eq!(pending_failed.completed.unwrap_err().message, "pending");
    }
}

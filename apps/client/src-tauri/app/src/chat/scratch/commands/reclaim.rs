//! Read-only report of scratch generations that the confirmed cleanup can reclaim.

use super::inspection::MAX_SIZE_BYTES;
use super::{parse_generation_lifecycle, parse_id, parse_scope_lifecycle, require_local_owner};
use crate::chat::channels::{chat_pool, persistence_error, timestamp, u64_value};
use crate::chat::scratch;
use chrono::{DateTime, Duration, Utc};
use ganbaru_chat_contracts::models::*;
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

/// Most rows one reclaim report returns; the rest are counted in `omitted`.
pub const MAX_RECLAIM_REPORT_ROWS: usize = 50;
/// Days without a run or generation change after which scratch counts as idle.
pub const SCRATCH_RECLAIM_IDLE_DAYS: i64 = 30;
const MAX_RECLAIM_SIZE_ENTRIES: u64 = 10_000;

/// Reports the largest scratch generations that are safe to clean, without deleting anything.
///
/// Sizes are measured inside the managed scratch storage of this device with the same bounds
/// as the scratch manager, because the stored size is not maintained. Project working folders
/// are never read.
#[tauri::command]
pub async fn chat_read_scratch_reclaim_report(
    app: tauri::AppHandle,
    db_url: String,
) -> ChatResult<ChatScratchReclaimReportRead> {
    let pool = chat_pool(app.clone(), db_url).await?;
    require_local_owner(&pool).await?;
    let candidates = read_reclaim_candidates(&pool).await?;
    let qualifying = qualify_candidates(candidates, Utc::now())?;
    let mut rows = Vec::with_capacity(qualifying.len());
    for (candidate, reason) in qualifying {
        let (device_availability, size) = measure_generation(&app, &pool, &candidate).await;
        rows.push(candidate.into_row(reason, device_availability, size));
    }
    Ok(build_reclaim_report(rows, MAX_RECLAIM_REPORT_ROWS))
}

/// One live scratch generation with the facts that decide whether it is reclaimable.
#[derive(Clone, Debug)]
struct ReclaimCandidate {
    scope_id: ChatScratchScopeId,
    generation_id: ChatScratchGenerationId,
    execution_environment_id: ChatExecutionEnvironmentId,
    reply_thread_id: ChatReplyThreadId,
    generation: u64,
    teammate_name: String,
    channel_name: String,
    project_name: String,
    group_name: Option<String>,
    scope_lifecycle: ChatScratchScopeLifecycleState,
    generation_lifecycle: ChatScratchGenerationLifecycleState,
    active_run_count: u64,
    stored_bytes: u64,
    last_activity_at: UtcTimestamp,
}

impl ReclaimCandidate {
    fn into_row(
        self,
        reason: ChatScratchReclaimReason,
        device_availability: ChatScratchDeviceAvailability,
        size: scratch::BoundedScratchSize,
    ) -> ChatScratchReclaimRow {
        ChatScratchReclaimRow {
            scratch_scope_id: self.scope_id,
            scratch_generation_id: self.generation_id,
            reply_thread_id: self.reply_thread_id,
            generation: self.generation,
            teammate_name: self.teammate_name,
            channel_name: self.channel_name,
            project_name: self.project_name,
            group_name: self.group_name,
            reason,
            byte_size: size.bytes,
            size_truncated: size.truncated,
            device_availability,
            last_activity_at: self.last_activity_at,
        }
    }
}

/// Reads every live generation whose scope and generation states the cleanup accepts.
async fn read_reclaim_candidates(pool: &SqlitePool) -> ChatResult<Vec<ReclaimCandidate>> {
    let rows = sqlx::query(
        "SELECT scope.id AS scope_id, scope.reply_thread_id,
                scope.lifecycle_state AS scope_lifecycle,
                teammate.display_name AS teammate_name,
                channel.name AS channel_name, project.name AS project_name,
                project_group.name AS group_name,
                generation.id AS generation_id, generation.generation,
                generation.lifecycle_state AS generation_lifecycle,
                generation.byte_size, generation.updated_at AS generation_updated_at,
                environment.id AS execution_environment_id,
                (SELECT count(*) FROM chat_agent_runs run
                 WHERE run.scratch_generation_id = generation.id
                   AND run.state IN ('queued', 'starting', 'working', 'waiting'))
                  AS active_run_count,
                (SELECT max(run.updated_at) FROM chat_agent_runs run
                 WHERE run.scratch_generation_id = generation.id) AS last_run_at
         FROM chat_scratch_generations generation
         JOIN chat_scratch_scopes scope ON scope.id = generation.scratch_scope_id
         JOIN chat_participants teammate ON teammate.id = scope.teammate_id
         JOIN chat_reply_threads thread ON thread.id = scope.reply_thread_id
         JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
         JOIN projects project ON project.id = channel.project_id
         LEFT JOIN project_groups project_group ON project_group.id = project.group_id
         JOIN chat_execution_environments environment
           ON environment.scratch_generation_id = generation.id
          AND environment.kind = 'scratch'
         WHERE generation.removed_at IS NULL AND scope.removed_at IS NULL
           AND scope.lifecycle_state IN ('active', 'archived', 'cleanup_failed')
           AND generation.lifecycle_state IN ('active', 'quarantined', 'cleanup_failed')
         ORDER BY generation.id, channel.created_at, channel.id",
    )
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    let mut seen = HashSet::new();
    let mut candidates = Vec::with_capacity(rows.len());
    for row in rows {
        let generation_id: ChatScratchGenerationId =
            parse_id(row.try_get("generation_id").map_err(persistence_error)?)?;
        if !seen.insert(generation_id.as_str().to_string()) {
            continue;
        }
        let generation_updated_at: String = row
            .try_get("generation_updated_at")
            .map_err(persistence_error)?;
        let last_run_at: Option<String> = row.try_get("last_run_at").map_err(persistence_error)?;
        candidates.push(ReclaimCandidate {
            scope_id: parse_id(row.try_get("scope_id").map_err(persistence_error)?)?,
            generation_id,
            execution_environment_id: parse_id(
                row.try_get("execution_environment_id")
                    .map_err(persistence_error)?,
            )?,
            reply_thread_id: parse_id(row.try_get("reply_thread_id").map_err(persistence_error)?)?,
            generation: u64_value(row.try_get("generation").map_err(persistence_error)?)?,
            teammate_name: row.try_get("teammate_name").map_err(persistence_error)?,
            channel_name: row.try_get("channel_name").map_err(persistence_error)?,
            project_name: row.try_get("project_name").map_err(persistence_error)?,
            group_name: row.try_get("group_name").map_err(persistence_error)?,
            scope_lifecycle: parse_scope_lifecycle(
                &row.try_get::<String, _>("scope_lifecycle")
                    .map_err(persistence_error)?,
            )?,
            generation_lifecycle: parse_generation_lifecycle(
                &row.try_get::<String, _>("generation_lifecycle")
                    .map_err(persistence_error)?,
            )?,
            active_run_count: u64_value(
                row.try_get("active_run_count").map_err(persistence_error)?,
            )?,
            stored_bytes: u64_value(row.try_get("byte_size").map_err(persistence_error)?)?,
            last_activity_at: latest_activity(generation_updated_at, last_run_at)?,
        });
    }
    Ok(candidates)
}

/// Returns the later of the generation change and the newest run update.
fn latest_activity(
    generation_updated_at: String,
    last_run_at: Option<String>,
) -> ChatResult<UtcTimestamp> {
    let generation = parse_instant(&generation_updated_at)?;
    match last_run_at {
        Some(run) if parse_instant(&run)? > generation => timestamp(run),
        _ => timestamp(generation_updated_at),
    }
}

fn parse_instant(value: &str) -> ChatResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| {
            ChatError::new(
                ChatErrorCode::Persistence,
                "Stored Chat timestamp is invalid",
                false,
            )
        })
}

/// Keeps the candidates that are reclaimable at `now`, each with its reason.
fn qualify_candidates(
    candidates: Vec<ReclaimCandidate>,
    now: DateTime<Utc>,
) -> ChatResult<Vec<(ReclaimCandidate, ChatScratchReclaimReason)>> {
    let idle_cutoff = now - Duration::days(SCRATCH_RECLAIM_IDLE_DAYS);
    let mut qualifying = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let idle = parse_instant(candidate.last_activity_at.as_str())? < idle_cutoff;
        if let Some(reason) = reclaim_reason(&candidate, idle) {
            qualifying.push((candidate, reason));
        }
    }
    Ok(qualifying)
}

/// Decides whether a candidate is reclaimable and names the highest priority reason.
///
/// Work that is queued, starting, working, or waiting always excludes the generation.
fn reclaim_reason(candidate: &ReclaimCandidate, idle: bool) -> Option<ChatScratchReclaimReason> {
    if candidate.active_run_count > 0 {
        return None;
    }
    match (candidate.scope_lifecycle, candidate.generation_lifecycle) {
        (ChatScratchScopeLifecycleState::Archived, _) => {
            Some(ChatScratchReclaimReason::ScopeArchived)
        }
        (ChatScratchScopeLifecycleState::CleanupFailed, _) => {
            Some(ChatScratchReclaimReason::ScopeCleanupFailed)
        }
        (_, ChatScratchGenerationLifecycleState::CleanupFailed) => {
            Some(ChatScratchReclaimReason::GenerationCleanupFailed)
        }
        (_, ChatScratchGenerationLifecycleState::Quarantined) => {
            Some(ChatScratchReclaimReason::GenerationQuarantined)
        }
        _ if idle => Some(ChatScratchReclaimReason::Idle),
        _ => None,
    }
}

/// Measures a generation on this device, falling back to its stored size when unavailable.
async fn measure_generation(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    candidate: &ReclaimCandidate,
) -> (ChatScratchDeviceAvailability, scratch::BoundedScratchSize) {
    let stored = || {
        (
            ChatScratchDeviceAvailability::UnavailableOnThisDevice,
            scratch::BoundedScratchSize {
                bytes: candidate.stored_bytes.min(MAX_SIZE_BYTES),
                entries: 0,
                truncated: candidate.stored_bytes > MAX_SIZE_BYTES,
            },
        )
    };
    let Ok(path) = scratch::resolve_managed_scratch_path_for_inspection(
        app,
        pool,
        candidate.generation_id.as_str(),
        candidate.execution_environment_id.as_str(),
    )
    .await
    else {
        return stored();
    };
    match scratch::bounded_scratch_size(&path, MAX_RECLAIM_SIZE_ENTRIES, MAX_SIZE_BYTES) {
        Ok(size) => (ChatScratchDeviceAvailability::Available, size),
        Err(_) => stored(),
    }
}

/// Sorts rows by size, largest first, totals what this device can reclaim, and caps the rows.
///
/// Generations unavailable on this device stay listed, because cleanup still records their
/// removal, but add nothing to the total.
fn build_reclaim_report(
    mut rows: Vec<ChatScratchReclaimRow>,
    limit: usize,
) -> ChatScratchReclaimReportRead {
    rows.sort_by(|left, right| {
        right.byte_size.cmp(&left.byte_size).then_with(|| {
            left.scratch_generation_id
                .as_str()
                .cmp(right.scratch_generation_id.as_str())
        })
    });
    let available = rows
        .iter()
        .filter(|row| row.device_availability == ChatScratchDeviceAvailability::Available);
    let total_bytes = available
        .clone()
        .fold(0_u64, |total, row| total.saturating_add(row.byte_size));
    let total_truncated = available.into_iter().any(|row| row.size_truncated);
    let omitted = rows.len().saturating_sub(limit) as u64;
    rows.truncate(limit);
    ChatScratchReclaimReportRead {
        rows,
        total_bytes,
        total_truncated,
        omitted,
        idle_days: SCRATCH_RECLAIM_IDLE_DAYS.unsigned_abs(),
    }
}

#[cfg(test)]
mod tests;

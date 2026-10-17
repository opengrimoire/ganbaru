//! Native recurrence scope analysis over one bounded Calendar/Focus snapshot.

use ganbaru_db::impl_sqlite_from_row;
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use tokio::sync::OwnedSemaphorePermit;

#[cfg(test)]
use crate::recurrence::canonical::ScopePlan;
use crate::recurrence::canonical::{EditScope, ScopeClock, ScopeEvidence, parse_date};
use crate::work_gate::WorkGate;

use super::occurrence::{Geometry, ReadBudget, read_geometry};

pub static SCOPE_GATE: WorkGate = WorkGate::new();
pub const SCOPE_WORKER_TIMEOUT: Duration = Duration::from_secs(10);

/// Admits a preview's blocking worker after its SQLite snapshot is released.
///
/// Opening an event requests its edit and deletion previews together, so a preview waits for the
/// other instead of failing. Admission follows the snapshot read because the vault has one SQLite
/// connection: a permit held while waiting for it would stall every preview behind an unrelated
/// Save or read, and Saves that need the gate inside their transaction would fail.
pub(super) async fn admit_preview_worker() -> Result<OwnedSemaphorePermit, String> {
    SCOPE_GATE
        .acquire_within(SCOPE_WORKER_TIMEOUT)
        .await
        .ok_or_else(|| "A Calendar preview is being prepared; retry after it finishes".into())
}

/// The device-local date at an instant, which decides floating all-day protection.
///
/// Implementations may block on a platform query, so callers invoke them from a blocking
/// worker held behind [`SCOPE_GATE`].
pub trait DeviceDate: Send + Sync {
    /// Returns the device's local calendar date at `epoch_ms`.
    fn local_date(&self, epoch_ms: i64) -> Result<NaiveDate, String>;
}

/// The selected original identity and requested scope. Clients do not provide
/// history, active run identity, a clock, or precomputed protection boundaries.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScopeRequest {
    pub(super) template_id: String,
    pub(super) recurrence_date: String,
    pub(super) scope: EditScope,
}

#[derive(Serialize)]
struct RunEvidence {
    id: String,
    event_id: Option<String>,
    original_event_id: String,
    current_occurrence_id: Option<String>,
    calendar_archive_id: Option<String>,
    ended_at: Option<String>,
}
impl_sqlite_from_row!(RunEvidence {
    id,
    event_id,
    original_event_id,
    current_occurrence_id,
    calendar_archive_id,
    ended_at
});

#[derive(Serialize)]
struct SegmentEvidence {
    id: String,
    run_id: String,
    event_id: String,
    original_event_id: String,
    current_occurrence_id: Option<String>,
}
impl_sqlite_from_row!(SegmentEvidence {
    id,
    run_id,
    event_id,
    original_event_id,
    current_occurrence_id
});

pub(super) struct ExecutionEvidence {
    runs: Vec<RunEvidence>,
    segments: Vec<SegmentEvidence>,
}

impl ExecutionEvidence {
    /// Include exact records in deletion review, with stable ordering independent
    /// of the database's chosen index or insertion order.
    pub(super) fn revision(&self) -> Result<String, String> {
        let mut runs: Vec<_> = self.runs.iter().collect();
        runs.sort_by(|left, right| left.id.cmp(&right.id));
        let mut segments: Vec<_> = self.segments.iter().collect();
        segments.sort_by(|left, right| left.id.cmp(&right.id));
        super::metadata::revision(&(runs, segments))
    }

    /// Undo retains only nullable projections changed by this deletion. It
    /// never stores execution rows to replay timestamps or completed outcomes.
    pub(super) fn deletion_undo_references(
        &self,
        targets: &[(String, String, String)],
    ) -> Result<super::deletion::UndoReferences, String> {
        let mut runs = Vec::with_capacity(targets.len());
        let by_id: std::collections::BTreeMap<_, _> =
            self.runs.iter().map(|run| (run.id.as_str(), run)).collect();
        for (id, expected, archive) in targets {
            let run = by_id
                .get(id.as_str())
                .ok_or("Calendar Undo lost its captured run")?;
            runs.push(super::deletion::UndoRunReference {
                id: id.clone(),
                event_id: run
                    .event_id
                    .clone()
                    .ok_or("Calendar Undo run has no live reference")?,
                current_identity: expected.clone(),
                previous_archive_id: run.calendar_archive_id.clone(),
                expected_archive_id: archive.clone(),
            });
        }
        let selected: std::collections::BTreeSet<_> =
            targets.iter().map(|(id, _, _)| id.as_str()).collect();
        let segments = self
            .segments
            .iter()
            .filter(|segment| selected.contains(segment.run_id.as_str()))
            .map(|segment| super::deletion::UndoSegmentReference {
                id: segment.id.clone(),
                run_id: segment.run_id.clone(),
                event_id: segment.event_id.clone(),
            })
            .collect();
        Ok(super::deletion::UndoReferences { runs, segments })
    }

    /// Exact current run aliases choose their archive. A separately archived
    /// identity is never attached to another master snapshot by a prefix guess.
    pub(super) fn archived_references(
        &self,
        source: &str,
        anchor: NaiveDate,
        targets: &[(String, String)],
        master: Option<&str>,
        source_removed: bool,
    ) -> Result<Vec<(String, String, String)>, String> {
        let targets: std::collections::BTreeMap<_, _> = targets
            .iter()
            .map(|(date, id)| (date.as_str(), id.as_str()))
            .collect();
        let mut result = Vec::new();
        for run in &self.runs {
            if run.event_id.as_deref() != Some(source) {
                continue;
            }
            let current = run
                .current_occurrence_id
                .as_deref()
                .unwrap_or(&run.original_event_id);
            let (root, date) = super::ids::split_synthetic_id(current);
            if root != source {
                return Err("Calendar history has lost its current occurrence identity".into());
            }
            let date = date
                .map(parse_date)
                .transpose()?
                .unwrap_or(anchor)
                .to_string();
            let archive = targets
                .get(date.as_str())
                .copied()
                .or(master.filter(|_| source_removed));
            if let Some(archive) = archive {
                result.push((run.id.clone(), current.into(), archive.into()));
            } else if source_removed {
                return Err("Calendar deletion has no archive for committed Focus history".into());
            }
        }
        Ok(result)
    }

    /// Clock correction cannot place a current edit before already completed
    /// execution on that exact occurrence, even when a run has no current occurrence.
    pub(super) fn verify_completed_before(
        &self,
        occurrence_id: &str,
        now_ms: i64,
    ) -> Result<(), String> {
        for run in &self.runs {
            if run
                .current_occurrence_id
                .as_deref()
                .unwrap_or(&run.original_event_id)
                != occurrence_id
            {
                continue;
            }
            if let Some(ended_at) = &run.ended_at {
                let ended = chrono::DateTime::parse_from_rfc3339(ended_at)
                    .map_err(|error| {
                        format!("Calendar recorded Focus completion is invalid: {error}")
                    })?
                    .timestamp_millis();
                if ended > now_ms {
                    return Err("Calendar native time precedes recorded Focus completion; refresh after clock correction".into());
                }
            }
        }
        Ok(())
    }

    /// Select closed runs by canonical occurrence identity before writing. Their
    /// original IDs, day-plan dates and accepted segments remain historical facts.
    pub(super) fn historical_retargets(
        &self,
        source: &str,
        anchor: NaiveDate,
        targets: &[(String, String)],
    ) -> Result<Vec<(String, String, String)>, String> {
        let targets: std::collections::BTreeMap<_, _> = targets
            .iter()
            .map(|(date, id)| (date.as_str(), id.as_str()))
            .collect();
        let mut result = Vec::new();
        for run in &self.runs {
            if run.ended_at.is_none() || run.event_id.as_deref() != Some(source) {
                continue;
            }
            let current = run
                .current_occurrence_id
                .as_deref()
                .unwrap_or(&run.original_event_id);
            let (root, date) = super::ids::split_synthetic_id(current);
            if root != source {
                return Err("Calendar history has lost its current occurrence identity".into());
            }
            let date = date
                .map(parse_date)
                .transpose()?
                .unwrap_or(anchor)
                .to_string();
            if let Some(target) = targets.get(date.as_str()) {
                result.push((run.id.clone(), current.into(), (*target).into()));
            }
        }
        Ok(result)
    }

    /// Focus day-plan dates use the device zone. Recurrence identities use the
    /// event's home zone, so protection must resolve identities on the worker.
    pub(super) fn resolve(&self, source: &str, anchor: NaiveDate) -> Result<ScopeEvidence, String> {
        let identity_date = |id: &str| -> Result<Option<NaiveDate>, String> {
            let (root, date) = super::ids::split_synthetic_id(id);
            if root != source {
                return Ok(None);
            }
            date.map(parse_date)
                .transpose()
                .map(|date| Some(date.unwrap_or(anchor)))
        };
        let mut evidence = ScopeEvidence::default();
        for segment in &self.segments {
            let current = segment
                .current_occurrence_id
                .as_deref()
                .unwrap_or(&segment.original_event_id);
            let date = if segment.event_id.contains("::") {
                identity_date(&segment.event_id)?
            } else {
                identity_date(current)?.or(identity_date(&segment.original_event_id)?)
            }
            .ok_or("Calendar execution segment has lost its recurrence identity")?;
            evidence.history_dates.insert(date);
        }
        for run in &self.runs {
            if let Some(date) = identity_date(&run.original_event_id)? {
                evidence.history_dates.insert(date);
            }
            if run.event_id.as_deref() == Some(source) {
                let current = run
                    .current_occurrence_id
                    .as_deref()
                    .unwrap_or(&run.original_event_id);
                let date = identity_date(current)?
                    .ok_or("Calendar execution run has lost its current recurrence identity")?;
                evidence.history_dates.insert(date);
                if run.ended_at.is_none()
                    && evidence.active.replace((run.id.clone(), date)).is_some()
                {
                    return Err("Calendar source has multiple active Focus runs".into());
                }
            }
        }
        Ok(evidence)
    }
}

pub(super) struct ScopeSnapshot {
    pub(super) geometry: Geometry,
    pub(super) evidence: ExecutionEvidence,
    pub(super) metadata: super::metadata::Metadata,
}

impl ScopeSnapshot {
    /// Pure planning can run after releasing a preview read snapshot, or inside
    /// a retained commit transaction before any of its writes are performed.
    #[cfg(test)]
    pub(super) fn plan(
        self,
        selected: NaiveDate,
        scope: EditScope,
        clock: ScopeClock,
    ) -> Result<ScopePlan, String> {
        let template = self.geometry.template(true)?;
        let evidence = self
            .evidence
            .resolve(&self.geometry.source.id, template.anchor_date())?;
        template.plan_scope(selected, scope, evidence, clock)
    }
}

/// Capture complete source-related execution records, independently of UI windows.
/// Geometry and history share one allowance, including duplicate history rows.
pub(super) async fn read_snapshot(
    connection: &mut SqliteConnection,
    template_id: &str,
) -> Result<ScopeSnapshot, String> {
    let mut budget = ReadBudget::default();
    let geometry = read_geometry(connection, template_id, true, &mut budget).await?;
    // The prefix upper bound advances the final ':' byte. Equality/range probes
    // can use the existing event and original-event indexes, unlike suffix scans.
    // Sort the bounded evidence in Rust; SQL ordering before LIMIT could sort an
    // oversized history before the allocation preflight has rejected it.
    let runs = budget
        .read::<RunEvidence>(
            connection,
            template_id,
            "SELECT id, event_id, original_event_id, current_occurrence_id, calendar_archive_id, ended_at FROM pomodoro_runs
         WHERE event_id = ?1 OR original_event_id = ?1
            OR (original_event_id >= ?1 || '::' AND original_event_id < ?1 || ':;')",
            &["id", "event_id", "original_event_id", "current_occurrence_id", "calendar_archive_id", "ended_at"],
        )
        .await?;
    let segments = budget
        .read::<SegmentEvidence>(
            connection,
            template_id,
            "SELECT s.id, s.run_id, s.event_id, r.original_event_id, r.current_occurrence_id
             FROM pomodoro_segments s JOIN pomodoro_runs r ON r.id = s.run_id
             WHERE s.event_id = ?1 OR (s.event_id >= ?1 || '::' AND s.event_id < ?1 || ':;')",
            &[
                "id",
                "run_id",
                "event_id",
                "original_event_id",
                "current_occurrence_id",
            ],
        )
        .await?;
    let evidence = ExecutionEvidence { runs, segments };
    let metadata = super::metadata::Metadata::read(connection, template_id, &mut budget).await?;
    Ok(ScopeSnapshot {
        geometry,
        evidence,
        metadata,
    })
}

/// Release SQLite and the Focus owner before doing preview CPU work. A clock
/// floor captured from that owner prevents previews moving backwards in time.
pub(super) async fn prepare_request_with_clock<T, F>(
    pool: &SqlitePool,
    device: Arc<dyn DeviceDate>,
    request: ScopeRequest,
    clock_floor_ms: i64,
    prepare: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(ScopeSnapshot, NaiveDate, EditScope, ScopeClock) -> Result<T, String>
        + Send
        + 'static,
{
    if request.recurrence_date.len() != 10 {
        return Err("Calendar occurrence identity requires a YYYY-MM-DD date".into());
    }
    let selected = parse_date(&request.recurrence_date)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Calendar scope snapshot: {error}"))?;
    let snapshot = read_snapshot(&mut tx, &request.template_id).await?;
    let native_now = super::time::current_utc_iso(&mut tx).await?;
    let now_ms = super::time::calendar_timestamp_millis(&native_now)
        .ok_or("Calendar scope requires a valid native clock")?;
    let persisted_floor: i64 = sqlx::query_scalar(
        "SELECT updated_at_ms FROM pomodoro_execution_state WHERE singleton = 1",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| format!("read Calendar execution clock: {error}"))?;
    let now_ms = now_ms.max(clock_floor_ms).max(persisted_floor);
    tx.commit()
        .await
        .map_err(|error| format!("finish Calendar scope snapshot: {error}"))?;
    let permit = admit_preview_worker().await?;
    let worker = tokio::task::spawn_blocking(move || {
        // Keep admission closed until CPU work ends, even after waiter cancellation.
        let _permit = permit;
        let floating_today = if snapshot.geometry.source.all_day != 0 {
            Some(device.local_date(now_ms)?)
        } else {
            None
        };
        prepare(
            snapshot,
            selected,
            request.scope,
            ScopeClock {
                epoch_ms: now_ms,
                floating_today,
            },
        )
    });
    // Timeout reports failure without releasing the worker's admission permit.
    // A stalled platform query cannot accumulate replacement blocking workers.
    tokio::time::timeout(SCOPE_WORKER_TIMEOUT, worker)
    .await.map_err(|_| "Calendar scope worker timed out; wait for its native query to finish before retrying")?
    .map_err(|error| format!("Calendar scope planning worker: {error}"))?
}

//! Transaction-bound Calendar commitment selection for native Focus execution.

use std::collections::BTreeMap;
use std::io::{self, Write};

use chrono::DateTime;
use ganbaru_pomodoro::{
    FocusCommitment, FocusConfiguration, PomodoroRunRhythm, PomodoroRunSequenceStep,
};
use jiff::tz::TimeZone;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

use super::native_window::{self, NativeCalendarWindow};
use super::{DbCalendarEventRow, DbOverrideRow};
use crate::recurrence::canonical::Window;
use ganbaru_civil_time as civil_time;

const MAX_PLANNED_BLOCKS: usize = 512;

/// A device-local day-plan snapshot derived from canonical occurrences.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusPlannedBlock {
    pub event_date: String,
    pub event_id: String,
    pub original_event_id: String,
    pub planned_start: String,
    pub planned_end: String,
    pub source_kind: &'static str,
}

pub struct FocusCalendarContext {
    pub commitment: Option<FocusCommitment>,
    pub next_boundary_ms: Option<i64>,
    pub planned_blocks: Vec<FocusPlannedBlock>,
}

/// Resolve the device zone natively and retain the caller's accepted SQLite transaction.
pub async fn resolve(
    connection: &mut SqliteConnection,
    now_ms: i64,
    requested_occurrence_id: Option<&str>,
) -> Result<FocusCalendarContext, String> {
    resolve_in_zone(
        connection,
        now_ms,
        requested_occurrence_id,
        &civil_time::system_zone()?,
    )
    .await
}

async fn resolve_in_zone(
    connection: &mut SqliteConnection,
    now_ms: i64,
    requested_occurrence_id: Option<&str>,
    device_zone: &TimeZone,
) -> Result<FocusCalendarContext, String> {
    if requested_occurrence_id.is_some_and(|id| id.is_empty() || id.len() > 2_048) {
        return Err("invalid requested Focus occurrence identity".into());
    }
    let local_date = civil_time::instant_to_local(now_ms, device_zone)?.date();
    let next_date = local_date
        .checked_add_signed(chrono::Duration::days(1))
        .ok_or("Focus day-plan window exceeds its date range")?;
    let window = Window::new(&local_date.to_string(), &next_date.to_string(), device_zone)?;
    let active_occurrence: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(r.current_occurrence_id, r.original_event_id) FROM pomodoro_execution_state s JOIN pomodoro_runs r
         ON r.id = json_extract(s.state_json, '$.runId')
         WHERE s.singleton = 1 AND r.ended_at IS NULL",
    )
    .fetch_optional(&mut *connection)
    .await
    .map_err(|error| format!("read committed Focus owner: {error}"))?;
    let source = native_window::read(
        connection,
        &window,
        native_window::WindowPurpose::Focus,
        false,
    )
    .await?;
    // The writer transaction remains with the caller while bounded CPU work leaves the async executor.
    let projected = tokio::task::spawn_blocking(move || source.expand(&window))
        .await
        .map_err(|error| format!("expand native Focus commitments: {error}"))??;
    let mut context = select(
        projected,
        now_ms,
        local_date.to_string(),
        device_zone,
        active_occurrence.as_deref(),
        requested_occurrence_id,
    )?;
    // An overnight or multi-day owner can have started outside today's window.
    // Capture its full original device-day plan in the same accepted transaction.
    if let Some(commitment) = context
        .commitment
        .as_ref()
        .filter(|value| value.event_date != local_date.to_string())
    {
        let plan_date = chrono::NaiveDate::parse_from_str(&commitment.event_date, "%Y-%m-%d")
            .map_err(|error| format!("invalid native Focus plan date: {error}"))?;
        let plan_end = plan_date
            .succ_opt()
            .ok_or("Focus day plan exceeds its date range")?;
        let plan_window = Window::new(&plan_date.to_string(), &plan_end.to_string(), device_zone)?;
        let source = native_window::read(
            connection,
            &plan_window,
            native_window::WindowPurpose::Focus,
            false,
        )
        .await?;
        let projected = tokio::task::spawn_blocking(move || source.expand(&plan_window))
            .await
            .map_err(|error| format!("expand original Focus day plan: {error}"))??;
        context.planned_blocks = select(
            projected,
            now_ms,
            plan_date.to_string(),
            device_zone,
            active_occurrence.as_deref(),
            Some(&commitment.occurrence_id),
        )?
        .planned_blocks;
    }
    Ok(context)
}

fn configuration(event: &DbCalendarEventRow) -> Result<FocusConfiguration, String> {
    let required = |value: Option<i64>| {
        value
            .filter(|value| *value > 0)
            .ok_or_else(|| format!("incomplete Focus configuration for {}", event.id))
    };
    let rhythm = match event.rhythm_kind.as_deref() {
        Some("count") => PomodoroRunRhythm::Count {
            focus_duration_minutes: required(event.count_focus_duration_minutes)?,
            short_break_minutes: required(event.count_short_break_minutes)?,
            long_break_minutes: required(event.count_long_break_minutes)?,
            long_break_after_focus_count: required(event.count_long_break_after_focus_count)?,
        },
        Some("sequence") => {
            let steps: Vec<PomodoroRunSequenceStep> = serde_json::from_str(
                event
                    .sequence_steps
                    .as_deref()
                    .ok_or("missing canonical Focus sequence steps")?,
            )
            .map_err(|error| format!("invalid canonical Focus sequence: {error}"))?;
            if steps.is_empty()
                || steps.len() > 12
                || steps.iter().any(|step| {
                    step.focus_duration_minutes <= 0
                        || step.break_duration_minutes <= 0
                        || !matches!(step.break_phase.as_str(), "short_break" | "long_break")
                })
            {
                return Err("invalid canonical Focus sequence rhythm".into());
            }
            PomodoroRunRhythm::Sequence { steps }
        }
        _ => return Err("canonical Focus event lacks a supported rhythm".into()),
    };
    Ok(FocusConfiguration {
        rhythm,
        rhythm_source: event
            .rhythm_source
            .clone()
            .ok_or("missing Focus rhythm source")?,
        preset_key: event.preset_key.clone(),
        idle_timeout_minutes: event.idle_timeout_minutes,
    })
}

fn milliseconds(value: &str) -> Result<i64, String> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.timestamp_millis())
        .map_err(|error| format!("invalid native Focus occurrence instant: {error}"))
}

fn select(
    projection: NativeCalendarWindow,
    now_ms: i64,
    local_date: String,
    device_zone: &TimeZone,
    active_occurrence: Option<&str>,
    requested_occurrence: Option<&str>,
) -> Result<FocusCalendarContext, String> {
    let events: BTreeMap<_, _> = projection
        .events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect();
    let overrides: BTreeMap<_, _> = projection
        .overrides
        .iter()
        .map(|value| (value.id.as_str(), value))
        .collect();
    let mut revisions = BTreeMap::new();
    let mut candidates = Vec::new();
    let mut planned_blocks = Vec::new();
    let mut next_boundary_ms = None;
    for occurrence in projection.occurrences {
        let event = events
            .get(occurrence.template_id.as_str())
            .ok_or("native Focus template is missing")?;
        let selected_override = occurrence
            .override_id
            .as_deref()
            .and_then(|id| overrides.get(id));
        if event.all_day == 1
            || selected_override
                .and_then(|value| value.status.as_deref())
                .unwrap_or(&event.status)
                == "cancelled"
        {
            continue;
        }
        let start_ms = milliseconds(&occurrence.start_time)?;
        let end_ms = milliseconds(&occurrence.end_time)?;
        let configuration = configuration(event)?;
        for boundary in [start_ms, end_ms] {
            if boundary > now_ms {
                next_boundary_ms =
                    Some(next_boundary_ms.map_or(boundary, |previous: i64| previous.min(boundary)));
            }
        }
        let start_date = civil_time::instant_to_local(start_ms, device_zone)?
            .date()
            .to_string();
        // The expanded occurrence set has native admission bounds. Apply the
        // 512-block day-plan limit after selecting the owner's device-local day.
        if start_date <= local_date {
            planned_blocks.push(FocusPlannedBlock {
                event_date: start_date.clone(),
                event_id: occurrence.id.clone(),
                original_event_id: event.id.clone(),
                planned_start: occurrence.start_time.clone(),
                planned_end: occurrence.end_time.clone(),
                source_kind: "scheduler_snapshot",
            });
        }
        if start_ms > now_ms || now_ms >= end_ms {
            continue;
        }
        let revision = match revisions.entry((
            event.id.as_str(),
            selected_override.map(|value| value.id.as_str()),
        )) {
            std::collections::btree_map::Entry::Occupied(value) => value.into_mut(),
            std::collections::btree_map::Entry::Vacant(value) => {
                value.insert(revision(event, selected_override.copied())?)
            }
        };
        candidates.push((
            event.created_at.as_str(),
            FocusCommitment {
                event_id: event.id.clone(),
                occurrence_id: occurrence.id,
                event_date: start_date,
                title: Some(
                    selected_override
                        .and_then(|value| value.title.as_ref())
                        .unwrap_or(&event.title)
                        .clone(),
                ),
                start_ms,
                end_ms,
                configuration,
                calendar_revision: revision.clone(),
            },
        ));
    }
    candidates.sort_by(|(left_created, left), (right_created, right)| {
        left.end_ms
            .cmp(&right.end_ms)
            .then_with(|| {
                left_created
                    .encode_utf16()
                    .cmp(right_created.encode_utf16())
            })
            .then_with(|| {
                left.occurrence_id
                    .encode_utf16()
                    .cmp(right.occurrence_id.encode_utf16())
            })
    });
    let selected = if let Some(requested) = requested_occurrence {
        candidates
            .iter()
            .position(|(_, value)| value.occurrence_id == requested)
    } else {
        active_occurrence
            .and_then(|active| {
                candidates
                    .iter()
                    .position(|(_, value)| value.occurrence_id == active)
            })
            .or_else(|| (!candidates.is_empty()).then_some(0))
    };
    let commitment = selected.map(|index| candidates.remove(index).1);
    let plan_date = commitment
        .as_ref()
        .map(|value| value.event_date.as_str())
        .unwrap_or(&local_date);
    planned_blocks.retain(|block| block.event_date == plan_date);
    if planned_blocks.len() > MAX_PLANNED_BLOCKS {
        return Err("Focus day plan exceeds 512 canonical blocks".into());
    }
    Ok(FocusCalendarContext {
        commitment,
        next_boundary_ms,
        planned_blocks,
    })
}

struct RevisionWriter(Sha256);
impl Write for RevisionWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn revision(
    event: &DbCalendarEventRow,
    selected_override: Option<&DbOverrideRow>,
) -> Result<String, String> {
    let mut writer = RevisionWriter(Sha256::new());
    serde_json::to_writer(&mut writer, &(event, selected_override))
        .map_err(|error| format!("fingerprint native Focus Calendar context: {error}"))?;
    Ok(format!("{:x}", writer.0.finalize()))
}

#[cfg(test)]
mod tests;

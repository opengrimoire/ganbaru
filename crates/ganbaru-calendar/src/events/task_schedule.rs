//! Project scheduling is one reviewed Calendar, Project and Focus operation.

use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

use super::create::CreateScope;
use super::edit::EventDraft;
use super::metadata::{Metadata, PreparedMutation, revision};
use super::types::{
    CalendarEventUpdateField as Field, CalendarPomodoroConfig, CalendarPomodoroConfigPatch,
    CalendarPomodoroRhythm,
};
use crate::reads::native_window::NativeCalendarWindow;
use crate::recurrence::canonical::{EditScope, TimingIntent, Window};
use ganbaru_projects::scheduling::{ScheduleSnapshot, ScheduleTaskSelection, ScheduledTaskWrite};

const MAX_DURATION_MINUTES: i64 = 24 * 60;
const MILLIS_PER_MINUTE: i64 = 60 * 1_000;

/// Established count presets, in focus, short break, long break and cycle order.
/// Event configuration snapshots remain independent of later preset changes.
type CountPresetDurations = (i64, i64, i64, i64);
const COUNT_PRESETS: [(&str, CountPresetDurations); 5] = [
    ("adaptive", (40, 5, 10, 4)),
    ("creative", (25, 5, 15, 4)),
    ("balanced", (30, 5, 10, 4)),
    ("deep", (40, 5, 10, 4)),
    ("extended", (50, 10, 10, 4)),
];

/// Authored scheduling intent contains no Calendar rows or task replacement payloads.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ScheduleRequest {
    kind: ScheduleKind,
    project_id: String,
    tasks: Vec<ScheduleTaskSelection>,
    start_time: String,
    timezone: String,
    duration_minutes: i64,
    /// An explicit snapshot of the user's global idle preference. Project-specific
    /// preferences are always loaded natively and override this choice.
    global_idle_timeout_minutes: Option<i64>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ScheduleKind {
    ScheduleTasks,
}

pub(super) struct PreparedSchedule {
    rows: PreparedMutation,
    tasks: Vec<ScheduledTaskWrite>,
    review_revision: String,
}

/// Accepted task/event association returned by both review and durable retry.
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScheduledTaskIdentity {
    pub(crate) task_id: String,
    pub(crate) event_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SchedulePreview {
    command_id: String,
    source_id: String,
    edited_id: String,
    review_revision: String,
    changed: bool,
    scope: CreateScope,
    window: NativeCalendarWindow,
    previewed_ids: Vec<String>,
    editing_id: Option<String>,
    scheduled_tasks: Vec<ScheduledTaskIdentity>,
}

impl ScheduleRequest {
    /// Reject excessive and ambiguous requests before any source lookup.
    pub(super) fn check_limits(&self) -> Result<(), String> {
        if !(1..=MAX_DURATION_MINUTES).contains(&self.duration_minutes)
            || self.start_time.len() > 64
            || self.timezone.is_empty()
            || self.timezone.len() > 255
            || self
                .global_idle_timeout_minutes
                .is_some_and(|value| ![1, 2, 3, 4, 5, 10, 15].contains(&value))
        {
            return Err("Task scheduling exceeds its selection, time or idle limits".into());
        }
        ganbaru_projects::scheduling::validate_selection(&self.project_id, &self.tasks)
    }

    /// Capture the canonical Project source under the caller's SQLite snapshot.
    pub(super) async fn read(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<ScheduleSnapshot, String> {
        self.check_limits()?;
        ganbaru_projects::scheduling::read(tx, &self.project_id, &self.tasks).await
    }

    /// The first civil label resolves through the same native gap and fold rules
    /// as event creation. Subsequent timed blocks advance by elapsed minutes.
    pub(super) fn prepare(
        &self,
        snapshot: ScheduleSnapshot,
        command_id: &str,
        now_ms: i64,
    ) -> Result<PreparedSchedule, String> {
        self.check_limits()?;
        super::commit::validate_command_id(command_id)?;
        let reviewed_input = revision(&("project-scheduling-v1", command_id, self, &snapshot))?;
        let zone = ganbaru_civil_time::zone(&self.timezone)?;
        let all_day = match snapshot.defaults.time_mode.as_str() {
            "all_day" => true,
            "timed" => false,
            _ => return Err("Project has an invalid event time mode".into()),
        };
        let civil = crate::recurrence::canonical::stored_time(&self.start_time, &zone, false)?;
        if !(1..=9999).contains(&chrono::Datelike::year(&civil.0)) {
            return Err("Schedule start is outside supported years".into());
        }
        let pomodoro = focus_config(
            &snapshot.defaults,
            self.global_idle_timeout_minutes,
            all_day,
        )?;
        let mut metadata = Vec::with_capacity(snapshot.tasks.len());
        let mut tasks = Vec::with_capacity(snapshot.tasks.len());
        for (ordinal, task) in snapshot.tasks.into_iter().enumerate() {
            let id = format!("calendar-schedule-{}", revision(&(command_id, &task.id))?);
            let start_ms = civil
                .1
                .checked_add((ordinal as i64) * self.duration_minutes * MILLIS_PER_MINUTE)
                .ok_or("Scheduled start exceeds the instant range")?;
            let end_ms = start_ms
                .checked_add(self.duration_minutes * MILLIS_PER_MINUTE)
                .ok_or("Scheduled end exceeds the instant range")?;
            let instant = |ms| {
                DateTime::from_timestamp_millis(ms)
                    .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
                    .ok_or_else(|| "Schedule exceeds supported instants".to_string())
            };
            // Floating tasks all belong to the chosen day. A minute duration
            // cannot move an all-day event to a different day.
            let date = if all_day {
                civil.0.date().to_string()
            } else {
                ganbaru_civil_time::instant_to_local(start_ms, &zone)?
                    .date()
                    .to_string()
            };
            let mut draft = EventDraft {
                timing: TimingIntent {
                    start_time: Some(if all_day {
                        date.clone()
                    } else {
                        instant(start_ms)?
                    }),
                    end_time: Some(if all_day {
                        date.clone()
                    } else {
                        instant(end_ms)?
                    }),
                    timezone: Some(self.timezone.clone()),
                    all_day: Some(all_day),
                    ..TimingIntent::default()
                },
                fields: vec![
                    Field::Title(task.title.clone()),
                    Field::ProjectId(Some(snapshot.project_id.clone())),
                    Field::Color(snapshot.defaults.color),
                    Field::EnvironmentId(snapshot.defaults.environment.clone()),
                    Field::PlaylistId(snapshot.defaults.playlist.clone()),
                    Field::MusicSnapshotAssignments(snapshot.music.clone()),
                ],
                pomodoro_config: pomodoro.clone().map(CalendarPomodoroConfigPatch::Set),
                ..EventDraft::default()
            };
            draft.validate()?;
            metadata.push(Metadata::create(
                id.clone(),
                &draft,
                draft.timing.resolve_creation()?,
                now_ms,
            )?);
            tasks.push(ScheduledTaskWrite {
                task,
                event_id: id,
                date,
            });
        }
        Ok(PreparedSchedule {
            rows: PreparedMutation::creations(metadata)?,
            tasks,
            review_revision: reviewed_input,
        })
    }

    /// Admit one actual blocking worker and retain its permit through cancellation.
    pub(super) async fn prepare_commit(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        command_id: String,
        expected: String,
        now_ms: i64,
    ) -> Result<super::commit::PreparedCommit, String> {
        let permit = super::scope::SCOPE_GATE
            .try_acquire()
            .map_err(|_| "A Calendar operation is being prepared; retry after it finishes")?;
        let snapshot = self.read(tx).await?;
        let request = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            request.prepare_reviewed(snapshot, &command_id, &expected, now_ms)
        });
        tokio::time::timeout(super::scope::SCOPE_WORKER_TIMEOUT, worker)
            .await
            .map_err(|_| "Project scheduling preparation timed out")?
            .map_err(|error| format!("Project scheduling worker: {error}"))?
    }

    /// Recompute the reviewed native snapshot under the writer before any write.
    pub(super) fn prepare_reviewed(
        &self,
        snapshot: ScheduleSnapshot,
        command_id: &str,
        expected: &str,
        now_ms: i64,
    ) -> Result<super::commit::PreparedCommit, String> {
        let prepared = self.prepare(snapshot, command_id, now_ms)?;
        if prepared.review_revision != expected {
            return Err("Project scheduling inputs changed; review again".into());
        }
        Ok(prepared.into_commit(command_id))
    }
}

fn focus_config(
    defaults: &ganbaru_projects::scheduling::ScheduleDefaults,
    global_idle: Option<i64>,
    all_day: bool,
) -> Result<Option<CalendarPomodoroConfig>, String> {
    if all_day || defaults.pomodoro_mode == "none" {
        return Ok(None);
    }
    let idle = match defaults.idle_source.as_str() {
        "global" => global_idle,
        "custom" => {
            if defaults.idle_enabled {
                Some(defaults.idle_threshold)
            } else {
                None
            }
        }
        _ => return Err("Project has an invalid idle settings source".into()),
    };
    let (focus, short, long, cycle) = match defaults.pomodoro_mode.as_str() {
        "preset" => COUNT_PRESETS
            .iter()
            .find(|(key, _)| Some(*key) == defaults.preset.as_deref())
            .map(|(_, rhythm)| *rhythm)
            .ok_or("Project has an invalid Focus preset")?,
        "custom" => (
            defaults.focus.ok_or("Project Focus duration is missing")?,
            defaults
                .short_break
                .ok_or("Project short break is missing")?,
            defaults.long_break.ok_or("Project long break is missing")?,
            defaults.cycle.ok_or("Project Focus cycle is missing")?,
        ),
        _ => return Err("Project has an invalid Focus mode".into()),
    };
    Ok(Some(CalendarPomodoroConfig {
        rhythm: CalendarPomodoroRhythm::Count {
            focus_duration_minutes: focus,
            short_break_minutes: short,
            long_break_minutes: long,
            long_break_after_focus_count: cycle,
        },
        rhythm_source: defaults.pomodoro_mode.clone(),
        preset_key: if defaults.pomodoro_mode == "preset" {
            defaults.preset.clone()
        } else {
            None
        },
        idle_timeout_minutes: idle,
    }))
}

impl PreparedSchedule {
    /// Attach Project writes to the existing Calendar and Focus owner transaction.
    pub(super) fn into_commit(self, command_id: &str) -> super::commit::PreparedCommit {
        let identities = self.identities();
        let mut commit = super::commit::PreparedCommit::from_creation(self.rows, command_id);
        commit.attach_scheduled_tasks(self.tasks, identities);
        commit
    }

    fn identities(&self) -> Vec<ScheduledTaskIdentity> {
        self.tasks
            .iter()
            .map(|row| ScheduledTaskIdentity {
                task_id: row.task.id.clone(),
                event_id: row.event_id.clone(),
            })
            .collect()
    }

    /// Use the persisted-window projection for all reviewed newly created sources.
    pub(super) fn project(
        self,
        command_id: &str,
        window: &Window,
    ) -> Result<SchedulePreview, String> {
        let projected = self.rows.project(&Metadata::default(), window)?;
        if !projected.diagnostics.is_empty() {
            return Err("Scheduled Calendar preview failed to expand".into());
        }
        let scheduled_tasks = self.identities();
        Ok(SchedulePreview {
            command_id: command_id.into(),
            source_id: self.rows.edited_id.clone(),
            editing_id: projected
                .occurrences
                .iter()
                .find(|row| row.id == self.rows.edited_id)
                .map(|row| row.id.clone()),
            edited_id: self.rows.edited_id,
            review_revision: self.review_revision,
            changed: true,
            scope: CreateScope {
                effective_scope: EditScope::This,
                selected_started: false,
                selected_has_history: false,
                selected_active: false,
            },
            previewed_ids: projected
                .occurrences
                .iter()
                .map(|row| row.id.clone())
                .collect(),
            window: projected,
            scheduled_tasks,
        })
    }
}

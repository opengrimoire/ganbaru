//! Canonical Project inputs and task-only writes for Calendar scheduling.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};
use std::collections::BTreeSet;

use super::ProjectTaskEventLinkCreate;
use super::history::insert_task_change_event;
use super::relationship_commands::link_task_event_with_project_assignment;

pub const MAX_SCHEDULE_TASKS: usize = 1_000;
const MAX_SNAPSHOT_BYTES: i64 = 64 * 1024;
const MAX_TASK_BYTES: i64 = 8 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleTaskSelection {
    pub id: String,
    pub revision: i64,
}

#[derive(Deserialize, Serialize)]
pub struct ScheduleDefaults {
    pub time_mode: String,
    pub color: Option<i64>,
    pub environment: Option<String>,
    pub playlist: Option<String>,
    pub pomodoro_mode: String,
    pub preset: Option<String>,
    pub focus: Option<i64>,
    pub short_break: Option<i64>,
    pub long_break: Option<i64>,
    pub cycle: Option<i64>,
    pub idle_source: String,
    pub idle_enabled: bool,
    pub idle_threshold: i64,
}

#[derive(Deserialize, Serialize)]
pub struct ScheduleTask {
    pub id: String,
    pub revision: i64,
    pub title: String,
    pub start_date: Option<String>,
    pub target_end_date: Option<String>,
    pub due_date: Option<String>,
}

#[derive(Serialize)]
pub struct ScheduleSnapshot {
    pub project_id: String,
    pub defaults: ScheduleDefaults,
    pub tasks: Vec<ScheduleTask>,
    pub music: Vec<ganbaru_music::assignments::MusicContextAssignmentDraft>,
}

/// Bound canonical selection reads independently of the Calendar command adapter.
pub fn validate_selection(
    project_id: &str,
    selected: &[ScheduleTaskSelection],
) -> Result<(), String> {
    let is_valid_id =
        |id: &str| !id.trim().is_empty() && id.len() <= 1_024 && !id.chars().any(char::is_control);
    let mut ids = BTreeSet::new();
    if selected.is_empty()
        || selected.len() > MAX_SCHEDULE_TASKS
        || !is_valid_id(project_id)
        || selected.iter().any(|task| {
            !is_valid_id(&task.id)
                || !(0..=9_007_199_254_740_991).contains(&task.revision)
                || !ids.insert(&task.id)
        })
    {
        return Err(
            "Task scheduling contains an invalid, repeated or excessive task selection".into(),
        );
    }
    Ok(())
}

/// Copy only scheduling inputs, with bounded returned bytes, in one read snapshot.
pub async fn read(
    tx: &mut Transaction<'_, Sqlite>,
    project_id: &str,
    selected: &[ScheduleTaskSelection],
) -> Result<ScheduleSnapshot, String> {
    validate_selection(project_id, selected)?;
    let defaults: Option<Option<String>> = sqlx::query_scalar(
        "SELECT CASE WHEN length(CAST(payload AS BLOB)) <= ?2 THEN payload END FROM
         (SELECT json_object('time_mode', default_event_time_mode, 'color', color,
          'environment', work_environment_id, 'playlist', focus_playlist_id,
          'pomodoro_mode', default_pomodoro_mode, 'preset', default_pomodoro_preset_key,
          'focus', default_pomodoro_focus_minutes, 'short_break', default_pomodoro_short_break_minutes,
          'long_break', default_pomodoro_long_break_minutes, 'cycle', default_pomodoro_long_break_after_focus_count,
          'idle_source', default_idle_settings_source, 'idle_enabled', json(CASE default_idle_pause_enabled WHEN 1 THEN 'true' ELSE 'false' END),
          'idle_threshold', default_idle_threshold_minutes) AS payload FROM projects WHERE id = ?1)",
    ).bind(project_id).bind(MAX_SNAPSHOT_BYTES).fetch_optional(&mut **tx).await
        .map_err(|error| format!("read scheduling project: {error}"))?;
    let defaults = serde_json::from_str(
        &defaults
            .ok_or("Scheduling project is missing")?
            .ok_or("Project scheduling defaults exceed their byte budget")?,
    )
    .map_err(|error| format!("decode scheduling defaults: {error}"))?;
    let ids = serde_json::to_string(&selected.iter().map(|task| &task.id).collect::<Vec<_>>())
        .map_err(|error| format!("encode scheduled task selection: {error}"))?;
    let rows: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT CASE WHEN length(CAST(payload AS BLOB)) <= ?3 THEN payload END FROM
         (SELECT json_object('id', t.id, 'revision', t.revision, 'title', t.title,
          'start_date', t.start_date, 'target_end_date', t.target_end_date, 'due_date', t.due_date) AS payload,
          CAST(chosen.key AS INTEGER) AS ordinal
          FROM json_each(?1) chosen JOIN project_tasks t ON t.id = chosen.value
          WHERE t.project_id = ?2 AND t.archived_at IS NULL) ORDER BY ordinal",
    ).bind(ids).bind(project_id).bind(MAX_TASK_BYTES).fetch_all(&mut **tx).await
        .map_err(|error| format!("read scheduled tasks: {error}"))?;
    if rows.len() != selected.len() {
        return Err("A scheduled task is missing, archived or belongs to another project".into());
    }
    let tasks: Vec<ScheduleTask> = rows
        .into_iter()
        .map(|row| {
            serde_json::from_str(&row.ok_or("Scheduled task exceeds its byte budget")?)
                .map_err(|error| format!("decode scheduled task: {error}"))
        })
        .collect::<Result<_, _>>()?;
    if tasks
        .iter()
        .zip(selected)
        .any(|(task, expected)| task.id != expected.id || task.revision != expected.revision)
    {
        return Err("A scheduled task changed; refresh the selection before scheduling".into());
    }
    let music: Vec<Option<String>> = sqlx::query_scalar(
        "SELECT CASE WHEN length(CAST(payload AS BLOB)) <= ?2 THEN payload END FROM
         (SELECT json_object('phase', phase, 'behavior', behavior, 'playlistId', playlist_id,
          'soundscapeId', soundscape_id, 'soundscapeBehavior', soundscape_behavior,
          'provenanceKind', 'copied-project', 'provenanceId', owner_id) AS payload
          FROM music_context_assignments WHERE owner_kind = 'project-default' AND owner_id = ?1
          ORDER BY phase LIMIT 4)",
    )
    .bind(project_id)
    .bind(MAX_SNAPSHOT_BYTES)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| format!("read scheduling soundtrack: {error}"))?;
    if music.len() > 3 {
        return Err("Project soundtrack exceeds its phase budget".into());
    }
    let music = music
        .into_iter()
        .map(|row| {
            serde_json::from_str(&row.ok_or("Project soundtrack exceeds its byte budget")?)
                .map_err(|error| format!("decode project soundtrack: {error}"))
        })
        .collect::<Result<_, _>>()?;
    Ok(ScheduleSnapshot {
        project_id: project_id.into(),
        defaults,
        tasks,
        music,
    })
}

pub struct ScheduledTaskWrite {
    pub task: ScheduleTask,
    pub event_id: String,
    pub date: String,
}

/// Called inside the same owner transaction as Calendar rows and its receipt.
pub async fn write(
    tx: &mut Transaction<'_, Sqlite>,
    tasks: Vec<ScheduledTaskWrite>,
) -> Result<(), String> {
    for scheduled in tasks {
        let task = scheduled.task;
        let result = sqlx::query(
            "UPDATE project_tasks SET start_date = ?1, target_end_date = ?1,
             due_date = COALESCE(due_date, ?1), updated_at = datetime('now')
             WHERE id = ?2 AND revision = ?3 AND archived_at IS NULL",
        )
        .bind(&scheduled.date)
        .bind(&task.id)
        .bind(task.revision)
        .execute(&mut **tx)
        .await
        .map_err(|error| format!("write scheduled task dates: {error}"))?;
        if result.rows_affected() != 1 {
            return Err("Scheduled task changed before writing".into());
        }
        for (field, old) in [
            ("start_date", task.start_date),
            ("target_end_date", task.target_end_date),
            ("due_date", task.due_date),
        ] {
            if (field != "due_date" || old.is_none()) && old.as_ref() != Some(&scheduled.date) {
                insert_task_change_event(
                    tx,
                    &task.id,
                    "updated",
                    Some(field),
                    old.as_deref(),
                    Some(&scheduled.date),
                )
                .await?;
            }
        }
        link_task_event_with_project_assignment(
            tx,
            &ProjectTaskEventLinkCreate {
                task_id: task.id,
                event_id: scheduled.event_id,
                link_kind: "scheduled".into(),
            },
        )
        .await?;
    }
    Ok(())
}

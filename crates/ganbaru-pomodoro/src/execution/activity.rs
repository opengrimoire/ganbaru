//! Device Focus activity: the row each device publishes so other devices can see what it is
//! focusing on. It is projected from the committed execution snapshot and written in the same
//! transaction as the transition, only when the projection changes, so heartbeats and no-op
//! observations never write it.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqliteConnection, Transaction};

use super::models::{
    FocusExecutionContext, FocusExecutionError, FocusExecutionSnapshot, FocusMode, FocusPhase,
};

/// How long a running phase stays fresh after its planned end.
pub const FOCUS_ACTIVITY_RUNNING_GRACE_MS: i64 = 10 * 60 * 1000;
/// How long a pause stays fresh after it started.
pub const FOCUS_ACTIVITY_PAUSED_GRACE_MS: i64 = 30 * 60 * 1000;
/// Most activity rows one read returns; one row per linked device keeps this far above use.
const MAX_ACTIVITY_ROWS: i64 = 64;

/// What a device is doing, as stored in `pomodoro_device_activity.state`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusActivityState {
    Idle,
    Focus,
    ShortBreak,
    LongBreak,
    Paused,
}

impl FocusActivityState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Focus => "focus",
            Self::ShortBreak => "short_break",
            Self::LongBreak => "long_break",
            Self::Paused => "paused",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [
            Self::Idle,
            Self::Focus,
            Self::ShortBreak,
            Self::LongBreak,
            Self::Paused,
        ]
        .into_iter()
        .find(|state| state.as_str() == value)
    }
}

/// The activity of one device, without its key or write time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusActivityProjection {
    pub state: FocusActivityState,
    pub run_id: Option<String>,
    pub occurrence_id: Option<String>,
    pub event_title: Option<String>,
    pub phase_started_at_ms: Option<i64>,
    pub phase_ends_at_ms: Option<i64>,
    pub expires_at_ms: i64,
}

/// A fresh activity row of another device.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusDeviceActivity {
    pub device_id: String,
    pub state: FocusActivityState,
    pub occurrence_id: Option<String>,
    pub event_title: Option<String>,
    pub phase_started_at_ms: Option<i64>,
    pub phase_ends_at_ms: Option<i64>,
    pub expires_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Projects the activity a committed snapshot shows: a running phase stays fresh until its
/// planned end plus `FOCUS_ACTIVITY_RUNNING_GRACE_MS`, any paused or waiting mode until its
/// start plus `FOCUS_ACTIVITY_PAUSED_GRACE_MS`, and a device without an open run is idle and
/// already expired.
pub fn focus_activity_projection(snapshot: &FocusExecutionSnapshot) -> FocusActivityProjection {
    let now_ms = snapshot.observed_at_ms;
    let open_run = snapshot
        .run
        .as_ref()
        .filter(|run| run.ended_at_ms.is_none());
    let (Some(run), Some(segment)) = (open_run, snapshot.segment.as_ref()) else {
        return FocusActivityProjection {
            state: FocusActivityState::Idle,
            run_id: None,
            occurrence_id: None,
            event_title: None,
            phase_started_at_ms: None,
            phase_ends_at_ms: None,
            expires_at_ms: now_ms,
        };
    };
    let (state, phase_ends_at_ms, expires_at_ms) = if snapshot.mode == FocusMode::Running {
        let ends = snapshot.phase_deadline_ms.unwrap_or(now_ms);
        let state = match segment.phase {
            FocusPhase::Focus => FocusActivityState::Focus,
            FocusPhase::ShortBreak => FocusActivityState::ShortBreak,
            FocusPhase::LongBreak => FocusActivityState::LongBreak,
        };
        (
            state,
            Some(ends),
            ends.saturating_add(FOCUS_ACTIVITY_RUNNING_GRACE_MS),
        )
    } else {
        let paused_at = pause_started_at_ms(snapshot).unwrap_or(now_ms);
        (
            FocusActivityState::Paused,
            None,
            paused_at.saturating_add(FOCUS_ACTIVITY_PAUSED_GRACE_MS),
        )
    };
    FocusActivityProjection {
        state,
        run_id: Some(run.id.clone()),
        occurrence_id: Some(run.occurrence_id.clone()),
        event_title: run.title.clone(),
        phase_started_at_ms: Some(segment.actual_start_ms),
        phase_ends_at_ms,
        expires_at_ms,
    }
}

/// The start of the interruption a non-running snapshot is in.
fn pause_started_at_ms(snapshot: &FocusExecutionSnapshot) -> Option<i64> {
    let open_pause = snapshot.segment.as_ref().and_then(|segment| {
        segment
            .pauses
            .iter()
            .rev()
            .find(|pause| pause.ended_at_ms.is_none())
            .map(|pause| pause.started_at_ms)
    });
    open_pause
        .or(snapshot.idle_started_at_ms)
        .or(snapshot.suspend_started_at_ms)
        .or(snapshot.focus_failed_at_ms)
        .or(snapshot.return_started_at_ms)
}

type StoredActivity = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
);

/// Writes the activity of `device_id` for a committed snapshot in the caller's transaction.
/// Nothing is written when the stored row already shows the projection, or when a device
/// that never published activity is idle.
pub async fn focus_write_device_activity_tx(
    tx: &mut Transaction<'_, Sqlite>,
    device_id: &str,
    snapshot: &FocusExecutionSnapshot,
) -> Result<bool, FocusExecutionError> {
    let projection = focus_activity_projection(snapshot);
    let stored = sqlx::query_as::<_, StoredActivity>(
        "SELECT state, run_id, occurrence_id, event_title, phase_started_at_ms,
                phase_ends_at_ms, expires_at_ms
         FROM pomodoro_device_activity WHERE device_id = ?",
    )
    .bind(device_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| format!("Read Focus device activity: {error}"))?;
    let unchanged = match &stored {
        None => projection.state == FocusActivityState::Idle,
        Some((state, run, occurrence, title, started, ends, expires)) => {
            if projection.state == FocusActivityState::Idle {
                state == FocusActivityState::Idle.as_str()
            } else {
                state == projection.state.as_str()
                    && run == &projection.run_id
                    && occurrence == &projection.occurrence_id
                    && title == &projection.event_title
                    && started == &projection.phase_started_at_ms
                    && ends == &projection.phase_ends_at_ms
                    && *expires == projection.expires_at_ms
            }
        }
    };
    if unchanged {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO pomodoro_device_activity
             (device_id, state, run_id, occurrence_id, event_title, phase_started_at_ms,
              phase_ends_at_ms, expires_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT (device_id) DO UPDATE SET
             state = excluded.state, run_id = excluded.run_id,
             occurrence_id = excluded.occurrence_id, event_title = excluded.event_title,
             phase_started_at_ms = excluded.phase_started_at_ms,
             phase_ends_at_ms = excluded.phase_ends_at_ms,
             expires_at_ms = excluded.expires_at_ms, updated_at_ms = excluded.updated_at_ms",
    )
    .bind(device_id)
    .bind(projection.state.as_str())
    .bind(&projection.run_id)
    .bind(&projection.occurrence_id)
    .bind(&projection.event_title)
    .bind(projection.phase_started_at_ms)
    .bind(projection.phase_ends_at_ms)
    .bind(projection.expires_at_ms)
    .bind(snapshot.observed_at_ms)
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("Write Focus device activity: {error}"))?;
    Ok(true)
}

/// Writes the activity of the context's device for a committed snapshot, when it has one.
pub(super) async fn record_activity(
    tx: &mut Transaction<'_, Sqlite>,
    context: &FocusExecutionContext,
    snapshot: &FocusExecutionSnapshot,
) -> Result<(), FocusExecutionError> {
    if let Some(device_id) = &context.device_id {
        focus_write_device_activity_tx(tx, device_id, snapshot).await?;
    }
    Ok(())
}

type ActivityRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
);

/// Reads the activity rows of devices other than `local_device_id` that are focusing, on a
/// break, or paused and have not expired at `now_ms`. Expired rows are only "last seen" facts
/// and never count as focusing.
pub async fn focus_read_other_device_activity(
    conn: &mut SqliteConnection,
    local_device_id: &str,
    now_ms: i64,
) -> Result<Vec<FocusDeviceActivity>, FocusExecutionError> {
    let rows = sqlx::query_as::<_, ActivityRow>(
        "SELECT device_id, state, occurrence_id, event_title, phase_started_at_ms,
                phase_ends_at_ms, expires_at_ms, updated_at_ms
         FROM pomodoro_device_activity
         WHERE device_id <> ?1 AND state <> 'idle' AND expires_at_ms > ?2
         ORDER BY updated_at_ms DESC, device_id
         LIMIT ?3",
    )
    .bind(local_device_id)
    .bind(now_ms)
    .bind(MAX_ACTIVITY_ROWS)
    .fetch_all(&mut *conn)
    .await
    .map_err(|error| format!("Read Focus device activity: {error}"))?;
    rows.into_iter()
        .map(
            |(device_id, state, occurrence_id, event_title, started, ends, expires, updated)| {
                let state = FocusActivityState::parse(&state).ok_or_else(|| {
                    FocusExecutionError::from(format!("Unknown Focus activity state {state}"))
                })?;
                Ok(FocusDeviceActivity {
                    device_id,
                    state,
                    occurrence_id,
                    event_title,
                    phase_started_at_ms: started,
                    phase_ends_at_ms: ends,
                    expires_at_ms: expires,
                    updated_at_ms: updated,
                })
            },
        )
        .collect()
}

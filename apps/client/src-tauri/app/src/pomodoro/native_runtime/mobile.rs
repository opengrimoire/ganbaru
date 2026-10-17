//! Android presentation receives only canonical accepted phases and localized copy.

use ganbaru_pomodoro::{
    FocusExecutionError, FocusExecutionSnapshot, FocusMode, FocusPhase, FocusRunSnapshot,
    FocusSegmentSnapshot,
};
use serde::{Deserialize, Serialize};

const MAX_TEXT_UTF16_UNITS: usize = 160;
const MILLIS_PER_SECOND: i64 = 1000;

/// Localized presentation values cannot supply execution state or scheduling inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FocusNotificationCopy {
    channel_name: String,
    channel_description: String,
    alerts_channel_name: String,
    alerts_channel_description: String,
    focus_title: String,
    short_break_title: String,
    long_break_title: String,
    paused_text: String,
    focus_complete_title: String,
    break_complete_title: String,
    session_complete_text: String,
}

impl FocusNotificationCopy {
    pub fn validate(&self) -> Result<(), FocusExecutionError> {
        for value in [
            &self.channel_name,
            &self.channel_description,
            &self.alerts_channel_name,
            &self.alerts_channel_description,
            &self.focus_title,
            &self.short_break_title,
            &self.long_break_title,
            &self.paused_text,
            &self.focus_complete_title,
            &self.break_complete_title,
            &self.session_complete_text,
        ] {
            if value.trim().is_empty() || value.encode_utf16().count() > MAX_TEXT_UTF16_UNITS {
                return Err(super::error(
                    ganbaru_pomodoro::FocusErrorCode::InvalidIntent,
                    "Focus notification text must contain 1 to 160 UTF-16 units",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedPhase {
    id: String,
    phase: FocusPhase,
    rhythm_position: i64,
    starts_at_epoch_ms: i64,
    ends_at_epoch_ms: i64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedNotification {
    run_id: String,
    event_id: String,
    event_title: Option<String>,
    event_date: String,
    event_ends_at_epoch_ms: i64,
    generated_at_epoch_ms: i64,
    is_running: bool,
    remaining_seconds: i64,
    total_seconds: i64,
    config_json: String,
    phases: [AcceptedPhase; 1],
    copy: FocusNotificationCopy,
}

fn seconds(milliseconds: i64) -> i64 {
    milliseconds.saturating_add(MILLIS_PER_SECOND - 1) / MILLIS_PER_SECOND
}

/// Waiting, failed and closed phases revoke presentation; they never publish a successor.
fn accepted_notification(
    snapshot: &FocusExecutionSnapshot,
    now: i64,
    copy: FocusNotificationCopy,
) -> Result<Option<AcceptedNotification>, FocusExecutionError> {
    copy.validate()?;
    if !matches!(
        snapshot.mode,
        FocusMode::Running | FocusMode::ManualPause | FocusMode::IdlePause | FocusMode::Suspended
    ) {
        return Ok(None);
    }
    let run = snapshot
        .run
        .as_ref()
        .ok_or_else(|| "Accepted Focus phase has no run".to_owned())?;
    let segment = snapshot
        .segment
        .as_ref()
        .ok_or_else(|| "Accepted Focus phase has no segment".to_owned())?;
    if run.ended_at_ms.is_some() || now >= run.planned_end_ms {
        return Ok(None);
    }
    if segment.run_id != run.id
        || segment.status != "active"
        || segment.actual_end_ms.is_some()
        || segment.actual_start_ms > now
        || segment.chosen_duration_ms <= 0
        || snapshot.observed_at_ms > now
    {
        return Err(
            "Accepted Focus phase is inconsistent with its canonical run"
                .to_owned()
                .into(),
        );
    }
    let running = snapshot.mode == FocusMode::Running;
    let phase_end = if running {
        snapshot
            .phase_deadline_ms
            .ok_or_else(|| "Running Focus phase has no accepted deadline".to_owned())?
            .min(run.planned_end_ms)
    } else {
        run.planned_end_ms
    };
    let remaining_ms = if running {
        phase_end.saturating_sub(now)
    } else {
        snapshot
            .remaining_ms
            .min(run.planned_end_ms.saturating_sub(now))
    };
    if remaining_ms <= 0 {
        return Ok(None);
    }
    notification_payload(
        run,
        segment,
        NotificationTiming {
            now,
            phase_end,
            remaining_ms,
            running,
        },
        copy,
    )
    .map(Some)
}

struct NotificationTiming {
    now: i64,
    phase_end: i64,
    remaining_ms: i64,
    running: bool,
}

fn notification_payload(
    run: &FocusRunSnapshot,
    segment: &FocusSegmentSnapshot,
    timing: NotificationTiming,
    copy: FocusNotificationCopy,
) -> Result<AcceptedNotification, FocusExecutionError> {
    let total_seconds = seconds(segment.chosen_duration_ms);
    let remaining_seconds = seconds(timing.remaining_ms).min(total_seconds);
    let title = run.title.as_ref().and_then(|title| {
        let trimmed = title.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut units = 0;
        Some(
            trimmed
                .chars()
                .take_while(|character| {
                    units += character.len_utf16();
                    units <= MAX_TEXT_UTF16_UNITS
                })
                .collect(),
        )
    });
    Ok(AcceptedNotification {
        run_id: run.id.clone(),
        event_id: run
            .event_id
            .clone()
            .unwrap_or_else(|| run.occurrence_id.clone()),
        event_title: title,
        event_date: run.event_date.clone(),
        event_ends_at_epoch_ms: run.planned_end_ms,
        generated_at_epoch_ms: timing.now,
        is_running: timing.running,
        remaining_seconds,
        total_seconds,
        config_json: serde_json::to_string(&run.configuration)
            .map_err(|error| format!("Encode accepted Focus configuration: {error}"))?,
        phases: [AcceptedPhase {
            id: segment.id.clone(),
            phase: segment.phase,
            rhythm_position: segment.rhythm_position,
            starts_at_epoch_ms: segment.actual_start_ms,
            ends_at_epoch_ms: timing.phase_end,
        }],
        copy,
    })
}

/// Canonical closed segments can deliver reminders even if their alarm was revoked first.
fn completion_notification(
    run: &FocusRunSnapshot,
    segment: &FocusSegmentSnapshot,
    now: i64,
    copy: FocusNotificationCopy,
) -> Result<AcceptedNotification, FocusExecutionError> {
    copy.validate()?;
    let ended_at = segment
        .actual_end_ms
        .ok_or_else(|| "Focus completion has no committed phase ending".to_owned())?;
    if segment.run_id != run.id
        || segment.status == "active"
        || ended_at <= segment.actual_start_ms
        || ended_at > now
        || ended_at > run.planned_end_ms
        || segment.chosen_duration_ms <= 0
    {
        return Err("Focus completion has no valid committed phase ending"
            .to_owned()
            .into());
    }
    notification_payload(
        run,
        segment,
        NotificationTiming {
            now: segment.actual_start_ms,
            phase_end: ended_at,
            remaining_ms: 0,
            running: false,
        },
        copy,
    )
}

#[derive(Clone, PartialEq)]
struct PublishedPhase {
    generation: u64,
    revision: i64,
    run_id: Option<String>,
    phase_id: Option<String>,
    active: bool,
}

/// A closed phase must follow a current accepted publication attempt, not old history.
fn newly_closed_phase<'a>(
    previous: Option<&PublishedPhase>,
    generation: u64,
    snapshot: &'a FocusExecutionSnapshot,
) -> Option<&'a FocusSegmentSnapshot> {
    let previous = previous?;
    let run = snapshot.run.as_ref()?;
    if !previous.active
        || previous.generation != generation
        || snapshot.revision <= previous.revision
        || previous.run_id.as_deref() != Some(run.id.as_str())
        || !matches!(
            snapshot.mode,
            FocusMode::Running | FocusMode::ReturnWait | FocusMode::Expired
        )
    {
        return None;
    }
    snapshot
        .segment
        .iter()
        .chain(snapshot.changed_segments.iter())
        .find(|segment| {
            Some(segment.id.as_str()) == previous.phase_id.as_deref()
                && segment.run_id == run.id
                && segment.status != "active"
                && segment
                    .actual_end_ms
                    .is_some_and(|end| end > segment.actual_start_ms)
        })
}

#[cfg(target_os = "android")]
#[derive(Default)]
pub(super) struct AndroidPresentation {
    copy: Option<FocusNotificationCopy>,
    published: Option<PublishedPhase>,
    last_attempted: Option<PublishedPhase>,
    pending_completion: Option<AcceptedNotification>,
    completion_captured_at: Option<std::time::Instant>,
    copy_changed: bool,
}

#[cfg(target_os = "android")]
impl AndroidPresentation {
    pub async fn configure_copy(
        &mut self,
        app: &tauri::AppHandle,
        copy: FocusNotificationCopy,
    ) -> Result<(), FocusExecutionError> {
        use ganbaru_mobile_notifications::MobileNotificationsExt;
        copy.validate()?;
        if self.copy.as_ref() == Some(&copy) {
            return Ok(());
        }
        let app = app.clone();
        let retained = copy.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.mobile_notifications()
                .configure_focus_notification_copy(&retained)
        })
        .await
        .map_err(|error| format!("Configure native Focus language: {error}"))??;
        self.copy = Some(copy);
        self.copy_changed = true;
        Ok(())
    }

    pub async fn apply(
        &mut self,
        app: &tauri::AppHandle,
        projection: &super::FocusProjection,
        now: i64,
        ownership_generation: u64,
    ) -> Result<(), FocusExecutionError> {
        use ganbaru_mobile_notifications::MobileNotificationsExt;
        use tauri::Manager;
        let Some(snapshot) = &projection.snapshot else {
            return Ok(());
        };
        if self.completion_captured_at.is_some_and(|captured| {
            captured.elapsed() >= std::time::Duration::from_millis(super::EFFECT_LEASE_MS as u64)
        }) {
            self.pending_completion = None;
            self.completion_captured_at = None;
        }
        let has_phase = matches!(
            snapshot.mode,
            FocusMode::Running
                | FocusMode::ManualPause
                | FocusMode::IdlePause
                | FocusMode::Suspended
        ) && snapshot
            .run
            .as_ref()
            .is_some_and(|run| run.ended_at_ms.is_none() && now < run.planned_end_ms)
            && (snapshot.mode != FocusMode::Running
                || snapshot
                    .phase_deadline_ms
                    .is_some_and(|deadline| now < deadline));
        let key = PublishedPhase {
            generation: projection.vault_generation,
            revision: snapshot.revision,
            run_id: snapshot.run.as_ref().map(|run| run.id.clone()),
            phase_id: snapshot.segment.as_ref().map(|segment| segment.id.clone()),
            active: has_phase,
        };
        if self.published.as_ref() == Some(&key) && !self.copy_changed {
            return Ok(());
        }
        let closed = newly_closed_phase(
            self.last_attempted.as_ref(),
            projection.vault_generation,
            snapshot,
        );
        let has_completion = closed.is_some() || self.pending_completion.is_some();
        if (has_phase || has_completion) && self.copy.is_none() {
            let app = app.clone();
            self.copy = tauri::async_runtime::spawn_blocking(move || {
                app.mobile_notifications()
                    .focus_notification_copy::<FocusNotificationCopy>()
            })
            .await
            .map_err(|error| format!("Read native Focus language: {error}"))??;
        }
        let notification = if has_phase {
            accepted_notification(
                snapshot,
                now,
                self.copy
                    .clone()
                    .ok_or_else(|| "Focus notification language is not ready".to_owned())?,
            )?
        } else {
            None
        };
        if let Some(closed) = closed.filter(|closed| {
            closed
                .actual_end_ms
                .is_some_and(|ended| now.saturating_sub(ended) < super::EFFECT_LEASE_MS)
        }) {
            self.pending_completion = Some(completion_notification(
                snapshot
                    .run
                    .as_ref()
                    .ok_or_else(|| "Focus completion has no canonical run".to_owned())?,
                closed,
                now,
                self.copy
                    .clone()
                    .ok_or_else(|| "Focus notification language is not ready".to_owned())?,
            )?);
            self.completion_captured_at = Some(std::time::Instant::now());
        }
        if matches!(
            snapshot.mode,
            FocusMode::Stopped | FocusMode::IdleFailed | FocusMode::Suspended
        ) || self
            .pending_completion
            .as_ref()
            .is_some_and(|completion| Some(&completion.run_id) != key.run_id.as_ref())
        {
            self.pending_completion = None;
        }
        let completion = self.pending_completion.clone();
        self.last_attempted = Some(key.clone());
        let app = app.clone();
        let vault_id = projection
            .vault_id
            .clone()
            .ok_or_else(|| "Focus notification has no vault".to_owned())?;
        let generation = projection.vault_generation;
        let revision = snapshot.revision;
        tauri::async_runtime::spawn_blocking(move || {
            if !super::presentation_is_current(&app, generation, revision) {
                return Err("Native Focus phase delivery was superseded or expired".to_owned());
            }
            let status = app
                .state::<crate::vault::ownership::VaultOwnershipManager>()
                .status(&vault_id)?;
            if !status.can_write
                || status.generation != ownership_generation
                || crate::vault::active_vault_id(&app)? != vault_id
            {
                return Err("Focus notification belongs to an obsolete vault owner".to_owned());
            }
            let completed = completion.is_some();
            if let Some(completion) = completion {
                app.mobile_notifications().complete_focus_notification(
                    &completion,
                    generation,
                    revision,
                )?;
            }
            match notification {
                Some(notification) => app.mobile_notifications().publish_focus_notification(
                    &notification,
                    generation,
                    revision,
                ),
                None if completed => Ok(()),
                None => app.mobile_notifications().cancel_focus_notification(),
            }
        })
        .await
        .map_err(|error| format!("Publish accepted native Focus phase: {error}"))??;
        self.published = Some(key);
        self.pending_completion = None;
        self.completion_captured_at = None;
        self.copy_changed = false;
        Ok(())
    }

    pub async fn revoke(&mut self, app: &tauri::AppHandle) -> Result<(), FocusExecutionError> {
        use ganbaru_mobile_notifications::MobileNotificationsExt;
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            app.mobile_notifications().cancel_focus_notification()
        })
        .await
        .map_err(|error| format!("Revoke native Focus phase: {error}"))??;
        self.published = None;
        self.last_attempted = None;
        self.pending_completion = None;
        self.completion_captured_at = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

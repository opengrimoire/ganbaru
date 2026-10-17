//! Desktop surfaces follow accepted native state, independently of WebView execution.

#[cfg(test)]
mod tests;

use chrono::{DateTime, Datelike, NaiveDate, Weekday};
use ganbaru_pomodoro::{FocusExecutionError, FocusExecutionSnapshot, FocusMode, FocusPhase};
use sqlx::SqlitePool;
use tauri::Manager;

use crate::pomodoro::overlay::PomodoroOverlayState;
use crate::sound_effects::{AppSound, AppSoundState};

const MAX_PREFERENCES_BYTES: usize = 4 * 1024 * 1024;
const IDLE_ALERT_INTERVAL_MS: i64 = 10_000;
const FOCUS_WARNING_INTERVAL_MS: i64 = 60_000;
const PAUSED_REMINDER_INTERVAL_MS: i64 = 60_000;
const MAX_NOTIFICATION_TEXT_UNITS: usize = 160;

/// Localization supplies presentation text, never phase clocks or execution intent.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DesktopNotificationCopy {
    ending_warning_title: String,
    extend_focus_label: String,
    paused_reminder_title: String,
    paused_reminder_body: String,
    resume_focus_label: String,
    dismiss_prompts_label: String,
}

impl DesktopNotificationCopy {
    pub fn validate(&self) -> Result<(), FocusExecutionError> {
        for text in [
            &self.ending_warning_title,
            &self.extend_focus_label,
            &self.paused_reminder_title,
            &self.paused_reminder_body,
            &self.resume_focus_label,
            &self.dismiss_prompts_label,
        ] {
            if text.trim().is_empty() || text.encode_utf16().count() > MAX_NOTIFICATION_TEXT_UNITS {
                return Err(super::error(
                    ganbaru_pomodoro::FocusErrorCode::InvalidIntent,
                    "Focus notification text must contain 1 to 160 UTF-16 units",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Preferences {
    repeat_seconds: i64,
    warning_seconds: i64,
    esc_presses: Option<u32>,
    extension_limit: Option<u32>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            repeat_seconds: 10,
            warning_seconds: 10,
            esc_presses: Some(10),
            extension_limit: Some(3),
        }
    }
}

impl Preferences {
    fn parse(root: &serde_json::Value) -> Self {
        let defaults = Self::default();
        let branch = &root["preferences"];
        let interval = |key: &str, fallback| {
            branch[key]
                .as_i64()
                .filter(|value| matches!(value, 0 | 10 | 15 | 30 | 60))
                .unwrap_or(fallback)
        };
        let option = |key: &str, allowed: &[u32], fallback| match branch.get(key) {
            Some(serde_json::Value::Null) => None,
            Some(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| allowed.contains(value))
                .or(fallback),
            None => fallback,
        };
        Self {
            repeat_seconds: interval("focusBreakFinishedRepeatSeconds", defaults.repeat_seconds),
            warning_seconds: interval("focusBreakEndWarningSeconds", defaults.warning_seconds),
            esc_presses: option(
                "focusBreakEndEscPresses",
                &[1, 3, 10, 20, 50],
                defaults.esc_presses,
            ),
            extension_limit: option(
                "focusBreakExtensionLimit",
                &[1, 3, 5, 10, 15],
                defaults.extension_limit,
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Identity {
    generation: u64,
    run: Option<String>,
    segment: Option<String>,
    mode: FocusMode,
}

#[derive(Clone, Debug, PartialEq)]
enum Surface {
    Close,
    Break(i64),
    ReturnWait,
    Idle(u32),
    IdleFailed,
    Completion,
}

#[derive(Clone, Default)]
struct PresentationState {
    identity: Option<Identity>,
    warned_deadline: Option<i64>,
    next_alert_ms: Option<i64>,
    next_warning_ms: Option<i64>,
}

struct Plan {
    surface: Option<Surface>,
    sound: Option<AppSound>,
    notification: Option<DesktopAlert>,
    next: PresentationState,
}

#[derive(Debug, PartialEq)]
enum DesktopAlert {
    Ending {
        remaining_ms: i64,
        allow_extension: bool,
    },
    Paused,
}

impl PresentationState {
    fn plan(
        &self,
        generation: u64,
        snapshot: &FocusExecutionSnapshot,
        now: i64,
        preferences: &Preferences,
    ) -> Plan {
        let identity = Identity {
            generation,
            run: snapshot.run.as_ref().map(|run| run.id.clone()),
            segment: snapshot.segment.as_ref().map(|segment| segment.id.clone()),
            mode: snapshot.mode,
        };
        let changed = self.identity.as_ref() != Some(&identity);
        let break_phase = snapshot
            .segment
            .as_ref()
            .is_some_and(|segment| segment.phase != FocusPhase::Focus);
        let mut next = self.clone();
        let mut surface = None;
        let mut sound = None;
        let mut notification = None;
        if changed {
            next.identity = Some(identity);
            next.next_alert_ms = None;
            next.warned_deadline = None;
            match snapshot.mode {
                FocusMode::Running if break_phase => {
                    surface = snapshot.phase_deadline_ms.map(Surface::Break);
                    sound = Some(AppSound::BreakStart);
                }
                FocusMode::IdlePause => {
                    let seconds = snapshot
                        .idle_detected_at_ms
                        .unwrap_or(now)
                        .saturating_sub(snapshot.idle_started_at_ms.unwrap_or(now))
                        .max(0)
                        / 1000;
                    surface = Some(Surface::Idle(u32::try_from(seconds).unwrap_or(u32::MAX)));
                    sound = Some(AppSound::IdleAlert);
                    next.next_alert_ms = Some(now.saturating_add(IDLE_ALERT_INTERVAL_MS));
                }
                FocusMode::IdleFailed => {
                    surface = Some(Surface::IdleFailed);
                    sound = Some(AppSound::FocusSessionFailedLongIdle);
                }
                FocusMode::ReturnWait => {
                    surface = Some(Surface::ReturnWait);
                    sound = Some(AppSound::BreakFinished);
                    next.next_alert_ms = (preferences.repeat_seconds > 0)
                        .then(|| now.saturating_add(preferences.repeat_seconds * 1000));
                }
                FocusMode::ManualPause if !snapshot.paused_prompts_dismissed => {
                    next.next_alert_ms = Some(now.saturating_add(PAUSED_REMINDER_INTERVAL_MS));
                    if self.identity.is_some() {
                        surface = Some(Surface::Close);
                    }
                }
                FocusMode::Expired
                    if self.identity.as_ref().is_some_and(|previous| {
                        previous.generation == generation
                            && previous.run == snapshot.run.as_ref().map(|run| run.id.clone())
                            && !matches!(previous.mode, FocusMode::Stopped | FocusMode::Expired)
                    }) =>
                {
                    surface = Some(Surface::Completion);
                }
                _ => {
                    if self.identity.is_some() {
                        surface = Some(Surface::Close);
                    }
                }
            }
        }
        if snapshot.mode == FocusMode::Running
            && !break_phase
            && let Some(deadline) = snapshot.phase_deadline_ms
            && now >= deadline.saturating_sub(FOCUS_WARNING_INTERVAL_MS)
            && now < deadline
            && next.warned_deadline != Some(deadline)
        {
            next.warned_deadline = Some(deadline);
            notification = Some(DesktopAlert::Ending {
                remaining_ms: deadline.saturating_sub(now),
                allow_extension: !snapshot.focus_extension_used,
            });
            sound = Some(AppSound::FocusEndingWarning);
        }
        if snapshot.mode == FocusMode::ManualPause && snapshot.paused_prompts_dismissed {
            next.next_alert_ms = None;
        }
        if snapshot.mode == FocusMode::Running
            && break_phase
            && preferences.warning_seconds > 0
            && let Some(deadline) = snapshot.phase_deadline_ms
            && now >= deadline.saturating_sub(preferences.warning_seconds * 1000)
            && now < deadline
            && next.warned_deadline != Some(deadline)
        {
            next.warned_deadline = Some(deadline);
            sound = Some(AppSound::BreakFinished);
        }
        if next.next_alert_ms.is_some_and(|target| now >= target) {
            let interval = match snapshot.mode {
                FocusMode::IdlePause => IDLE_ALERT_INTERVAL_MS,
                FocusMode::ReturnWait => preferences.repeat_seconds * 1000,
                FocusMode::ManualPause if !snapshot.paused_prompts_dismissed => {
                    PAUSED_REMINDER_INTERVAL_MS
                }
                _ => 0,
            };
            sound = match snapshot.mode {
                FocusMode::IdlePause => Some(AppSound::IdleAlert),
                FocusMode::ReturnWait if interval > 0 => Some(AppSound::BreakFinished),
                FocusMode::ManualPause if interval > 0 => {
                    notification = Some(DesktopAlert::Paused);
                    Some(AppSound::EventNotification)
                }
                _ => None,
            };
            next.next_alert_ms = (interval > 0).then(|| now.saturating_add(interval));
        }
        let warning_interval = if break_phase {
            preferences.warning_seconds * 1000
        } else {
            FOCUS_WARNING_INTERVAL_MS
        };
        next.next_warning_ms = if snapshot.mode == FocusMode::Running && warning_interval > 0 {
            snapshot
                .phase_deadline_ms
                .filter(|deadline| next.warned_deadline != Some(*deadline))
                .map(|deadline| deadline.saturating_sub(warning_interval))
                .filter(|warning| *warning > now)
        } else {
            None
        };
        Plan {
            surface,
            sound,
            notification,
            next,
        }
    }
}

#[derive(Default)]
pub(super) struct DesktopPresentation {
    state: PresentationState,
    preferences: Preferences,
    preferences_generation: Option<u64>,
    /// Set when a vault config patch changed the preferences branch.
    preferences_stale: bool,
    notification_copy: Option<DesktopNotificationCopy>,
}

impl DesktopPresentation {
    pub fn configure_copy(
        &mut self,
        copy: DesktopNotificationCopy,
    ) -> Result<(), FocusExecutionError> {
        copy.validate()?;
        self.notification_copy = Some(copy);
        Ok(())
    }
    pub async fn apply(
        &mut self,
        app: &tauri::AppHandle,
        generation: u64,
        snapshot: &FocusExecutionSnapshot,
        now: i64,
        pool: Option<&SqlitePool>,
    ) -> Result<(), FocusExecutionError> {
        if self.preferences_generation != Some(generation) || self.preferences_stale {
            let app = app.clone();
            self.preferences = tauri::async_runtime::spawn_blocking(move || {
                let raw = crate::vault::read_active_config_bounded(&app, MAX_PREFERENCES_BYTES)?;
                let value = serde_json::from_str(&raw)
                    .map_err(|error| format!("Parse native Focus preferences: {error}"))?;
                Ok::<_, String>(Preferences::parse(&value))
            })
            .await
            .map_err(|error| format!("Read native Focus preferences: {error}"))??;
            self.preferences_generation = Some(generation);
            self.preferences_stale = false;
        }
        let plan = self
            .state
            .plan(generation, snapshot, now, &self.preferences);
        if plan.surface.is_none() && plan.sound.is_none() && plan.notification.is_none() {
            self.state = plan.next;
            return Ok(());
        }
        let completion = if plan.surface == Some(Surface::Completion) {
            Some(
                completion_kind(
                    pool.ok_or_else(|| {
                        "Focus completion has no canonical Calendar pool".to_owned()
                    })?,
                    snapshot,
                )
                .await?,
            )
        } else {
            None
        };
        let app = app.clone();
        let revision = snapshot.revision;
        let preferences = self.preferences.clone();
        let surface = plan.surface;
        let sound = plan.sound;
        let notification = plan.notification;
        let notification_copy = if notification.is_some() {
            Some(
                self.notification_copy
                    .clone()
                    .ok_or_else(|| "Native Focus notification language is not ready".to_owned())?,
            )
        } else {
            None
        };
        let notification_context = if notification.is_some() {
            let context = super::capture_native_context(&app)?;
            if !context.matches_snapshot(generation, snapshot) {
                return Err("Native Focus notification context changed"
                    .to_owned()
                    .into());
            }
            Some(context)
        } else {
            None
        };
        let scope = (generation, revision);
        let completion_sound = completion.map(|kind| match kind {
            "day" => AppSound::PomodoroDayComplete,
            "workweek" => AppSound::PomodoroWorkweekComplete,
            _ => AppSound::EventFinished,
        });
        let completion_app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            if !super::presentation_is_current(&app, generation, revision) {
                return Err("Native Focus presentation was superseded or expired".to_owned());
            }
            if let Some(notification) = notification {
                let copy =
                    notification_copy.ok_or("Native Focus notification language is missing")?;
                let context =
                    notification_context.ok_or("Native Focus notification context is missing")?;
                let alert = match notification {
                    DesktopAlert::Ending {
                        remaining_ms,
                        allow_extension,
                    } => crate::notifications::desktop::NativeFocusAlert::Ending {
                        title: copy.ending_warning_title,
                        extend_label: allow_extension.then_some(copy.extend_focus_label),
                        remaining_ms,
                    },
                    DesktopAlert::Paused => {
                        crate::notifications::desktop::NativeFocusAlert::Paused {
                            title: copy.paused_reminder_title,
                            body: copy.paused_reminder_body,
                            resume_label: copy.resume_focus_label,
                            dismiss_label: copy.dismiss_prompts_label,
                        }
                    }
                };
                crate::notifications::desktop::show_native_focus_alert(
                    app.clone(),
                    context,
                    alert,
                )?;
            }
            match surface {
                Some(Surface::Close) => crate::pomodoro::overlay::close_pomodoro_overlay(
                    app.clone(),
                    app.state::<PomodoroOverlayState>(),
                ),
                Some(Surface::Break(deadline)) => crate::pomodoro::overlay::show_break_overlay(
                    app.clone(),
                    deadline.max(0) as u64,
                    preferences.esc_presses,
                    preferences.extension_limit,
                    scope,
                )?,
                Some(Surface::Idle(seconds)) => {
                    crate::pomodoro::overlay::show_idle_overlay(app.clone(), seconds, scope)?;
                }
                Some(Surface::IdleFailed) => {
                    crate::pomodoro::overlay::show_idle_overlay(app.clone(), 0, scope)?;
                    crate::pomodoro::overlay::set_pomodoro_overlay_state(
                        app.clone(),
                        app.state::<PomodoroOverlayState>(),
                        "idle_failed".into(),
                        scope,
                    )?;
                }
                Some(Surface::ReturnWait) => {
                    crate::pomodoro::overlay::show_break_overlay(
                        app.clone(),
                        now.max(0) as u64,
                        preferences.esc_presses,
                        preferences.extension_limit,
                        scope,
                    )?;
                    crate::pomodoro::overlay::set_pomodoro_overlay_state(
                        app.clone(),
                        app.state::<PomodoroOverlayState>(),
                        "break_finished".into(),
                        scope,
                    )?;
                }
                Some(Surface::Completion) => {
                    let kind =
                        completion.ok_or("Native Focus completion classification is missing")?;
                    crate::pomodoro::overlay::show_pomodoro_completion_overlay(
                        app.clone(),
                        kind.into(),
                        scope,
                    )?;
                }
                None => {}
            }
            if let Some(sound) = sound {
                if !super::presentation_is_current(&app, generation, revision) {
                    return Err("Native Focus sound was superseded or expired".to_owned());
                }
                app.state::<AppSoundState>().play(sound);
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|error| format!("Present accepted native Focus: {error}"))??;
        if let Some(sound) = completion_sound {
            crate::music::session::runtime::play_focus_completion(&completion_app, scope, sound)
                .await?;
        }
        self.state = plan.next;
        Ok(())
    }

    pub fn next_deadline(&self) -> Option<i64> {
        self.state
            .next_alert_ms
            .into_iter()
            .chain(self.state.next_warning_ms)
            .min()
    }

    pub fn invalidate_preferences(&mut self) {
        self.preferences_stale = true;
    }

    pub async fn revoke(&mut self, app: &tauri::AppHandle) -> Result<(), FocusExecutionError> {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            crate::pomodoro::overlay::close_pomodoro_overlay(
                app.clone(),
                app.state::<PomodoroOverlayState>(),
            );
        })
        .await
        .map_err(|error| format!("Revoke native Focus overlays: {error}"))?;
        *self = Self::default();
        Ok(())
    }
}

async fn completion_kind(
    pool: &SqlitePool,
    snapshot: &FocusExecutionSnapshot,
) -> Result<&'static str, FocusExecutionError> {
    let run = snapshot
        .run
        .as_ref()
        .ok_or_else(|| "Focus completion has no accepted run".to_owned())?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("Read Focus completion Calendar: {error}"))?;
    let calendar =
        super::calendar::resolve(&mut tx, run.planned_end_ms.saturating_sub(1), None).await?;
    let kind = classify_completion(
        &run.event_date,
        &run.occurrence_id,
        run.planned_end_ms,
        &calendar.planned_blocks,
    )?;
    tx.commit()
        .await
        .map_err(|error| format!("Finish Focus completion Calendar read: {error}"))?;
    Ok(kind)
}

fn classify_completion(
    event_date: &str,
    occurrence_id: &str,
    ended_at_ms: i64,
    blocks: &[ganbaru_calendar::reads::focus_context::FocusPlannedBlock],
) -> Result<&'static str, FocusExecutionError> {
    let later = blocks
        .iter()
        .filter(|block| block.event_date == event_date && block.event_id != occurrence_id)
        .map(|block| {
            DateTime::parse_from_rfc3339(&block.planned_start).map(|start| start.timestamp_millis())
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("Read Focus completion start: {error}"))?
        .into_iter()
        .any(|start| start >= ended_at_ms);
    if later {
        return Ok("event");
    }
    let date = NaiveDate::parse_from_str(event_date, "%Y-%m-%d")
        .map_err(|error| format!("Read Focus completion date: {error}"))?;
    Ok(if date.weekday() == Weekday::Fri {
        "workweek"
    } else {
        "day"
    })
}

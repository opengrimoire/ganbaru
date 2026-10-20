//! Desktop native notifications for Calendar, Notes, Projects tasks, benchmarks, Focus, and the distraction blocker.

#[cfg(target_os = "linux")]
use notify_rust::Hint;
use notify_rust::Notification;
use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::sound_effects::{AppSound, AppSoundState};

#[derive(Clone, Serialize)]
struct NotesNotificationOpenPayload {
    page_id: String,
    block_id: Option<String>,
}

#[derive(Clone, Serialize)]
struct ProjectTaskNotificationOpenPayload {
    project_id: String,
    task_id: String,
}

fn apply_linux_notification_hints(
    notification: &mut Notification,
    category: Option<&str>,
    desktop_entry: bool,
    transient: bool,
) {
    #[cfg(target_os = "linux")]
    {
        if let Some(category) = category {
            notification.hint(Hint::Category(category.to_string()));
        }
        if desktop_entry {
            notification.hint(Hint::DesktopEntry("ganbaru".to_string()));
        }
        if transient {
            notification.hint(Hint::Transient(true));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = notification;
        let _ = category;
        let _ = desktop_entry;
        let _ = transient;
    }
}

/// Notifications that may listen for an action at once; further ones show without action buttons.
#[cfg(any(target_os = "linux", test))]
const MAX_NOTIFICATION_ACTION_WAITS: usize = 16;
/// How long a shown notification keeps listening for its action.
#[cfg(target_os = "linux")]
const NOTIFICATION_ACTION_WAIT: std::time::Duration = std::time::Duration::from_secs(120);
const MAX_NOTIFICATION_SUMMARY_CHARS: usize = 160;
const MAX_NOTIFICATION_BODY_CHARS: usize = 1_000;
const NOTIFICATION_TRUNCATION_MARK: char = '\u{2026}';

#[cfg(target_os = "linux")]
static NOTIFICATION_ACTION_WAITS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_NOTIFICATION_ACTION_WAITS))
    });

/// Takes one action wait slot, or strips the action buttons when every slot is taken so the
/// notification never offers an action nothing listens for.
#[cfg(any(target_os = "linux", test))]
fn reserve_action_wait(
    slots: &std::sync::Arc<tokio::sync::Semaphore>,
    notification: &mut Notification,
) -> Option<tokio::sync::OwnedSemaphorePermit> {
    let slot = slots.clone().try_acquire_owned().ok();
    if slot.is_none() {
        notification.actions.clear();
    }
    slot
}

/// Shows a notification off the calling thread and runs `on_action` for the action the user picks.
///
/// On Linux each listening notification holds one of `MAX_NOTIFICATION_ACTION_WAITS` slots for at
/// most `NOTIFICATION_ACTION_WAIT`, so repeated requests cannot pin threads or connections; once the
/// slots are taken, notifications still show but without action buttons.
fn spawn_notification<F>(notification: Notification, error_context: &'static str, on_action: F)
where
    F: FnOnce(&str) + Send + 'static,
{
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn(async move {
        let mut notification = notification;
        let action_slot = reserve_action_wait(&NOTIFICATION_ACTION_WAITS, &mut notification);
        let handle = match notification.show_async().await {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("{error_context}: {error}");
                return;
            }
        };
        let Some(_action_slot) = action_slot else {
            return;
        };
        let wait = handle.wait_for_action_async(|response| {
            if let notify_rust::ActionResponse::Custom(action) = response {
                on_action(action);
            }
        });
        // An elapsed wait only stops listening; the notification server still owns the notification.
        let _elapsed = tokio::time::timeout(NOTIFICATION_ACTION_WAIT, wait).await;
    });

    #[cfg(not(target_os = "linux"))]
    {
        let _ = on_action;
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(error) = notification.show() {
                eprintln!("{error_context}: {error}");
            }
        });
    }
}

fn focus_main_window(app: tauri::AppHandle) {
    let app_for_lookup = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = app_for_lookup.get_webview_window("main") {
            if let Err(e) = window.show() {
                eprintln!("Failed to show main window from notification: {e}");
            }
            if let Err(e) = window.unminimize() {
                eprintln!("Failed to unminimize main window from notification: {e}");
            }
            if let Err(e) = window.set_always_on_top(true) {
                eprintln!("Failed to raise main window from notification: {e}");
            }
            crate::app_windows::announce_window_raised(&app_for_lookup, window.label());
            let reset_app = app_for_lookup.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let app_for_reset = reset_app.clone();
                let _ = reset_app.run_on_main_thread(move || {
                    if let Some(window) = app_for_reset.get_webview_window("main") {
                        if let Err(e) = window.set_always_on_top(false) {
                            eprintln!("Failed to restore main window stacking mode: {e}");
                        }
                    }
                });
            });
            if let Err(e) = window.set_focus() {
                eprintln!("Failed to focus main window from notification: {e}");
            }
        }
    });
}

fn escape_notification_markup(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Shortens `value` to at most `max_chars` characters, marking any cut with an ellipsis.
fn bounded_notification_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut bounded = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    bounded.push(NOTIFICATION_TRUNCATION_MARK);
    bounded
}

fn notification_summary(value: &str, fallback: &str) -> String {
    let summary = value.lines().next().unwrap_or("").trim();
    if summary.is_empty() {
        fallback.to_string()
    } else {
        escape_notification_markup(&bounded_notification_text(
            summary,
            MAX_NOTIFICATION_SUMMARY_CHARS,
        ))
    }
}

fn notification_body(value: &str) -> String {
    escape_notification_markup(&bounded_notification_text(
        value,
        MAX_NOTIFICATION_BODY_CHARS,
    ))
}

#[tauri::command]
pub fn show_event_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    open_calendar: Option<bool>,
    play_sound: Option<bool>,
    app_sounds: State<'_, AppSoundState>,
) {
    if play_sound.unwrap_or(true) {
        app_sounds.play(AppSound::EventNotification);
    }
    let opens_calendar = open_calendar.unwrap_or(false);
    let action_label = if opens_calendar {
        "Open calendar"
    } else {
        "Open Ganbaru"
    };
    let mut notification = Notification::new();
    notification
        .appname("Ganbaru")
        .summary(&notification_summary(&title, "Calendar event"))
        .body(&notification_body(&body))
        .action("open", action_label)
        .timeout(15_000);
    apply_linux_notification_hints(&mut notification, Some("calendar"), true, true);
    spawn_notification(
        notification,
        "Failed to show event notification",
        move |action| {
            if action == "open" || action == "default" {
                if opens_calendar {
                    let _ = app.emit("calendar-notification-open", ());
                }
                focus_main_window(app);
            }
        },
    );
}

#[tauri::command]
pub fn show_notes_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    page_id: String,
    block_id: Option<String>,
    play_sound: Option<bool>,
    app_sounds: State<'_, AppSoundState>,
) {
    if play_sound.unwrap_or(true) {
        app_sounds.play(AppSound::EventNotification);
    }
    let mut notification = Notification::new();
    notification
        .appname("Ganbaru")
        .summary(&notification_summary(&title, "Note reminder"))
        .body(&notification_body(&body))
        .action("open", "Open note")
        .timeout(15_000);
    apply_linux_notification_hints(&mut notification, Some("reminder"), true, true);
    spawn_notification(
        notification,
        "Failed to show notes notification",
        move |action| {
            if action == "open" || action == "default" {
                let _ = app.emit(
                    "notes-notification-open",
                    NotesNotificationOpenPayload { page_id, block_id },
                );
                focus_main_window(app);
            }
        },
    );
}

#[tauri::command]
pub fn show_project_task_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    project_id: String,
    task_id: String,
    play_sound: Option<bool>,
    app_sounds: State<'_, AppSoundState>,
) {
    if play_sound.unwrap_or(true) {
        app_sounds.play(AppSound::EventNotification);
    }
    let mut notification = Notification::new();
    notification
        .appname("Ganbaru")
        .summary(&notification_summary(&title, "Task reminder"))
        .body(&notification_body(&body))
        .action("open", "Open task")
        .timeout(15_000);
    apply_linux_notification_hints(&mut notification, Some("reminder"), true, true);
    spawn_notification(
        notification,
        "Failed to show project task notification",
        move |action| {
            if action == "open" || action == "default" {
                let _ = app.emit(
                    "project-task-notification-open",
                    ProjectTaskNotificationOpenPayload {
                        project_id,
                        task_id,
                    },
                );
                focus_main_window(app);
            }
        },
    );
}

#[tauri::command]
pub fn show_benchmark_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    let mut notification = Notification::new();
    notification
        .summary(&notification_summary(&title, "Benchmark finished"))
        .body(&notification_body(&body))
        .action("show_summary", "Show summary")
        .timeout(10_000)
        .id(9002);
    apply_linux_notification_hints(&mut notification, None, false, true);
    spawn_notification(
        notification,
        "Failed to show benchmark notification",
        move |action| {
            if action == "show_summary" || action == "default" {
                focus_main_window(app);
            }
        },
    );
}

/// Longest app or limit name a distraction notification quotes.
const MAX_DISTRACTION_NAME_CHARS: usize = 80;

/// First line of a distraction app or limit name, bounded, or `fallback` when it is blank.
fn distraction_notification_name(value: &str, fallback: &str) -> String {
    let name = value.lines().next().unwrap_or("").trim();
    if name.is_empty() {
        fallback.to_string()
    } else {
        bounded_notification_text(name, MAX_DISTRACTION_NAME_CHARS)
    }
}

pub(crate) fn show_distractions_desktop_block_notification(
    app: tauri::AppHandle,
    app_name: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    let app_name = distraction_notification_name(&app_name, "The blocked app");
    let body = escape_notification_markup(&format!(
        "{app_name} was closed because it is blocked by your desktop rules. Change this in Settings > Distractions > Desktop apps (or click this notification)"
    ));
    let mut notification = Notification::new();
    notification
        .appname("Ganbaru")
        .summary("App closed by Ganbaru")
        .body(&body)
        .action("default", "Open desktop apps")
        .timeout(10_000);
    apply_linux_notification_hints(&mut notification, Some("device"), true, true);
    spawn_notification(
        notification,
        "Failed to show distraction desktop block notification",
        move |action| {
            if action == "default" {
                let _ = app.emit("distractions-open-desktop-settings", ());
                focus_main_window(app);
            }
        },
    );
}

pub(crate) fn show_distractions_desktop_limit_notification(
    app: tauri::AppHandle,
    app_name: String,
    limit_name: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    let app_name = distraction_notification_name(&app_name, "The app");
    let limit_name = distraction_notification_name(&limit_name, "a usage limit");
    let body = escape_notification_markup(&format!(
        "{app_name} was closed because {limit_name} reached its limit. Change this in Settings > Distractions > Limits (or click this notification)"
    ));
    let mut notification = Notification::new();
    notification
        .appname("Ganbaru")
        .summary("Usage limit reached")
        .body(&body)
        .action("default", "Open limits")
        .timeout(10_000);
    apply_linux_notification_hints(&mut notification, Some("device"), true, true);
    spawn_notification(
        notification,
        "Failed to show distraction desktop limit notification",
        move |action| {
            if action == "default" {
                let _ = app.emit("distractions-open-limits-settings", ());
                focus_main_window(app);
            }
        },
    );
}

/// Desktop alerts are emitted only from an accepted native Focus presentation.
pub(crate) enum NativeFocusAlert {
    Ending {
        title: String,
        extend_label: Option<String>,
        remaining_ms: i64,
    },
    Paused {
        title: String,
        body: String,
        resume_label: String,
        dismiss_label: String,
    },
}

#[cfg(target_os = "linux")]
static FOCUS_ALERT_ACTIONS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));
#[cfg(any(target_os = "linux", test))]
const FOCUS_EXTENSION_SECONDS: i64 = 180;
const PAUSED_ALERT_TIMEOUT_MS: i64 = 15_000;

#[derive(Clone, Copy)]
#[cfg(any(target_os = "linux", test))]
enum NativeFocusAlertActions {
    Ending { allow_extension: bool },
    Paused,
}

#[cfg(any(target_os = "linux", test))]
impl NativeFocusAlertActions {
    fn intent(self, action: &str) -> Option<ganbaru_pomodoro::FocusIntent> {
        match (self, action) {
            (
                Self::Ending {
                    allow_extension: true,
                },
                "add_time",
            ) => Some(ganbaru_pomodoro::FocusIntent::ExtendFocus {
                seconds: FOCUS_EXTENSION_SECONDS,
            }),
            (Self::Paused, "resume") => Some(ganbaru_pomodoro::FocusIntent::Resume),
            (Self::Paused, "stop_asking") => {
                Some(ganbaru_pomodoro::FocusIntent::DismissPausedPrompts)
            }
            _ => None,
        }
    }
}

/// Show localized text and bind supported actions to the displayed native run and phase.
pub(crate) fn show_native_focus_alert(
    app: tauri::AppHandle,
    context: crate::pomodoro::native_runtime::FocusNativeContext,
    alert: NativeFocusAlert,
) -> Result<(), String> {
    if !crate::pomodoro::native_runtime::presentation_is_current(
        &app,
        context.vault_generation,
        context.revision,
    ) {
        return Err("Native Focus alert was superseded or expired".into());
    }
    #[cfg(target_os = "linux")]
    let action_slot = FOCUS_ALERT_ACTIONS
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Native Focus notification actions are still occupied".to_string())?;
    #[cfg(target_os = "linux")]
    let actions = match &alert {
        NativeFocusAlert::Ending { extend_label, .. } => NativeFocusAlertActions::Ending {
            allow_extension: extend_label.is_some(),
        },
        NativeFocusAlert::Paused { .. } => NativeFocusAlertActions::Paused,
    };
    let mut notification = Notification::new();
    notification.appname("Ganbaru");
    let timeout_ms = match alert {
        NativeFocusAlert::Ending {
            title,
            extend_label,
            remaining_ms,
        } => {
            notification.summary(&title).id(9001);
            #[cfg(target_os = "linux")]
            if let Some(label) = extend_label {
                notification.action("add_time", &label);
            }
            #[cfg(not(target_os = "linux"))]
            let _ = extend_label;
            remaining_ms.clamp(1, i32::MAX as i64)
        }
        NativeFocusAlert::Paused {
            title,
            body,
            resume_label,
            dismiss_label,
        } => {
            notification.summary(&title).body(&body).id(9003);
            #[cfg(target_os = "linux")]
            notification
                .action("resume", &resume_label)
                .action("stop_asking", &dismiss_label);
            #[cfg(not(target_os = "linux"))]
            let _ = (resume_label, dismiss_label);
            PAUSED_ALERT_TIMEOUT_MS
        }
    };
    notification.timeout(timeout_ms as i32);
    apply_linux_notification_hints(&mut notification, Some("reminder"), true, true);
    if !crate::pomodoro::native_runtime::presentation_is_current(
        &app,
        context.vault_generation,
        context.revision,
    ) {
        return Err("Native Focus alert context changed before publication".into());
    }
    let handle = notification
        .show()
        .map_err(|error| format!("Show native Focus alert: {error}"))?;
    #[cfg(target_os = "linux")]
    tauri::async_runtime::spawn(async move {
        let _slot = action_slot;
        let wait = handle.wait_for_action_async(|response| {
            let notify_rust::ActionResponse::Custom(action) = response else {
                return;
            };
            if *action == "default" {
                if crate::pomodoro::native_runtime::presentation_is_current(
                    &app,
                    context.vault_generation,
                    context.revision,
                ) {
                    focus_main_window(app.clone());
                }
            }
            let intent = actions.intent(action);
            if let Some(intent) = intent
                && let Err(error) = crate::pomodoro::native_runtime::native_control_in_context(
                    &app,
                    intent,
                    context.clone(),
                )
            {
                eprintln!("Native Focus notification action: {error}");
            }
        });
        if tokio::time::timeout(std::time::Duration::from_millis(timeout_ms as u64), wait)
            .await
            .is_err()
        {
            handle.close_async().await;
        }
    });
    #[cfg(not(target_os = "linux"))]
    let _ = handle;
    Ok(())
}

#[cfg(test)]
mod notification_bounds_tests {
    use super::*;

    #[test]
    fn notification_text_is_bounded_before_markup_is_escaped() {
        let long_title = format!(
            "{}\nsecond line",
            "<".repeat(MAX_NOTIFICATION_SUMMARY_CHARS * 4)
        );
        let summary = notification_summary(&long_title, "Calendar event");
        assert_eq!(
            summary,
            format!(
                "{}{NOTIFICATION_TRUNCATION_MARK}",
                "&lt;".repeat(MAX_NOTIFICATION_SUMMARY_CHARS - 1)
            ),
            "only the first line is kept and no entity is split"
        );
        assert_eq!(
            notification_summary(" \n title", "Task reminder"),
            "Task reminder"
        );

        let body = notification_body(&"\u{e9}".repeat(MAX_NOTIFICATION_BODY_CHARS + 1));
        assert_eq!(body.chars().count(), MAX_NOTIFICATION_BODY_CHARS);
        assert!(body.ends_with(NOTIFICATION_TRUNCATION_MARK));
        assert_eq!(notification_body("a & b"), "a &amp; b");
        assert_eq!(
            distraction_notification_name(&"x".repeat(200), "The app")
                .chars()
                .count(),
            MAX_DISTRACTION_NAME_CHARS
        );
        assert_eq!(distraction_notification_name("\n", "The app"), "The app");
    }

    #[test]
    fn notifications_beyond_the_wait_limit_show_without_actions() {
        let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_NOTIFICATION_ACTION_WAITS));
        let mut held = Vec::new();
        for _ in 0..MAX_NOTIFICATION_ACTION_WAITS {
            let mut notification = Notification::new();
            notification.action("open", "Open");
            held.push(reserve_action_wait(&slots, &mut notification).expect("slot should be free"));
            assert_eq!(notification.actions.len(), 2);
        }

        let mut overflow = Notification::new();
        overflow.action("open", "Open");
        assert!(reserve_action_wait(&slots, &mut overflow).is_none());
        assert!(overflow.actions.is_empty());

        held.pop();
        let mut later = Notification::new();
        later.action("open", "Open");
        assert!(reserve_action_wait(&slots, &mut later).is_some());
        assert_eq!(later.actions.len(), 2);
    }
}

#[cfg(test)]
mod focus_alert_tests {
    use super::*;

    #[test]
    fn alerts_admit_only_actions_offered_for_the_displayed_mode_and_extension_state() {
        let warning = NativeFocusAlertActions::Ending {
            allow_extension: true,
        };
        assert!(matches!(
            warning.intent("add_time"),
            Some(ganbaru_pomodoro::FocusIntent::ExtendFocus {
                seconds: FOCUS_EXTENSION_SECONDS
            })
        ));
        assert!(warning.intent("resume").is_none());
        assert!(warning.intent("stop_asking").is_none());
        assert!(
            NativeFocusAlertActions::Ending {
                allow_extension: false
            }
            .intent("add_time")
            .is_none()
        );
        assert!(matches!(
            NativeFocusAlertActions::Paused.intent("resume"),
            Some(ganbaru_pomodoro::FocusIntent::Resume)
        ));
        assert!(matches!(
            NativeFocusAlertActions::Paused.intent("stop_asking"),
            Some(ganbaru_pomodoro::FocusIntent::DismissPausedPrompts)
        ));
        assert!(NativeFocusAlertActions::Paused.intent("add_time").is_none());
        for action in ["default", "unknown", "stop", ""] {
            assert!(NativeFocusAlertActions::Paused.intent(action).is_none());
            assert!(warning.intent(action).is_none());
        }
    }
}

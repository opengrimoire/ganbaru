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
            notification.hint(Hint::DesktopEntry("ganbaru-ai".to_string()));
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

fn show_notification_with_linux_action<F>(
    notification: &Notification,
    error_context: &str,
    on_action: F,
) where
    F: FnOnce(&str),
{
    #[cfg(target_os = "linux")]
    {
        match notification.show() {
            Ok(handle) => {
                handle.wait_for_action(on_action);
            }
            Err(e) => {
                eprintln!("{error_context}: {e}");
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = on_action;
        if let Err(e) = notification.show() {
            eprintln!("{error_context}: {e}");
        }
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

fn notification_summary(value: &str) -> String {
    let summary = value.lines().next().unwrap_or("").trim();
    if summary.is_empty() {
        "Calendar event".to_string()
    } else {
        escape_notification_markup(summary)
    }
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
    std::thread::spawn(move || {
        let summary = notification_summary(&title);
        let body = escape_notification_markup(&body);
        let opens_calendar = open_calendar.unwrap_or(false);
        let action_label = if opens_calendar {
            "Open calendar"
        } else {
            "Open Ganbaru AI"
        };
        let mut notification = Notification::new();
        notification
            .appname("Ganbaru AI")
            .summary(&summary)
            .body(&body)
            .action("open", action_label)
            .timeout(15_000);
        apply_linux_notification_hints(&mut notification, Some("calendar"), true, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show event notification",
            |action| {
                if action == "open" || action == "default" {
                    if opens_calendar {
                        let _ = app.emit("calendar-notification-open", ());
                    }
                    focus_main_window(app.clone());
                }
            },
        );
    });
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
    std::thread::spawn(move || {
        let summary = notification_summary(&title);
        let body = escape_notification_markup(&body);
        let mut notification = Notification::new();
        notification
            .appname("Ganbaru AI")
            .summary(&summary)
            .body(&body)
            .action("open", "Open note")
            .timeout(15_000);
        apply_linux_notification_hints(&mut notification, Some("reminder"), true, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show notes notification",
            |action| {
                if action == "open" || action == "default" {
                    let _ = app.emit(
                        "notes-notification-open",
                        NotesNotificationOpenPayload {
                            page_id: page_id.clone(),
                            block_id: block_id.clone(),
                        },
                    );
                    focus_main_window(app.clone());
                }
            },
        );
    });
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
    std::thread::spawn(move || {
        let summary = notification_summary(&title);
        let body = escape_notification_markup(&body);
        let mut notification = Notification::new();
        notification
            .appname("Ganbaru AI")
            .summary(&summary)
            .body(&body)
            .action("open", "Open task")
            .timeout(15_000);
        apply_linux_notification_hints(&mut notification, Some("reminder"), true, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show project task notification",
            |action| {
                if action == "open" || action == "default" {
                    let _ = app.emit(
                        "project-task-notification-open",
                        ProjectTaskNotificationOpenPayload {
                            project_id: project_id.clone(),
                            task_id: task_id.clone(),
                        },
                    );
                    focus_main_window(app.clone());
                }
            },
        );
    });
}

#[tauri::command]
pub fn show_benchmark_notification(
    app: tauri::AppHandle,
    title: String,
    body: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    std::thread::spawn(move || {
        let mut notification = Notification::new();
        notification
            .summary(&title)
            .body(&body)
            .action("show_summary", "Show summary")
            .timeout(10_000)
            .id(9002);
        apply_linux_notification_hints(&mut notification, None, false, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show benchmark notification",
            |action| {
                if action == "show_summary" || action == "default" {
                    focus_main_window(app.clone());
                }
            },
        );
    });
}

#[tauri::command]
pub fn show_distractions_desktop_block_notification(
    app: tauri::AppHandle,
    app_name: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    std::thread::spawn(move || {
        let app_name = app_name
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(80)
            .collect::<String>();
        let app_name = if app_name.is_empty() {
            "The blocked app".to_string()
        } else {
            app_name
        };
        let body = escape_notification_markup(&format!(
            "{app_name} was closed because it is blocked by your desktop rules. Change this in Settings > Distractions > Desktop apps (or click this notification)"
        ));
        let mut notification = Notification::new();
        notification
            .appname("Ganbaru AI")
            .summary("App closed by Ganbaru AI")
            .body(&body)
            .action("default", "Open desktop apps")
            .timeout(10_000);
        apply_linux_notification_hints(&mut notification, Some("device"), true, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show distraction desktop block notification",
            |action| {
                if action == "default" {
                    let _ = app.emit("distractions-open-desktop-settings", ());
                    focus_main_window(app.clone());
                }
            },
        );
    });
}

#[tauri::command]
pub fn show_distractions_desktop_limit_notification(
    app: tauri::AppHandle,
    app_name: String,
    limit_name: String,
    app_sounds: State<'_, AppSoundState>,
) {
    app_sounds.play(AppSound::EventNotification);
    std::thread::spawn(move || {
        let app_name = app_name
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(80)
            .collect::<String>();
        let app_name = if app_name.is_empty() {
            "The app".to_string()
        } else {
            app_name
        };
        let limit_name = limit_name
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(80)
            .collect::<String>();
        let limit_name = if limit_name.is_empty() {
            "a usage limit".to_string()
        } else {
            limit_name
        };
        let body = escape_notification_markup(&format!(
            "{app_name} was closed because {limit_name} reached its limit. Change this in Settings > Distractions > Limits (or click this notification)"
        ));
        let mut notification = Notification::new();
        notification
            .appname("Ganbaru AI")
            .summary("Usage limit reached")
            .body(&body)
            .action("default", "Open limits")
            .timeout(10_000);
        apply_linux_notification_hints(&mut notification, Some("device"), true, true);
        show_notification_with_linux_action(
            &notification,
            "Failed to show distraction desktop limit notification",
            |action| {
                if action == "default" {
                    let _ = app.emit("distractions-open-limits-settings", ());
                    focus_main_window(app.clone());
                }
            },
        );
    });
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
    notification.appname("Ganbaru AI");
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

//! Android notification channel, exact-alarm, and background execution capability commands.

use serde::Serialize;
use tauri::{AppHandle, Runtime};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileExactAlarmStatus {
    api_level: u32,
    required: bool,
    granted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileBackgroundExecutionStatus {
    manufacturer: String,
    autostart_settings_available: bool,
    background_restricted: bool,
    battery_optimization_exempt: bool,
}

/// Report Android background-execution state that can be observed without privileged APIs.
#[tauri::command]
pub fn mobile_notification_background_execution_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<MobileBackgroundExecutionStatus, String> {
    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        let status = app.mobile_notifications().background_execution_status()?;
        Ok(MobileBackgroundExecutionStatus {
            manufacturer: status.manufacturer,
            autostart_settings_available: status.autostart_settings_available,
            background_restricted: status.background_restricted,
            battery_optimization_exempt: status.battery_optimization_exempt,
        })
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("background-execution status is only available on Android".to_string())
    }
}

/// Open Android's app-specific background-execution settings.
#[tauri::command]
pub fn mobile_notification_open_background_execution_settings<R: Runtime>(
    app: AppHandle<R>,
    destination: String,
) -> Result<(), String> {
    if !matches!(destination.as_str(), "autostart" | "battery") {
        return Err("background settings destination must be autostart or battery".to_string());
    }

    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        app.mobile_notifications()
            .open_background_execution_settings(&destination)
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, destination);
        Err("background-execution settings are only available on Android".to_string())
    }
}

/// Ensure Android's Calendar reminder channel follows the device notification sound.
#[tauri::command]
pub fn mobile_notification_ensure_calendar_channel<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    description: String,
) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err("notification channel name must contain 1 to 80 characters".to_string());
    }
    if description.chars().count() > 300 {
        return Err("notification channel description must not exceed 300 characters".to_string());
    }

    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        app.mobile_notifications()
            .ensure_calendar_channel(name.trim(), description.trim())
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, name, description);
        Err("Calendar notification channels are only available on Android".to_string())
    }
}

/// Report whether Android allows exact Calendar reminder alarms.
#[tauri::command]
pub fn mobile_notification_exact_alarm_status<R: Runtime>(
    app: AppHandle<R>,
) -> Result<MobileExactAlarmStatus, String> {
    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        let status = app.mobile_notifications().exact_alarm_status()?;
        Ok(MobileExactAlarmStatus {
            api_level: status.api_level,
            required: status.required,
            granted: status.granted,
        })
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("exact-alarm status is only available on Android".to_string())
    }
}

/// Open Android's app-specific Alarms and reminders access screen.
#[tauri::command]
pub fn mobile_notification_open_exact_alarm_settings<R: Runtime>(
    app: AppHandle<R>,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        app.mobile_notifications().open_exact_alarm_settings()
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("exact-alarm settings are only available on Android".to_string())
    }
}

/// Open Android's app-specific notification settings screen.
#[tauri::command]
pub fn mobile_notification_open_settings<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        use ganbaru_mobile_notifications::MobileNotificationsExt;

        app.mobile_notifications().open_notification_settings()
    }

    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("notification settings are only available on Android".to_string())
    }
}

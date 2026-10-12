use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaruai.mobile.notifications";

#[derive(Debug)]
pub struct MobileNotifications<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for MobileNotifications<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Android exact-alarm capability exposed without widening notification access.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAlarmStatus {
    pub api_level: u32,
    pub required: bool,
    pub granted: bool,
}

/// Android and OEM background-execution state that the app can observe.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundExecutionStatus {
    pub manufacturer: String,
    pub autostart_settings_available: bool,
    pub background_restricted: bool,
    pub battery_optimization_exempt: bool,
}

/// Device-zone facts from Android's timezone database for bounded native policy inputs.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeviceLocalTimeFact {
    pub epoch_ms: i64,
    pub date_key: String,
    pub date_string: String,
    pub hour: u8,
}

/// Android Activity lifecycle evidence delivered directly to the Rust owner.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeFocusLifecycle {
    pub sequence: u64,
    pub foreground: bool,
    pub observed_at_ms: i64,
    pub elapsed_realtime_ms: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CalendarChannelRequest<'a> {
    name: &'a str,
    description: &'a str,
}

pub(crate) fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> tauri::Result<MobileNotifications<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MobileNotificationsPlugin")?;
    Ok(MobileNotifications(handle))
}

impl<R: Runtime> MobileNotifications<R> {
    /// Persist presentation copy separately from accepted execution projections.
    pub fn configure_focus_notification_copy<T: Serialize>(&self, copy: &T) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a, T> {
            copy: &'a T,
            process_nonce: i64,
        }
        self.0
            .run_mobile_plugin(
                "configureFocusNotificationCopy",
                Request {
                    copy,
                    process_nonce: super::authority::process_nonce()?,
                },
            )
            .map_err(|error| format!("Configure Android Focus notification language: {error}"))
    }

    /// Read retained notification language without using a phase as execution evidence.
    pub fn focus_notification_copy<T: serde::de::DeserializeOwned>(
        &self,
    ) -> Result<Option<T>, String> {
        #[derive(Deserialize)]
        struct Response<T> {
            copy: Option<T>,
        }
        self.0
            .run_mobile_plugin::<Response<T>>("focusNotificationCopy", ())
            .map(|response| response.copy)
            .map_err(|error| format!("Read Android Focus notification language: {error}"))
    }

    /// Publish a canonical accepted phase through the private Guardian adapter.
    pub fn publish_focus_notification<T: Serialize>(
        &self,
        state: &T,
        generation: u64,
        revision: i64,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a, T> {
            state: &'a T,
            process_nonce: i64,
            generation: u64,
            revision: i64,
        }
        self.0
            .run_mobile_plugin(
                "updatePomodoroNotification",
                Request {
                    state,
                    process_nonce: super::authority::process_nonce()?,
                    generation,
                    revision,
                },
            )
            .map_err(|error| format!("Publish accepted Android Focus notification: {error}"))
    }

    /// Deliver a committed phase boundary with Guardian's retained notification receipt.
    pub fn complete_focus_notification<T: Serialize>(
        &self,
        state: &T,
        generation: u64,
        revision: i64,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a, T> {
            state: &'a T,
            process_nonce: i64,
            generation: u64,
            revision: i64,
        }
        self.0
            .run_mobile_plugin(
                "completeFocusNotification",
                Request {
                    state,
                    process_nonce: super::authority::process_nonce()?,
                    generation,
                    revision,
                },
            )
            .map_err(|error| format!("Deliver committed Android Focus boundary: {error}"))
    }

    /// Revoke presentation and Guardian phase validity without changing execution history.
    pub fn cancel_focus_notification(&self) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request {
            process_nonce: i64,
        }
        self.0
            .run_mobile_plugin(
                "cancelPomodoroNotification",
                Request {
                    process_nonce: super::authority::process_nonce()?,
                },
            )
            .map_err(|error| format!("Revoke Android Focus notification: {error}"))
    }

    /// Attach a native-only lifecycle callback. It never admits or advances phases.
    pub fn attach_focus_lifecycle(&self, channel: tauri::ipc::Channel) -> Result<(), String> {
        #[derive(Serialize)]
        struct Request {
            channel: tauri::ipc::Channel,
        }
        self.0
            .run_mobile_plugin("attachFocusLifecycle", Request { channel })
            .map_err(|error| format!("Attach Android Focus lifecycle observation: {error}"))
    }

    /// Resolve historical local dates and hours using Android's current device zone.
    /// Values are observation facts, never a frontend-supplied fixed offset.
    pub fn device_local_time_facts(
        &self,
        instants: &[i64],
    ) -> Result<Vec<DeviceLocalTimeFact>, String> {
        const MAX_INSTANTS: usize = 4096;
        if instants.len() > MAX_INSTANTS {
            return Err("Device local time request exceeds its instant limit".to_owned());
        }
        #[derive(Serialize)]
        struct Request<'a> {
            instants: &'a [i64],
        }
        self.0
            .run_mobile_plugin("deviceLocalTimeFacts", Request { instants })
            .map_err(|error| format!("Read Android device local time facts: {error}"))
    }

    /// Ensure Calendar reminders use Android's current default notification sound.
    pub fn ensure_calendar_channel(&self, name: &str, description: &str) -> Result<(), String> {
        self.0
            .run_mobile_plugin(
                "ensureCalendarChannel",
                CalendarChannelRequest { name, description },
            )
            .map_err(|error| format!("create Calendar notification channel: {error}"))
    }

    /// Read whether this Android install may schedule exact alarms.
    pub fn exact_alarm_status(&self) -> Result<ExactAlarmStatus, String> {
        self.0
            .run_mobile_plugin("exactAlarmStatus", ())
            .map_err(|error| format!("read exact-alarm status: {error}"))
    }

    /// Read the observable Android background-execution state.
    pub fn background_execution_status(&self) -> Result<BackgroundExecutionStatus, String> {
        self.0
            .run_mobile_plugin("backgroundExecutionStatus", ())
            .map_err(|error| format!("read background-execution status: {error}"))
    }

    /// Open an Android or OEM background-execution settings screen.
    pub fn open_background_execution_settings(&self, destination: &str) -> Result<(), String> {
        #[derive(Serialize)]
        struct Request<'a> {
            destination: &'a str,
        }

        self.0
            .run_mobile_plugin("openBackgroundExecutionSettings", Request { destination })
            .map_err(|error| format!("open background-execution settings: {error}"))
    }

    /// Open Android's app-specific Alarms and reminders access screen.
    pub fn open_exact_alarm_settings(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin("openExactAlarmSettings", ())
            .map_err(|error| format!("open exact-alarm settings: {error}"))
    }

    /// Open Android's app-specific notification settings screen.
    pub fn open_notification_settings(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin("openNotificationSettings", ())
            .map_err(|error| format!("open notification settings: {error}"))
    }
}

/// Access Ganbaru AI's Android notification capability bridge.
pub trait MobileNotificationsExt<R: Runtime> {
    fn mobile_notifications(&self) -> &MobileNotifications<R>;
}

impl<R: Runtime, T: Manager<R>> MobileNotificationsExt<R> for T {
    fn mobile_notifications(&self) -> &MobileNotifications<R> {
        self.state::<MobileNotifications<R>>().inner()
    }
}

use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::{
    AppHandle, Manager, Runtime,
    plugin::{PluginApi, PluginHandle},
};

const PLUGIN_IDENTIFIER: &str = "org.opengrimoire.ganbaru.mobile.sync";

/// The Android background sync plugin handle.
#[derive(Debug)]
pub struct MobileSync<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> Clone for MobileSync<R> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// Android power state that can keep background sync from running.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConnectivityStatus {
    /// The person restricted the app's background activity.
    pub background_restricted: bool,
    /// The system placed the app in the restricted standby bucket.
    pub standby_restricted: bool,
}

pub(crate) fn init<R: Runtime, C: serde::de::DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> tauri::Result<MobileSync<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "MobileSyncPlugin")?;
    Ok(MobileSync(handle))
}

impl<R: Runtime> MobileSync<R> {
    /// Schedules background passes while `linked`, asks for one soon while changes are
    /// `pending`, and cancels all background work otherwise. `config_dir` is the app config
    /// directory the worker passes to the headless pass.
    pub fn configure(&self, linked: bool, pending: bool, config_dir: &Path) -> Result<(), String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Request<'a> {
            linked: bool,
            pending: bool,
            config_dir: &'a str,
        }

        let config_dir = config_dir
            .to_str()
            .ok_or_else(|| "the app config directory is not UTF-8".to_string())?;
        self.0
            .run_mobile_plugin::<()>(
                "configure",
                Request {
                    linked,
                    pending,
                    config_dir,
                },
            )
            .map_err(|error| format!("configure background sync: {error}"))
    }

    /// Reads the Android power state that can keep background sync from running.
    pub fn connectivity_status(&self) -> Result<ConnectivityStatus, String> {
        self.0
            .run_mobile_plugin("connectivityStatus", ())
            .map_err(|error| format!("read background sync connectivity status: {error}"))
    }
}

/// Access to the background sync plugin from any Tauri manager.
pub trait MobileSyncExt<R: Runtime> {
    /// The plugin handle, or `None` when the plugin did not register.
    fn mobile_sync(&self) -> Option<MobileSync<R>>;
}

impl<R: Runtime, T: Manager<R>> MobileSyncExt<R> for T {
    fn mobile_sync(&self) -> Option<MobileSync<R>> {
        self.try_state::<MobileSync<R>>()
            .map(|state| state.inner().clone())
    }
}

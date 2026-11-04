//! Android background sync scheduling for Ganbaru: the app tells the plugin whether this device
//! syncs with a coordinator and whether local changes wait, and WorkManager runs headless passes.

#![deny(clippy::undocumented_unsafe_blocks, unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "android")]
mod mobile;

#[cfg(target_os = "android")]
pub use mobile::{ConnectivityStatus, MobileSync, MobileSyncExt};

#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::{Runtime, plugin::TauriPlugin};

const PLUGIN_NAME: &str = "ganbaru-mobile-sync";

/// Initialize Ganbaru's Android background sync scheduling plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri::plugin::Builder::new(PLUGIN_NAME);

    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let sync = mobile::init(app, api)?;
        app.manage(sync);
        Ok(())
    });

    builder.build()
}

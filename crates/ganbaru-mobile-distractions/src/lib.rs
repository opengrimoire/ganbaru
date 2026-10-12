//! Narrow Android Distractions enforcement bridge for Ganbaru AI.

#[cfg(target_os = "android")]
mod mobile;

#[cfg(target_os = "android")]
pub use mobile::{MobileDistractions, MobileDistractionsExt, PendingEvent};

#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::{Runtime, plugin::TauriPlugin};

const PLUGIN_NAME: &str = "ganbaru-mobile-distractions";

/// Initialize Ganbaru AI's Android Distractions enforcement bridge.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri::plugin::Builder::new(PLUGIN_NAME);

    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let distractions = mobile::init(app, api)?;
        app.manage(distractions);
        Ok(())
    });

    builder.build()
}

//! Bounded native document transfer for Ganbaru mobile platforms.

#[cfg(target_os = "android")]
mod mobile;

#[cfg(target_os = "android")]
pub use mobile::{MobileDocuments, MobileDocumentsExt};

#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::{Runtime, plugin::TauriPlugin};

/// Initialize Ganbaru's native mobile document adapter.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri::plugin::Builder::new("mobile-documents");

    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let documents = mobile::init(app, api)?;
        app.manage(documents);
        Ok(())
    });

    builder.build()
}

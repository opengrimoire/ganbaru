//! Android Media3 playback and selected music access for Ganbaru AI.

#![deny(clippy::undocumented_unsafe_blocks, unsafe_op_in_unsafe_fn)]

#[cfg(any(target_os = "android", test))]
mod authority;
#[cfg(target_os = "android")]
pub use authority::set_music_authority_checker;

#[cfg(target_os = "android")]
mod mobile;

#[cfg(target_os = "android")]
pub use mobile::{
    MobileMedia, MobileMediaExt, MobileMediaProbe, MobileMediaTree, MobileMediaTreeTrack,
};

#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::{Runtime, plugin::TauriPlugin};

/// Initialize Ganbaru AI's native mobile media adapter.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let builder = tauri::plugin::Builder::new("mobile-media");

    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let media = mobile::init(app, api)?;
        app.manage(media);
        Ok(())
    });

    builder.build()
}

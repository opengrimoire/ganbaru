//! Music library domain independent of Tauri and the WebView: library items and sources,
//! playlists and memberships, review and snoozes, search, listening history, transfer and
//! interchange, soundscapes, desktop local media refresh, relink, repair, and artwork, and the
//! deterministic playback session policy.
//! Callers supply an authorized vault pool and device-local root bindings.

#[cfg(desktop)]
pub mod artwork;
mod defaults;
#[cfg(desktop)]
pub mod fixtures;
mod interchange;
#[cfg(desktop)]
pub mod item_repair;
#[cfg(desktop)]
pub mod local_refresh;
#[cfg(desktop)]
pub mod media;
mod models;
mod playback;
pub use playback::record_listening_in_transaction;
pub mod playlist_edits;
pub mod queries;
#[cfg(desktop)]
pub mod relink;
mod rows;
pub mod search;
pub mod session;
#[cfg(desktop)]
pub mod soundscape_groups;
#[cfg(desktop)]
pub mod soundscapes;
pub mod source_lifecycle;
pub mod transfer;
mod transfer_codec;
mod transfer_export;
mod transfer_read;
mod validation;
pub mod writes;
pub mod youtube;

#[cfg(test)]
pub use ganbaru_music::error::MusicLibraryErrorCode;
pub use ganbaru_music::error::{MusicLibraryError, MusicLibraryResult};
pub use models::*;
pub(crate) use rows::*;
pub use transfer::{
    MusicTransferCommit, MusicTransferExport, MusicTransferFormat, MusicTransferPreview,
    MusicTransferSource,
};
pub(crate) use validation::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_support {
    /// Runs one async test body on a current-thread Tokio runtime.
    pub(crate) fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create music library test runtime")
            .block_on(future)
    }
}

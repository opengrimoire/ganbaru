//! SQLite source of truth for Chat history and projections.
//!
//! Attachment import and reads and primary working folder resolution build on every platform, so
//! a phone attaches images without the native runtime; the rest needs it.

pub mod attachments;
#[cfg(feature = "native-runtime")]
pub mod drafts;
#[cfg(feature = "native-runtime")]
pub mod events;
#[cfg(feature = "native-runtime")]
pub mod lifecycle;
#[cfg(feature = "native-runtime")]
pub mod live_execution;
pub mod primary_folder;
#[cfg(feature = "native-runtime")]
pub mod project_files;
#[cfg(feature = "native-runtime")]
pub mod provider_lifecycle;
#[cfg(feature = "native-runtime")]
pub mod reads;
#[cfg(feature = "native-runtime")]
pub mod rebuild;
#[cfg(feature = "native-runtime")]
pub mod receipts;
#[cfg(feature = "native-runtime")]
pub mod recovery;
#[cfg(feature = "native-runtime")]
pub mod resources;
#[cfg(feature = "native-runtime")]
pub mod state_writes;
#[cfg(feature = "native-runtime")]
pub mod workspaces;

//! Notes domain, persistence, transfers, history, assets, and validation.

pub mod assets;
pub mod block_storage;
pub mod blocking;
pub mod comments;
pub mod data_sources;
pub mod databases;
pub mod folders;
pub mod links;
pub mod local_user;
pub mod mention_notifications;
pub mod models;
pub mod page_history;
pub mod page_property_storage;
pub mod project_history;
pub mod property_storage;
pub mod reads;
pub mod search;
pub(crate) mod structure;
pub mod suggestions;
pub mod sync_keys;
pub mod templates;
pub mod transfers;
pub mod undo_state;
pub mod validation;
pub mod working_markdown;
pub mod workspace_shell;
pub mod writes;

pub use models::*;
pub use project_history::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Notes test runtime must start")
        .block_on(future)
}

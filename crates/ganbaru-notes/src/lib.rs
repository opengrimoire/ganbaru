//! Notes domain, persistence, transfers, history, assets, and validation.

pub mod assets;
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
pub mod project_history;
pub mod reads;
pub mod search;
pub mod suggestions;
pub mod templates;
pub mod transfers;
pub mod undo_state;
pub mod validation;
pub mod working_markdown;
pub mod workspace_shell;
pub mod writes;

pub use models::*;
pub use project_history::*;

/// Page property that assigns a top-level page to a project.
pub(crate) const PAGE_PROJECT_ID_PROPERTY: &str = "__ganbaru_project_id";

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

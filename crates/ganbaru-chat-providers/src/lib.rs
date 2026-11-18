//! Provider processes, transports, drivers, event sinks, and registry.

#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

pub(crate) use ganbaru_chat_contracts::{events, models};

pub mod executable_search;
pub mod model_recommendation;
mod probe_directory;
pub mod process;

pub mod agent_preferences;
mod claude;
mod cli_install;
pub mod cli_updates;
mod codex;
mod confinement;
mod cursor;
mod driver;
mod home_identity;
mod host_path;
mod opencode;
mod registry;
#[cfg(test)]
mod replay;
pub mod runtime_review;
mod unsupported;

pub use cli_install::{InstallRoots, installer_executable_directories};
pub use driver::*;
pub use ganbaru_chat_contracts::models::ProviderAuthoritySupport;
pub use home_identity::provider_home_identity;
pub use host_path::{host_variable, search_path};
pub use registry::ProviderDriverRegistry;
pub use unsupported::*;

/// Whether agent processes on this device run under an enforcing sandbox helper, which
/// keeps them from reading the user's home folders outside their provider home.
///
/// Without one, the agents that run at all run unconfined and read those folders natively.
pub fn agents_run_confined() -> bool {
    confinement::sandbox_helper().is_some()
}

#[cfg(test)]
pub(crate) fn test_block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("provider test runtime must start")
        .block_on(future)
}

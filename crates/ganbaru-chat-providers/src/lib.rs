//! Provider processes, transports, drivers, event sinks, and registry.

#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

pub(crate) use ganbaru_chat_contracts::{events, models};

pub mod process;

mod claude;
mod codex;
mod cursor;
mod driver;
mod opencode;
mod registry;
mod unsupported;

pub use driver::*;
pub use ganbaru_chat_contracts::models::ProviderAuthoritySupport;
pub use registry::ProviderDriverRegistry;
pub use unsupported::*;

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

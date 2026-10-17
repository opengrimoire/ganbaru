//! Chat persistence, orchestration, runtime, and workspace services.

#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "native-runtime")]
pub mod agent_runs;
#[cfg(feature = "native-runtime")]
pub mod checkpoints;
#[cfg(feature = "native-runtime")]
pub mod composer;
pub mod coordination;
pub mod credentials;
#[cfg(feature = "native-runtime")]
pub mod driver_operations;
#[cfg(feature = "native-runtime")]
pub mod git;
#[cfg(feature = "native-runtime")]
pub mod ingestion;
#[cfg(feature = "native-runtime")]
pub mod repository;
#[cfg(feature = "native-runtime")]
pub mod review_engine;
#[cfg(feature = "native-runtime")]
pub mod runtime;
#[cfg(feature = "native-runtime")]
pub mod source_control;
#[cfg(feature = "native-runtime")]
pub mod state;
#[cfg(feature = "native-runtime")]
pub mod workspace;

pub use ganbaru_chat_contracts::{config, events, models};

#[cfg(feature = "native-runtime")]
pub use ganbaru_chat_providers as providers;
#[cfg(feature = "native-runtime")]
pub use ganbaru_chat_providers::process;

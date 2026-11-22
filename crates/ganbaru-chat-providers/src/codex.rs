//! Codex app-server provider.

mod capacity_retry;
mod driver;
mod home;
mod normalizer;
mod organizational;
mod protocol;
mod sandbox_check;
mod session;
mod transport;
mod turn_route;

#[cfg(test)]
pub(crate) mod replay;

pub use driver::CodexProviderDriver;
pub(crate) use protocol::CODEX_COMPATIBILITY;

#[cfg(test)]
mod tests;

//! Claude Code provider over the bounded SDK JSONL protocol.

mod approvals;
mod driver;
mod home;
mod instructions;
mod normalizer;
mod operations;
mod organizational;
mod preferences;
mod protocol;
mod session;
mod support;
mod transport;

#[cfg(test)]
pub(crate) mod replay;

pub use driver::ClaudeProviderDriver;
pub(crate) use home::resolve_claude_home;
pub(crate) use protocol::CLAUDE_COMPATIBILITY;

#[cfg(test)]
mod tests;

//! Claude Code provider over the bounded SDK JSONL protocol.

mod driver;
mod home;
mod normalizer;
mod operations;
mod protocol;
mod session;
mod support;
mod transport;

pub use driver::ClaudeProviderDriver;

#[cfg(test)]
mod tests;

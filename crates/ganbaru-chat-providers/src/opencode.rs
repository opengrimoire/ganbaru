//! OpenCode HTTP and event-stream provider.

mod cli;
mod config;
mod driver;
mod event_stream;
mod http_client;
mod local_server;
mod normalizer;
mod operations;
mod permissions;
mod protocol;
mod support;

pub use driver::OpenCodeProviderDriver;

#[cfg(test)]
mod tests;

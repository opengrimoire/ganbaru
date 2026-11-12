//! Replay hook for the OpenCode driver: a driver attached to the replay server as an external
//! OpenCode server.

use super::driver::OpenCodeProviderDriver;
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplaySetup, organizational_mcp};
use serde_json::json;
use std::time::Duration;

/// An OpenCode driver attached to the server at `origin`, with `busy_stream_timeout` as the
/// silence bound of a busy event stream when one is given.
pub(crate) fn replay_driver(
    setup: &ReplaySetup<'_>,
    origin: &str,
    busy_stream_timeout: Option<Duration>,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut configuration = super::tests::configuration(json!({
        "mode": "external",
        "serverUrl": origin,
        "confirmExternalWorkspaceAccess": true,
    }));
    if setup.organizational {
        configuration.internal_mcp = Some(organizational_mcp());
    }
    let driver = OpenCodeProviderDriver::new(configuration)?;
    Ok(Box::new(match busy_stream_timeout {
        Some(timeout) => driver.with_busy_stream_timeout(timeout),
        None => driver,
    }))
}

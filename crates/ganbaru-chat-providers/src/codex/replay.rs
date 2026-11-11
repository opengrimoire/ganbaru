//! Replay hooks for the Codex driver: a driver over a replay connection and the reader check.

use super::driver::CodexProviderDriver;
use super::home::CodexHomeLayout;
use super::transport::{CodexRpcConnection, read_bounded_line};
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplayConnection, ReplaySetup, one_shot, organizational_mcp};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::BufReader;

/// A Codex driver whose next session runs over `connection`.
pub(crate) fn replay_driver(
    setup: &ReplaySetup<'_>,
    connection: ReplayConnection,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut driver = CodexProviderDriver::new(replay_configuration(setup))?;
    let home = setup.home.to_path_buf();
    let take = one_shot(CodexRpcConnection::from_test_io(
        connection.reader,
        connection.writer,
    ));
    driver.set_connection_factory(Arc::new(move |_| {
        Ok((
            take()?,
            CodexHomeLayout {
                shared_home: home.clone(),
                effective_home: home.clone(),
                shadowed: false,
            },
        ))
    }));
    Ok(Box::new(driver))
}

/// A Codex driver that spawns `executable` with `environment`, for recording a real CLI through
/// the fake CLI's record mode.
pub(crate) fn recording_driver(
    setup: &ReplaySetup<'_>,
    executable: &Path,
    environment: BTreeMap<String, String>,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut configuration = replay_configuration(setup);
    configuration.executable = executable.to_string_lossy().into_owned();
    configuration.environment = environment;
    Ok(Box::new(CodexProviderDriver::new(configuration)?))
}

fn replay_configuration(setup: &ReplaySetup<'_>) -> ProviderInstanceConfig {
    let mut configuration = super::tests::configuration(setup.home, None);
    if setup.organizational {
        configuration.internal_mcp = Some(organizational_mcp());
    }
    configuration
}

/// Every frame the Codex JSON Lines reader splits out of `bytes`, parsed.
pub(crate) async fn read_jsonl_frames(bytes: &[u8]) -> Vec<Value> {
    let mut reader = BufReader::new(bytes);
    let mut frames = Vec::new();
    while let Ok(Some(line)) = read_bounded_line(&mut reader).await {
        frames.push(serde_json::from_slice(&line).expect("each line is one JSON frame"));
    }
    frames
}

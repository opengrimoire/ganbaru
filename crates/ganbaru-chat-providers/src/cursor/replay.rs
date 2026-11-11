//! Replay hooks for the Cursor driver: a driver over a replay connection and the reader check.

use super::driver::CursorProviderDriver;
use super::executable::AcpProviderAbout;
use super::transport::{AcpRpcConnection, read_bounded_line};
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplayConnection, ReplaySetup, one_shot, organizational_mcp};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::BufReader;

/// A Cursor driver whose next session runs over `connection`, reporting the transcript's CLI
/// version as the probed one.
pub(crate) fn replay_driver(
    setup: &ReplaySetup<'_>,
    connection: ReplayConnection,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut driver = CursorProviderDriver::new(replay_configuration(setup))?;
    let version = setup.cli_version.to_string();
    let take = one_shot(AcpRpcConnection::from_test_io(
        connection.reader,
        connection.writer,
    ));
    driver.set_connection_factory(Arc::new(move |_| {
        Ok((
            take()?,
            AcpProviderAbout {
                version: version.clone(),
                account_label: None,
                authenticated: Some(true),
            },
        ))
    }));
    Ok(Box::new(driver))
}

/// A Cursor driver that spawns `executable` with `environment`, for recording a real CLI through
/// the fake CLI's record mode.
pub(crate) fn recording_driver(
    setup: &ReplaySetup<'_>,
    executable: &Path,
    environment: BTreeMap<String, String>,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut configuration = replay_configuration(setup);
    configuration.executable = executable.to_string_lossy().into_owned();
    configuration.environment = environment;
    Ok(Box::new(CursorProviderDriver::new(configuration)?))
}

fn replay_configuration(setup: &ReplaySetup<'_>) -> ProviderInstanceConfig {
    let mut configuration = super::tests::support::configuration(setup.home, None);
    if setup.organizational {
        configuration.internal_mcp = Some(organizational_mcp());
    }
    configuration
}

/// Every frame the Cursor JSON Lines reader splits out of `bytes`, parsed.
pub(crate) async fn read_jsonl_frames(bytes: &[u8]) -> Vec<Value> {
    let mut reader = BufReader::new(bytes);
    let mut frames = Vec::new();
    while let Ok(Some(line)) = read_bounded_line(&mut reader).await {
        frames.push(serde_json::from_slice(&line).expect("each line is one JSON frame"));
    }
    frames
}

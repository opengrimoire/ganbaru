//! Replay hooks for the ACP driver: a driver over a replay connection and the reader check.

use super::driver::{AcpProviderFlavor, CursorProviderDriver};
use super::executable::AcpProviderAbout;
use super::transport::{AcpRpcConnection, read_bounded_line};
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplayConnection, ReplaySetup, one_shot, organizational_mcp};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use tokio::io::BufReader;

/// An ACP driver of `flavor` whose next session runs over `connection`, reporting the
/// transcript's CLI version as the probed one.
pub(crate) fn replay_driver(
    flavor: AcpProviderFlavor,
    setup: &ReplaySetup<'_>,
    connection: ReplayConnection,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut driver = new_driver(flavor, replay_configuration(flavor, setup))?;
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

/// An ACP driver of `flavor` that spawns `executable` with `environment`, for recording a real
/// CLI through the fake CLI's record mode.
pub(crate) fn recording_driver(
    flavor: AcpProviderFlavor,
    setup: &ReplaySetup<'_>,
    executable: &Path,
    environment: BTreeMap<String, String>,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut configuration = replay_configuration(flavor, setup);
    configuration.executable = executable.to_string_lossy().into_owned();
    configuration.environment.extend(environment);
    Ok(Box::new(new_driver(flavor, configuration)?))
}

fn new_driver(
    flavor: AcpProviderFlavor,
    configuration: ProviderInstanceConfig,
) -> ChatResult<CursorProviderDriver> {
    match flavor {
        AcpProviderFlavor::Cursor => CursorProviderDriver::new(configuration),
        AcpProviderFlavor::Grok => CursorProviderDriver::new_grok(configuration),
        AcpProviderFlavor::Generic => CursorProviderDriver::new_generic(configuration),
    }
}

/// The instance a replay of `flavor` runs: Grok signs in with its cached token rather than an
/// API key from the test process, and a generic agent offers no authentication method.
fn replay_configuration(
    flavor: AcpProviderFlavor,
    setup: &ReplaySetup<'_>,
) -> ProviderInstanceConfig {
    let mut configuration = match flavor {
        AcpProviderFlavor::Cursor => super::tests::support::configuration(setup.home, None),
        AcpProviderFlavor::Grok => {
            let mut configuration = super::tests::support::configuration(setup.home, None);
            configuration.family_id =
                ProviderFamilyId::new("grok".to_string()).expect("the grok family id is valid");
            configuration.instance_id = ProviderInstanceId::new("grok-instance-1".to_string())
                .expect("the replay instance id is valid");
            configuration.label = "Grok".to_string();
            configuration.executable = "grok".to_string();
            configuration
                .environment
                .insert("XAI_API_KEY".to_string(), String::new());
            configuration.provider_config.value = json!({});
            configuration
        }
        AcpProviderFlavor::Generic => {
            super::tests::support::generic_configuration(setup.home, None)
        }
    };
    if setup.organizational {
        configuration.internal_mcp = Some(organizational_mcp());
    }
    configuration
}

/// Every frame the ACP JSON Lines reader splits out of `bytes`, parsed.
pub(crate) async fn read_jsonl_frames(bytes: &[u8]) -> Vec<Value> {
    let mut reader = BufReader::new(bytes);
    let mut frames = Vec::new();
    while let Ok(Some(line)) = read_bounded_line(&mut reader).await {
        frames.push(serde_json::from_slice(&line).expect("each line is one JSON frame"));
    }
    frames
}

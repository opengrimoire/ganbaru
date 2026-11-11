//! Replay hooks for the Codex driver: a driver over a replay connection and the reader check.

use super::driver::CodexProviderDriver;
use super::home::CodexHomeLayout;
use super::transport::{CodexRpcConnection, read_bounded_line};
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplayConnection, ReplaySetup, one_shot, organizational_mcp};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
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

/// Every `method` enum value the vendored Codex schema declares under an object's `properties`.
pub(crate) fn vendored_schema_methods() -> BTreeSet<String> {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/codex/compat/codex_app_server_protocol.schemas.json");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(&schema_path).expect("the vendored Codex schema is readable"),
    )
    .expect("the vendored Codex schema is JSON");
    let mut methods = BTreeSet::new();
    collect_schema_methods(&schema, &mut methods);
    assert!(
        methods.contains("turn/start"),
        "the schema walk finds methods"
    );
    methods
}

fn collect_schema_methods(schema: &Value, methods: &mut BTreeSet<String>) {
    match schema {
        Value::Object(fields) => {
            if let Some(Value::Array(values)) = fields
                .get("properties")
                .and_then(|properties| properties.get("method"))
                .and_then(|method| method.get("enum"))
            {
                methods.extend(values.iter().filter_map(Value::as_str).map(str::to_string));
            }
            for value in fields.values() {
                collect_schema_methods(value, methods);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_schema_methods(item, methods);
            }
        }
        _ => {}
    }
}

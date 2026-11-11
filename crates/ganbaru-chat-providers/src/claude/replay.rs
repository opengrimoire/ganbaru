//! Replay hooks for the Claude driver: a driver over a replay connection and the reader check.

use super::driver::ClaudeProviderDriver;
use super::home::ClaudeHome;
use super::protocol::MINIMUM_CLAUDE_VERSION;
use super::transport::ClaudeJsonlConnection;
use crate::ProviderDriver;
use crate::models::*;
use crate::replay::runner::{ReplayConnection, ReplaySetup};
use std::sync::{Arc, Mutex};

/// A Claude driver whose next session runs over `connection`.
pub(crate) fn replay_driver(
    setup: &ReplaySetup<'_>,
    connection: ReplayConnection,
) -> ChatResult<Box<dyn ProviderDriver>> {
    let mut configuration = super::tests::configuration(setup.home);
    if setup.organizational {
        configuration.internal_mcp = Some(ProviderInternalMcpConfig {
            name: "ganbaru-chat".to_string(),
            url: "http://127.0.0.1:41234/mcp".to_string(),
            bearer_token: "replay-token".to_string(),
            organizational_authority: true,
        });
    }
    let mut driver = ClaudeProviderDriver::new(configuration)?;
    let home = setup.home.to_path_buf();
    let connection = Arc::new(Mutex::new(Some(ClaudeJsonlConnection::from_test_io(
        connection.reader,
        connection.writer,
    ))));
    driver.set_connection_factory(Arc::new(move |_| {
        let connection = connection.lock().unwrap().take().ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::TransportUnavailable,
                "the replay connection was already used",
                false,
            )
        })?;
        Ok((
            connection,
            ClaudeHome {
                config_directory: home.clone(),
            },
            MINIMUM_CLAUDE_VERSION,
        ))
    }));
    Ok(Box::new(driver))
}

/// Every frame the Claude JSON Lines reader splits out of `bytes`, parsed.
pub(crate) async fn read_jsonl_frames(bytes: &[u8]) -> Vec<serde_json::Value> {
    let mut reader = tokio::io::BufReader::new(bytes);
    let mut frames = Vec::new();
    let mut buffer = Vec::new();
    while let Ok(Some(line)) =
        super::transport::read_bounded_line(&mut reader, &mut buffer, 1024 * 1024).await
    {
        assert!(!line.oversized);
        frames.push(serde_json::from_slice(&buffer).expect("each line is one JSON frame"));
        buffer.clear();
    }
    frames
}

//! Replay hooks for the Codex driver.

use super::transport::read_bounded_line;
use serde_json::Value;
use tokio::io::BufReader;

/// Every frame the Codex JSON Lines reader splits out of `bytes`, parsed.
pub(crate) async fn read_jsonl_frames(bytes: &[u8]) -> Vec<Value> {
    let mut reader = BufReader::new(bytes);
    let mut frames = Vec::new();
    while let Ok(Some(line)) = read_bounded_line(&mut reader).await {
        frames.push(serde_json::from_slice(&line).expect("each line is one JSON frame"));
    }
    frames
}

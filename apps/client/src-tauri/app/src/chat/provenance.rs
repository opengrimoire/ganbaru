//! Chat message provenance: which linked devices sealed a message.
//!
//! A message's author and display label are claims its writer sealed. The sync log names the
//! device behind each creation, so a reader can compare the claim with the sealing device.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_sync::manifest::vault::chat::MESSAGES_TABLE;
use ganbaru_sync::{DeviceNames, DeviceRef, provenance};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::chat::channels::chat_pool;

/// The devices that sealed a Chat message.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageProvenance {
    /// Empty when sync holds no record of the message.
    devices: Vec<DeviceRef>,
}

fn sync_error(error: ganbaru_sync::SyncError) -> ChatError {
    eprintln!("Read Chat message provenance: {error}");
    ChatError::new(
        ChatErrorCode::Persistence,
        "Read Chat message provenance",
        true,
    )
}

/// Reads the sealing devices of a message, named from the devices the sync log records.
async fn provenance_from_pool(
    pool: &SqlitePool,
    own_device_id: &str,
    message_item_id: &str,
) -> ChatResult<ChatMessageProvenance> {
    let mut connection = pool.acquire().await.map_err(|error| {
        eprintln!("Open Chat message provenance connection: {error}");
        ChatError::new(
            ChatErrorCode::Persistence,
            "Read Chat message provenance",
            true,
        )
    })?;
    let device_ids =
        provenance::row_creating_devices(&mut connection, MESSAGES_TABLE, message_item_id)
            .await
            .map_err(sync_error)?;
    let labels = provenance::device_labels(&mut connection)
        .await
        .map_err(sync_error)?;
    let names = DeviceNames::new(own_device_id, labels);
    Ok(ChatMessageProvenance {
        devices: device_ids
            .iter()
            .map(|device_id| names.describe(device_id))
            .collect(),
    })
}

/// The devices that sealed a message, for comparing with the author it claims.
#[tauri::command]
pub async fn chat_message_provenance(
    app: tauri::AppHandle,
    db_url: String,
    message_item_id: String,
) -> ChatResult<ChatMessageProvenance> {
    let own_device_id = crate::sync::devices::stable_device_id(&app).map_err(|error| {
        eprintln!("Read this device id: {error}");
        ChatError::new(ChatErrorCode::Persistence, "Read this device id", true)
    })?;
    let pool = chat_pool(app, db_url).await?;
    provenance_from_pool(&pool, &own_device_id, &message_item_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn migrated_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        ganbaru_db::test_support::apply_migrated_template(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn a_message_without_a_sync_record_has_no_provenance() {
        let pool = migrated_pool().await;
        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();
        assert!(provenance.devices.is_empty());
    }
}

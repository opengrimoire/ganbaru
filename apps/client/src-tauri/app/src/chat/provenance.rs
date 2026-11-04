//! Chat message provenance: which linked devices sealed a message and its current text.
//!
//! A message's author and display label are claims its writer sealed, and its author and content
//! registers stay editable by any linked device. The sync log names the device behind each row
//! creation, and an edit creates a new revision, so a reader can compare the claim with the
//! device that sealed the message and with the device that wrote its current text.

use ganbaru_chat_contracts::models::{ChatError, ChatErrorCode, ChatResult};
use ganbaru_sync::manifest::vault::VAULT_MANIFEST;
use ganbaru_sync::manifest::vault::chat::{MESSAGES_TABLE, REVISIONS_TABLE};
use ganbaru_sync::{DeviceNames, DeviceRef, provenance};
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};

use crate::chat::channels::chat_pool;

/// The devices that sealed a Chat message.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessageProvenance {
    /// Empty when sync holds no record of the message.
    devices: Vec<DeviceRef>,
    /// Devices that created the current revision. Empty when they are the same as `devices`, or
    /// when sync holds no record of the revision.
    last_edited_by: Vec<DeviceRef>,
}

fn sync_error(error: ganbaru_sync::SyncError) -> ChatError {
    eprintln!("Read Chat message provenance: {error}");
    ChatError::new(
        ChatErrorCode::Persistence,
        "Read Chat message provenance",
        true,
    )
}

fn database_error(error: sqlx::Error) -> ChatError {
    eprintln!("Read Chat message provenance: {error}");
    ChatError::new(
        ChatErrorCode::Persistence,
        "Read Chat message provenance",
        true,
    )
}

/// Reads the devices that created the current revision of a message, ordered by device id.
///
/// Empty when the message has no current revision or sync holds no record of it.
async fn current_revision_devices(
    connection: &mut SqliteConnection,
    message_item_id: &str,
) -> ChatResult<Vec<String>> {
    let revision_id: Option<Option<String>> = sqlx::query_scalar(
        "SELECT current_revision_id FROM chat_communication_messages WHERE item_id = ?",
    )
    .bind(message_item_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database_error)?;
    let Some(revision_id) = revision_id.flatten() else {
        return Ok(Vec::new());
    };
    provenance::row_creating_devices(connection, &VAULT_MANIFEST, REVISIONS_TABLE, &revision_id)
        .await
        .map_err(sync_error)
}

/// Reads the sealing devices of a message and of its current text, named from the devices the
/// sync log records.
async fn provenance_from_pool(
    pool: &SqlitePool,
    own_device_id: &str,
    message_item_id: &str,
) -> ChatResult<ChatMessageProvenance> {
    let mut connection = pool.acquire().await.map_err(database_error)?;
    let device_ids = provenance::row_creating_devices(
        &mut connection,
        &VAULT_MANIFEST,
        MESSAGES_TABLE,
        message_item_id,
    )
    .await
    .map_err(sync_error)?;
    let editor_ids = current_revision_devices(&mut connection, message_item_id).await?;
    let labels = provenance::device_labels(&mut connection)
        .await
        .map_err(sync_error)?;
    let names = DeviceNames::new(own_device_id, labels);
    let describe =
        |ids: &[String]| -> Vec<DeviceRef> { ids.iter().map(|id| names.describe(id)).collect() };
    Ok(ChatMessageProvenance {
        devices: describe(&device_ids),
        last_edited_by: if editor_ids == device_ids {
            Vec::new()
        } else {
            describe(&editor_ids)
        },
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
    use ganbaru_sync::manifest::MergeKind;
    use ganbaru_sync_contracts::TableId;

    const SPACE: [u8; 16] = [7; 16];

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

    /// A pool for rows seeded alone, without the conversation and space rows they refer to.
    async fn seeded_pool() -> SqlitePool {
        let pool = migrated_pool().await;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    /// Enrolls `device_id` under `label` with one writer, identified by `writer`.
    async fn seed_device(pool: &SqlitePool, writer: u8, device_id: &str, label: &str) {
        sqlx::query(
            "INSERT INTO sync_devices
                (space_id, device_id, device_key, tls_fingerprint, label, kind, role,
                 enrolled_revision)
             VALUES (?, ?, ?, ?, ?, 'desktop', 'member', 0)",
        )
        .bind(SPACE.as_slice())
        .bind(device_id)
        .bind([writer; 32].as_slice())
        .bind([writer; 32].as_slice())
        .bind(label)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sync_writers
                (space_id, writer_id, public_key, device_id, certificate, state, stored_seq,
                 applied_seq, head_hash, head_clock, created_at_ms)
             VALUES (?, ?, ?, ?, x'00', 'active', 1, 1, ?, 0, 0)",
        )
        .bind(SPACE.as_slice())
        .bind([writer; 16].as_slice())
        .bind([writer; 32].as_slice())
        .bind(device_id)
        .bind([writer; 32].as_slice())
        .execute(pool)
        .await
        .unwrap();
    }

    /// Records that `writer` created `row_key` and holds its versions of the Immutable groups.
    async fn seed_creation(pool: &SqlitePool, table: TableId, row_key: &str, writer: u8) {
        seed_skipped_creation(pool, table, row_key, writer).await;
        let spec = VAULT_MANIFEST.table(table).unwrap();
        for group in spec
            .groups
            .iter()
            .filter(|group| group.kind == MergeKind::Immutable)
        {
            sqlx::query(
                "INSERT INTO sync_register_versions
                    (table_id, row_key, group_id, writer_id, seq, clock, value_hash)
                 VALUES (?, ?, ?, ?, 1, 0, ?)",
            )
            .bind(i64::from(table.0))
            .bind(row_key)
            .bind(i64::from(group.id.get()))
            .bind([writer; 16].as_slice())
            .bind([writer; 32].as_slice())
            .execute(pool)
            .await
            .unwrap();
        }
    }

    /// Records that `writer` created `row_key` with a Create that left the Immutable groups to
    /// another writer.
    async fn seed_skipped_creation(pool: &SqlitePool, table: TableId, row_key: &str, writer: u8) {
        sqlx::query(
            "INSERT INTO sync_row_creations (table_id, row_key, writer_id, seq)
             VALUES (?, ?, ?, 1)",
        )
        .bind(i64::from(table.0))
        .bind(row_key)
        .bind([writer; 16].as_slice())
        .execute(pool)
        .await
        .unwrap();
    }

    /// Stores message `item_id` whose current revision is `revision_id`.
    async fn seed_message(pool: &SqlitePool, item_id: &str, revision_id: &str) {
        sqlx::query(
            "INSERT INTO chat_communication_messages (item_id, current_revision_id, created_at)
             VALUES (?, ?, '2026-11-01T10:00:00.000Z')",
        )
        .bind(item_id)
        .bind(revision_id)
        .execute(pool)
        .await
        .unwrap();
    }

    /// A creation under the wrong table or the wrong key must not reach the result.
    #[tokio::test]
    async fn provenance_reads_the_message_row() {
        let pool = seeded_pool().await;
        seed_device(&pool, 1, "honest", "Desk").await;
        seed_device(&pool, 2, "other", "Phone").await;
        seed_message(&pool, "item-1", "revision-1").await;
        seed_creation(&pool, MESSAGES_TABLE, "item-1", 1).await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-1", 1).await;
        seed_creation(&pool, MESSAGES_TABLE, "item-2", 2).await;
        seed_creation(&pool, MESSAGES_TABLE, "revision-1", 2).await;
        seed_creation(&pool, REVISIONS_TABLE, "item-1", 2).await;

        let provenance = provenance_from_pool(&pool, "honest", "item-1")
            .await
            .unwrap();

        let creators: Vec<(&str, Option<&str>, bool)> = provenance
            .devices
            .iter()
            .map(|device| {
                (
                    device.device_id.as_str(),
                    device.device_label.as_deref(),
                    device.own_device,
                )
            })
            .collect();
        assert_eq!(creators, [("honest", Some("Desk"), true)]);
        assert!(provenance.last_edited_by.is_empty());
    }

    #[tokio::test]
    async fn a_later_edit_names_its_device() {
        let pool = seeded_pool().await;
        seed_device(&pool, 1, "honest", "Desk").await;
        seed_device(&pool, 2, "editor", "Phone").await;
        seed_message(&pool, "item-1", "revision-2").await;
        seed_creation(&pool, MESSAGES_TABLE, "item-1", 1).await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-1", 1).await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-2", 2).await;

        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();

        let creators: Vec<&str> = provenance
            .devices
            .iter()
            .map(|device| device.device_id.as_str())
            .collect();
        assert_eq!(creators, ["honest"]);
        let editors: Vec<(&str, Option<&str>)> = provenance
            .last_edited_by
            .iter()
            .map(|device| (device.device_id.as_str(), device.device_label.as_deref()))
            .collect();
        assert_eq!(editors, [("editor", Some("Phone"))]);
    }

    #[tokio::test]
    async fn an_edit_from_the_creating_device_adds_no_line() {
        let pool = seeded_pool().await;
        seed_device(&pool, 1, "honest", "Desk").await;
        seed_message(&pool, "item-1", "revision-2").await;
        seed_creation(&pool, MESSAGES_TABLE, "item-1", 1).await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-2", 1).await;

        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();

        assert_eq!(provenance.devices.len(), 1);
        assert!(provenance.last_edited_by.is_empty());
    }

    #[tokio::test]
    async fn a_revision_without_a_sync_record_stays_silent() {
        let pool = seeded_pool().await;
        seed_device(&pool, 1, "honest", "Desk").await;
        seed_message(&pool, "item-1", "revision-2").await;
        seed_creation(&pool, MESSAGES_TABLE, "item-1", 1).await;

        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();

        assert_eq!(provenance.devices.len(), 1);
        assert!(provenance.last_edited_by.is_empty());
    }

    #[tokio::test]
    async fn a_revision_is_named_when_the_message_has_no_sync_record() {
        let pool = seeded_pool().await;
        seed_device(&pool, 2, "editor", "Phone").await;
        seed_message(&pool, "item-1", "revision-2").await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-2", 2).await;

        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();

        assert!(provenance.devices.is_empty());
        assert_eq!(provenance.last_edited_by.len(), 1);
    }

    /// A device whose Create left the message's Immutable values to another device is not
    /// named as a sealing device.
    #[tokio::test]
    async fn a_create_that_kept_another_devices_values_is_not_named() {
        let pool = seeded_pool().await;
        seed_device(&pool, 1, "honest", "Desk").await;
        seed_device(&pool, 2, "forger", "Phone").await;
        seed_message(&pool, "item-1", "revision-1").await;
        seed_creation(&pool, MESSAGES_TABLE, "item-1", 1).await;
        seed_skipped_creation(&pool, MESSAGES_TABLE, "item-1", 2).await;
        seed_creation(&pool, REVISIONS_TABLE, "revision-1", 1).await;
        seed_skipped_creation(&pool, REVISIONS_TABLE, "revision-1", 2).await;

        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();

        let creators: Vec<&str> = provenance
            .devices
            .iter()
            .map(|device| device.device_id.as_str())
            .collect();
        assert_eq!(creators, ["honest"]);
        assert!(provenance.last_edited_by.is_empty());
    }

    #[tokio::test]
    async fn a_message_without_a_sync_record_has_no_provenance() {
        let pool = migrated_pool().await;
        let provenance = provenance_from_pool(&pool, "own", "item-1").await.unwrap();
        assert!(provenance.devices.is_empty());
    }
}

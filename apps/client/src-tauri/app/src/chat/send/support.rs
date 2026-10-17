//! Shared time, operation, persistence, and error helpers for sends.

use crate::db;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, UtcTimestamp, VersionedJson,
};
use ganbaru_chat_providers::{DriverCancellation, DriverOperationContext};
use serde::Serialize;
use sqlx::SqlitePool;
use std::time::{Duration, Instant};

pub(super) const PROVIDER_START_TIMEOUT: Duration = Duration::from_secs(45);
pub(super) const TURN_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn operation_context(operation_id: &str, timeout: Duration) -> DriverOperationContext {
    DriverOperationContext {
        operation_id: operation_id.to_string(),
        deadline: Instant::now() + timeout,
        cancellation: DriverCancellation::default(),
    }
}

pub(super) fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

pub(super) async fn chat_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    db::connect_sqlite(app, db_url)
        .await
        .map_err(|_| ChatError::new(ChatErrorCode::Persistence, "open Chat database", true))
}

pub(super) fn versioned_value<T: Serialize>(value: &T) -> ChatResult<VersionedJson> {
    Ok(VersionedJson {
        schema_version: 1,
        value: serde_json::to_value(value).map_err(json_error)?,
    })
}

pub(super) fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value)
        .map_err(|_| ChatError::validation("revision", "Chat revision is too large"))
}

pub(super) fn runtime_not_running_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::InvalidStateTransition,
        "Chat provider session is not running",
        true,
    )
}

pub(super) fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat device state could not be read",
        true,
    )
}

pub(super) fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat send persistence failed",
        true,
    )
}

pub(super) fn json_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat send data is invalid",
        false,
    )
}

pub(super) fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat send data is invalid",
        false,
    )
}

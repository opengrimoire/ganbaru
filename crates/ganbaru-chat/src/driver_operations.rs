//! Durable receipt handling for provider driver operations.

use super::models::{
    ChatCommandId, ChatError, ChatErrorCode, ChatResult, DriverOperationReceipt, UtcTimestamp,
    VersionedJson,
};
use super::repository::receipts::{
    CommandReceiptRead, CommandReceiptState, complete_command_receipt,
};
use chrono::{SecondsFormat, Utc};
use sqlx::SqlitePool;

pub fn replay_driver_receipt(receipt: CommandReceiptRead) -> ChatResult<DriverOperationReceipt> {
    match receipt.state {
        CommandReceiptState::Accepted => Err(ChatError::new(
            ChatErrorCode::Busy,
            "This Chat command is still being processed",
            true,
        )),
        CommandReceiptState::Completed => {
            serde_json::from_value(receipt.result.ok_or_else(corrupt_data_error)?.value)
                .map_err(json_error)
        }
        CommandReceiptState::Failed => Err(serde_json::from_value(
            receipt.error.ok_or_else(corrupt_data_error)?.value,
        )
        .map_err(json_error)?),
    }
}

pub async fn complete_driver_operation(
    pool: &SqlitePool,
    command_id: &ChatCommandId,
    result: ChatResult<DriverOperationReceipt>,
) -> ChatResult<DriverOperationReceipt> {
    let now = now_timestamp()?;
    match result {
        Ok(receipt) => {
            let value = versioned_value(&receipt)?;
            complete_command_receipt(
                pool,
                command_id,
                CommandReceiptState::Completed,
                Some(&value),
                None,
                &now,
            )
            .await?;
            Ok(receipt)
        }
        Err(error) => {
            let value = versioned_value(&error)?;
            complete_command_receipt(
                pool,
                command_id,
                CommandReceiptState::Failed,
                None,
                Some(&value),
                &now,
            )
            .await?;
            Err(error)
        }
    }
}

fn versioned_value<T: serde::Serialize>(value: &T) -> ChatResult<VersionedJson> {
    Ok(VersionedJson {
        schema_version: 1,
        value: serde_json::to_value(value).map_err(json_error)?,
    })
}

fn now_timestamp() -> ChatResult<UtcTimestamp> {
    let now: chrono::DateTime<Utc> = std::time::SystemTime::now().into();
    UtcTimestamp::new(now.to_rfc3339_opts(SecondsFormat::Millis, true))
        .map_err(|_| ChatError::new(ChatErrorCode::Internal, "create Chat timestamp", false))
}

fn json_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat command receipt is invalid",
        false,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat command receipt is incomplete",
        false,
    )
}

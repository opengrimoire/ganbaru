use crate::models::{
    ChatCommandId, ChatError, ChatErrorCode, ChatResult, ChatThreadId, UtcTimestamp, VersionedJson,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandReceiptState {
    Accepted,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceiptRead {
    pub client_command_id: ChatCommandId,
    pub thread_id: ChatThreadId,
    pub command_kind: String,
    pub submitted_revision: Option<u64>,
    pub state: CommandReceiptState,
    pub result: Option<VersionedJson>,
    pub error: Option<VersionedJson>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommandReceiptClaim {
    Claimed(CommandReceiptRead),
    Replay(CommandReceiptRead),
}

pub async fn claim_command_receipt(
    pool: &SqlitePool,
    client_command_id: &ChatCommandId,
    thread_id: &ChatThreadId,
    command_kind: &str,
    submitted_revision: Option<u64>,
    now: &UtcTimestamp,
) -> ChatResult<CommandReceiptClaim> {
    validate_command_kind(command_kind)?;
    let mut transaction = pool.begin().await.map_err(persistence_error)?;
    if let Some(existing) = read_receipt_from_executor(&mut *transaction, client_command_id).await?
    {
        if existing.thread_id != *thread_id
            || existing.command_kind != command_kind
            || existing.submitted_revision != submitted_revision
        {
            return Err(ChatError::new(
                ChatErrorCode::Conflict,
                "Chat command ID was already used for a different operation",
                false,
            ));
        }
        transaction.commit().await.map_err(persistence_error)?;
        return Ok(CommandReceiptClaim::Replay(existing));
    }
    if let Some(expected_revision) = submitted_revision {
        let current_revision: Option<i64> =
            sqlx::query_scalar("SELECT revision FROM chat_threads WHERE id = ?")
                .bind(thread_id.as_str())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(persistence_error)?;
        let current_revision = current_revision.ok_or_else(not_found_error)?;
        if u64::try_from(current_revision).map_err(|_| corrupt_data_error())? != expected_revision {
            return Err(ChatError::new(
                ChatErrorCode::StaleRevision,
                "Chat thread revision is stale",
                true,
            ));
        }
    }
    sqlx::query(
        "INSERT INTO chat_command_receipts
            (client_command_id, thread_id, command_kind, submitted_revision,
             state, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'accepted', ?, ?)",
    )
    .bind(client_command_id.as_str())
    .bind(thread_id.as_str())
    .bind(command_kind)
    .bind(submitted_revision.map(i64_value).transpose()?)
    .bind(now.as_str())
    .bind(now.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(persistence_error)?;
    let receipt = read_receipt_from_executor(&mut *transaction, client_command_id)
        .await?
        .ok_or_else(corrupt_data_error)?;
    transaction.commit().await.map_err(persistence_error)?;
    Ok(CommandReceiptClaim::Claimed(receipt))
}

pub async fn complete_command_receipt(
    pool: &SqlitePool,
    client_command_id: &ChatCommandId,
    state: CommandReceiptState,
    result: Option<&VersionedJson>,
    error: Option<&VersionedJson>,
    now: &UtcTimestamp,
) -> ChatResult<CommandReceiptRead> {
    if state == CommandReceiptState::Accepted || (result.is_some() && error.is_some()) {
        return Err(ChatError::validation(
            "receipt.state",
            "Completed command receipts require exactly one terminal outcome",
        ));
    }
    if state == CommandReceiptState::Completed && result.is_none()
        || state == CommandReceiptState::Failed && error.is_none()
    {
        return Err(ChatError::validation(
            "receipt.outcome",
            "Command receipt outcome does not match its state",
        ));
    }
    let result = versioned_parts(result)?;
    let error = versioned_parts(error)?;
    let updated = sqlx::query(
        "UPDATE chat_command_receipts
         SET state = ?, result_schema_version = ?, result_data = ?,
             error_schema_version = ?, error_data = ?, updated_at = ?
         WHERE client_command_id = ? AND state = 'accepted'",
    )
    .bind(wire_state(state))
    .bind(result.0)
    .bind(result.1)
    .bind(error.0)
    .bind(error.1)
    .bind(now.as_str())
    .bind(client_command_id.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    if updated.rows_affected() != 1 {
        let existing = read_command_receipt(pool, client_command_id).await?;
        return existing.ok_or_else(not_found_error);
    }
    read_command_receipt(pool, client_command_id)
        .await?
        .ok_or_else(corrupt_data_error)
}

pub async fn read_command_receipt(
    pool: &SqlitePool,
    client_command_id: &ChatCommandId,
) -> ChatResult<Option<CommandReceiptRead>> {
    read_receipt_from_executor(pool, client_command_id).await
}

async fn read_receipt_from_executor<'e, E>(
    executor: E,
    client_command_id: &ChatCommandId,
) -> ChatResult<Option<CommandReceiptRead>>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let row = sqlx::query(
        "SELECT client_command_id, thread_id, command_kind, submitted_revision,
                state, result_schema_version, result_data, error_schema_version,
                error_data, created_at, updated_at
         FROM chat_command_receipts WHERE client_command_id = ?",
    )
    .bind(client_command_id.as_str())
    .fetch_optional(executor)
    .await
    .map_err(persistence_error)?;
    row.map(row_to_receipt).transpose()
}

fn row_to_receipt(row: sqlx::sqlite::SqliteRow) -> ChatResult<CommandReceiptRead> {
    let state: String = row.try_get("state").map_err(persistence_error)?;
    Ok(CommandReceiptRead {
        client_command_id: ChatCommandId::new(string_column(&row, "client_command_id")?)
            .map_err(|_| corrupt_data_error())?,
        thread_id: ChatThreadId::new(string_column(&row, "thread_id")?)
            .map_err(|_| corrupt_data_error())?,
        command_kind: string_column(&row, "command_kind")?,
        submitted_revision: row
            .try_get::<Option<i64>, _>("submitted_revision")
            .map_err(persistence_error)?
            .map(|value| u64::try_from(value).map_err(|_| corrupt_data_error()))
            .transpose()?,
        state: match state.as_str() {
            "accepted" => CommandReceiptState::Accepted,
            "completed" => CommandReceiptState::Completed,
            "failed" => CommandReceiptState::Failed,
            _ => return Err(corrupt_data_error()),
        },
        result: read_versioned(&row, "result_schema_version", "result_data")?,
        error: read_versioned(&row, "error_schema_version", "error_data")?,
        created_at: UtcTimestamp::new(string_column(&row, "created_at")?)
            .map_err(|_| corrupt_data_error())?,
        updated_at: UtcTimestamp::new(string_column(&row, "updated_at")?)
            .map_err(|_| corrupt_data_error())?,
    })
}

fn validate_command_kind(value: &str) -> ChatResult<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err(ChatError::validation(
            "commandKind",
            "Chat command kind is invalid",
        ));
    }
    Ok(())
}

fn wire_state(state: CommandReceiptState) -> &'static str {
    match state {
        CommandReceiptState::Accepted => "accepted",
        CommandReceiptState::Completed => "completed",
        CommandReceiptState::Failed => "failed",
    }
}

fn versioned_parts(value: Option<&VersionedJson>) -> ChatResult<(Option<i64>, Option<String>)> {
    value
        .map(|value| {
            Ok((
                Some(i64::from(value.schema_version)),
                Some(serde_json::to_string(&value.value).map_err(serialization_error)?),
            ))
        })
        .unwrap_or(Ok((None, None)))
}

fn read_versioned(
    row: &sqlx::sqlite::SqliteRow,
    version_column: &str,
    data_column: &str,
) -> ChatResult<Option<VersionedJson>> {
    match (
        row.try_get::<Option<i64>, _>(version_column)
            .map_err(persistence_error)?,
        row.try_get::<Option<String>, _>(data_column)
            .map_err(persistence_error)?,
    ) {
        (None, None) => Ok(None),
        (Some(version), Some(data)) => Ok(Some(VersionedJson {
            schema_version: u32::try_from(version).map_err(|_| corrupt_data_error())?,
            value: serde_json::from_str(&data).map_err(serialization_error)?,
        })),
        _ => Err(corrupt_data_error()),
    }
}

fn string_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<String> {
    row.try_get(column).map_err(persistence_error)
}

fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| {
        ChatError::validation(
            "revision",
            "Chat revision exceeds the supported storage range",
        )
    })
}

fn not_found_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::NotFound,
        "Chat command receipt was not found",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat command receipt persistence failed",
        true,
    )
}

fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat command receipt could not be serialized",
        false,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat command receipt is invalid",
        false,
    )
}

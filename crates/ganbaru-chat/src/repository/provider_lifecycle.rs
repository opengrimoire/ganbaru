use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProviderFamilyId, ProviderInstanceId,
    ProviderThreadId, UtcTimestamp,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLifecycleOperation {
    Rename,
    Archive,
    Delete,
    Unsubscribe,
    Cleanup,
}

impl ProviderLifecycleOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Archive => "archive",
            Self::Delete => "delete",
            Self::Unsubscribe => "unsubscribe",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderLifecycleJobRead {
    pub id: String,
    pub thread_id: Option<ChatThreadId>,
    pub provider_family_id: ProviderFamilyId,
    pub provider_instance_id: ProviderInstanceId,
    pub provider_thread_id: ProviderThreadId,
    pub operation: ProviderLifecycleOperation,
    pub payload: Value,
    pub state: String,
    pub attempt_count: u64,
    pub last_error_code: Option<String>,
    pub last_error_detail: Option<String>,
    pub retry_after: Option<UtcTimestamp>,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
}

pub struct EnqueueProviderLifecycleFailure<'a> {
    pub thread_id: Option<&'a ChatThreadId>,
    pub provider_family_id: &'a ProviderFamilyId,
    pub provider_instance_id: &'a ProviderInstanceId,
    pub provider_thread_id: &'a ProviderThreadId,
    pub operation: ProviderLifecycleOperation,
    pub payload: &'a Value,
    pub error: &'a ChatError,
    pub now: &'a UtcTimestamp,
}

pub async fn enqueue_failure(
    pool: &SqlitePool,
    request: EnqueueProviderLifecycleFailure<'_>,
) -> ChatResult<()> {
    let payload = serde_json::to_string(request.payload).map_err(serialization_error)?;
    let error_detail = truncate_utf8(&request.error.message, 2_000);
    sqlx::query(
        "INSERT INTO chat_provider_cleanup_jobs (
            id, thread_id, provider_family_id, provider_instance_id, provider_thread_id,
            operation, payload_data, state, attempt_count, last_error_code,
            last_error_detail, retry_after, created_at, updated_at
         ) VALUES (
            lower(hex(randomblob(16))), ?, ?, ?, ?, ?, ?, 'failed', 1, ?, ?, ?, ?, ?
         )",
    )
    .bind(request.thread_id.map(ChatThreadId::as_str))
    .bind(request.provider_family_id.as_str())
    .bind(request.provider_instance_id.as_str())
    .bind(request.provider_thread_id.as_str())
    .bind(request.operation.as_str())
    .bind(payload)
    .bind(error_code(request.error.code))
    .bind(error_detail)
    .bind(request.now.as_str())
    .bind(request.now.as_str())
    .bind(request.now.as_str())
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(())
}

pub async fn list_retryable(
    pool: &SqlitePool,
    thread_id: Option<&ChatThreadId>,
) -> ChatResult<Vec<ProviderLifecycleJobRead>> {
    let rows = sqlx::query(
        "SELECT id, thread_id, provider_family_id, provider_instance_id, provider_thread_id,
                operation, payload_data, state, attempt_count, last_error_code,
                last_error_detail, retry_after, created_at, updated_at
         FROM chat_provider_cleanup_jobs
         WHERE state IN ('queued', 'failed') AND (? IS NULL OR thread_id = ?)
         ORDER BY created_at, id",
    )
    .bind(thread_id.map(ChatThreadId::as_str))
    .bind(thread_id.map(ChatThreadId::as_str))
    .fetch_all(pool)
    .await
    .map_err(persistence_error)?;
    rows.into_iter().map(parse_job).collect()
}

fn parse_job(row: sqlx::sqlite::SqliteRow) -> ChatResult<ProviderLifecycleJobRead> {
    Ok(ProviderLifecycleJobRead {
        id: row.try_get("id").map_err(persistence_error)?,
        thread_id: row
            .try_get::<Option<String>, _>("thread_id")
            .map_err(persistence_error)?
            .map(ChatThreadId::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        provider_family_id: ProviderFamilyId::new(
            row.try_get::<String, _>("provider_family_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        provider_instance_id: ProviderInstanceId::new(
            row.try_get::<String, _>("provider_instance_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        provider_thread_id: ProviderThreadId::new(
            row.try_get::<String, _>("provider_thread_id")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        operation: parse_operation(
            &row.try_get::<String, _>("operation")
                .map_err(persistence_error)?,
        )?,
        payload: serde_json::from_str(
            &row.try_get::<String, _>("payload_data")
                .map_err(persistence_error)?,
        )
        .map_err(serialization_error)?,
        state: row.try_get("state").map_err(persistence_error)?,
        attempt_count: u64::try_from(
            row.try_get::<i64, _>("attempt_count")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        last_error_code: row.try_get("last_error_code").map_err(persistence_error)?,
        last_error_detail: row
            .try_get("last_error_detail")
            .map_err(persistence_error)?,
        retry_after: row
            .try_get::<Option<String>, _>("retry_after")
            .map_err(persistence_error)?
            .map(UtcTimestamp::new)
            .transpose()
            .map_err(|_| corrupt_data_error())?,
        created_at: UtcTimestamp::new(
            row.try_get::<String, _>("created_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
        updated_at: UtcTimestamp::new(
            row.try_get::<String, _>("updated_at")
                .map_err(persistence_error)?,
        )
        .map_err(|_| corrupt_data_error())?,
    })
}

fn parse_operation(value: &str) -> ChatResult<ProviderLifecycleOperation> {
    match value {
        "rename" => Ok(ProviderLifecycleOperation::Rename),
        "archive" => Ok(ProviderLifecycleOperation::Archive),
        "delete" => Ok(ProviderLifecycleOperation::Delete),
        "unsubscribe" => Ok(ProviderLifecycleOperation::Unsubscribe),
        "cleanup" => Ok(ProviderLifecycleOperation::Cleanup),
        _ => Err(corrupt_data_error()),
    }
}

fn error_code(code: ChatErrorCode) -> &'static str {
    match code {
        ChatErrorCode::Validation => "validation",
        ChatErrorCode::NotFound => "not_found",
        ChatErrorCode::Conflict => "conflict",
        ChatErrorCode::StaleRevision => "stale_revision",
        ChatErrorCode::InvalidStateTransition => "invalid_state_transition",
        ChatErrorCode::Busy => "busy",
        ChatErrorCode::CapabilityUnsupported => "capability_unsupported",
        ChatErrorCode::DriverUnavailable => "driver_unavailable",
        ChatErrorCode::ExecutableMissing => "executable_missing",
        ChatErrorCode::UnsupportedVersion => "unsupported_version",
        ChatErrorCode::AuthenticationRequired => "authentication_required",
        ChatErrorCode::ConfigurationInvalid => "configuration_invalid",
        ChatErrorCode::TransportUnavailable => "transport_unavailable",
        ChatErrorCode::ResumeNotFound => "resume_not_found",
        ChatErrorCode::Protocol => "protocol",
        ChatErrorCode::Permission => "permission",
        ChatErrorCode::Timeout => "timeout",
        ChatErrorCode::Cancelled => "cancelled",
        ChatErrorCode::Persistence => "persistence",
        ChatErrorCode::Internal => "internal",
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn persistence_error(_: sqlx::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Provider lifecycle synchronization could not be persisted",
        true,
    )
}

fn serialization_error(_: serde_json::Error) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Provider lifecycle synchronization data is invalid",
        true,
    )
}

fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored provider lifecycle synchronization data is invalid",
        true,
    )
}

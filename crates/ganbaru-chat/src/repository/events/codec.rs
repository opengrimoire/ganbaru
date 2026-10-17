use super::*;

pub(super) fn row_to_event(row: sqlx::sqlite::SqliteRow) -> ChatResult<CanonicalStoredEvent> {
    let schema_version = u32_column(&row, "event_schema_version")?;
    let event: CanonicalEvent =
        serde_json::from_str(row.try_get("payload_data").map_err(persistence_error)?)
            .map_err(serialization_error)?;
    let runtime = CanonicalRuntimeEvent {
        schema_version,
        event_id: crate::models::ChatEventId::new(string_column(&row, "id")?)
            .map_err(|_| corrupt_data_error())?,
        provider_family_id: crate::models::ProviderFamilyId::new(string_column(
            &row,
            "provider_family_id",
        )?)
        .map_err(|_| corrupt_data_error())?,
        provider_instance_id: crate::models::ProviderInstanceId::new(string_column(
            &row,
            "provider_instance_id",
        )?)
        .map_err(|_| corrupt_data_error())?,
        thread_id: ChatThreadId::new(string_column(&row, "thread_id")?)
            .map_err(|_| corrupt_data_error())?,
        created_at: UtcTimestamp::new(string_column(&row, "created_at")?)
            .map_err(|_| corrupt_data_error())?,
        turn_id: optional_identifier(&row, "turn_id", crate::models::ChatTurnId::new)?,
        provider_turn_id: optional_identifier(
            &row,
            "provider_turn_id",
            crate::models::ProviderTurnId::new,
        )?,
        provider_item_id: optional_identifier(
            &row,
            "provider_item_id",
            crate::models::ProviderItemId::new,
        )?,
        provider_request_id: optional_identifier(
            &row,
            "provider_request_id",
            crate::models::ProviderRequestId::new,
        )?,
        provider_task_id: optional_identifier(
            &row,
            "provider_task_id",
            crate::models::ProviderTaskId::new,
        )?,
        provider_reference: read_versioned(
            &row,
            "provider_reference_schema_version",
            "provider_reference_data",
        )?,
        event,
        redacted_diagnostic: read_versioned(
            &row,
            "redacted_diagnostic_schema_version",
            "redacted_diagnostic_data",
        )?,
    };
    Ok(CanonicalStoredEvent {
        sequence: u64_column(&row, "sequence")?,
        ingested_at: UtcTimestamp::new(string_column(&row, "ingested_at")?)
            .map_err(|_| corrupt_data_error())?,
        runtime,
    })
}

pub(super) fn required_turn_id(runtime: &CanonicalRuntimeEvent) -> ChatResult<&str> {
    runtime
        .turn_id
        .as_ref()
        .map(|value| value.as_str())
        .ok_or_else(|| ChatError::validation("event.turnId", "Turn event requires a turn ID"))
}

pub(super) fn event_type(event: &CanonicalEvent) -> ChatResult<String> {
    serde_json::to_value(event)
        .map_err(serialization_error)?
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(corrupt_data_error)
}

pub(super) fn wire_literal<T: Serialize>(value: &T) -> ChatResult<String> {
    serde_json::to_value(value)
        .map_err(serialization_error)?
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(corrupt_data_error)
}

pub(super) fn versioned_parts(
    value: Option<&VersionedJson>,
) -> ChatResult<(Option<i64>, Option<String>)> {
    value
        .map(|value| {
            Ok((
                Some(i64::from(value.schema_version)),
                Some(serde_json::to_string(&value.value).map_err(serialization_error)?),
            ))
        })
        .unwrap_or(Ok((None, None)))
}

pub(super) fn read_versioned(
    row: &sqlx::sqlite::SqliteRow,
    version_column: &str,
    data_column: &str,
) -> ChatResult<Option<VersionedJson>> {
    let version: Option<i64> = row.try_get(version_column).map_err(persistence_error)?;
    let data: Option<String> = row.try_get(data_column).map_err(persistence_error)?;
    match (version, data) {
        (None, None) => Ok(None),
        (Some(version), Some(data)) => Ok(Some(VersionedJson {
            schema_version: u32::try_from(version).map_err(|_| corrupt_data_error())?,
            value: serde_json::from_str(&data).map_err(serialization_error)?,
        })),
        _ => Err(corrupt_data_error()),
    }
}

pub(super) fn optional_identifier<T>(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
    create: impl FnOnce(String) -> Result<T, String>,
) -> ChatResult<Option<T>> {
    row.try_get::<Option<String>, _>(column)
        .map_err(persistence_error)?
        .map(create)
        .transpose()
        .map_err(|_| corrupt_data_error())
}

pub(super) fn string_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<String> {
    row.try_get(column).map_err(persistence_error)
}

pub(super) fn u32_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u32> {
    let value: i64 = row.try_get(column).map_err(persistence_error)?;
    u32::try_from(value).map_err(|_| corrupt_data_error())
}

pub(super) fn u64_column(row: &sqlx::sqlite::SqliteRow, column: &str) -> ChatResult<u64> {
    let value: i64 = row.try_get(column).map_err(persistence_error)?;
    u64::try_from(value).map_err(|_| corrupt_data_error())
}

pub(super) fn i64_value(value: u64) -> ChatResult<i64> {
    i64::try_from(value).map_err(|_| {
        ChatError::validation(
            "sequence",
            "Chat sequence exceeds the supported storage range",
        )
    })
}

pub(super) fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat persistence operation failed",
        true,
    )
}

pub(super) fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Internal,
        "Chat data could not be serialized",
        false,
    )
}

pub(super) fn corrupt_data_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat data is invalid",
        false,
    )
}

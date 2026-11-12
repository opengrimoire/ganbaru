use super::*;

pub(super) async fn target_turn_id(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_count: u64,
) -> ChatResult<Option<ChatTurnId>> {
    if turn_count == 0 {
        return Ok(None);
    }
    sqlx::query_scalar::<_, String>("SELECT id FROM chat_turns WHERE thread_id = ? AND ordinal = ?")
        .bind(thread_id.as_str())
        .bind(i64_value(turn_count - 1)?)
        .fetch_optional(pool)
        .await
        .map_err(persistence_error)?
        .map(ChatTurnId::new)
        .transpose()
        .map_err(|_| corrupt_data_error())
}

/// The provider families whose drivers rewind native history, each with the JSON path of the
/// native identifier its event provider references carry and its rollback cursor names.
const ROLLBACK_CURSOR_PATHS: [(&str, &str); 2] =
    [("opencode", "$.messageId"), ("codex", "$.turnId")];

/// The provider reference of the latest valid event in `turn_id` that names a native rewind
/// point, which the provider's rollback takes as its cursor.
pub(super) async fn stored_provider_rollback_cursor(
    pool: &SqlitePool,
    thread_id: &ChatThreadId,
    turn_id: &ChatTurnId,
) -> ChatResult<Option<VersionedJson>> {
    let [(first_family, first_path), (second_family, second_path)] = ROLLBACK_CURSOR_PATHS;
    let row = sqlx::query(
        "SELECT provider_reference_schema_version, provider_reference_data
         FROM chat_events
         WHERE thread_id = ? AND turn_id = ?
           AND invalidated_at IS NULL
           AND provider_reference_schema_version IS NOT NULL
           AND provider_reference_data IS NOT NULL
           AND length(CAST(provider_reference_data AS BLOB)) <= 8192
           AND (
               (provider_family_id = ? AND json_type(provider_reference_data, ?) = 'text')
               OR (provider_family_id = ? AND json_type(provider_reference_data, ?) = 'text')
           )
         ORDER BY sequence DESC, id DESC LIMIT 1",
    )
    .bind(thread_id.as_str())
    .bind(turn_id.as_str())
    .bind(first_family)
    .bind(first_path)
    .bind(second_family)
    .bind(second_path)
    .fetch_optional(pool)
    .await
    .map_err(persistence_error)?;
    let Some(row) = row else { return Ok(None) };
    let schema_version = row
        .try_get::<Option<i64>, _>("provider_reference_schema_version")
        .map_err(persistence_error)?;
    let data = row
        .try_get::<Option<String>, _>("provider_reference_data")
        .map_err(persistence_error)?;
    stored_rollback_cursor(schema_version, data.as_deref()).map(Some)
}

pub(super) fn stored_rollback_cursor(
    schema_version: Option<i64>,
    data: Option<&str>,
) -> ChatResult<VersionedJson> {
    let schema_version = schema_version.ok_or_else(corrupt_data_error)?;
    let data = data.ok_or_else(corrupt_data_error)?;
    Ok(VersionedJson {
        schema_version: u32::try_from(schema_version).map_err(|_| corrupt_data_error())?,
        value: serde_json::from_str(data).map_err(json_error)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_opencode_cursor_preserves_native_message_and_part_ids() {
        let cursor = stored_rollback_cursor(
            Some(1),
            Some(r#"{"messageId":"message-7","partId":"part-2","source":"message.updated"}"#),
        )
        .expect("stored cursor should parse");

        assert_eq!(cursor.schema_version, 1);
        assert_eq!(cursor.value["messageId"], "message-7");
        assert_eq!(cursor.value["partId"], "part-2");
    }

    #[test]
    fn stored_codex_cursor_preserves_the_native_turn_id() {
        let cursor = stored_rollback_cursor(
            Some(1),
            Some(r#"{"method":"turn/completed","turnId":"codex-turn-7"}"#),
        )
        .expect("stored cursor should parse");

        assert_eq!(cursor.schema_version, 1);
        assert_eq!(cursor.value["turnId"], "codex-turn-7");
    }
}

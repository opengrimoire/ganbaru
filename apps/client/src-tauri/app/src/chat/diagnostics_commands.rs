//! Redacted diagnostics, retention, and disruptive Chat maintenance commands.

use super::credentials::{CredentialStore, PlatformCredentialStore};
use super::device_state::{
    ChatDiagnosticPreferences, MAX_DIAGNOSTIC_RETENTION_DAYS, read_active_device_scope,
    update_active_device_scope,
};
use super::terminal::ChatTerminalRegistry;
use chrono::{SecondsFormat, Utc};
use ganbaru_chat::repository::rebuild::rebuild_thread_projections;
use ganbaru_chat::runtime::ChatRuntimeRegistry;
use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProbeState, VersionedJson,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, FilePath};

const STOP_CONFIRMATION: &str = "STOP ALL CHAT PROCESSES";
const REBUILD_CONFIRMATION: &str = "REBUILD CHAT PROJECTIONS";
const MAINTENANCE_TIMEOUT: Duration = Duration::from_secs(8);
const DIAGNOSTIC_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDiagnosticCounts {
    pub retained_events: u64,
    pub retained_bytes: u64,
    pub attachment_count: u64,
    pub attachment_bytes: u64,
    pub pending_attachment_cleanup: u64,
    pub failed_attachment_cleanup: u64,
    pub command_output_events: u64,
    pub command_output_bytes: u64,
    pub checkpoint_failures: u64,
    pub pending_checkpoint_cleanup: u64,
    pub failed_checkpoint_cleanup: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDiagnosticsRead {
    pub preferences: ChatDiagnosticPreferences,
    pub captured_fields: Vec<&'static str>,
    pub excluded_fields: Vec<&'static str>,
    pub storage_location: &'static str,
    pub projection_healthy: bool,
    pub inconsistent_projection_count: u64,
    pub credential_store_available: bool,
    pub healthy_provider_probes: u64,
    pub unhealthy_provider_probes: u64,
    pub unknown_provider_probes: u64,
    pub live_provider_processes: u64,
    pub active_turns: u64,
    pub live_terminals: u64,
    pub counts: ChatDiagnosticCounts,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMaintenanceConfirmation {
    pub confirmation: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStopAllResult {
    pub provider_processes_stopped: u64,
    pub terminals_stopped: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRebuildResult {
    pub rebuilt_threads: u64,
}

#[tauri::command]
pub async fn chat_read_diagnostics(
    app: tauri::AppHandle,
    db_url: String,
    runtimes: tauri::State<'_, ChatRuntimeRegistry>,
    terminals: tauri::State<'_, ChatTerminalRegistry>,
) -> ChatResult<ChatDiagnosticsRead> {
    let pool = connect_pool(app.clone(), db_url).await?;
    delete_expired_diagnostics(&pool).await?;
    read_diagnostics(&app, &pool, &runtimes, &terminals).await
}

#[tauri::command]
pub fn chat_update_diagnostic_preferences(
    app: tauri::AppHandle,
    preferences: ChatDiagnosticPreferences,
) -> ChatResult<ChatDiagnosticPreferences> {
    validate_preferences(&preferences)?;
    update_active_device_scope(&app, |scope| {
        scope.diagnostics = preferences.clone();
        Ok(())
    })
    .map_err(device_state_error)?;
    Ok(preferences)
}

#[tauri::command]
pub async fn chat_delete_diagnostics(app: tauri::AppHandle, db_url: String) -> ChatResult<u64> {
    let pool = connect_pool(app, db_url).await?;
    clear_diagnostics(&pool).await
}

#[tauri::command]
pub async fn chat_export_redacted_diagnostics(
    app: tauri::AppHandle,
    db_url: String,
    picker_title: String,
) -> ChatResult<bool> {
    if picker_title.trim().is_empty()
        || picker_title.len() > 200
        || picker_title.chars().any(char::is_control)
    {
        return Err(ChatError::validation(
            "pickerTitle",
            "Diagnostic export title is invalid",
        ));
    }
    let pool = connect_pool(app.clone(), db_url).await?;
    delete_expired_diagnostics(&pool).await?;
    let export = build_redacted_export(&pool).await?;
    let Some(path) = app
        .dialog()
        .file()
        .set_title(picker_title)
        .set_file_name("ganbaru-chat-diagnostics.json")
        .add_filter("JSON", &["json"])
        .blocking_save_file()
        .map(dialog_path)
        .transpose()?
    else {
        return Ok(false);
    };
    write_restrictive(&path, export.as_bytes())?;
    Ok(true)
}

#[tauri::command]
pub async fn chat_stop_all_processes(
    app: tauri::AppHandle,
    runtimes: tauri::State<'_, ChatRuntimeRegistry>,
    mutations: tauri::State<'_, super::workspace::mutation::ChatWorkspaceMutationRegistry>,
    terminals: tauri::State<'_, ChatTerminalRegistry>,
    internal_mcp: tauri::State<'_, super::internal_mcp::InternalMcpRegistry>,
    request: ChatMaintenanceConfirmation,
) -> ChatResult<ChatStopAllResult> {
    require_confirmation(&request.confirmation, STOP_CONFIRMATION)?;
    let provider_processes_stopped = runtimes
        .stop_all_and_reset(MAINTENANCE_TIMEOUT, &mutations)
        .await?;
    let terminals_stopped = terminals.stop_all()?;
    internal_mcp.stop_all().await;
    app.state::<super::preview::ChatBrowserManager>()
        .close_all(&app);
    Ok(ChatStopAllResult {
        provider_processes_stopped,
        terminals_stopped,
    })
}

#[tauri::command]
pub async fn chat_rebuild_projections(
    app: tauri::AppHandle,
    db_url: String,
    runtimes: tauri::State<'_, ChatRuntimeRegistry>,
    request: ChatMaintenanceConfirmation,
) -> ChatResult<ChatRebuildResult> {
    require_confirmation(&request.confirmation, REBUILD_CONFIRMATION)?;
    let (_, active_turns) = runtimes.process_counts()?;
    if active_turns != 0 {
        return Err(active_turn_error());
    }
    let pool = connect_pool(app, db_url).await?;
    let database_active_turns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_turns WHERE invalidated_at IS NULL
         AND state IN ('dispatching', 'active', 'waiting_for_approval', 'waiting_for_user_input')",
    )
    .fetch_one(&pool)
    .await
    .map_err(persistence_error)?;
    if database_active_turns != 0 {
        return Err(active_turn_error());
    }
    let thread_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM chat_threads ORDER BY id")
        .fetch_all(&pool)
        .await
        .map_err(persistence_error)?;
    for thread_id in &thread_ids {
        rebuild_thread_projections(
            &pool,
            &ChatThreadId::new(thread_id.clone()).map_err(identifier_error)?,
        )
        .await?;
    }
    Ok(ChatRebuildResult {
        rebuilt_threads: u64::try_from(thread_ids.len()).unwrap_or(u64::MAX),
    })
}

pub fn attach_opt_in_diagnostic<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    event: &mut ganbaru_chat_contracts::events::CanonicalRuntimeEvent,
) -> ChatResult<Option<ganbaru_chat_contracts::models::UtcTimestamp>> {
    let preferences = read_active_device_scope(app)
        .map_err(device_state_error)?
        .diagnostics;
    if !preferences.capture_enabled {
        event.redacted_diagnostic = None;
        return Ok(None);
    }
    validate_preferences(&preferences)?;
    let event_type = serde_json::to_value(&event.event)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let protocol_envelope = redacted_protocol_envelope(event.provider_reference.as_ref());
    event.redacted_diagnostic = Some(VersionedJson {
        schema_version: DIAGNOSTIC_SCHEMA_VERSION,
        value: json!({
            "providerFamilyId": event.provider_family_id.as_str(),
            "providerInstanceId": event.provider_instance_id.as_str(),
            "threadId": event.thread_id.as_str(),
            "turnId": event.turn_id.as_ref().map(|value| value.as_str()),
            "eventType": event_type,
            "createdAt": event.created_at.as_str(),
            "protocolEnvelope": protocol_envelope,
        }),
    });
    let expires = utc_now()
        .checked_add_signed(chrono::Duration::days(i64::from(
            preferences.retention_days,
        )))
        .ok_or_else(|| {
            ChatError::new(ChatErrorCode::Internal, "create diagnostic expiry", false)
        })?;
    ganbaru_chat_contracts::models::UtcTimestamp::new(
        expires.to_rfc3339_opts(SecondsFormat::Millis, true),
    )
    .map(Some)
    .map_err(identifier_error)
}

async fn read_diagnostics(
    app: &tauri::AppHandle,
    pool: &SqlitePool,
    runtimes: &ChatRuntimeRegistry,
    terminals: &ChatTerminalRegistry,
) -> ChatResult<ChatDiagnosticsRead> {
    let scope = read_active_device_scope(app).map_err(device_state_error)?;
    let inconsistent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_threads t WHERE t.last_projected_sequence != COALESCE(
            (SELECT MAX(e.sequence) FROM chat_events e
             WHERE e.thread_id = t.id AND e.invalidated_at IS NULL), 0)",
    )
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let diagnostic_row = sqlx::query(
        "SELECT COUNT(*) AS count, COALESCE(SUM(length(redacted_diagnostic_data)), 0) AS bytes
         FROM chat_events WHERE redacted_diagnostic_data IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let attachment_row = sqlx::query(
        "SELECT COUNT(*) AS count, COALESCE(SUM(byte_size), 0) AS bytes
         FROM chat_attachments WHERE deletion_state != 'deleted'",
    )
    .fetch_one(pool)
    .await
    .map_err(persistence_error)?;
    let output_row = sqlx::query(
        "SELECT COUNT(*) AS count, COALESCE(SUM(length(json_extract(payload_data, '$.payload.delta'))), 0) AS bytes
         FROM chat_events WHERE event_type = 'content_delta'
           AND json_extract(payload_data, '$.payload.streamKind') IN ('command_output', 'file_change_output')",
    ).fetch_one(pool).await.map_err(persistence_error)?;
    let checkpoint_failures: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chat_checkpoint_failures")
            .fetch_one(pool)
            .await
            .map_err(persistence_error)?;
    let cleanup_row = sqlx::query(
        "SELECT
            COALESCE(SUM(CASE WHEN cleanup_kind = 'checkpoint_ref' AND state IN ('pending', 'running') THEN 1 ELSE 0 END), 0) AS pending,
            COALESCE(SUM(CASE WHEN cleanup_kind = 'checkpoint_ref' AND state = 'failed' THEN 1 ELSE 0 END), 0) AS failed,
            COALESCE(SUM(CASE WHEN cleanup_kind = 'attachment_file' AND state IN ('pending', 'running') THEN 1 ELSE 0 END), 0) AS attachment_pending,
            COALESCE(SUM(CASE WHEN cleanup_kind = 'attachment_file' AND state = 'failed' THEN 1 ELSE 0 END), 0) AS attachment_failed
         FROM chat_cleanup_jobs",
    ).fetch_one(pool).await.map_err(persistence_error)?;
    let (live_provider_processes, active_turns) = runtimes.process_counts()?;
    let mut healthy_provider_probes = 0u64;
    let mut unhealthy_provider_probes = 0u64;
    let mut unknown_provider_probes = 0u64;
    for provider in scope.provider_instances.values() {
        match provider.last_probe.as_ref().map(|probe| probe.state) {
            Some(ProbeState::Healthy) => healthy_provider_probes += 1,
            Some(_) => unhealthy_provider_probes += 1,
            None => unknown_provider_probes += 1,
        }
    }
    Ok(ChatDiagnosticsRead {
        preferences: scope.diagnostics,
        captured_fields: vec![
            "provider_family",
            "provider_instance",
            "thread_id",
            "turn_id",
            "event_type",
            "protocol_label",
            "timestamp",
        ],
        excluded_fields: vec![
            "prompts_responses",
            "tool_content",
            "paths",
            "environment_credentials",
        ],
        storage_location: "active_vault_sqlite",
        projection_healthy: inconsistent == 0,
        inconsistent_projection_count: count(inconsistent)?,
        credential_store_available: matches!(
            PlatformCredentialStore::default().availability(),
            super::credentials::CredentialStoreAvailability::Available
        ),
        healthy_provider_probes,
        unhealthy_provider_probes,
        unknown_provider_probes,
        live_provider_processes: u64::try_from(live_provider_processes).unwrap_or(u64::MAX),
        active_turns: u64::try_from(active_turns).unwrap_or(u64::MAX),
        live_terminals: u64::try_from(terminals.live_count()?).unwrap_or(u64::MAX),
        counts: ChatDiagnosticCounts {
            retained_events: row_count(&diagnostic_row, "count")?,
            retained_bytes: row_count(&diagnostic_row, "bytes")?,
            attachment_count: row_count(&attachment_row, "count")?,
            attachment_bytes: row_count(&attachment_row, "bytes")?,
            pending_attachment_cleanup: row_count(&cleanup_row, "attachment_pending")?,
            failed_attachment_cleanup: row_count(&cleanup_row, "attachment_failed")?,
            command_output_events: row_count(&output_row, "count")?,
            command_output_bytes: row_count(&output_row, "bytes")?,
            checkpoint_failures: count(checkpoint_failures)?,
            pending_checkpoint_cleanup: row_count(&cleanup_row, "pending")?,
            failed_checkpoint_cleanup: row_count(&cleanup_row, "failed")?,
        },
    })
}

async fn build_redacted_export(pool: &SqlitePool) -> ChatResult<String> {
    let rows = sqlx::query(
        "SELECT provider_family_id, provider_instance_id, event_type, created_at,
                redacted_diagnostic_schema_version, redacted_diagnostic_data, diagnostic_expires_at
         FROM chat_events WHERE redacted_diagnostic_data IS NOT NULL ORDER BY sequence, id LIMIT 10000",
    ).fetch_all(pool).await.map_err(persistence_error)?;
    let events = rows.into_iter().map(|row| {
        let data: String = row.try_get("redacted_diagnostic_data").map_err(persistence_error)?;
        let diagnostic: Value = serde_json::from_str(&data).map_err(serialization_error)?;
        Ok(json!({
            "providerFamilyId": row.try_get::<String, _>("provider_family_id").map_err(persistence_error)?,
            "providerInstanceId": row.try_get::<String, _>("provider_instance_id").map_err(persistence_error)?,
            "eventType": row.try_get::<String, _>("event_type").map_err(persistence_error)?,
            "createdAt": row.try_get::<String, _>("created_at").map_err(persistence_error)?,
            "diagnosticSchemaVersion": row.try_get::<i64, _>("redacted_diagnostic_schema_version").map_err(persistence_error)?,
            "diagnostic": diagnostic,
            "expiresAt": row.try_get::<String, _>("diagnostic_expires_at").map_err(persistence_error)?,
        }))
    }).collect::<ChatResult<Vec<_>>>()?;
    serde_json::to_string_pretty(&json!({
        "schemaVersion": 1,
        "redacted": true,
        "excluded": ["prompts", "responses", "tool arguments", "command output", "paths", "environment", "credentials"],
        "events": events,
    })).map_err(serialization_error)
}

async fn delete_expired_diagnostics(pool: &SqlitePool) -> ChatResult<u64> {
    let result = sqlx::query(
        "UPDATE chat_events SET redacted_diagnostic_schema_version = NULL,
            redacted_diagnostic_data = NULL, diagnostic_expires_at = NULL
         WHERE diagnostic_expires_at IS NOT NULL AND diagnostic_expires_at <= ?",
    )
    .bind(utc_now().to_rfc3339_opts(SecondsFormat::Millis, true))
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

pub(crate) async fn prune_expired_diagnostics(pool: &SqlitePool) -> ChatResult<u64> {
    delete_expired_diagnostics(pool).await
}

async fn clear_diagnostics(pool: &SqlitePool) -> ChatResult<u64> {
    let result = sqlx::query(
        "UPDATE chat_events SET redacted_diagnostic_schema_version = NULL,
            redacted_diagnostic_data = NULL, diagnostic_expires_at = NULL
         WHERE redacted_diagnostic_data IS NOT NULL",
    )
    .execute(pool)
    .await
    .map_err(persistence_error)?;
    Ok(result.rows_affected())
}

fn validate_preferences(preferences: &ChatDiagnosticPreferences) -> ChatResult<()> {
    if !(1..=MAX_DIAGNOSTIC_RETENTION_DAYS).contains(&preferences.retention_days) {
        return Err(ChatError::validation(
            "retentionDays",
            "Diagnostic retention must be between 1 and 30 days",
        ));
    }
    Ok(())
}

fn redacted_protocol_envelope(reference: Option<&VersionedJson>) -> Value {
    let Some(fields) = reference.and_then(|reference| reference.value.as_object()) else {
        return Value::Null;
    };
    let retained = ["method", "source", "type"]
        .into_iter()
        .filter_map(|key| {
            fields.get(key).and_then(Value::as_str).and_then(|value| {
                let normalized = value.to_ascii_lowercase();
                let safe = value.len() <= 256
                    && !value.chars().any(char::is_control)
                    && !normalized.contains("bearer ")
                    && !normalized.contains("/home/")
                    && !normalized.contains("\\users\\");
                safe.then(|| (key.to_string(), Value::String(value.to_string())))
            })
        })
        .collect();
    Value::Object(retained)
}

fn require_confirmation(value: &str, expected: &str) -> ChatResult<()> {
    if value != expected {
        return Err(ChatError::validation(
            "confirmation",
            "The exact maintenance confirmation is required",
        ));
    }
    Ok(())
}

fn row_count(row: &sqlx::sqlite::SqliteRow, field: &str) -> ChatResult<u64> {
    count(row.try_get::<i64, _>(field).map_err(persistence_error)?)
}

fn count(value: i64) -> ChatResult<u64> {
    u64::try_from(value).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Persistence,
            "Chat diagnostic count is invalid",
            false,
        )
    })
}

async fn connect_pool(app: tauri::AppHandle, db_url: String) -> ChatResult<SqlitePool> {
    crate::db::connect_sqlite(app, db_url)
        .await
        .map_err(persistence_error)
}

fn dialog_path(path: FilePath) -> ChatResult<std::path::PathBuf> {
    path.into_path()
        .map_err(|_| ChatError::validation("path", "Diagnostic export requires a local file path"))
}

fn write_restrictive(path: &Path, bytes: &[u8]) -> ChatResult<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn active_turn_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Conflict,
        "Stop active Chat turns before rebuilding projections",
        true,
    )
}

fn persistence_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat diagnostics database operation failed",
        true,
    )
}

fn serialization_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Redacted Chat diagnostics could not be encoded",
        false,
    )
}

fn device_state_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Chat diagnostic preferences could not be read",
        true,
    )
}

fn identifier_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Stored Chat diagnostic identifier is invalid",
        false,
    )
}

fn io_error<T>(_error: T) -> ChatError {
    ChatError::new(
        ChatErrorCode::Persistence,
        "Redacted Chat diagnostics could not be written",
        true,
    )
}

fn utc_now() -> chrono::DateTime<Utc> {
    std::time::SystemTime::now().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENTINEL_SECRET: &str = "ganbaru-chat-secret-sentinel-diagnostics-93c7";

    #[test]
    fn maintenance_confirmations_and_retention_are_exact() {
        assert!(require_confirmation(STOP_CONFIRMATION, STOP_CONFIRMATION).is_ok());
        assert!(require_confirmation("stop all chat processes", STOP_CONFIRMATION).is_err());
        assert!(
            validate_preferences(&ChatDiagnosticPreferences {
                capture_enabled: true,
                retention_days: 1
            })
            .is_ok()
        );
        assert!(
            validate_preferences(&ChatDiagnosticPreferences {
                capture_enabled: true,
                retention_days: 31
            })
            .is_err()
        );
    }

    #[test]
    fn protocol_diagnostic_keeps_only_bounded_allowlisted_metadata() {
        let reference = VersionedJson {
            schema_version: 1,
            value: json!({
                "method": "item/commandExecution/outputDelta",
                "payload": SENTINEL_SECRET,
                "authorization": format!("Bearer {SENTINEL_SECRET}"),
                "source": "/home/person/private",
            }),
        };
        let redacted = redacted_protocol_envelope(Some(&reference)).to_string();
        assert!(redacted.contains("item/commandExecution/outputDelta"));
        assert!(!redacted.contains(SENTINEL_SECRET));
        assert!(!redacted.contains("/home/"));
    }

    #[test]
    fn redacted_export_omits_payloads_paths_and_sentinel_secrets() {
        tauri::async_runtime::block_on(async {
            let pool = crate::chat::tests::repository::pool_with_thread().await;
            sqlx::query(
                "INSERT INTO chat_events
                    (id, thread_id, sequence, event_schema_version, provider_family_id,
                     provider_instance_id, event_type, payload_schema_version, payload_data,
                     provider_reference_schema_version, provider_reference_data, created_at,
                     ingested_at, redacted_diagnostic_schema_version,
                     redacted_diagnostic_data, diagnostic_expires_at)
                 VALUES ('diagnostic-event', 'thread-1', 1, 1, 'codex', 'codex-personal',
                     'runtime_warning', 1, ?, 1, ?, ?, ?, 1, ?, ?)",
            )
            .bind(serde_json::json!({ "type": "runtime_warning", "payload": { "detail": "ordinary provider content" } }).to_string())
            .bind(serde_json::json!({ "path": "/home/person/private" }).to_string())
            .bind("2026-07-21T12:00:00Z")
            .bind("2026-07-21T12:00:00Z")
            .bind(serde_json::json!({ "eventType": "runtime_warning", "providerFamilyId": "codex" }).to_string())
            .bind("2099-07-21T12:00:00Z")
            .execute(&pool)
            .await
            .unwrap();
            let export = build_redacted_export(&pool).await.unwrap();
            assert!(export.contains("runtime_warning"));
            for prohibited in [
                SENTINEL_SECRET,
                "ordinary provider content",
                "/home/person/private",
            ] {
                assert!(!export.contains(prohibited));
            }
            sqlx::query(
                "UPDATE chat_events SET diagnostic_expires_at = '2000-01-01T00:00:00Z'
                 WHERE id = 'diagnostic-event'",
            )
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(delete_expired_diagnostics(&pool).await.unwrap(), 1);
            sqlx::query(
                "UPDATE chat_events SET redacted_diagnostic_schema_version = 1,
                    redacted_diagnostic_data = ?, diagnostic_expires_at = '2099-07-21T12:00:00Z'
                 WHERE id = 'diagnostic-event'",
            )
            .bind(serde_json::json!({ "eventType": "runtime_warning" }).to_string())
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(clear_diagnostics(&pool).await.unwrap(), 1);
            let remaining: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM chat_events WHERE redacted_diagnostic_data IS NOT NULL",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(remaining, 0);
        });
    }
}

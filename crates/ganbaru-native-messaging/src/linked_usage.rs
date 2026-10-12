//! Durable desktop usage spool shared by the browser host and Tauri process.

use serde::Deserialize;
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;

const APP_STATE_FILE: &str = "app-state.json";
const OWNERSHIP_STATE_FILE: &str = "vault-ownership.json";
const SPOOL_FILE: &str = "distractions-device-spool.sqlite";
const MAX_PENDING_SAMPLES: i64 = 4_000;

#[derive(Debug)]
pub(super) struct UsageSample {
    pub id: String,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
    pub created_at_ms: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceState {
    device_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VaultManifest {
    vault_id: String,
}

pub(super) fn record_usage_sample(
    config_dir: Option<&Path>,
    vault_path: Option<&Path>,
    sample: UsageSample,
) -> Result<(), String> {
    let config_dir =
        config_dir.ok_or_else(|| "Ganbaru AI device storage is unavailable".to_string())?;
    let vault_path =
        vault_path.ok_or_else(|| "active Ganbaru AI folder is unavailable".to_string())?;
    let state: DeviceState = read_json(&config_dir.join(APP_STATE_FILE), "device state")?;
    let device_id = state
        .device_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Ganbaru AI device identity is unavailable".to_string())?;
    let manifest: VaultManifest = read_json(&vault_path.join("vault.json"), "vault identity")?;
    if manifest.vault_id.trim().is_empty() {
        return Err("active vault identity is unavailable".to_string());
    }
    let spool_path = config_dir.join(SPOOL_FILE);
    super::block_on(async move {
        let options = SqliteConnectOptions::new()
            .filename(spool_path)
            .create_if_missing(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .map_err(|e| format!("connect Distractions spool: {e}"))?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS pending_usage_samples (
                sample_id TEXT PRIMARY KEY,
                vault_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                source_type TEXT NOT NULL,
                source_key TEXT NOT NULL,
                display_name TEXT,
                started_at_ms INTEGER NOT NULL,
                elapsed_seconds INTEGER NOT NULL,
                local_date TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .map_err(|e| format!("initialize Distractions spool: {e}"))?;
        let mut pending_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
        )
        .bind(&manifest.vault_id)
        .bind(&device_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| format!("count Distractions spool: {e}"))?;
        if pending_count >= MAX_PENDING_SAMPLES {
            pending_count = compact(&pool, &manifest.vault_id, &device_id).await?;
        }
        if pending_count >= MAX_PENDING_SAMPLES {
            return Err("the linked-device Distractions spool is full".to_string());
        }
        sqlx::query(
            "INSERT OR IGNORE INTO pending_usage_samples
                (sample_id, vault_id, device_id, source_type, source_key, display_name,
                 started_at_ms, elapsed_seconds, local_date, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(sample.id)
        .bind(manifest.vault_id)
        .bind(device_id)
        .bind(sample.source_type)
        .bind(sample.source_key)
        .bind(sample.display_name)
        .bind(sample.started_at_ms)
        .bind(sample.elapsed_seconds)
        .bind(sample.local_date)
        .bind(sample.created_at_ms)
        .execute(&pool)
        .await
        .map_err(|e| format!("spool usage sample: {e}"))?;
        pool.close().await;
        Ok(())
    })
}

async fn compact(pool: &sqlx::SqlitePool, vault_id: &str, device_id: &str) -> Result<i64, String> {
    let compacted_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
            SELECT 1 FROM pending_usage_samples
            WHERE vault_id = ? AND device_id = ?
            GROUP BY source_type, source_key, local_date
         )",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("plan Distractions spool compaction: {e}"))?;
    let overlong_group_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
            SELECT 1 FROM pending_usage_samples
            WHERE vault_id = ? AND device_id = ?
            GROUP BY source_type, source_key, local_date
            HAVING SUM(elapsed_seconds) > 86400
         )",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("validate Distractions spool compaction: {e}"))?;
    if compacted_count >= MAX_PENDING_SAMPLES || overlong_group_count > 0 {
        return Err("the linked-device Distractions spool cannot be compacted safely".to_string());
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| format!("begin Distractions spool compaction: {e}"))?;
    sqlx::query(
        "CREATE TEMP TABLE compacted_usage_samples AS
         SELECT MIN(sample_id) AS sample_id, vault_id, device_id, source_type,
                source_key, MAX(display_name) AS display_name,
                MIN(started_at_ms) AS started_at_ms, SUM(elapsed_seconds) AS elapsed_seconds,
                local_date, MAX(created_at_ms) AS created_at_ms
         FROM pending_usage_samples
         WHERE vault_id = ? AND device_id = ?
         GROUP BY vault_id, device_id, source_type, source_key, local_date",
    )
    .bind(vault_id)
    .bind(device_id)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("aggregate Distractions spool: {e}"))?;
    sqlx::query("DELETE FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?")
        .bind(vault_id)
        .bind(device_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| format!("replace Distractions spool: {e}"))?;
    sqlx::query("INSERT INTO pending_usage_samples SELECT * FROM compacted_usage_samples")
        .execute(&mut *transaction)
        .await
        .map_err(|e| format!("store compacted Distractions spool: {e}"))?;
    sqlx::query("DROP TABLE compacted_usage_samples")
        .execute(&mut *transaction)
        .await
        .map_err(|e| format!("finish Distractions spool compaction: {e}"))?;
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit Distractions spool compaction: {e}"))?;
    Ok(compacted_count)
}

pub(super) fn native_vault_is_writable(config_dir: &Path, vault_path: Option<&Path>) -> bool {
    let Some(vault_path) = vault_path else {
        return false;
    };
    let Ok(state) = read_json::<DeviceState>(&config_dir.join(APP_STATE_FILE), "device state")
    else {
        return false;
    };
    let Some(device_id) = state.device_id.filter(|value| !value.trim().is_empty()) else {
        return false;
    };
    let Ok(manifest) = read_json::<VaultManifest>(&vault_path.join("vault.json"), "vault identity")
    else {
        return false;
    };
    let Some(ownership) = std::fs::read(config_dir.join(OWNERSHIP_STATE_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return false;
    };
    let Some(record) = ownership
        .get("vaults")
        .and_then(|vaults| vaults.get(&manifest.vault_id))
    else {
        return false;
    };
    record.get("ownerDeviceId").and_then(Value::as_str) == Some(device_id.as_str())
        && record
            .get("transferPhase")
            .and_then(|phase| phase.get("kind"))
            .and_then(Value::as_str)
            == Some("stable")
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {label}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {label}: {error}"))
}

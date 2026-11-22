//! Device-local spool the browser host writes and the app drains into the vault.
//!
//! The host never opens the vault database. Usage samples and block events wait here, keyed by
//! the vault and device that produced them, until the app moves them into the vault.

use ganbaru_distractions_authority::spool::{SPOOL_RESET_SQL, SPOOL_SCHEMA_VERSION};
use serde::Deserialize;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::{Path, PathBuf};

const APP_STATE_FILE: &str = "app-state.json";
const SPOOL_FILE: &str = "distractions-device-spool.sqlite";
const MAX_PENDING_SAMPLES: i64 = 4_000;
/// Block events a device keeps waiting for the app; later blocks are not recorded until it drains.
const MAX_PENDING_BLOCK_EVENTS: i64 = 4_000;

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

/// One browser block with the rule that matched it. The source key is a normalized host.
#[derive(Debug)]
pub(super) struct BlockEvent {
    pub id: String,
    pub run_id: Option<String>,
    pub occurred_at: String,
    pub source_key: String,
    pub phase: Option<String>,
    pub decision: &'static str,
    pub rule_kind: Option<&'static str>,
    pub rule_label: Option<String>,
    pub blocker_mode: &'static str,
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

/// The spool file and the vault and device a spooled row belongs to.
struct SpoolTarget {
    path: PathBuf,
    vault_id: String,
    device_id: String,
}

/// Reads the device identity and the vault marker; the vault database stays closed.
fn spool_target(
    config_dir: Option<&Path>,
    vault_path: Option<&Path>,
) -> Result<SpoolTarget, String> {
    let config_dir =
        config_dir.ok_or_else(|| "Ganbaru device storage is unavailable".to_string())?;
    let vault_path =
        vault_path.ok_or_else(|| "active Ganbaru folder is unavailable".to_string())?;
    let state: DeviceState = read_json(&config_dir.join(APP_STATE_FILE), "device state")?;
    let device_id = state
        .device_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Ganbaru device identity is unavailable".to_string())?;
    let manifest: VaultManifest = read_json(&vault_path.join("vault.json"), "vault identity")?;
    if manifest.vault_id.trim().is_empty() {
        return Err("active vault identity is unavailable".to_string());
    }
    Ok(SpoolTarget {
        path: config_dir.join(SPOOL_FILE),
        vault_id: manifest.vault_id,
        device_id,
    })
}

/// Replaces the spool tables when their schema version is not the current one, inside one
/// immediate transaction so the host and the app never reset the same spool twice.
async fn prepare(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin_with("BEGIN IMMEDIATE").await?;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut *transaction)
        .await?;
    if version != SPOOL_SCHEMA_VERSION {
        sqlx::raw_sql(SPOOL_RESET_SQL)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await
}

/// Opens the spool with the current schema, which the app shares.
async fn connect(path: &Path) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|e| format!("connect Distractions spool: {e}"))?;
    prepare(&pool)
        .await
        .map_err(|e| format!("initialize Distractions spool: {e}"))?;
    Ok(pool)
}

pub(super) fn record_usage_sample(
    config_dir: Option<&Path>,
    vault_path: Option<&Path>,
    sample: UsageSample,
) -> Result<(), String> {
    let target = spool_target(config_dir, vault_path)?;
    super::block_on(async move {
        let pool = connect(&target.path).await?;
        let result = spool_usage_sample(&pool, &target, sample).await;
        pool.close().await;
        result
    })
}

async fn spool_usage_sample(
    pool: &SqlitePool,
    target: &SpoolTarget,
    sample: UsageSample,
) -> Result<(), String> {
    let mut pending_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
    )
    .bind(&target.vault_id)
    .bind(&target.device_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("count Distractions spool: {e}"))?;
    if pending_count >= MAX_PENDING_SAMPLES {
        pending_count = compact(pool, &target.vault_id, &target.device_id).await?;
    }
    if pending_count >= MAX_PENDING_SAMPLES {
        return Err("the Distractions usage spool is full".to_string());
    }
    sqlx::query(
        "INSERT OR IGNORE INTO pending_usage_samples
            (sample_id, vault_id, device_id, source_type, source_key, display_name,
             started_at_ms, elapsed_seconds, local_date, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sample.id)
    .bind(&target.vault_id)
    .bind(&target.device_id)
    .bind(sample.source_type)
    .bind(sample.source_key)
    .bind(sample.display_name)
    .bind(sample.started_at_ms)
    .bind(sample.elapsed_seconds)
    .bind(sample.local_date)
    .bind(sample.created_at_ms)
    .execute(pool)
    .await
    .map_err(|e| format!("spool usage sample: {e}"))?;
    Ok(())
}

/// Spools one browser block for the app to record in the vault.
pub(super) fn record_block_event(
    config_dir: Option<&Path>,
    vault_path: Option<&Path>,
    event: BlockEvent,
) -> Result<(), String> {
    let target = spool_target(config_dir, vault_path)?;
    super::block_on(async move {
        let pool = connect(&target.path).await?;
        let result = spool_block_event(&pool, &target, event).await;
        pool.close().await;
        result
    })
}

async fn spool_block_event(
    pool: &SqlitePool,
    target: &SpoolTarget,
    event: BlockEvent,
) -> Result<(), String> {
    let pending_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_block_events WHERE vault_id = ? AND device_id = ?",
    )
    .bind(&target.vault_id)
    .bind(&target.device_id)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("count spooled block events: {e}"))?;
    if pending_count >= MAX_PENDING_BLOCK_EVENTS {
        return Err("the Distractions block event spool is full".to_string());
    }
    sqlx::query(
        "INSERT OR IGNORE INTO pending_block_events
            (event_id, vault_id, device_id, run_id, occurred_at, source_type, source_key,
             display_name, phase, decision, rule_kind, rule_label, blocker_mode)
         VALUES (?, ?, ?, ?, ?, 'browser', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.id)
    .bind(&target.vault_id)
    .bind(&target.device_id)
    .bind(&event.run_id)
    .bind(&event.occurred_at)
    .bind(&event.source_key)
    .bind(&event.source_key)
    .bind(&event.phase)
    .bind(event.decision)
    .bind(event.rule_kind)
    .bind(&event.rule_label)
    .bind(event.blocker_mode)
    .execute(pool)
    .await
    .map_err(|e| format!("spool block event: {e}"))?;
    Ok(())
}

/// Merges each source and day of the unclaimed pending samples into one sample and returns the
/// pending count afterwards. Rows the app has claimed for a drain may already be in the vault
/// journal under their own ids, so they stay untouched.
async fn compact(pool: &SqlitePool, vault_id: &str, device_id: &str) -> Result<i64, String> {
    let compacted_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM (
            SELECT 1 FROM pending_usage_samples
            WHERE vault_id = ? AND device_id = ? AND claimed = 0
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
            WHERE vault_id = ? AND device_id = ? AND claimed = 0
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
        return Err("the Distractions usage spool cannot be compacted safely".to_string());
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
         WHERE vault_id = ? AND device_id = ? AND claimed = 0
         GROUP BY vault_id, device_id, source_type, source_key, local_date",
    )
    .bind(vault_id)
    .bind(device_id)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("aggregate Distractions spool: {e}"))?;
    sqlx::query(
        "DELETE FROM pending_usage_samples WHERE vault_id = ? AND device_id = ? AND claimed = 0",
    )
    .bind(vault_id)
    .bind(device_id)
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("replace Distractions spool: {e}"))?;
    sqlx::query(
        "INSERT INTO pending_usage_samples
            (sample_id, vault_id, device_id, source_type, source_key, display_name,
             started_at_ms, elapsed_seconds, local_date, created_at_ms)
         SELECT sample_id, vault_id, device_id, source_type, source_key, display_name,
                started_at_ms, elapsed_seconds, local_date, created_at_ms
         FROM compacted_usage_samples",
    )
    .execute(&mut *transaction)
    .await
    .map_err(|e| format!("store compacted Distractions spool: {e}"))?;
    sqlx::query("DROP TABLE compacted_usage_samples")
        .execute(&mut *transaction)
        .await
        .map_err(|e| format!("finish Distractions spool compaction: {e}"))?;
    let pending_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|e| format!("count compacted Distractions spool: {e}"))?;
    transaction
        .commit()
        .await
        .map_err(|e| format!("commit Distractions spool compaction: {e}"))?;
    Ok(pending_count)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {label}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {label}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{compact, connect};

    #[test]
    fn a_spool_from_an_older_schema_is_replaced_with_the_current_tables() {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-host-spool-older-schema-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        crate::block_on(async {
            let older = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    sqlx::sqlite::SqliteConnectOptions::new()
                        .filename(&path)
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            sqlx::raw_sql(
                "CREATE TABLE pending_usage_samples (sample_id TEXT PRIMARY KEY);
                 INSERT INTO pending_usage_samples VALUES ('stale');",
            )
            .execute(&older)
            .await
            .unwrap();
            older.close().await;

            let pool = connect(&path).await.unwrap();
            let pending: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM pending_usage_samples WHERE claimed = 0")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(pending, 0);
            pool.close().await;
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compaction_merges_only_unclaimed_samples() {
        let path = std::env::temp_dir().join(format!(
            "ganbaru-host-spool-compaction-{}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        crate::block_on(async {
            let pool = connect(&path).await.unwrap();
            sqlx::raw_sql(
                "INSERT INTO pending_usage_samples
                    (sample_id, vault_id, device_id, source_type, source_key, display_name,
                     started_at_ms, elapsed_seconds, local_date, created_at_ms, claimed)
                 VALUES
                    ('a', 'v', 'd', 'website', 'example.com', NULL, 1, 12, '2026-06-10', 1, 1),
                    ('b', 'v', 'd', 'website', 'example.com', NULL, 2, 18, '2026-06-10', 2, 1),
                    ('c', 'v', 'd', 'website', 'example.com', NULL, 3, 30, '2026-06-10', 3, 0),
                    ('e', 'v', 'd', 'website', 'example.com', NULL, 4, 5, '2026-06-10', 4, 0)",
            )
            .execute(&pool)
            .await
            .unwrap();
            assert_eq!(compact(&pool, "v", "d").await.unwrap(), 3);
            let rows: Vec<(String, i64, i64)> = sqlx::query_as(
                "SELECT sample_id, elapsed_seconds, claimed FROM pending_usage_samples
                 ORDER BY sample_id",
            )
            .fetch_all(&pool)
            .await
            .unwrap();
            assert_eq!(
                rows,
                [
                    ("a".to_string(), 12, 1),
                    ("b".to_string(), 18, 1),
                    ("c".to_string(), 35, 0)
                ]
            );
            pool.close().await;
        });
        let _ = std::fs::remove_file(&path);
    }
}

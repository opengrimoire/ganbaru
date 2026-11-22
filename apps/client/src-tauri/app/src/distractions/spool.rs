//! Device-local spool of desktop usage samples and browser block events awaiting the vault.
//!
//! The native owner records each interval batch here first, so a batch survives a restart or a
//! vault that is briefly unavailable. Draining moves the samples into this device's
//! `distractions_usage_samples` journal, from which the replicated daily totals are flushed.
//! The browser host never opens the vault; it spools its block events here, and ingestion moves
//! them into the replicated `distractions_block_events`.

use crate::distractions::DistractionsUsageSampleRow;
use ganbaru_distractions::contracts::spool::{SPOOL_RESET_SQL, SPOOL_SCHEMA_VERSION};
use ganbaru_distractions::usage::totals::insert_own_samples;
use ganbaru_distractions::usage::{
    SpooledBlockEvent, SpooledBlockEventError, UsageDateWindow, record_spooled_block_event,
};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqliteConnection, SqlitePool};
use std::path::{Path, PathBuf};
use tauri::{Manager, Runtime};

const SPOOL_FILE: &str = "distractions-device-spool.sqlite";
const MAX_PENDING_SAMPLES: i64 = 4_000;
const DRAIN_BATCH_SAMPLES: i64 = 200;
/// Block events the browser host keeps waiting per device before it stops recording more.
const MAX_PENDING_BLOCK_EVENTS: i64 = 4_000;
const INGEST_BATCH_BLOCK_EVENTS: i64 = 200;
const ACCOUNTING_SESSION_BYTES: usize = 16;
/// Longest elapsed time one compacted sample carries.
const MAX_COMPACTED_SECONDS: i64 = 86_400;

fn spool_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(SPOOL_FILE))
        .map_err(|error| format!("find Distractions spool directory: {error}"))
}

/// Replaces the spool tables when their schema version is not the current one, inside one
/// immediate transaction so the browser host and the app never reset the same spool twice.
async fn prepare_spool(pool: &SqlitePool) -> Result<(), sqlx::Error> {
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

async fn open_spool(path: &Path) -> Result<SqlitePool, String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("create Distractions spool directory: {error}"))?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(|error| format!("open Distractions spool: {error}"))?;
    prepare_spool(&pool)
        .await
        .map_err(|error| format!("initialize Distractions spool: {error}"))?;
    Ok(pool)
}

/// Allocate a restart-independent namespace without opening a usage database during idle startup.
pub(crate) fn new_accounting_session() -> Result<String, String> {
    let mut nonce = [0_u8; ACCOUNTING_SESSION_BYTES];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut nonce)
        .map_err(|error| format!("allocate desktop usage session: {error:?}"))?;
    Ok(nonce.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Persist a native interval batch before publishing totals or attempting limit enforcement.
pub(crate) async fn enqueue_native_samples<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    samples: &[DistractionsUsageSampleRow],
) -> Result<(), String> {
    enqueue_native_samples_at(&spool_path(app)?, vault_id, device_id, samples).await
}

/// The serialized native owner retries its pending batch before producing another batch; a
/// receipt keeps a retry of a batch that already drained from adding it twice.
async fn enqueue_native_samples_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    samples: &[DistractionsUsageSampleRow],
) -> Result<(), String> {
    if samples.len() > MAX_PENDING_SAMPLES as usize {
        return Err("native usage batch exceeds its limit".into());
    }
    if samples.is_empty() {
        return Ok(());
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(samples).map_err(|error| error.to_string())?)
    );
    let batch_id = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&samples.iter().map(|sample| &sample.id).collect::<Vec<_>>())
                .map_err(|error| error.to_string())?
        )
    );
    let pool = open_spool(path).await?;
    let result = async {
        let mut tx = pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(|error| format!("begin native interval batch: {error}"))?;
        let receipt: Option<(String, String)> = sqlx::query_as(
            "SELECT batch_id, digest FROM native_usage_batch_receipts
             WHERE vault_id = ? AND device_id = ?",
        )
        .bind(vault_id)
        .bind(device_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| format!("read native usage receipt: {error}"))?;
        if let Some((prior_id, prior_digest)) = receipt
            && prior_id == batch_id
        {
            return if prior_digest == digest {
                Ok(())
            } else {
                Err("native usage retry changed its payload".into())
            };
        }
        if pending_count(&mut tx, vault_id, device_id).await? + samples.len() as i64
            > MAX_PENDING_SAMPLES
        {
            compact_pending(&mut tx, vault_id, device_id).await?;
        }
        let count = pending_count(&mut tx, vault_id, device_id).await?;
        let mut added = 0;
        for sample in samples {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pending_usage_samples WHERE sample_id = ?)",
            )
            .bind(&sample.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("check pending usage sample: {error}"))?;
            if !exists {
                added += 1;
            }
            if count + added > MAX_PENDING_SAMPLES {
                return Err("the Distractions usage spool is full".into());
            }
            insert_pending(&mut tx, vault_id, device_id, sample).await?;
        }
        sqlx::query(
            "INSERT INTO native_usage_batch_receipts (vault_id, device_id, batch_id, digest)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (vault_id, device_id)
             DO UPDATE SET batch_id = excluded.batch_id, digest = excluded.digest",
        )
        .bind(vault_id)
        .bind(device_id)
        .bind(batch_id)
        .bind(digest)
        .execute(&mut *tx)
        .await
        .map_err(|error| format!("retain native usage receipt: {error}"))?;
        tx.commit()
            .await
            .map_err(|error| format!("commit native interval batch: {error}"))
    }
    .await;
    pool.close().await;
    result
}

async fn pending_count(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<i64, String> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(connection)
    .await
    .map_err(|error| format!("count pending Distractions samples: {error}"))
}

/// Replaces the unclaimed pending samples of one device with one sample per source and day,
/// keeping their exact elapsed total. Rows claimed by a drain may already be in the vault
/// journal under their own ids, so they stay untouched until that drain removes them.
async fn compact_pending(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<(), String> {
    let rows = sqlx::query(
        "SELECT source_type, source_key, MAX(display_name) AS display_name,
                MIN(started_at_ms) AS started_at_ms, SUM(elapsed_seconds) AS elapsed_seconds,
                local_date, MAX(created_at_ms) AS created_at_ms,
                GROUP_CONCAT(hex(CAST(sample_id AS BLOB)), '|' ORDER BY sample_id) AS members
         FROM pending_usage_samples
         WHERE vault_id = ? AND device_id = ? AND claimed = 0
         GROUP BY source_type, source_key, local_date",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| format!("compact pending Distractions samples: {error}"))?;
    let mut compacted = Vec::new();
    for row in rows {
        let read = |error: sqlx::Error| format!("read pending Distractions sample: {error}");
        let source_type: String = row.try_get("source_type").map_err(read)?;
        let source_key: String = row.try_get("source_key").map_err(read)?;
        let display_name: Option<String> = row.try_get("display_name").map_err(read)?;
        let started_at_ms: i64 = row.try_get("started_at_ms").map_err(read)?;
        let mut elapsed_seconds: i64 = row.try_get("elapsed_seconds").map_err(read)?;
        let local_date: String = row.try_get("local_date").map_err(read)?;
        let created_at_ms: i64 = row.try_get("created_at_ms").map_err(read)?;
        let members: String = row.try_get("members").map_err(read)?;
        let identity = format!("{device_id}|{source_type}|{source_key}|{local_date}|{members}");
        let identity = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let mut part = 0_u32;
        while elapsed_seconds > 0 {
            let chunk = elapsed_seconds.min(MAX_COMPACTED_SECONDS);
            compacted.push(DistractionsUsageSampleRow {
                id: format!("compact-{identity}-{part}"),
                source_type: source_type.clone(),
                source_key: source_key.clone(),
                display_name: display_name.clone(),
                started_at_ms,
                elapsed_seconds: chunk,
                local_date: local_date.clone(),
                created_at_ms,
            });
            elapsed_seconds -= chunk;
            part += 1;
        }
    }
    if compacted.len() as i64 >= MAX_PENDING_SAMPLES {
        return Err("the Distractions usage spool cannot be compacted safely".to_string());
    }
    sqlx::query(
        "DELETE FROM pending_usage_samples WHERE vault_id = ? AND device_id = ? AND claimed = 0",
    )
    .bind(vault_id)
    .bind(device_id)
    .execute(&mut *connection)
    .await
    .map_err(|error| format!("replace compacted Distractions spool: {error}"))?;
    for sample in &compacted {
        insert_pending(&mut *connection, vault_id, device_id, sample).await?;
    }
    Ok(())
}

async fn insert_pending(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
    sample: &DistractionsUsageSampleRow,
) -> Result<(), String> {
    sqlx::query(
        "INSERT OR IGNORE INTO pending_usage_samples
            (sample_id, vault_id, device_id, source_type, source_key, display_name,
             started_at_ms, elapsed_seconds, local_date, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&sample.id)
    .bind(vault_id)
    .bind(device_id)
    .bind(&sample.source_type)
    .bind(&sample.source_key)
    .bind(&sample.display_name)
    .bind(sample.started_at_ms)
    .bind(sample.elapsed_seconds)
    .bind(&sample.local_date)
    .bind(sample.created_at_ms)
    .execute(connection)
    .await
    .map_err(|error| format!("store pending Distractions sample: {error}"))?;
    Ok(())
}

/// Returns the claimed drain batch of one device, first claiming the oldest pending samples in
/// a committed spool transaction when no earlier drain left a claimed batch behind.
async fn claim_batch(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<DistractionsUsageSampleRow>, String> {
    let mut transaction = sqlx::Connection::begin(&mut *connection)
        .await
        .map_err(|error| format!("begin Distractions spool claim: {error}"))?;
    sqlx::query(
        "UPDATE pending_usage_samples SET claimed = 1
         WHERE sample_id IN (
             SELECT sample_id FROM pending_usage_samples
             WHERE vault_id = ? AND device_id = ? AND claimed = 0
             ORDER BY created_at_ms ASC, sample_id ASC LIMIT ?)
           AND NOT EXISTS (
             SELECT 1 FROM pending_usage_samples
             WHERE vault_id = ? AND device_id = ? AND claimed = 1)",
    )
    .bind(vault_id)
    .bind(device_id)
    .bind(DRAIN_BATCH_SAMPLES)
    .bind(vault_id)
    .bind(device_id)
    .execute(&mut *transaction)
    .await
    .map_err(|error| format!("claim pending Distractions samples: {error}"))?;
    let batch = read_pending(&mut transaction, vault_id, device_id, true).await?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit Distractions spool claim: {error}"))?;
    Ok(batch)
}

async fn read_pending(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
    claimed_only: bool,
) -> Result<Vec<DistractionsUsageSampleRow>, String> {
    let rows = sqlx::query(
        "SELECT sample_id, source_type, source_key, display_name, started_at_ms,
                elapsed_seconds, local_date, created_at_ms
         FROM pending_usage_samples
         WHERE vault_id = ? AND device_id = ? AND (claimed = 1 OR NOT ?)
         ORDER BY created_at_ms ASC, sample_id ASC LIMIT ?",
    )
    .bind(vault_id)
    .bind(device_id)
    .bind(claimed_only)
    .bind(DRAIN_BATCH_SAMPLES)
    .fetch_all(connection)
    .await
    .map_err(|error| format!("read pending Distractions samples: {error}"))?;
    rows.into_iter()
        .map(|row| {
            let read = |error: sqlx::Error| format!("read pending Distractions sample: {error}");
            Ok(DistractionsUsageSampleRow {
                id: row.try_get("sample_id").map_err(read)?,
                source_type: row.try_get("source_type").map_err(read)?,
                source_key: row.try_get("source_key").map_err(read)?,
                display_name: row.try_get("display_name").map_err(read)?,
                started_at_ms: row.try_get("started_at_ms").map_err(read)?,
                elapsed_seconds: row.try_get("elapsed_seconds").map_err(read)?,
                local_date: row.try_get("local_date").map_err(read)?,
                created_at_ms: row.try_get("created_at_ms").map_err(read)?,
            })
        })
        .collect()
}

/// Moves this device's pending samples into the vault's usage journal. Returns how many samples
/// left the spool.
pub(crate) async fn drain<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
    window: &UsageDateWindow,
) -> Result<usize, String> {
    drain_at(&spool_path(app)?, pool, vault_id, device_id, window).await
}

/// Each batch is claimed in the spool, commits to the vault, and only then leaves the spool. The
/// journal ignores samples it already holds, so a claimed batch that committed but stayed in the
/// spool drains again harmlessly, and compaction never merges claimed rows into new ids. A sample
/// the journal refuses by a constraint or trigger leaves the spool with its batch and a log line,
/// so it never holds the claim; any other vault error keeps the whole batch claimed for the next
/// attempt.
async fn drain_at(
    path: &Path,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
    window: &UsageDateWindow,
) -> Result<usize, String> {
    if !tokio::fs::try_exists(path)
        .await
        .map_err(|error| format!("inspect Distractions spool: {error}"))?
    {
        return Ok(0);
    }
    let spool = open_spool(path).await?;
    let result = async {
        let mut drained = 0;
        for _ in 0..=(MAX_PENDING_SAMPLES / DRAIN_BATCH_SAMPLES) {
            let mut connection = spool
                .acquire()
                .await
                .map_err(|error| format!("open Distractions spool batch: {error}"))?;
            let batch = claim_batch(&mut connection, vault_id, device_id).await?;
            // The spool pool holds one connection, which the cleanup transaction needs.
            drop(connection);
            if batch.is_empty() {
                return Ok(drained);
            }
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| format!("begin usage journal import: {error}"))?;
            let import = insert_own_samples(&mut transaction, &batch, window).await?;
            transaction
                .commit()
                .await
                .map_err(|error| format!("commit usage journal import: {error}"))?;
            if import.skipped > 0 {
                eprintln!(
                    "skipped {} Distractions usage samples dated outside {}..={}",
                    import.skipped, window.earliest, window.latest
                );
            }
            for reason in &import.refused {
                eprintln!("dropped spooled Distractions {reason}");
            }
            let mut transaction = spool
                .begin()
                .await
                .map_err(|error| format!("begin Distractions spool cleanup: {error}"))?;
            for sample in &batch {
                sqlx::query(
                    "DELETE FROM pending_usage_samples
                     WHERE vault_id = ? AND device_id = ? AND sample_id = ? AND claimed = 1",
                )
                .bind(vault_id)
                .bind(device_id)
                .bind(&sample.id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| format!("remove drained Distractions sample: {error}"))?;
            }
            transaction
                .commit()
                .await
                .map_err(|error| format!("commit Distractions spool cleanup: {error}"))?;
            drained += batch.len();
        }
        Err("native usage drain exceeds the bounded pending window".to_string())
    }
    .await;
    spool.close().await;
    result
}

/// Whether the browser host left block events of this device in `vault_id` for the vault.
pub(crate) async fn has_pending_block_events<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
) -> Result<bool, String> {
    let path = spool_path(app)?;
    if !tokio::fs::try_exists(&path)
        .await
        .map_err(|error| format!("inspect Distractions spool: {error}"))?
    {
        return Ok(false);
    }
    let spool = open_spool(&path).await?;
    let pending = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pending_block_events WHERE vault_id = ? AND device_id = ?)",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(&spool)
    .await
    .map_err(|error| format!("check spooled block events: {error}"));
    spool.close().await;
    pending
}

/// Moves the block events the browser host spooled for this device into the vault. Returns how
/// many events left the spool.
pub(crate) async fn ingest_block_events<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
) -> Result<usize, String> {
    ingest_block_events_at(&spool_path(app)?, pool, vault_id, device_id).await
}

async fn read_pending_block_events(
    connection: &mut SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<SpooledBlockEvent>, String> {
    let rows = sqlx::query(
        "SELECT event_id, run_id, occurred_at, source_type, source_key, display_name, phase,
                decision, rule_kind, rule_label, blocker_mode
         FROM pending_block_events
         WHERE vault_id = ? AND device_id = ?
         ORDER BY occurred_at ASC, event_id ASC LIMIT ?",
    )
    .bind(vault_id)
    .bind(device_id)
    .bind(INGEST_BATCH_BLOCK_EVENTS)
    .fetch_all(connection)
    .await
    .map_err(|error| format!("read spooled block events: {error}"))?;
    rows.into_iter()
        .map(|row| {
            let read = |error: sqlx::Error| format!("read spooled block event: {error}");
            Ok(SpooledBlockEvent {
                id: row.try_get("event_id").map_err(read)?,
                run_id: row.try_get("run_id").map_err(read)?,
                occurred_at: row.try_get("occurred_at").map_err(read)?,
                source_type: row.try_get("source_type").map_err(read)?,
                source_key: row.try_get("source_key").map_err(read)?,
                display_name: row.try_get("display_name").map_err(read)?,
                phase: row.try_get("phase").map_err(read)?,
                decision: row.try_get("decision").map_err(read)?,
                rule_id: None,
                rule_kind: row.try_get("rule_kind").map_err(read)?,
                rule_label: row.try_get("rule_label").map_err(read)?,
                blocker_mode: row.try_get("blocker_mode").map_err(read)?,
            })
        })
        .collect()
}

/// Each batch commits to the vault before it leaves the spool; the vault ignores events it
/// already holds, so a batch that committed but stayed in the spool ingests again harmlessly. A
/// spooled row the vault would refuse, by validation or by a constraint or trigger, is dropped
/// with a log line instead of blocking the rest; any other vault error keeps the whole batch in
/// the spool for the next attempt.
async fn ingest_block_events_at(
    path: &Path,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
) -> Result<usize, String> {
    if !tokio::fs::try_exists(path)
        .await
        .map_err(|error| format!("inspect Distractions spool: {error}"))?
    {
        return Ok(0);
    }
    let spool = open_spool(path).await?;
    let result = async {
        let mut ingested = 0;
        for _ in 0..=(MAX_PENDING_BLOCK_EVENTS / INGEST_BATCH_BLOCK_EVENTS) {
            let mut connection = spool
                .acquire()
                .await
                .map_err(|error| format!("open spooled block event batch: {error}"))?;
            let batch = read_pending_block_events(&mut connection, vault_id, device_id).await?;
            // The spool pool holds one connection, which the cleanup transaction needs.
            drop(connection);
            if batch.is_empty() {
                return Ok(ingested);
            }
            let mut transaction = pool
                .begin()
                .await
                .map_err(|error| format!("begin block event import: {error}"))?;
            for event in &batch {
                match record_spooled_block_event(&mut transaction, event).await {
                    Ok(()) => {}
                    Err(SpooledBlockEventError::Refused(reason)) => {
                        eprintln!("dropped spooled Distractions block event: {reason}");
                    }
                    Err(SpooledBlockEventError::Failed(reason)) => return Err(reason),
                }
            }
            transaction
                .commit()
                .await
                .map_err(|error| format!("commit block event import: {error}"))?;
            let mut transaction = spool
                .begin()
                .await
                .map_err(|error| format!("begin block event spool cleanup: {error}"))?;
            for event in &batch {
                sqlx::query(
                    "DELETE FROM pending_block_events
                     WHERE vault_id = ? AND device_id = ? AND event_id = ?",
                )
                .bind(vault_id)
                .bind(device_id)
                .bind(&event.id)
                .execute(&mut *transaction)
                .await
                .map_err(|error| format!("remove ingested block event: {error}"))?;
            }
            transaction
                .commit()
                .await
                .map_err(|error| format!("commit block event spool cleanup: {error}"))?;
            ingested += batch.len();
        }
        Err("block event ingestion exceeds the bounded pending window".to_string())
    }
    .await;
    spool.close().await;
    result
}

#[cfg(test)]
mod tests;

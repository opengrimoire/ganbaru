//! Device-local Distractions usage spool and linked-device accounting cache.

use crate::vault::handoff::protocol::{DistractionsSampleMessage, MAX_DISTRACTIONS_SAMPLES};
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use tauri::{Manager, Runtime};

const SPOOL_FILE: &str = "distractions-device-spool.sqlite";
#[cfg(desktop)]
const MAX_PENDING_SAMPLES: i64 = 4_000;
#[cfg(desktop)]
const EXCHANGE_BATCH_SAMPLES: i64 = 200;
#[cfg(desktop)]
const ACCOUNTING_SESSION_BYTES: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(desktop)]
pub(crate) struct LinkedUsageRow {
    pub id: String,
    pub source_type: String,
    pub source_key: String,
    pub display_name: Option<String>,
    pub started_at_ms: i64,
    pub elapsed_seconds: i64,
    pub local_date: String,
    pub created_at_ms: i64,
}

pub(crate) fn spool_path<R: Runtime>(app: &tauri::AppHandle<R>) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|path| path.join(SPOOL_FILE))
        .map_err(|error| format!("find Distractions spool directory: {error}"))
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
    .map_err(|error| format!("initialize Distractions spool: {error}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS accepted_usage_samples (
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
    .map_err(|error| format!("initialize accepted Distractions cache: {error}"))?;
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| format!("begin usage export-marker initialization: {error}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS exported_usage_samples (
        vault_id TEXT NOT NULL, device_id TEXT NOT NULL, sample_id TEXT NOT NULL,
        PRIMARY KEY (vault_id, device_id, sample_id))",
    )
    .execute(&mut *tx)
    .await
    .map_err(|error| format!("initialize usage export markers: {error}"))?;
    tx.commit()
        .await
        .map_err(|error| format!("commit usage export-marker initialization: {error}"))?;
    sqlx::raw_sql("CREATE TRIGGER IF NOT EXISTS remove_usage_export_marker AFTER DELETE ON pending_usage_samples
        BEGIN DELETE FROM exported_usage_samples WHERE vault_id = OLD.vault_id AND device_id = OLD.device_id AND sample_id = OLD.sample_id; END")
        .execute(&pool).await.map_err(|error| format!("initialize usage export-marker cleanup: {error}"))?;
    sqlx::query("CREATE TABLE IF NOT EXISTS native_usage_batch_receipts (
        vault_id TEXT NOT NULL, device_id TEXT NOT NULL, batch_id TEXT NOT NULL, digest TEXT NOT NULL,
        PRIMARY KEY (vault_id, device_id))")
        .execute(&pool).await.map_err(|error| format!("initialize native usage receipts: {error}"))?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS guardian_usage_acknowledgements (
        vault_id TEXT NOT NULL, device_id TEXT NOT NULL, sample_id TEXT NOT NULL,
        PRIMARY KEY (vault_id, device_id, sample_id))",
    )
    .execute(&pool)
    .await
    .map_err(|error| format!("initialize Guardian usage acknowledgements: {error}"))?;
    Ok(pool)
}

/// Allocate a restart-independent namespace without opening a usage database during idle startup.
#[cfg(desktop)]
pub(crate) fn new_accounting_session() -> Result<String, String> {
    let mut nonce = [0_u8; ACCOUNTING_SESSION_BYTES];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut nonce)
        .map_err(|error| format!("allocate desktop usage session: {error:?}"))?;
    Ok(nonce.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Persist a native interval batch before publishing totals or attempting limit enforcement.
#[cfg(desktop)]
pub(crate) async fn enqueue_native_samples<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    samples: &[crate::distractions::DistractionsUsageSampleRow],
) -> Result<(), String> {
    enqueue_native_samples_at(&spool_path(app)?, vault_id, device_id, samples).await
}

/// The serialized native owner retries its pending batch before producing another batch.
#[cfg(desktop)]
async fn enqueue_native_samples_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    samples: &[crate::distractions::DistractionsUsageSampleRow],
) -> Result<(), String> {
    if samples.len() > MAX_PENDING_SAMPLES as usize {
        return Err("native usage batch exceeds its limit".into());
    }
    if samples.is_empty() {
        return Ok(());
    }
    let messages = samples
        .iter()
        .map(|sample| {
            row_to_message(
                &LinkedUsageRow {
                    id: sample.id.clone(),
                    source_type: sample.source_type.clone(),
                    source_key: sample.source_key.clone(),
                    display_name: sample.display_name.clone(),
                    started_at_ms: sample.started_at_ms,
                    elapsed_seconds: sample.elapsed_seconds,
                    local_date: sample.local_date.clone(),
                    created_at_ms: sample.created_at_ms,
                },
                device_id,
            )
        })
        .collect::<Vec<_>>();
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&messages).map_err(|error| error.to_string())?)
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
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(|error| format!("begin native interval batch: {error}"))?;
        let receipt: Option<(String, String)> = sqlx::query_as("SELECT batch_id, digest FROM native_usage_batch_receipts WHERE vault_id = ? AND device_id = ?")
            .bind(vault_id).bind(device_id).fetch_optional(&mut *tx).await.map_err(|error| error.to_string())?;
        if let Some((prior_id, prior_digest)) = receipt {
            if prior_id == batch_id {
                return if prior_digest == digest { Ok(()) } else { Err("native usage retry changed its payload".into()) };
            }
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?")
            .bind(vault_id).bind(device_id).fetch_one(&mut *tx).await.map_err(|error| error.to_string())?;
        if count + samples.len() as i64 > MAX_PENDING_SAMPLES { compact_pending_tx(&mut tx, vault_id, device_id).await?; }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?")
            .bind(vault_id).bind(device_id).fetch_one(&mut *tx).await.map_err(|error| error.to_string())?;
        let mut added = 0;
        for sample in samples {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pending_usage_samples WHERE sample_id = ?)")
                .bind(&sample.id).fetch_one(&mut *tx).await.map_err(|error| error.to_string())?;
            if !exists { added += 1; }
            if count + added > MAX_PENDING_SAMPLES { return Err("the linked-device Distractions spool is full".into()); }
            let message = row_to_message(&LinkedUsageRow { id: sample.id.clone(), source_type: sample.source_type.clone(), source_key: sample.source_key.clone(),
                display_name: sample.display_name.clone(), started_at_ms: sample.started_at_ms, elapsed_seconds: sample.elapsed_seconds,
                local_date: sample.local_date.clone(), created_at_ms: sample.created_at_ms }, device_id);
            insert_message_executor(&mut tx, "pending_usage_samples", vault_id, &message).await?;
        }
        sqlx::query("INSERT INTO native_usage_batch_receipts (vault_id, device_id, batch_id, digest) VALUES (?, ?, ?, ?)
            ON CONFLICT (vault_id, device_id) DO UPDATE SET batch_id = excluded.batch_id, digest = excluded.digest")
            .bind(vault_id).bind(device_id).bind(batch_id).bind(digest).execute(&mut *tx).await
            .map_err(|error| format!("retain native usage receipt: {error}"))?;
        tx.commit().await.map_err(|error| format!("commit native interval batch: {error}"))
    }.await;
    pool.close().await;
    result
}

#[cfg(test)]
async fn enqueue_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    sample: &DistractionsSampleMessage,
) -> Result<(), String> {
    let pool = open_spool(path).await?;
    let mut count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_one(&pool)
    .await
    .map_err(|error| format!("count pending Distractions samples: {error}"))?;
    if count >= MAX_PENDING_SAMPLES {
        compact_pending(&pool, vault_id, device_id).await?;
        count = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pending_usage_samples WHERE vault_id = ? AND device_id = ?",
        )
        .bind(vault_id)
        .bind(device_id)
        .fetch_one(&pool)
        .await
        .map_err(|error| format!("count compacted Distractions samples: {error}"))?;
    }
    if count >= MAX_PENDING_SAMPLES {
        pool.close().await;
        return Err("the linked-device Distractions spool is full".to_string());
    }
    insert_message(&pool, "pending_usage_samples", vault_id, sample).await?;
    pool.close().await;
    Ok(())
}

#[cfg(test)]
async fn compact_pending(pool: &SqlitePool, vault_id: &str, device_id: &str) -> Result<(), String> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| format!("begin Distractions spool compaction: {error}"))?;
    compact_pending_tx(&mut transaction, vault_id, device_id).await?;
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit Distractions spool compaction: {error}"))
}

#[cfg(desktop)]
async fn compact_pending_tx(
    transaction: &mut sqlx::SqliteConnection,
    vault_id: &str,
    device_id: &str,
) -> Result<(), String> {
    let rows = sqlx::query(
        "SELECT source_type, source_key, MAX(display_name) AS display_name,
                MIN(started_at_ms) AS started_at_ms, SUM(elapsed_seconds) AS elapsed_seconds,
                local_date, MAX(created_at_ms) AS created_at_ms,
                GROUP_CONCAT(hex(CAST(sample_id AS BLOB)), '|' ORDER BY sample_id) AS identity_members
         FROM pending_usage_samples p
         WHERE vault_id = ? AND device_id = ? AND NOT EXISTS (
            SELECT 1 FROM exported_usage_samples e WHERE e.vault_id = p.vault_id
              AND e.device_id = p.device_id AND e.sample_id = p.sample_id)
         GROUP BY source_type, source_key, local_date",
    )
    .bind(vault_id)
    .bind(device_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| format!("compact pending Distractions samples: {error}"))?;
    let mut compacted = Vec::new();
    for row in rows {
        let source_type: String = row.try_get("source_type").map_err(|e| e.to_string())?;
        let source_key: String = row.try_get("source_key").map_err(|e| e.to_string())?;
        let display_name: Option<String> =
            row.try_get("display_name").map_err(|e| e.to_string())?;
        let started_at_ms: i64 = row.try_get("started_at_ms").map_err(|e| e.to_string())?;
        let mut elapsed_seconds: i64 = row.try_get("elapsed_seconds").map_err(|e| e.to_string())?;
        let local_date: String = row.try_get("local_date").map_err(|e| e.to_string())?;
        let created_at_ms: i64 = row.try_get("created_at_ms").map_err(|e| e.to_string())?;
        let members: String = row
            .try_get("identity_members")
            .map_err(|error| error.to_string())?;
        let identity = format!("{device_id}|{source_type}|{source_key}|{local_date}|{members}");
        let mut part = 0_u32;
        while elapsed_seconds > 0 {
            compacted.push(DistractionsSampleMessage {
                sample_id: format!("compact-{:x}-{part}", Sha256::digest(identity.as_bytes())),
                device_id: device_id.to_string(),
                source_type: source_type.clone(),
                source_key: source_key.clone(),
                display_name: display_name.clone(),
                started_at_ms,
                elapsed_seconds: elapsed_seconds.min(86_400),
                local_date: local_date.clone(),
                created_at_ms,
            });
            elapsed_seconds -= elapsed_seconds.min(86_400);
            part += 1;
        }
    }
    if compacted.len() as i64 >= MAX_PENDING_SAMPLES {
        return Err("the linked-device Distractions spool cannot be compacted safely".to_string());
    }
    sqlx::query("DELETE FROM pending_usage_samples AS p WHERE vault_id = ? AND device_id = ? AND NOT EXISTS (
        SELECT 1 FROM exported_usage_samples e WHERE e.vault_id = p.vault_id AND e.device_id = p.device_id AND e.sample_id = p.sample_id)")
        .bind(vault_id)
        .bind(device_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("replace compacted Distractions spool: {error}"))?;
    for sample in &compacted {
        insert_message_executor(&mut *transaction, "pending_usage_samples", vault_id, sample)
            .await?;
    }
    Ok(())
}

#[cfg(desktop)]
pub(crate) async fn pending<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    pending_at(&spool_path(app)?, vault_id, device_id).await
}

#[cfg(desktop)]
pub(crate) async fn drain_local_spool<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &SqlitePool,
    vault_id: &str,
    device_id: &str,
) -> Result<usize, String> {
    drain_local_spool_at(&spool_path(app)?, pool, vault_id, device_id).await
}

#[cfg(desktop)]
async fn drain_local_spool_at(
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
    let mut total = 0;
    for _ in 0..(MAX_PENDING_SAMPLES / EXCHANGE_BATCH_SAMPLES) {
        let samples = pending_at(path, vault_id, device_id).await?;
        if samples.is_empty() {
            return Ok(total);
        }
        let acknowledged = import_linked_samples(pool, &samples).await?;
        acknowledge_at(path, vault_id, device_id, &acknowledged).await?;
        total += acknowledged.len();
    }
    if !pending_at(path, vault_id, device_id).await?.is_empty() {
        return Err("native usage drain exceeds the bounded pending window".into());
    }
    Ok(total)
}

#[cfg(desktop)]
async fn pending_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    let pool = open_spool(path).await?;
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| format!("begin usage export batch: {error}"))?;
    let samples =
        read_messages(&mut tx, "pending_usage_samples", vault_id, Some(device_id)).await?;
    for sample in &samples {
        sqlx::query("INSERT OR IGNORE INTO exported_usage_samples (vault_id, device_id, sample_id) VALUES (?, ?, ?)")
            .bind(vault_id).bind(device_id).bind(&sample.sample_id)
            .execute(&mut *tx).await.map_err(|error| format!("retain exported usage identity: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit usage export batch: {error}"))?;
    pool.close().await;
    Ok(samples)
}

#[cfg(desktop)]
pub(crate) async fn acknowledge<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    sample_ids: &[String],
) -> Result<(), String> {
    acknowledge_at(&spool_path(app)?, vault_id, device_id, sample_ids).await
}

/// Return the complete accepted transport snapshot; a pending batch limit must not truncate it.
#[cfg(desktop)]
pub(crate) async fn accepted<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    accepted_at(&spool_path(app)?, vault_id).await
}

#[cfg(desktop)]
async fn accepted_at(
    path: &Path,
    vault_id: &str,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    let pool = open_spool(path).await?;
    let mut connection = pool.acquire().await.map_err(|error| error.to_string())?;
    let samples = read_messages_with_limit(
        &mut connection,
        "accepted_usage_samples",
        vault_id,
        None,
        (MAX_DISTRACTIONS_SAMPLES + 1) as i64,
    )
    .await?;
    drop(connection);
    pool.close().await;
    if samples.len() > MAX_DISTRACTIONS_SAMPLES {
        return Err("accepted Distractions snapshot exceeds the exchange limit".into());
    }
    Ok(samples)
}

#[cfg(desktop)]
async fn acknowledge_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    sample_ids: &[String],
) -> Result<(), String> {
    if sample_ids.len() > MAX_DISTRACTIONS_SAMPLES {
        return Err("too many Distractions sample acknowledgements".to_string());
    }
    let pool = open_spool(path).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin Distractions acknowledgement: {error}"))?;
    for sample_id in sample_ids {
        sqlx::query(
            "DELETE FROM pending_usage_samples
             WHERE vault_id = ? AND device_id = ? AND sample_id = ?",
        )
        .bind(vault_id)
        .bind(device_id)
        .bind(sample_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("acknowledge Distractions sample: {error}"))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit Distractions acknowledgement: {error}"))?;
    pool.close().await;
    Ok(())
}

pub(crate) async fn apply_owner_snapshot<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    acknowledged: &[String],
    samples: &[DistractionsSampleMessage],
) -> Result<(), String> {
    apply_owner_snapshot_at(
        &spool_path(app)?,
        vault_id,
        device_id,
        acknowledged,
        samples,
    )
    .await
}

#[cfg(test)]
async fn replace_accepted_at(
    path: &Path,
    vault_id: &str,
    samples: &[DistractionsSampleMessage],
) -> Result<(), String> {
    apply_owner_snapshot_at(path, vault_id, "", &[], samples).await
}

/// Acknowledgement and replacement are one boundary, preventing accepted/pending double counting.
async fn apply_owner_snapshot_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    acknowledged: &[String],
    samples: &[DistractionsSampleMessage],
) -> Result<(), String> {
    commit_owner_snapshot_at(
        path,
        vault_id,
        device_id,
        acknowledged,
        samples,
        AcknowledgementTarget::Spool,
    )
    .await
}

enum AcknowledgementTarget {
    Spool,
    #[cfg(any(target_os = "android", test))]
    Guardian,
}

/// Retain external journal acknowledgements with the accepted snapshot until Android confirms deletion.
#[cfg(target_os = "android")]
pub(crate) async fn apply_guardian_owner_snapshot<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    acknowledged: &[String],
    samples: &[DistractionsSampleMessage],
) -> Result<(), String> {
    commit_owner_snapshot_at(
        &spool_path(app)?,
        vault_id,
        device_id,
        acknowledged,
        samples,
        AcknowledgementTarget::Guardian,
    )
    .await
}

async fn commit_owner_snapshot_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    acknowledged: &[String],
    samples: &[DistractionsSampleMessage],
    target: AcknowledgementTarget,
) -> Result<(), String> {
    if samples.len() > MAX_DISTRACTIONS_SAMPLES || acknowledged.len() > MAX_DISTRACTIONS_SAMPLES {
        return Err("too many accepted Distractions samples".to_string());
    }
    let pool = open_spool(path).await?;
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin accepted Distractions replacement: {error}"))?;
    for id in acknowledged {
        if id.is_empty() || id.len() > 120 {
            return Err("accepted Distractions acknowledgement identity is invalid".into());
        }
        let query = match target {
            AcknowledgementTarget::Spool => {
                "DELETE FROM pending_usage_samples WHERE vault_id = ? AND device_id = ? AND sample_id = ?"
            }
            #[cfg(any(target_os = "android", test))]
            AcknowledgementTarget::Guardian => {
                "INSERT OR IGNORE INTO guardian_usage_acknowledgements (vault_id, device_id, sample_id) VALUES (?, ?, ?)"
            }
        };
        sqlx::query(query)
            .bind(vault_id)
            .bind(device_id)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .map_err(|error| format!("acknowledge accepted Distractions sample: {error}"))?;
    }
    #[cfg(any(target_os = "android", test))]
    if matches!(target, AcknowledgementTarget::Guardian) {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM guardian_usage_acknowledgements WHERE vault_id = ? AND device_id = ?")
            .bind(vault_id).bind(device_id).fetch_one(&mut *transaction).await
            .map_err(|error| format!("count retained Guardian acknowledgements: {error}"))?;
        if count > MAX_GUARDIAN_ACKNOWLEDGEMENTS {
            return Err("retained Guardian acknowledgements exceed their limit".into());
        }
    }
    sqlx::query("DELETE FROM accepted_usage_samples WHERE vault_id = ?")
        .bind(vault_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("clear accepted Distractions samples: {error}"))?;
    for sample in samples {
        insert_message_executor(&mut transaction, "accepted_usage_samples", vault_id, sample)
            .await?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit accepted Distractions replacement: {error}"))?;
    pool.close().await;
    Ok(())
}

#[cfg(any(target_os = "android", test))]
const MAX_GUARDIAN_ACKNOWLEDGEMENTS: i64 = 2_000;

#[cfg(target_os = "android")]
pub(crate) async fn guardian_acknowledgements<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<String>, String> {
    guardian_acknowledgements_at(&spool_path(app)?, vault_id, device_id).await
}

#[cfg(any(target_os = "android", test))]
async fn guardian_acknowledgements_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
) -> Result<Vec<String>, String> {
    let pool = open_spool(path).await?;
    let ids: Vec<String> = sqlx::query_scalar("SELECT sample_id FROM guardian_usage_acknowledgements WHERE vault_id = ? AND device_id = ? ORDER BY sample_id LIMIT ?")
        .bind(vault_id).bind(device_id).bind(MAX_GUARDIAN_ACKNOWLEDGEMENTS + 1)
        .fetch_all(&pool).await.map_err(|error| format!("read retained Guardian acknowledgements: {error}"))?;
    pool.close().await;
    if ids.len() > MAX_GUARDIAN_ACKNOWLEDGEMENTS as usize {
        return Err("retained Guardian acknowledgements exceed their limit".into());
    }
    Ok(ids)
}

#[cfg(target_os = "android")]
pub(crate) async fn forget_guardian_acknowledgements<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    ids: &[String],
) -> Result<(), String> {
    forget_guardian_acknowledgements_at(&spool_path(app)?, vault_id, device_id, ids).await
}

#[cfg(any(target_os = "android", test))]
async fn forget_guardian_acknowledgements_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    ids: &[String],
) -> Result<(), String> {
    if ids.len() > MAX_GUARDIAN_ACKNOWLEDGEMENTS as usize {
        return Err("too many confirmed Guardian acknowledgements".into());
    }
    let pool = open_spool(path).await?;
    let mut tx = pool
        .begin()
        .await
        .map_err(|error| format!("begin Guardian acknowledgement cleanup: {error}"))?;
    for id in ids {
        sqlx::query("DELETE FROM guardian_usage_acknowledgements WHERE vault_id = ? AND device_id = ? AND sample_id = ?")
            .bind(vault_id).bind(device_id).bind(id).execute(&mut *tx).await
            .map_err(|error| format!("remove confirmed Guardian acknowledgement: {error}"))?;
    }
    tx.commit()
        .await
        .map_err(|error| format!("commit Guardian acknowledgement cleanup: {error}"))?;
    pool.close().await;
    Ok(())
}

/// Read the whole accounting window, separately from the 200-row transport batch limit.
pub(crate) async fn accounting_source_days<R: Runtime>(
    app: &tauri::AppHandle<R>,
    vault_id: &str,
    device_id: &str,
    week: &str,
    local_date: &str,
) -> Result<Vec<crate::distractions::limits::UsageSourceDay>, String> {
    accounting_source_days_at(&spool_path(app)?, vault_id, device_id, week, local_date).await
}

async fn accounting_source_days_at(
    path: &Path,
    vault_id: &str,
    device_id: &str,
    week: &str,
    local_date: &str,
) -> Result<Vec<crate::distractions::limits::UsageSourceDay>, String> {
    use crate::distractions::limits::{MAX_SOURCE_GROUPS, MAX_WINDOW_SAMPLES, UsageSourceDay};
    let pool = open_spool(path).await?;
    let result = async {
        let mut tx = pool
            .begin()
            .await
            .map_err(|error| format!("begin linked usage accounting: {error}"))?;
        let source_sql = "SELECT source_type, source_key, local_date, elapsed_seconds
            FROM accepted_usage_samples WHERE vault_id = ? AND local_date >= ? AND local_date <= ?
            UNION ALL SELECT p.source_type, p.source_key, p.local_date, p.elapsed_seconds
            FROM pending_usage_samples p WHERE p.vault_id = ? AND p.device_id = ?
              AND p.local_date >= ? AND p.local_date <= ? AND NOT EXISTS (
                SELECT 1 FROM accepted_usage_samples a WHERE a.vault_id = p.vault_id
                  AND a.device_id = p.device_id AND a.sample_id = p.sample_id)";
        let count_sql = format!("SELECT COUNT(*) FROM ({source_sql} LIMIT ?)");
        let count: i64 = sqlx::query_scalar(&count_sql)
            .bind(vault_id)
            .bind(week)
            .bind(local_date)
            .bind(vault_id)
            .bind(device_id)
            .bind(week)
            .bind(local_date)
            .bind(MAX_WINDOW_SAMPLES + 1)
            .fetch_one(&mut *tx)
            .await
            .map_err(|error| format!("count linked usage window: {error}"))?;
        if count > MAX_WINDOW_SAMPLES {
            return Err("linked usage window exceeds the native sample limit".into());
        }
        let query = format!(
            "SELECT source_type, source_key, local_date, SUM(elapsed_seconds) AS elapsed_seconds
            FROM ({source_sql}) GROUP BY source_type, source_key, local_date
            ORDER BY local_date, source_type, source_key LIMIT ?"
        );
        let rows = sqlx::query(&query)
            .bind(vault_id)
            .bind(week)
            .bind(local_date)
            .bind(vault_id)
            .bind(device_id)
            .bind(week)
            .bind(local_date)
            .bind((MAX_SOURCE_GROUPS + 1) as i64)
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| format!("aggregate linked usage window: {error}"))?;
        if rows.len() > MAX_SOURCE_GROUPS {
            return Err("linked usage sources exceed the native limit".into());
        }
        let sources = rows
            .into_iter()
            .map(|row| {
                Ok(UsageSourceDay {
                    source_type: row
                        .try_get("source_type")
                        .map_err(|error| error.to_string())?,
                    source_key: row
                        .try_get("source_key")
                        .map_err(|error| error.to_string())?,
                    local_date: row
                        .try_get("local_date")
                        .map_err(|error| error.to_string())?,
                    elapsed_seconds: row
                        .try_get("elapsed_seconds")
                        .map_err(|error| error.to_string())?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        tx.commit()
            .await
            .map_err(|error| format!("finish linked usage accounting: {error}"))?;
        Ok(sources)
    }
    .await;
    pool.close().await;
    result
}

pub(crate) async fn import_linked_samples(
    pool: &SqlitePool,
    samples: &[DistractionsSampleMessage],
) -> Result<Vec<String>, String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin linked Distractions import: {error}"))?;
    for sample in samples {
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_usage_samples
                (id, source_type, source_key, display_name, started_at_ms,
                 elapsed_seconds, local_date, created_at_ms)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(canonical_sample_id(&sample.device_id, &sample.sample_id))
        .bind(&sample.source_type)
        .bind(&sample.source_key)
        .bind(&sample.display_name)
        .bind(sample.started_at_ms)
        .bind(sample.elapsed_seconds)
        .bind(&sample.local_date)
        .bind(sample.created_at_ms)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("import linked Distractions sample: {error}"))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit linked Distractions import: {error}"))?;
    Ok(samples
        .iter()
        .map(|sample| sample.sample_id.clone())
        .collect())
}

pub(crate) async fn aggregate_owner_samples(
    pool: &SqlitePool,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    let rows = sqlx::query(
        "SELECT source_type, source_key, MAX(display_name) AS display_name,
                MIN(started_at_ms) AS started_at_ms, SUM(elapsed_seconds) AS elapsed_seconds,
                local_date, MAX(created_at_ms) AS created_at_ms
         FROM distractions_usage_samples
         WHERE local_date IN (
             SELECT DISTINCT local_date FROM distractions_usage_samples
             ORDER BY local_date DESC LIMIT 8
         )
         GROUP BY source_type, source_key, local_date
         ORDER BY local_date ASC, source_type ASC, source_key ASC
         LIMIT ?",
    )
    .bind((MAX_DISTRACTIONS_SAMPLES + 1) as i64)
    .fetch_all(pool)
    .await
    .map_err(|error| format!("aggregate linked Distractions usage: {error}"))?;
    let mut samples = Vec::new();
    for row in rows {
        let source_type: String = row.try_get("source_type").map_err(|e| e.to_string())?;
        let source_key: String = row.try_get("source_key").map_err(|e| e.to_string())?;
        let local_date: String = row.try_get("local_date").map_err(|e| e.to_string())?;
        let mut elapsed_seconds: i64 = row
            .try_get::<i64, _>("elapsed_seconds")
            .map_err(|e| e.to_string())?;
        let identity = format!("{source_type}|{source_key}|{local_date}");
        let display_name: Option<String> =
            row.try_get("display_name").map_err(|e| e.to_string())?;
        let started_at_ms: i64 = row.try_get("started_at_ms").map_err(|e| e.to_string())?;
        let created_at_ms: i64 = row.try_get("created_at_ms").map_err(|e| e.to_string())?;
        let mut part = 0_u32;
        while elapsed_seconds > 0 {
            if samples.len() >= MAX_DISTRACTIONS_SAMPLES {
                return Err("combined Distractions usage exceeds the exchange limit".to_string());
            }
            let chunk = elapsed_seconds.min(86_400);
            samples.push(DistractionsSampleMessage {
                sample_id: format!("combined-{:x}-{part}", Sha256::digest(identity.as_bytes())),
                device_id: "combined-devices".to_string(),
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
    Ok(samples)
}

#[cfg(desktop)]
pub(crate) fn row_to_message(
    sample: &LinkedUsageRow,
    device_id: &str,
) -> DistractionsSampleMessage {
    DistractionsSampleMessage {
        sample_id: sample.id.clone(),
        device_id: device_id.to_string(),
        source_type: sample.source_type.clone(),
        source_key: sample.source_key.clone(),
        display_name: sample.display_name.clone(),
        started_at_ms: sample.started_at_ms,
        elapsed_seconds: sample.elapsed_seconds,
        local_date: sample.local_date.clone(),
        created_at_ms: sample.created_at_ms,
    }
}

fn canonical_sample_id(device_id: &str, sample_id: &str) -> String {
    format!(
        "linked-{:x}",
        Sha256::digest(format!("{device_id}|{sample_id}").as_bytes())
    )
}

#[cfg(all(test, desktop))]
async fn insert_message(
    pool: &SqlitePool,
    table: &str,
    vault_id: &str,
    sample: &DistractionsSampleMessage,
) -> Result<(), String> {
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    insert_message_executor(&mut transaction, table, vault_id, sample).await?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())
}

async fn insert_message_executor(
    executor: &mut sqlx::SqliteConnection,
    table: &str,
    vault_id: &str,
    sample: &DistractionsSampleMessage,
) -> Result<(), String> {
    if !matches!(table, "pending_usage_samples" | "accepted_usage_samples") {
        return Err("invalid Distractions spool table".to_string());
    }
    let query = format!(
        "INSERT OR IGNORE INTO {table}
            (sample_id, vault_id, device_id, source_type, source_key, display_name,
             started_at_ms, elapsed_seconds, local_date, created_at_ms)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    );
    sqlx::query(&query)
        .bind(&sample.sample_id)
        .bind(vault_id)
        .bind(&sample.device_id)
        .bind(&sample.source_type)
        .bind(&sample.source_key)
        .bind(&sample.display_name)
        .bind(sample.started_at_ms)
        .bind(sample.elapsed_seconds)
        .bind(&sample.local_date)
        .bind(sample.created_at_ms)
        .execute(executor)
        .await
        .map_err(|error| format!("store linked Distractions sample: {error}"))?;
    Ok(())
}

#[cfg(desktop)]
async fn read_messages(
    connection: &mut sqlx::SqliteConnection,
    table: &str,
    vault_id: &str,
    device_id: Option<&str>,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    read_messages_with_limit(
        connection,
        table,
        vault_id,
        device_id,
        EXCHANGE_BATCH_SAMPLES,
    )
    .await
}

#[cfg(desktop)]
async fn read_messages_with_limit(
    connection: &mut sqlx::SqliteConnection,
    table: &str,
    vault_id: &str,
    device_id: Option<&str>,
    maximum: i64,
) -> Result<Vec<DistractionsSampleMessage>, String> {
    if !matches!(table, "pending_usage_samples" | "accepted_usage_samples") {
        return Err("invalid Distractions spool table".to_string());
    }
    let query = format!(
        "SELECT sample_id, device_id, source_type, source_key, display_name,
                started_at_ms, elapsed_seconds, local_date, created_at_ms
         FROM {table}
         WHERE vault_id = ? AND (? IS NULL OR device_id = ?)
         ORDER BY created_at_ms ASC, sample_id ASC LIMIT ?"
    );
    let rows = sqlx::query(&query)
        .bind(vault_id)
        .bind(device_id)
        .bind(device_id)
        .bind(maximum)
        .fetch_all(connection)
        .await
        .map_err(|error| format!("read linked Distractions samples: {error}"))?;
    rows.into_iter()
        .map(|row| {
            Ok(DistractionsSampleMessage {
                sample_id: row.try_get("sample_id").map_err(|e| e.to_string())?,
                device_id: row.try_get("device_id").map_err(|e| e.to_string())?,
                source_type: row.try_get("source_type").map_err(|e| e.to_string())?,
                source_key: row.try_get("source_key").map_err(|e| e.to_string())?,
                display_name: row.try_get("display_name").map_err(|e| e.to_string())?,
                started_at_ms: row.try_get("started_at_ms").map_err(|e| e.to_string())?,
                elapsed_seconds: row.try_get("elapsed_seconds").map_err(|e| e.to_string())?,
                local_date: row.try_get("local_date").map_err(|e| e.to_string())?,
                created_at_ms: row.try_get("created_at_ms").map_err(|e| e.to_string())?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn idle_usage_drain_does_not_create_a_spool_before_any_samples_exist() {
        let path = temporary_spool("idle-without-samples");
        let canonical = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        // No canonical schema is needed when no device spool has been written.
        for _ in 0..3 {
            assert_eq!(
                drain_local_spool_at(&path, &canonical, "vault", "device")
                    .await
                    .unwrap(),
                0
            );
        }
        assert!(!path.parent().unwrap().exists());
        canonical.close().await;
    }

    fn temporary_spool(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "ganbaru-linked-usage-{name}-{}-{unique}",
                std::process::id()
            ))
            .join(SPOOL_FILE)
    }

    fn sample(id: &str, seconds: i64) -> DistractionsSampleMessage {
        DistractionsSampleMessage {
            sample_id: id.to_string(),
            device_id: "phone".to_string(),
            source_type: "mobile-app".to_string(),
            source_key: "com.example.video".to_string(),
            display_name: Some("Video".to_string()),
            started_at_ms: 1_700_000_000_000,
            elapsed_seconds: seconds,
            local_date: "2026-09-13".to_string(),
            created_at_ms: 1_700_000_001_000,
        }
    }

    fn native_sample(id: &str, seconds: i64) -> crate::distractions::DistractionsUsageSampleRow {
        let message = sample(id, seconds);
        crate::distractions::DistractionsUsageSampleRow {
            id: message.sample_id,
            source_type: message.source_type,
            source_key: message.source_key,
            display_name: message.display_name,
            started_at_ms: message.started_at_ms,
            elapsed_seconds: message.elapsed_seconds,
            local_date: message.local_date,
            created_at_ms: message.created_at_ms,
        }
    }

    #[tokio::test]
    async fn accepted_transport_snapshot_preserves_every_batch_and_rejects_oversized_cache() {
        let path = temporary_spool("accepted-complete");
        let pool = open_spool(&path).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        for index in 0..250 {
            insert_message_executor(
                &mut tx,
                "accepted_usage_samples",
                "vault",
                &sample(&format!("sample-{index}"), 2),
            )
            .await
            .unwrap();
        }
        insert_message_executor(
            &mut tx,
            "accepted_usage_samples",
            "other-vault",
            &sample("unrelated", 999),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(accepted_at(&path, "vault").await.unwrap().len(), 250);
        assert_eq!(
            accepted_at(&path, "other-vault").await.unwrap(),
            vec![sample("unrelated", 999)]
        );
        let mut tx = pool.begin().await.unwrap();
        for index in 250..=MAX_DISTRACTIONS_SAMPLES {
            insert_message_executor(
                &mut tx,
                "accepted_usage_samples",
                "vault",
                &sample(&format!("sample-{index}"), 2),
            )
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
        assert!(accepted_at(&path, "vault").await.is_err());
        pool.close().await;
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn exported_identities_survive_compaction_and_lost_acknowledgements() {
        let path = temporary_spool("export-compaction");
        enqueue_at(&path, "vault", "phone", &sample("sent", 10))
            .await
            .unwrap();
        assert_eq!(
            pending_at(&path, "vault", "phone").await.unwrap(),
            [sample("sent", 10)]
        );
        enqueue_at(&path, "vault", "phone", &sample("unsent-one", 2))
            .await
            .unwrap();
        enqueue_at(&path, "vault", "phone", &sample("unsent-two", 3))
            .await
            .unwrap();
        let pool = open_spool(&path).await.unwrap();
        compact_pending(&pool, "vault", "phone").await.unwrap();
        pool.close().await;
        let exported = pending_at(&path, "vault", "phone").await.unwrap();
        assert!(exported.contains(&sample("sent", 10)));
        assert_eq!(exported.len(), 2);
        assert_eq!(
            exported
                .iter()
                .map(|sample| sample.elapsed_seconds)
                .sum::<i64>(),
            15
        );
        assert_eq!(pending_at(&path, "vault", "phone").await.unwrap(), exported);
        let compact_id = exported
            .iter()
            .find(|sample| sample.sample_id != "sent")
            .unwrap()
            .sample_id
            .clone();
        acknowledge_at(
            &path,
            "vault",
            "phone",
            &["sent".into(), compact_id.clone()],
        )
        .await
        .unwrap();
        enqueue_at(&path, "vault", "phone", &sample("later-one", 2))
            .await
            .unwrap();
        enqueue_at(&path, "vault", "phone", &sample("later-two", 3))
            .await
            .unwrap();
        let pool = open_spool(&path).await.unwrap();
        compact_pending(&pool, "vault", "phone").await.unwrap();
        pool.close().await;
        let later = pending_at(&path, "vault", "phone").await.unwrap();
        assert_eq!(later.len(), 1);
        assert_ne!(later[0].sample_id, compact_id);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn native_batch_retry_after_acknowledgement_does_not_reinsert_usage() {
        let path = temporary_spool("native-receipt");
        let rows = [native_sample("one", 12), native_sample("two", 18)];
        enqueue_native_samples_at(&path, "vault", "phone", &rows)
            .await
            .unwrap();
        acknowledge_at(&path, "vault", "phone", &["one".into(), "two".into()])
            .await
            .unwrap();
        enqueue_native_samples_at(&path, "vault", "phone", &rows)
            .await
            .unwrap();
        assert!(
            pending_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        let changed = [native_sample("one", 13), native_sample("two", 18)];
        assert!(
            enqueue_native_samples_at(&path, "vault", "phone", &changed)
                .await
                .unwrap_err()
                .contains("changed its payload")
        );
        assert!(
            pending_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        enqueue_native_samples_at(&path, "vault", "phone", &[native_sample("next", 5)])
            .await
            .unwrap();
        assert_eq!(
            pending_at(&path, "vault", "phone").await.unwrap(),
            [sample("next", 5)]
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn failed_native_batch_rolls_back_all_rows_and_receipt_before_retry() {
        let path = temporary_spool("native-rollback");
        let pool = open_spool(&path).await.unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_native_batch BEFORE INSERT ON pending_usage_samples
            WHEN NEW.sample_id = 'two' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        let rows = [native_sample("one", 12), native_sample("two", 18)];
        assert!(
            enqueue_native_samples_at(&path, "vault", "phone", &rows)
                .await
                .is_err()
        );
        assert!(
            pending_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        let pool = open_spool(&path).await.unwrap();
        let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM native_usage_batch_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(receipts, 0);
        sqlx::query("DROP TRIGGER reject_native_batch")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        enqueue_native_samples_at(&path, "vault", "phone", &rows)
            .await
            .unwrap();
        assert_eq!(pending_at(&path, "vault", "phone").await.unwrap().len(), 2);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn accounting_reads_all_transport_batches_and_deduplicates_accepted_pending_identity() {
        let path = temporary_spool("accounting");
        let pool = open_spool(&path).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        for index in 0..250 {
            insert_message_executor(
                &mut tx,
                "accepted_usage_samples",
                "vault",
                &sample(&format!("accepted-{index}"), 2),
            )
            .await
            .unwrap();
            insert_message_executor(
                &mut tx,
                "pending_usage_samples",
                "vault",
                &sample(&format!("pending-{index}"), 3),
            )
            .await
            .unwrap();
        }
        insert_message_executor(
            &mut tx,
            "pending_usage_samples",
            "vault",
            &sample("accepted-0", 2),
        )
        .await
        .unwrap();
        let mut other_device = sample("other-device", 900);
        other_device.device_id = "other".into();
        insert_message_executor(&mut tx, "pending_usage_samples", "vault", &other_device)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        pool.close().await;
        let rows = accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].elapsed_seconds, 1250);
        assert!(
            accounting_source_days_at(
                &path,
                "different-vault",
                "phone",
                "2026-09-07",
                "2026-09-13"
            )
            .await
            .unwrap()
            .is_empty()
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn writable_accounting_drains_all_local_transport_batches_before_publication() {
        let path = temporary_spool("writable-batches");
        let spool = open_spool(&path).await.unwrap();
        let mut tx = spool.begin().await.unwrap();
        for index in 0..250 {
            insert_message_executor(
                &mut tx,
                "pending_usage_samples",
                "vault",
                &sample(&format!("local-{index}"), 2),
            )
            .await
            .unwrap();
        }
        tx.commit().await.unwrap();
        spool.close().await;
        let canonical = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE distractions_usage_samples (
            id TEXT PRIMARY KEY, source_type TEXT NOT NULL, source_key TEXT NOT NULL,
            display_name TEXT, started_at_ms INTEGER NOT NULL, elapsed_seconds INTEGER NOT NULL,
            local_date TEXT NOT NULL, created_at_ms INTEGER NOT NULL)",
        )
        .execute(&canonical)
        .await
        .unwrap();
        assert_eq!(
            drain_local_spool_at(&path, &canonical, "vault", "phone")
                .await
                .unwrap(),
            250
        );
        let totals: (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), SUM(elapsed_seconds) FROM distractions_usage_samples")
                .fetch_one(&canonical)
                .await
                .unwrap();
        assert_eq!(totals, (250, 500));
        assert!(
            pending_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            drain_local_spool_at(&path, &canonical, "vault", "phone")
                .await
                .unwrap(),
            0
        );
        canonical.close().await;
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn failed_owner_snapshot_rolls_back_acknowledgement_and_retry_preserves_totals() {
        let path = temporary_spool("atomic-accounting");
        replace_accepted_at(&path, "vault", &[sample("combined-old", 30)])
            .await
            .unwrap();
        enqueue_at(&path, "vault", "phone", &sample("one", 12))
            .await
            .unwrap();
        enqueue_at(&path, "vault", "phone", &sample("two", 18))
            .await
            .unwrap();
        let pool = open_spool(&path).await.unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_owner_snapshot BEFORE INSERT ON accepted_usage_samples
            WHEN NEW.sample_id = 'combined-new' BEGIN SELECT RAISE(ABORT, 'snapshot failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        assert!(
            apply_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                &["one".into()],
                &[sample("combined-new", 42)]
            )
            .await
            .is_err()
        );
        assert_eq!(pending_at(&path, "vault", "phone").await.unwrap().len(), 2);
        let rows = accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
            .await
            .unwrap();
        assert_eq!(rows[0].elapsed_seconds, 60);
        let pool = open_spool(&path).await.unwrap();
        sqlx::query("DROP TRIGGER reject_owner_snapshot")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        for _ in 0..2 {
            apply_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                &["one".into()],
                &[sample("combined-new", 42)],
            )
            .await
            .unwrap();
            let rows =
                accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
                    .await
                    .unwrap();
            assert_eq!(rows[0].elapsed_seconds, 60);
            assert_eq!(
                pending_at(&path, "vault", "phone").await.unwrap(),
                vec![sample("two", 18)]
            );
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn pending_samples_survive_restart_and_only_acknowledged_ids_are_removed() {
        let path = temporary_spool("pending");
        enqueue_at(&path, "vault", "phone", &sample("one", 12))
            .await
            .expect("enqueue first");
        enqueue_at(&path, "vault", "phone", &sample("two", 18))
            .await
            .expect("enqueue second");

        assert_eq!(
            pending_at(&path, "vault", "phone")
                .await
                .expect("read after restart")
                .len(),
            2
        );
        acknowledge_at(&path, "vault", "phone", &["one".to_string()])
            .await
            .expect("acknowledge");
        let remaining = pending_at(&path, "vault", "phone")
            .await
            .expect("read remaining");
        assert_eq!(remaining, vec![sample("two", 18)]);
        std::fs::remove_dir_all(path.parent().expect("spool parent")).expect("remove temp spool");
    }

    #[tokio::test]
    async fn guardian_snapshot_and_external_acknowledgements_survive_lost_response_and_restart() {
        let path = temporary_spool("guardian-ack-retry");
        replace_accepted_at(&path, "vault", &[sample("old", 30)])
            .await
            .unwrap();
        let pool = open_spool(&path).await.unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER reject_guardian_snapshot BEFORE INSERT ON accepted_usage_samples
            WHEN NEW.sample_id = 'new' BEGIN SELECT RAISE(ABORT, 'snapshot failure'); END",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
        assert!(
            commit_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                &["local".into()],
                &[sample("new", 42)],
                AcknowledgementTarget::Guardian
            )
            .await
            .is_err()
        );
        assert!(
            guardian_acknowledgements_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
                .await
                .unwrap()[0]
                .elapsed_seconds,
            30
        );
        let pool = open_spool(&path).await.unwrap();
        sqlx::query("DROP TRIGGER reject_guardian_snapshot")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        for _ in 0..2 {
            commit_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                &["local".into()],
                &[sample("new", 42)],
                AcknowledgementTarget::Guardian,
            )
            .await
            .unwrap();
            assert_eq!(
                guardian_acknowledgements_at(&path, "vault", "phone")
                    .await
                    .unwrap(),
                vec!["local"]
            );
            assert!(
                guardian_acknowledgements_at(&path, "other-vault", "phone")
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(
                guardian_acknowledgements_at(&path, "vault", "other-device")
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
                    .await
                    .unwrap()[0]
                    .elapsed_seconds,
                42
            );
        }
        forget_guardian_acknowledgements_at(&path, "other-vault", "phone", &["local".into()])
            .await
            .unwrap();
        assert_eq!(
            guardian_acknowledgements_at(&path, "vault", "phone")
                .await
                .unwrap(),
            vec!["local"]
        );
        forget_guardian_acknowledgements_at(&path, "vault", "phone", &["local".into()])
            .await
            .unwrap();
        assert!(
            guardian_acknowledgements_at(&path, "vault", "phone")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
                .await
                .unwrap()[0]
                .elapsed_seconds,
            42
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn guardian_acknowledgement_capacity_rolls_back_the_new_snapshot() {
        let path = temporary_spool("guardian-ack-capacity");
        let ids: Vec<String> = (0..MAX_GUARDIAN_ACKNOWLEDGEMENTS)
            .map(|index| format!("event-{index}"))
            .collect();
        for batch in ids.chunks(MAX_DISTRACTIONS_SAMPLES) {
            commit_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                batch,
                &[sample("old", 30)],
                AcknowledgementTarget::Guardian,
            )
            .await
            .unwrap();
        }
        assert!(
            commit_owner_snapshot_at(
                &path,
                "vault",
                "phone",
                &["overflow".into()],
                &[sample("new", 60)],
                AcknowledgementTarget::Guardian
            )
            .await
            .is_err()
        );
        assert_eq!(
            guardian_acknowledgements_at(&path, "vault", "phone")
                .await
                .unwrap()
                .len(),
            MAX_GUARDIAN_ACKNOWLEDGEMENTS as usize
        );
        assert_eq!(
            accounting_source_days_at(&path, "vault", "phone", "2026-09-07", "2026-09-13")
                .await
                .unwrap()[0]
                .elapsed_seconds,
            30
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[tokio::test]
    async fn accepted_combined_samples_replace_atomically_without_duplication() {
        let path = temporary_spool("accepted");
        replace_accepted_at(&path, "vault", &[sample("combined-one", 30)])
            .await
            .expect("store first snapshot");
        replace_accepted_at(&path, "vault", &[sample("combined-two", 45)])
            .await
            .expect("replace snapshot");

        let pool = open_spool(&path).await.expect("open cache");
        let mut connection = pool.acquire().await.unwrap();
        let accepted = read_messages(&mut connection, "accepted_usage_samples", "vault", None)
            .await
            .expect("read cache");
        assert_eq!(accepted, vec![sample("combined-two", 45)]);
        drop(connection);
        pool.close().await;
        std::fs::remove_dir_all(path.parent().expect("spool parent")).expect("remove temp spool");
    }

    #[tokio::test]
    async fn retried_device_samples_commit_once_and_aggregate_with_owner_usage() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("open usage database");
        sqlx::query(
            "CREATE TABLE distractions_usage_samples (
                id TEXT PRIMARY KEY,
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
        .expect("create usage table");
        let remote = sample("retry-stable", 18);
        import_linked_samples(&pool, std::slice::from_ref(&remote))
            .await
            .expect("first import");
        import_linked_samples(&pool, std::slice::from_ref(&remote))
            .await
            .expect("retry import");
        sqlx::query(
            "INSERT INTO distractions_usage_samples
                (id, source_type, source_key, display_name, started_at_ms,
                 elapsed_seconds, local_date, created_at_ms)
             VALUES ('owner', 'mobile-app', 'com.example.video', 'Video', 1, 12,
                     '2026-09-13', 2)",
        )
        .execute(&pool)
        .await
        .expect("insert owner sample");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM distractions_usage_samples")
            .fetch_one(&pool)
            .await
            .expect("count committed samples");
        assert_eq!(count, 2);
        let combined = aggregate_owner_samples(&pool)
            .await
            .expect("aggregate usage");
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].elapsed_seconds, 30);
    }

    #[tokio::test]
    async fn spool_compaction_preserves_the_exact_unacknowledged_total() {
        let path = temporary_spool("compaction");
        enqueue_at(&path, "vault", "phone", &sample("one", 12))
            .await
            .expect("enqueue first");
        enqueue_at(&path, "vault", "phone", &sample("two", 18))
            .await
            .expect("enqueue second");
        let pool = open_spool(&path).await.expect("open spool");
        compact_pending(&pool, "vault", "phone")
            .await
            .expect("compact spool");
        pool.close().await;

        let compacted = pending_at(&path, "vault", "phone")
            .await
            .expect("read compacted spool");
        assert_eq!(compacted.len(), 1);
        assert_eq!(compacted[0].elapsed_seconds, 30);
        std::fs::remove_dir_all(path.parent().expect("spool parent")).expect("remove temp spool");
    }
}

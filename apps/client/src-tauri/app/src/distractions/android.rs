#[cfg(target_os = "android")]
use crate::db::connect_sqlite;
#[cfg(target_os = "android")]
use crate::vault::{self, handoff::pairing::PairingManager, ownership::VaultOwnershipManager};
use chrono::{DateTime, SecondsFormat, Utc};
#[cfg(target_os = "android")]
use ganbaru_mobile_distractions::{MobileDistractionsExt, PendingEvent};
#[cfg(not(target_os = "android"))]
use serde::Deserialize;
use serde::Serialize;
use sqlx::SqlitePool;
#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::Runtime;

const ACTIVE_DB_URL: &str = "sqlite:ganbaru-ai.sqlite";
const MAX_BATCH: usize = 200;
const MAX_JOURNAL_EVENTS: usize = 2_000;
#[cfg(target_os = "android")]
const MAX_DRAIN_BATCHES: usize = MAX_JOURNAL_EVENTS / MAX_BATCH + 1;

mod projection;
#[cfg(target_os = "android")]
pub(crate) mod runtime;

/// Update localized notification text without accepting frontend enforcement policy.
#[tauri::command]
pub fn distractions_mobile_update_copy<R: Runtime>(
    app: tauri::AppHandle<R>,
    copy: projection::NotificationCopy,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        runtime::update_copy(app, copy)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, copy);
        Err("Guardian is available only on Android".into())
    }
}

/// Read the independent native owner's last accepted, bounded usage projection.
#[tauri::command]
pub fn distractions_mobile_load_usage_projection<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    {
        runtime::usage_projection(app)
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Err("Guardian is available only on Android".into())
    }
}

#[cfg(target_os = "android")]
static GUARDIAN_SYNC: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(not(target_os = "android"))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingEvent {
    id: String,
    kind: String,
    package_name: String,
    display_name: String,
    started_at_ms: i64,
    elapsed_seconds: i64,
    local_date: String,
    occurred_at_ms: i64,
    reason: Option<String>,
    rule_id: Option<String>,
    run_id: Option<String>,
    phase: Option<String>,
    vault_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileDistractionsSyncResult {
    imported: usize,
    full_batch: bool,
}

fn is_valid_package_name(value: &str) -> bool {
    if value.len() < 3 || value.len() > 255 {
        return false;
    }
    let mut segments = value.split('.');
    let mut count = 0;
    for segment in &mut segments {
        count += 1;
        let mut chars = segment.chars();
        if !chars
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            return false;
        }
        if !chars.all(|character| character.is_ascii_alphanumeric() || character == '_') {
            return false;
        }
    }
    count >= 2
}

fn normalize_event(mut event: PendingEvent) -> Result<PendingEvent, String> {
    if event.id.is_empty()
        || event.id.len() > 120
        || event.id.trim() != event.id
        || event.id.chars().any(char::is_control)
    {
        return Err("mobile Distractions event ID is invalid".to_string());
    }
    if event.kind != "usage" && event.kind != "block" {
        return Err("mobile Distractions event kind is invalid".to_string());
    }
    event.package_name = event.package_name.trim().to_ascii_lowercase();
    if !is_valid_package_name(&event.package_name) {
        return Err("mobile Distractions package name is invalid".to_string());
    }
    event.display_name = event
        .display_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(120)
        .collect();
    if event.display_name.is_empty() {
        return Err("mobile Distractions display name is required".to_string());
    }
    if !crate::distractions::limits::is_valid_local_date(&event.local_date) {
        return Err("mobile Distractions local date is invalid".to_string());
    }
    if event.started_at_ms < 0 || event.occurred_at_ms < 0 {
        return Err("mobile Distractions timestamp is invalid".to_string());
    }
    if event.kind == "usage" && !(1..=86_400).contains(&event.elapsed_seconds) {
        return Err("mobile Distractions elapsed time is invalid".to_string());
    }
    if event.kind == "block" && event.elapsed_seconds != 0 {
        return Err("mobile Distractions block duration must be zero".to_string());
    }
    if !matches!(
        event.phase.as_deref(),
        None | Some("focus") | Some("short_break") | Some("long_break")
    ) {
        return Err("mobile Distractions phase is invalid".to_string());
    }
    event.reason = event
        .reason
        .map(|value| value.trim().chars().take(80).collect())
        .filter(|value: &String| !value.is_empty());
    event.rule_id = event
        .rule_id
        .map(|value| value.trim().chars().take(80).collect())
        .filter(|value: &String| !value.is_empty());
    event.run_id = event
        .run_id
        .map(|value| value.trim().chars().take(128).collect())
        .filter(|value: &String| !value.is_empty());
    if event.vault_id.is_empty()
        || event.vault_id.len() > 128
        || event.vault_id.trim() != event.vault_id
        || event.vault_id.chars().any(char::is_control)
    {
        return Err("mobile Distractions vault ID is invalid".to_string());
    }
    Ok(event)
}

async fn import_events(pool: &SqlitePool, events: &[PendingEvent]) -> Result<(), String> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin mobile Distractions import: {error}"))?;
    for event in events {
        if event.kind == "usage" {
            sqlx::query(
                "INSERT OR IGNORE INTO distractions_usage_samples
                    (id, source_type, source_key, display_name, started_at_ms,
                     elapsed_seconds, local_date, created_at_ms)
                 VALUES (?, 'mobile-app', ?, ?, ?, ?, ?, ?)",
            )
            .bind(&event.id)
            .bind(&event.package_name)
            .bind(&event.display_name)
            .bind(event.started_at_ms)
            .bind(event.elapsed_seconds)
            .bind(&event.local_date)
            .bind(event.occurred_at_ms)
            .execute(&mut *transaction)
            .await
            .map_err(|error| format!("import mobile Distractions usage: {error}"))?;
            continue;
        }

        let run_id = if let Some(run_id) = &event.run_id {
            let exists =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pomodoro_runs WHERE id = ?")
                    .bind(run_id)
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(|error| format!("validate mobile Distractions run: {error}"))?
                    > 0;
            exists.then_some(run_id.clone())
        } else {
            None
        };
        let occurred_at = DateTime::<Utc>::from_timestamp_millis(event.occurred_at_ms)
            .ok_or_else(|| "mobile Distractions block timestamp is invalid".to_string())?
            .to_rfc3339_opts(SecondsFormat::Millis, true);
        let decision = if event.reason.as_deref() == Some("usage_limit") {
            "limit_exhausted"
        } else {
            "blocked"
        };
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_block_events
                (id, run_id, segment_id, occurred_at, source_type, source_key,
                 display_name, phase, decision, rule_id, category_id)
             VALUES (?, ?, NULL, ?, 'mobile_app', ?, ?, ?, ?, ?, NULL)",
        )
        .bind(&event.id)
        .bind(run_id)
        .bind(occurred_at)
        .bind(&event.package_name)
        .bind(&event.display_name)
        .bind(&event.phase)
        .bind(decision)
        .bind(&event.rule_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("import mobile Distractions block: {error}"))?;

        let rule_kind = if event.reason.as_deref() == Some("usage_limit") {
            "usage_limit"
        } else {
            "mobile_app"
        };
        let blocker_mode = if rule_kind == "usage_limit" {
            "limit"
        } else {
            "blacklist"
        };
        sqlx::query(
            "INSERT OR IGNORE INTO distractions_block_event_rule_snapshots
                (block_event_id, rule_id, rule_kind, rule_label, environment_id, blocker_mode)
             VALUES (?, ?, ?, ?, NULL, ?)",
        )
        .bind(&event.id)
        .bind(&event.rule_id)
        .bind(rule_kind)
        .bind(&event.display_name)
        .bind(blocker_mode)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("import mobile Distractions rule snapshot: {error}"))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit mobile Distractions import: {error}"))
}

/// Exclude both canonical local identities and peer receipts retained across ownership transfer.
#[cfg(any(target_os = "android", test))]
async fn canonical_pending_ids(
    connection: &mut sqlx::SqliteConnection,
    capture: &projection::GuardianCapture,
    retained: Vec<String>,
) -> Result<std::collections::HashSet<String>, String> {
    let mut ids: std::collections::HashSet<_> = retained.into_iter().collect();
    for batch in capture.pending.chunks(MAX_BATCH) {
        let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id FROM distractions_usage_samples WHERE id IN (",
        );
        let mut values = query.separated(",");
        for event in batch {
            values.push_bind(&event.id);
        }
        values.push_unseparated(")");
        let imported: Vec<String> = query
            .build_query_scalar()
            .fetch_all(&mut *connection)
            .await
            .map_err(|error| format!("deduplicate Guardian canonical usage: {error}"))?;
        ids.extend(imported);
    }
    Ok(ids)
}

#[cfg(target_os = "android")]
/// Drain the bounded native journal before a canonical vault snapshot, without a frontend command.
pub(crate) async fn synchronize_for_snapshot<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let _sync = GUARDIAN_SYNC.lock().await;
    for _ in 0..MAX_DRAIN_BATCHES {
        if !synchronize_locked(app.clone()).await?.full_batch {
            return Ok(());
        }
    }
    Err("Guardian journal still has a full batch; retry the vault snapshot".into())
}

#[cfg(target_os = "android")]
async fn synchronize_locked<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<MobileDistractionsSyncResult, String> {
    let active_vault_id = vault::active_vault_id(&app)?;
    let ownership = app
        .state::<VaultOwnershipManager>()
        .status(&active_vault_id)?;
    let retained = crate::distractions::linked::guardian_acknowledgements(
        &app,
        &active_vault_id,
        &ownership.device_id,
    )
    .await?;
    for ids in retained.chunks(MAX_BATCH) {
        acknowledge_guardian(&app, ids).await?;
        crate::distractions::linked::forget_guardian_acknowledgements(
            &app,
            &active_vault_id,
            &ownership.device_id,
            ids,
        )
        .await?;
    }
    let pending_app = app.clone();
    let pending_vault = active_vault_id.clone();
    let usage_only = !ownership.can_write;
    let events = tauri::async_runtime::spawn_blocking(move || {
        pending_app
            .mobile_distractions()
            .pending_events(&pending_vault, usage_only)
    })
    .await
    .map_err(|error| format!("Guardian pending worker: {error}"))??;
    if events.len() > MAX_BATCH {
        return Err("mobile Distractions journal batch is too large".to_string());
    }
    let normalized_events = events
        .into_iter()
        .map(normalize_event)
        .collect::<Result<Vec<_>, _>>()?;
    if normalized_events
        .iter()
        .any(|event| event.vault_id != active_vault_id)
    {
        return Err("Guardian returned events from a different vault".into());
    }
    let normalized = normalized_events
        .into_iter()
        .filter(|event| event.vault_id == active_vault_id)
        .collect::<Vec<_>>();
    let pairing = app.state::<PairingManager>().inner().clone();
    let linked = pairing.coordinator_pin()?.is_some();
    let mut imported = 0;
    let mut full_batch = false;

    if ownership.can_write {
        full_batch = normalized.len() == MAX_BATCH;
        if !normalized.is_empty() {
            let _permit = app
                .state::<VaultOwnershipManager>()
                .acquire_managed_write(&active_vault_id)?;
            if vault::active_vault_id(&app)? != active_vault_id {
                return Err("mobile Distractions vault changed before import".into());
            }
            let pool = connect_sqlite(app.clone(), ACTIVE_DB_URL.to_string()).await?;
            if vault::active_vault_id(&app)? != active_vault_id {
                return Err("mobile Distractions vault changed while connecting".into());
            }
            import_events(&pool, &normalized).await?;
            let processed_ids = normalized
                .iter()
                .map(|event| event.id.clone())
                .collect::<Vec<_>>();
            acknowledge_guardian(&app, &processed_ids).await?;
            imported += normalized.len();
        }
        if linked {
            if let Err(error) = exchange_as_owner(&app, &pairing, &active_vault_id).await {
                eprintln!("linked Distractions exchange deferred: {error}");
            }
        }
    } else if linked {
        let usage_events = normalized
            .iter()
            .filter(|event| event.kind == "usage")
            .cloned()
            .collect::<Vec<_>>();
        let samples = usage_events
            .iter()
            .map(|event| pending_event_message(event, &ownership.device_id))
            .collect();
        match crate::vault::handoff::transport::exchange_distractions(
            &pairing,
            samples,
            Vec::new(),
            Vec::new(),
        )
        .await
        {
            Ok((acknowledged, _, combined)) => {
                validate_guardian_acknowledgements(&usage_events, &acknowledged)?;
                crate::distractions::linked::apply_guardian_owner_snapshot(
                    &app,
                    &active_vault_id,
                    &ownership.device_id,
                    &acknowledged,
                    &combined,
                )
                .await?;
                acknowledge_guardian(&app, &acknowledged).await?;
                crate::distractions::linked::forget_guardian_acknowledgements(
                    &app,
                    &active_vault_id,
                    &ownership.device_id,
                    &acknowledged,
                )
                .await?;
                imported += acknowledged.len();
                full_batch = acknowledged.len() == MAX_BATCH;
            }
            Err(error) => eprintln!("linked Distractions exchange deferred: {error}"),
        }
    }
    Ok(MobileDistractionsSyncResult {
        imported,
        full_batch,
    })
}

/// Await one blocking Guardian acknowledgement without occupying an async runtime worker.
#[cfg(target_os = "android")]
async fn acknowledge_guardian<R: Runtime>(
    app: &tauri::AppHandle<R>,
    ids: &[String],
) -> Result<(), String> {
    let app = app.clone();
    let ids = ids.to_vec();
    tauri::async_runtime::spawn_blocking(move || app.mobile_distractions().acknowledge_events(&ids))
        .await
        .map_err(|error| format!("Guardian acknowledgement worker: {error}"))?
}

/// An owner may acknowledge only identities in this immutable outgoing batch.
#[cfg(any(target_os = "android", test))]
fn validate_guardian_acknowledgements(
    events: &[PendingEvent],
    ids: &[String],
) -> Result<(), String> {
    let sent: std::collections::HashSet<&str> =
        events.iter().map(|event| event.id.as_str()).collect();
    let mut seen = std::collections::HashSet::new();
    if ids.len() > MAX_BATCH
        || ids
            .iter()
            .any(|id| !sent.contains(id.as_str()) || !seen.insert(id.as_str()))
    {
        return Err("owner acknowledged an unknown or repeated Guardian usage identity".into());
    }
    Ok(())
}

#[cfg(target_os = "android")]
async fn exchange_as_owner<R: Runtime>(
    app: &tauri::AppHandle<R>,
    pairing: &PairingManager,
    vault_id: &str,
) -> Result<(), String> {
    let _permit = app
        .state::<VaultOwnershipManager>()
        .acquire_managed_write(vault_id)?;
    if vault::active_vault_id(app)? != vault_id {
        return Err("mobile Distractions vault changed before owner exchange".into());
    }
    let pool = connect_sqlite(app.clone(), ACTIVE_DB_URL.to_string()).await?;
    if vault::active_vault_id(app)? != vault_id {
        return Err("mobile Distractions vault changed while connecting for owner exchange".into());
    }
    let snapshot = owner_snapshot(&pool).await?;
    let (_, peer_samples, combined) = crate::vault::handoff::transport::exchange_distractions(
        pairing,
        Vec::new(),
        Vec::new(),
        snapshot,
    )
    .await?;
    if peer_samples.is_empty() {
        let device = app
            .state::<VaultOwnershipManager>()
            .status(vault_id)?
            .device_id;
        return crate::distractions::linked::apply_owner_snapshot(
            app,
            vault_id,
            &device,
            &[],
            &combined,
        )
        .await;
    }
    let acknowledged =
        crate::distractions::linked::import_linked_samples(&pool, &peer_samples).await?;
    let refreshed = owner_snapshot(&pool).await?;
    let (_, _, combined) = crate::vault::handoff::transport::exchange_distractions(
        pairing,
        Vec::new(),
        acknowledged,
        refreshed,
    )
    .await?;
    let device = app
        .state::<VaultOwnershipManager>()
        .status(vault_id)?
        .device_id;
    crate::distractions::linked::apply_owner_snapshot(app, vault_id, &device, &[], &combined).await
}

#[cfg(target_os = "android")]
async fn owner_snapshot(
    pool: &SqlitePool,
) -> Result<Vec<crate::vault::handoff::protocol::DistractionsSampleMessage>, String> {
    crate::distractions::linked::aggregate_owner_samples(pool).await
}

#[cfg(target_os = "android")]
fn pending_event_message(
    event: &PendingEvent,
    device_id: &str,
) -> crate::vault::handoff::protocol::DistractionsSampleMessage {
    crate::vault::handoff::protocol::DistractionsSampleMessage {
        sample_id: event.id.clone(),
        device_id: device_id.to_string(),
        source_type: "mobile-app".to_string(),
        source_key: event.package_name.clone(),
        display_name: Some(event.display_name.clone()),
        started_at_ms: event.started_at_ms,
        elapsed_seconds: event.elapsed_seconds,
        local_date: event.local_date.clone(),
        created_at_ms: event.occurred_at_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str) -> PendingEvent {
        PendingEvent {
            id: id.into(),
            kind: "usage".into(),
            package_name: "com.example.video".into(),
            display_name: "Video".into(),
            started_at_ms: 1_000,
            elapsed_seconds: 1,
            local_date: "2026-10-02".into(),
            occurred_at_ms: 2_000,
            reason: None,
            rule_id: None,
            run_id: None,
            phase: None,
            vault_id: "vault".into(),
        }
    }

    #[test]
    fn invalid_identities_are_rejected_instead_of_truncated_into_another_receipt() {
        assert!(normalize_event(event(&"a".repeat(121))).is_err());
        assert!(normalize_event(event(" accepted ")).is_err());
        let mut invalid_vault = event("one");
        invalid_vault.vault_id = "v".repeat(129);
        assert!(normalize_event(invalid_vault).is_err());
        assert_eq!(normalize_event(event("one")).unwrap().id, "one");
    }

    #[test]
    fn owner_acknowledgements_are_unique_and_belong_to_the_sent_batch() {
        let sent = vec![event("one"), event("two")];
        assert!(validate_guardian_acknowledgements(&sent, &["two".into()]).is_ok());
        assert!(validate_guardian_acknowledgements(&sent, &["unknown".into()]).is_err());
        assert!(validate_guardian_acknowledgements(&sent, &["one".into(), "one".into()]).is_err());
    }

    #[test]
    fn package_validation_requires_bounded_java_segments() {
        assert!(is_valid_package_name("com.example.video"));
        assert!(is_valid_package_name("app_1.social.feed2"));
        assert!(!is_valid_package_name("android"));
        assert!(!is_valid_package_name("1com.example"));
        assert!(!is_valid_package_name("com.example-app"));
    }

    #[tokio::test]
    async fn writable_accounting_retains_peer_receipt_exclusions_after_ownership_transfer() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE distractions_usage_samples (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO distractions_usage_samples VALUES ('local'), ('linked-peer')")
            .execute(&pool)
            .await
            .unwrap();
        let now = "2026-10-02T12:00:00Z"
            .parse::<jiff::Timestamp>()
            .unwrap()
            .as_millisecond();
        let pending: Vec<_> = ["local", "peer", "new"].into_iter().map(|id| serde_json::json!({
            "id": id, "kind": "usage", "packageName": "com.example.video", "displayName": "Video",
            "startedAtMs": now - 1_000, "elapsedSeconds": 1, "occurredAtMs": now,
            "localDate": "2026-10-02", "vaultId": "vault",
        })).collect();
        let capture = projection::GuardianCapture::decode(&serde_json::json!({
            "vaultId": "vault", "journalVaultId": "vault", "observedAtEpochMs": now,
            "utcOffsetSeconds": 0, "localDate": "2026-10-02", "weekStartLocalDate": "2026-09-28",
            "localSources": [], "pending": pending,
        }).to_string(), "vault", now).unwrap();
        let mut tx = pool.begin().await.unwrap();
        let excluded = canonical_pending_ids(&mut tx, &capture, vec!["peer".into()])
            .await
            .unwrap();
        assert_eq!(
            excluded,
            std::collections::HashSet::from(["local".into(), "peer".into()])
        );
        let sources = projection::include_pending(
            vec![crate::distractions::limits::UsageSourceDay {
                source_type: "mobile-app".into(),
                source_key: "com.example.video".into(),
                local_date: "2026-10-02".into(),
                elapsed_seconds: 30,
            }],
            &capture,
            &excluded,
        )
        .unwrap();
        assert_eq!(sources[0].elapsed_seconds, 31);
        tx.commit().await.unwrap();
        sqlx::query("DROP TABLE distractions_usage_samples")
            .execute(&pool)
            .await
            .unwrap();
        let mut connection = pool.acquire().await.unwrap();
        assert!(
            canonical_pending_ids(&mut connection, &capture, vec!["peer".into()])
                .await
                .is_err()
        );
        drop(connection);
        pool.close().await;
    }

    #[tokio::test]
    async fn journal_import_rolls_back_usage_and_block_history_when_rule_snapshot_fails() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .unwrap();
        ganbaru_db::run_migrations(&pool).await.unwrap();
        let usage = event("usage-one");
        let mut block = event("block-one");
        block.kind = "block".into();
        block.elapsed_seconds = 0;
        block.reason = Some("usage_limit".into());
        let batch = vec![usage, block];
        sqlx::raw_sql("CREATE TRIGGER reject_mobile_rule_snapshot BEFORE INSERT ON distractions_block_event_rule_snapshots
            BEGIN SELECT RAISE(ABORT, 'rule snapshot failure'); END")
            .execute(&pool).await.unwrap();
        assert!(import_events(&pool, &batch).await.is_err());
        for table in [
            "distractions_usage_samples",
            "distractions_block_events",
            "distractions_block_event_rule_snapshots",
        ] {
            let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(
                count, 0,
                "{table} must roll back with the rejected journal batch"
            );
        }
        sqlx::query("DROP TRIGGER reject_mobile_rule_snapshot")
            .execute(&pool)
            .await
            .unwrap();
        for _ in 0..2 {
            import_events(&pool, &batch).await.unwrap();
            for table in [
                "distractions_usage_samples",
                "distractions_block_events",
                "distractions_block_event_rule_snapshots",
            ] {
                let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
                assert_eq!(
                    count, 1,
                    "{table} must preserve identity on a lost-response retry"
                );
            }
        }
        pool.close().await;
    }
}

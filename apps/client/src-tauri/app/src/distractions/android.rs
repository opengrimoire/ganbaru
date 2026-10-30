#[cfg(target_os = "android")]
use crate::vault;
#[cfg(target_os = "android")]
use ganbaru_mobile_distractions::{MobileDistractionsExt, PendingEvent};
#[cfg(not(target_os = "android"))]
use serde::Deserialize;
use serde::Serialize;
#[cfg(target_os = "android")]
use tauri::Manager;
use tauri::Runtime;

const MAX_BATCH: usize = 200;
const MAX_JOURNAL_EVENTS: usize = 2_000;
#[cfg(target_os = "android")]
const MAX_DRAIN_BATCHES: usize = MAX_JOURNAL_EVENTS / MAX_BATCH + 1;

mod journal;
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

/// The pending Guardian identities the usage journal already holds, which must not count twice.
#[cfg(any(target_os = "android", test))]
async fn canonical_pending_ids(
    connection: &mut sqlx::SqliteConnection,
    capture: &projection::GuardianCapture,
) -> Result<std::collections::HashSet<String>, String> {
    let mut ids = std::collections::HashSet::new();
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

/// Imports the whole bounded Guardian journal and forces this device's stale usage totals into
/// the vault, before a vault snapshot or when the app moves to the background.
#[cfg(target_os = "android")]
pub(crate) async fn drain_and_flush_usage<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<(), String> {
    let _sync = GUARDIAN_SYNC.lock().await;
    for _ in 0..MAX_DRAIN_BATCHES {
        if !synchronize_locked(app.clone(), true).await?.full_batch {
            return Ok(());
        }
    }
    Err("Guardian journal still has a full batch; retry the usage flush".into())
}

/// Imports one Guardian journal batch into the vault, then flushes this device's due usage
/// totals; `force` flushes every stale total. Usage and block history both replicate, so every
/// linked device imports both.
#[cfg(target_os = "android")]
async fn synchronize_locked<R: Runtime>(
    app: tauri::AppHandle<R>,
    force: bool,
) -> Result<MobileDistractionsSyncResult, String> {
    let active_vault_id = vault::active_vault_id(&app)?;
    let device_id = vault::ensure_device_id(&app)?;
    let pending_app = app.clone();
    let pending_vault = active_vault_id.clone();
    let events = tauri::async_runtime::spawn_blocking(move || {
        pending_app
            .mobile_distractions()
            .pending_events(&pending_vault)
    })
    .await
    .map_err(|error| format!("Guardian pending worker: {error}"))??;
    if events.len() > MAX_BATCH {
        return Err("mobile Distractions journal batch is too large".to_string());
    }
    let full_batch = events.len() == MAX_BATCH;
    let _permit = app
        .state::<vault::write_fence::VaultWriteFence>()
        .acquire_managed_write()?;
    if vault::active_vault_id(&app)? != active_vault_id {
        return Err("mobile Distractions vault changed before import".into());
    }
    let pool = crate::db::connect_vault(&app).await?;
    if vault::active_vault_id(&app)? != active_vault_id {
        return Err("mobile Distractions vault changed while connecting".into());
    }
    let imported = if events.is_empty() {
        Ok(0)
    } else {
        let window = crate::distractions::usage_totals::current_usage_date_window();
        match journal::import_journal_batch(&pool, events, &active_vault_id, &window).await {
            Ok(import) => acknowledge_guardian(&app, &import.processed_ids)
                .await
                .map(|()| import.processed_ids.len()),
            Err(error) => Err(error),
        }
    };
    // Totals already in the journal flush even when this batch failed, so limits keep holding.
    let flushed = crate::distractions::usage_totals::flush(
        &pool,
        &active_vault_id,
        &device_id,
        force,
    )
    .await;
    let imported = imported?;
    flushed?;
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
    fn package_validation_requires_bounded_java_segments() {
        assert!(is_valid_package_name("com.example.video"));
        assert!(is_valid_package_name("app_1.social.feed2"));
        assert!(!is_valid_package_name("android"));
        assert!(!is_valid_package_name("1com.example"));
        assert!(!is_valid_package_name("com.example-app"));
    }

    #[tokio::test]
    async fn pending_guardian_usage_counts_once_beside_the_imported_journal() {
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
        let excluded = canonical_pending_ids(&mut tx, &capture).await.unwrap();
        assert_eq!(excluded, std::collections::HashSet::from(["local".into()]));
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
        assert_eq!(sources[0].elapsed_seconds, 32);
        tx.commit().await.unwrap();
        sqlx::query("DROP TABLE distractions_usage_samples")
            .execute(&pool)
            .await
            .unwrap();
        let mut connection = pool.acquire().await.unwrap();
        assert!(
            canonical_pending_ids(&mut connection, &capture)
                .await
                .is_err()
        );
        drop(connection);
        pool.close().await;
    }
}

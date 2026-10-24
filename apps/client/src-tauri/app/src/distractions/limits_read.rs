//! Canonical budget reads and native publication for desktop and browser enforcement.

use super::{limits::*, *};
pub(crate) use crate::distractions::limits::store::read_source_days;
use ganbaru_sync::manifest::vault::distractions::DOMAIN_NAME as DISTRACTIONS_DOMAIN;
use serde::Serialize;
use std::sync::LazyLock;

static READ_GATE: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageProjection {
    vault_id: String,
    local_date: String,
    week_start_local_date: String,
    updated_at: String,
    pub(super) totals: Vec<BudgetTotal>,
    foreground_status: DistractionsForegroundDesktopAppStatus,
}

/// Read the owner's last accepted projection without scheduling observation or enforcement work.
#[tauri::command]
pub fn distractions_load_usage_projection<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<UsageProjection, String> {
    runtime::usage_projection(&app)
}

/// Derive exhaustion from the vault's rules and usage, never from a frontend total.
pub(super) async fn derive_usage_projection<R: Runtime>(
    app: tauri::AppHandle<R>,
) -> Result<UsageProjection, String> {
    let _guard = READ_GATE
        .try_lock()
        .map_err(|_| "native usage read is already running")?;
    let publication_token = runtime::publication_token(&app)?;
    let vault_id = vault::active_vault_id(&app)?;
    let database_path = vault::active_database_path(&app)?;
    let rules = rule_store::snapshot(&app).await?;
    if rules.vault_id != vault_id {
        return Err("native usage vault changed before its rules were read".into());
    }
    let rules_revision = rules.revision;
    let (config_root, config, local_date, checked_at) =
        tauri::async_runtime::spawn_blocking(move || {
            let root = rules.root;
            let config = parse_config(&root)?;
            let zone = ganbaru_civil_time::system_zone()?;
            let checked_at = now_utc();
            let local_date =
                ganbaru_civil_time::instant_to_local(checked_at.timestamp_millis(), &zone)?
                    .date()
                    .format("%Y-%m-%d")
                    .to_string();
            Ok::<_, String>((root, config, local_date, checked_at))
        })
        .await
        .map_err(|error| format!("native usage configuration worker: {error}"))??;
    let week = week_start(&local_date)?;
    let status = app
        .state::<vault::ownership::VaultOwnershipManager>()
        .status(&vault_id)?;
    // Without budgets there is nothing to total. Spooled samples stay bounded and drain on the
    // first read after a budget exists, on exit, or before a vault snapshot.
    let sources = if config.items.is_empty() {
        Vec::new()
    } else {
        let writable = status.can_write_domain(DISTRACTIONS_DOMAIN);
        let _write_permit = if writable {
            Some(
                app.state::<vault::ownership::VaultOwnershipManager>()
                    .acquire_replicated_write(&vault_id, DISTRACTIONS_DOMAIN)?,
            )
        } else {
            None
        };
        if vault::active_vault_id(&app)? != vault_id
            || vault::active_database_path(&app)? != database_path
        {
            return Err("native usage vault changed before its read".into());
        }
        let pool = crate::db::connect_vault(&app).await?;
        if writable {
            crate::distractions::usage_totals::drain_and_flush(
                &app,
                &pool,
                &vault_id,
                &status.device_id,
                false,
            )
            .await?;
        }
        let mut tx = pool
            .begin()
            .await
            .map_err(|error| format!("begin native usage snapshot: {error}"))?;
        let mut sources = read_source_days(&mut tx, &status.device_id, &week, &local_date).await?;
        tx.commit()
            .await
            .map_err(|error| format!("finish native usage snapshot: {error}"))?;
        if !writable {
            // Usage this device may not journal yet still counts against its own budgets.
            sources.extend(
                crate::distractions::spool::pending_source_days(
                    &app,
                    &vault_id,
                    &status.device_id,
                    &week,
                    &local_date,
                )
                .await?,
            );
        }
        sources
    };
    let publication_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let totals = totals(&config, &sources, &local_date)?;
        if vault::active_vault_id(&publication_app)? != vault_id
            || vault::active_database_path(&publication_app)? != database_path
            || publication_app
                .state::<vault::ownership::VaultOwnershipManager>()
                .status(&vault_id)?
                != status
            || !rule_store::is_current(rules_revision)
        {
            return Err("native usage context changed during the read".into());
        }
        let current_date = ganbaru_civil_time::instant_to_local(
            now_epoch_ms(),
            &ganbaru_civil_time::system_zone()?,
        )?
        .date()
        .format("%Y-%m-%d")
        .to_string();
        if current_date != local_date {
            return Err("native usage date changed during the read".into());
        }
        let updated_at = checked_at.to_rfc3339_opts(SecondsFormat::Millis, true);
        let state = DistractionsLimitState {
            local_date: local_date.clone(),
            week_start_local_date: week.clone(),
            updated_at: updated_at.clone(),
            database_path: Some(
                database_path
                    .to_str()
                    .ok_or("usage database path is not UTF-8")?
                    .to_owned(),
            ),
            configuration_digest: Some(configuration_digest(&config_root)?),
            // Disabled rules remain visible in settings, but never enter the enforcement file.
            limits: totals
                .iter()
                .filter(|total| {
                    config.enabled
                        && config
                            .items
                            .iter()
                            .any(|limit| limit.id == total.limit_id && limit.enabled)
                })
                .map(|total| DistractionsLimitStateItem {
                    id: total.limit_id.clone(),
                    period: total.period.to_owned(),
                    window_start_local_date: total.window_start_local_date.clone(),
                    window_end_local_date: total.window_end_local_date.clone(),
                    used_seconds: total.used_seconds,
                    limit_seconds: total.limit_seconds,
                    remaining_seconds: total.remaining_seconds,
                    exhausted: total.exhausted,
                })
                .collect(),
        };
        state_files::validate_limit_state(&state)?;
        let json = serde_json::to_string(&state).map_err(|error| error.to_string())?;
        let foreground_status = runtime::foreground_projection(&publication_app);
        runtime::publish(&publication_app, publication_token, || {
            state_files::write_text_file_atomically(&limit_state_path(&publication_app)?, &json)?;
            Ok(UsageProjection {
                vault_id,
                local_date,
                week_start_local_date: week,
                updated_at,
                totals,
                foreground_status,
            })
        })
    })
    .await
    .map_err(|error| format!("native usage publication worker: {error}"))?
}

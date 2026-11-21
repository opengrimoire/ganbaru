//! Publishes what this desktop can run for its linked devices.
//!
//! The loop writes the shared status of each provider family configured here to the replicated
//! provider status table every [`PROVIDER_STATUS_PUBLISH_SECONDS`], and shortly after the Chat
//! device settings or device state change, so a phone that leaves agent work to this desktop
//! shows its availability, models, and plan limits. A publish that changes nothing writes
//! nothing, so the loop adds no sync traffic while the provider stays the same.

use super::command_support::now_timestamp;
use super::organization::DeviceProviders;

/// How often the loop republishes without a wake, to catch a vault switch or a provider that
/// stopped being ready without a settings change.
const PROVIDER_STATUS_PUBLISH_SECONDS: u64 = 300;

/// How long the loop waits after a wake before it publishes, so a burst of settings writes
/// becomes one publish.
const PROVIDER_STATUS_SETTLE_MILLIS: u64 = 2_000;

/// Wakes the publish loop after a change to the Chat device settings or device state.
#[derive(Default)]
pub(crate) struct ProviderStatusWake(tokio::sync::Notify);

/// Publishes this device's provider statuses to the active vault, logging failures without
/// their details.
async fn publish_active_vault(app: &tauri::AppHandle) {
    if crate::vault::active_vault_path(app).is_err() {
        return;
    }
    let device_id = match crate::vault::ensure_device_id(app) {
        Ok(device_id) => device_id,
        Err(error) => {
            eprintln!("Chat provider status could not read the device id: {error}");
            return;
        }
    };
    let statuses = match DeviceProviders::read_shared_statuses(app) {
        Ok(statuses) => statuses,
        Err(error) => {
            eprintln!(
                "Chat provider status could not read the providers: {:?}",
                error.code
            );
            return;
        }
    };
    let db_url = format!("sqlite:{}", crate::vault::APP_SQLITE_FILE);
    let pool = match crate::db::connect_sqlite(app.clone(), db_url).await {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("Chat provider status could not open the database: {error}");
            return;
        }
    };
    let published = match now_timestamp() {
        Ok(now) => {
            ganbaru_chat::provider_status::publish_provider_statuses(
                &pool, &device_id, &statuses, &now,
            )
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = published {
        eprintln!(
            "Chat provider status publish failed with code {:?}",
            error.code
        );
    }
}

/// Starts the publish loop. The first publish runs at startup.
pub(crate) fn start(app: &tauri::AppHandle) {
    use tauri::Manager;
    app.manage(ProviderStatusWake::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            PROVIDER_STATUS_PUBLISH_SECONDS,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let wake = app.state::<ProviderStatusWake>();
        loop {
            tokio::select! {
                _ = interval.tick() => {}
                () = wake.0.notified() => {
                    tokio::time::sleep(std::time::Duration::from_millis(
                        PROVIDER_STATUS_SETTLE_MILLIS,
                    ))
                    .await;
                }
            }
            publish_active_vault(&app).await;
        }
    });
}

/// Wakes the publish loop when it runs; does nothing on a device without it.
pub(crate) fn wake<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    if let Some(wake) = app.try_state::<ProviderStatusWake>() {
        wake.0.notify_one();
    }
}

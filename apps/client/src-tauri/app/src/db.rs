use crate::vault;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use tauri::{AppHandle, Manager, Runtime};

pub use ganbaru_db::DatabasePoolRegistry as DatabaseState;

#[cfg(desktop)]
pub const BENCHMARK_SQLITE_URL: &str = "sqlite:benchmark.sqlite";

const ALLOWED_SQLITE_FILES: &[&str] = &["ganbaru-ai.sqlite", "benchmark.sqlite"];
static VAULT_CONNECTION_GATE: LazyLock<Arc<tokio::sync::RwLock<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::RwLock::new(())));

pub(crate) type VaultExclusiveGuard = tokio::sync::OwnedRwLockWriteGuard<()>;

/// Prevent new SQLite connections while an active vault is being replaced.
pub(crate) async fn begin_vault_exclusive() -> VaultExclusiveGuard {
    VAULT_CONNECTION_GATE.clone().write_owned().await
}

fn resolve_sqlite_path<R: Runtime>(app: &AppHandle<R>, db_url: &str) -> Result<PathBuf, String> {
    let file_name = db_url
        .strip_prefix("sqlite:")
        .ok_or_else(|| format!("invalid db url '{db_url}', expected 'sqlite:<file>'"))?;

    if !ALLOWED_SQLITE_FILES.contains(&file_name) {
        return Err(format!("unsupported sqlite file '{file_name}'"));
    }

    let mut path = if file_name == vault::APP_SQLITE_FILE {
        vault::active_vault_path(app)?
    } else {
        let path = app
            .path()
            .app_config_dir()
            .map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("create app config dir: {error}"))?;
        path
    };
    path.push(file_name);
    Ok(path)
}

pub async fn connect_sqlite<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<sqlx::SqlitePool, String> {
    let _connection_guard = VAULT_CONNECTION_GATE.read().await;
    let is_vault_database = db_url == format!("sqlite:{}", vault::APP_SQLITE_FILE);
    let path = resolve_sqlite_path(&app, &db_url)?;
    let registry = app.state::<DatabaseState>().inner().clone();
    let access = if is_vault_database {
        let vault_id = vault::active_vault_id(&app)?;
        app.state::<vault::ownership::VaultOwnershipManager>()
            .database_access(&vault_id)?
    } else {
        vault::ownership::VaultDatabaseAccess::ReadWrite
    };
    drop(app);
    drop(db_url);
    match access {
        vault::ownership::VaultDatabaseAccess::ReadOnly => {
            registry.connect_path_read_only(path).await
        }
        vault::ownership::VaultDatabaseAccess::ReadWrite => registry.connect_path(path).await,
        // A pool opened read-only before the replica was linked stays read-only until the next
        // vault replacement or restart closes it, so readers are never cut off mid-session.
        vault::ownership::VaultDatabaseAccess::Guarded => {
            if registry.access(&path).await == Some(ganbaru_db::DatabaseAccessMode::ReadOnly) {
                registry.connect_path_read_only(path).await
            } else {
                registry
                    .connect_path_guarded(path)
                    .await
                    .map(|(pool, _)| pool)
            }
        }
    }
}

/// Opens the active vault for sync work and returns the pool only when this device may write
/// replicated rows: an owner opens it read-write, a linked replica opens it guarded. A replica
/// whose schema differs from the embedded migrations gets a read-only pool and no sync access.
pub(crate) async fn connect_active_vault_for_sync<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<sqlx::SqlitePool>, String> {
    let pool = connect_sqlite(app.clone(), format!("sqlite:{}", vault::APP_SQLITE_FILE)).await?;
    let path = resolve_sqlite_path(app, &format!("sqlite:{}", vault::APP_SQLITE_FILE))?;
    let writable = matches!(
        app.state::<DatabaseState>().access(path).await,
        Some(ganbaru_db::DatabaseAccessMode::ReadWrite | ganbaru_db::DatabaseAccessMode::Guarded)
    );
    Ok(writable.then_some(pool))
}

/// Open the authorized active vault without creation, migrations, or write access.
/// Callers that retain the pool across a replacement boundary must also reserve
/// the vault transition until their read and identity checks finish.
/// The supplied admission permit stays with path IO after an awaiting caller times out.
pub(crate) async fn connect_active_vault_read_only<R: Runtime>(
    app: &AppHandle<R>,
    work_permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<sqlx::SqlitePool, String> {
    let _connection_guard = VAULT_CONNECTION_GATE.read().await;
    let path_app = app.clone();
    let path = tauri::async_runtime::spawn_blocking(move || {
        let _work_permit = work_permit;
        resolve_sqlite_path(&path_app, &format!("sqlite:{}", vault::APP_SQLITE_FILE))
    })
    .await
    .map_err(|error| format!("Read-only vault database path worker: {error}"))??;
    app.state::<DatabaseState>()
        .inner()
        .clone()
        .connect_path_read_only(path)
        .await
}

pub(crate) async fn close_all_sqlite_pools_for_restore<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<(), String> {
    app.state::<DatabaseState>().close_all().await
}

#[cfg(desktop)]
pub async fn close_sqlite_pool<R: Runtime>(app: &AppHandle<R>, db_url: &str) -> Result<(), String> {
    let path = resolve_sqlite_path(app, db_url)?;
    app.state::<DatabaseState>().close_path(path).await
}

#[cfg(desktop)]
pub async fn close_all_sqlite_pools<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    app.state::<DatabaseState>().close_all().await
}

#[cfg(test)]
mod tests {
    use super::ALLOWED_SQLITE_FILES;

    #[test]
    fn allowed_sqlite_files_are_plain_file_names() {
        for file_name in ALLOWED_SQLITE_FILES {
            assert!(!file_name.contains('/'));
            assert!(!file_name.contains('\\'));
            assert!(!file_name.contains(".."));
        }
    }
}

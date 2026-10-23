use crate::vault;
use crate::vault::ownership::{VaultDatabaseAccess, VaultOwnershipManager};
use ganbaru_db::DatabaseAccessMode;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, OnceLock};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::Notify;

pub use ganbaru_db::DatabasePoolRegistry as DatabaseState;

/// File name of the isolated benchmark database in the app config directory.
pub(crate) const BENCHMARK_SQLITE_FILE: &str = "benchmark.sqlite";

const ALLOWED_SQLITE_FILES: &[&str] = &[vault::APP_SQLITE_FILE, BENCHMARK_SQLITE_FILE];
static VAULT_CONNECTION_GATE: LazyLock<Arc<tokio::sync::RwLock<()>>> =
    LazyLock::new(|| Arc::new(tokio::sync::RwLock::new(())));

/// Signaled after every commit on a writable vault connection of the process registry.
static VAULT_COMMITS: LazyLock<Arc<Notify>> = LazyLock::new(Arc::default);

/// The pool registry of this process, which the Tauri app manages and a headless sync pass
/// shares, so the process never holds two pools on one vault database.
static VAULT_POOLS: LazyLock<DatabaseState> = LazyLock::new(|| {
    let registry = DatabaseState::default();
    registry.set_connection_hook(Some(ganbaru_sync_replica::access::vault_connection_hook(
        vault_commits().clone(),
    )));
    registry
});

/// The process pool registry. Its connection hook, which installs replica guards and signals
/// commits, is in place before the first pool opens.
pub(crate) fn vault_pools() -> &'static DatabaseState {
    &VAULT_POOLS
}

/// Signaled after every commit on a writable connection of [`vault_pools`].
pub(crate) fn vault_commits() -> &'static Arc<Notify> {
    &VAULT_COMMITS
}

pub(crate) type VaultExclusiveGuard = tokio::sync::OwnedRwLockWriteGuard<()>;

/// Prevent new SQLite connections while an active vault is being replaced.
pub(crate) async fn begin_vault_exclusive() -> VaultExclusiveGuard {
    VAULT_CONNECTION_GATE.clone().write_owned().await
}

/// Holds the process vault lock of the vault whose database is `database_path` before a pool
/// opens on it, so another Ganbaru process never shares the vault. Closing pools keeps the lock,
/// which only a vault switch, a replacement, or app exit releases.
fn hold_vault_lock(database_path: &Path) -> Result<(), String> {
    let root = database_path
        .parent()
        .ok_or_else(|| "Ganbaru database has no vault folder".to_string())?;
    vault::vault_lock::process_vault_lock().acquire(root)?;
    Ok(())
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

/// The database the app's webviews read and write in this process.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DatabaseTarget {
    /// The active vault's `ganbaru.sqlite`.
    Vault,
    /// The isolated benchmark database, used only by a benchmark run's relaunch.
    Benchmark,
}

/// Fixed by the first selection in this process and never changed afterwards, so a webview
/// cannot point commands at a different database than the one the process booted with.
static SELECTED_DATABASE: OnceLock<DatabaseTarget> = OnceLock::new();

impl DatabaseTarget {
    fn from_url(db_url: &str) -> Result<Self, String> {
        match db_url.strip_prefix("sqlite:") {
            Some(vault::APP_SQLITE_FILE) => Ok(Self::Vault),
            Some(BENCHMARK_SQLITE_FILE) => Ok(Self::Benchmark),
            Some(file_name) => Err(format!("unsupported sqlite file '{file_name}'")),
            None => Err(format!(
                "invalid db url '{db_url}', expected 'sqlite:<file>'"
            )),
        }
    }

    /// The connection string the webviews pass for this target.
    pub(crate) fn url(self) -> String {
        match self {
            Self::Vault => format!("sqlite:{}", vault::APP_SQLITE_FILE),
            Self::Benchmark => format!("sqlite:{BENCHMARK_SQLITE_FILE}"),
        }
    }
}

/// Checks a database a command names against the process selection. The vault stays usable
/// until a benchmark is selected; the benchmark database is usable only once it is.
fn admit_database_target(
    selected: Option<DatabaseTarget>,
    requested: DatabaseTarget,
) -> Result<(), String> {
    match (selected, requested) {
        (Some(DatabaseTarget::Benchmark), DatabaseTarget::Benchmark)
        | (None | Some(DatabaseTarget::Vault), DatabaseTarget::Vault) => Ok(()),
        (Some(DatabaseTarget::Benchmark), DatabaseTarget::Vault) => {
            Err("The vault database is unavailable while a benchmark run is active".to_string())
        }
        (None | Some(DatabaseTarget::Vault), DatabaseTarget::Benchmark) => {
            Err("The benchmark database is available only during a benchmark run".to_string())
        }
    }
}

/// Selects this process's database once. A benchmark is selected only when it is requested
/// and `benchmark_pending` confirms a benchmark relaunch is waiting; a later call returns the
/// first selection unchanged.
#[cfg(desktop)]
pub(crate) fn select_database_target(
    benchmark_requested: bool,
    benchmark_pending: impl FnOnce() -> bool,
) -> DatabaseTarget {
    select_database_target_in(&SELECTED_DATABASE, benchmark_requested, benchmark_pending)
}

#[cfg(any(desktop, test))]
fn select_database_target_in(
    selected: &OnceLock<DatabaseTarget>,
    benchmark_requested: bool,
    benchmark_pending: impl FnOnce() -> bool,
) -> DatabaseTarget {
    *selected.get_or_init(|| {
        if benchmark_requested && benchmark_pending() {
            DatabaseTarget::Benchmark
        } else {
            DatabaseTarget::Vault
        }
    })
}

/// The database selected for this process, if one has been selected.
pub(crate) fn selected_database_target() -> Option<DatabaseTarget> {
    SELECTED_DATABASE.get().copied()
}

/// Opens the database a command names, after checking it is the one this process selected.
/// The URL is an assertion from the webview, never a choice: a mismatch fails instead of
/// reaching another database.
pub async fn connect_sqlite<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<sqlx::SqlitePool, String> {
    admit_database_target(
        selected_database_target(),
        DatabaseTarget::from_url(&db_url)?,
    )?;
    connect_database(app, db_url).await
}

/// Opens the benchmark database for harness-only writes. It fails unless this process selected
/// the benchmark database, so benchmark fixtures never reach the vault.
#[cfg(desktop)]
pub(crate) async fn connect_benchmark_database<R: Runtime>(
    app: AppHandle<R>,
) -> Result<sqlx::SqlitePool, String> {
    connect_sqlite(app, DatabaseTarget::Benchmark.url()).await
}

/// Opens the active vault for Rust-owned work, whichever database the webviews use.
pub(crate) async fn connect_vault<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<sqlx::SqlitePool, String> {
    connect_database(app.clone(), DatabaseTarget::Vault.url()).await
}

async fn connect_database<R: Runtime>(
    app: AppHandle<R>,
    db_url: String,
) -> Result<sqlx::SqlitePool, String> {
    let _connection_guard = VAULT_CONNECTION_GATE.read().await;
    let is_vault_database = db_url == format!("sqlite:{}", vault::APP_SQLITE_FILE);
    let path = resolve_sqlite_path(&app, &db_url)?;
    let registry = app.state::<DatabaseState>().inner().clone();
    let access = if is_vault_database {
        hold_vault_lock(&path)?;
        let vault_id = vault::active_vault_id(&app)?;
        app.state::<VaultOwnershipManager>()
            .database_access(&vault_id)?
    } else {
        VaultDatabaseAccess::ReadWrite
    };
    drop(app);
    drop(db_url);
    connect_with_access(&registry, path, access).await
}

/// Opens `path` in `registry` with the access ownership grants.
async fn connect_with_access(
    registry: &DatabaseState,
    path: PathBuf,
    access: VaultDatabaseAccess,
) -> Result<sqlx::SqlitePool, String> {
    match access {
        VaultDatabaseAccess::ReadOnly => registry.connect_path_read_only(path).await,
        VaultDatabaseAccess::ReadWrite => {
            let opening = registry.access(&path).await.is_none();
            let pool = registry.connect_path(&path).await?;
            if opening {
                convert_projects(&pool, &path).await?;
            }
            Ok(pool)
        }
        // A pool opened read-only before the replica was linked stays read-only until the next
        // vault replacement or restart closes it, so readers are never cut off mid-session.
        VaultDatabaseAccess::Guarded => {
            if registry.access(&path).await == Some(DatabaseAccessMode::ReadOnly) {
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

/// Runs the Projects conversion pass on a writable database the registry just opened, so placeholder keys left by
/// migrations are rewritten before any command or sync capture reads them. The pass is idempotent, so a concurrent
/// first open that runs it twice changes nothing the second time.
async fn convert_projects(pool: &sqlx::SqlitePool, database_path: &Path) -> Result<(), String> {
    let vault_root = database_path
        .parent()
        .ok_or_else(|| format!("database {} has no folder", database_path.display()))?;
    ganbaru_projects::conversion::convert(pool, vault_root).await?;
    ganbaru_projects::conversion::assert_converted(pool).await
}

/// Opens the active vault for sync work and returns the pool only when this device may write
/// replicated rows: an owner opens it read-write, a linked replica opens it guarded. A replica
/// whose schema differs from the embedded migrations gets a read-only pool and no sync access.
pub(crate) async fn connect_active_vault_for_sync<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<Option<sqlx::SqlitePool>, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("find app config directory: {error}"))?;
    connect_vault_for_sync_in(
        app.state::<DatabaseState>().inner(),
        app.state::<VaultOwnershipManager>().inner(),
        &config_dir,
    )
    .await
}

/// Opens the active vault of the app config directory `config_dir` for sync work through
/// `registry`, with the access `ownership` grants, as [`connect_active_vault_for_sync`] does
/// for the app.
pub(crate) async fn connect_vault_for_sync_in(
    registry: &DatabaseState,
    ownership: &VaultOwnershipManager,
    config_dir: &Path,
) -> Result<Option<sqlx::SqlitePool>, String> {
    let _connection_guard = VAULT_CONNECTION_GATE.read().await;
    let path = vault::active_database_path_in(config_dir)?;
    hold_vault_lock(&path)?;
    let access = ownership.database_access(&vault::active_vault_id_in(config_dir)?)?;
    let pool = connect_with_access(registry, path.clone(), access).await?;
    let writable = matches!(
        registry.access(path).await,
        Some(DatabaseAccessMode::ReadWrite | DatabaseAccessMode::Guarded)
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
        let path = resolve_sqlite_path(&path_app, &format!("sqlite:{}", vault::APP_SQLITE_FILE))?;
        hold_vault_lock(&path)?;
        Ok::<_, String>(path)
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
    use super::{
        ALLOWED_SQLITE_FILES, DatabaseTarget, admit_database_target, select_database_target_in,
    };
    use std::sync::OnceLock;

    #[test]
    fn commands_reach_only_the_selected_database() {
        use DatabaseTarget::{Benchmark, Vault};
        assert!(admit_database_target(None, Vault).is_ok());
        assert!(admit_database_target(Some(Vault), Vault).is_ok());
        assert!(admit_database_target(Some(Benchmark), Benchmark).is_ok());
        assert!(admit_database_target(None, Benchmark).is_err());
        assert!(admit_database_target(Some(Vault), Benchmark).is_err());
        assert!(admit_database_target(Some(Benchmark), Vault).is_err());
    }

    #[test]
    fn database_urls_name_only_known_files() {
        assert_eq!(
            DatabaseTarget::from_url("sqlite:ganbaru.sqlite"),
            Ok(DatabaseTarget::Vault)
        );
        assert_eq!(
            DatabaseTarget::from_url("sqlite:benchmark.sqlite"),
            Ok(DatabaseTarget::Benchmark)
        );
        for url in [
            "sqlite:../ganbaru.sqlite",
            "sqlite:other.sqlite",
            "ganbaru.sqlite",
            "sqlite:",
        ] {
            assert!(DatabaseTarget::from_url(url).is_err(), "{url}");
        }
        for target in [DatabaseTarget::Vault, DatabaseTarget::Benchmark] {
            assert_eq!(DatabaseTarget::from_url(&target.url()), Ok(target));
        }
    }

    #[test]
    fn benchmark_is_selected_only_for_a_pending_relaunch() {
        let selected = OnceLock::new();
        assert_eq!(
            select_database_target_in(&selected, true, || false),
            DatabaseTarget::Vault
        );
        let selected = OnceLock::new();
        assert_eq!(
            select_database_target_in(&selected, false, || true),
            DatabaseTarget::Vault
        );
        let selected = OnceLock::new();
        assert_eq!(
            select_database_target_in(&selected, true, || true),
            DatabaseTarget::Benchmark
        );
    }

    #[test]
    fn the_first_database_selection_is_final() {
        let selected = OnceLock::new();
        assert_eq!(
            select_database_target_in(&selected, false, || true),
            DatabaseTarget::Vault
        );
        assert_eq!(
            select_database_target_in(&selected, true, || true),
            DatabaseTarget::Vault
        );
        let selected = OnceLock::new();
        assert_eq!(
            select_database_target_in(&selected, true, || true),
            DatabaseTarget::Benchmark
        );
        assert_eq!(
            select_database_target_in(&selected, false, || false),
            DatabaseTarget::Benchmark
        );
    }

    #[test]
    fn allowed_sqlite_files_are_plain_file_names() {
        for file_name in ALLOWED_SQLITE_FILES {
            assert!(!file_name.contains('/'));
            assert!(!file_name.contains('\\'));
            assert!(!file_name.contains(".."));
        }
    }
}

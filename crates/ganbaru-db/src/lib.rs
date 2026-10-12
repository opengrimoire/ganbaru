use sqlx::{
    Row, SqliteConnection, SqlitePool,
    pool::PoolConnectionMetadata,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
const WRITE_CONTENTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Native access mode applied when opening a vault database pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DatabaseAccessMode {
    /// Reads only, through a read-only connection.
    ReadOnly,
    /// Full write access with migrations applied at open.
    ReadWrite,
    /// A writable connection without migrations whose connection hook restricts writes, used by
    /// replicas that write replicated tables while the rest of the vault stays read-only.
    Guarded,
}

/// Future returned by a [`ConnectionHook`].
pub type ConnectionHookFuture<'c> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'c>>;

/// Setup that runs on every new writable connection, with the access mode of its pool.
///
/// Pools may recycle their connection, so per-connection state such as TEMP triggers and
/// commit hooks belongs here instead of in a one-time initialization.
pub type ConnectionHook = Arc<
    dyn for<'c> Fn(&'c mut SqliteConnection, DatabaseAccessMode) -> ConnectionHookFuture<'c>
        + Send
        + Sync,
>;

/// Wraps a hook function, fixing the higher-ranked signature that closures cannot infer.
pub fn connection_hook<F>(hook: F) -> ConnectionHook
where
    F: for<'c> Fn(&'c mut SqliteConnection, DatabaseAccessMode) -> ConnectionHookFuture<'c>
        + Send
        + Sync
        + 'static,
{
    Arc::new(hook)
}

struct RegisteredPool {
    access: DatabaseAccessMode,
    /// Whether a guarded request opened this pool read-only because the schema differs.
    guarded_fallback: bool,
    pool: SqlitePool,
}

/// Shared registry for SQLite pools keyed by their authorized filesystem path.
///
/// Opening, initialization, and closing are serialized so concurrent first reads
/// cannot create competing pools or run the migration chain more than once.
#[derive(Clone, Default)]
pub struct DatabasePoolRegistry {
    pools: Arc<Mutex<HashMap<PathBuf, RegisteredPool>>>,
    hook: Arc<std::sync::RwLock<Option<ConnectionHook>>>,
}

impl DatabasePoolRegistry {
    /// Sets the hook that later writable connections run. Open pools keep their connections
    /// until they are closed.
    pub fn set_connection_hook(&self, hook: Option<ConnectionHook>) {
        let mut slot = self
            .hook
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = hook;
    }

    fn connection_hook(&self) -> Option<ConnectionHook> {
        self.hook
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Connects to an authorized SQLite path or returns its existing pool.
    pub async fn connect_path(&self, path: impl AsRef<Path>) -> Result<SqlitePool, String> {
        self.connect_path_with_access(path, DatabaseAccessMode::ReadWrite)
            .await
            .map(|(pool, _)| pool)
    }

    /// Connects to an existing authorized SQLite path without write access. An open guarded
    /// pool for the path is returned instead, because a path holds one pool at a time.
    pub async fn connect_path_read_only(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<SqlitePool, String> {
        self.connect_path_with_access(path, DatabaseAccessMode::ReadOnly)
            .await
            .map(|(pool, _)| pool)
    }

    /// Connects to an existing authorized SQLite path in guarded mode, without migrations.
    ///
    /// When the database schema differs from the embedded migration set, the pool reopens
    /// read-only instead, and the returned mode says so.
    pub async fn connect_path_guarded(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<(SqlitePool, DatabaseAccessMode), String> {
        self.connect_path_with_access(path, DatabaseAccessMode::Guarded)
            .await
    }

    /// Access mode of the open pool for a path.
    pub async fn access(&self, path: impl AsRef<Path>) -> Option<DatabaseAccessMode> {
        self.pools
            .lock()
            .await
            .get(path.as_ref())
            .map(|registered| registered.access)
    }

    async fn connect_path_with_access(
        &self,
        path: impl AsRef<Path>,
        access: DatabaseAccessMode,
    ) -> Result<(SqlitePool, DatabaseAccessMode), String> {
        let path = path.as_ref().to_path_buf();
        let mut pools = self.pools.lock().await;
        if let Some(registered) = pools.get(&path) {
            let reusable = registered.access == access
                || (access == DatabaseAccessMode::ReadOnly
                    && registered.access == DatabaseAccessMode::Guarded)
                || (access == DatabaseAccessMode::Guarded && registered.guarded_fallback);
            if !reusable {
                return Err("database access changed; close the existing pool first".to_string());
            }
            return Ok((registered.pool.clone(), registered.access));
        }

        let mut effective = access;
        let mut pool = self.open(&path, access).await?;
        if access == DatabaseAccessMode::Guarded && validate_current_schema(&pool).await.is_err() {
            close_pool(&pool).await?;
            effective = DatabaseAccessMode::ReadOnly;
            pool = self.open(&path, effective).await?;
        }
        pools.insert(
            path,
            RegisteredPool {
                access: effective,
                guarded_fallback: access != effective,
                pool: pool.clone(),
            },
        );
        Ok((pool, effective))
    }

    async fn open(&self, path: &Path, access: DatabaseAccessMode) -> Result<SqlitePool, String> {
        let mut options = SqliteConnectOptions::new()
            .filename(path)
            .foreign_keys(true)
            .busy_timeout(WRITE_CONTENTION_TIMEOUT);
        options = match access {
            DatabaseAccessMode::ReadOnly => options.read_only(true),
            DatabaseAccessMode::ReadWrite => options
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full),
            DatabaseAccessMode::Guarded => options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full),
        };
        let mut pool_options = SqlitePoolOptions::new().max_connections(1);
        if access != DatabaseAccessMode::ReadOnly
            && let Some(hook) = self.connection_hook()
        {
            pool_options = pool_options.after_connect(
                move |conn: &mut SqliteConnection, _: PoolConnectionMetadata| {
                    let hook = hook.clone();
                    Box::pin(async move {
                        hook(conn, access)
                            .await
                            .map_err(|error| sqlx::Error::Configuration(error.into()))
                    })
                },
            );
        }
        let pool = pool_options
            .connect_with(options)
            .await
            .map_err(|error| format!("connect: {error}"))?;
        let initialization = async {
            match access {
                DatabaseAccessMode::ReadWrite => {
                    run_migrations(&pool).await?;
                    sqlx::raw_sql("PRAGMA optimize")
                        .execute(&pool)
                        .await
                        .map_err(|error| format!("pragma optimize: {error}"))?;
                }
                DatabaseAccessMode::ReadOnly => {
                    sqlx::raw_sql("PRAGMA query_only = ON")
                        .execute(&pool)
                        .await
                        .map_err(|error| format!("enable query-only database access: {error}"))?;
                }
                DatabaseAccessMode::Guarded => {}
            }
            Ok::<(), String>(())
        }
        .await;
        if let Err(error) = initialization {
            return Err(match close_pool(&pool).await {
                Ok(()) => error,
                Err(close) => format!("{error}; {close}"),
            });
        }
        Ok(pool)
    }

    /// Closes and removes the pool registered for a filesystem path.
    pub async fn close_path(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let mut pools = self.pools.lock().await;
        let pool = pools.remove(path);
        if let Some(registered) = pool {
            close_pool(&registered.pool)
                .await
                .map_err(|error| format!("{error}: {}", path.display()))?;
        }
        Ok(())
    }

    /// Closes every pool currently held by the registry. Every pool is closed even when one
    /// fails, and the first failure is returned.
    pub async fn close_all(&self) -> Result<(), String> {
        let mut pools = self.pools.lock().await;
        let mut result = Ok(());
        for (path, registered) in std::mem::take(&mut *pools) {
            if let Err(error) = close_pool(&registered.pool).await
                && result.is_ok()
            {
                result = Err(format!("{error}: {}", path.display()));
            }
        }
        result
    }
}

/// Close passes allowed after the first one before a pool counts as still open.
const CLOSE_DRAIN_PASSES: usize = 2;

/// Closes a pool and returns once none of its connections holds the database open.
///
/// sqlx 0.8 `Pool::close` can return while a connection that began returning to the pool before
/// the close sits idle. That connection would close later on its worker thread and checkpoint
/// the WAL after callers had already copied, moved, or replaced the database file. After a
/// completed close no connection can be checked out, so another pass closes the idle ones.
pub async fn close_pool(pool: &SqlitePool) -> Result<(), String> {
    pool.close().await;
    let mut passes = 0;
    while pool.size() > 0 {
        if passes == CLOSE_DRAIN_PASSES {
            return Err(format!(
                "close database pool: {} connections remain open",
                pool.size()
            ));
        }
        pool.close().await;
        passes += 1;
    }
    Ok(())
}

/// Applies the embedded Ganbaru AI migration chain to a SQLite pool.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), String> {
    MIGRATOR
        .run(pool)
        .await
        .map_err(|error| format!("run database migrations: {error}"))
}

/// Returns deterministic bytes identifying the complete embedded up-migration set.
///
/// Callers can hash this bounded material when negotiating database compatibility
/// without exposing migration contents over an external protocol.
pub fn migration_set_identity_material() -> Vec<u8> {
    let mut identity = Vec::new();
    for migration in MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
    {
        identity.extend_from_slice(&migration.version.to_be_bytes());
        identity.extend_from_slice(&(migration.checksum.len() as u64).to_be_bytes());
        identity.extend_from_slice(migration.checksum.as_ref());
    }
    identity
}

/// Validates that a read-only database has the complete embedded migration history.
pub async fn validate_current_schema(pool: &SqlitePool) -> Result<(), String> {
    let rows =
        sqlx::query("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version ASC")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("read database migration history: {error}"))?;
    let expected = MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .collect::<Vec<_>>();
    if rows.len() != expected.len() {
        return Err("database schema does not match the current migration set".to_string());
    }
    for (row, migration) in rows.iter().zip(expected) {
        let version: i64 = row
            .try_get("version")
            .map_err(|error| format!("read migration version: {error}"))?;
        let checksum: Vec<u8> = row
            .try_get("checksum")
            .map_err(|error| format!("read migration checksum: {error}"))?;
        let success: bool = row
            .try_get("success")
            .map_err(|error| format!("read migration status: {error}"))?;
        if version != migration.version || checksum != migration.checksum.as_ref() || !success {
            return Err("database migration history is incompatible".to_string());
        }
    }
    Ok(())
}

#[doc(hidden)]
pub mod __private {
    pub use sqlx;
}

/// Implements `sqlx::FromRow` by reading fields with their Rust names.
#[macro_export]
macro_rules! impl_sqlite_from_row {
    ($type:ty { $($field:ident),+ $(,)? }) => {
        impl<'r> $crate::__private::sqlx::FromRow<'r, $crate::__private::sqlx::sqlite::SqliteRow>
            for $type
        {
            fn from_row(
                row: &'r $crate::__private::sqlx::sqlite::SqliteRow,
            ) -> Result<Self, $crate::__private::sqlx::Error> {
                use $crate::__private::sqlx::Row as _;
                Ok(Self {
                    $($field: row.try_get(stringify!($field))?,)+
                })
            }
        }
    };
}

#[cfg(test)]
mod tests;

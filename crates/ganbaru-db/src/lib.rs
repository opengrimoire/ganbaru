use sqlx::{
    Row, SqliteConnection, SqlitePool,
    pool::PoolConnectionMetadata,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{
    collections::{BTreeSet, HashMap},
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, LazyLock},
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
        if access != DatabaseAccessMode::ReadOnly {
            restrict_database_files(path, access == DatabaseAccessMode::ReadWrite);
        }
        let mut options = hardened_options(
            SqliteConnectOptions::new()
                .filename(path)
                .foreign_keys(true)
                .busy_timeout(WRITE_CONTENTION_TIMEOUT),
        );
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
                    // An opened or imported vault folder can come from another device, so its
                    // catalog is checked before migration writes can reach its triggers.
                    validate_schema_catalog(&pool).await?;
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
/// sqlx `Pool::close` can return while a connection that began returning to the pool before
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

/// Applies the embedded Ganbaru migration chain to a SQLite pool.
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

/// Validates that a database has the complete embedded migration history and exactly the schema
/// that history produces.
pub async fn validate_current_schema(pool: &SqlitePool) -> Result<(), String> {
    let applied = validate_schema_catalog(pool).await?;
    if applied != up_migrations().count() {
        return Err("database schema does not match the current migration set".to_string());
    }
    Ok(())
}

/// Tables SQLite creates for query planner statistics. They hold no application data and appear
/// after `PRAGMA optimize`, so their presence never makes a catalog differ.
const SQLITE_STATISTICS_TABLES: [&str; 2] = ["sqlite_stat1", "sqlite_stat4"];

/// One `sqlite_schema` row as (type, name, table name, SQL text).
type SchemaObject = (String, String, String, Option<String>);
type SchemaCatalog = BTreeSet<SchemaObject>;

/// Reference catalogs keyed by the version of the last applied migration.
static REFERENCE_CATALOGS: LazyLock<Mutex<HashMap<i64, Arc<SchemaCatalog>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn up_migrations() -> impl Iterator<Item = &'static sqlx::migrate::Migration> {
    MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
}

/// Validates an untrusted database catalog before any application query or integrity check.
///
/// The applied migration history must be a successful prefix of the embedded migration set,
/// `_sqlx_migrations` must be a table, and every table, index, trigger, and view must equal the
/// catalog that prefix produces on an empty database. Extra objects, such as a trigger that runs
/// attacker SQL on application writes or a tampered FTS5 shadow table, are rejected. An empty
/// database passes with no applied migrations.
///
/// Returns the number of applied migrations.
pub async fn validate_schema_catalog(pool: &SqlitePool) -> Result<usize, String> {
    let catalog = read_schema_catalog(pool).await?;
    if catalog.is_empty() {
        return Ok(0);
    }
    let history = catalog
        .iter()
        .filter(|(_, name, _, _)| name == MIGRATIONS_TABLE)
        .collect::<Vec<_>>();
    if history.len() != 1 || history[0].0 != "table" {
        return Err("database migration history is not a table".to_string());
    }
    let applied = validate_migration_prefix(pool).await?;
    let Some(last) = applied.last() else {
        return Err("database has a schema without migration history".to_string());
    };
    if catalog != *reference_catalog(*last).await? {
        return Err("database schema differs from its migration history".to_string());
    }
    Ok(applied.len())
}

const MIGRATIONS_TABLE: &str = "_sqlx_migrations";

async fn read_schema_catalog(pool: &SqlitePool) -> Result<SchemaCatalog, String> {
    let rows = sqlx::query("SELECT type, name, tbl_name, sql FROM main.sqlite_schema")
        .fetch_all(pool)
        .await
        .map_err(|error| format!("read database schema: {error}"))?;
    let mut catalog = SchemaCatalog::new();
    for row in rows {
        let object: SchemaObject = (
            row.try_get("type")
                .map_err(|error| format!("read schema object type: {error}"))?,
            row.try_get("name")
                .map_err(|error| format!("read schema object name: {error}"))?,
            row.try_get("tbl_name")
                .map_err(|error| format!("read schema object table: {error}"))?,
            row.try_get("sql")
                .map_err(|error| format!("read schema object SQL: {error}"))?,
        );
        if object.0 == "table" && SQLITE_STATISTICS_TABLES.contains(&object.1.as_str()) {
            continue;
        }
        catalog.insert(object);
    }
    Ok(catalog)
}

/// Returns the applied migration versions after checking that they are a successful prefix of
/// the embedded migration set with matching checksums.
async fn validate_migration_prefix(pool: &SqlitePool) -> Result<Vec<i64>, String> {
    let rows =
        sqlx::query("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version ASC")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("read database migration history: {error}"))?;
    let expected = up_migrations().collect::<Vec<_>>();
    if rows.len() > expected.len() {
        return Err("database schema does not match the current migration set".to_string());
    }
    let mut versions = Vec::with_capacity(rows.len());
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
        versions.push(version);
    }
    Ok(versions)
}

/// Builds, or returns the cached, catalog of an empty database migrated through `version`.
async fn reference_catalog(version: i64) -> Result<Arc<SchemaCatalog>, String> {
    let mut catalogs = REFERENCE_CATALOGS.lock().await;
    if let Some(catalog) = catalogs.get(&version) {
        return Ok(catalog.clone());
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(hardened_options(
            SqliteConnectOptions::new().in_memory(true),
        ))
        .await
        .map_err(|error| format!("open reference schema: {error}"))?;
    let built = async {
        MIGRATOR
            .run_to(version, &pool)
            .await
            .map_err(|error| format!("build reference schema: {error}"))?;
        read_schema_catalog(&pool).await
    }
    .await;
    pool.close().await;
    let catalog = Arc::new(built?);
    catalogs.insert(version, catalog.clone());
    Ok(catalog)
}

/// Suffixes of the files SQLite keeps beside a database, which it creates with the
/// database file's permissions.
const DATABASE_FILE_SUFFIXES: [&str; 4] = ["", "-wal", "-shm", "-journal"];

/// Keeps a writable vault database and its journal files private to the current user.
///
/// A missing database is created owner-only so its journal files inherit that mode, and an
/// existing one loses group and other access. A vault on a file system without Unix
/// permissions, such as a FAT or exFAT drive, refuses the change; the vault still opens there
/// because its location is the user's choice, and the refusal is reported on standard error.
fn restrict_database_files(path: &Path, create: bool) {
    if create {
        match ganbaru_atomic_files::create_private_file(path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            // The connection reports any other creation failure with its open error.
            Err(_) => return,
        }
    }
    for suffix in DATABASE_FILE_SUFFIXES {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        let file = PathBuf::from(file);
        match ganbaru_atomic_files::restrict_to_owner(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "could not make '{}' private to the current user: {error}",
                file.display()
            ),
        }
    }
}

/// Connection settings that limit what SQL stored in an untrusted schema can do: schema
/// functions and virtual tables must be innocuous, and page cells are bounds-checked as they are
/// read. Defensive mode is the bundled SQLite default through `SQLITE_DEFAULT_DEFENSIVE`.
fn hardened_options(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .pragma("trusted_schema", "OFF")
        .pragma("cell_size_check", "ON")
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

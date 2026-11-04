//! Test fixtures that copy a migrated template instead of applying the schema per test.
//!
//! Most tests across the workspace open their own database. Applying the schema costs a
//! noticeable fraction of a second in a debug build, while loading a copy of a serialized
//! database costs a small fraction of that, so each test process applies the schema once and
//! copies the result.

use crate::MIGRATOR;
use sqlx::sqlite::SqliteOwnedBuf;
use sqlx::{Connection, SqliteConnection, SqlitePool};
use std::path::Path;
use tokio::sync::OnceCell;

/// Serialized in-memory database with every embedded migration applied.
static MIGRATED_TEMPLATE: OnceCell<Vec<u8>> = OnceCell::const_new();

/// Returns the process-wide template, building it on first use.
///
/// The template is built on a standalone connection, which spawns no tasks on the caller's
/// runtime, so tests on separate per-test runtimes can share it. Building records the reference
/// catalog that schema validation compares against. Concurrent callers wait for one build.
async fn migrated_template() -> Result<&'static [u8], String> {
    MIGRATED_TEMPLATE
        .get_or_try_init(build_migrated_template)
        .await
        .map(Vec::as_slice)
}

async fn build_migrated_template() -> Result<Vec<u8>, String> {
    let mut conn = SqliteConnection::connect("sqlite::memory:")
        .await
        .map_err(|error| format!("connect template database: {error}"))?;
    MIGRATOR
        .run(&mut conn)
        .await
        .map_err(|error| format!("run template database migrations: {error}"))?;
    crate::remember_reference_catalog(&mut conn).await?;
    let bytes = conn
        .serialize(None)
        .await
        .map_err(|error| format!("serialize template database: {error}"))?
        .to_vec();
    conn.close()
        .await
        .map_err(|error| format!("close template database: {error}"))?;
    Ok(bytes)
}

/// Replaces the database of a single-connection in-memory pool with a copy of the migrated
/// template.
///
/// The result matches running [`crate::run_migrations`] on an empty database, including the
/// SQLx migration history. Connection settings such as `PRAGMA foreign_keys` are kept, so call
/// this after connecting and setting pragmas, in place of running the migrations.
///
/// # Errors
///
/// Returns an error when the pool may open more than one connection or holds a file-backed
/// database, since another connection would not see the copy and a file would be overwritten.
pub async fn apply_migrated_template(pool: &SqlitePool) -> Result<(), String> {
    if pool.options().get_max_connections() != 1 {
        return Err("migrated template requires a pool with one connection".to_string());
    }
    let template = migrated_template().await?;
    let mut conn = pool
        .acquire()
        .await
        .map_err(|error| format!("acquire template target: {error}"))?;
    let main_file: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
            .fetch_one(&mut *conn)
            .await
            .map_err(|error| format!("read template target location: {error}"))?;
    if !main_file.is_empty() {
        return Err("migrated template requires an in-memory database".to_string());
    }
    let buffer = SqliteOwnedBuf::try_from(template)
        .map_err(|error| format!("copy template database: {error}"))?;
    conn.deserialize(None, buffer, false)
        .await
        .map_err(|error| format!("load template database: {error}"))
}

/// Writes a copy of the migrated template to the database file at `path`, so opening it
/// through [`crate::DatabasePoolRegistry`] validates the schema and applies no migrations.
///
/// The file is created when missing. An existing empty file, such as the one a new vault
/// skeleton holds, is filled in place and keeps its permissions. The file uses the rollback
/// journal until a connection switches it to WAL, as a registry pool does on open.
///
/// # Errors
///
/// Returns an error when a non-empty file already exists at `path` or the file cannot be
/// written.
pub async fn write_migrated_database(path: impl AsRef<Path>) -> Result<(), String> {
    let path = path.as_ref();
    let template = migrated_template().await?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("open migrated database {}: {error}", path.display()))?;
    let length = file
        .metadata()
        .map_err(|error| format!("inspect migrated database {}: {error}", path.display()))?
        .len();
    if length != 0 {
        return Err(format!(
            "refusing to overwrite the non-empty database {}",
            path.display()
        ));
    }
    std::io::Write::write_all(&mut file, template)
        .map_err(|error| format!("write migrated database {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{apply_migrated_template, write_migrated_database};
    use crate::validate_current_schema;
    use sqlx::SqlitePool;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create template test runtime")
            .block_on(future)
    }

    async fn template_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        apply_migrated_template(&pool).await.unwrap();
        pool
    }

    async fn event_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM calendar_events")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[test]
    fn template_copy_has_the_complete_migration_history() {
        block_on(async {
            let pool = template_pool().await;
            validate_current_schema(&pool).await.unwrap();
        });
    }

    #[test]
    fn template_copy_enforces_foreign_keys() {
        block_on(async {
            let pool = template_pool().await;
            let enabled: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(enabled, 1);
            let orphan = sqlx::query(
                "INSERT INTO pomodoro_runs
                    (id, event_id, original_event_id, event_date, planned_start, planned_end,
                     started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat,
                     start_trigger)
                 VALUES ('run-1', 'missing-event', 'missing-event', '2026-05-23',
                         '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                         '2026-05-23T09:00:00Z', 'count', 'preset', 'adaptive',
                         '2026-05-23T09:00:00Z', 'manual')",
            )
            .execute(&pool)
            .await
            .unwrap_err();
            assert!(
                orphan.to_string().contains("FOREIGN KEY constraint failed"),
                "{orphan}"
            );
        });
    }

    #[test]
    fn template_copies_are_independent() {
        block_on(async {
            let first = template_pool().await;
            let second = template_pool().await;
            sqlx::query(
                "INSERT INTO calendar_events (id, title, start_time, end_time)
                 VALUES ('event-1', 'Focus block', '2026-05-23T09:00:00Z',
                         '2026-05-23T10:00:00Z')",
            )
            .execute(&first)
            .await
            .unwrap();
            assert_eq!(event_count(&first).await, 1);
            assert_eq!(event_count(&second).await, 0);
            assert_eq!(event_count(&template_pool().await).await, 0);
        });
    }

    #[test]
    fn template_refuses_pools_it_cannot_fully_replace() {
        block_on(async {
            let shared = SqlitePoolOptions::new()
                .max_connections(2)
                .connect("sqlite::memory:")
                .await
                .unwrap();
            assert!(apply_migrated_template(&shared).await.is_err());

            let directory = std::env::temp_dir().join(format!(
                "ganbaru-db-template-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&directory).unwrap();
            let file = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::new()
                        .filename(directory.join("vault.sqlite"))
                        .create_if_missing(true),
                )
                .await
                .unwrap();
            let refused = apply_migrated_template(&file).await;
            file.close().await;
            std::fs::remove_dir_all(&directory).unwrap();
            assert!(refused.is_err());
        });
    }

    #[test]
    fn written_database_opens_through_the_registry_without_migrating() {
        block_on(async {
            let directory = std::env::temp_dir().join(format!(
                "ganbaru-db-written-template-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("ganbaru.sqlite");
            write_migrated_database(&path).await.unwrap();
            assert!(write_migrated_database(&path).await.is_err());

            let registry = crate::DatabasePoolRegistry::default();
            let pool = registry.connect_path(&path).await.unwrap();
            validate_current_schema(&pool).await.unwrap();
            let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(mode, "wal");
            registry.close_all().await.unwrap();
            std::fs::remove_dir_all(&directory).unwrap();
        });
    }
}

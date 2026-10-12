//! Guarded access mode and per-connection hooks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sqlx::{SqliteConnection, SqlitePool};

use super::{block_on, registry_test_directory};
use crate::{
    ConnectionHook, ConnectionHookFuture, DatabaseAccessMode, DatabasePoolRegistry, connection_hook,
};

const GUARD_MESSAGE: &str = "guarded table";
const GUARD_SQL: &str =
    "CREATE TEMP TRIGGER IF NOT EXISTS access_guard BEFORE INSERT ON main.access_guarded
     BEGIN SELECT RAISE(ABORT, 'guarded table'); END;";

/// Boxing the statement erases the reborrowed connection lifetime from the hook future, which
/// the compiler cannot otherwise prove `Send` for every lifetime.
fn install_guard(conn: &mut SqliteConnection) -> ConnectionHookFuture<'_> {
    Box::pin(async move {
        sqlx::Executor::execute(conn, GUARD_SQL)
            .await
            .map(|_| ())
            .map_err(|error| format!("install guard: {error}"))
    })
}

/// A hook that records each call and guards `access_guarded` on guarded connections.
fn recording_hook(calls: Arc<Mutex<Vec<DatabaseAccessMode>>>) -> ConnectionHook {
    connection_hook(move |conn, access| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.lock().unwrap().push(access);
            if access == DatabaseAccessMode::Guarded {
                install_guard(conn).await?;
            }
            Ok(())
        })
    })
}

/// A migrated vault database with one guarded and one open test table.
async fn seeded_vault(directory: &Path) -> PathBuf {
    let path = directory.join("ganbaru-ai.sqlite");
    let registry = DatabasePoolRegistry::default();
    let pool = registry.connect_path(&path).await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE access_guarded (value TEXT NOT NULL);
         CREATE TABLE access_open (value TEXT NOT NULL);",
    )
    .execute(&pool)
    .await
    .unwrap();
    registry.close_all().await.unwrap();
    path
}

async fn insert(pool: &SqlitePool, table: &str) -> Result<(), String> {
    sqlx::query(&format!("INSERT INTO {table} (value) VALUES ('x')"))
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn guarded_pool_runs_the_hook_on_every_connection() {
    block_on(async {
        let directory = registry_test_directory("guarded-hook");
        let path = seeded_vault(&directory).await;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(recording_hook(calls.clone())));

        let (pool, access) = registry.connect_path_guarded(&path).await.unwrap();
        assert_eq!(access, DatabaseAccessMode::Guarded);
        assert_eq!(
            registry.access(&path).await,
            Some(DatabaseAccessMode::Guarded)
        );
        insert(&pool, "access_open").await.unwrap();
        assert!(
            insert(&pool, "access_guarded")
                .await
                .unwrap_err()
                .contains(GUARD_MESSAGE)
        );

        // A recycled connection must carry the guard again.
        pool.acquire().await.unwrap().close().await.unwrap();
        assert!(
            insert(&pool, "access_guarded")
                .await
                .unwrap_err()
                .contains(GUARD_MESSAGE)
        );
        assert_eq!(
            *calls.lock().unwrap(),
            vec![DatabaseAccessMode::Guarded, DatabaseAccessMode::Guarded]
        );

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn read_only_requests_reuse_an_open_guarded_pool() {
    block_on(async {
        let directory = registry_test_directory("guarded-reuse");
        let path = seeded_vault(&directory).await;
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(recording_hook(Arc::default())));
        registry.connect_path_guarded(&path).await.unwrap();

        let reader = registry.connect_path_read_only(&path).await.unwrap();
        insert(&reader, "access_open").await.unwrap();
        assert_eq!(
            registry.access(&path).await,
            Some(DatabaseAccessMode::Guarded)
        );
        assert!(registry.connect_path(&path).await.is_err());

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn guarded_open_degrades_to_read_only_on_schema_mismatch() {
    block_on(async {
        let directory = registry_test_directory("guarded-mismatch");
        let path = seeded_vault(&directory).await;
        let seed = DatabasePoolRegistry::default();
        let pool = seed.connect_path(&path).await.unwrap();
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = (SELECT MAX(version) FROM _sqlx_migrations)")
            .execute(&pool)
            .await
            .unwrap();
        seed.close_all().await.unwrap();

        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(recording_hook(calls.clone())));
        let (pool, access) = registry.connect_path_guarded(&path).await.unwrap();
        assert_eq!(access, DatabaseAccessMode::ReadOnly);
        assert!(insert(&pool, "access_open").await.is_err());
        let (_, again) = registry.connect_path_guarded(&path).await.unwrap();
        assert_eq!(again, DatabaseAccessMode::ReadOnly);
        // Read-only connections never run the hook.
        assert_eq!(*calls.lock().unwrap(), vec![DatabaseAccessMode::Guarded]);

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn guarded_open_never_creates_or_migrates_a_database() {
    block_on(async {
        let directory = registry_test_directory("guarded-missing");
        let path = directory.join("ganbaru-ai.sqlite");
        let registry = DatabasePoolRegistry::default();
        assert!(registry.connect_path_guarded(&path).await.is_err());
        assert!(!path.exists());
        assert_eq!(registry.access(&path).await, None);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn read_write_pools_run_the_hook_and_a_plain_read_only_pool_refuses_guarded_reuse() {
    block_on(async {
        let directory = registry_test_directory("hook-read-write");
        let path = seeded_vault(&directory).await;
        let calls = Arc::new(Mutex::new(Vec::new()));
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(recording_hook(calls.clone())));
        let pool = registry.connect_path(&path).await.unwrap();
        insert(&pool, "access_guarded").await.unwrap();
        assert_eq!(*calls.lock().unwrap(), vec![DatabaseAccessMode::ReadWrite]);
        registry.close_all().await.unwrap();

        registry.connect_path_read_only(&path).await.unwrap();
        assert!(registry.connect_path_guarded(&path).await.is_err());

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

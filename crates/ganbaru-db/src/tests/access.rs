//! Guarded access mode and per-connection hooks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sqlx::{SqliteConnection, SqlitePool};

use super::{block_on, registry_test_directory};
use crate::{
    ConnectionHook, ConnectionHookFuture, DatabaseAccessMode, DatabasePoolRegistry, MigratedHook,
    connection_hook,
};

const GUARD_MESSAGE: &str = "guarded table";
const GUARD_SQL: &str =
    "CREATE TEMP TRIGGER IF NOT EXISTS access_guard BEFORE INSERT ON main.notes_link_facts_state
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

/// A hook that records each call and guards `notes_link_facts_state` on guarded connections.
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

/// A migrated vault database written from the shared template. The hook guards one of its
/// tables and leaves another open.
async fn seeded_vault(directory: &Path) -> PathBuf {
    let path = directory.join("ganbaru.sqlite");
    crate::test_support::write_migrated_database(&path)
        .await
        .unwrap();
    path
}

async fn insert(pool: &SqlitePool, table: &str) -> Result<(), String> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT OR REPLACE INTO {table} (key, value) VALUES ('x', 'x')"
    )))
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
        insert(&pool, "notes_backlink_index_state").await.unwrap();
        assert!(
            insert(&pool, "notes_link_facts_state")
                .await
                .unwrap_err()
                .contains(GUARD_MESSAGE)
        );

        // A recycled connection must carry the guard again.
        pool.acquire().await.unwrap().close().await.unwrap();
        assert!(
            insert(&pool, "notes_link_facts_state")
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
        insert(&reader, "notes_backlink_index_state").await.unwrap();
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
        assert!(insert(&pool, "notes_backlink_index_state").await.is_err());
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
        let path = directory.join("ganbaru.sqlite");
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
        insert(&pool, "notes_link_facts_state").await.unwrap();
        assert_eq!(*calls.lock().unwrap(), vec![DatabaseAccessMode::ReadWrite]);
        registry.close_all().await.unwrap();

        registry.connect_path_read_only(&path).await.unwrap();
        assert!(registry.connect_path_guarded(&path).await.is_err());

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[cfg(unix)]
#[test]
fn writable_pools_keep_the_database_and_its_journal_files_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let sidecar = |path: &Path, suffix: &str| {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        PathBuf::from(file)
    };
    block_on(async {
        let directory = registry_test_directory("private-files");
        let path = directory.join("ganbaru.sqlite");
        let registry = DatabasePoolRegistry::default();
        let pool = registry.connect_path(&path).await.unwrap();
        insert(&pool, "notes_link_facts_state").await.unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&sidecar(&path, "-wal")), 0o600);
        assert_eq!(mode(&sidecar(&path, "-shm")), 0o600);
        registry.close_all().await.unwrap();

        let shared = std::fs::Permissions::from_mode(0o644);
        std::fs::set_permissions(&path, shared.clone()).unwrap();
        let journal = sidecar(&path, "-journal");
        std::fs::write(&journal, b"").unwrap();
        std::fs::set_permissions(&journal, shared.clone()).unwrap();
        registry.connect_path_read_only(&path).await.unwrap();
        assert_eq!(mode(&path), 0o644);
        registry.close_all().await.unwrap();
        std::fs::remove_file(&journal).unwrap();

        registry.connect_path_guarded(&path).await.unwrap();
        assert_eq!(mode(&path), 0o600);
        registry.close_all().await.unwrap();
        std::fs::set_permissions(&path, shared).unwrap();
        registry.connect_path(&path).await.unwrap();
        assert_eq!(mode(&path), 0o600);
        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn migrated_hook_runs_after_migrations_on_read_write_opens_only() {
    block_on(async {
        let directory = registry_test_directory("migrated-hook");
        let path = seeded_vault(&directory).await;
        let migrated = Arc::new(Mutex::new(Vec::new()));
        let registry = DatabasePoolRegistry::default();
        let recorded = migrated.clone();
        let hook: MigratedHook = Arc::new(move |pool: SqlitePool| {
            let recorded = recorded.clone();
            Box::pin(async move {
                let migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
                    .fetch_one(&pool)
                    .await
                    .map_err(|error| error.to_string())?;
                recorded.lock().unwrap().push(migrations);
                Ok(())
            })
        });
        registry.set_migrated_hook(Some(hook));

        registry.connect_path_read_only(&path).await.unwrap();
        registry.close_all().await.unwrap();
        assert!(migrated.lock().unwrap().is_empty());
        registry.connect_path(&path).await.unwrap();
        registry.connect_path(&path).await.unwrap();
        registry.close_all().await.unwrap();
        registry.connect_path(&path).await.unwrap();
        let expected = crate::MIGRATOR.iter().count() as i64;
        assert_eq!(*migrated.lock().unwrap(), vec![expected, expected]);

        registry.close_all().await.unwrap();
        registry.set_migrated_hook(Some(Arc::new(|_| {
            Box::pin(async { Err("conversion failed".to_string()) })
        })));
        assert!(
            registry
                .connect_path(&path)
                .await
                .unwrap_err()
                .contains("conversion failed")
        );
        assert_eq!(registry.access(&path).await, None);
        std::fs::remove_dir_all(directory).unwrap();
    });
}

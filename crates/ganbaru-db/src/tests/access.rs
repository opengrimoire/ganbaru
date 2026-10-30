//! Pool access modes and per-connection hooks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sqlx::SqlitePool;

use super::{block_on, registry_test_directory};
use crate::{ConnectionHook, DatabaseAccessMode, DatabasePoolRegistry, MigratedHook, connection_hook};

/// A hook that counts the connections it set up.
fn counting_hook(calls: Arc<Mutex<usize>>) -> ConnectionHook {
    connection_hook(move |_conn| {
        let calls = calls.clone();
        Box::pin(async move {
            *calls.lock().unwrap() += 1;
            Ok(())
        })
    })
}

/// A migrated vault database written from the shared template.
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
fn read_write_pools_run_the_hook_on_every_connection_and_read_only_pools_never_do() {
    block_on(async {
        let directory = registry_test_directory("hook-read-write");
        let path = seeded_vault(&directory).await;
        let calls = Arc::new(Mutex::new(0));
        let registry = DatabasePoolRegistry::default();
        registry.set_connection_hook(Some(counting_hook(calls.clone())));

        let pool = registry.connect_path(&path).await.unwrap();
        insert(&pool, "notes_link_facts_state").await.unwrap();
        assert_eq!(*calls.lock().unwrap(), 1);
        // A recycled connection runs the hook again.
        pool.acquire().await.unwrap().close().await.unwrap();
        insert(&pool, "notes_link_facts_state").await.unwrap();
        assert_eq!(*calls.lock().unwrap(), 2);
        registry.close_all().await.unwrap();

        let reader = registry.connect_path_read_only(&path).await.unwrap();
        assert_eq!(
            registry.access(&path).await,
            Some(DatabaseAccessMode::ReadOnly)
        );
        assert!(insert(&reader, "notes_link_facts_state").await.is_err());
        assert_eq!(*calls.lock().unwrap(), 2);

        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn an_open_pool_refuses_a_request_for_different_access() {
    block_on(async {
        let directory = registry_test_directory("access-change");
        let path = seeded_vault(&directory).await;
        let registry = DatabasePoolRegistry::default();

        registry.connect_path(&path).await.unwrap();
        assert!(registry.connect_path_read_only(&path).await.is_err());
        registry.close_all().await.unwrap();

        registry.connect_path_read_only(&path).await.unwrap();
        assert!(registry.connect_path(&path).await.is_err());
        assert_eq!(
            registry.access(&path).await,
            Some(DatabaseAccessMode::ReadOnly)
        );

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

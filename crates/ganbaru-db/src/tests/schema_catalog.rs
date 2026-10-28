use super::{block_on, registry_test_directory};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions};
use std::path::{Path, PathBuf};

/// Oldest bundled SQLite release with the fixes for CVE-2025-7709, CVE-2025-6965,
/// CVE-2025-29087, and CVE-2025-3277.
const MINIMUM_SQLITE_VERSION: (u32, u32, u32) = (3, 51, 3);

/// Opens a plain pool on a vault database, outside the registry, to tamper with it.
async fn raw_pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .unwrap()
}

/// Writes the shared migrated template as a vault database and returns its directory and path.
async fn migrated_vault(name: &str) -> (PathBuf, PathBuf) {
    let directory = registry_test_directory(name);
    let path = directory.join("ganbaru.sqlite");
    crate::test_support::write_migrated_database(&path)
        .await
        .unwrap();
    (directory, path)
}

/// Builds a database the way a crafted vault forges its history: every embedded migration's
/// SQL runs through `edit`, and history rows carry the genuine versions and checksums.
async fn forged_vault(name: &str, edit: impl Fn(&str) -> String) -> (PathBuf, PathBuf) {
    let directory = registry_test_directory(name);
    let path = directory.join("ganbaru.sqlite");
    let reference = super::helpers::migrated_memory_pool().await;
    let history_table: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = '_sqlx_migrations'")
            .fetch_one(&reference)
            .await
            .unwrap();
    reference.close().await;
    let pool = raw_pool(&path).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(history_table))
        .execute(&pool)
        .await
        .unwrap();
    for migration in crate::up_migrations() {
        sqlx::raw_sql(sqlx::AssertSqlSafe(edit(migration.sql.as_str())))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
             VALUES (?, ?, 1, ?, 0)",
        )
        .bind(migration.version)
        .bind(migration.description.as_ref())
        .bind(migration.checksum.as_ref())
        .execute(&pool)
        .await
        .unwrap();
    }
    pool.close().await;
    (directory, path)
}

/// Opens the database read-only through the registry and returns the current schema check.
async fn current_schema_check(path: &Path) -> Result<(), String> {
    let registry = crate::DatabasePoolRegistry::default();
    let pool = registry.connect_path_read_only(path).await.unwrap();
    let result = crate::validate_current_schema(&pool).await;
    registry.close_all().await.unwrap();
    result
}

fn parse_version(version: &str) -> (u32, u32, u32) {
    let mut parts = version.split('.').map(|part| part.parse::<u32>().unwrap());
    (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap_or(0),
    )
}

#[test]
fn bundled_sqlite_is_at_least_the_documented_minimum() {
    block_on(async {
        let (directory, path) = migrated_vault("sqlite-version").await;
        let registry = crate::DatabasePoolRegistry::default();
        let pool = registry.connect_path(&path).await.unwrap();
        let version: String = sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(
            parse_version(&version) >= MINIMUM_SQLITE_VERSION,
            "bundled SQLite {version} is older than the documented minimum"
        );
        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn registry_pools_distrust_schema_sql_and_check_cells() {
    block_on(async {
        let (directory, path) = migrated_vault("hardened-pragmas").await;
        let registry = crate::DatabasePoolRegistry::default();
        for read_only in [false, true] {
            let pool = if read_only {
                registry.connect_path_read_only(&path).await.unwrap()
            } else {
                registry.connect_path(&path).await.unwrap()
            };
            let trusted: i64 = sqlx::query_scalar("PRAGMA trusted_schema")
                .fetch_one(&pool)
                .await
                .unwrap();
            let cell_checks: i64 = sqlx::query_scalar("PRAGMA cell_size_check")
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(trusted, 0, "read-only: {read_only}");
            assert_eq!(cell_checks, 1, "read-only: {read_only}");
            registry.close_all().await.unwrap();
        }
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn defensive_mode_keeps_fts5_shadow_tables_read_only() {
    block_on(async {
        let (directory, path) = migrated_vault("defensive-mode").await;
        let registry = crate::DatabasePoolRegistry::default();
        let pool = registry.connect_path(&path).await.unwrap();
        let error = sqlx::query("DELETE FROM notes_search_fts_data")
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("may not be modified"), "{error}");
        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn faithfully_forged_history_matches_the_reference_catalog() {
    block_on(async {
        let (directory, path) = forged_vault("forged-faithful", str::to_string).await;
        current_schema_check(&path).await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn fresh_vault_passes_and_reopens_read_write() {
    block_on(async {
        let directory = registry_test_directory("fresh-catalog");
        let path = directory.join("ganbaru.sqlite");
        let registry = crate::DatabasePoolRegistry::default();
        registry.connect_path(&path).await.unwrap();
        registry.close_all().await.unwrap();
        current_schema_check(&path).await.unwrap();
        registry.connect_path(&path).await.unwrap();
        registry.close_all().await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn matching_history_with_an_extra_trigger_is_rejected() {
    block_on(async {
        let (directory, path) = migrated_vault("extra-trigger").await;
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(
            "CREATE TRIGGER forged_payload AFTER INSERT ON notes_link_facts_state
             BEGIN DELETE FROM notes_search_index_state; END",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let error = current_schema_check(&path).await.unwrap_err();
        assert!(error.contains("schema differs"), "{error}");
        let registry = crate::DatabasePoolRegistry::default();
        let error = registry.connect_path(&path).await.unwrap_err();
        assert!(error.contains("schema differs"), "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn migration_history_view_is_rejected() {
    block_on(async {
        let (directory, path) = migrated_vault("history-view").await;
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(
            "ALTER TABLE _sqlx_migrations RENAME TO forged_history;
             CREATE VIEW _sqlx_migrations AS SELECT * FROM forged_history;",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let error = current_schema_check(&path).await.unwrap_err();
        assert!(error.contains("not a table"), "{error}");
        let registry = crate::DatabasePoolRegistry::default();
        let error = registry.connect_path(&path).await.unwrap_err();
        assert!(error.contains("not a table"), "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn modified_column_default_is_rejected() {
    const ORIGINAL: &str = "CREATE TABLE notes_link_facts_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL,";
    const FORGED: &str = "CREATE TABLE notes_link_facts_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL DEFAULT 'forged',";
    block_on(async {
        assert_eq!(
            crate::up_migrations()
                .filter(|migration| migration.sql.as_str().contains(ORIGINAL))
                .count(),
            1,
            "the edited statement must exist in exactly one migration"
        );
        let (directory, path) =
            forged_vault("forged-default", |sql| sql.replace(ORIGINAL, FORGED)).await;

        let error = current_schema_check(&path).await.unwrap_err();
        assert!(error.contains("schema differs"), "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn tampered_fts5_shadow_table_is_rejected_before_integrity_checks() {
    block_on(async {
        let (directory, path) = migrated_vault("fts5-shadow").await;
        let pool = raw_pool(&path).await;
        sqlx::raw_sql(
            "DROP TABLE notes_search_fts;
             CREATE TABLE notes_search_fts_data (id INTEGER PRIMARY KEY, block BLOB);",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let error = current_schema_check(&path).await.unwrap_err();
        assert!(error.contains("schema differs"), "{error}");
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn statistics_tables_do_not_change_the_catalog() {
    block_on(async {
        let (directory, path) = migrated_vault("statistics").await;
        let pool = raw_pool(&path).await;
        sqlx::raw_sql("ANALYZE").execute(&pool).await.unwrap();
        pool.close().await;

        current_schema_check(&path).await.unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

use crate::run_migrations;
use sqlx::{Row, SqlitePool};

pub(super) async fn migrated_memory_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    pool
}

pub(super) async fn insert_event(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO calendar_events (id, title, start_time, end_time)
         VALUES ('event-1', 'Focus block', '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z')",
    )
    .execute(pool)
    .await
    .unwrap();
}

pub(super) async fn insert_open_run(
    pool: &SqlitePool,
    id: &str,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO pomodoro_runs
            (id, event_id, original_event_id, event_date, planned_start, planned_end,
             started_at, rhythm_kind, rhythm_source, preset_key, last_heartbeat, start_trigger)
         VALUES (?, 'event-1', 'event-1', '2026-05-23',
                 '2026-05-23T09:00:00Z', '2026-05-23T10:00:00Z',
                 '2026-05-23T09:00:00Z', 'count', 'preset', 'adaptive',
                 '2026-05-23T09:00:00Z', 'manual')",
    )
    .bind(id)
    .execute(pool)
    .await
}

pub(super) async fn query_plan(pool: &SqlitePool, sql: &str) -> String {
    sqlx::query(&format!("EXPLAIN QUERY PLAN {sql}"))
        .fetch_all(pool)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn assert_plan_uses(plan: &str, expected: &str) {
    assert!(
        plan.contains(expected),
        "expected query plan to use {expected}, got:\n{plan}",
    );
}

/// Embedded migrations up to and including one version, for upgrade tests.
#[derive(Debug)]
struct MigrationsThrough(i64);

impl sqlx::migrate::MigrationSource<'static> for MigrationsThrough {
    fn resolve(
        self,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Vec<sqlx::migrate::Migration>, sqlx::error::BoxDynError>,
                > + Send,
        >,
    > {
        let migrations = crate::MIGRATOR
            .iter()
            .filter(|migration| migration.version <= self.0)
            .cloned()
            .collect();
        Box::pin(async move { Ok(migrations) })
    }
}

/// A memory pool migrated through `version`; `run_migrations` later applies the rest.
pub(super) async fn memory_pool_migrated_through(version: i64) -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(MigrationsThrough(version))
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    pool
}

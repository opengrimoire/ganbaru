mod acknowledgements;
mod authorization;
mod budgets;
mod calendar;
mod capture;
mod compaction;
mod conformance;
mod distractions;
mod elision;
mod engine;
mod exclusion;
mod fixture;
mod focus;
mod fork;
mod groups;
mod guards;
mod holds;
mod local;
mod music;
mod owned;
mod preferences;
mod projects;
mod publication;
mod replica;
mod sim;
mod stability;
mod trigger_policies;
mod values;

use sqlx::{AssertSqlSafe, SqlitePool};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("sync test runtime should initialize")
        .block_on(future)
}

/// A freshly migrated vault database in memory, with one connection like vault pools.
async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=ON")
        .execute(&pool)
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    pool
}

async fn execute(pool: &SqlitePool, sql: &str) {
    sqlx::raw_sql(AssertSqlSafe(sql))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

async fn rejects(pool: &SqlitePool, sql: &str) -> bool {
    sqlx::raw_sql(AssertSqlSafe(sql))
        .execute(pool)
        .await
        .is_err()
}

async fn set_applying(pool: &SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(pool)
        .await
        .unwrap();
}

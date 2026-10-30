mod acknowledgements;
mod authorization;
mod budgets;
mod calendar;
mod capture;
mod chains;
mod chat;
mod compaction;
mod conformance;
mod contest;
mod distractions;
mod elision;
mod engine;
mod exclusion;
mod fixture;
mod focus;
mod fork;
mod group_members;
mod groups;
mod handover;
mod holds;
mod local;
mod local_assets;
mod music;
mod notes;
mod notes_assets;
mod notes_comments;
mod owned;
mod preferences;
mod projects;
mod publication;
mod record_exchange;
mod replica;
mod settle;
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

/// Arrival orders of `count` items that order-independence tests run: the stored order, its
/// reverse, and 16 seeded random orders, the same for every run of one count.
fn arrival_orders(count: usize) -> Vec<Vec<usize>> {
    let forward: Vec<usize> = (0..count).collect();
    let mut orders = vec![forward.clone(), forward.iter().rev().copied().collect()];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::try_from(count).unwrap();
    for _ in 0..16 {
        let mut order = forward.clone();
        for index in (1..count).rev() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let other = usize::try_from(state % (u64::try_from(index).unwrap() + 1)).unwrap();
            order.swap(index, other);
        }
        orders.push(order);
    }
    orders
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

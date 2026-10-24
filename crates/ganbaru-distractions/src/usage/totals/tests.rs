use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

use super::*;
use crate::test_support::block_on;
use crate::usage::usage_date_window;

const DEVICE: &str = "device-own";
const VAULT: &str = "vault-a";
const TODAY: &str = "2026-10-23";

async fn migrated_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory database");
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .expect("apply migrations");
    pool
}

fn window() -> UsageDateWindow {
    usage_date_window(chrono::NaiveDate::from_ymd_opt(2026, 10, 23).unwrap())
}

fn sample(
    id: &str,
    source_key: &str,
    local_date: &str,
    elapsed: i64,
) -> DistractionsUsageSampleRow {
    DistractionsUsageSampleRow {
        id: id.to_string(),
        source_type: "website".to_string(),
        source_key: source_key.to_string(),
        display_name: Some(source_key.to_string()),
        started_at_ms: 1,
        elapsed_seconds: elapsed,
        local_date: local_date.to_string(),
        created_at_ms: 1,
    }
}

fn bucket(source_key: &str) -> UsageBucket {
    UsageBucket {
        source_type: "website".to_string(),
        source_key: source_key.to_string(),
        local_date: TODAY.to_string(),
    }
}

fn stale(source_key: &str, elapsed_seconds: i64) -> StaleTotal {
    StaleTotal {
        bucket: bucket(source_key),
        elapsed_seconds,
        display_name: None,
    }
}

async fn record(pool: &SqlitePool, samples: &[DistractionsUsageSampleRow]) -> usize {
    let mut connection = pool.acquire().await.unwrap();
    insert_own_samples(&mut connection, samples, &window())
        .await
        .unwrap()
}

async fn own_totals(pool: &SqlitePool) -> Vec<(String, i64)> {
    sqlx::query_as(
        "SELECT source_key, elapsed_seconds FROM distractions_usage_totals
         WHERE device_id = ? ORDER BY source_key",
    )
    .bind(DEVICE)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn captures(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sync_capture WHERE table_id = 278")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn planner_flushes_a_bucket_at_most_once_per_interval_unless_forced() {
    let start = Instant::now();
    let mut planner = FlushPlanner::default();
    assert_eq!(
        planner
            .due(VAULT, vec![stale("a.com", 10)], start, false)
            .len(),
        1
    );
    planner.record(VAULT, [&bucket("a.com")], start);

    let soon = start + FLUSH_INTERVAL - Duration::from_secs(1);
    let due = planner.due(
        VAULT,
        vec![stale("a.com", 20), stale("b.com", 5)],
        soon,
        false,
    );
    assert_eq!(due, vec![stale("b.com", 5)], "a new bucket is due at once");
    assert_eq!(
        planner.due(VAULT, vec![stale("a.com", 20)], soon, true),
        vec![stale("a.com", 20)],
        "exit and background flushes skip the interval"
    );
    assert_eq!(
        planner
            .due(
                VAULT,
                vec![stale("a.com", 20)],
                start + FLUSH_INTERVAL,
                false
            )
            .len(),
        1
    );
}

#[test]
fn planner_starts_fresh_for_another_vault() {
    let now = Instant::now();
    let mut planner = FlushPlanner::default();
    planner.record(VAULT, [&bucket("a.com")], now);
    assert!(
        planner
            .due(VAULT, vec![stale("a.com", 20)], now, false)
            .is_empty()
    );
    assert_eq!(
        planner
            .due("vault-b", vec![stale("a.com", 20)], now, false)
            .len(),
        1
    );
}

#[test]
fn own_samples_flush_as_one_growing_total_per_bucket_on_the_flush_cadence() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut planner = FlushPlanner::default();
        let start = Instant::now();
        record(
            &pool,
            &[
                sample("s1", "video.example.com", TODAY, 120),
                sample("s2", "video.example.com", TODAY, 60),
                sample("s3", "news.example.com", TODAY, 30),
            ],
        )
        .await;
        let flushed = flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), start, false)
            .await
            .unwrap();
        assert_eq!(flushed, 2);
        assert_eq!(
            own_totals(&pool).await,
            vec![
                ("news.example.com".to_string(), 30),
                ("video.example.com".to_string(), 180)
            ]
        );
        let key: String = sqlx::query_scalar(
            "SELECT id FROM distractions_usage_totals WHERE source_key = 'video.example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            key,
            usage_total_key(DEVICE, &bucket("video.example.com")).unwrap()
        );

        record(&pool, &[sample("s4", "video.example.com", TODAY, 40)]).await;
        let soon = start + Duration::from_secs(60);
        assert_eq!(
            flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), soon, false)
                .await
                .unwrap(),
            0,
            "a bucket waits for its interval"
        );
        assert_eq!(own_totals(&pool).await[1].1, 180);
        assert_eq!(
            flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), soon, true)
                .await
                .unwrap(),
            1,
            "the exit flush writes only the bucket that changed"
        );
        assert_eq!(own_totals(&pool).await[1].1, 220);

        let before = captures(&pool).await;
        let later = start + FLUSH_INTERVAL * 2;
        assert_eq!(
            flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), later, true)
                .await
                .unwrap(),
            0,
            "current totals are not rewritten"
        );
        write_totals(
            &mut pool.acquire().await.unwrap(),
            DEVICE,
            &[StaleTotal {
                bucket: bucket("video.example.com"),
                elapsed_seconds: 220,
                display_name: Some("video.example.com".to_string()),
            }],
        )
        .await
        .unwrap();
        assert_eq!(
            captures(&pool).await,
            before,
            "an equal flush records no change"
        );
        pool.close().await;
    });
}

#[test]
fn flushes_never_shrink_a_total_and_cap_it_at_the_longest_day() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut planner = FlushPlanner::default();
        let now = Instant::now();
        record(
            &pool,
            &[
                sample("s1", "video.example.com", TODAY, 86_400),
                sample("s2", "video.example.com", TODAY, 86_400),
            ],
        )
        .await;
        flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), now, true)
            .await
            .unwrap();
        assert_eq!(own_totals(&pool).await[0].1, MAX_USAGE_DAY_SECONDS);

        sqlx::query("DELETE FROM distractions_usage_samples WHERE id = 's2'")
            .execute(&pool)
            .await
            .unwrap();
        flush_own_totals(&pool, &mut planner, VAULT, DEVICE, &window(), now, true)
            .await
            .unwrap();
        assert_eq!(own_totals(&pool).await[0].1, MAX_USAGE_DAY_SECONDS);
        pool.close().await;
    });
}

#[test]
fn own_samples_keep_the_retention_window_and_retries_are_ignored() {
    block_on(async {
        let pool = migrated_pool().await;
        let old = sample("old", "video.example.com", "2025-01-01", 30);
        let future = sample("future", "video.example.com", "2026-12-01", 30);
        let today = sample("today", "video.example.com", TODAY, 30);
        assert_eq!(
            record(&pool, &[old, future, today.clone(), today.clone()]).await,
            2
        );
        assert_eq!(record(&pool, &[today]).await, 0);
        let ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM distractions_usage_samples ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(ids, vec!["today".to_string()]);

        let mut planner = FlushPlanner::default();
        flush_own_totals(
            &pool,
            &mut planner,
            VAULT,
            DEVICE,
            &window(),
            Instant::now(),
            true,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE distractions_usage_samples SET local_date = '2024-01-01'")
            .execute(&pool)
            .await
            .unwrap();
        record(&pool, &[]).await;
        assert_eq!(
            own_totals(&pool).await.len(),
            1,
            "retention drops samples but keeps flushed totals"
        );
        pool.close().await;
    });
}

#[test]
fn flushes_refuse_an_invalid_device_id() {
    block_on(async {
        let pool = migrated_pool().await;
        let mut planner = FlushPlanner::default();
        for device in ["", &"d".repeat(MAX_DEVICE_ID_BYTES + 1)] {
            assert!(
                flush_own_totals(
                    &pool,
                    &mut planner,
                    VAULT,
                    device,
                    &window(),
                    Instant::now(),
                    true
                )
                .await
                .is_err()
            );
        }
        pool.close().await;
    });
}

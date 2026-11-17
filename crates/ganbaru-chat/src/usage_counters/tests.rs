use super::*;
use crate::events::ProviderAttributedCost;
use sqlx::SqlitePool;

fn report(
    scope: Option<UsageCountScope>,
    input: u64,
    cached_input: u64,
    cache_write_input: u64,
    output: u64,
) -> ThreadUsageUpdatedEvent {
    ThreadUsageUpdatedEvent {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: Some(cached_input),
        cache_write_input_tokens: Some(cache_write_input),
        context_tokens: None,
        context_limit: None,
        cost: None,
        count_scope: scope,
    }
}

fn with_cost(mut event: ThreadUsageUpdatedEvent, amount: f64) -> ThreadUsageUpdatedEvent {
    event.cost = Some(ProviderAttributedCost {
        amount,
        currency: "USD".to_string(),
        provider_reported: true,
    });
    event
}

fn tokens(input: u64, cached_input: u64, cache_write_input: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input,
        cached_input,
        cache_write_input,
        output,
    }
}

fn fold_all(events: &[ThreadUsageUpdatedEvent], carried: Option<&ThreadTotals>) -> RunUsage {
    events.iter().fold(RunUsage::default(), |usage, event| {
        fold_usage(&usage, event, carried).unwrap()
    })
}

#[test]
fn counted_tokens_leave_out_cache_reads() {
    assert_eq!(tokens(10, 500, 20, 5).counted(), 35);
}

#[test]
fn fold_usage_request_scope_adds() {
    let request = Some(UsageCountScope::Request);
    let usage = fold_all(
        &[
            with_cost(report(request, 10, 100, 5, 20), 0.01),
            with_cost(report(request, 3, 50, 0, 7), 0.02),
        ],
        None,
    );
    assert_eq!(usage.tokens, tokens(13, 150, 5, 27));
    assert_eq!(usage.tokens.counted(), 45);
    let cost = usage.cost.expect("the cost should be counted");
    assert!((cost.amount - 0.03).abs() < 1e-12);
    assert_eq!(usage.thread_last, None);
}

#[test]
fn fold_usage_turn_scope_replaces_running_total() {
    let turn = Some(UsageCountScope::Turn);
    let usage = fold_all(
        &[
            with_cost(report(turn, 10, 100, 5, 20), 0.01),
            with_cost(report(turn, 25, 300, 5, 60), 0.04),
        ],
        None,
    );
    assert_eq!(
        usage.tokens,
        tokens(25, 300, 5, 60),
        "a later turn total replaces the earlier one instead of adding to it"
    );
    assert_eq!(usage.cost.map(|cost| cost.amount), Some(0.04));
}

#[test]
fn fold_usage_turn_completed_after_requests_does_not_double_count() {
    let request = Some(UsageCountScope::Request);
    let usage = fold_all(
        &[
            report(request, 10, 100, 5, 20),
            report(request, 4, 40, 0, 6),
            // The turn total equals the request sum already counted.
            report(Some(UsageCountScope::Turn), 14, 140, 5, 26),
        ],
        None,
    );
    assert_eq!(usage.tokens, tokens(14, 140, 5, 26));
    assert_eq!(usage.tokens.counted(), 45);

    let repeated = fold_all(
        &[
            report(Some(UsageCountScope::Turn), 14, 140, 5, 26),
            report(Some(UsageCountScope::Turn), 14, 140, 5, 26),
        ],
        None,
    );
    assert_eq!(
        repeated.tokens.counted(),
        45,
        "a repeated turn report adds nothing"
    );
}

#[test]
fn fold_usage_thread_cumulative_uses_delta_from_last_total() {
    let cumulative = Some(UsageCountScope::ThreadCumulative);
    let carried = ThreadTotals {
        tokens: tokens(1_000, 5_000, 100, 400),
        cost: None,
    };
    let usage = fold_all(
        &[
            report(cumulative, 1_200, 6_000, 100, 450),
            report(cumulative, 1_300, 6_500, 150, 500),
        ],
        Some(&carried),
    );
    assert_eq!(
        usage.tokens,
        tokens(300, 1_500, 50, 100),
        "only the increase over the earlier run's totals counts"
    );
    assert_eq!(
        usage.thread_last.map(|last| last.tokens),
        Some(tokens(1_300, 6_500, 150, 500))
    );

    let fresh = fold_all(&[report(cumulative, 70, 0, 0, 30)], None);
    assert_eq!(
        fresh.tokens,
        tokens(70, 0, 0, 30),
        "a new provider thread counts from zero"
    );
}

#[test]
fn fold_usage_thread_cumulative_restart_below_carried_totals_counts_from_zero() {
    let cumulative = Some(UsageCountScope::ThreadCumulative);
    let carried = ThreadTotals {
        tokens: tokens(1_000, 5_000, 100, 400),
        cost: None,
    };
    let event = report(cumulative, 80, 0, 0, 20);
    assert!(thread_count_restarted(
        &RunUsage::default(),
        &event,
        Some(&carried)
    ));
    let usage = fold_usage(&RunUsage::default(), &event, Some(&carried)).unwrap();
    assert_eq!(usage.tokens, tokens(80, 0, 0, 20));
}

#[test]
fn fold_usage_thread_cumulative_refuses_backwards_total() {
    let cumulative = Some(UsageCountScope::ThreadCumulative);
    let counted = fold_all(&[report(cumulative, 500, 1_000, 0, 200)], None);
    assert_eq!(
        fold_usage(&counted, &report(cumulative, 499, 1_000, 0, 200), None),
        Err(UsageCountError::TotalWentBackwards)
    );
    assert_eq!(
        fold_usage(&counted, &report(cumulative, 500, 1_000, 0, 200), None).unwrap(),
        counted,
        "a repeated total adds nothing"
    );
}

#[test]
fn fold_usage_thread_cumulative_cost_counts_its_increase() {
    let cumulative = Some(UsageCountScope::ThreadCumulative);
    let usage = fold_all(
        &[
            with_cost(report(cumulative, 10, 0, 0, 5), 0.10),
            with_cost(report(cumulative, 20, 0, 0, 10), 0.25),
        ],
        None,
    );
    let cost = usage.cost.expect("the cost should be counted");
    assert!((cost.amount - 0.25).abs() < 1e-12);
}

#[test]
fn fold_usage_unknown_scope_is_ignored() {
    let current = fold_all(&[report(Some(UsageCountScope::Request), 5, 0, 0, 5)], None);
    assert_eq!(
        fold_usage(&current, &report(None, 100, 0, 0, 100), None),
        Err(UsageCountError::UnknownScope)
    );
    let fill_only = ThreadUsageUpdatedEvent {
        input_tokens: None,
        output_tokens: None,
        cached_input_tokens: None,
        cache_write_input_tokens: None,
        context_tokens: Some(4_000),
        context_limit: Some(200_000),
        cost: None,
        count_scope: None,
    };
    assert_eq!(
        fold_usage(&current, &fill_only, None).unwrap(),
        current,
        "a context fill report has nothing to count and is not an unknown scope"
    );
}

#[test]
fn fold_usage_keeps_one_cost_currency() {
    let request = Some(UsageCountScope::Request);
    let mut other = with_cost(report(request, 1, 0, 0, 1), 5.0);
    if let Some(cost) = other.cost.as_mut() {
        cost.currency = "EUR".to_string();
    }
    let usage = fold_all(&[with_cost(report(request, 1, 0, 0, 1), 0.5), other], None);
    assert_eq!(
        usage.cost,
        Some(RunCost {
            amount: 0.5,
            currency: "USD".to_string()
        })
    );
    assert_eq!(usage.tokens.counted(), 4, "tokens still count");
}

const RUN: &str = "run:usage-1";
const EARLIER_RUN: &str = "run:usage-0";
const THREAD: &str = "thread:usage";

async fn migrated_pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    ganbaru_db::test_support::apply_migrated_template(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA foreign_keys=OFF")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// Seeds the run `run_id` in `state` on the test thread as a sync apply, so it needs none of
/// the assignment, policy, and authorization rows a dispatch creates.
async fn seed_run(pool: &SqlitePool, run_id: &str, ordinal: i64, state: &str, at: &str) {
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         INSERT INTO chat_agent_runs
            (id, assignment_id, project_id, agent_policy_revision_id,
             authorization_revision_id, authorization_scope_digest, provider_turn_id,
             provider_thread_id, state, run_ordinal, created_at, updated_at)
         VALUES ('{run_id}', 'assignment:usage', 'project-usage', 'policy:usage',
                 'authorization:usage', '{digest}', 'turn:{run_id}', '{THREAD}', '{state}',
                 {ordinal}, '{at}', '{at}');
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
        digest = "a".repeat(64),
    )))
    .execute(pool)
    .await
    .unwrap();
}

async fn set_state(pool: &SqlitePool, state: &str, at: &str) {
    sqlx::query("UPDATE chat_agent_runs SET state = ?, updated_at = ? WHERE id = ?")
        .bind(state)
        .bind(at)
        .bind(RUN)
        .execute(pool)
        .await
        .unwrap();
}

async fn working_time(pool: &SqlitePool) -> (i64, Option<String>) {
    sqlx::query_as(
        "SELECT working_seconds, working_since FROM chat_agent_run_usage WHERE run_id = ?",
    )
    .bind(RUN)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn record(pool: &SqlitePool, event: &ThreadUsageUpdatedEvent) {
    let mut transaction = pool.begin().await.unwrap();
    record_run_usage(
        &mut transaction,
        RUN,
        THREAD,
        event,
        "2026-11-12T10:00:00.000Z",
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
}

async fn stored(pool: &SqlitePool) -> (i64, i64, Option<i64>, i64) {
    sqlx::query_as(
        "SELECT counted_tokens, input_tokens, thread_input_tokens_last, scope_unknown_logged
         FROM chat_agent_run_usage WHERE run_id = ?",
    )
    .bind(RUN)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn working_time_excludes_waiting_states() {
    let pool = migrated_pool().await;
    seed_run(&pool, RUN, 1, "starting", "2026-11-12T09:00:00.000Z").await;
    set_state(&pool, "working", "2026-11-12T09:00:10.000Z").await;
    set_state(&pool, "waiting", "2026-11-12T09:01:10.000Z").await;
    assert_eq!(working_time(&pool).await, (60, None));
    // An hour waiting for a person adds nothing.
    set_state(&pool, "working", "2026-11-12T10:01:10.000Z").await;
    assert_eq!(
        working_time(&pool).await,
        (60, Some("2026-11-12T10:01:10.000Z".to_string()))
    );
    set_state(&pool, "completed", "2026-11-12T10:01:40.000Z").await;
    assert_eq!(working_time(&pool).await, (90, None));
}

#[tokio::test]
async fn working_time_skips_a_reversed_interval_and_sync_applies() {
    let pool = migrated_pool().await;
    seed_run(&pool, RUN, 1, "starting", "2026-11-12T09:00:00.000Z").await;
    set_state(&pool, "working", "2026-11-12T09:00:10.000Z").await;
    set_state(&pool, "failed", "2026-11-12T09:00:00.000Z").await;
    assert_eq!(working_time(&pool).await, (0, None));

    sqlx::raw_sql(
        "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
         UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:usage-1';
         UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(working_time(&pool).await, (0, None));
}

#[tokio::test]
async fn recorded_usage_carries_thread_totals_from_the_earlier_run() {
    let pool = migrated_pool().await;
    seed_run(
        &pool,
        EARLIER_RUN,
        1,
        "completed",
        "2026-11-12T08:00:00.000Z",
    )
    .await;
    sqlx::query(
        "INSERT INTO chat_agent_run_usage
            (run_id, input_tokens, output_tokens, counted_tokens,
             thread_input_tokens_last, thread_cached_input_tokens_last,
             thread_cache_write_input_tokens_last, thread_output_tokens_last, updated_at)
         VALUES (?, 1000, 400, 1400, 1000, 0, 0, 400, '2026-11-12T08:30:00.000Z')",
    )
    .bind(EARLIER_RUN)
    .execute(&pool)
    .await
    .unwrap();
    seed_run(&pool, RUN, 2, "working", "2026-11-12T09:00:00.000Z").await;

    record(
        &pool,
        &report(Some(UsageCountScope::ThreadCumulative), 1_300, 0, 0, 500),
    )
    .await;
    assert_eq!(stored(&pool).await, (400, 300, Some(1_300), 0));
}

#[tokio::test]
async fn recorded_usage_logs_an_unknown_scope_once_and_counts_nothing() {
    let pool = migrated_pool().await;
    seed_run(&pool, RUN, 1, "working", "2026-11-12T09:00:00.000Z").await;
    record(&pool, &report(None, 100, 0, 0, 100)).await;
    record(&pool, &report(None, 100, 0, 0, 100)).await;
    assert_eq!(stored(&pool).await, (0, 0, None, 1));
}

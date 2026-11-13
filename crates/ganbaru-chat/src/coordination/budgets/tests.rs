use super::*;
use crate::coordination::projection::{
    ProviderEventProjection, project_provider_event_in_transaction,
};
use crate::coordination::reports::tests::{
    ASSIGNMENT, NOW, RUN, TURN, migrated_pool, seed, states, updates,
};
use crate::events::{
    CanonicalEvent, CanonicalRuntimeEvent, RateLimitStatusEvent, ThreadUsageUpdatedEvent,
    TurnAbortedEvent, TurnCompletedEvent, UsageCountScope,
};
use crate::models::{
    ChatEventId, ChatThreadId, ChatTurnId, ChatTurnState, ProviderFamilyId, ProviderInstanceId,
    UtcTimestamp,
};

const PROVIDER_THREAD: &str = "thread:reports";
const RESETS_AT: &str = "2026-11-12T14:00:00.000Z";

/// Gives the assignment a budget with the given limits and the run a usage row that started
/// working at `working_since`, when given.
async fn seed_budget(
    pool: &SqlitePool,
    token_limit: Option<i64>,
    working_seconds_limit: Option<i64>,
    working_since: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO chat_assignment_budgets
            (id, assignment_id, token_limit, working_seconds_limit, set_at)
         VALUES ('budget:reports', ?, ?, ?, ?)",
    )
    .bind(ASSIGNMENT)
    .bind(token_limit)
    .bind(working_seconds_limit)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_agent_run_usage (run_id, working_since, updated_at) VALUES (?, ?, ?)",
    )
    .bind(RUN)
    .bind(working_since)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

fn at_seconds(seconds: u32) -> String {
    format!("2026-11-12T09:{:02}:{:02}.000Z", seconds / 60, seconds % 60)
}

fn runtime_event(
    event_id: &str,
    turn: &str,
    created_at: &str,
    event: CanonicalEvent,
) -> CanonicalRuntimeEvent {
    CanonicalRuntimeEvent {
        schema_version: 1,
        event_id: ChatEventId::new(event_id.to_owned()).unwrap(),
        provider_family_id: ProviderFamilyId::new("codex".to_owned()).unwrap(),
        provider_instance_id: ProviderInstanceId::new("codex-local".to_owned()).unwrap(),
        thread_id: ChatThreadId::new(PROVIDER_THREAD.to_owned()).unwrap(),
        created_at: UtcTimestamp::new(created_at.to_owned()).unwrap(),
        turn_id: Some(ChatTurnId::new(turn.to_owned()).unwrap()),
        provider_turn_id: None,
        provider_item_id: None,
        provider_request_id: None,
        provider_task_id: None,
        provider_reference: None,
        event,
        redacted_diagnostic: None,
    }
}

fn request_usage(input: u64, output: u64) -> CanonicalEvent {
    CanonicalEvent::ThreadUsageUpdated(ThreadUsageUpdatedEvent {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: Some(0),
        cache_write_input_tokens: Some(0),
        context_tokens: None,
        context_limit: None,
        cost: None,
        count_scope: Some(UsageCountScope::Request),
    })
}

async fn project(pool: &SqlitePool, runtime: &CanonicalRuntimeEvent) -> ProviderEventProjection {
    let mut transaction = pool.begin().await.unwrap();
    let projection = project_provider_event_in_transaction(&mut transaction, runtime)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    projection
}

/// Stores `event` in the provider thread's event log, as ingestion does before projecting.
async fn store_event(pool: &SqlitePool, sequence: i64, runtime: &CanonicalRuntimeEvent) {
    let payload = serde_json::to_value(&runtime.event).unwrap();
    sqlx::raw_sql("UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO chat_events
            (id, thread_id, sequence, turn_id, provider_family_id, provider_instance_id,
             event_type, payload_schema_version, payload_data, created_at, ingested_at)
         VALUES (?, ?, ?, ?, 'codex', 'codex-local', ?, 1, ?, ?, ?)",
    )
    .bind(runtime.event_id.as_str())
    .bind(PROVIDER_THREAD)
    .bind(sequence)
    .bind(runtime.turn_id.as_ref().map(ChatTurnId::as_str))
    .bind(payload["type"].as_str().unwrap())
    .bind(payload.to_string())
    .bind(runtime.created_at.as_str())
    .bind(runtime.created_at.as_str())
    .execute(pool)
    .await
    .unwrap();
    sqlx::raw_sql("UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1")
        .execute(pool)
        .await
        .unwrap();
}

async fn recorded(pool: &SqlitePool) -> (i64, i64) {
    sqlx::query_as(
        "SELECT used_counted_tokens, used_working_seconds FROM chat_assignment_budgets
         WHERE assignment_id = ?",
    )
    .bind(ASSIGNMENT)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn state_reason(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar("SELECT state_reason FROM chat_work_assignments WHERE id = ?")
        .bind(ASSIGNMENT)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// The update payload of every failure update of the assignment.
async fn failure_payloads(pool: &SqlitePool) -> Vec<serde_json::Value> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT json_extract(payload_data, '$.payload') FROM chat_work_semantic_updates
         WHERE assignment_id = ? AND update_kind = 'failure'",
    )
    .bind(ASSIGNMENT)
    .fetch_all(pool)
    .await
    .unwrap();
    rows.iter()
        .map(|row| serde_json::from_str(row).unwrap())
        .collect()
}

#[tokio::test]
async fn budget_token_limit_stops_the_run_once_with_its_reason() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    seed_budget(&pool, Some(1_000), None, None).await;

    let first = runtime_event(
        "event:usage-1",
        TURN,
        &at_seconds(10),
        request_usage(700, 400),
    );
    let projection = project(&pool, &first).await;

    let assignment_id = ChatWorkAssignmentId::new(ASSIGNMENT.to_owned()).unwrap();
    assert_eq!(projection.budget_stop, Some(assignment_id));
    assert!(projection.projected);
    assert_eq!(
        states(&pool).await,
        ("failed".to_owned(), "failed".to_owned())
    );
    assert_eq!(
        state_reason(&pool).await.as_deref(),
        Some(BUDGET_LIMIT_REACHED_REASON)
    );
    assert_eq!(recorded(&pool).await.0, 1_100);
    assert_eq!(
        failure_payloads(&pool).await,
        vec![json!({
            "code": "budget_limit_reached",
            "limit": "tokens",
            "used": 1_100,
            "limitValue": 1_000,
        })]
    );

    let late = runtime_event(
        "event:usage-2",
        TURN,
        &at_seconds(20),
        request_usage(50, 50),
    );
    let projection = project(&pool, &late).await;
    assert_eq!(projection, ProviderEventProjection::default());
    assert_eq!(
        updates(&pool).await,
        vec![(
            "failure".to_owned(),
            "Stopped at the token limit set for this work.".to_owned()
        )]
    );
}

#[tokio::test]
async fn budget_used_amount_records_only_at_thresholds_and_settle() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    seed_budget(&pool, Some(10_000), None, None).await;

    project(
        &pool,
        &runtime_event(
            "event:usage-1",
            TURN,
            &at_seconds(10),
            request_usage(30, 12),
        ),
    )
    .await;
    assert_eq!(
        recorded(&pool).await,
        (0, 0),
        "below a threshold writes nothing"
    );

    project(
        &pool,
        &runtime_event(
            "event:usage-2",
            TURN,
            &at_seconds(20),
            request_usage(7_000, 600),
        ),
    )
    .await;
    assert_eq!(
        recorded(&pool).await.0,
        7_642,
        "crossing 75 percent records"
    );

    project(
        &pool,
        &runtime_event(
            "event:usage-3",
            TURN,
            &at_seconds(30),
            request_usage(100, 0),
        ),
    )
    .await;
    assert_eq!(
        recorded(&pool).await.0,
        7_642,
        "within one band writes nothing"
    );

    project(
        &pool,
        &runtime_event(
            "event:completed",
            TURN,
            &at_seconds(40),
            CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                state: ChatTurnState::Completed,
                stop_reason: None,
                usage: None,
                changed_files: Vec::new(),
            }),
        ),
    )
    .await;
    assert_eq!(
        recorded(&pool).await.0,
        7_742,
        "settling records what the run used"
    );
    assert_eq!(states(&pool).await.0, "completed");
}

#[tokio::test]
async fn budget_working_time_stops_on_the_tick_without_provider_events() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    seed_budget(&pool, None, Some(60), Some(NOW)).await;

    assert!(
        check_local_budgets(&pool, &at_seconds(30))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(recorded(&pool).await, (0, 0));

    assert!(
        check_local_budgets(&pool, &at_seconds(50))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(recorded(&pool).await.1, 50, "crossing 75 percent records");

    let stopped = check_local_budgets(&pool, &at_seconds(70)).await.unwrap();
    assert_eq!(
        stopped,
        vec![ChatWorkAssignmentId::new(ASSIGNMENT.to_owned()).unwrap()]
    );
    assert_eq!(
        states(&pool).await,
        ("failed".to_owned(), "failed".to_owned())
    );
    assert_eq!(recorded(&pool).await.1, 70);
    assert_eq!(failure_payloads(&pool).await[0]["limit"], "working_time");

    assert!(
        check_local_budgets(&pool, &at_seconds(90))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(failure_payloads(&pool).await.len(), 1);
}

#[tokio::test]
async fn budget_free_provider_limit_abort_fails_with_the_provider_reason() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let limited = runtime_event(
        "event:limited",
        TURN,
        &at_seconds(10),
        CanonicalEvent::RateLimitStatus(RateLimitStatusEvent {
            limited: true,
            resets_at: Some(UtcTimestamp::new(RESETS_AT.to_owned()).unwrap()),
            detail: None,
            provider_data: None,
        }),
    );
    store_event(&pool, 1, &limited).await;
    let aborted = runtime_event(
        "event:aborted",
        TURN,
        &at_seconds(11),
        CanonicalEvent::TurnAborted(TurnAbortedEvent {
            state: ChatTurnState::Failed,
            reason: "Usage limit reached".to_owned(),
            recoverable: false,
        }),
    );
    store_event(&pool, 2, &aborted).await;
    project(&pool, &aborted).await;

    assert_eq!(states(&pool).await.0, "failed");
    assert_eq!(
        state_reason(&pool).await.as_deref(),
        Some(PROVIDER_USAGE_LIMIT_REASON)
    );
    assert_eq!(
        failure_payloads(&pool).await,
        vec![json!({ "code": "provider_usage_limit", "resetsAt": RESETS_AT })]
    );
}

#[tokio::test]
async fn budget_free_failed_codex_turn_after_a_limit_fails_with_the_provider_reason() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let limited = runtime_event(
        "event:limited",
        TURN,
        &at_seconds(10),
        CanonicalEvent::RateLimitStatus(RateLimitStatusEvent {
            limited: true,
            resets_at: None,
            detail: None,
            provider_data: None,
        }),
    );
    store_event(&pool, 1, &limited).await;
    let failed = runtime_event(
        "event:failed",
        TURN,
        &at_seconds(11),
        CanonicalEvent::TurnCompleted(TurnCompletedEvent {
            state: ChatTurnState::Failed,
            stop_reason: None,
            usage: None,
            changed_files: Vec::new(),
        }),
    );
    store_event(&pool, 2, &failed).await;
    project(&pool, &failed).await;

    assert_eq!(states(&pool).await.0, "failed");
    assert_eq!(
        state_reason(&pool).await.as_deref(),
        Some(PROVIDER_USAGE_LIMIT_REASON)
    );
    assert_eq!(
        failure_payloads(&pool).await,
        vec![json!({ "code": "provider_usage_limit", "resetsAt": null })]
    );
}

#[tokio::test]
async fn budget_free_stale_provider_limit_does_not_relabel_a_later_abort() {
    let pool = migrated_pool().await;
    seed(&pool, "working", "working").await;
    let limited = runtime_event(
        "event:limited",
        "turn:earlier",
        &at_seconds(1),
        CanonicalEvent::RateLimitStatus(RateLimitStatusEvent {
            limited: true,
            resets_at: None,
            detail: None,
            provider_data: None,
        }),
    );
    store_event(&pool, 1, &limited).await;
    let earlier_end = runtime_event(
        "event:earlier-aborted",
        "turn:earlier",
        &at_seconds(2),
        CanonicalEvent::TurnAborted(TurnAbortedEvent {
            state: ChatTurnState::Failed,
            reason: "Usage limit reached".to_owned(),
            recoverable: false,
        }),
    );
    store_event(&pool, 2, &earlier_end).await;
    let aborted = runtime_event(
        "event:aborted",
        TURN,
        &at_seconds(20),
        CanonicalEvent::TurnAborted(TurnAbortedEvent {
            state: ChatTurnState::Interrupted,
            reason: "Stopped".to_owned(),
            recoverable: true,
        }),
    );
    store_event(&pool, 3, &aborted).await;
    project(&pool, &aborted).await;

    assert_eq!(state_reason(&pool).await.as_deref(), Some("Stopped"));
    assert_eq!(
        failure_payloads(&pool).await,
        vec![json!({ "recoverable": true })]
    );
}

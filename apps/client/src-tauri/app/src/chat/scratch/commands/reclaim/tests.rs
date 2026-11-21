use super::*;
use crate::chat::scratch::tests::{AGENT, GENERATION, pool_with_scratch_assignment};

/// The scratch fixture writes every timestamp at this instant.
const FIXTURE_NOW: &str = "2026-09-18T12:00:00.000Z";

fn fixture_instant() -> DateTime<Utc> {
    parse_instant(FIXTURE_NOW).unwrap()
}

/// Builds the fixture scratch assignment with its run already finished.
async fn pool_with_finished_run() -> SqlitePool {
    let pool = pool_with_scratch_assignment(AGENT).await;
    sqlx::query("UPDATE chat_agent_runs SET state = 'completed' WHERE id = 'run:scratch'")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

async fn set_states(pool: &SqlitePool, scope: &str, generation: &str) {
    sqlx::query("UPDATE chat_scratch_scopes SET lifecycle_state = ? WHERE id = 'scope:scratch'")
        .bind(scope)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chat_scratch_generations SET lifecycle_state = ? WHERE id = ?")
        .bind(generation)
        .bind(GENERATION)
        .execute(pool)
        .await
        .unwrap();
}

async fn reasons_at(
    pool: &SqlitePool,
    now: DateTime<Utc>,
) -> Vec<(String, ChatScratchReclaimReason)> {
    qualify_candidates(read_reclaim_candidates(pool).await.unwrap(), now)
        .unwrap()
        .into_iter()
        .map(|(candidate, reason)| (candidate.generation_id.as_str().to_string(), reason))
        .collect()
}

#[tokio::test]
async fn each_reclaimable_state_reports_its_reason() {
    let pool = pool_with_finished_run().await;
    let soon = fixture_instant() + Duration::days(1);
    for (scope, generation, reason) in [
        (
            "archived",
            "active",
            ChatScratchReclaimReason::ScopeArchived,
        ),
        (
            "archived",
            "quarantined",
            ChatScratchReclaimReason::ScopeArchived,
        ),
        (
            "cleanup_failed",
            "active",
            ChatScratchReclaimReason::ScopeCleanupFailed,
        ),
        (
            "active",
            "cleanup_failed",
            ChatScratchReclaimReason::GenerationCleanupFailed,
        ),
        (
            "active",
            "quarantined",
            ChatScratchReclaimReason::GenerationQuarantined,
        ),
    ] {
        set_states(&pool, scope, generation).await;
        assert_eq!(
            reasons_at(&pool, soon).await,
            vec![(GENERATION.to_string(), reason)],
            "{scope} scope with {generation} generation",
        );
    }
}

#[tokio::test]
async fn active_scratch_qualifies_only_after_the_idle_period() {
    let pool = pool_with_finished_run().await;
    let just_inside = fixture_instant() + Duration::days(SCRATCH_RECLAIM_IDLE_DAYS);
    assert!(reasons_at(&pool, just_inside).await.is_empty());
    let past = just_inside + Duration::seconds(1);
    assert_eq!(
        reasons_at(&pool, past).await,
        vec![(GENERATION.to_string(), ChatScratchReclaimReason::Idle)],
    );
}

#[tokio::test]
async fn a_recent_run_update_keeps_scratch_from_idling() {
    let pool = pool_with_finished_run().await;
    sqlx::query("UPDATE chat_agent_runs SET updated_at = ? WHERE id = 'run:scratch'")
        .bind("2026-10-18T12:00:00.000Z")
        .execute(&pool)
        .await
        .unwrap();
    let candidates = read_reclaim_candidates(&pool).await.unwrap();
    assert_eq!(
        candidates[0].last_activity_at.as_str(),
        "2026-10-18T12:00:00.000Z"
    );
    let after_generation_idle = fixture_instant() + Duration::days(40);
    assert!(reasons_at(&pool, after_generation_idle).await.is_empty());
}

#[tokio::test]
async fn active_work_excludes_otherwise_reclaimable_scratch() {
    let pool = pool_with_scratch_assignment(AGENT).await;
    set_states(&pool, "archived", "quarantined").await;
    let much_later = fixture_instant() + Duration::days(365);
    assert!(reasons_at(&pool, much_later).await.is_empty());
    for state in ["queued", "starting", "waiting"] {
        sqlx::query("UPDATE chat_agent_runs SET state = ? WHERE id = 'run:scratch'")
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
        assert!(reasons_at(&pool, much_later).await.is_empty(), "{state}");
    }
}

#[tokio::test]
async fn generations_the_cleanup_refuses_are_not_candidates() {
    let pool = pool_with_finished_run().await;
    for (scope, generation) in [
        ("cleanup_pending", "active"),
        ("archived", "cleanup_pending"),
    ] {
        set_states(&pool, scope, generation).await;
        assert!(
            read_reclaim_candidates(&pool).await.unwrap().is_empty(),
            "{scope} scope with {generation} generation",
        );
    }
}

fn row(
    id: &str,
    byte_size: u64,
    availability: ChatScratchDeviceAvailability,
    truncated: bool,
) -> ChatScratchReclaimRow {
    ChatScratchReclaimRow {
        scratch_scope_id: ChatScratchScopeId::new(format!("scope:{id}")).unwrap(),
        scratch_generation_id: ChatScratchGenerationId::new(format!("generation:{id}")).unwrap(),
        reply_thread_id: ChatReplyThreadId::new(format!("reply:{id}")).unwrap(),
        generation: 1,
        agent_name: "Scratch agent".to_string(),
        channel_name: "general".to_string(),
        project_name: "Project".to_string(),
        group_name: None,
        reason: ChatScratchReclaimReason::ScopeArchived,
        byte_size,
        size_truncated: truncated,
        device_availability: availability,
        last_activity_at: UtcTimestamp::new(FIXTURE_NOW).unwrap(),
    }
}

fn row_ids(report: &ChatScratchReclaimReportRead) -> Vec<&str> {
    report
        .rows
        .iter()
        .map(|row| row.scratch_generation_id.as_str())
        .collect()
}

#[test]
fn rows_sort_by_size_and_ties_by_generation() {
    let available = ChatScratchDeviceAvailability::Available;
    let report = build_reclaim_report(
        vec![
            row("small", 10, available, false),
            row("b-large", 500, available, false),
            row("a-large", 500, available, false),
            row("middle", 200, available, false),
        ],
        MAX_RECLAIM_REPORT_ROWS,
    );
    assert_eq!(
        row_ids(&report),
        [
            "generation:a-large",
            "generation:b-large",
            "generation:middle",
            "generation:small"
        ]
    );
    assert_eq!(report.total_bytes, 1_210);
    assert_eq!(report.omitted, 0);
    assert!(!report.total_truncated);
}

#[test]
fn the_row_cap_counts_omitted_rows_and_totals_every_qualifying_row() {
    let available = ChatScratchDeviceAvailability::Available;
    let rows = (0..MAX_RECLAIM_REPORT_ROWS as u64 + 3)
        .map(|index| row(&format!("{index:03}"), index + 1, available, index == 0))
        .collect();
    let report = build_reclaim_report(rows, MAX_RECLAIM_REPORT_ROWS);
    assert_eq!(report.rows.len(), MAX_RECLAIM_REPORT_ROWS);
    assert_eq!(report.omitted, 3);
    let count = MAX_RECLAIM_REPORT_ROWS as u64 + 3;
    assert_eq!(report.total_bytes, count * (count + 1) / 2);
    assert!(
        report.total_truncated,
        "the smallest row is omitted but still counted"
    );
    assert_eq!(report.rows[0].byte_size, count);
}

#[test]
fn unavailable_generations_stay_listed_without_counting_toward_the_total() {
    let report = build_reclaim_report(
        vec![
            row(
                "elsewhere",
                900,
                ChatScratchDeviceAvailability::UnavailableOnThisDevice,
                true,
            ),
            row("here", 100, ChatScratchDeviceAvailability::Available, false),
        ],
        MAX_RECLAIM_REPORT_ROWS,
    );
    assert_eq!(
        row_ids(&report),
        ["generation:elsewhere", "generation:here"]
    );
    assert_eq!(report.total_bytes, 100);
    assert!(!report.total_truncated);
}

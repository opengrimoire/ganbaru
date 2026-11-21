use super::{MAX_REVIEW_QUEUE_ITEMS, MAX_REVIEW_QUEUE_REASON_CHARS, read_review_queue};
use crate::chat::channels::failure_window_start;
use crate::chat::tests::attention::{
    SeedAssignment, days_ago, seed_agent, seed_assignment, seed_channel, seed_project,
    seed_reply_thread, seed_review_update,
};
use crate::chat::tests::repository::pool_with_thread;
use chrono::Utc;
use ganbaru_chat_contracts::models::{ChatReviewQueueKind, ChatReviewQueueRead};

const AGENT: &str = "participant:queue-agent";
const THIS_DEVICE: &str = "device:desk";
/// A window start before every fixture timestamp, so only tests of the window drop failures.
const OPEN_WINDOW: &str = "2026-01-01T00:00:00.000Z";

/// A pool with the queue agent and one channel of the seeded project holding `threads`.
async fn pool_with_channel(threads: &[&str]) -> sqlx::SqlitePool {
    let pool = pool_with_thread().await;
    seed_agent(&pool, AGENT, "Queue agent").await;
    let conversation_id =
        seed_channel(&pool, "project-chat", "channel:queue", "queue", false).await;
    for thread in threads {
        seed_reply_thread(&pool, &conversation_id, thread).await;
    }
    pool
}

async fn queue(pool: &sqlx::SqlitePool) -> ChatReviewQueueRead {
    read_review_queue(pool, THIS_DEVICE, None, OPEN_WINDOW)
        .await
        .unwrap()
}

fn ids(queue: &ChatReviewQueueRead) -> Vec<&str> {
    queue
        .items
        .iter()
        .map(|item| item.assignment_id.as_str())
        .collect()
}

#[tokio::test]
async fn review_queue_orders_by_kind_then_recency() {
    let threads = ["t:1", "t:2", "t:3", "t:4", "t:5", "t:6", "t:7"];
    let pool = pool_with_channel(&threads).await;
    let rows = [
        ("assignment:failure", "t:1", "failed", 50),
        ("assignment:review-old", "t:2", "ready_for_review", 10),
        ("assignment:review-new", "t:3", "ready_for_review", 30),
        ("assignment:approval", "t:4", "waiting_for_approval", 5),
        ("assignment:answer", "t:5", "waiting_for_answer", 1),
        ("assignment:working", "t:6", "working", 60),
        ("assignment:completed", "t:7", "completed", 70),
    ];
    for (id, thread, state, minutes) in rows {
        seed_assignment(
            &pool,
            &SeedAssignment::new(id, thread, AGENT, state, minutes),
        )
        .await;
    }

    let queue = queue(&pool).await;

    assert_eq!(
        ids(&queue),
        [
            "assignment:answer",
            "assignment:approval",
            "assignment:review-new",
            "assignment:review-old",
            "assignment:failure",
        ]
    );
    let kinds = queue.items.iter().map(|item| item.kind).collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            ChatReviewQueueKind::Answer,
            ChatReviewQueueKind::Approval,
            ChatReviewQueueKind::Review,
            ChatReviewQueueKind::Review,
            ChatReviewQueueKind::Failure,
        ]
    );
    assert_eq!(queue.total, 5);
    assert_eq!(queue.failure_count, 1);
    assert!(!queue.truncated);
    let answer = &queue.items[0];
    assert_eq!(answer.project_id, "project-chat");
    assert_eq!(answer.project_name, "Chat");
    assert_eq!(answer.channel_id.as_str(), "channel:queue");
    assert_eq!(answer.channel_name, "queue");
    assert_eq!(answer.reply_thread_id.as_str(), "t:5");
    assert_eq!(answer.agent_participant_id.as_str(), AGENT);
    assert_eq!(answer.agent_display_name, "Queue agent");
    assert_eq!(answer.revision, 1);
}

#[tokio::test]
async fn review_queue_skips_continued_stale_and_delegated_failures() {
    let pool = pool_with_channel(&["t:1", "t:2", "t:3", "t:4", "t:5"]).await;
    let mut continued = SeedAssignment::new("assignment:continued", "t:1", AGENT, "failed", 1);
    continued.settled_at = Some(days_ago(1));
    seed_assignment(&pool, &continued).await;
    let mut successor = SeedAssignment::new("assignment:successor", "t:1", AGENT, "working", 2);
    successor.previous_assignment_id = Some("assignment:continued");
    seed_assignment(&pool, &successor).await;
    let mut stale = SeedAssignment::new("assignment:stale", "t:2", AGENT, "failed", 1);
    stale.settled_at = Some(days_ago(8));
    seed_assignment(&pool, &stale).await;
    let mut recent = SeedAssignment::new("assignment:recent", "t:3", AGENT, "failed", 1);
    recent.settled_at = Some(days_ago(6));
    seed_assignment(&pool, &recent).await;
    seed_assignment(
        &pool,
        &SeedAssignment::new("assignment:parent", "t:4", AGENT, "waiting_for_answer", 1),
    )
    .await;
    let mut child = SeedAssignment::new("assignment:child", "t:5", AGENT, "failed", 1);
    child.settled_at = Some(days_ago(1));
    child.parent_assignment_id = Some("assignment:parent");
    seed_assignment(&pool, &child).await;

    let queue = read_review_queue(&pool, THIS_DEVICE, None, &failure_window_start(Utc::now()))
        .await
        .unwrap();

    assert_eq!(ids(&queue), ["assignment:parent", "assignment:recent"]);
    assert_eq!(queue.total, 2);
}

#[tokio::test]
async fn review_queue_leaves_out_archived_channels_and_projects() {
    let pool = pool_with_channel(&["t:live"]).await;
    seed_assignment(
        &pool,
        &SeedAssignment::new("assignment:live", "t:live", AGENT, "ready_for_review", 1),
    )
    .await;
    let archived_channel =
        seed_channel(&pool, "project-chat", "channel:archived", "archived", true).await;
    seed_reply_thread(&pool, &archived_channel, "t:archived-channel").await;
    seed_assignment(
        &pool,
        &SeedAssignment::new(
            "assignment:archived-channel",
            "t:archived-channel",
            AGENT,
            "ready_for_review",
            2,
        ),
    )
    .await;
    seed_project(&pool, "project-archived", "Archived", false).await;
    let archived_project = seed_channel(
        &pool,
        "project-archived",
        "channel:old-project",
        "old",
        false,
    )
    .await;
    seed_reply_thread(&pool, &archived_project, "t:archived-project").await;
    seed_assignment(
        &pool,
        &SeedAssignment::new(
            "assignment:archived-project",
            "t:archived-project",
            AGENT,
            "ready_for_review",
            3,
        ),
    )
    .await;
    seed_project(&pool, "project-other", "Other", false).await;
    let other = seed_channel(&pool, "project-other", "channel:other", "other", false).await;
    seed_reply_thread(&pool, &other, "t:other").await;
    seed_assignment(
        &pool,
        &SeedAssignment::new("assignment:other", "t:other", AGENT, "ready_for_review", 4),
    )
    .await;
    sqlx::query("UPDATE projects SET status = 'archived' WHERE id = 'project-archived'")
        .execute(&pool)
        .await
        .unwrap();

    let everywhere = queue(&pool).await;
    let one_project = read_review_queue(&pool, THIS_DEVICE, Some("project-chat"), OPEN_WINDOW)
        .await
        .unwrap();

    assert_eq!(ids(&everywhere), ["assignment:other", "assignment:live"]);
    assert_eq!(ids(&one_project), ["assignment:live"]);
    assert_eq!(one_project.total, 1);
}

#[tokio::test]
async fn review_queue_reports_total_and_truncation_past_its_cap() {
    let pool = pool_with_channel(&[]).await;
    let conversation_id = "conversation:channel:queue";
    let count = MAX_REVIEW_QUEUE_ITEMS + 1;
    for index in 0..count {
        let thread = format!("t:{index:03}");
        seed_reply_thread(&pool, conversation_id, &thread).await;
        let id = format!("assignment:{index:03}");
        seed_assignment(
            &pool,
            &SeedAssignment::new(&id, &thread, AGENT, "ready_for_review", i64::from(index)),
        )
        .await;
    }

    let queue = queue(&pool).await;

    assert_eq!(queue.items.len(), 200);
    assert_eq!(queue.total, count);
    assert!(queue.truncated);
    assert_eq!(
        queue.items[0].assignment_id.as_str(),
        "assignment:200",
        "the newest item comes first"
    );
}

#[tokio::test]
async fn review_queue_counts_distinct_changed_files_across_review_updates() {
    let pool = pool_with_channel(&["t:changed", "t:clean"]).await;
    seed_assignment(
        &pool,
        &SeedAssignment::new(
            "assignment:changed",
            "t:changed",
            AGENT,
            "ready_for_review",
            2,
        ),
    )
    .await;
    seed_assignment(
        &pool,
        &SeedAssignment::new("assignment:clean", "t:clean", AGENT, "ready_for_review", 1),
    )
    .await;
    seed_review_update(
        &pool,
        "t:changed",
        "assignment:changed",
        "item:review-1",
        &["src/lib.rs", "README.md"],
    )
    .await;
    seed_review_update(
        &pool,
        "t:changed",
        "assignment:changed",
        "item:review-2",
        &["src/lib.rs", "src/main.rs"],
    )
    .await;

    let queue = queue(&pool).await;

    assert_eq!(ids(&queue), ["assignment:changed", "assignment:clean"]);
    assert_eq!(queue.items[0].changed_file_count, 3);
    assert_eq!(queue.items[1].changed_file_count, 0);
}

#[tokio::test]
async fn review_queue_marks_work_this_device_executed() {
    let pool = pool_with_channel(&["t:here", "t:elsewhere", "t:unassigned"]).await;
    let mut here = SeedAssignment::new("assignment:here", "t:here", AGENT, "ready_for_review", 3);
    here.executing_device_id = Some(THIS_DEVICE);
    seed_assignment(&pool, &here).await;
    let mut elsewhere = SeedAssignment::new(
        "assignment:elsewhere",
        "t:elsewhere",
        AGENT,
        "ready_for_review",
        2,
    );
    elsewhere.executing_device_id = Some("device:laptop");
    seed_assignment(&pool, &elsewhere).await;
    seed_assignment(
        &pool,
        &SeedAssignment::new(
            "assignment:unassigned",
            "t:unassigned",
            AGENT,
            "ready_for_review",
            1,
        ),
    )
    .await;

    let queue = queue(&pool).await;

    let executed = queue
        .items
        .iter()
        .map(|item| (item.assignment_id.as_str(), item.executed_here))
        .collect::<Vec<_>>();
    assert_eq!(
        executed,
        [
            ("assignment:here", true),
            ("assignment:elsewhere", false),
            ("assignment:unassigned", false),
        ]
    );
}

#[tokio::test]
async fn review_queue_clips_reasons_at_a_character_boundary() {
    let pool = pool_with_channel(&["t:long", "t:short"]).await;
    let long_reason = "é".repeat(MAX_REVIEW_QUEUE_REASON_CHARS + 10);
    let mut long = SeedAssignment::new("assignment:long", "t:long", AGENT, "failed", 2);
    long.state_reason = Some(&long_reason);
    seed_assignment(&pool, &long).await;
    let exact_reason = "ü".repeat(MAX_REVIEW_QUEUE_REASON_CHARS);
    let mut short = SeedAssignment::new("assignment:short", "t:short", AGENT, "failed", 1);
    short.state_reason = Some(&exact_reason);
    seed_assignment(&pool, &short).await;

    let queue = queue(&pool).await;

    let clipped = queue.items[0].state_reason.as_deref().unwrap();
    assert_eq!(
        clipped,
        format!("{}…", "é".repeat(MAX_REVIEW_QUEUE_REASON_CHARS))
    );
    assert_eq!(
        queue.items[1].state_reason.as_deref(),
        Some(exact_reason.as_str()),
        "a reason at the limit is kept whole"
    );
}

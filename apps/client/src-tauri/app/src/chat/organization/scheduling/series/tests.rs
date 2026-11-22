use super::*;
use crate::chat::organization::scheduling::{
    apply_scheduled_update, claim_scheduled_message_for_immediate_send, read_scheduled_message,
};
use crate::chat::organization::tests::{
    SCHEDULING_DEVICE, chat_writers_of, insert_test_scheduled_message, message, migrated_pool,
    seed_test_channel,
};
use crate::chat::organization::workflow::PostingDevice;
use crate::chat::organization::{ScheduledSend, persist_post};
use ganbaru_chat::coordination::contracts::{
    ChatScheduledMessageRead, ChatScheduledMessageState, PostChatMessageCommand,
    UpdateScheduledChatMessageCommand,
};

/// Due time of the first occurrence of the test series, its anchor.
const ANCHOR: &str = "2026-08-04T00:00:00.000Z";

fn hourly() -> RecurrenceSpec {
    RecurrenceSpec {
        rule: RecurrenceRule::Interval { minutes: 60 },
        time_zone: "UTC".into(),
        quiet_policy: QuietPolicy::Defer,
    }
}

fn millis(value: &str) -> i64 {
    timestamp_millis(value).unwrap()
}

fn series_due_at(next_occurrence_at: &str) -> StoredSeries {
    StoredSeries {
        spec: hourly(),
        anchor_ms: millis(ANCHOR),
        next_occurrence_at: next_occurrence_at.to_owned(),
        deferred_until_ms: None,
    }
}

fn test_id() -> ChatScheduledMessageId {
    ChatScheduledMessageId::new("scheduled:test").unwrap()
}

/// Stores `scheduled:test` as an hourly series executed here, first due at its anchor.
async fn insert_test_series(pool: &SqlitePool, now: &UtcTimestamp) -> PostChatMessageCommand {
    seed_test_channel(pool, now).await;
    let request = message("Stand up");
    let request_data = serde_json::to_string(&request).unwrap();
    insert_test_scheduled_message(pool, &request_data, Some(SCHEDULING_DEVICE), true, now).await;
    let first_at = first_occurrence_at(&hourly(), &UtcTimestamp::new(ANCHOR).unwrap()).unwrap();
    assert_eq!(first_at, ANCHOR);
    let mut transaction = pool.begin().await.unwrap();
    insert_series(&mut transaction, &test_id(), &hourly(), &first_at, now)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    request
}

async fn claim(pool: &SqlitePool, now: &UtcTimestamp, claim_token: &str) {
    claim_scheduled_message_for_immediate_send(
        pool,
        &test_id(),
        SCHEDULING_DEVICE,
        now,
        claim_token,
    )
    .await
    .unwrap();
}

async fn progress(pool: &SqlitePool) -> (String, i64) {
    sqlx::query_as(
        "SELECT next_occurrence_at, fired_count FROM chat_scheduled_message_recurrences
         WHERE scheduled_message_id = 'scheduled:test'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn dispatch(pool: &SqlitePool) -> (String, i64, Option<String>, i64) {
    sqlx::query_as(
        "SELECT available_at, attempt_count, claim_token, skipped_occurrences
         FROM chat_scheduled_message_dispatch WHERE scheduled_message_id = 'scheduled:test'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn item_count(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn scheduled_occurrence_posts_within_the_grace_and_skips_after_it() {
    let series = series_due_at("2026-08-04T01:00:00.000Z");
    let posted = decide_occurrence(
        "scheduled:test",
        &series,
        millis("2026-08-04T01:10:00.000Z"),
        None,
    )
    .unwrap();
    assert_eq!(
        posted,
        OccurrenceDecision::Post(PendingOccurrence {
            key: occurrence_key("scheduled:test", millis("2026-08-04T01:00:00.000Z")),
            due_at: "2026-08-04T01:00:00.000Z".into(),
            next_at: "2026-08-04T02:00:00.000Z".into(),
        })
    );

    let skipped = decide_occurrence(
        "scheduled:test",
        &series,
        millis("2026-08-04T01:10:00.001Z"),
        None,
    )
    .unwrap();
    assert_eq!(
        skipped,
        OccurrenceDecision::Skip {
            due_at: "2026-08-04T01:00:00.000Z".into(),
            next_at: "2026-08-04T02:00:00.000Z".into(),
        }
    );

    // Several missed occurrences collapse into one skip to the first one still ahead.
    let far_behind = decide_occurrence(
        "scheduled:test",
        &series,
        millis("2026-08-04T03:30:00.000Z"),
        None,
    )
    .unwrap();
    assert_eq!(
        far_behind,
        OccurrenceDecision::Skip {
            due_at: "2026-08-04T01:00:00.000Z".into(),
            next_at: "2026-08-04T04:00:00.000Z".into(),
        }
    );

    let early = decide_occurrence(
        "scheduled:test",
        &series,
        millis("2026-08-04T00:59:59.999Z"),
        None,
    )
    .unwrap();
    assert_eq!(
        early,
        OccurrenceDecision::Wait {
            due_at: "2026-08-04T01:00:00.000Z".into(),
        }
    );
}

#[test]
fn scheduled_occurrence_sent_now_is_the_pending_one_early_or_late() {
    let series = series_due_at("2026-08-04T01:00:00.000Z");
    let early = occurrence_sent_now(
        "scheduled:test",
        &series,
        millis("2026-08-04T00:20:00.000Z"),
    )
    .unwrap();
    assert_eq!(
        early,
        PendingOccurrence {
            key: occurrence_key("scheduled:test", millis("2026-08-04T01:00:00.000Z")),
            due_at: "2026-08-04T01:00:00.000Z".into(),
            next_at: "2026-08-04T02:00:00.000Z".into(),
        }
    );
    let late = occurrence_sent_now(
        "scheduled:test",
        &series,
        millis("2026-08-04T02:30:00.000Z"),
    )
    .unwrap();
    assert_eq!(late.key, early.key);
    assert_eq!(late.next_at, "2026-08-04T03:00:00.000Z");
}

#[test]
fn scheduled_series_posts_each_occurrence_once_and_advances() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-04T00:01:00.000Z").unwrap();
        let request = insert_test_series(&pool, &now).await;
        let id = test_id();
        let writers = chat_writers_of(&[SCHEDULING_DEVICE]);
        let device = PostingDevice::this_device(SCHEDULING_DEVICE, &writers);
        let series = read_series(&pool, &test_id()).await.unwrap().unwrap();

        claim(&pool, &now, "claim:first").await;
        let ClaimedSeries::Post(first) =
            resolve_claimed_series(&pool, &test_id(), "claim:first", &now, &QuietGate::Open)
                .await
                .unwrap()
        else {
            panic!("the first occurrence is due");
        };
        let first_send = ScheduledSend {
            scheduled_message_id: &id,
            claim_token: "claim:first",
            occurrence: Some(first.as_send()),
        };
        let first_post = persist_post(&pool, &request, Some(first_send), device)
            .await
            .unwrap();
        assert_eq!(
            progress(&pool).await,
            ("2026-08-04T01:00:00.000Z".to_owned(), 1)
        );
        assert_eq!(
            dispatch(&pool).await,
            ("2026-08-04T01:00:00.000Z".to_owned(), 0, None, 0)
        );
        let read = read_scheduled_message(&pool, &test_id()).await.unwrap();
        assert_eq!(read.state, ChatScheduledMessageState::Scheduled);
        let recurrence = read.recurrence.unwrap();
        assert_eq!(recurrence.spec, hourly());
        assert_eq!(
            recurrence.next_occurrence_at.as_str(),
            "2026-08-04T01:00:00.000Z"
        );
        assert_eq!(recurrence.fired_count, 1);

        // A repeated send of the posted occurrence answers from its receipt.
        claim(&pool, &now, "claim:repeat").await;
        let repeat_send = ScheduledSend {
            claim_token: "claim:repeat",
            ..first_send
        };
        let repeated = persist_post(&pool, &request, Some(repeat_send), device)
            .await
            .unwrap();
        assert_eq!(repeated.item_id, first_post.item_id);
        assert_eq!(item_count(&pool).await, 1);
        assert_eq!(dispatch(&pool).await.2, None);

        let later = UtcTimestamp::new("2026-08-04T01:02:00.000Z").unwrap();
        claim(&pool, &later, "claim:second").await;
        let second = occurrence_sent_now(
            "scheduled:test",
            &StoredSeries {
                next_occurrence_at: "2026-08-04T01:00:00.000Z".into(),
                ..series
            },
            millis(later.as_str()),
        )
        .unwrap();
        let second_post = persist_post(
            &pool,
            &request,
            Some(ScheduledSend {
                scheduled_message_id: &id,
                claim_token: "claim:second",
                occurrence: Some(second.as_send()),
            }),
            device,
        )
        .await
        .unwrap();
        assert_ne!(second_post.item_id, first_post.item_id);
        assert_eq!(item_count(&pool).await, 2);
        assert_eq!(
            progress(&pool).await,
            ("2026-08-04T02:00:00.000Z".to_owned(), 2)
        );
        let messages: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(messages, 1);
    });
}

#[test]
fn scheduled_series_rejects_an_occurrence_it_already_moved_past() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-04T00:01:00.000Z").unwrap();
        let request = insert_test_series(&pool, &now).await;
        let id = test_id();
        let writers = chat_writers_of(&[SCHEDULING_DEVICE]);
        let device = PostingDevice::this_device(SCHEDULING_DEVICE, &writers);
        claim(&pool, &now, "claim:stale").await;
        let stale = PendingOccurrence {
            key: occurrence_key("scheduled:test", millis("2026-08-03T23:00:00.000Z")),
            due_at: "2026-08-03T23:00:00.000Z".into(),
            next_at: ANCHOR.into(),
        };

        let error = persist_post(
            &pool,
            &request,
            Some(ScheduledSend {
                scheduled_message_id: &id,
                claim_token: "claim:stale",
                occurrence: Some(stale.as_send()),
            }),
            device,
        )
        .await
        .err()
        .expect("a stale occurrence is refused");

        assert_eq!(error.code, ChatErrorCode::Conflict);
        assert_eq!(item_count(&pool).await, 0);
        assert_eq!(progress(&pool).await, (ANCHOR.to_owned(), 0));
    });
}

#[test]
fn scheduled_series_skips_a_missed_occurrence_without_posting() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let created = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        insert_test_series(&pool, &created).await;
        let now = UtcTimestamp::new("2026-08-04T02:30:00.000Z").unwrap();
        claim(&pool, &now, "claim:late").await;

        let resolved =
            resolve_claimed_series(&pool, &test_id(), "claim:late", &now, &QuietGate::Open)
                .await
                .unwrap();

        assert!(matches!(resolved, ClaimedSeries::Settled));
        assert_eq!(item_count(&pool).await, 0);
        assert_eq!(
            progress(&pool).await,
            ("2026-08-04T03:00:00.000Z".to_owned(), 0)
        );
        assert_eq!(
            dispatch(&pool).await,
            ("2026-08-04T03:00:00.000Z".to_owned(), 0, None, 1)
        );
        let recurrence = read_scheduled_message(&pool, &test_id())
            .await
            .unwrap()
            .recurrence
            .unwrap();
        assert_eq!(recurrence.skipped_occurrences, 1);
    });
}

#[test]
fn scheduled_series_waits_for_an_occurrence_that_is_not_due() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        insert_test_series(&pool, &now).await;
        claim(&pool, &now, "claim:early").await;

        let resolved =
            resolve_claimed_series(&pool, &test_id(), "claim:early", &now, &QuietGate::Open)
                .await
                .unwrap();

        assert!(matches!(resolved, ClaimedSeries::Settled));
        assert_eq!(progress(&pool).await, (ANCHOR.to_owned(), 0));
        assert_eq!(dispatch(&pool).await, (ANCHOR.to_owned(), 0, None, 0));
    });
}

#[test]
fn scheduled_series_are_limited_per_channel() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let channel_id = ChatChannelId::new("channel:test").unwrap();
        for index in 0..MAX_RECURRING_SCHEDULES_PER_CHANNEL {
            let id = ChatScheduledMessageId::new(format!("scheduled:{index}")).unwrap();
            sqlx::query(
                "INSERT INTO chat_scheduled_messages
                    (id, client_command_id, channel_id, request_data, scheduled_for, created_at,
                     updated_at)
                 VALUES (?1, ?2, 'channel:test', '{}', ?3, ?4, ?4)",
            )
            .bind(id.as_str())
            .bind(format!("command:{index}"))
            .bind(ANCHOR)
            .bind(now.as_str())
            .execute(&pool)
            .await
            .unwrap();
            let mut transaction = pool.begin().await.unwrap();
            require_series_room(&mut transaction, &channel_id)
                .await
                .unwrap();
            insert_series(&mut transaction, &id, &hourly(), ANCHOR, &now)
                .await
                .unwrap();
            transaction.commit().await.unwrap();
        }

        let mut transaction = pool.begin().await.unwrap();
        let error = require_series_room(&mut transaction, &channel_id)
            .await
            .unwrap_err();
        assert_eq!(error.field.as_deref(), Some("recurrence"));
    });
}

async fn deferred_until(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar(
        "SELECT deferred_until FROM chat_scheduled_message_dispatch
         WHERE scheduled_message_id = 'scheduled:test'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn scheduled_quiet_period_defers_or_skips_an_occurrence_by_policy() {
    let series = series_due_at("2026-08-04T01:00:00.000Z");
    let now = millis("2026-08-04T01:00:00.000Z");
    assert_eq!(
        decide_occurrence(
            "scheduled:test",
            &series,
            now,
            Some(millis("2026-08-04T01:40:00.000Z")),
        )
        .unwrap(),
        OccurrenceDecision::Defer {
            due_at: "2026-08-04T01:00:00.000Z".into(),
            until_ms: millis("2026-08-04T01:40:00.000Z"),
        }
    );

    // A quiet period lasting until the following occurrence skips this one and counts it.
    assert_eq!(
        decide_occurrence(
            "scheduled:test",
            &series,
            now,
            Some(millis("2026-08-04T02:00:00.000Z")),
        )
        .unwrap(),
        OccurrenceDecision::Skip {
            due_at: "2026-08-04T01:00:00.000Z".into(),
            next_at: "2026-08-04T02:00:00.000Z".into(),
        }
    );

    let ignoring = StoredSeries {
        spec: RecurrenceSpec {
            quiet_policy: QuietPolicy::Ignore,
            ..hourly()
        },
        ..series.clone()
    };
    assert!(matches!(
        decide_occurrence(
            "scheduled:test",
            &ignoring,
            now,
            Some(millis("2026-08-04T01:40:00.000Z")),
        )
        .unwrap(),
        OccurrenceDecision::Post(_)
    ));

    // A held occurrence counts its grace from the end of the quiet period it waited for.
    let held = StoredSeries {
        deferred_until_ms: Some(millis("2026-08-04T01:40:00.000Z")),
        ..series.clone()
    };
    assert!(matches!(
        decide_occurrence(
            "scheduled:test",
            &held,
            millis("2026-08-04T01:45:00.000Z"),
            None,
        )
        .unwrap(),
        OccurrenceDecision::Post(_)
    ));
    assert!(matches!(
        decide_occurrence(
            "scheduled:test",
            &series,
            millis("2026-08-04T01:45:00.000Z"),
            None,
        )
        .unwrap(),
        OccurrenceDecision::Skip { .. }
    ));
}

#[test]
fn scheduled_quiet_hold_posts_when_the_quiet_period_ends() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-04T00:01:00.000Z").unwrap();
        insert_test_series(&pool, &now).await;
        claim(&pool, &now, "claim:quiet").await;
        let quiet = QuietGate::Quiet {
            until_ms: millis("2026-08-04T00:40:00.000Z"),
        };

        let resolved = resolve_claimed_series(&pool, &test_id(), "claim:quiet", &now, &quiet)
            .await
            .unwrap();

        assert!(matches!(resolved, ClaimedSeries::Settled));
        assert_eq!(item_count(&pool).await, 0);
        assert_eq!(progress(&pool).await, (ANCHOR.to_owned(), 0));
        assert_eq!(
            dispatch(&pool).await,
            ("2026-08-04T00:40:00.000Z".to_owned(), 0, None, 0)
        );
        let recurrence = read_scheduled_message(&pool, &test_id())
            .await
            .unwrap()
            .recurrence
            .unwrap();
        assert_eq!(
            recurrence.deferred_until.unwrap().as_str(),
            "2026-08-04T00:40:00.000Z"
        );

        // A quiet period that lasts longer leaves the hold as it is.
        let later_quiet = QuietGate::Quiet {
            until_ms: millis("2026-08-04T00:50:00.000Z"),
        };
        release_stale_quiet_holds(&pool, &later_quiet, &now)
            .await
            .unwrap();
        assert_eq!(dispatch(&pool).await.0, "2026-08-04T00:40:00.000Z");

        // The quiet period ended early, as when a Focus phase stops: the hold is due now.
        let open_at = UtcTimestamp::new("2026-08-04T00:20:00.000Z").unwrap();
        release_stale_quiet_holds(&pool, &QuietGate::Open, &open_at)
            .await
            .unwrap();
        assert_eq!(dispatch(&pool).await.0, "2026-08-04T00:20:00.000Z");
        assert_eq!(
            deferred_until(&pool).await.as_deref(),
            Some("2026-08-04T00:20:00.000Z")
        );

        // Past the grace from its due time, the held occurrence still posts.
        let end = UtcTimestamp::new("2026-08-04T00:25:00.000Z").unwrap();
        claim(&pool, &end, "claim:end").await;
        let ClaimedSeries::Post(occurrence) =
            resolve_claimed_series(&pool, &test_id(), "claim:end", &end, &QuietGate::Open)
                .await
                .unwrap()
        else {
            panic!("the held occurrence posts once the quiet period ends");
        };
        assert_eq!(occurrence.due_at, ANCHOR);
        assert_eq!(occurrence.next_at, "2026-08-04T01:00:00.000Z");
    });
}

#[test]
fn scheduled_quiet_state_unavailable_fails_only_a_deferring_attempt() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-04T00:01:00.000Z").unwrap();
        insert_test_series(&pool, &now).await;
        claim(&pool, &now, "claim:unavailable").await;
        let gate = QuietGate::Unavailable(ChatError::new(
            ChatErrorCode::Internal,
            "no local time",
            true,
        ));

        let resolved = resolve_claimed_series(&pool, &test_id(), "claim:unavailable", &now, &gate)
            .await
            .unwrap();

        let ClaimedSeries::Failed(error) = resolved else {
            panic!("a series that defers cannot post without the quiet state");
        };
        assert_eq!(error.message, "no local time");
        assert_eq!(progress(&pool).await, (ANCHOR.to_owned(), 0));
        assert_eq!(
            dispatch(&pool).await.2.as_deref(),
            Some("claim:unavailable")
        );

        sqlx::query(
            "UPDATE chat_scheduled_message_recurrences SET quiet_policy = 'ignore'
             WHERE scheduled_message_id = 'scheduled:test'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(matches!(
            resolve_claimed_series(&pool, &test_id(), "claim:unavailable", &now, &gate)
                .await
                .unwrap(),
            ClaimedSeries::Post(_)
        ));
    });
}

#[test]
fn scheduled_quiet_period_without_a_known_end_holds_for_one_retry_interval() {
    use crate::chat::quiet_periods::QuietReading;
    use ganbaru_chat::scheduling::{QuietPeriodState, QuietReason};
    let now = millis("2026-08-04T00:00:00.000Z");
    let quiet = |ends_at_ms| QuietReading {
        state: QuietPeriodState::Quiet {
            reason: QuietReason::Focus,
            ends_local: None,
        },
        ends_at_ms,
    };
    assert_eq!(
        super::super::quiet_gate_of(quiet(None), now).until_ms(),
        Some(now + super::super::SCHEDULE_RETRY_SECONDS * 1000)
    );
    assert_eq!(
        super::super::quiet_gate_of(quiet(Some(now + 5_000)), now).until_ms(),
        Some(now + 5_000)
    );
    let open = QuietReading {
        state: QuietPeriodState::Open,
        ends_at_ms: None,
    };
    assert_eq!(super::super::quiet_gate_of(open, now).until_ms(), None);
}

/// An edit of the test series to `markdown`, from `scheduled_for` on `recurrence`.
fn series_edit(
    markdown: &str,
    scheduled_for: &str,
    recurrence: Option<RecurrenceSpec>,
) -> UpdateScheduledChatMessageCommand {
    let content = message(markdown);
    UpdateScheduledChatMessageCommand {
        scheduled_message_id: test_id(),
        scheduled_for: UtcTimestamp::new(scheduled_for).unwrap(),
        normalized_markdown: content.normalized_markdown,
        rich_content: content.rich_content,
        attachment_ids: Vec::new(),
        references: Vec::new(),
        also_send_to_channel: false,
        recurrence,
    }
}

async fn edit_series(
    pool: &SqlitePool,
    now: &UtcTimestamp,
    edit: UpdateScheduledChatMessageCommand,
) -> ChatScheduledMessageRead {
    apply_scheduled_update(pool, edit, now).await.unwrap()
}

async fn recurrence_rows(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_scheduled_message_recurrences")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn editing_a_series_keeps_its_progress_unless_its_timing_changes() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        insert_test_series(&pool, &now).await;
        sqlx::query("UPDATE chat_scheduled_message_recurrences SET fired_count = 2")
            .execute(&pool)
            .await
            .unwrap();

        // New wording on the same rule and pending occurrence leaves the series where it was.
        let reworded = edit_series(
            &pool,
            &now,
            series_edit("Stand up now", ANCHOR, Some(hourly())),
        )
        .await;
        assert_eq!(reworded.normalized_markdown, "Stand up now");
        assert_eq!(progress(&pool).await, (ANCHOR.to_owned(), 2));
        assert_eq!(dispatch(&pool).await, (ANCHOR.to_owned(), 0, None, 0));

        // A new rule and start move the pending occurrence and keep the posted count.
        let every_two_hours = RecurrenceSpec {
            rule: RecurrenceRule::Interval { minutes: 120 },
            ..hourly()
        };
        let moved = edit_series(
            &pool,
            &now,
            series_edit(
                "Stand up now",
                "2026-08-06T00:00:00.000Z",
                Some(every_two_hours.clone()),
            ),
        )
        .await;
        let recurrence = moved.recurrence.unwrap();
        assert_eq!(recurrence.spec, every_two_hours);
        assert_eq!(
            recurrence.next_occurrence_at.as_str(),
            "2026-08-06T00:00:00.000Z"
        );
        assert_eq!(recurrence.fired_count, 2);
        assert_eq!(
            dispatch(&pool).await,
            ("2026-08-06T00:00:00.000Z".to_owned(), 0, None, 0)
        );

        // Without a rule the message is sent once at the given time.
        let once = edit_series(
            &pool,
            &now,
            series_edit("Stand up once", "2026-08-07T00:00:00.000Z", None),
        )
        .await;
        assert_eq!(once.recurrence, None);
        assert_eq!(once.scheduled_for.as_str(), "2026-08-07T00:00:00.000Z");
        assert_eq!(recurrence_rows(&pool).await, 0);
        assert_eq!(
            dispatch(&pool).await.0,
            "2026-08-07T00:00:00.000Z".to_owned()
        );

        // A rule added to a one-time message starts a series from its first occurrence.
        let repeating = edit_series(
            &pool,
            &now,
            series_edit("Stand up again", "2026-08-08T00:00:00.000Z", Some(hourly())),
        )
        .await;
        let recurrence = repeating.recurrence.unwrap();
        assert_eq!(
            recurrence.next_occurrence_at.as_str(),
            "2026-08-08T00:00:00.000Z"
        );
        assert_eq!(recurrence.fired_count, 0);
        assert_eq!(
            dispatch(&pool).await.0,
            "2026-08-08T00:00:00.000Z".to_owned()
        );
    });
}

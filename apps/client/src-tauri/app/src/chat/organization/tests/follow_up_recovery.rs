//! Follow-up recovery: replies to finished work carried into follow-up assignments after a restart.

use super::*;

/// Writes the seeded inputs of the review thread again under the ids that derive from their
/// messages, since input ids are immutable keys, and returns those ids.
async fn rekey_review_inputs_by_message(pool: &SqlitePool) -> Vec<String> {
    let mut input_ids = Vec::new();
    for message in [
        "item:review-root",
        "item:review-reply-1",
        "item:review-reply-2",
    ] {
        let id = super::common::assignment_input_id(message).unwrap();
        let seeded = sqlx::query(
            "SELECT assignment_id, ordinal, routing_kind, delivery_state, created_at,
                    delivered_at
             FROM chat_work_assignment_inputs WHERE message_item_id = ?",
        )
        .bind(message)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM chat_work_assignment_inputs WHERE message_item_id = ?")
            .bind(message)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO chat_work_assignment_inputs
                (id, assignment_id, message_item_id, ordinal, routing_kind,
                 delivery_state, created_at, delivered_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(seeded.get::<String, _>("assignment_id"))
        .bind(message)
        .bind(seeded.get::<i64, _>("ordinal"))
        .bind(seeded.get::<String, _>("routing_kind"))
        .bind(seeded.get::<String, _>("delivery_state"))
        .bind(seeded.get::<String, _>("created_at"))
        .bind(seeded.get::<Option<String>, _>("delivered_at"))
        .execute(pool)
        .await
        .unwrap();
        input_ids.push(id);
    }
    input_ids
}

#[test]
fn restart_rehomes_review_replies_into_one_ordered_follow_up_assignment() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let input_ids = rekey_review_inputs_by_message(&pool).await;

        let desk_writers = chat_writers_of(&["device:desk"]);
        let scope = scope_accepting(&pool, "assignment:review").await;
        let elsewhere = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:phone", &desk_writers),
            &scope,
            &now,
        )
        .await
        .unwrap();
        assert_eq!(elsewhere, 0, "only the executing device rehomes replies");
        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &desk_writers),
            &scope,
            &now,
        )
        .await
        .unwrap();

        assert_eq!(recovered, 1);
        let all_inputs: Vec<String> =
            sqlx::query_scalar("SELECT id FROM chat_work_assignment_inputs ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        let mut expected_ids = input_ids.clone();
        expected_ids.sort();
        assert_eq!(
            all_inputs, expected_ids,
            "inputs move instead of being replaced"
        );
        let old_state: String = sqlx::query_scalar(
            "SELECT state FROM chat_work_assignments WHERE id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(old_state, "completed");
        let follow_up = sqlx::query(
            "SELECT id, triggering_message_item_id, previous_assignment_id, state,
                    executing_device_id
             FROM chat_work_assignments WHERE previous_assignment_id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let follow_up_id = follow_up.get::<String, _>("id");
        assert_eq!(
            follow_up.get::<String, _>("executing_device_id"),
            "device:desk"
        );
        assert_eq!(
            follow_up.get::<String, _>("triggering_message_item_id"),
            "item:review-reply-1"
        );
        assert_eq!(follow_up.get::<String, _>("state"), "queued");
        let inputs = sqlx::query(
            "SELECT message_item_id, ordinal, routing_kind, delivery_state
             FROM chat_work_assignment_inputs WHERE assignment_id = ? ORDER BY ordinal",
        )
        .bind(&follow_up_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            inputs[0].get::<String, _>("message_item_id"),
            "item:review-reply-1"
        );
        assert_eq!(inputs[0].get::<String, _>("routing_kind"), "follow_up");
        assert_eq!(
            inputs[1].get::<String, _>("message_item_id"),
            "item:review-reply-2"
        );
        assert_eq!(inputs[1].get::<String, _>("routing_kind"), "steer");
        assert!(
            inputs
                .iter()
                .all(|input| input.get::<String, _>("delivery_state") == "pending")
        );
        let queued_job: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_assignment_dispatch_jobs
             WHERE assignment_id = ? AND state = 'queued'",
        )
        .bind(follow_up_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(queued_job, 1);
    });
}

#[test]
fn unmentioned_replies_continue_only_with_a_agent_that_still_participates() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let conversation_id = ChatConversationId::new("conversation:review").unwrap();
        let reply_thread_id = ChatReplyThreadId::new("reply-thread:review").unwrap();

        let invocation =
            resolve_invoked_agent(&pool, &conversation_id, Some(&reply_thread_id), &[])
                .await
                .unwrap()
                .expect("a reply in an assignment thread continues with its agent");
        assert_eq!(invocation.agent_id.as_str(), "participant:review-agent");

        sqlx::query(
            "UPDATE chat_conversation_memberships SET removed_at = '2026-08-04T17:03:00.000Z'
             WHERE participant_id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let error = resolve_invoked_agent(&pool, &conversation_id, Some(&reply_thread_id), &[])
            .await
            .expect_err("a removed agent is not re-invoked by an unmentioned reply");
        assert_eq!(error.code, ChatErrorCode::Conflict);
    });
}

#[test]
fn follow_up_recovery_skips_unaccepted_work() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);
        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &writers),
            &ChatDeviceScope::default(),
            &UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(recovered, 0);
        let assignments: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_work_assignments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(assignments, 1, "no queued assignment names this device");
    });
}

#[test]
fn follow_up_recovery_replays_replies_while_the_recorded_acceptance_matches() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_settled_dispatch_job(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);
        let scope = scope_accepting(&pool, "assignment:review").await;

        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &writers),
            &scope,
            &UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(
            recovered, 1,
            "the gesture that queued this work accepted it"
        );
        assert_eq!(review_follow_ups(&pool).await, 1);
    });
}

#[test]
fn follow_up_recovery_stops_when_access_widens_after_the_work_ran_here() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_settled_dispatch_job(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();
        let accepted_before = scope_accepting(&pool, "assignment:review").await;
        widen_review_channel_access(&pool).await;

        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            device,
            &accepted_before,
            &now,
        )
        .await
        .unwrap();

        assert_eq!(recovered, 0);
        let assignments: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_work_assignments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(assignments, 1, "no successor starts under the wider access");
        let jobs: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_assignment_dispatch_jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(jobs, 1, "only the settled job remains");
        assert_eq!(pending_review_inputs(&pool).await, 2, "the replies stay");
        assert!(
            assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &accepted_before,
            )
            .await
            .unwrap(),
            "the thread offers a local action"
        );

        let accepted_now = scope_accepting(&pool, "assignment:review").await;
        assert!(
            !assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &accepted_now,
            )
            .await
            .unwrap(),
            "accepting the wider access clears the wait"
        );
        let recovered =
            assignments::recover_stranded_follow_up_assignments(&pool, device, &accepted_now, &now)
                .await
                .unwrap();
        assert_eq!(recovered, 1, "the replies recover after the user accepts");
        assert_eq!(review_follow_ups(&pool).await, 1);
    });
}

#[test]
fn unaccepted_follow_ups_do_not_block_accepted_ones() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let scope = scope_accepting(&pool, "assignment:review").await;
        insert_unaccepted_review_assignments(&pool, 40).await;
        let writers = chat_writers_of(&["device:desk"]);

        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &writers),
            &scope,
            &UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(recovered, 1);
    });
}

#[test]
fn recovery_skips_a_row_it_cannot_freeze() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let scope = scope_accepting(&pool, "assignment:review").await;
        insert_review_assignment_with_outside_reply(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);

        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &writers),
            &scope,
            &UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap(),
        )
        .await
        .expect("a row that cannot freeze its context does not stop recovery");

        assert_eq!(recovered, 1, "the rows after it still recover");
        assert_eq!(review_follow_ups(&pool).await, 1);
        let (state, pending): (String, i64) = sqlx::query_as(
            "SELECT assignment.state,
                    (SELECT count(*) FROM chat_work_assignment_inputs input
                     WHERE input.assignment_id = assignment.id
                       AND input.delivery_state = 'pending')
             FROM chat_work_assignments assignment WHERE assignment.id = 'assignment:a-outside'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "ready_for_review", "the refused row stays as it was");
        assert_eq!(pending, 1, "its reply stays pending");
    });
}

#[test]
fn follow_up_recovery_stays_in_the_thread() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        rekey_review_inputs_by_message(&pool).await;
        let scope = scope_accepting(&pool, "assignment:review").await;
        insert_pending_review_input_from_outside_the_thread(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);

        let recovered = assignments::recover_stranded_follow_up_assignments(
            &pool,
            PostingDevice::this_device("device:desk", &writers),
            &scope,
            &UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap(),
        )
        .await
        .unwrap();

        assert_eq!(recovered, 1, "the replies inside the thread recover");
        let (follow_up_id, trigger): (String, String) = sqlx::query_as(
            "SELECT id, triggering_message_item_id FROM chat_work_assignments
             WHERE previous_assignment_id = 'assignment:review'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            trigger, "item:review-reply-1",
            "a message outside the thread never triggers the follow-up"
        );
        assert_eq!(
            assignment_input_messages(&pool, &follow_up_id).await,
            ["item:review-reply-1", "item:review-reply-2"],
            "only replies of the thread move onto the follow-up"
        );
        assert_eq!(
            assignment_input_messages(&pool, "assignment:review").await,
            ["item:review-root", "item:review-elsewhere"],
            "the outside message stays with the settled assignment"
        );
    });
}

#[test]
fn a_local_post_carries_pending_review_replies_forward() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let writers = chat_writers_of(&["device:desk"]);

        let posted = persist_post(
            &pool,
            &review_reply_mention("command:review-follow-up"),
            None,
            PostingDevice::this_device("device:desk", &writers),
        )
        .await
        .unwrap();

        let follow_up_id = posted.assignment_write.assignment_id.unwrap();
        let (previous, state): (Option<String>, String) = sqlx::query_as(
            "SELECT follow_up.previous_assignment_id, previous.state
             FROM chat_work_assignments follow_up
             JOIN chat_work_assignments previous ON previous.id = follow_up.previous_assignment_id
             WHERE follow_up.id = ?",
        )
        .bind(follow_up_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(previous.as_deref(), Some("assignment:review"));
        assert_eq!(state, "completed");
        assert_eq!(
            pending_review_inputs(&pool).await,
            0,
            "no reply is stranded on the settled assignment"
        );
        let inputs = sqlx::query(
            "SELECT message_item_id, routing_kind, delivery_state
             FROM chat_work_assignment_inputs WHERE assignment_id = ? ORDER BY ordinal, id",
        )
        .bind(follow_up_id.as_str())
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(inputs.len(), 3, "the replies follow the new post");
        assert_eq!(inputs[0].get::<String, _>("routing_kind"), "follow_up");
        assert_eq!(
            inputs[1..]
                .iter()
                .map(|input| input.get::<String, _>("message_item_id"))
                .collect::<Vec<_>>(),
            ["item:review-reply-1", "item:review-reply-2"]
        );
        assert!(inputs[1..].iter().all(|input| {
            input.get::<String, _>("routing_kind") == "steer"
                && input.get::<String, _>("delivery_state") == "pending"
        }));
    });
}

/// Message ids of the inputs of `assignment_id`, in delivery order.
async fn assignment_input_messages(pool: &SqlitePool, assignment_id: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT message_item_id FROM chat_work_assignment_inputs
         WHERE assignment_id = ? ORDER BY ordinal, id",
    )
    .bind(assignment_id)
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Queues on `assignment:review`, ahead of its replies, a pending input whose message is a
/// top-level post of the same conversation, as a synced input row can name.
async fn insert_pending_review_input_from_outside_the_thread(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:review-elsewhere', 'conversation:review', 'message', 'a2',
                 '2026-08-04T17:00:30.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:review-elsewhere', 'participant:local-owner', 'You',
                 '2026-08-04T17:00:30.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:review-elsewhere', 'item:review-elsewhere', 'Run something else',
                 '2026-08-04T17:00:30.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:review-elsewhere'
         WHERE item_id = 'item:review-elsewhere';
         UPDATE chat_work_assignment_inputs SET ordinal = ordinal + 1
         WHERE assignment_id = 'assignment:review' AND ordinal >= 2;
         INSERT INTO chat_work_assignment_inputs
             (id, assignment_id, message_item_id, ordinal, routing_kind,
              delivery_state, created_at, delivered_at)
         VALUES ('input:review-elsewhere', 'assignment:review', 'item:review-elsewhere', 2,
                 'queued_continuation', 'pending', '2026-08-04T17:00:30.000Z', NULL);",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Inserts a review assignment sorted before `assignment:review`, accepted under the same
/// subject, whose pending reply was written by a participant outside the conversation, so its
/// follow-up cannot freeze a context.
async fn insert_review_assignment_with_outside_reply(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO chat_participants
             (id, participant_kind, display_name, created_at, updated_at)
         VALUES ('participant:outside', 'human', 'Outside', '2026-08-04T17:00:00.000Z',
                 '2026-08-04T17:00:00.000Z');
         INSERT INTO chat_conversation_items
             (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
         VALUES ('item:outside-reply', 'conversation:review', 'reply-thread:review', 'message',
                 'a0', '2026-08-04T17:00:30.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:outside-reply', 'participant:outside', 'Outside',
                 '2026-08-04T17:00:30.000Z');
         INSERT INTO chat_communication_message_revisions
             (id, message_item_id, normalized_markdown, created_at)
         VALUES ('revision:outside-reply', 'item:outside-reply', 'Run this',
                 '2026-08-04T17:00:30.000Z');
         UPDATE chat_communication_messages
         SET current_revision_id = 'revision:outside-reply'
         WHERE item_id = 'item:outside-reply';
         INSERT INTO chat_work_assignments
             (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
              executing_device_id, state, created_at, updated_at)
         VALUES ('assignment:a-outside', 'reply-thread:review', 'participant:review-agent',
                 'item:review-root', 1, json_object('providerManagedModel', json('true')), 'device:desk', 'ready_for_review',
                 '2026-08-04T17:00:00.000Z', '2026-08-04T17:02:00.000Z');
         INSERT INTO chat_work_assignment_inputs
             (id, assignment_id, message_item_id, ordinal, routing_kind,
              delivery_state, created_at, delivered_at)
         VALUES ('input:outside-reply', 'assignment:a-outside', 'item:outside-reply', 2,
                 'queued_continuation', 'pending', '2026-08-04T17:00:30.000Z', NULL);",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Records the settled dispatch job of `assignment:review`, as work this device ran leaves.
async fn seed_settled_dispatch_job(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO chat_assignment_dispatch_jobs
             (id, assignment_id, state, available_at, created_at, updated_at)
         VALUES ('dispatch:review', 'assignment:review', 'completed',
                 '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z',
                 '2026-08-04T17:00:00.000Z')",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Number of pending inputs still on `assignment:review`.
async fn pending_review_inputs(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM chat_work_assignment_inputs
         WHERE assignment_id = 'assignment:review' AND delivery_state = 'pending'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Widens the review channel to own, as a synced settings edit does.
async fn widen_review_channel_access(pool: &SqlitePool) {
    sqlx::query("UPDATE chat_channels SET agent_access = 'own' WHERE conversation_id = 'conversation:review'")
        .execute(pool)
        .await
        .unwrap();
}

/// Inserts `count` review assignments with a pending reply, sorted before `assignment:review`,
/// whose trigger sits outside their thread, so no acceptance covers them.
async fn insert_unaccepted_review_assignments(pool: &SqlitePool, count: u32) {
    sqlx::raw_sql(
        "INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES ('item:foreign', 'conversation:review', 'message', 'a9',
                 '2026-08-04T17:00:30.000Z');
         INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES ('item:foreign', 'participant:local-owner', 'You', '2026-08-04T17:00:30.000Z')",
    )
    .execute(pool)
    .await
    .unwrap();
    for index in 0..count {
        let assignment_id = format!("assignment:a-{index:02}");
        sqlx::query(
            "INSERT INTO chat_work_assignments
                 (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
                  executing_device_id, state, created_at, updated_at)
             VALUES (?1, 'reply-thread:review', 'participant:review-agent', 'item:foreign', 1, json_object('providerManagedModel', json('true')),
                     'device:desk', 'ready_for_review',
                     '2026-08-04T17:00:00.000Z', '2026-08-04T17:02:00.000Z')",
        )
        .bind(&assignment_id)
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_work_assignment_inputs
                 (id, assignment_id, message_item_id, ordinal, routing_kind,
                  delivery_state, created_at, delivered_at)
             VALUES (?1, ?2, 'item:foreign', 1, 'queued_continuation', 'pending',
                     '2026-08-04T17:01:00.000Z', NULL)",
        )
        .bind(format!("input:a-{index:02}"))
        .bind(&assignment_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

//! Which device executes work: routing new assignments, takeover of stranded work, and dispatch claims.

use super::*;

/// Routes the review reply `item:review-reply-1` to the review agent as `device` posts it.
async fn route_review_reply(
    pool: &SqlitePool,
    device: super::workflow::PostingDevice<'_>,
) -> super::workflow::AssignmentWrite {
    let invocation = super::workflow::ResolvedInvocation {
        agent_id: ChatParticipantId::new("participant:review-agent").unwrap(),
        active_assignment: None,
        latest_assignment: None,
    };
    let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
    let mut transaction = pool.begin().await.unwrap();
    let write = super::workflow::persist_assignment_routing(
        &mut transaction,
        &ChatReplyThreadId::new("reply-thread:review").unwrap(),
        &ChatConversationItemId::new("item:review-reply-1").unwrap(),
        Some(&invocation),
        super::workflow::PostRoutingChoices::default(),
        device,
        &now,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    write
}

/// The executing device and dispatch job count of the assignment the review reply triggers.
async fn routed_assignment(pool: &SqlitePool) -> Option<(Option<String>, i64)> {
    sqlx::query(
        "SELECT assignment.executing_device_id,
                (SELECT count(*) FROM chat_assignment_dispatch_jobs job
                 WHERE job.assignment_id = assignment.id) AS dispatch_jobs
         FROM chat_work_assignments assignment
         WHERE assignment.triggering_message_item_id = 'item:review-reply-1'",
    )
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|row| {
        (
            row.get::<Option<String>, _>("executing_device_id"),
            row.get::<i64, _>("dispatch_jobs"),
        )
    })
}

#[test]
fn a_new_assignment_runs_on_the_posting_desktop_only_when_its_provider_is_ready_there() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:desk", "device:laptop"]);
        let phone = super::workflow::PostingDevice {
            id: "device:phone",
            can_execute: false,
            chat_writers: &writers,
        };
        let desk = super::workflow::PostingDevice {
            id: "device:desk",
            can_execute: true,
            chat_writers: &writers,
        };

        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(&pool, phone).await;
        assert!(
            write.assignment_id.is_none(),
            "Android records the mention only"
        );
        assert!(!write.executes_here);
        assert_eq!(routed_assignment(&pool).await, None);

        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(&pool, desk).await;
        assert!(write.executes_here);
        assert_eq!(
            routed_assignment(&pool).await,
            Some((Some("device:desk".to_owned()), 1))
        );

        let unready = super::workflow::ChatWriters::new(["device:desk".to_owned()]);
        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(
            &pool,
            super::workflow::PostingDevice {
                chat_writers: &unready,
                ..desk
            },
        )
        .await;
        assert!(
            write.assignment_id.is_none(),
            "a desktop without the provider ready records the mention for Start here"
        );
        assert_eq!(routed_assignment(&pool).await, None);

        let other_writers = chat_writers_of(&["device:laptop"]);
        let pool = review_thread_without_assignment().await;
        let write = route_review_reply(
            &pool,
            super::workflow::PostingDevice {
                chat_writers: &other_writers,
                ..desk
            },
        )
        .await;
        assert!(
            write.assignment_id.is_none(),
            "a desktop that cannot write Chat runs no new work"
        );
    });
}

#[test]
fn the_new_owner_takes_over_unstarted_work_queued_on_the_old_owner() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let phone = super::workflow::PostingDevice {
            id: "device:phone",
            can_execute: false,
            chat_writers: &writers,
        };
        assert_eq!(
            assignments::take_over_stranded_assignments(&pool, phone, &now)
                .await
                .unwrap(),
            0,
            "only a device that runs work takes it over"
        );
        let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
        let taken = assignments::take_over_stranded_assignments(&pool, new_owner, &now)
            .await
            .unwrap();

        assert_eq!(taken, 1);
        let (state, reason, _, successor) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "cancelled");
        assert_eq!(reason.as_deref(), Some(assignments::REROUTED_REASON));
        let successor = successor.expect("the work moved to a successor");
        let (successor_state, _, executor, _) = assignment_takeover(&pool, &successor).await;
        assert_eq!(successor_state, "queued");
        assert_eq!(executor.as_deref(), Some(NEW_OWNER));
        let inputs: Vec<(String, String, i64, String)> = sqlx::query_as(
            "SELECT message_item_id, assignment_id, ordinal, routing_kind
             FROM chat_work_assignment_inputs ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            inputs,
            vec![
                (
                    "item:review-reply-1".to_owned(),
                    successor.clone(),
                    1,
                    "follow_up".to_owned()
                ),
                (
                    "item:review-reply-2".to_owned(),
                    successor.clone(),
                    2,
                    "queued_continuation".to_owned()
                ),
            ]
        );
        let jobs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_assignment_dispatch_jobs
             WHERE assignment_id = ? AND state = 'queued'",
        )
        .bind(&successor)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(jobs, 1, "the new owner dispatches the successor");
        assert_eq!(
            assignments::take_over_stranded_assignments(&pool, new_owner, &now)
                .await
                .unwrap(),
            0,
            "a second recovery leaves the moved work alone"
        );
    });
}

#[test]
fn stranded_takeover_skips_work_whose_agent_cannot_run_here() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        for (providers, case) in [
            (
                test_providers_with(ProviderAuthoritySupport::default()),
                "no access resolves on this device",
            ),
            (
                super::provider_agents::DeviceProviders::default(),
                "no provider installation runs the agent",
            ),
        ] {
            let writers =
                super::workflow::ChatWriters::new([NEW_OWNER.to_owned()]).with_providers(providers);
            let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, "queued").await;

            let taken = assignments::take_over_stranded_assignments(&pool, new_owner, &now)
                .await
                .unwrap();

            assert_eq!(taken, 0, "{case}");
            let (state, reason, executor, successor) =
                assignment_takeover(&pool, "assignment:old").await;
            assert_eq!(state, "queued", "{case}: the work stays visible");
            assert_eq!(reason, None, "{case}");
            assert_eq!(executor.as_deref(), Some("device:old"), "{case}");
            assert_eq!(successor, None, "{case}: nothing moved onto this device");
            let jobs: i64 =
                sqlx::query_scalar("SELECT count(*) FROM chat_assignment_dispatch_jobs")
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(jobs, 0, "{case}");
        }
    });
}

#[test]
fn started_work_on_the_old_owner_fails_visibly_and_retry_or_start_moves_it_here() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();

        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "working").await;
        let taken = assignments::take_over_stranded_assignments(&pool, new_owner, &now)
            .await
            .unwrap();
        assert_eq!(taken, 1);
        let (state, reason, _, successor) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "failed");
        assert_eq!(reason.as_deref(), Some(assignments::STRANDED_REASON));
        assert_eq!(successor, None, "started work waits for the user");
        let failed = read_assignment(&pool, &ChatWorkAssignmentId::new("assignment:old").unwrap())
            .await
            .unwrap();
        let retried =
            assignments::persist_retry(&pool, &failed.id, failed.revision, new_owner, &now)
                .await
                .unwrap();
        assert_ne!(retried, failed.id, "the executing device never changes");
        let (state, _, executor, _) = assignment_takeover(&pool, retried.as_str()).await;
        assert_eq!(state, "queued");
        assert_eq!(executor.as_deref(), Some(NEW_OWNER));
        let again = assignments::persist_retry(&pool, &failed.id, failed.revision, new_owner, &now)
            .await
            .unwrap_err();
        assert_eq!(again.code, ChatErrorCode::Conflict);

        // Start takes over a routed mention while the old owner's work still looks active.
        let pool = review_thread_with_unrouted_mention().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let elsewhere = chat_writers_of(&["device:old"]);
        let refused = assignments::persist_started_assignment(
            &pool,
            &mention,
            &agent_id,
            super::workflow::PostingDevice::this_device(NEW_OWNER, &elsewhere),
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(
            refused.code,
            ChatErrorCode::Conflict,
            "work on a device that can write Chat is not taken over"
        );
        let started =
            assignments::persist_started_assignment(&pool, &mention, &agent_id, new_owner, &now)
                .await
                .unwrap();
        let (state, reason, _, successor) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "cancelled");
        assert_eq!(reason.as_deref(), Some(assignments::REROUTED_REASON));
        assert_eq!(successor.as_deref(), Some(started.as_str()));
        let (state, _, executor, _) = assignment_takeover(&pool, started.as_str()).await;
        assert_eq!(state, "queued");
        assert_eq!(executor.as_deref(), Some(NEW_OWNER));
        let trigger: String = sqlx::query_scalar(
            "SELECT assignment_id FROM chat_work_assignment_inputs WHERE message_item_id = ?",
        )
        .bind(mention.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(trigger, started.as_str());

        // Start, takeover, and Retry derive the successor from the stranded assignment alone,
        // so two devices moving the same work on different paths merge into one row.
        let old = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let expected = super::common::work_assignment_id(
            &ChatConversationItemId::new("item:review-reply-1").unwrap(),
            &agent_id,
            Some(&old),
        )
        .unwrap();
        assert_eq!(started, expected);
        let pool = review_thread_with_unrouted_mention().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        assignments::take_over_stranded_assignments(&pool, new_owner, &now)
            .await
            .unwrap();
        let (_, _, _, taken_over) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(taken_over.as_deref(), Some(expected.as_str()));
        assert_eq!(
            retried, expected,
            "Retry of started work derives the same successor"
        );
    });
}

#[test]
fn start_refuses_finished_work_whose_device_can_no_longer_write_chat() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        for state in ["completed", "ready_for_review", "failed", "cancelled"] {
            let pool = review_thread_with_unrouted_mention().await;
            seed_assignment_on_old_owner(&pool, state).await;
            let refused = assignments::persist_started_assignment(
                &pool, &mention, &agent_id, new_owner, &now,
            )
            .await
            .unwrap_err();
            assert_eq!(refused.code, ChatErrorCode::Conflict, "{state} work reruns");
            let (kept, _, _, successor) = assignment_takeover(&pool, "assignment:old").await;
            assert_eq!(kept, state);
            assert_eq!(successor, None, "{state} work gets no successor");
        }
    });
}

#[test]
fn takeover_ends_the_local_dispatch_jobs_of_the_work_it_settles() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        for (seeded, job_state) in [("working", "failed"), ("queued", "cancelled")] {
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, seeded).await;
            sqlx::query(
                "INSERT INTO chat_assignment_dispatch_jobs
                    (id, assignment_id, state, available_at, created_at, updated_at)
                 VALUES ('dispatch:old', 'assignment:old', 'claimed', ?1, ?1, ?1)",
            )
            .bind(now.as_str())
            .execute(&pool)
            .await
            .unwrap();
            assignments::take_over_stranded_assignments(&pool, new_owner, &now)
                .await
                .unwrap();
            let state: String = sqlx::query_scalar(
                "SELECT state FROM chat_assignment_dispatch_jobs WHERE id = 'dispatch:old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state, job_state, "{seeded} work ends its dispatch");
        }
    });
}

/// Stores a sync writer of device `device_id` in `state`, numbered `index` for its unique keys,
/// with a removal cutoff when `removed`.
pub(super) async fn insert_sync_writer(
    pool: &SqlitePool,
    index: u8,
    device_id: &str,
    state: &str,
    removed: bool,
) {
    sqlx::query(
        "INSERT OR IGNORE INTO sync_spaces (space_id, vault_id, created_at_ms)
         VALUES (zeroblob(16), 'vault', 0)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sync_writers
            (space_id, writer_id, public_key, device_id, certificate, state, stored_seq,
             applied_seq, head_hash, head_clock, created_at_ms, removal_cutoff_seq)
         VALUES (zeroblob(16), ?1, ?2, ?3, x'00', ?4, 1, 0, zeroblob(32), 0, 0, ?5)",
    )
    .bind(vec![index; 16])
    .bind(vec![index; 32])
    .bind(device_id)
    .bind(state)
    .bind(removed.then_some(0_i64))
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn chat_writers_list_linked_devices_with_an_active_writer_that_was_not_removed() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        insert_sync_writer(&pool, 1, "device:replica", "active", false).await;
        insert_sync_writer(&pool, 2, "device:replica", "retired", false).await;
        insert_sync_writer(&pool, 3, "device:unlinked", "revoked", false).await;
        insert_sync_writer(&pool, 4, "device:removed", "active", true).await;
        insert_sync_writer(&pool, 5, "device:forked", "forked", false).await;
        assert_eq!(
            super::common::active_writer_devices(&pool).await.unwrap(),
            vec!["device:replica".to_string()]
        );
    });
}

#[test]
fn a_device_whose_start_merged_onto_another_executor_ends_only_its_own_dispatch() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "working").await;
        sqlx::query(
            "INSERT INTO chat_assignment_dispatch_jobs
                (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:old', 'assignment:old', 'claimed', ?1, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let job_state = || async {
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM chat_assignment_dispatch_jobs WHERE id = 'dispatch:old'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };

        let executor = assignments::release_moved_assignment(&pool, &assignment_id, "device:old")
            .await
            .unwrap();
        assert!(!executor, "the executing device keeps its run");
        assert_eq!(job_state().await, "claimed");

        let released = assignments::release_moved_assignment(&pool, &assignment_id, NEW_OWNER)
            .await
            .unwrap();
        assert!(released);
        assert_eq!(job_state().await, "cancelled");
        let (state, ..) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(
            state, "working",
            "the replicated assignment is left to its executor"
        );
        let repeated = assignments::release_moved_assignment(&pool, &assignment_id, NEW_OWNER)
            .await
            .unwrap();
        assert!(!repeated);
    });
}

#[test]
fn takeover_visits_every_stranded_assignment_past_one_batch() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&[NEW_OWNER]);
        let new_owner = super::workflow::PostingDevice::this_device(NEW_OWNER, &writers);
        let now = UtcTimestamp::new("2026-08-04T17:05:00.000Z").unwrap();
        let pool = review_thread_without_assignment().await;
        let stranded = 70_i64;
        for index in 0..stranded {
            sqlx::query(
                "INSERT INTO chat_work_assignments
                     (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
                      executing_device_id, state, created_at, updated_at)
                 VALUES (?, 'reply-thread:review', 'participant:review-agent',
                         'item:review-reply-1', 1, json_object('providerManagedModel', json('true')), 'device:old', 'working',
                         '2026-08-04T17:01:00.000Z', '2026-08-04T17:01:00.000Z')",
            )
            .bind(format!("assignment:old-{index:03}"))
            .execute(&pool)
            .await
            .unwrap();
        }

        let taken = assignments::take_over_stranded_assignments(&pool, new_owner, &now)
            .await
            .unwrap();
        assert_eq!(i64::from(taken), stranded);
        let active: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_work_assignments WHERE state = 'working'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            active, 0,
            "no stranded work stays hidden past the first batch"
        );
    });
}

#[test]
fn only_the_executing_device_claims_dispatch_and_writes_active_states() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();
        sqlx::query(
            "INSERT INTO chat_assignment_dispatch_jobs
                (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:review', 'assignment:review', 'queued', ?1, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();

        let refused = assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        assignments::require_executing_device(&pool, &assignment_id, "device:desk")
            .await
            .unwrap();
        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            "device:phone",
            &test_providers(),
            &now,
        )
        .await
        .unwrap();
        assert!(claim.is_none(), "another device never claims the dispatch");
        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            "device:desk",
            &test_providers(),
            &now,
        )
        .await
        .unwrap();
        assert!(claim.is_some());

        // An assignment written before executing devices existed names none.
        sqlx::raw_sql(
            "DELETE FROM chat_assignment_dispatch_jobs;
             DELETE FROM chat_work_assignment_inputs;
             DELETE FROM chat_work_assignments;
             INSERT INTO chat_work_assignments
                 (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
                  executing_device_id, state, created_at, updated_at)
             VALUES ('assignment:review', 'reply-thread:review', 'participant:review-agent',
                     'item:review-root', 1, json_object('providerManagedModel', json('true')), NULL, 'queued', '2026-08-04T17:00:00.000Z',
                     '2026-08-04T17:00:00.000Z');
             INSERT INTO chat_assignment_dispatch_jobs
                 (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:review', 'assignment:review', 'queued',
                     '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z',
                     '2026-08-04T17:00:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .expect("an assignment without an executing device runs where its job is");
        sqlx::query("DELETE FROM chat_assignment_dispatch_jobs")
            .execute(&pool)
            .await
            .unwrap();
        let refused = assignments::require_executing_device(&pool, &assignment_id, "device:phone")
            .await
            .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
    });
}

#[test]
fn concurrent_starts_of_one_mention_on_two_devices_write_one_assignment() {
    tauri::async_runtime::block_on(async {
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let writers = chat_writers_of(&["device:desk", "device:laptop"]);
        let mut started = Vec::new();
        for (device, now) in [
            ("device:desk", "2026-08-04T17:03:00.000Z"),
            ("device:laptop", "2026-08-04T17:03:01.000Z"),
        ] {
            let pool = review_thread_with_unrouted_mention().await;
            started.push(
                assignments::persist_started_assignment(
                    &pool,
                    &mention,
                    &agent_id,
                    super::workflow::PostingDevice::this_device(device, &writers),
                    &UtcTimestamp::new(now).unwrap(),
                )
                .await
                .unwrap(),
            );
        }

        assert_eq!(started[0], started[1], "sync merges the two Starts");
        assert_eq!(
            started[0],
            super::common::work_assignment_id(&mention, &agent_id, None).unwrap()
        );
        let follow_up =
            super::common::work_assignment_id(&mention, &agent_id, Some(&started[0])).unwrap();
        assert_ne!(
            follow_up, started[0],
            "work that follows an assignment of the same mention gets its own id"
        );
    });
}

#[test]
fn start_creates_the_assignment_of_an_unrouted_mention_on_this_device() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_with_unrouted_mention().await;
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let writers = chat_writers_of(&["device:desk", "device:laptop"]);

        let unmentioned = assignments::persist_started_assignment(
            &pool,
            &ChatConversationItemId::new("item:review-reply-1").unwrap(),
            &agent_id,
            super::workflow::PostingDevice::this_device("device:desk", &writers),
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(unmentioned.code, ChatErrorCode::Validation);

        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let assignment_id = assignments::persist_started_assignment(
            &pool,
            &mention,
            &agent_id,
            super::workflow::PostingDevice::this_device("device:desk", &writers),
            &now,
        )
        .await
        .unwrap();
        let started = sqlx::query(
            "SELECT assignment.executing_device_id, assignment.triggering_message_item_id,
                    input.id AS input_id,
                    (SELECT count(*) FROM chat_assignment_dispatch_jobs job
                     WHERE job.assignment_id = assignment.id AND job.state = 'queued')
                        AS queued_jobs
             FROM chat_work_assignments assignment
             JOIN chat_work_assignment_inputs input ON input.assignment_id = assignment.id
             WHERE assignment.id = ?",
        )
        .bind(assignment_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            started.get::<String, _>("executing_device_id"),
            "device:desk"
        );
        assert_eq!(
            started.get::<String, _>("triggering_message_item_id"),
            "item:review-reply-2"
        );
        assert_eq!(
            started.get::<String, _>("input_id"),
            super::common::assignment_input_id("item:review-reply-2").unwrap()
        );
        assert_eq!(started.get::<i64, _>("queued_jobs"), 1);

        let again = assignments::persist_started_assignment(
            &pool,
            &mention,
            &agent_id,
            super::workflow::PostingDevice::this_device("device:laptop", &writers),
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(again.code, ChatErrorCode::Conflict);
    });
}

#[tokio::test]
async fn an_assignment_read_names_the_newest_local_run_thread_and_none_without_local_runs() {
    let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
    let assignment_id = ChatWorkAssignmentId::new("assignment:folders").unwrap();

    let read = read_assignment(&pool, &assignment_id).await.unwrap();
    assert_eq!(
        read.latest_run_provider_thread_id
            .as_ref()
            .map(ChatThreadId::as_str),
        Some("thread-1")
    );

    sqlx::query("DELETE FROM chat_agent_runs")
        .execute(&pool)
        .await
        .unwrap();
    let elsewhere = read_assignment(&pool, &assignment_id).await.unwrap();
    assert_eq!(elsewhere.latest_run_provider_thread_id, None);
}

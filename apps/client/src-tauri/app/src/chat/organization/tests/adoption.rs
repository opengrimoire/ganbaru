//! Adoption of synced work: queued work a linked device created for this device dispatches by
//! itself when its agent can run here.

use super::*;

/// Number of dispatch jobs of `assignment_id`.
async fn dispatch_jobs_of(pool: &SqlitePool, assignment_id: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?")
        .bind(assignment_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn synced_work_for_this_device_dispatches_without_a_local_acceptance() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();

        assert!(
            !assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:other"
            )
            .await
            .unwrap(),
            "a device that does not execute the work leaves it alone"
        );
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0);

        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old"
            )
            .await
            .unwrap()
        );
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 1);
        let now = super::now_timestamp().unwrap();
        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &assignment_id,
                "device:old",
                &test_providers(),
                &now
            )
            .await
            .unwrap()
            .is_some(),
            "the adopted work starts"
        );

        assert!(
            !assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old"
            )
            .await
            .unwrap(),
            "work with a dispatch is not adopted again"
        );
    });
}

#[test]
fn recovery_adopts_every_queued_synced_assignment_of_this_device() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;

        let adopted = assignments::adopt_queued_assignments(&pool, device)
            .await
            .unwrap();

        assert_eq!(
            adopted,
            vec![ChatWorkAssignmentId::new("assignment:old").unwrap()]
        );
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 1);
    });
}

#[test]
fn synced_work_stays_queued_while_its_agent_cannot_run_here() {
    tauri::async_runtime::block_on(async {
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
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
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, "queued").await;

            let adopted = assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &providers,
                "device:old",
            )
            .await
            .unwrap();

            assert!(!adopted, "{case}");
            assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0, "{case}");
            let (state, reason, _, _) = assignment_takeover(&pool, "assignment:old").await;
            assert_eq!(state, "queued", "{case}: the work waits and stays visible");
            assert_eq!(reason, None, "{case}");
        }
    });
}

#[test]
fn a_reply_to_queued_synced_work_queues_as_its_input() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let writers = chat_writers_of(&["device:old"]);
        let device = PostingDevice::this_device("device:old", &writers);

        let posted = persist_post(&pool, &review_reply_mention("command:reply"), None, device)
            .await
            .unwrap();

        assert_eq!(posted.assignment_write.assignment_id, Some(assignment_id));
        assert!(posted.assignment_write.input_queued);
        assert!(posted.assignment_write.executes_here);
        assert_eq!(
            dispatch_jobs_of(&pool, "assignment:old").await,
            0,
            "an input reaches the work's own dispatch instead of queuing another"
        );
    });
}

#[test]
fn assignment_with_foreign_trigger_fails_adoption() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        sqlx::raw_sql(
            "INSERT INTO chat_conversation_items
                 (id, conversation_id, item_kind, order_key, created_at)
             VALUES ('item:foreign', 'conversation:review', 'message', 'a9',
                     '2026-08-04T17:00:30.000Z');
             INSERT INTO chat_communication_messages
                 (item_id, author_participant_id, author_label_snapshot, created_at)
             VALUES ('item:foreign', 'participant:local-owner', 'You',
                     '2026-08-04T17:00:30.000Z');
             INSERT INTO chat_communication_message_revisions
                 (id, message_item_id, normalized_markdown, created_at)
             VALUES ('revision:foreign', 'item:foreign', 'Outside the reply thread',
                     '2026-08-04T17:00:30.000Z');
             UPDATE chat_communication_messages SET current_revision_id = 'revision:foreign'
             WHERE item_id = 'item:foreign';
             INSERT INTO chat_work_assignments
                 (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
                  executing_device_id, state, created_at, updated_at)
             VALUES ('assignment:old', 'reply-thread:review', 'participant:review-agent',
                     'item:foreign', 1, json_object('providerManagedModel', json('true')), 'device:old', 'queued', '2026-08-04T17:01:00.000Z',
                     '2026-08-04T17:01:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();

        let adopted = assignments::adopt_synced_assignment(
            &pool,
            &ChatWorkAssignmentId::new("assignment:old").unwrap(),
            &test_providers(),
            "device:old",
        )
        .await
        .unwrap();

        assert!(!adopted);
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0);
        let (state, reason, _, _) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "failed");
        assert_eq!(reason.as_deref(), Some(assignments::ADOPTION_FAILED_REASON));
    });
}

#[test]
fn assignment_of_a_removed_agent_fails_adoption() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        sqlx::raw_sql(
            "UPDATE chat_conversation_memberships SET removed_at = '2026-08-04T17:00:30.000Z'
             WHERE participant_id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let adopted = assignments::adopt_synced_assignment(
            &pool,
            &ChatWorkAssignmentId::new("assignment:old").unwrap(),
            &test_providers(),
            "device:old",
        )
        .await
        .unwrap();

        assert!(!adopted);
        let (state, reason, _, _) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "failed");
        assert_eq!(reason.as_deref(), Some(assignments::ADOPTION_FAILED_REASON));
    });
}

#[test]
fn a_local_post_claims_the_dispatch_it_queued() {
    tauri::async_runtime::block_on(async {
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let mention = ChatConversationItemId::new("item:review-reply-2").unwrap();
        let writers = chat_writers_of(&["device:desk"]);
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let pool = review_thread_with_unrouted_mention().await;
        let started = assignments::persist_started_assignment(
            &pool,
            &mention,
            &agent_id,
            super::workflow::PostingDevice::this_device("device:desk", &writers),
            &now,
        )
        .await
        .unwrap();
        assert_eq!(dispatch_job_count(&pool, &started).await, 1);

        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &started,
            "device:desk",
            &test_providers(),
            &now,
        )
        .await
        .unwrap()
        .expect("the post queued its dispatch");

        let (display_name, instructions): (String, String) = sqlx::query_as(
            "SELECT participant.display_name, assignment.instructions
             FROM chat_work_assignments assignment
             JOIN chat_participants participant ON participant.id = assignment.agent_id
             WHERE assignment.id = ?",
        )
        .bind(started.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            claim.instructions,
            super::continuation::compose_system_instructions(&display_name, &instructions)
        );
    });
}

#[test]
fn a_queued_job_claims_only_on_its_executing_device() {
    tauri::async_runtime::block_on(async {
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        sqlx::query(
            "INSERT INTO chat_assignment_dispatch_jobs
                (id, assignment_id, state, available_at, created_at, updated_at)
             VALUES ('dispatch:old', 'assignment:old', 'queued', ?1, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &assignment_id,
                "device:other",
                &test_providers(),
                &now
            )
            .await
            .unwrap()
            .is_none()
        );
        assert_eq!(
            dispatch_job_count(&pool, &assignment_id).await,
            1,
            "a device that does not execute the work leaves the job alone"
        );

        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &assignment_id,
                "device:old",
                &test_providers(),
                &now
            )
            .await
            .unwrap()
            .is_some(),
            "the executing device starts the work"
        );
    });
}

#[test]
fn a_scheduled_send_dispatches_the_work_it_queues_here() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);

        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();

        assert!(posted.assignment_write.executes_here);
        assert!(!posted.assignment_write.input_queued);
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_eq!(
            dispatch_job_count(&pool, &assignment_id).await,
            1,
            "a scheduled message from any linked device starts like a local post"
        );
        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &assignment_id,
                "device:desk",
                &test_providers(),
                &super::now_timestamp().unwrap()
            )
            .await
            .unwrap()
            .is_some()
        );
    });
}

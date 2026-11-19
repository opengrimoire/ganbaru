//! Acceptance of synced work: Start here, adoption, acceptance offers, and the waiting list.

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
fn synced_assignment_stays_queued_until_accepted() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();

        let adopted = assignments::adopt_synced_assignment(
            &pool,
            &assignment_id,
            &test_providers(),
            "device:old",
            &ChatDeviceScope::default(),
        )
        .await
        .unwrap();
        assert!(!adopted, "nobody accepted this work on this device");
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0);
        let (state, reason, _, _) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "queued", "the work waits and stays visible");
        assert_eq!(reason, None);

        let mut other_subject = ChatDeviceScope::default();
        record_assignment_acceptance(
            &mut other_subject,
            "0".repeat(64),
            UtcTimestamp::new("2026-08-04T17:00:00.000Z").unwrap(),
        );
        assert!(
            !assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old",
                &other_subject
            )
            .await
            .unwrap(),
            "an acceptance of something else does not start it"
        );

        let scope = scope_accepting(&pool, "assignment:old").await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old",
                &scope
            )
            .await
            .unwrap()
        );
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 1);
    });
}

#[test]
fn queued_input_records_no_acceptance() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let writers = chat_writers_of(&["device:old"]);
        let device = PostingDevice::this_device("device:old", &writers);
        let unaccepted = ChatDeviceScope::default();
        assert!(
            assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &unaccepted,
            )
            .await
            .unwrap(),
            "the synced work waits for Start here"
        );

        let posted = persist_post(
            &pool,
            &review_reply_mention("command:stale-reply"),
            None,
            device,
        )
        .await
        .unwrap();

        assert_eq!(posted.assignment_write.assignment_id, Some(assignment_id));
        assert!(posted.assignment_write.input_queued);
        assert!(posted.assignment_write.executes_here);
        assert!(
            !posted.assignment_write.dispatches_here(),
            "a reply queued into unaccepted work dispatches nothing, so it accepts nothing"
        );
        assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0);

        let pool = review_thread_without_assignment().await;
        let writers = chat_writers_of(&["device:desk"]);
        let posted = persist_post(
            &pool,
            &review_reply_mention("command:new-work"),
            None,
            PostingDevice::this_device("device:desk", &writers),
        )
        .await
        .unwrap();
        assert!(!posted.assignment_write.input_queued);
        assert!(
            posted.assignment_write.dispatches_here(),
            "a mention that queues new work here carries the gesture"
        );
        let new_assignment = posted.assignment_write.assignment_id.unwrap();
        assert_eq!(dispatch_job_count(&pool, &new_assignment).await, 1);
    });
}

#[test]
fn acceptance_lapses_when_synced_access_changes_at_any_scope() {
    tauri::async_runtime::block_on(async {
        for change in [
            "UPDATE chat_channels SET agent_access = 'own'
             WHERE conversation_id = 'conversation:review'",
            "UPDATE chat_channels SET agent_access = 'read'
             WHERE conversation_id = 'conversation:review'",
            "INSERT INTO chat_project_agent_settings
                 (project_id, agent_access, created_at, updated_at)
             VALUES ('project:review', 'talk', '2026-08-04T18:00:00.000Z',
                     '2026-08-04T18:00:00.000Z')",
            "UPDATE chat_agent_app_settings SET agent_access = 'read'",
        ] {
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, "queued").await;
            let scope = scope_accepting(&pool, "assignment:old").await;
            sqlx::raw_sql(change).execute(&pool).await.unwrap();

            assert!(
                !assignments::adopt_synced_assignment(
                    &pool,
                    &ChatWorkAssignmentId::new("assignment:old").unwrap(),
                    &test_providers(),
                    "device:old",
                    &scope,
                )
                .await
                .unwrap(),
                "{change}"
            );
            assert_eq!(dispatch_jobs_of(&pool, "assignment:old").await, 0);
        }
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
            &ChatDeviceScope::default(),
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
            &ChatDeviceScope::default(),
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
fn only_unadopted_work_for_this_device_awaits_local_acceptance() {
    tauri::async_runtime::block_on(async {
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let named_here = chat_writers_of(&["device:old", NEW_OWNER]);
        let stranded = chat_writers_of(&[NEW_OWNER]);
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;

        for (device, expected) in [
            (
                super::workflow::PostingDevice::this_device("device:old", &named_here),
                true,
            ),
            (
                super::workflow::PostingDevice::this_device(NEW_OWNER, &named_here),
                false,
            ),
            (
                super::workflow::PostingDevice::this_device(NEW_OWNER, &stranded),
                true,
            ),
            (
                super::workflow::PostingDevice {
                    id: "device:phone",
                    can_execute: false,
                    chat_writers: &stranded,
                    unattended: false,
                },
                false,
            ),
        ] {
            assert_eq!(
                assignments::assignment_awaits_local_acceptance(
                    &pool,
                    &assignment_id,
                    device,
                    &ChatDeviceScope::default(),
                )
                .await
                .unwrap(),
                expected,
                "{} with its writers",
                device.id
            );
        }

        let scope = scope_accepting(&pool, "assignment:old").await;
        assignments::adopt_synced_assignment(
            &pool,
            &assignment_id,
            &test_providers(),
            "device:old",
            &scope,
        )
        .await
        .unwrap();
        assert!(
            !assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                super::workflow::PostingDevice::this_device("device:old", &named_here),
                &scope,
            )
            .await
            .unwrap(),
            "a queued dispatch already runs it"
        );
    });
}

/// A second reply thread under `root_item_id` holding a queued assignment of `agent_id` that
/// `device:old` executes.
async fn seed_queued_assignment_in_new_thread(
    pool: &SqlitePool,
    assignment_id: &str,
    root_item_id: &str,
    order_key: &str,
    agent_id: &str,
) {
    sqlx::query(
        "INSERT INTO chat_conversation_items
             (id, conversation_id, item_kind, order_key, created_at)
         VALUES (?, 'conversation:review', 'message', ?, '2026-08-04T17:02:00.000Z')",
    )
    .bind(root_item_id)
    .bind(order_key)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_communication_messages
             (item_id, author_participant_id, author_label_snapshot, created_at)
         VALUES (?, 'participant:local-owner', 'You', '2026-08-04T17:02:00.000Z')",
    )
    .bind(root_item_id)
    .execute(pool)
    .await
    .unwrap();
    let reply_thread_id = format!("reply-thread:{assignment_id}");
    sqlx::query(
        "INSERT INTO chat_reply_threads
             (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
         VALUES (?, 'conversation:review', ?, '2026-08-04T17:02:00.000Z',
                 '2026-08-04T17:02:00.000Z', '2026-08-04T17:02:00.000Z')",
    )
    .bind(&reply_thread_id)
    .bind(root_item_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_work_assignments
             (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
              executing_device_id, state, created_at, updated_at)
         VALUES (?, ?, ?, ?, 1, json_object('providerManagedModel', json('true')), 'device:old', 'queued', '2026-08-04T17:02:00.000Z',
                 '2026-08-04T17:02:00.000Z')",
    )
    .bind(assignment_id)
    .bind(&reply_thread_id)
    .bind(agent_id)
    .bind(root_item_id)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn start_here_adopts_queued_siblings_of_the_same_digest() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        seed_queued_assignment_in_new_thread(
            &pool,
            "assignment:sibling",
            "item:sibling-root",
            "a8",
            "participant:review-agent",
        )
        .await;
        seed_other_review_agent(&pool).await;
        seed_queued_assignment_in_new_thread(
            &pool,
            "assignment:other-digest",
            "item:other-root",
            "a9",
            "participant:other-agent",
        )
        .await;
        let accepted = digest_of(&pool, "assignment:old").await;
        assert_eq!(accepted, digest_of(&pool, "assignment:sibling").await);
        assert_ne!(accepted, digest_of(&pool, "assignment:other-digest").await);

        let scope = scope_accepting(&pool, "assignment:old").await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &ChatWorkAssignmentId::new("assignment:old").unwrap(),
                &test_providers(),
                "device:old",
                &scope,
            )
            .await
            .unwrap()
        );

        let adopted = assignments::adopt_accepted_queued_assignments(&pool, device, &scope)
            .await
            .unwrap();

        assert_eq!(
            adopted,
            vec![ChatWorkAssignmentId::new("assignment:sibling").unwrap()]
        );
        assert_eq!(
            dispatch_job_count(
                &pool,
                &ChatWorkAssignmentId::new("assignment:sibling").unwrap()
            )
            .await,
            1
        );
        assert_eq!(
            dispatch_job_count(
                &pool,
                &ChatWorkAssignmentId::new("assignment:other-digest").unwrap()
            )
            .await,
            0
        );
        let (state, _, _, _) = assignment_takeover(&pool, "assignment:other-digest").await;
        assert_eq!(state, "queued");
    });
}

#[test]
fn start_here_refuses_a_digest_the_user_did_not_see() {
    tauri::async_runtime::block_on(async {
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let seen = digest_of(&pool, "assignment:old").await;
        assert!(
            assignments::seen_digest_is_current(&pool, &assignment_id, &test_providers(), &seen)
                .await
                .unwrap()
        );

        sqlx::query(
            "UPDATE chat_channels SET agent_access = 'own'
             WHERE conversation_id = 'conversation:review'",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            !assignments::seen_digest_is_current(&pool, &assignment_id, &test_providers(), &seen)
                .await
                .unwrap(),
            "a sync between render and click changes what the user would accept"
        );
    });
}

#[test]
fn a_changed_subject_adopts_nothing_on_the_old_acceptance() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let scope = scope_accepting(&pool, "assignment:old").await;
        sqlx::query(
            "UPDATE chat_channels SET agent_access = 'own'
             WHERE conversation_id = 'conversation:review'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let adopted = assignments::adopt_accepted_queued_assignments(&pool, device, &scope)
            .await
            .unwrap();

        assert!(adopted.is_empty());
        assert_eq!(
            dispatch_job_count(&pool, &ChatWorkAssignmentId::new("assignment:old").unwrap()).await,
            0
        );
    });
}

#[test]
fn the_acceptance_offer_names_the_provider_folders_and_approvals_behind_its_digest() {
    tauri::async_runtime::block_on(async {
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;

        let offer = assignments::read_acceptance_offer(&pool, &assignment_id, &test_providers())
            .await
            .unwrap()
            .expect("queued work with intact references has an offer");

        assert_eq!(offer.digest, digest_of(&pool, "assignment:old").await);
        assert_eq!(offer.provider_instance_id, "provider:review");
        assert_eq!(offer.folders.len(), 1);
        assert_eq!(offer.folders[0].display_name, "Review");
        let wire = serde_json::to_value(&offer).unwrap();
        assert_eq!(wire["folders"][0]["capability"], "publish");
        assert!(wire["folders"][0]["runtimeApproval"].is_string());

        sqlx::query(
            "UPDATE chat_participants SET archived_at = '2026-08-04T17:30:00.000Z'
             WHERE id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            assignments::read_acceptance_offer(&pool, &assignment_id, &test_providers())
                .await
                .unwrap(),
            None,
            "work Start would refuse offers nothing"
        );
    });
}

#[test]
fn the_acceptance_offer_carries_the_approval_wire_values_of_the_client() {
    tauri::async_runtime::block_on(async {
        for (access, expected) in [("own", "autoApprove"), ("ask", "ask")] {
            let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, "queued").await;
            sqlx::query(
                "UPDATE chat_channels SET agent_access = ?
                 WHERE conversation_id = 'conversation:review'",
            )
            .bind(access)
            .execute(&pool)
            .await
            .unwrap();

            let offer =
                assignments::read_acceptance_offer(&pool, &assignment_id, &test_providers())
                    .await
                    .unwrap()
                    .expect("queued work with intact references has an offer");

            let wire = serde_json::to_value(&offer).unwrap();
            assert_eq!(wire["folders"][0]["runtimeApproval"], expected);
            assert_eq!(wire["folders"][0]["capability"], "publish");
            assert_eq!(offer.digest, digest_of(&pool, "assignment:old").await);
        }
    });
}

#[test]
fn a_job_whose_subject_changed_after_adoption_returns_to_start_here() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let first = ChatWorkAssignmentId::new("assignment:old").unwrap();
        let sibling = ChatWorkAssignmentId::new("assignment:sibling").unwrap();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        seed_queued_assignment_in_new_thread(
            &pool,
            "assignment:sibling",
            "item:sibling-root",
            "a8",
            "participant:review-agent",
        )
        .await;
        let scope = scope_accepting(&pool, "assignment:old").await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &first,
                &test_providers(),
                "device:old",
                &scope
            )
            .await
            .unwrap()
        );
        assignments::adopt_accepted_queued_assignments(&pool, device, &scope)
            .await
            .unwrap();
        assert_eq!(dispatch_job_count(&pool, &sibling).await, 1);
        let now = super::now_timestamp().unwrap();
        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &first,
                "device:old",
                &test_providers(),
                &now
            )
            .await
            .unwrap()
            .is_some(),
            "a job whose subject still matches claims"
        );

        sqlx::query(
            "UPDATE chat_channels SET agent_access = 'own'
             WHERE conversation_id = 'conversation:review'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &sibling,
            "device:old",
            &test_providers(),
            &now,
        )
        .await
        .unwrap();

        assert!(claim.is_none(), "the changed subject never starts");
        assert_eq!(dispatch_job_count(&pool, &sibling).await, 0);
        let (state, _, _, _) = assignment_takeover(&pool, "assignment:sibling").await;
        assert_eq!(state, "queued");
        let waiting = assignments::waiting_assignments(&pool, device, &scope)
            .await
            .unwrap()
            .items;
        assert!(
            waiting.iter().any(|work| work.assignment_id == sibling),
            "the work is listed for Start here again"
        );
    });
}

#[test]
fn a_local_start_claims_without_a_recorded_acceptance() {
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
        .expect("the post itself is the consent");

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
fn a_job_without_a_consented_digest_never_claims() {
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
            .is_none()
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);
    });
}

#[test]
fn waiting_work_reaches_the_device_that_runs_it_across_reply_threads() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let phone = super::workflow::PostingDevice {
            id: "device:phone",
            can_execute: false,
            chat_writers: &writers,
            unattended: false,
        };
        let scope = ChatDeviceScope::default();
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        seed_queued_assignment_in_new_thread(
            &pool,
            "assignment:sibling",
            "item:sibling-root",
            "a8",
            "participant:review-agent",
        )
        .await;

        assert!(
            assignments::waiting_assignments(&pool, phone, &scope)
                .await
                .unwrap()
                .items
                .is_empty(),
            "a device that runs no work has nothing to start"
        );
        let waiting = assignments::waiting_assignments(&pool, device, &scope)
            .await
            .unwrap()
            .items;
        let mut ids: Vec<&str> = waiting
            .iter()
            .map(|row| row.assignment_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, ["assignment:old", "assignment:sibling"]);
        let sibling = waiting
            .iter()
            .find(|row| row.assignment_id.as_str() == "assignment:sibling")
            .unwrap();
        assert_eq!(sibling.project_id, "project:review");
        assert_eq!(sibling.channel_id.as_str(), "channel:review");
        assert_eq!(sibling.channel_name, "general");
        assert_eq!(sibling.agent_display_name, "Review agent");
        assert_eq!(
            sibling.reply_thread_id.as_str(),
            "reply-thread:assignment:sibling"
        );

        sqlx::raw_sql(
            "UPDATE sync_apply_state SET applying = 1 WHERE singleton = 1;
             UPDATE chat_channels SET archived_at = '2026-08-04T17:30:00.000Z'
             WHERE id = 'channel:review';
             UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            assignments::waiting_assignments(&pool, device, &scope)
                .await
                .unwrap()
                .items
                .is_empty(),
            "an archived channel has no thread to open"
        );
    });
}

#[test]
fn waiting_list_reports_the_total_above_its_cap() {
    tauri::async_runtime::block_on(async {
        let writers = chat_writers_of(&["device:old"]);
        let device = super::workflow::PostingDevice::this_device("device:old", &writers);
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        for index in 0..34 {
            seed_queued_assignment_in_new_thread(
                &pool,
                &format!("assignment:waiting-{index:02}"),
                &format!("item:waiting-root-{index:02}"),
                &format!("a9{index:02}1"),
                "participant:review-agent",
            )
            .await;
        }

        let waiting = assignments::waiting_assignments(&pool, device, &ChatDeviceScope::default())
            .await
            .unwrap();

        assert_eq!(waiting.items.len(), 32, "the list stays capped");
        assert_eq!(waiting.total, 35, "the total counts what the cap hides");
    });
}

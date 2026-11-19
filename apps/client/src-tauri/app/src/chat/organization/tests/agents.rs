//! Archiving, restoring, and removing agents, and how archiving meets queued and accepted work.

use super::*;

async fn seed_unused_agent(pool: &SqlitePool, agent_id: &str, policy_id: &str) {
    sqlx::query(
        "INSERT INTO chat_participants
            (id, participant_kind, display_name, created_at, updated_at)
         VALUES (?, 'ai_agent', 'Unused agent', ?, ?)",
    )
    .bind(agent_id)
    .bind("2026-08-14T17:30:00.000Z")
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_agents
            (participant_id, provider_family_id, created_at, updated_at)
         VALUES (?, 'claude', ?, ?)",
    )
    .bind(agent_id)
    .bind("2026-08-14T17:30:00.000Z")
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_agent_policy_revisions
            (id, agent_id, model_selection_data, created_at)
         VALUES (?, ?,
                 '{\"providerManagedModel\":true,\"modelId\":null,\"modelOptions\":[]}', ?)",
    )
    .bind(policy_id)
    .bind(agent_id)
    .bind("2026-08-14T17:30:00.000Z")
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn unused_archived_agents_can_be_restored() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let agent_id = ChatParticipantId::new("participant:unused-agent").unwrap();
        seed_unused_agent(&pool, agent_id.as_str(), "policy:unused-agent").await;

        let archived = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            1,
            true,
            &Default::default(),
        )
        .await
        .unwrap();
        assert!(archived.participant.archived_at.is_some());
        assert_eq!(archived.active_assignment_count, 0);

        let restored = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            archived.participant.revision,
            false,
            &Default::default(),
        )
        .await
        .unwrap();
        assert!(restored.participant.archived_at.is_none());
    });
}

#[test]
fn active_work_blocks_agent_archiving() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_review_assignment_with_stranded_replies(&pool).await;
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();

        let error = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            1,
            true,
            &Default::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        let archived_at: Option<String> =
            sqlx::query_scalar("SELECT archived_at FROM chat_participants WHERE id = ?")
                .bind(agent_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(archived_at.is_none());
    });
}

/// Rows of `agent_id` as `participant agent policies`, each a count.
async fn agent_row_counts(pool: &SqlitePool, agent_id: &str) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM chat_participants WHERE id = ?1),
                (SELECT count(*) FROM chat_agents WHERE participant_id = ?1),
                (SELECT count(*) FROM chat_agent_policy_revisions WHERE agent_id = ?1)",
    )
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn removing_a_never_used_named_agent_deletes_it() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_unused_agent(&pool, NAMED_AGENT_ID, "policy:named").await;
        let agent_id = ChatParticipantId::new(NAMED_AGENT_ID).unwrap();

        let removal =
            super::agent_lifecycle::remove_custom_agent(&pool, &agent_id, 1, &Default::default())
                .await
                .unwrap();
        assert_eq!(removal, ChatCustomAgentRemoval::Deleted);
        assert_eq!(agent_row_counts(&pool, NAMED_AGENT_ID).await, (0, 0, 0));
    });
}

#[test]
fn removing_a_used_named_agent_archives_and_hides_it_until_restored() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_unused_agent(&pool, NAMED_AGENT_ID, "policy:named").await;
        sqlx::query(
            "INSERT INTO chat_agent_usage_daily
                (id, device_id, agent_id, local_date, counted_tokens, working_seconds, run_count)
             VALUES ('usage:named', 'device:desk', ?, '2026-08-14', 10, 5, 1)",
        )
        .bind(NAMED_AGENT_ID)
        .execute(&pool)
        .await
        .unwrap();
        let agent_id = ChatParticipantId::new(NAMED_AGENT_ID).unwrap();

        let removal =
            super::agent_lifecycle::remove_custom_agent(&pool, &agent_id, 1, &Default::default())
                .await
                .unwrap();
        let ChatCustomAgentRemoval::Archived { agent } = removal else {
            panic!("a used agent is archived, not deleted: {removal:?}");
        };
        assert!(agent.participant.archived_at.is_some());
        assert!(agent.hidden_at.is_some());
        assert_eq!(agent_row_counts(&pool, NAMED_AGENT_ID).await, (1, 1, 1));

        let restored = super::agent_lifecycle::restore_custom_agent(
            &pool,
            &agent_id,
            agent.participant.revision,
            &Default::default(),
        )
        .await
        .unwrap();
        assert!(restored.hidden_at.is_none());
        assert_eq!(
            restored.participant.archived_at,
            agent.participant.archived_at
        );
    });
}

#[test]
fn removing_refuses_built_in_agents_stale_revisions_and_active_work() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        seed_unused_agent(&pool, "participant:unused-agent", "policy:unused").await;
        let built_in = ChatParticipantId::new("participant:unused-agent").unwrap();
        let error =
            super::agent_lifecycle::remove_custom_agent(&pool, &built_in, 1, &Default::default())
                .await
                .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        let error =
            super::agent_lifecycle::restore_custom_agent(&pool, &built_in, 1, &Default::default())
                .await
                .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(
            agent_row_counts(&pool, "participant:unused-agent").await,
            (1, 1, 1)
        );

        seed_review_assignment_with_stranded_replies(&pool).await;
        seed_unused_agent(&pool, NAMED_AGENT_ID, "policy:named").await;
        let named = ChatParticipantId::new(NAMED_AGENT_ID).unwrap();
        let error =
            super::agent_lifecycle::remove_custom_agent(&pool, &named, 2, &Default::default())
                .await
                .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::StaleRevision);

        sqlx::query(
            "INSERT INTO chat_work_assignments
                (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data, executing_device_id,
                 state, created_at, updated_at)
             VALUES ('assignment:named', 'reply-thread:review', ?, 'item:review-root', 1,
                     json_object('providerManagedModel', json('true')), 'device:desk', 'working',
                     '2026-08-04T17:00:00.000Z', '2026-08-04T17:00:00.000Z')",
        )
        .bind(NAMED_AGENT_ID)
        .execute(&pool)
        .await
        .unwrap();
        let error =
            super::agent_lifecycle::remove_custom_agent(&pool, &named, 1, &Default::default())
                .await
                .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Conflict);
        let lifecycle: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT archived_at, hidden_at FROM chat_participants WHERE id = ?")
                .bind(NAMED_AGENT_ID)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(lifecycle, (None, None));
    });
}

#[test]
fn removing_a_agent_from_a_channel_keeps_the_rows_and_hides_them_from_readers() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let agent_id = ChatParticipantId::new("participant:design-agent").unwrap();
        seed_unused_agent(&pool, agent_id.as_str(), "policy:design-agent").await;
        sqlx::raw_sql(
            "INSERT INTO project_groups (id, name, icon, order_key)
             VALUES ('group-design', 'Design', 'lucide:folder', 'a0');
             INSERT INTO projects (id, group_id, name, icon, order_key)
             VALUES ('project-design', 'group-design', 'Design', 'lucide:folder', 'a0');
             INSERT INTO chat_conversations
                 (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation-design', 'project-design', 'channel',
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z',
                     '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_channels
                 (id, project_id, conversation_id, name, is_default, created_at, updated_at)
             VALUES ('channel-design', 'project-design', 'conversation-design', 'design', 0,
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_conversation_memberships
                 (conversation_id, participant_id, sync_key, membership_role, created_at,
                  updated_at)
             VALUES ('conversation-design', 'participant:design-agent', 'membership:design',
                     'member', '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');
             INSERT INTO chat_agent_channel_memberships
                 (conversation_id, agent_id, sync_key, created_at,
                  updated_at)
             VALUES ('conversation-design', 'participant:design-agent',
                     'channel-membership:design',
                     '2026-08-14T17:30:00.000Z', '2026-08-14T17:30:00.000Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let conversation_id = ChatConversationId::new("conversation-design").unwrap();
        let mut connection = pool.acquire().await.unwrap();
        let before = access::read_channel_settings(
            &mut connection,
            conversation_id.as_str(),
            &test_providers(),
        )
        .await
        .unwrap();
        drop(connection);
        assert_eq!(before.agents.len(), 1);

        let removed_at = UtcTimestamp::new("2026-08-15T09:00:00.000Z").unwrap();
        let mut transaction = pool.begin().await.unwrap();
        access::remove_agent_from_conversation(
            &mut transaction,
            &agent_id,
            conversation_id.as_str(),
            &removed_at,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();

        let kept: Option<String> = sqlx::query_scalar(
            "SELECT removed_at FROM chat_agent_channel_memberships
             WHERE conversation_id = 'conversation-design'
               AND agent_id = 'participant:design-agent'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept.as_deref(), Some(removed_at.as_str()));
        let mut connection = pool.acquire().await.unwrap();
        let after = access::read_channel_settings(
            &mut connection,
            conversation_id.as_str(),
            &test_providers(),
        )
        .await
        .unwrap();
        drop(connection);
        assert!(after.agents.is_empty());
        let roster = reads::read_memberships_for_conversation(&pool, &conversation_id, false)
            .await
            .unwrap();
        assert!(roster.is_empty());
        let history = reads::read_memberships_for_conversation(&pool, &conversation_id, true)
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].removed_at.is_some());
        assert!(
            !history[0].agent_in_channel,
            "a removed membership is no longer an agent of the channel"
        );
    });
}

#[test]
fn unaccepted_work_does_not_block_archiving_a_agent() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();

        let archived = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            1,
            true,
            &Default::default(),
        )
        .await
        .expect("queued work nobody accepted here does not block archiving");
        assert!(archived.participant.archived_at.is_some());
        assert_eq!(archived.active_assignment_count, 0);
        let (state, reason, executor, _) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "queued", "the unaccepted work stays queued");
        assert_eq!(reason, None);
        assert_eq!(executor.as_deref(), Some("device:old"));
        assert!(
            !assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old",
                &ChatDeviceScope::default(),
            )
            .await
            .unwrap(),
            "an archived agent's queued work does not start"
        );

        let restored = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            archived.participant.revision,
            false,
            &Default::default(),
        )
        .await
        .unwrap();
        assert!(restored.participant.archived_at.is_none());
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
            .unwrap(),
            "the restored agent's queued work starts once accepted"
        );

        let error = super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            restored.participant.revision,
            true,
            &Default::default(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.code,
            ChatErrorCode::Conflict,
            "work accepted on this device still blocks archiving"
        );
    });
}

#[test]
fn accepted_work_of_a_agent_archived_elsewhere_waits_at_claim() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
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
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 1);
        sqlx::query(
            "UPDATE chat_participants SET archived_at = '2026-08-04T17:02:00.000Z',
                 revision = revision + 1
             WHERE id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let now = super::now_timestamp().unwrap();
        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            "device:old",
            &test_providers(),
            &now,
        )
        .await
        .unwrap();

        assert!(
            claim.is_none(),
            "an archived agent's accepted work does not start"
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);
        let (state, reason, executor, _) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "queued", "the accepted work waits for a restore");
        assert_eq!(reason, None);
        assert_eq!(executor.as_deref(), Some("device:old"));

        let archived_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM chat_participants WHERE id = ?")
                .bind(agent_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        super::agent_lifecycle::set_agent_archived(
            &pool,
            &agent_id,
            u64::try_from(archived_revision).unwrap(),
            false,
            &Default::default(),
        )
        .await
        .unwrap();
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:old",
                &scope
            )
            .await
            .unwrap(),
            "the restored agent's work starts once accepted again"
        );
    });
}

#[test]
fn accepted_work_of_a_agent_archived_after_its_claim_waits_for_a_restore() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
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
        let now = super::now_timestamp().unwrap();
        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            "device:old",
            &test_providers(),
            &now,
        )
        .await
        .unwrap()
        .expect("the accepted work claims while the agent is active");
        sqlx::query(
            "UPDATE chat_participants SET archived_at = '2026-08-04T17:02:00.000Z',
                 revision = revision + 1
             WHERE id = 'participant:review-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let error = ChatError::new(
            ChatErrorCode::Permission,
            "The agent is no longer available for organizational work",
            true,
        );

        let settlement =
            super::dispatch::settle_dispatch_error(&pool, &assignment_id, &claim.token, &error)
                .await
                .unwrap();

        assert_eq!(settlement, super::dispatch::DispatchSettlement::Waiting);
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);
        let (state, reason, ..) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "queued", "the accepted work waits for a restore");
        assert_eq!(reason, None);
    });
}

#[test]
fn a_dispatch_error_of_an_active_agent_still_fails_the_work() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "queued").await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:old").unwrap();
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
        let now = super::now_timestamp().unwrap();
        let claim = super::dispatch::claim_dispatch_job(
            &pool,
            &assignment_id,
            "device:old",
            &test_providers(),
            &now,
        )
        .await
        .unwrap()
        .unwrap();
        let error = ChatError::new(
            ChatErrorCode::Permission,
            "The execution folder is no longer authorized for this assignment",
            false,
        );

        let settlement =
            super::dispatch::settle_dispatch_error(&pool, &assignment_id, &claim.token, &error)
                .await
                .unwrap();

        assert_eq!(settlement, super::dispatch::DispatchSettlement::Failed);
        let (state, reason, ..) = assignment_takeover(&pool, "assignment:old").await;
        assert_eq!(state, "failed");
        assert_eq!(reason.as_deref(), Some(error.message.as_str()));
    });
}

#[test]
fn only_live_dispatch_jobs_block_archiving_a_agent() {
    tauri::async_runtime::block_on(async {
        let agent_id = ChatParticipantId::new("participant:review-agent").unwrap();
        for (job_state, blocks) in [
            ("queued", true),
            ("claimed", true),
            ("cancelled", false),
            ("completed", false),
            ("failed", false),
        ] {
            let pool = review_thread_without_assignment().await;
            seed_assignment_on_old_owner(&pool, "queued").await;
            sqlx::query(
                "INSERT INTO chat_assignment_dispatch_jobs
                    (id, assignment_id, state, available_at, created_at, updated_at)
                 VALUES ('dispatch:old', 'assignment:old', ?1, ?2, ?2, ?2)",
            )
            .bind(job_state)
            .bind("2026-08-04T17:02:00.000Z")
            .execute(&pool)
            .await
            .unwrap();

            let result = super::agent_lifecycle::set_agent_archived(
                &pool,
                &agent_id,
                1,
                true,
                &Default::default(),
            )
            .await;

            if blocks {
                assert_eq!(
                    result.unwrap_err().code,
                    ChatErrorCode::Conflict,
                    "a {job_state} job blocks archiving"
                );
            } else {
                let archived = result.unwrap_or_else(|error| {
                    panic!("a {job_state} job must not block archiving: {error:?}")
                });
                assert_eq!(archived.active_assignment_count, 0);
                let (state, ..) = assignment_takeover(&pool, "assignment:old").await;
                assert_eq!(state, "queued");
            }
        }
    });
}

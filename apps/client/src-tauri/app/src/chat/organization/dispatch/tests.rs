use super::{
    ChatModelSelection, ChatRuntimeApprovalPolicy, DispatchModel, InteractionMode,
    ModelAvailability, ModelCatalogSource, ModelId, ProviderInstanceId, ProviderModel,
    ProviderModelCatalog, SafetyMode, UtcTimestamp, assignment_turn_modes, dispatch_model,
};

#[test]
fn targetless_assignments_use_a_narrow_conversation_mode() {
    let modes = assignment_turn_modes(ChatRuntimeApprovalPolicy::AutoApprove, false);
    assert_eq!(modes.safety_mode, SafetyMode::AskForApproval);
    assert_eq!(modes.interaction_mode, InteractionMode::Build);
}

#[test]
fn native_assignments_run_with_the_frozen_runtime_approval() {
    for (resolved, expected) in [
        (ChatRuntimeApprovalPolicy::Ask, SafetyMode::AskForApproval),
        (
            ChatRuntimeApprovalPolicy::AutoApprove,
            SafetyMode::ApproveForMe,
        ),
    ] {
        let modes = assignment_turn_modes(resolved, true);
        assert_eq!(modes.safety_mode, expected, "{resolved:?}");
        assert_eq!(modes.interaction_mode, InteractionMode::Build);
    }
}

fn named(model: &str) -> ChatModelSelection {
    ChatModelSelection {
        provider_managed_model: false,
        model_id: Some(ModelId::new(model).unwrap()),
        model_options: Vec::new(),
    }
}

fn catalog(models: &[(&str, &str)]) -> ProviderModelCatalog {
    ProviderModelCatalog {
        instance_id: ProviderInstanceId::new("codex").unwrap(),
        models: models
            .iter()
            .map(|(id, display_name)| ProviderModel {
                id: ModelId::new(*id).unwrap(),
                display_name: (*display_name).to_string(),
                description: None,
                context_limit: None,
                availability: ModelAvailability::Available,
                capabilities: Vec::new(),
                options: Vec::new(),
            })
            .collect(),
        recommended_model_id: None,
        source: ModelCatalogSource::Provider,
        discovered_at: UtcTimestamp::new("2026-11-18T00:00:00.000Z").unwrap(),
        stale: false,
    }
}

#[test]
fn a_model_the_catalog_offers_runs_under_its_display_name() {
    let catalog = catalog(&[("gpt-5", "GPT-5")]);
    assert_eq!(
        dispatch_model(&named("gpt-5"), Some(&catalog)),
        DispatchModel {
            selection: named("gpt-5"),
            label: Some("GPT-5".to_string()),
        }
    );
}

#[test]
fn an_unknown_model_without_a_recommendation_runs_the_provider_configured_model() {
    let catalog = catalog(&[("gpt-5", "GPT-5")]);
    let model = dispatch_model(&named("gpt-9"), Some(&catalog));
    assert_eq!(model.selection, ChatModelSelection::provider_default());
    assert_eq!(model.label, None);
}

#[test]
fn without_a_catalog_the_named_model_runs_as_frozen() {
    assert_eq!(
        dispatch_model(&named("gpt-9"), None),
        DispatchModel {
            selection: named("gpt-9"),
            label: Some("gpt-9".to_string()),
        }
    );
}

#[test]
fn a_selection_without_a_model_stays_empty_when_nothing_is_recommended() {
    let default = ChatModelSelection::provider_default();
    let model = dispatch_model(&default, Some(&catalog(&[])));
    assert_eq!(model.selection, default);
    assert_eq!(model.label, None);
}

#[test]
fn a_selection_without_a_known_model_runs_the_recommended_model() {
    let mut catalog = catalog(&[("sonnet", "Sonnet 9"), ("opus", "Opus 9")]);
    catalog.recommended_model_id = Some(ModelId::new("opus").unwrap());
    let expected = DispatchModel {
        selection: named("opus"),
        label: Some("Opus 9".to_string()),
    };
    assert_eq!(
        dispatch_model(&ChatModelSelection::provider_default(), Some(&catalog)),
        expected
    );
    assert_eq!(dispatch_model(&named("gpt-9"), Some(&catalog)), expected);
}

#[test]
fn a_model_line_runs_its_newest_model_with_the_frozen_options() {
    use ganbaru_chat_contracts::models::{ModelOptionSelection, ModelOptionValue};
    let catalog = catalog(&[
        ("gpt-5.5-sol", "GPT-5.5-Sol"),
        ("gpt-5.6-sol", "GPT-5.6-Sol"),
        ("gpt-5.6-astra", "GPT-5.6-Astra"),
    ]);
    let effort = vec![ModelOptionSelection {
        key: "reasoningEffort".to_string(),
        value: ModelOptionValue::Choice("high".to_string()),
    }];
    let frozen = ChatModelSelection {
        model_options: effort.clone(),
        ..named("sol")
    };
    assert_eq!(
        dispatch_model(&frozen, Some(&catalog)),
        DispatchModel {
            selection: ChatModelSelection {
                model_options: effort,
                ..named("gpt-5.6-sol")
            },
            label: Some("GPT-5.6-Sol".to_string()),
        }
    );
}

/// Which earlier run a later assignment of the fixture's reply thread could continue.
mod context_delivery {
    use super::super::{PlannedDelivery, choose_context_delivery};
    use crate::chat::organization::continuation::DeltaPackage;
    use ganbaru_chat::agent_runs::{
        AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind, FullPackage, HandoffReason,
    };

    fn source(source_id: &str, digit: char) -> DeliveredSource {
        DeliveredSource {
            source_id: source_id.to_string(),
            content_sha256: digit.to_string().repeat(64),
            kind: DeliveryKind::Package,
        }
    }

    fn full() -> FullPackage {
        FullPackage {
            prompt: "header\nroot\nearlier\ntrigger".to_string(),
            delivered_sources: vec![
                source("item:root", 'a'),
                source("item:earlier", 'b'),
                source("item:trigger", 'c'),
            ],
        }
    }

    fn delta() -> DeltaPackage {
        DeltaPackage {
            text: "continuation header\ntrigger".to_string(),
            sources: vec![source("item:trigger", 'c')],
        }
    }

    #[test]
    fn a_continuation_sends_its_delta_and_keeps_the_full_package_for_a_fresh_session() {
        assert_eq!(
            choose_context_delivery(AttemptReason::Continuation, Some(delta()), full()),
            PlannedDelivery {
                prompt: "continuation header\ntrigger".to_string(),
                context_delivery: ContextDelivery::Delta,
                delivered_sources: vec![source("item:trigger", 'c')],
                full_prompt_on_fresh_session: Some(full()),
            }
        );
    }

    #[test]
    fn a_continuation_whose_delta_did_not_fit_sends_the_full_package() {
        assert_eq!(
            choose_context_delivery(AttemptReason::Continuation, None, full()),
            PlannedDelivery {
                prompt: full().prompt,
                context_delivery: ContextDelivery::Full,
                delivered_sources: full().delivered_sources,
                full_prompt_on_fresh_session: Some(full()),
            }
        );
    }

    #[test]
    fn every_run_in_a_new_session_sends_the_full_package_without_a_fallback() {
        for reason in [
            AttemptReason::Fresh,
            AttemptReason::Retry,
            AttemptReason::Handoff(HandoffReason::NativeSessionLost),
            AttemptReason::Handoff(HandoffReason::KeyChanged),
            AttemptReason::Handoff(HandoffReason::AuthorityRevoked),
        ] {
            assert_eq!(
                choose_context_delivery(reason, Some(delta()), full()),
                PlannedDelivery {
                    prompt: full().prompt,
                    context_delivery: ContextDelivery::Full,
                    delivered_sources: full().delivered_sources,
                    full_prompt_on_fresh_session: None,
                },
                "{reason:?}"
            );
        }
    }
}

mod continuation_candidate {
    use super::super::{CandidateScope, StoredCandidate, read_continuation_candidate};
    use crate::chat::organization::continuation::ContinuationKey;
    use ganbaru_chat_contracts::models::*;
    use sqlx::SqlitePool;

    const MODEL_SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const INSTRUCTIONS_SHA: &str =
        "2222222222222222222222222222222222222222222222222222222222222222";

    /// The fixture's working run completed under a recorded key, with its assignment
    /// completed and the queued run removed.
    async fn completed_pool() -> SqlitePool {
        let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "DELETE FROM chat_agent_runs WHERE id = 'run:queued';
                 UPDATE chat_agent_runs SET state = 'starting' WHERE id = 'run:working';
                 UPDATE chat_agent_runs
                 SET model_selection_sha256 = '{MODEL_SHA}',
                     instructions_sha256 = '{INSTRUCTIONS_SHA}', state = 'completed'
                 WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'completed'
                 WHERE id = 'assignment:folders';"
        )))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn candidate_for(pool: &SqlitePool, agent: &str) -> Option<StoredCandidate> {
        let environment: String = sqlx::query_scalar(
            "SELECT execution_environment_id FROM chat_agent_runs WHERE id = 'run:working'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        read_continuation_candidate(
            pool,
            &CandidateScope {
                assignment_id: &ChatWorkAssignmentId::new("assignment:later").unwrap(),
                reply_thread_id: &ChatReplyThreadId::new("reply:folders").unwrap(),
                agent_id: &ChatParticipantId::new(agent).unwrap(),
                working_folder_id: Some(&ProjectWorkingFolderId::new("workspace-1").unwrap()),
                scratch_generation_id: None,
                execution_environment_id: Some(&environment),
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_completed_run_of_the_same_agent_is_the_candidate_with_its_stored_key() {
        let pool = completed_pool().await;
        let provider: String = sqlx::query_scalar(
            "SELECT provider_instance_id FROM chat_threads WHERE id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let candidate = candidate_for(&pool, "participant:folder-agent")
            .await
            .expect("the completed run can be continued");
        assert_eq!(candidate.thread_id, "thread-1");
        assert!(candidate.authorization_valid);
        assert_eq!(
            candidate.key,
            Some(ContinuationKey {
                authorization_scope_digest: format!("{:064}", 0),
                provider_instance_id: provider,
                model_selection_sha256: MODEL_SHA.to_string(),
                instructions_sha256: INSTRUCTIONS_SHA.to_string(),
            })
        );
    }

    #[tokio::test]
    async fn another_agent_in_the_thread_never_continues_the_session() {
        let pool = completed_pool().await;
        assert!(
            candidate_for(&pool, "participant:other-agent")
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_revoked_authorization_or_missing_key_is_reported_not_hidden() {
        let pool = completed_pool().await;
        sqlx::query(
            "UPDATE chat_assignment_authorization_revisions
                 SET decision_state = 'revoked', revoked_at = '2026-10-20T12:05:00.000Z'
                 WHERE id = 'authorization:working'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let candidate = candidate_for(&pool, "participant:folder-agent")
            .await
            .expect("a revoked candidate is still reported");
        assert!(!candidate.authorization_valid);

        let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
        sqlx::raw_sql(
            "DELETE FROM chat_agent_runs WHERE id = 'run:queued';
                 UPDATE chat_agent_runs SET state = 'completed' WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'completed'
                 WHERE id = 'assignment:folders';",
        )
        .execute(&pool)
        .await
        .unwrap();
        let candidate = candidate_for(&pool, "participant:folder-agent")
            .await
            .expect("a run without a recorded key is still reported");
        assert_eq!(candidate.key, None);
    }

    #[tokio::test]
    async fn an_open_assignment_or_a_closed_thread_is_no_candidate() {
        for update in [
            "UPDATE chat_work_assignments SET state = 'working'
                 WHERE id = 'assignment:folders'",
            "UPDATE chat_threads SET state = 'closed' WHERE id = 'thread-1'",
        ] {
            let pool = completed_pool().await;
            sqlx::query(update).execute(&pool).await.unwrap();
            assert!(
                candidate_for(&pool, "participant:folder-agent")
                    .await
                    .is_none(),
                "{update}"
            );
        }
    }
}

/// Launch state the launch tests share: the fixture's working run back at `starting`, its
/// assignment queued, and its dispatch job claimed by `claim:folders`.
mod launch {
    use super::super::{
        AssignmentLaunch, fail_assignment_dispatch, mark_initial_assignment_input_delivered,
        pending_live_input_ids, prepare_assignment_input, record_assignment_input_delivered,
    };
    use crate::chat::organization::assignments::reclaim_stale_dispatch_claims;
    use crate::chat::organization::continuation::{
        self, CandidateRun, ContinuationKey, TransitionInput,
    };
    use ganbaru_chat::agent_runs::{
        AgentRunBinding, AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind,
        HandoffReason, LaunchAttempt, LaunchSettlement, RunAttempt, RunLaunch,
        insert_assignment_failure_message, settle_launch,
    };
    use ganbaru_chat_contracts::models::*;
    use sqlx::SqlitePool;

    const CLAIMED_AT: &str = "2026-10-20T12:00:00.000Z";

    fn assignment() -> ChatWorkAssignmentId {
        ChatWorkAssignmentId::new("assignment:folders").unwrap()
    }

    fn at(timestamp: &str) -> UtcTimestamp {
        UtcTimestamp::new(timestamp.to_string()).unwrap()
    }

    fn launch_error() -> ChatError {
        ChatError::new(ChatErrorCode::DriverUnavailable, "Provider exited", true)
    }

    async fn starting_pool() -> SqlitePool {
        let pool = crate::chat::revocation::tests::pool_with_folder_runs().await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "UPDATE chat_agent_runs SET state = 'starting' WHERE id = 'run:working';
                 UPDATE chat_work_assignments SET state = 'queued'
                 WHERE id = 'assignment:folders';
                 INSERT INTO chat_assignment_dispatch_jobs
                    (id, assignment_id, state, claim_token, claimed_at, available_at,
                     created_at, updated_at)
                 VALUES ('dispatch:folders', 'assignment:folders', 'claimed', 'claim:folders',
                         '{CLAIMED_AT}', '{CLAIMED_AT}', '{CLAIMED_AT}', '{CLAIMED_AT}');"
        )))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn working_binding(pool: &SqlitePool) -> AgentRunBinding {
        let execution_environment_id: String = sqlx::query_scalar(
            "SELECT execution_environment_id FROM chat_agent_runs WHERE id = 'run:working'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        AgentRunBinding {
            run_id: ChatAgentRunId::new("run:working").unwrap(),
            assignment_id: assignment(),
            project_id: "project-chat".to_string(),
            authorization_revision_id: ChatAuthorizationRevisionId::new("authorization:working")
                .unwrap(),
            authorization_scope_digest: format!("{:064}", 0),
            working_folder_id: Some("workspace-1".to_string()),
            scratch_generation_id: None,
            execution_environment_id: Some(execution_environment_id),
            run_ordinal: 1,
        }
    }

    fn delivered(source_id: &str, digit: char) -> DeliveredSource {
        DeliveredSource {
            source_id: source_id.to_string(),
            content_sha256: digit.to_string().repeat(64),
            kind: DeliveryKind::Package,
        }
    }

    /// The launch of the fixture run under the fixture claim, delivering two sources.
    fn folder_launch() -> RunLaunch {
        RunLaunch {
            claim_token: "claim:folders".to_string(),
            attempt: RunAttempt {
                reason: AttemptReason::Fresh,
                instructions_sha256: None,
                model_selection_sha256: None,
                context_delivery: ContextDelivery::Full,
                delivered_context_bytes: 42,
                delivered_sources: vec![
                    delivered("item:root", 'a'),
                    delivered("item:folders", 'b'),
                ],
            },
            full_prompt_on_fresh_session: None,
            model_label: None,
        }
    }

    async fn delivered_sources(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT source_id, delivery_kind FROM chat_agent_run_delivered_sources
                 WHERE run_id = 'run:working' ORDER BY source_id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    /// Posts a message by the local owner in the assignment's reply thread and records it
    /// as an assignment input routed as `routing_kind`.
    async fn seed_input(
        pool: &SqlitePool,
        item_id: &str,
        ordinal: i64,
        routing_kind: &str,
        text: &str,
    ) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO chat_conversation_items
                    (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
                 SELECT '{item_id}', conversation_id, id, 'message', 'a{ordinal}', '{CLAIMED_AT}'
                 FROM chat_reply_threads WHERE id = 'reply:folders';
                 INSERT INTO chat_communication_messages
                    (item_id, author_participant_id, created_at)
                 VALUES ('{item_id}', 'participant:local-owner', '{CLAIMED_AT}');
                 INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES ('revision:{item_id}', '{item_id}', '{text}', '{CLAIMED_AT}');
                 UPDATE chat_communication_messages
                 SET current_revision_id = 'revision:{item_id}' WHERE item_id = '{item_id}';
                 INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
                 VALUES ('input:{item_id}', 'assignment:folders', '{item_id}', {ordinal},
                         '{routing_kind}', '{CLAIMED_AT}');"
        )))
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_inputs_of_every_route(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, created_at)
                 VALUES ('input:trigger', 'assignment:folders', 'item:folders', 1, 'trigger',
                         '2026-10-20T12:00:00.000Z')",
        )
        .execute(pool)
        .await
        .unwrap();
        seed_input(
            pool,
            "item:follow-up",
            2,
            "follow_up",
            "Also check the logs",
        )
        .await;
        seed_input(pool, "item:steer", 3, "steer", "Prefer the smaller fix").await;
        seed_input(
            pool,
            "item:continuation",
            4,
            "queued_continuation",
            "Then update the docs",
        )
        .await;
    }

    async fn delivery_states(pool: &SqlitePool) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT routing_kind, delivery_state FROM chat_work_assignment_inputs
                 WHERE assignment_id = 'assignment:folders' ORDER BY ordinal",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    fn states(package: &str) -> Vec<(String, String)> {
        [
            ("trigger", package),
            ("follow_up", package),
            ("steer", "pending"),
            ("queued_continuation", "pending"),
        ]
        .into_iter()
        .map(|(kind, state)| (kind.to_string(), state.to_string()))
        .collect()
    }

    async fn failure_messages(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM chat_work_semantic_updates
                 WHERE assignment_id = 'assignment:folders' AND update_kind = 'failure'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn assignment_and_job(pool: &SqlitePool) -> (String, String) {
        sqlx::query_as(
            "SELECT assignment.state, job.state
                 FROM chat_work_assignments assignment
                 JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = assignment.id
                 WHERE assignment.id = 'assignment:folders'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn thread_and_conversation_revisions(pool: &SqlitePool) -> (i64, i64) {
        sqlx::query_as(
            "SELECT thread.revision, conversation.revision
                 FROM chat_reply_threads thread
                 JOIN chat_conversations conversation ON conversation.id = thread.conversation_id
                 WHERE thread.id = 'reply:folders'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn pair(first: &str, second: &str) -> (String, String) {
        (first.to_string(), second.to_string())
    }

    #[tokio::test]
    async fn a_failed_launch_posts_one_failure_message_per_attempt() {
        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        let error = launch_error();

        let settlement = settle_launch(
            &pool,
            &binding,
            &folder_launch(),
            Some(&error),
            &at(CLAIMED_AT),
        )
        .await
        .unwrap();
        assert_eq!(settlement, LaunchSettlement::Recorded);
        assert_eq!(failure_messages(&pool).await, 1);
        assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));

        // The dispatch error path that follows a failed launch finds the job settled.
        fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
            .await
            .unwrap();
        assert_eq!(failure_messages(&pool).await, 1);

        let revisions = thread_and_conversation_revisions(&pool).await;
        let repeated = {
            let mut connection = pool.acquire().await.unwrap();
            insert_assignment_failure_message(
                &mut connection,
                &assignment(),
                LaunchAttempt::Run(&binding.run_id),
                &error.message,
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap()
        };
        assert!(!repeated, "the same attempt posts no second message");
        assert_eq!(failure_messages(&pool).await, 1);
        assert_eq!(thread_and_conversation_revisions(&pool).await, revisions);

        let claim = {
            let mut connection = pool.acquire().await.unwrap();
            insert_assignment_failure_message(
                &mut connection,
                &assignment(),
                LaunchAttempt::Claim("claim:folders"),
                &error.message,
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap()
        };
        assert!(claim, "a claim attempt is keyed apart from the run attempt");
        assert_eq!(failure_messages(&pool).await, 2);
        assert_ne!(thread_and_conversation_revisions(&pool).await, revisions);
    }

    #[tokio::test]
    async fn only_a_launch_that_starts_its_run_records_delivered_sources() {
        let error = launch_error();
        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        settle_launch(
            &pool,
            &binding,
            &folder_launch(),
            Some(&error),
            &at(CLAIMED_AT),
        )
        .await
        .unwrap();
        assert!(delivered_sources(&pool).await.is_empty(), "failed launch");

        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'cancelled',
                     settled_at = '2026-10-20T12:01:00.000Z'
                 WHERE id = 'assignment:folders'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let superseded = settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        assert_eq!(superseded, LaunchSettlement::Superseded);
        assert!(
            delivered_sources(&pool).await.is_empty(),
            "superseded launch"
        );

        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        sqlx::query("UPDATE chat_agent_runs SET state = 'failed' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        let settled = settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        assert_eq!(settled, LaunchSettlement::RunSettled);
        assert!(delivered_sources(&pool).await.is_empty(), "settled run");

        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        let recorded = settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        assert_eq!(recorded, LaunchSettlement::Recorded);
        let expected = vec![
            pair("item:folders", "package"),
            pair("item:root", "package"),
        ];
        assert_eq!(delivered_sources(&pool).await, expected);

        let mut connection = pool.acquire().await.unwrap();
        ganbaru_chat::agent_runs::insert_delivered_sources(
            &mut connection,
            &binding.run_id,
            &folder_launch().attempt.delivered_sources,
            &at("2026-10-20T12:01:00.000Z"),
        )
        .await
        .expect("a replayed delivery keeps its first record");
        drop(connection);
        assert_eq!(delivered_sources(&pool).await, expected);
    }

    #[tokio::test]
    async fn a_launch_whose_claim_was_lost_settles_only_its_run() {
        for error in [None, Some(launch_error())] {
            let pool = starting_pool().await;
            sqlx::query(
                "UPDATE chat_assignment_dispatch_jobs
                     SET claim_token = 'claim:newer' WHERE assignment_id = 'assignment:folders'",
            )
            .execute(&pool)
            .await
            .unwrap();
            let binding = working_binding(&pool).await;

            let settlement = settle_launch(
                &pool,
                &binding,
                &folder_launch(),
                error.as_ref(),
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap();

            assert_eq!(settlement, LaunchSettlement::ClaimLost);
            assert!(settlement.stops_session());
            assert_eq!(
                run_and_job(&pool).await,
                (
                    "cancelled".to_string(),
                    Some("2026-10-20T12:01:00.000Z".to_string()),
                    "claimed".to_string(),
                    None
                )
            );
            assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
            assert_eq!(failure_messages(&pool).await, 0);
            assert!(delivered_sources(&pool).await.is_empty());
        }
    }

    #[tokio::test]
    async fn a_dispatch_failure_settles_only_the_claim_that_holds_the_job() {
        let pool = starting_pool().await;
        let error = launch_error();

        fail_assignment_dispatch(&pool, &assignment(), "claim:stale", &error)
            .await
            .unwrap();
        assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
        assert_eq!(failure_messages(&pool).await, 0);

        sqlx::query(
            "UPDATE chat_assignment_dispatch_jobs
                 SET state = 'queued', claim_token = NULL, claimed_at = NULL",
        )
        .execute(&pool)
        .await
        .unwrap();
        fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
            .await
            .unwrap();
        assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
        assert_eq!(failure_messages(&pool).await, 0);

        sqlx::query(
            "UPDATE chat_assignment_dispatch_jobs
                 SET state = 'claimed', claim_token = 'claim:folders'",
        )
        .execute(&pool)
        .await
        .unwrap();
        fail_assignment_dispatch(&pool, &assignment(), "claim:folders", &error)
            .await
            .unwrap();
        assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));
        assert_eq!(failure_messages(&pool).await, 1);
    }

    #[tokio::test]
    async fn package_inputs_stay_pending_until_the_launched_run_starts() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        let run_id = ChatAgentRunId::new("run:working").unwrap();

        let failed = AssignmentLaunch {
            run_id: run_id.clone(),
            launch_error: Some(launch_error()),
            settlement: None,
            answered_input_ids: Vec::new(),
        };
        assert!(
            !mark_initial_assignment_input_delivered(&pool, &assignment(), &failed)
                .await
                .unwrap()
        );
        assert_eq!(delivery_states(&pool).await, states("pending"));

        let unstarted = AssignmentLaunch {
            run_id: run_id.clone(),
            launch_error: None,
            settlement: None,
            answered_input_ids: Vec::new(),
        };
        assert!(
            !mark_initial_assignment_input_delivered(&pool, &assignment(), &unstarted)
                .await
                .unwrap()
        );
        assert_eq!(delivery_states(&pool).await, states("pending"));

        let missing = AssignmentLaunch {
            run_id: ChatAgentRunId::new("run:missing").unwrap(),
            launch_error: None,
            settlement: None,
            answered_input_ids: Vec::new(),
        };
        assert!(
            !mark_initial_assignment_input_delivered(&pool, &assignment(), &missing)
                .await
                .unwrap()
        );

        let binding = working_binding(&pool).await;
        settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        assert!(
            mark_initial_assignment_input_delivered(&pool, &assignment(), &unstarted)
                .await
                .unwrap()
        );
        assert_eq!(delivery_states(&pool).await, states("delivered"));
    }

    #[tokio::test]
    async fn a_package_input_records_the_revision_its_package_froze() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO chat_assignment_context_packages
                    (id, assignment_id, revision, triggering_message_item_id, serialized_text,
                     serialized_bytes, sha256, created_at)
                 VALUES ('context:folders', 'assignment:folders', 1, 'item:follow-up', 'frozen',
                         6, '{sha}', '{CLAIMED_AT}');
                 INSERT INTO chat_assignment_context_sources
                    (context_package_id, source_kind, source_id, source_revision,
                     content_sha256, ordinal)
                 VALUES ('context:folders', 'trigger', 'item:follow-up', 1, '{sha}', 0);
                 INSERT INTO chat_communication_message_revisions
                    (id, message_item_id, normalized_markdown, created_at)
                 VALUES ('revision:item:follow-up:edited', 'item:follow-up',
                         'Check the logs and the metrics', '2026-10-20T12:01:00.000Z');
                 UPDATE chat_communication_messages
                 SET current_revision_id = 'revision:item:follow-up:edited'
                 WHERE item_id = 'item:follow-up';",
            sha = "c".repeat(64),
        )))
        .execute(&pool)
        .await
        .unwrap();
        let binding = working_binding(&pool).await;
        settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        let launch = AssignmentLaunch {
            run_id: ChatAgentRunId::new("run:working").unwrap(),
            launch_error: None,
            settlement: None,
            answered_input_ids: Vec::new(),
        };

        assert!(
            mark_initial_assignment_input_delivered(&pool, &assignment(), &launch)
                .await
                .unwrap()
        );

        let revisions: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT routing_kind, delivered_revision_id FROM chat_work_assignment_inputs
                 WHERE routing_kind IN ('trigger', 'follow_up') ORDER BY ordinal",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            revisions,
            vec![
                ("trigger".to_string(), None),
                (
                    "follow_up".to_string(),
                    Some("revision:item:follow-up".to_string())
                ),
            ],
            "the edit after the freeze never reached the run"
        );
    }

    #[tokio::test]
    async fn a_retried_assignment_frames_its_pending_steer_for_the_working_run() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        let steer = ChatConversationItemId::new("item:steer").unwrap();
        let continuation = ChatConversationItemId::new("item:continuation").unwrap();

        assert_eq!(
            pending_live_input_ids(&pool, &assignment()).await.unwrap(),
            vec![steer.clone()],
            "a queued continuation waits for the turn to end"
        );
        for state in ["starting", "failed"] {
            sqlx::query("UPDATE chat_agent_runs SET state = ? WHERE id = 'run:working'")
                .bind(state)
                .execute(&pool)
                .await
                .unwrap();
            let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
                .await
                .unwrap();
            assert!(prepared.is_none(), "a {state} run takes no live input");
        }

        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            prepare_assignment_input(&pool, &assignment(), &continuation)
                .await
                .unwrap()
                .is_none(),
            "the working run never takes a queued continuation"
        );
        let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
            .await
            .unwrap()
            .expect("the working run takes the pending steer");
        assert_eq!(prepared.thread_id.as_str(), "thread-1");
        assert_eq!(prepared.run_id.as_str(), "run:working");
        let framed = super::super::super::context::follow_up_context_line(
            "item:steer",
            1,
            "Prefer the smaller fix".to_string(),
            false,
        )
        .unwrap();
        assert_eq!(prepared.prompt, framed.line);
        assert_eq!(
            prepared.delivered,
            DeliveredSource {
                source_id: "item:steer".to_string(),
                content_sha256: framed.content_sha256,
                kind: DeliveryKind::Steer,
            }
        );
    }

    #[tokio::test]
    async fn held_replies_wait_and_steers_reach_the_run_in_queue_order() {
        use crate::chat::organization::inputs::{
            reorder_assignment_inputs, set_assignment_input_held, steer_assignment_input,
        };
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        seed_input(&pool, "item:second", 5, "steer", "Keep the tests green").await;
        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        let item = |id: &str| ChatConversationItemId::new(id).unwrap();
        let input = |id: &str| ChatAssignmentInputId::new(format!("input:{id}")).unwrap();

        let held = set_assignment_input_held(&pool, &input("item:steer"), true)
            .await
            .unwrap();
        assert!(held.held);
        assert_eq!(
            pending_live_input_ids(&pool, &assignment()).await.unwrap(),
            vec![item("item:second")],
            "delivery takes the first reply no one holds"
        );
        assert!(
            prepare_assignment_input(&pool, &assignment(), &item("item:steer"))
                .await
                .unwrap()
                .is_none(),
            "the working run never takes a held reply"
        );

        set_assignment_input_held(&pool, &input("item:steer"), false)
            .await
            .unwrap();
        assert!(
            prepare_assignment_input(&pool, &assignment(), &item("item:steer"))
                .await
                .unwrap()
                .is_some(),
            "a resumed reply is eligible again"
        );
        let reordered = reorder_assignment_inputs(
            &pool,
            &assignment(),
            &[
                input("item:second"),
                input("item:continuation"),
                input("item:steer"),
            ],
        )
        .await
        .unwrap();
        let order = reordered
            .iter()
            .filter(|read| read.routing_kind != "follow_up")
            .map(|read| (read.message_item_id.as_str(), read.ordinal))
            .collect::<Vec<_>>();
        assert_eq!(
            order,
            [
                ("item:second", 3),
                ("item:continuation", 4),
                ("item:steer", 5)
            ],
            "the reordered replies take the slots they held"
        );
        assert_eq!(
            pending_live_input_ids(&pool, &assignment()).await.unwrap(),
            vec![item("item:second"), item("item:steer")]
        );

        set_assignment_input_held(&pool, &input("item:continuation"), true)
            .await
            .unwrap();
        let steered = steer_assignment_input(&pool, &input("item:continuation"))
            .await
            .unwrap();
        assert_eq!(
            (steered.routing_kind.as_str(), steered.held),
            ("steer", false),
            "steering now resumes a held reply"
        );
        assert_eq!(
            pending_live_input_ids(&pool, &assignment()).await.unwrap(),
            vec![
                item("item:second"),
                item("item:continuation"),
                item("item:steer")
            ]
        );
    }

    #[tokio::test]
    async fn a_steered_input_records_its_delivery_on_the_run_once() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        let steer = ChatConversationItemId::new("item:steer").unwrap();
        let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
            .await
            .unwrap()
            .expect("the working run takes the pending steer");
        assert_eq!(prepared.delivered.kind, DeliveryKind::Steer);

        for _ in 0..2 {
            record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
                .await
                .unwrap();
        }

        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT source_id, content_sha256, delivery_kind
                 FROM chat_agent_run_delivered_sources WHERE run_id = 'run:working'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![(
                "item:steer".to_string(),
                prepared.delivered.content_sha256.clone(),
                "steer".to_string()
            )]
        );
        let states = delivery_states(&pool).await;
        assert!(states.contains(&("steer".to_string(), "delivered".to_string())));
    }

    #[tokio::test]
    async fn a_steer_the_provider_read_stays_delivered_over_a_later_cancel() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        let steer = ChatConversationItemId::new("item:steer").unwrap();
        let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
            .await
            .unwrap()
            .expect("the working run takes the pending steer");
        sqlx::query(
            "UPDATE chat_work_assignment_inputs SET delivery_state = 'ignored'
                 WHERE message_item_id = 'item:steer'",
        )
        .execute(&pool)
        .await
        .unwrap();

        record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
            .await
            .unwrap();

        let (state, revision): (String, Option<String>) = sqlx::query_as(
            "SELECT delivery_state, delivered_revision_id
                 FROM chat_work_assignment_inputs WHERE message_item_id = 'item:steer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "delivered");
        assert_eq!(revision.as_deref(), Some(prepared.revision_id.as_str()));
        let sources: Vec<(String,)> = sqlx::query_as(
            "SELECT source_id FROM chat_agent_run_delivered_sources WHERE run_id = 'run:working'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(sources, vec![("item:steer".to_string(),)]);
    }

    /// Posts agent output in the assignment's reply thread after every seeded reply.
    async fn seed_agent_output(pool: &SqlitePool) {
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "INSERT INTO chat_conversation_items
                    (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
                 SELECT 'item:agent-output', conversation_id, id, 'work_update', 'a9',
                        '{CLAIMED_AT}'
                 FROM chat_reply_threads WHERE id = 'reply:folders';"
        )))
        .execute(pool)
        .await
        .unwrap();
    }

    /// The ids of the reply thread's items in thread order.
    async fn thread_order(pool: &SqlitePool) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT id FROM chat_conversation_items WHERE reply_thread_id = 'reply:folders'
                 ORDER BY order_key, id",
        )
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn shown(pool: &SqlitePool, item_id: &str) -> ChatMessageRead {
        let item_id = ChatConversationItemId::new(item_id).unwrap();
        crate::chat::organization::reads::read_message(pool, &item_id)
            .await
            .unwrap()
    }

    async fn reply_count(pool: &SqlitePool) -> u64 {
        let thread = ChatReplyThreadId::new("reply:folders").unwrap();
        crate::chat::organization::reads::read_reply_thread_summary(pool, &thread)
            .await
            .unwrap()
            .reply_count
    }

    async fn delivered_at(pool: &SqlitePool, item_id: &str) -> String {
        sqlx::query_scalar(
            "SELECT delivered_at FROM chat_work_assignment_inputs WHERE message_item_id = ?",
        )
        .bind(item_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_steer_waits_out_of_the_thread_and_enters_it_after_the_output_the_run_had() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        seed_agent_output(&pool).await;
        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        let waiting = shown(&pool, "item:steer").await;
        assert!(waiting.awaiting_delivery);
        assert!(shown(&pool, "item:continuation").await.awaiting_delivery);
        assert!(!shown(&pool, "item:follow-up").await.awaiting_delivery);
        let replies_while_waiting = reply_count(&pool).await;

        let steer = ChatConversationItemId::new("item:steer").unwrap();
        let prepared = prepare_assignment_input(&pool, &assignment(), &steer)
            .await
            .unwrap()
            .expect("the working run takes the pending steer");
        record_assignment_input_delivered(&pool, &assignment(), &steer, &prepared)
            .await
            .unwrap();

        let entered = shown(&pool, "item:steer").await;
        assert!(!entered.awaiting_delivery);
        assert_eq!(
            entered.created_at.as_str(),
            delivered_at(&pool, "item:steer").await
        );
        assert_eq!(entered.normalized_markdown, waiting.normalized_markdown);
        assert_eq!(entered.revision_id, waiting.revision_id);
        assert_eq!(
            thread_order(&pool).await.last().map(String::as_str),
            Some("item:steer")
        );
        assert_eq!(reply_count(&pool).await, replies_while_waiting + 1);
        let posted: String = sqlx::query_scalar(
            "SELECT created_at FROM chat_communication_messages WHERE item_id = 'item:steer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(posted, CLAIMED_AT, "the message keeps its posting time");
    }

    #[tokio::test]
    async fn a_launch_places_the_answers_it_carried_and_leaves_package_inputs_in_place() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        seed_agent_output(&pool).await;
        let before = thread_order(&pool).await;
        let binding = working_binding(&pool).await;
        settle_launch(&pool, &binding, &folder_launch(), None, &at(CLAIMED_AT))
            .await
            .unwrap();
        let launch = AssignmentLaunch {
            run_id: ChatAgentRunId::new("run:working").unwrap(),
            launch_error: None,
            settlement: None,
            answered_input_ids: vec!["item:continuation".to_string()],
        };
        assert!(
            mark_initial_assignment_input_delivered(&pool, &assignment(), &launch)
                .await
                .unwrap()
        );

        let mut expected = before
            .into_iter()
            .filter(|id| id != "item:continuation")
            .collect::<Vec<_>>();
        expected.push("item:continuation".to_string());
        assert_eq!(thread_order(&pool).await, expected);
        let follow_up = shown(&pool, "item:follow-up").await;
        assert_eq!(follow_up.created_at.as_str(), CLAIMED_AT);
        assert_eq!(follow_up.order_key, "a2");
        let answer = shown(&pool, "item:continuation").await;
        assert_eq!(
            answer.created_at.as_str(),
            delivered_at(&pool, "item:continuation").await
        );
        assert!(shown(&pool, "item:steer").await.awaiting_delivery);
    }

    #[tokio::test]
    async fn held_and_cancelled_replies_stay_out_and_a_failed_work_shows_its_waiting_reply() {
        let pool = starting_pool().await;
        seed_inputs_of_every_route(&pool).await;
        let held = ChatAssignmentInputId::new("input:item:continuation").unwrap();
        crate::chat::organization::inputs::set_assignment_input_held(&pool, &held, true)
            .await
            .unwrap();
        assert!(shown(&pool, "item:continuation").await.awaiting_delivery);
        let cancelled = ChatAssignmentInputId::new("input:item:steer").unwrap();
        crate::chat::organization::inputs::cancel_assignment_input(&pool, &cancelled)
            .await
            .unwrap();
        let gone = shown(&pool, "item:steer").await;
        assert!(gone.awaiting_delivery);
        assert_eq!(gone.order_key, "a3", "a cancelled reply never moves");

        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'failed'
                 WHERE id = 'assignment:folders'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(!shown(&pool, "item:continuation").await.awaiting_delivery);
        assert!(shown(&pool, "item:steer").await.awaiting_delivery);
    }

    async fn run_and_job(pool: &SqlitePool) -> (String, Option<String>, String, Option<String>) {
        sqlx::query_as(
            "SELECT run.state, run.settled_at, job.state, job.last_error
                 FROM chat_agent_runs run
                 JOIN chat_assignment_dispatch_jobs job ON job.assignment_id = run.assignment_id
                 WHERE run.id = 'run:working'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn recovery_settles_the_starting_run_of_a_stale_claim_before_requeueing_it() {
        let pool = starting_pool().await;
        let now = "2026-10-20T12:06:00.000Z";

        reclaim_stale_dispatch_claims(&pool, &at(now))
            .await
            .unwrap();

        assert_eq!(
            run_and_job(&pool).await,
            (
                "failed".to_string(),
                Some(now.to_string()),
                "queued".to_string(),
                Some("launch_interrupted".to_string())
            )
        );
        let binding = working_binding(&pool).await;
        let late = settle_launch(
            &pool,
            &binding,
            &folder_launch(),
            None,
            &at("2026-10-20T12:07:00.000Z"),
        )
        .await
        .unwrap();
        assert_eq!(late, LaunchSettlement::RunSettled);
        assert_eq!(run_and_job(&pool).await.0, "failed");
        assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
    }

    /// The fixture launch as a continuation of an earlier native session.
    fn continuation_launch(reason: AttemptReason) -> RunLaunch {
        let launch = folder_launch();
        RunLaunch {
            attempt: RunAttempt {
                reason,
                context_delivery: ContextDelivery::Delta,
                ..launch.attempt
            },
            ..launch
        }
    }

    fn session_lost() -> ChatError {
        ChatError::new(
            ChatErrorCode::ResumeNotFound,
            "The provider session was not found",
            true,
        )
    }

    /// Opens an approval on the fixture provider thread, as a session that is later lost
    /// leaves it.
    async fn open_approval(pool: &SqlitePool) {
        sqlx::query(
            "INSERT INTO chat_pending_requests
                    (id, thread_id, turn_id, provider_request_id, request_kind,
                     safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
                 VALUES ('request:detached', 'thread-1', 'turn:working', 'native-request',
                         'approval', '{}', '[]', 1, ?)",
        )
        .bind(CLAIMED_AT)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn approval_state(pool: &SqlitePool) -> (String, Option<String>) {
        sqlx::query_as(
            "SELECT resolution_state, resolved_at FROM chat_pending_requests
                 WHERE id = 'request:detached'",
        )
        .fetch_one(pool)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_continuation_that_lost_its_session_fails_only_its_run_and_asks_for_a_handoff() {
        let pool = starting_pool().await;
        open_approval(&pool).await;
        let binding = working_binding(&pool).await;
        let now = "2026-10-20T12:01:00.000Z";

        let settlement = settle_launch(
            &pool,
            &binding,
            &continuation_launch(AttemptReason::Continuation),
            Some(&session_lost()),
            &at(now),
        )
        .await
        .unwrap();

        assert_eq!(settlement, LaunchSettlement::HandoffRequired);
        assert!(settlement.stops_session());
        assert_eq!(
            run_and_job(&pool).await,
            (
                "failed".to_string(),
                Some(now.to_string()),
                "claimed".to_string(),
                None
            )
        );
        assert_eq!(assignment_and_job(&pool).await, pair("queued", "claimed"));
        assert_eq!(failure_messages(&pool).await, 0);
        assert!(delivered_sources(&pool).await.is_empty());
        assert_eq!(
            approval_state(&pool).await,
            ("interrupted".to_string(), Some(now.to_string())),
            "an approval of the lost session is closed, never left open"
        );
    }

    #[tokio::test]
    async fn a_lost_session_fails_the_assignment_once_the_run_is_no_longer_a_continuation() {
        for reason in [
            AttemptReason::Handoff(HandoffReason::NativeSessionLost),
            AttemptReason::Retry,
            AttemptReason::Fresh,
        ] {
            let pool = starting_pool().await;
            let binding = working_binding(&pool).await;

            let settlement = settle_launch(
                &pool,
                &binding,
                &continuation_launch(reason),
                Some(&session_lost()),
                &at("2026-10-20T12:01:00.000Z"),
            )
            .await
            .unwrap();

            assert_eq!(settlement, LaunchSettlement::Recorded, "{reason:?}");
            assert_eq!(assignment_and_job(&pool).await, pair("failed", "failed"));
            assert_eq!(failure_messages(&pool).await, 1);
        }

        let pool = starting_pool().await;
        sqlx::query(
            "UPDATE chat_work_assignments SET state = 'cancelled',
                     settled_at = '2026-10-20T12:00:30.000Z'
                 WHERE id = 'assignment:folders'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let binding = working_binding(&pool).await;
        let settlement = settle_launch(
            &pool,
            &binding,
            &continuation_launch(AttemptReason::Continuation),
            Some(&session_lost()),
            &at("2026-10-20T12:01:00.000Z"),
        )
        .await
        .unwrap();
        assert_eq!(
            settlement,
            LaunchSettlement::Superseded,
            "a settled assignment never hands off"
        );
    }

    #[tokio::test]
    async fn a_restart_that_missed_the_handoff_retries_the_assignment_in_a_new_session() {
        let pool = starting_pool().await;
        let binding = working_binding(&pool).await;
        settle_launch(
            &pool,
            &binding,
            &continuation_launch(AttemptReason::Continuation),
            Some(&session_lost()),
            &at("2026-10-20T12:01:00.000Z"),
        )
        .await
        .unwrap();

        // The application stops before the handoff run starts, so the claimed job is
        // requeued once it is stale.
        reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:06:00.000Z"))
            .await
            .unwrap();
        assert_eq!(assignment_and_job(&pool).await, pair("queued", "queued"));
        let earlier_runs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_agent_runs WHERE assignment_id = 'assignment:folders'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(earlier_runs > 0);

        let key = ContinuationKey {
            authorization_scope_digest: format!("{:064}", 0),
            provider_instance_id: "fake-instance".to_string(),
            model_selection_sha256: "d".repeat(64),
            instructions_sha256: "c".repeat(64),
        };
        let decision = continuation::decide_session_transition(&TransitionInput {
            key: &key,
            unfinished_earlier_run: earlier_runs > 0,
            candidate: Some(CandidateRun {
                thread_id: "thread-1",
                key: Some(key.clone()),
                authorization_valid: true,
            }),
            native_session_lost: false,
        });
        assert_eq!(decision.reason, AttemptReason::Retry);
        assert_eq!(decision.reuse_thread, None);
    }

    #[tokio::test]
    async fn recovery_leaves_fresh_claims_and_working_runs_alone() {
        let pool = starting_pool().await;
        reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:03:00.000Z"))
            .await
            .unwrap();
        assert_eq!(
            run_and_job(&pool).await,
            ("starting".to_string(), None, "claimed".to_string(), None)
        );

        sqlx::query("UPDATE chat_agent_runs SET state = 'working' WHERE id = 'run:working'")
            .execute(&pool)
            .await
            .unwrap();
        reclaim_stale_dispatch_claims(&pool, &at("2026-10-20T12:06:00.000Z"))
            .await
            .unwrap();
        assert_eq!(
            run_and_job(&pool).await,
            ("working".to_string(), None, "queued".to_string(), None)
        );
    }
}

mod answered_question {
    use super::super::{
        AnsweredQuestion, AttemptReason, ContextDelivery, DeliveredSource, DeliveryKind,
        FullPackage, MAX_MESSAGE_BYTES, choose_context_delivery, continuation,
        frame_answered_question,
    };
    use ganbaru_chat::coordination::reports::ThreadMessage;
    use ganbaru_chat_contracts::models::ChatWorkAssignmentId;

    fn message(item_id: &str, markdown: &str) -> ThreadMessage {
        ThreadMessage {
            item_id: item_id.to_owned(),
            revision: 1,
            markdown: markdown.to_owned(),
        }
    }

    fn answered(answer: &str) -> AnsweredQuestion {
        AnsweredQuestion {
            asking_run_id: "run:1".to_owned(),
            question: message("item:question", "Keep the old page?"),
            answers: vec![message("item:answer", answer)],
        }
    }

    fn package() -> FullPackage {
        FullPackage {
            prompt: "{\"kind\":\"trigger\",\"text\":\"tidy the docs\"}\n".to_owned(),
            delivered_sources: vec![DeliveredSource {
                source_id: "item:root".to_owned(),
                content_sha256: "a".repeat(64),
                kind: DeliveryKind::Package,
            }],
        }
    }

    #[test]
    fn a_continued_session_receives_only_the_answers_and_a_new_one_the_question_too() {
        let framed = frame_answered_question(&answered("Keep it")).unwrap();
        assert!(framed.fits_after(&package().prompt));
        assert_eq!(framed.input_ids, ["item:answer"]);
        let assignment_id = ChatWorkAssignmentId::new("assignment:1".to_owned()).unwrap();
        let delta =
            continuation::answer_delta(&assignment_id, 1, &framed.answers, framed.sources.clone())
                .unwrap();
        let full = framed.after_package(package());
        let planned = choose_context_delivery(AttemptReason::Continuation, delta, full);

        assert_eq!(planned.context_delivery, ContextDelivery::Delta);
        assert!(planned.prompt.contains("Keep it"));
        assert!(
            !planned.prompt.contains("tidy the docs"),
            "the session holds the trigger"
        );
        assert!(!planned.prompt.contains("Keep the old page?"));
        assert_eq!(
            planned
                .delivered_sources
                .iter()
                .map(|source| (source.source_id.as_str(), source.kind))
                .collect::<Vec<_>>(),
            [("item:answer", DeliveryKind::QueuedContinuation)]
        );

        let fresh = planned.full_prompt_on_fresh_session.unwrap();
        let trigger = fresh.prompt.find("tidy the docs").unwrap();
        let question = fresh.prompt.find("Keep the old page?").unwrap();
        let answer = fresh.prompt.find("Keep it").unwrap();
        assert!(trigger < question && question < answer);
        assert_eq!(fresh.delivered_sources.len(), 2);
    }

    #[test]
    fn answers_that_do_not_fit_after_the_package_stay_pending() {
        let framed = frame_answered_question(&answered(&"x".repeat(MAX_MESSAGE_BYTES))).unwrap();
        assert!(!framed.fits_after(&package().prompt));
    }
}

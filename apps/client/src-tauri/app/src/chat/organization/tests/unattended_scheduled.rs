//! Unattended and consented scheduled sends: what work they queue or accept, and its target.

use super::*;

#[test]
fn unattended_scheduled_post_leaves_assignment_queued() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Unattended,
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);

        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();

        assert!(posted.assignment_write.awaits_acceptance);
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_eq!(
            dispatch_job_count(&pool, &assignment_id).await,
            0,
            "the unattended post queued its dispatch"
        );
        let read = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(read.state, ChatWorkAssignmentState::Queued);
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
            "the work waits for Start here"
        );
        assert!(
            !assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:desk",
                &unaccepted,
            )
            .await
            .unwrap()
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);

        let accepted = scope_accepting(&pool, assignment_id.as_str()).await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:desk",
                &accepted
            )
            .await
            .unwrap(),
            "an accepted agent runs through the adoption gate"
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 1);
    });
}

#[test]
fn recovery_after_a_stop_starts_accepted_unattended_work_and_leaves_the_rest_at_start_here() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Unattended,
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        // The post commits and the app stops before the unattended start runs.
        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();

        let unaccepted = ChatDeviceScope::default();
        let adopted = assignments::adopt_accepted_queued_assignments(&pool, device, &unaccepted)
            .await
            .unwrap();
        assert!(adopted.is_empty(), "recovery started unaccepted work");
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);
        assert!(
            assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &unaccepted,
            )
            .await
            .unwrap(),
            "unaccepted work still waits for Start here after recovery"
        );

        let accepted = scope_accepting(&pool, assignment_id.as_str()).await;
        let adopted = assignments::adopt_accepted_queued_assignments(&pool, device, &accepted)
            .await
            .unwrap();
        assert_eq!(
            adopted,
            std::slice::from_ref(&assignment_id),
            "recovery starts work whose agent this device accepted"
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 1);
    });
}

/// Adds a second, non-primary folder `folder:second` to the review project.
async fn seed_second_review_folder(pool: &SqlitePool) {
    sqlx::raw_sql(
        "INSERT INTO project_working_folders
             (id, project_id, display_name, kind, managed_relative_path, sort_order)
         VALUES ('folder:second', 'project:review', 'Second', 'external', NULL, 1);",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// The working folder the frozen authorization of `assignment_id` runs in.
async fn frozen_execution_folder(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> Option<String> {
    sqlx::query_scalar(
        "SELECT folder.working_folder_id
         FROM chat_assignment_authorized_folder_sources folder
         JOIN chat_assignment_authorization_revisions authorization
           ON authorization.id = folder.authorization_revision_id
         WHERE authorization.assignment_id = ? AND folder.is_execution_target = 1",
    )
    .bind(assignment_id.as_str())
    .fetch_optional(pool)
    .await
    .unwrap()
}

#[test]
fn unattended_work_keeps_the_execution_target_its_request_chose() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_second_review_folder(&pool).await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let target = ChatExecutionTarget::WorkingFolder {
            working_folder_id: ProjectWorkingFolderId::new("folder:second").unwrap(),
            execution_environment_id: ChatExecutionEnvironmentId::new(
                ganbaru_chat::project_defaults::current_folder_environment_id("folder:second"),
            )
            .unwrap(),
        };
        let (request, scheduled_message_id) =
            claim_scheduled_mention_on(&pool, &now, Some(target)).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Unattended,
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);

        let accepted = scope_accepting(&pool, assignment_id.as_str()).await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:desk",
                &accepted
            )
            .await
            .unwrap()
        );

        assert_eq!(
            frozen_execution_folder(&pool, &assignment_id)
                .await
                .as_deref(),
            Some("folder:second"),
            "the accepted work dropped the execution target its request chose"
        );
    });
}

/// Posts an unattended reply on `device:desk` whose request chose `folder:second`, returning
/// the queued assignment that waits for Start here.
async fn post_unattended_work_on_second_folder(pool: &SqlitePool) -> ChatWorkAssignmentRead {
    seed_second_review_folder(pool).await;
    let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
    let target = ChatExecutionTarget::WorkingFolder {
        working_folder_id: ProjectWorkingFolderId::new("folder:second").unwrap(),
        execution_environment_id: ChatExecutionEnvironmentId::new(
            ganbaru_chat::project_defaults::current_folder_environment_id("folder:second"),
        )
        .unwrap(),
    };
    let (request, scheduled_message_id) =
        claim_scheduled_mention_on(pool, &now, Some(target)).await;
    let send = ScheduledSend {
        scheduled_message_id: &scheduled_message_id,
        claim_token: "claim:a",
        consent: ScheduledConsent::Unattended,
        occurrence: None,
    };
    let writers = chat_writers_of(&["device:desk"]);
    let device = PostingDevice::this_device("device:desk", &writers);
    let posted = persist_post(pool, &request, Some(send), device)
        .await
        .unwrap();
    let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();
    read_assignment(pool, &assignment_id).await.unwrap()
}

/// How many execution targets this device keeps for `assignment_id`.
async fn requested_target_count(pool: &SqlitePool, assignment_id: &ChatWorkAssignmentId) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM chat_assignment_requested_targets WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn retry_of_cancelled_unattended_work_resolves_its_target_again() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let queued = post_unattended_work_on_second_folder(&pool).await;
        // A cancel synced from another device settles the work without this device's cancel path.
        sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'cancelled', settled_at = updated_at, revision = revision + 1
             WHERE id = ?",
        )
        .bind(queued.id.as_str())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE project_working_folders SET archived_at = '2026-08-04T17:00:30.000Z'
             WHERE id = 'folder:second'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let cancelled = read_assignment(&pool, &queued.id).await.unwrap();
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let now = UtcTimestamp::new("2026-08-04T17:04:00.000Z").unwrap();

        let retried =
            assignments::persist_retry(&pool, &cancelled.id, cancelled.revision, device, &now)
                .await;

        assert!(
            retried.is_ok(),
            "retry kept the target whose grant was removed: {retried:?}"
        );
        assert_eq!(
            frozen_execution_folder(&pool, &cancelled.id)
                .await
                .as_deref(),
            Some("folder:review")
        );
    });
}

#[test]
fn cancelling_unattended_work_drops_its_kept_target() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let queued = post_unattended_work_on_second_folder(&pool).await;
        assert_eq!(requested_target_count(&pool, &queued.id).await, 1);

        super::workflow::set_assignment_state(
            &pool,
            &queued.id,
            queued.revision,
            ChatWorkAssignmentState::Cancelled,
            Some("Cancelled by the user"),
        )
        .await
        .unwrap();

        assert_eq!(
            requested_target_count(&pool, &queued.id).await,
            0,
            "the target outlived the work it was kept for"
        );
    });
}

/// The subject digest the dispatch job of `assignment_id` consented to run.
async fn consented_dispatch_digest(
    pool: &SqlitePool,
    assignment_id: &ChatWorkAssignmentId,
) -> String {
    sqlx::query_scalar(
        "SELECT consented_digest FROM chat_assignment_dispatch_jobs WHERE assignment_id = ?",
    )
    .bind(assignment_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn consented_scheduled_send_accepts_the_work_it_dispatches() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();

        let digest = post_acceptance_digest(&pool, &posted.assignment_write, Some(send))
            .await
            .unwrap();

        assert_eq!(
            digest,
            Some(consented_dispatch_digest(&pool, &assignment_id).await),
            "a consented scheduled send did not accept the work it dispatched"
        );
    });
}

#[test]
fn unattended_scheduled_send_accepts_nothing() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Unattended,
            occurrence: None,
        };
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();

        let digest = post_acceptance_digest(&pool, &posted.assignment_write, Some(send))
            .await
            .unwrap();

        assert_eq!(digest, None, "an unattended send accepted its work");
    });
}

/// Sends the claimed scheduled reply of [`claim_scheduled_review_reply`] on `device:desk`.
async fn send_scheduled_review_reply(
    pool: &SqlitePool,
    request: &PostChatMessageCommand,
    scheduled_message_id: &ChatScheduledMessageId,
    consent: ScheduledConsent,
) -> super::PersistedPost {
    let send = ScheduledSend {
        scheduled_message_id,
        claim_token: "claim:a",
        consent,
        occurrence: None,
    };
    let writers = chat_writers_of(&["device:desk"]);
    persist_post(
        pool,
        request,
        Some(send),
        PostingDevice::this_device("device:desk", &writers),
    )
    .await
    .unwrap()
}

/// The number of assignment inputs that deliver the message `message_item_id`.
async fn inputs_of_message(pool: &SqlitePool, message_item_id: &ChatConversationItemId) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_work_assignment_inputs WHERE message_item_id = ?")
        .bind(message_item_id.as_str())
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn unattended_scheduled_reply_queues_no_input_on_active_work() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (pool, request, scheduled_message_id) =
            claim_scheduled_review_reply("working", &now).await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();
        let before = read_assignment(&pool, &assignment_id).await.unwrap();

        let posted = send_scheduled_review_reply(
            &pool,
            &request,
            &scheduled_message_id,
            ScheduledConsent::Unattended,
        )
        .await;

        assert!(
            !posted.assignment_write.input_queued,
            "the unattended reply queued input on active work"
        );
        assert_eq!(inputs_of_message(&pool, &posted.item_id).await, 0);
        let after = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(after.state, ChatWorkAssignmentState::Working);
        assert_eq!(after.revision, before.revision);
        assert!(
            read_post_receipt(&pool, &request.client_command_id)
                .await
                .unwrap()
                .is_some(),
            "the unattended reply stays visible"
        );
    });
}

#[test]
fn unattended_scheduled_reply_settles_no_reviewed_work() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (pool, request, scheduled_message_id) =
            claim_scheduled_review_reply("ready_for_review", &now).await;
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();

        let posted = send_scheduled_review_reply(
            &pool,
            &request,
            &scheduled_message_id,
            ScheduledConsent::Unattended,
        )
        .await;

        let reviewed = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(
            reviewed.state,
            ChatWorkAssignmentState::ReadyForReview,
            "the unattended reply settled the reviewed work"
        );
        let assignments: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_work_assignments")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(assignments, 1, "the unattended reply created a follow-up");
        assert!(!posted.assignment_write.dispatches_here());
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        assert!(
            assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &ChatDeviceScope::default(),
            )
            .await
            .unwrap(),
            "the reply waits for Start here"
        );

        let accepted = scope_accepting(&pool, assignment_id.as_str()).await;
        let started =
            assignments::recover_stranded_follow_up_assignments(&pool, device, &accepted, &now)
                .await
                .unwrap();
        assert_eq!(started, 1, "accepting the work did not start the follow-up");
        let settled = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(settled.state, ChatWorkAssignmentState::Completed);
    });
}

/// The execution target that runs work in the review agent's second folder `folder:second`.
fn second_review_folder_target() -> ChatExecutionTarget {
    ChatExecutionTarget::WorkingFolder {
        working_folder_id: ProjectWorkingFolderId::new("folder:second").unwrap(),
        execution_environment_id: ChatExecutionEnvironmentId::new(
            ganbaru_chat::project_defaults::current_folder_environment_id("folder:second"),
        )
        .unwrap(),
    }
}

/// Sends on `device:desk`, unattended, a scheduled reply to `assignment:review` awaiting review
/// whose request chose `folder:second`, returning the pool once the reply waits as a pending
/// input of the reviewed work.
async fn post_unattended_review_reply_on_second_folder(now: &UtcTimestamp) -> SqlitePool {
    let (pool, mut request, scheduled_message_id) =
        claim_scheduled_review_reply("ready_for_review", now).await;
    seed_second_review_folder(&pool).await;
    request.execution_target = Some(second_review_folder_target());
    sqlx::query("UPDATE chat_scheduled_messages SET request_data = ? WHERE id = ?")
        .bind(serde_json::to_string(&request).unwrap())
        .bind(scheduled_message_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let posted = send_scheduled_review_reply(
        &pool,
        &request,
        &scheduled_message_id,
        ScheduledConsent::Unattended,
    )
    .await;
    assert!(posted.assignment_write.input_queued);
    assert!(posted.assignment_write.awaits_acceptance);
    pool
}

/// The follow-up assignment that settled `assignment:review`, if one exists.
async fn review_follow_up_id(pool: &SqlitePool) -> Option<ChatWorkAssignmentId> {
    sqlx::query_scalar::<_, String>(
        "SELECT id FROM chat_work_assignments WHERE previous_assignment_id = 'assignment:review'",
    )
    .fetch_optional(pool)
    .await
    .unwrap()
    .map(|id| ChatWorkAssignmentId::new(id).unwrap())
}

#[test]
fn follow_up_of_an_unattended_review_reply_keeps_its_chosen_target() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let pool = post_unattended_review_reply_on_second_folder(&now).await;
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let accepted = scope_accepting(&pool, "assignment:review").await;

        let started =
            assignments::recover_stranded_follow_up_assignments(&pool, device, &accepted, &now)
                .await
                .unwrap();

        assert_eq!(started, 1, "accepting the work did not start the follow-up");
        let follow_up_id = review_follow_up_id(&pool).await.unwrap();
        assert_eq!(
            frozen_execution_folder(&pool, &follow_up_id)
                .await
                .as_deref(),
            Some("folder:second"),
            "the follow-up dropped the execution target its reply chose"
        );
        let kept: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_assignment_input_requested_targets")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kept, 0, "the target outlived the reply it was kept for");
    });
}

#[test]
fn follow_up_resolves_the_target_again_when_the_chosen_one_no_longer_resolves() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let pool = post_unattended_review_reply_on_second_folder(&now).await;
        sqlx::query(
            "UPDATE project_working_folders SET archived_at = '2026-08-04T17:00:30.000Z'
             WHERE id = 'folder:second'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        let accepted = scope_accepting(&pool, "assignment:review").await;

        let refused =
            assignments::recover_stranded_follow_up_assignments(&pool, device, &accepted, &now)
                .await
                .unwrap();
        assert_eq!(
            refused, 0,
            "the follow-up started on a target without a grant"
        );
        assert_eq!(review_follow_up_id(&pool).await, None);

        let started =
            assignments::recover_stranded_follow_up_assignments(&pool, device, &accepted, &now)
                .await
                .unwrap();
        assert_eq!(
            started, 1,
            "the reply stayed stuck on its unresolvable target"
        );
        let follow_up_id = review_follow_up_id(&pool).await.unwrap();
        assert_eq!(
            frozen_execution_folder(&pool, &follow_up_id)
                .await
                .as_deref(),
            Some("folder:review")
        );
    });
}

#[test]
fn consented_scheduled_replies_still_route_to_their_work() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let assignment_id = ChatWorkAssignmentId::new("assignment:review").unwrap();

        let (pool, request, scheduled_message_id) =
            claim_scheduled_review_reply("working", &now).await;
        let posted = send_scheduled_review_reply(
            &pool,
            &request,
            &scheduled_message_id,
            ScheduledConsent::Local,
        )
        .await;
        assert!(posted.assignment_write.input_queued);
        assert!(posted.assignment_write.executes_here);
        assert_eq!(
            posted.assignment_write.assignment_id.as_ref(),
            Some(&assignment_id)
        );
        assert_eq!(inputs_of_message(&pool, &posted.item_id).await, 1);

        let (pool, request, scheduled_message_id) =
            claim_scheduled_review_reply("ready_for_review", &now).await;
        let posted = send_scheduled_review_reply(
            &pool,
            &request,
            &scheduled_message_id,
            ScheduledConsent::Local,
        )
        .await;
        let follow_up_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_ne!(follow_up_id, assignment_id);
        assert_eq!(dispatch_job_count(&pool, &follow_up_id).await, 1);
        let reviewed = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(reviewed.state, ChatWorkAssignmentState::Completed);
    });
}

#[test]
fn unattended_scheduled_mention_of_another_agent_waits_for_start_here() {
    tauri::async_runtime::block_on(async {
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (pool, mut request, scheduled_message_id) =
            claim_scheduled_review_reply("ready_for_review", &now).await;
        seed_other_review_agent(&pool).await;
        request.normalized_markdown = "@Other agent take a look".to_owned();
        request.references = vec![participant_reference(
            "reference:other",
            ChatParticipantId::new("participant:other-agent").unwrap(),
            ChatParticipantKind::AiAgent,
            "Other agent",
            "@Other agent",
            0,
            12,
        )];
        sqlx::query("UPDATE chat_scheduled_messages SET request_data = ? WHERE id = ?")
            .bind(serde_json::to_string(&request).unwrap())
            .bind(scheduled_message_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let reviewed_id = ChatWorkAssignmentId::new("assignment:review").unwrap();

        let posted = send_scheduled_review_reply(
            &pool,
            &request,
            &scheduled_message_id,
            ScheduledConsent::Unattended,
        )
        .await;

        let reviewed = read_assignment(&pool, &reviewed_id).await.unwrap();
        assert_eq!(
            reviewed.state,
            ChatWorkAssignmentState::ReadyForReview,
            "the mention settled the reviewed work of another agent"
        );
        let assignment_id = posted
            .assignment_write
            .assignment_id
            .clone()
            .expect("the mention created no work for the other agent");
        assert_ne!(assignment_id, reviewed_id);
        assert!(posted.assignment_write.awaits_acceptance);
        assert!(!posted.assignment_write.dispatches_here());
        let created = read_assignment(&pool, &assignment_id).await.unwrap();
        assert_eq!(created.state, ChatWorkAssignmentState::Queued);
        assert_eq!(created.agent.id.as_str(), "participant:other-agent");
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 0);
        let writers = chat_writers_of(&["device:desk"]);
        let device = PostingDevice::this_device("device:desk", &writers);
        assert!(
            assignments::assignment_awaits_local_acceptance(
                &pool,
                &assignment_id,
                device,
                &ChatDeviceScope::default(),
            )
            .await
            .unwrap(),
            "the new work waits for Start here"
        );

        let accepted = scope_accepting(&pool, assignment_id.as_str()).await;
        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:desk",
                &accepted
            )
            .await
            .unwrap(),
            "an accepted agent runs through the adoption gate"
        );
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 1);
        let reviewed = read_assignment(&pool, &reviewed_id).await.unwrap();
        assert_eq!(reviewed.state, ChatWorkAssignmentState::ReadyForReview);
    });
}

#[test]
fn a_user_post_accepts_the_work_it_dispatches() {
    tauri::async_runtime::block_on(async {
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
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();

        let digest = post_acceptance_digest(&pool, &posted.assignment_write, None)
            .await
            .unwrap();

        assert_eq!(
            digest,
            Some(consented_dispatch_digest(&pool, &assignment_id).await)
        );
    });
}

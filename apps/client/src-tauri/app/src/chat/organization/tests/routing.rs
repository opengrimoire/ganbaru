//! Steer now and Queue on replies to active work, and how queued replies continue the work.

use super::*;

pub(super) const ROUTED_ASSIGNMENT_ID: &str = "assignment:review";
const ROUTING_TIME: &str = "2026-08-04T17:03:00.000Z";

/// The seeded review work with its stranded replies removed, so only the reply under test
/// routes to it, in `state`.
pub(super) async fn review_work_in(state: &str) -> SqlitePool {
    let pool = migrated_pool().await;
    seed_review_assignment_with_stranded_replies(&pool).await;
    sqlx::query(
        "DELETE FROM chat_work_assignment_inputs
         WHERE assignment_id = ?1 AND routing_kind <> 'trigger'",
    )
    .bind(ROUTED_ASSIGNMENT_ID)
    .execute(&pool)
    .await
    .unwrap();
    set_review_state(&pool, state).await;
    pool
}

pub(super) async fn set_review_state(pool: &SqlitePool, state: &str) {
    sqlx::query("UPDATE chat_work_assignments SET state = ?1 WHERE id = ?2")
        .bind(state)
        .bind(ROUTED_ASSIGNMENT_ID)
        .execute(pool)
        .await
        .unwrap();
}

/// A reply mentioning the review agent, under its own command and reference ids, asking for
/// `routing`.
pub(super) fn reply_request(
    command_id: &str,
    routing: Option<ChatFollowupRouting>,
) -> PostChatMessageCommand {
    let mut request = review_reply_mention(command_id);
    request.followup_routing = routing;
    for reference in &mut request.references {
        if let ChatMessageReference::Participant { metadata, .. } = reference {
            metadata.reference_id =
                ChatMessageReferenceId::new(format!("reference:{command_id}")).unwrap();
        }
    }
    request
}

/// Posts `request` from `device:desk`.
pub(super) async fn post_request(
    pool: &SqlitePool,
    request: &PostChatMessageCommand,
) -> ChatResult<super::super::PersistedPost> {
    let writers = chat_writers_of(&["device:desk"]);
    persist_post(
        pool,
        request,
        None,
        PostingDevice::this_device("device:desk", &writers),
    )
    .await
}

/// Posts a reply mentioning the review agent from `device:desk`, asking for `routing`.
async fn post_reply(
    pool: &SqlitePool,
    command_id: &str,
    routing: Option<ChatFollowupRouting>,
) -> super::super::PersistedPost {
    post_request(pool, &reply_request(command_id, routing))
        .await
        .unwrap()
}

/// Routing kind and delivery state of the input the post `posted` became.
pub(super) async fn routed_input(
    pool: &SqlitePool,
    posted: &super::super::PersistedPost,
) -> (String, String, String) {
    sqlx::query_as(
        "SELECT assignment_id, routing_kind, delivery_state
         FROM chat_work_assignment_inputs WHERE message_item_id = ?",
    )
    .bind(posted.item_id.as_str())
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Follow-ups recovery creates on `device:desk`.
pub(super) async fn recover_on_desk(pool: &SqlitePool) -> u32 {
    let writers = chat_writers_of(&["device:desk"]);
    assignments::recover_stranded_follow_up_assignments(
        pool,
        PostingDevice::this_device("device:desk", &writers),
        &UtcTimestamp::new(ROUTING_TIME).unwrap(),
    )
    .await
    .unwrap()
}

#[test]
fn the_requested_routing_reaches_only_a_working_turn() {
    use ChatFollowupRouting::{Queue, Steer};
    use ChatWorkAssignmentState as State;
    let active = [
        State::Queued,
        State::Working,
        State::WaitingForAnswer,
        State::WaitingForApproval,
    ];
    for state in active {
        let steers = state == State::Working;
        let expected = if steers { Steer } else { Queue };
        assert_eq!(
            super::super::workflow::active_input_routing(state, None),
            expected,
            "{state:?} without a choice"
        );
        assert_eq!(
            super::super::workflow::active_input_routing(state, Some(Steer)),
            expected,
            "Steer now on {state:?}"
        );
        assert_eq!(
            super::super::workflow::active_input_routing(state, Some(Queue)),
            Queue,
            "Queue on {state:?}"
        );
    }
}

#[test]
fn steer_on_working_work_reaches_the_current_turn() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;

        let posted = post_reply(&pool, "command:steer", Some(ChatFollowupRouting::Steer)).await;

        assert_eq!(posted.result.routed_as, Some(ChatFollowupRouting::Steer));
        assert_eq!(
            routed_input(&pool, &posted).await,
            (
                ROUTED_ASSIGNMENT_ID.to_owned(),
                "steer".to_owned(),
                "pending".to_owned()
            )
        );
    });
}

#[test]
fn a_reply_from_a_device_that_cannot_execute_reaches_the_working_run() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        let writers = chat_writers_of(&["device:desk", "device:phone"]);
        let phone = PostingDevice {
            id: "device:phone",
            can_execute: false,
            chat_writers: &writers,
        };

        for (command_id, routing, routing_kind) in [
            ("command:phone-steer", ChatFollowupRouting::Steer, "steer"),
            (
                "command:phone-queue",
                ChatFollowupRouting::Queue,
                "queued_continuation",
            ),
        ] {
            let posted = persist_post(
                &pool,
                &reply_request(command_id, Some(routing)),
                None,
                phone,
            )
            .await
            .unwrap();

            assert_eq!(posted.result.routed_as, Some(routing));
            assert!(posted.assignment_write.input_queued);
            assert!(!posted.assignment_write.executes_here);
            assert_eq!(
                routed_input(&pool, &posted).await,
                (
                    ROUTED_ASSIGNMENT_ID.to_owned(),
                    routing_kind.to_owned(),
                    "pending".to_owned()
                )
            );
        }
    });
}

#[test]
fn steer_requested_while_waiting_routes_as_queued() {
    tauri::async_runtime::block_on(async {
        for state in ["queued", "waiting_for_answer", "waiting_for_approval"] {
            let pool = review_work_in(state).await;

            let posted = post_reply(
                &pool,
                "command:early-steer",
                Some(ChatFollowupRouting::Steer),
            )
            .await;

            assert_eq!(
                posted.result.routed_as,
                Some(ChatFollowupRouting::Queue),
                "{state} work has no turn to steer"
            );
            assert_eq!(routed_input(&pool, &posted).await.1, "queued_continuation");
        }
    });
}

#[test]
fn queued_input_waits_for_turn_end() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;

        let posted = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;

        assert_eq!(posted.result.routed_as, Some(ChatFollowupRouting::Queue));
        let (assignment_id, kind, delivery) = routed_input(&pool, &posted).await;
        assert_eq!(assignment_id, ROUTED_ASSIGNMENT_ID);
        assert_eq!(
            kind, "queued_continuation",
            "the current turn never sees it"
        );
        assert_eq!(delivery, "pending");
        assert_eq!(
            recover_on_desk(&pool).await,
            0,
            "a running turn keeps its queued reply"
        );

        set_review_state(&pool, "completed").await;
        assert_eq!(recover_on_desk(&pool).await, 1);

        let (follow_up_id, kind, delivery) = routed_input(&pool, &posted).await;
        assert_ne!(follow_up_id, ROUTED_ASSIGNMENT_ID);
        assert_eq!(kind, "follow_up", "the queued reply starts the next turn");
        assert_eq!(delivery, "pending");
        let follow_up = read_assignment(&pool, &ChatWorkAssignmentId::new(follow_up_id).unwrap())
            .await
            .unwrap();
        assert_eq!(follow_up.state, ChatWorkAssignmentState::Queued);
        assert_eq!(review_follow_ups(&pool).await, 1);
        assert_eq!(recover_on_desk(&pool).await, 0, "the work continues once");
    });
}

#[test]
fn a_queued_reply_waits_out_of_the_thread_and_enters_it_after_the_turn_output() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        let posted = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;
        let (posted_key, posted_at): (String, String) = sqlx::query_as(
            "SELECT order_key, created_at FROM chat_conversation_items WHERE id = ?",
        )
        .bind(posted.item_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        let waiting = super::super::reads::read_message(&pool, &posted.item_id)
            .await
            .unwrap();
        assert!(
            waiting.awaiting_delivery,
            "a queued reply waits in the queue"
        );
        sqlx::query(
            "INSERT INTO chat_conversation_items
                (id, conversation_id, reply_thread_id, item_kind, order_key, created_at)
             SELECT 'item:turn-output', conversation_id, reply_thread_id, 'work_update',
                    order_key || 'm', created_at
             FROM chat_conversation_items WHERE id = ?",
        )
        .bind(posted.item_id.as_str())
        .execute(&pool)
        .await
        .unwrap();

        set_review_state(&pool, "completed").await;
        assert_eq!(recover_on_desk(&pool).await, 1);

        let (key, created_at): (String, String) = sqlx::query_as(
            "SELECT order_key, created_at FROM chat_conversation_items WHERE id = ?",
        )
        .bind(posted.item_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        let output_key: String = sqlx::query_scalar(
            "SELECT order_key FROM chat_conversation_items WHERE id = 'item:turn-output'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(key > output_key, "the reply follows the turn it waited for");
        assert_ne!(key, posted_key);
        assert_eq!(
            created_at, ROUTING_TIME,
            "the thread shows when it continued the work"
        );
        assert_ne!(created_at, posted_at);
        let continued = super::super::reads::read_message(&pool, &posted.item_id)
            .await
            .unwrap();
        assert!(
            !continued.awaiting_delivery,
            "the follow-up reply is in the thread"
        );
        assert_eq!(
            continued.normalized_markdown, waiting.normalized_markdown,
            "the reply text is unchanged"
        );
    });
}

#[test]
fn a_queued_reply_synced_after_the_turn_ended_continues_the_work() {
    tauri::async_runtime::block_on(async {
        for state in ["ready_for_review", "completed"] {
            let pool = review_work_in("working").await;
            let posted = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;
            // The turn-end hook ran before sync brought the reply, so only the synced work
            // continues it.
            set_review_state(&pool, state).await;
            let writers = chat_writers_of(&["device:desk"]);
            let device = PostingDevice::this_device("device:desk", &writers);
            let assignment_id = ChatWorkAssignmentId::new(ROUTED_ASSIGNMENT_ID).unwrap();
            let now = UtcTimestamp::new(ROUTING_TIME).unwrap();

            let follow_up =
                assignments::continue_stranded_replies(&pool, &assignment_id, device, &now)
                    .await
                    .unwrap()
                    .expect("the synced reply continues the work");

            let (input_assignment, kind, delivery) = routed_input(&pool, &posted).await;
            assert_eq!(input_assignment, follow_up.as_str(), "{state}");
            assert_eq!(kind, "follow_up", "{state}");
            assert_eq!(delivery, "pending", "{state}");
            assert_eq!(
                assignments::continue_stranded_replies(&pool, &assignment_id, device, &now)
                    .await
                    .unwrap(),
                None,
                "{state} work continues once"
            );
        }
    });
}

#[test]
fn later_queued_replies_reach_the_next_turn_together() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        let first = post_reply(&pool, "command:queue-1", Some(ChatFollowupRouting::Queue)).await;
        let second = post_reply(&pool, "command:queue-2", Some(ChatFollowupRouting::Queue)).await;

        set_review_state(&pool, "completed").await;
        assert_eq!(recover_on_desk(&pool).await, 1);

        let (first_assignment, first_kind, _) = routed_input(&pool, &first).await;
        let (second_assignment, second_kind, _) = routed_input(&pool, &second).await;
        assert_eq!(first_assignment, second_assignment);
        assert_eq!(first_kind, "follow_up");
        assert_eq!(
            second_kind, "steer",
            "the carried reply joins the first turn of the follow-up"
        );
    });
}

#[test]
fn settled_work_without_a_result_keeps_its_queued_replies() {
    tauri::async_runtime::block_on(async {
        for state in ["cancelled", "failed"] {
            let pool = review_work_in("working").await;
            let posted = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;

            set_review_state(&pool, state).await;

            assert_eq!(
                recover_on_desk(&pool).await,
                0,
                "{state} work does not continue"
            );
            assert_eq!(review_follow_ups(&pool).await, 0);
            assert_eq!(routed_input(&pool, &posted).await.0, ROUTED_ASSIGNMENT_ID);
        }
    });
}

#[test]
fn reviewed_or_continued_work_does_not_continue_again() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;
        sqlx::query(
            "UPDATE chat_work_assignments
             SET state = 'completed', review_outcome = 'accepted',
                 reviewed_by_participant_id = ?1, reviewed_at = ?2
             WHERE id = ?3",
        )
        .bind(LOCAL_PARTICIPANT_ID)
        .bind(ROUTING_TIME)
        .bind(ROUTED_ASSIGNMENT_ID)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(recover_on_desk(&pool).await, 0, "accepted work stays done");

        let pool = review_work_in("working").await;
        post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;
        set_review_state(&pool, "completed").await;
        sqlx::query(
            "INSERT INTO chat_work_assignments
                 (id, reply_thread_id, agent_id, triggering_message_item_id,
                 model_selection_schema_version, model_selection_data,
                  executing_device_id, state, previous_assignment_id, created_at, updated_at)
             VALUES ('assignment:successor', 'reply-thread:review', 'participant:review-agent',
                     'item:review-reply-1', 1, json_object('providerManagedModel', json('true')), 'device:desk', 'queued', ?1, ?2, ?2)",
        )
        .bind(ROUTED_ASSIGNMENT_ID)
        .bind(ROUTING_TIME)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            recover_on_desk(&pool).await,
            0,
            "work that already continued leaves its replies to the successor"
        );
    });
}

#[test]
fn a_replayed_post_reports_its_routing() {
    tauri::async_runtime::block_on(async {
        let pool = review_work_in("working").await;
        let posted = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;

        let replayed = post_reply(&pool, "command:queue", Some(ChatFollowupRouting::Queue)).await;

        assert_eq!(replayed.item_id, posted.item_id);
        assert_eq!(replayed.result.routed_as, Some(ChatFollowupRouting::Queue));
    });
}

#[test]
fn a_scheduled_message_cannot_choose_its_routing() {
    tauri::async_runtime::block_on(async {
        let mut request = message("Later");
        assert!(super::super::scheduling::refuse_scheduled_post_choices(&request).is_ok());
        request.followup_routing = Some(ChatFollowupRouting::Queue);
        let error = super::super::scheduling::refuse_scheduled_post_choices(&request).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("followupRouting"));

        let now = UtcTimestamp::new(ROUTING_TIME).unwrap();
        let (pool, mut request, scheduled_message_id) =
            claim_scheduled_review_reply("working", &now).await;
        request.followup_routing = Some(ChatFollowupRouting::Steer);
        sqlx::query("UPDATE chat_scheduled_messages SET request_data = ? WHERE id = ?")
            .bind(serde_json::to_string(&request).unwrap())
            .bind(scheduled_message_id.as_str())
            .execute(&pool)
            .await
            .unwrap();
        let writers = chat_writers_of(&["device:desk"]);
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            occurrence: None,
        };
        let Err(error) = write_post(
            &pool,
            &request,
            Some(send),
            None,
            PostingDevice::this_device("device:desk", &writers),
        )
        .await
        else {
            panic!("a scheduled send with a routing choice posted");
        };
        assert_eq!(error.field.as_deref(), Some("followupRouting"));
    });
}

/// Stores the linked device `device_id` of `kind`, numbered `index` for its unique keys.
async fn insert_linked_device(pool: &SqlitePool, index: u8, device_id: &str, kind: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO sync_spaces (space_id, vault_id, created_at_ms)
         VALUES (zeroblob(16), 'vault', 0)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sync_devices
            (space_id, device_id, device_key, tls_fingerprint, kind, role, enrolled_revision)
         VALUES (zeroblob(16), ?1, ?2, ?3, ?4, 'member', 0)",
    )
    .bind(device_id)
    .bind(vec![index; 32])
    .bind(vec![index; 32])
    .bind(kind)
    .execute(pool)
    .await
    .unwrap();
}

/// Posts a reply mentioning the review agent from `device:phone`, with `writers` writing Chat,
/// and returns the executing device of the work it created, if any.
async fn mention_from_phone(pool: &SqlitePool, writers: &[&str]) -> Option<String> {
    let writers = chat_writers_of(writers);
    let phone = PostingDevice {
        id: "device:phone",
        can_execute: false,
        chat_writers: &writers,
    };
    let posted = persist_post(pool, &review_reply_mention("command:phone"), None, phone)
        .await
        .unwrap();
    assert!(!posted.assignment_write.executes_here);
    let assignment_id = posted.assignment_write.assignment_id?;
    super::super::workflow::read_assignment_executing_device(pool, assignment_id.as_str())
        .await
        .unwrap()
}

#[test]
fn a_phone_mention_runs_on_the_desktop_that_last_ran_the_agent() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "completed").await;
        insert_linked_device(&pool, 1, "device:old", "desktop").await;
        insert_linked_device(&pool, 2, "device:other", "desktop").await;

        let executor =
            mention_from_phone(&pool, &["device:phone", "device:old", "device:other"]).await;

        assert_eq!(executor.as_deref(), Some("device:old"));
    });
}

#[test]
fn a_phone_mention_skips_a_past_executor_that_no_longer_writes_chat() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "completed").await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;

        let executor = mention_from_phone(&pool, &["device:phone", "device:desk"]).await;

        assert_eq!(
            executor.as_deref(),
            Some("device:desk"),
            "the only linked desktop runs it"
        );
    });
}

#[test]
fn a_phone_mention_without_history_runs_only_when_one_desktop_is_linked() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;
        insert_linked_device(&pool, 2, "device:server", "server").await;
        assert_eq!(
            mention_from_phone(&pool, &["device:phone", "device:desk", "device:server"])
                .await
                .as_deref(),
            Some("device:desk")
        );

        let pool = review_thread_without_assignment().await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;
        insert_linked_device(&pool, 2, "device:laptop", "desktop").await;
        assert_eq!(
            mention_from_phone(&pool, &["device:phone", "device:desk", "device:laptop"]).await,
            None,
            "with two desktops and no history the post records the mention only"
        );
    });
}

#[test]
fn a_phone_mention_ignores_work_that_never_started_on_a_desktop() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        seed_assignment_on_old_owner(&pool, "cancelled").await;
        insert_linked_device(&pool, 1, "device:old", "desktop").await;
        insert_linked_device(&pool, 2, "device:other", "desktop").await;

        assert_eq!(
            mention_from_phone(&pool, &["device:phone", "device:old", "device:other"]).await,
            None,
            "work that never ran does not show the agent runs on that desktop"
        );
    });
}

#[test]
fn a_phone_mention_starts_on_the_desktop_it_names() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;
        let writers = chat_writers_of(&["device:phone", "device:desk"]);
        let phone = PostingDevice {
            id: "device:phone",
            can_execute: false,
            chat_writers: &writers,
        };
        let posted = persist_post(&pool, &review_reply_mention("command:phone"), None, phone)
            .await
            .unwrap();
        let assignment_id = posted.assignment_write.assignment_id.unwrap();

        assert!(
            assignments::adopt_synced_assignment(
                &pool,
                &assignment_id,
                &test_providers(),
                "device:desk"
            )
            .await
            .unwrap(),
            "the named desktop queues the work without anything frozen by the phone"
        );
        let now = super::now_timestamp().unwrap();
        assert!(
            super::dispatch::claim_dispatch_job(
                &pool,
                &assignment_id,
                "device:desk",
                &test_providers(),
                &now
            )
            .await
            .unwrap()
            .is_some(),
            "the desktop starts the work"
        );
        let state: String =
            sqlx::query_scalar("SELECT state FROM chat_work_assignments WHERE id = ?")
                .bind(assignment_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_ne!(state, "failed");
    });
}

/// Publishes from `device_id` that it can run the test provider family, with one model.
async fn publish_desktop_status(pool: &SqlitePool, device_id: &str, available: bool) {
    let catalog = ProviderModelCatalog {
        instance_id: ProviderInstanceId::new(TEST_INSTANCE.to_string()).unwrap(),
        models: vec![ProviderModel {
            id: ModelId::new("gpt-5".to_string()).unwrap(),
            display_name: "GPT-5".to_string(),
            description: None,
            context_limit: None,
            availability: ModelAvailability::Available,
            capabilities: Vec::new(),
            options: Vec::new(),
        }],
        recommended_model_id: None,
        source: ModelCatalogSource::Provider,
        discovered_at: UtcTimestamp::new(ROUTING_TIME).unwrap(),
        stale: false,
    };
    let statuses = std::collections::BTreeMap::from([(
        TEST_FAMILY.to_string(),
        ganbaru_chat::provider_status::ProviderStatusContent::new(available, Some(&catalog), None),
    )]);
    ganbaru_chat::provider_status::publish_provider_statuses(
        pool,
        device_id,
        &statuses,
        &UtcTimestamp::new(ROUTING_TIME).unwrap(),
    )
    .await
    .unwrap();
}

#[test]
fn a_phone_agent_read_shows_the_status_of_the_desktop_that_would_run_it() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;
        super::takeover::insert_sync_writer(
            &pool,
            1,
            "device:desk",
            ganbaru_sync::WriterState::Active,
        )
        .await;
        publish_desktop_status(&pool, "device:desk", true).await;
        let agent_id = ChatParticipantId::new("participant:review-agent".to_string()).unwrap();
        let phone =
            super::super::reads::LocalProviderInstances::linked_reader("device:phone".to_string());

        let read = super::super::reads::read_agent(&pool, &agent_id, &phone)
            .await
            .unwrap();
        let linked = read
            .linked_provider_status
            .expect("the desktop shares its status");
        assert_eq!(linked.device_id, "device:desk");
        assert!(linked.available);
        assert_eq!(linked.models[0].display_name, "GPT-5");

        let desktop = super::super::reads::LocalProviderInstances::new();
        let read = super::super::reads::read_agent(&pool, &agent_id, &desktop)
            .await
            .unwrap();
        assert!(
            read.linked_provider_status.is_none(),
            "a device that runs agents reads no linked status"
        );
    });
}

#[test]
fn a_phone_agent_read_ignores_a_desktop_that_no_longer_writes_chat() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        insert_linked_device(&pool, 1, "device:desk", "desktop").await;
        super::takeover::insert_sync_writer(
            &pool,
            1,
            "device:desk",
            ganbaru_sync::WriterState::Revoked { cutoff: 0 },
        )
        .await;
        publish_desktop_status(&pool, "device:desk", true).await;
        let agent_id = ChatParticipantId::new("participant:review-agent".to_string()).unwrap();
        let phone =
            super::super::reads::LocalProviderInstances::linked_reader("device:phone".to_string());

        let read = super::super::reads::read_agent(&pool, &agent_id, &phone)
            .await
            .unwrap();
        assert!(read.linked_provider_status.is_none());
    });
}

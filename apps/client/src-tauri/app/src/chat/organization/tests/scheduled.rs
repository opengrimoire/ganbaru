//! Scheduled messages: delivery times, claims and sends, request mismatches, consent, and edits.

use super::*;

#[test]
fn scheduled_delivery_requires_a_near_future_time_within_one_year() {
    let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();

    assert!(
        validate_scheduled_for(
            &UtcTimestamp::new("2026-08-03T00:00:30.000Z").unwrap(),
            &now,
        )
        .is_ok()
    );
    assert_eq!(
        validate_scheduled_for(
            &UtcTimestamp::new("2026-08-03T00:00:29.999Z").unwrap(),
            &now,
        )
        .unwrap_err()
        .field
        .as_deref(),
        Some("scheduledFor"),
    );
    assert_eq!(
        validate_scheduled_for(
            &UtcTimestamp::new("2027-08-05T00:00:00.000Z").unwrap(),
            &now,
        )
        .unwrap_err()
        .field
        .as_deref(),
        Some("scheduledFor"),
    );
}

#[test]
fn immediate_scheduled_delivery_claim_is_atomic() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Send this now")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        let claimed = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:first",
        )
        .await
        .unwrap();
        assert_eq!(claimed, request_data);
        let row = sqlx::query(
            "SELECT attempt_count, claim_token
             FROM chat_scheduled_message_dispatch WHERE scheduled_message_id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<i64, _>("attempt_count"), 1);
        assert_eq!(row.get::<String, _>("claim_token"), "claim:first");
        let duplicate = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:second",
        )
        .await
        .unwrap_err();
        assert_eq!(duplicate.code, ChatErrorCode::Busy);
    });
}

#[test]
fn only_the_executing_device_sends_a_scheduled_message() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Send from the desk")).unwrap();
        // The scheduled message arrived through sync: this device keeps no dispatch row.
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), false, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();

        let refused = claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            "device:phone",
            &now,
            "claim:phone",
        )
        .await
        .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        scheduling::prepare_scheduled_dispatch(&pool, "device:phone", &now)
            .await
            .unwrap();
        let dispatch_rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_scheduled_message_dispatch")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(dispatch_rows, 0);

        // The executing device adopts the message even without its dispatch row.
        scheduling::prepare_scheduled_dispatch(&pool, SCHEDULING_DEVICE, &now)
            .await
            .unwrap();
        scheduling::require_scheduled_executor(&pool, &scheduled_message_id, SCHEDULING_DEVICE)
            .await
            .unwrap();
        let available_at: String = sqlx::query_scalar(
            "SELECT available_at FROM chat_scheduled_message_dispatch
             WHERE scheduled_message_id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(available_at, "2026-08-04T00:00:00.000Z");
    });
}

#[test]
fn a_scheduled_message_without_an_executing_device_is_sent_where_it_dispatches() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Older schedule")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, None, false, &now).await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();

        let refused =
            scheduling::require_scheduled_executor(&pool, &scheduled_message_id, "device:any")
                .await
                .unwrap_err();
        assert_eq!(refused.code, ChatErrorCode::Conflict);
        sqlx::query(
            "INSERT INTO chat_scheduled_message_dispatch
                (scheduled_message_id, attempt_count, available_at, updated_at)
             VALUES ('scheduled:test', 0, ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        scheduling::require_scheduled_executor(&pool, &scheduled_message_id, "device:any")
            .await
            .unwrap();
    });
}

#[test]
fn scheduled_content_must_fit_one_sync_operation() {
    let request = message("Short");
    let serialized = serde_json::to_string(&request).unwrap();
    scheduling::validate_scheduled_content(&serialized, &request.references).unwrap();

    let oversized = "x".repeat(ganbaru_sync::domains::chat::MAX_SCHEDULED_REQUEST_BYTES + 1);
    let error = scheduling::validate_scheduled_content(&oversized, &[]).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("message"));

    let mention = participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiAgent,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    let mentions = vec![mention; ganbaru_sync::domains::chat::MAX_SCHEDULED_REFERENCES + 1];
    let error = scheduling::validate_scheduled_content(&serialized, &mentions).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("references"));
}

#[test]
fn scheduled_send_posts_stored_attachment_values_under_derived_ids() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let mut request = message("Send the screenshot");
        request.attachment_ids = vec![ChatAttachmentId::new("attachment:gone").unwrap()];
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        // The device row of the attachment is absent, as after its working folder was deleted
        // or on a device that received the scheduled message through sync.
        let digest = "a".repeat(64);
        sqlx::query(
            "INSERT INTO chat_scheduled_message_attachment_references
                (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
                 original_display_name, mime_type, byte_size, sha256)
             VALUES ('scheduled:test', 0, 'attachment:gone', 'working-folder:gone', 'image',
                     'screen.png', 'image/png', 42, ?)",
        )
        .bind(&digest)
        .execute(&pool)
        .await
        .unwrap();
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };
        let writers = chat_writers_of(&[SCHEDULING_DEVICE]);
        let device = PostingDevice::this_device(SCHEDULING_DEVICE, &writers);

        let posted = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();

        let expected = super::common::scheduled_post_ids(
            "scheduled:test",
            super::common::ScheduledPostTarget::Message,
            None,
        )
        .unwrap();
        assert_eq!(posted.item_id, expected.item_id);
        // The receipt is keyed by the replicated command id and names the derived post, so
        // every device that sends or reads it agrees on the post.
        let (receipt_state, receipt_data): (String, String) = sqlx::query_as(
            "SELECT state, result_data FROM chat_organizational_command_receipts
             WHERE client_command_id = ?",
        )
        .bind(request.client_command_id.as_str())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(receipt_state, "completed");
        assert!(receipt_data.contains(expected.item_id.as_str()));
        let reference: (String, String, String, String, i64, String) = sqlx::query_as(
            "SELECT message_revision_id, attachment_id, working_folder_id, original_display_name,
                    byte_size, sha256
             FROM chat_communication_attachment_references",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            reference,
            (
                expected.revision_id.as_str().to_owned(),
                "attachment:gone".to_owned(),
                "working-folder:gone".to_owned(),
                "screen.png".to_owned(),
                42,
                digest,
            )
        );
        let scheduled_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(scheduled_rows, 0);

        let repeated = persist_post(&pool, &request, Some(send), device)
            .await
            .unwrap();
        assert_eq!(repeated.item_id, expected.item_id);
        let items: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 1);
    });
}

#[test]
fn scheduled_send_rejects_a_claim_that_changed() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request = message("Send this once");
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:stale",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };

        let Err(error) = persist_post(
            &pool,
            &request,
            Some(send),
            PostingDevice::this_device(SCHEDULING_DEVICE, &chat_writers_of(&[SCHEDULING_DEVICE])),
        )
        .await
        else {
            panic!("a stale claim posted");
        };

        assert_eq!(error.code, ChatErrorCode::Conflict);
        let items: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 0);
    });
}

#[test]
fn scheduled_request_mismatch_refuses_foreign_channel() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        sqlx::query(
            "INSERT INTO chat_conversations
                (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation:foreign', 'project:test', 'channel', ?1, ?1, ?1);
             INSERT INTO chat_channels (id, project_id, conversation_id, name, created_at, updated_at)
             VALUES ('channel:foreign', 'project:test', 'conversation:foreign', 'foreign', ?1, ?1)",
        )
        .bind(now.as_str())
        .execute(&pool)
        .await
        .unwrap();
        let mut request = message("Post this elsewhere");
        request.channel_id = ChatChannelId::new("channel:foreign").unwrap();
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };
        let writers = chat_writers_of(&[SCHEDULING_DEVICE]);
        let device = PostingDevice::this_device(SCHEDULING_DEVICE, &writers);

        let outcome = write_post(&pool, &request, Some(send), None, device)
            .await
            .unwrap();

        let PostOutcome::Refused(refusal) = outcome else {
            panic!("the mismatched request was not refused");
        };
        assert_eq!(refusal.code, ChatErrorCode::Conflict);
        let items: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_conversation_items")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(items, 0, "the mismatched request posted");
        let (failed_at, last_error): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT failed_at, last_error FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            failed_at.is_some(),
            "the refusal fails the message for good"
        );
        assert_eq!(
            last_error.as_deref(),
            Some(SCHEDULED_REQUEST_MISMATCH_REASON)
        );
    });
}

#[test]
fn scheduled_write_post_sends_a_matching_request() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request = message("Send this on time");
        let request_data = serde_json::to_string(&request).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };
        let writers = chat_writers_of(&[SCHEDULING_DEVICE]);
        let device = PostingDevice::this_device(SCHEDULING_DEVICE, &writers);

        let outcome = write_post(&pool, &request, Some(send), None, device)
            .await
            .unwrap();

        assert!(
            matches!(outcome, PostOutcome::Posted(_)),
            "the matching request was refused"
        );
        let scheduled_rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(scheduled_rows, 0);
    });
}

#[test]
fn scheduled_request_mismatch_names_the_field_that_differs() {
    let stored_thread = ChatReplyThreadId::new("thread:stored").unwrap();
    let mut request = message("@Review agent check this");
    request.reply_thread_id = Some(stored_thread.clone());
    request.references = vec![participant_reference(
        "reference:scheduled",
        ChatParticipantId::new("participant:review-agent").unwrap(),
        ChatParticipantKind::AiAgent,
        "Review agent",
        "@Review agent",
        0,
        13,
    )];
    let references: Vec<_> = request
        .references
        .iter()
        .map(scheduled_reference_target)
        .collect();
    let client_command_id = request.client_command_id.as_str().to_owned();
    let channel_id = request.channel_id.as_str().to_owned();
    let row = ScheduledRowBinding {
        client_command_id: &client_command_id,
        channel_id: &channel_id,
        reply_thread_id: Some(stored_thread.as_str()),
        references: &references,
    };
    assert_eq!(scheduled_request_mismatch(&row, &request), None);

    let mut foreign = request.clone();
    foreign.channel_id = ChatChannelId::new("channel:foreign").unwrap();
    assert_eq!(
        scheduled_request_mismatch(&row, &foreign),
        Some("channelId")
    );

    let mut rethreaded = request.clone();
    rethreaded.reply_thread_id = None;
    assert_eq!(
        scheduled_request_mismatch(&row, &rethreaded),
        Some("replyThreadId")
    );

    let mut recommanded = request.clone();
    recommanded.client_command_id = ChatCommandId::new("command:foreign").unwrap();
    assert_eq!(
        scheduled_request_mismatch(&row, &recommanded),
        Some("clientCommandId")
    );

    let mut retargeted = request.clone();
    retargeted.references = vec![participant_reference(
        "reference:scheduled",
        ChatParticipantId::new("participant:other-agent").unwrap(),
        ChatParticipantKind::AiAgent,
        "Review agent",
        "@Review agent",
        0,
        13,
    )];
    assert_eq!(
        scheduled_request_mismatch(&row, &retargeted),
        Some("references")
    );

    let mut unmentioned = request.clone();
    unmentioned.references.clear();
    assert_eq!(
        scheduled_request_mismatch(&row, &unmentioned),
        Some("references")
    );

    let mut extended = request.clone();
    extended.references.push(extended.references[0].clone());
    assert_eq!(
        scheduled_request_mismatch(&row, &extended),
        Some("references")
    );
}

#[test]
fn scheduled_request_mismatch_passes_a_row_with_its_stored_references() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, scheduled_message_id) = claim_scheduled_mention(&pool, &now).await;
        let mut transaction = pool.begin().await.unwrap();
        insert_scheduled_reference(
            &mut transaction,
            &scheduled_message_id,
            &request.references[0],
            0,
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        let send = ScheduledSend {
            scheduled_message_id: &scheduled_message_id,
            claim_token: "claim:a",
            consent: ScheduledConsent::Local,
            occurrence: None,
        };

        let refusal = refuse_mismatched_request(&pool, send, &request)
            .await
            .unwrap();

        assert!(refusal.is_none(), "the honest request was refused");
        let failed_at: Option<String> =
            sqlx::query_scalar("SELECT failed_at FROM chat_scheduled_messages WHERE id = ?")
                .bind(scheduled_message_id.as_str())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(failed_at, None);
    });
}

#[test]
fn scheduled_consent_covers_only_the_request_it_was_given_for() {
    let id = ChatScheduledMessageId::new("scheduled:test").unwrap();
    let other = ChatScheduledMessageId::new("scheduled:other").unwrap();
    let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
    let mut scope = ChatDeviceScope::default();
    assert_eq!(
        scheduled_consent_in(&scope, &id, "request", None),
        ScheduledConsent::Unattended
    );

    record_scheduled_message_consent(
        &mut scope,
        id.as_str().to_owned(),
        scheduled_consent_digest(id.as_str(), "request"),
        now,
    );

    assert_eq!(
        scheduled_consent_in(&scope, &id, "request", None),
        ScheduledConsent::Local
    );
    assert!(
        scope.assignment_acceptances.is_empty(),
        "a scheduled consent took an assignment acceptance slot"
    );
    assert_eq!(
        scheduled_consent_in(&scope, &id, "rewritten request", None),
        ScheduledConsent::Unattended,
        "a request rewritten on a peer kept the consent"
    );
    assert_eq!(
        scheduled_consent_in(&scope, &other, "request", None),
        ScheduledConsent::Unattended,
        "the consent reached another message"
    );

    forget_scheduled_message_consent(
        &mut scope,
        id.as_str(),
        &scheduled_consent_digest(id.as_str(), "rewritten request"),
    );
    assert_eq!(
        scheduled_consent_in(&scope, &id, "request", None),
        ScheduledConsent::Local,
        "forgetting another request dropped the consent"
    );
    forget_scheduled_message_consent(
        &mut scope,
        id.as_str(),
        &scheduled_consent_digest(id.as_str(), "request"),
    );

    assert_eq!(
        scheduled_consent_in(&scope, &id, "request", None),
        ScheduledConsent::Unattended
    );
}

#[test]
fn scheduled_consent_to_a_message_that_is_gone_is_swept() {
    tauri::async_runtime::block_on(async {
        let pool = review_thread_without_assignment().await;
        let now = UtcTimestamp::new("2026-08-04T17:03:00.000Z").unwrap();
        let (request, pending) = claim_scheduled_mention(&pool, &now).await;
        let request_data = serde_json::to_string(&request).unwrap();
        let mut scope = ChatDeviceScope::default();
        for id in [pending.as_str(), "scheduled:cancelled"] {
            record_scheduled_message_consent(
                &mut scope,
                id.to_owned(),
                scheduled_consent_digest(id, &request_data),
                now.clone(),
            );
        }

        let stale = scheduled_consents_without_a_row(&pool, &scope)
            .await
            .unwrap();

        assert_eq!(
            stale,
            vec![(
                "scheduled:cancelled".to_owned(),
                scheduled_consent_digest("scheduled:cancelled", &request_data)
            )],
            "consent to a message that no longer exists was kept"
        );
    });
}

#[test]
fn consented_scheduled_post_queues_its_dispatch() {
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

        assert!(!posted.assignment_write.awaits_acceptance);
        let assignment_id = posted.assignment_write.assignment_id.clone().unwrap();
        assert_eq!(dispatch_job_count(&pool, &assignment_id).await, 1);
    });
}

/// An edit of `scheduled:test` to `markdown`, sent once at `scheduled_for`.
fn scheduled_edit(markdown: &str, scheduled_for: &str) -> UpdateScheduledChatMessageCommand {
    let content = message(markdown);
    UpdateScheduledChatMessageCommand {
        scheduled_message_id: ChatScheduledMessageId::new("scheduled:test").unwrap(),
        scheduled_for: UtcTimestamp::new(scheduled_for).unwrap(),
        normalized_markdown: content.normalized_markdown,
        rich_content: content.rich_content,
        attachment_ids: Vec::new(),
        references: Vec::new(),
        also_send_to_channel: true,
        recurrence: None,
    }
}

async fn scheduled_available_at(pool: &SqlitePool) -> String {
    sqlx::query_scalar(
        "SELECT available_at FROM chat_scheduled_message_dispatch
         WHERE scheduled_message_id = 'scheduled:test'",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[test]
fn editing_a_scheduled_message_replaces_it_in_place_and_moves_its_delivery() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Draft wording")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let mut consents = Vec::new();

        let edited = scheduling::apply_scheduled_update(
            &pool,
            SCHEDULING_DEVICE,
            scheduled_edit("Final wording", "2026-08-05T09:00:00.000Z"),
            &now,
            |id, request_data, spec| {
                consents.push((
                    id.as_str().to_owned(),
                    request_data.to_owned(),
                    spec.cloned(),
                ));
            },
        )
        .await
        .unwrap();

        assert_eq!(edited.id.as_str(), "scheduled:test");
        assert_eq!(edited.normalized_markdown, "Final wording");
        assert!(edited.also_send_to_channel);
        assert_eq!(edited.scheduled_for.as_str(), "2026-08-05T09:00:00.000Z");
        let stored: PostChatMessageCommand = serde_json::from_str(
            &sqlx::query_scalar::<_, String>(
                "SELECT request_data FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(stored.client_command_id.as_str(), "command:test");
        assert_eq!(stored.normalized_markdown, "Final wording");
        assert_eq!(
            scheduled_available_at(&pool).await,
            "2026-08-05T09:00:00.000Z"
        );
        // The executing device consents to the request it now holds.
        assert_eq!(consents.len(), 1);
        assert_eq!(consents[0].0, "scheduled:test");
        assert_eq!(consents[0].1, serde_json::to_string(&stored).unwrap());
        assert_eq!(consents[0].2, None);

        // An edit on another device leaves the executing device without consent to it.
        let mut elsewhere = 0;
        scheduling::apply_scheduled_update(
            &pool,
            "device:phone",
            scheduled_edit("Phone wording", "2026-08-05T09:00:00.000Z"),
            &now,
            |_, _, _| elsewhere += 1,
        )
        .await
        .unwrap();
        assert_eq!(elsewhere, 0);
    });
}

#[test]
fn editing_a_scheduled_message_refuses_one_being_sent_or_gone() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Draft wording")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        let scheduled_message_id = ChatScheduledMessageId::new("scheduled:test").unwrap();
        claim_scheduled_message_for_immediate_send(
            &pool,
            &scheduled_message_id,
            SCHEDULING_DEVICE,
            &now,
            "claim:a",
        )
        .await
        .unwrap();

        let busy = scheduling::apply_scheduled_update(
            &pool,
            SCHEDULING_DEVICE,
            scheduled_edit("Too late", "2026-08-05T09:00:00.000Z"),
            &now,
            |_, _, _| panic!("a refused edit gives no consent"),
        )
        .await
        .unwrap_err();
        assert_eq!(busy.code, ChatErrorCode::Busy);
        let stored: String = sqlx::query_scalar(
            "SELECT request_data FROM chat_scheduled_messages WHERE id = 'scheduled:test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, request_data);

        let mut missing = scheduled_edit("Nothing", "2026-08-05T09:00:00.000Z");
        missing.scheduled_message_id = ChatScheduledMessageId::new("scheduled:gone").unwrap();
        let gone = scheduling::apply_scheduled_update(
            &pool,
            SCHEDULING_DEVICE,
            missing,
            &now,
            |_, _, _| (),
        )
        .await
        .unwrap_err();
        assert_eq!(gone.code, ChatErrorCode::NotFound);
    });
}

#[test]
fn a_delivery_time_edited_elsewhere_moves_the_local_due_time() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let now = UtcTimestamp::new("2026-08-03T00:00:00.000Z").unwrap();
        seed_test_channel(&pool, &now).await;
        let request_data = serde_json::to_string(&message("Send from the desk")).unwrap();
        insert_test_scheduled_message(&pool, &request_data, Some(SCHEDULING_DEVICE), true, &now)
            .await;
        // An edit from another device arrives through sync, which leaves the dispatch row alone.
        let move_to = |scheduled_for: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "UPDATE chat_scheduled_messages SET scheduled_for = ?
                     WHERE id = 'scheduled:test'",
                )
                .bind(scheduled_for)
                .execute(&pool)
                .await
                .unwrap();
            }
        };

        move_to("2026-08-06T00:00:00.000Z").await;
        scheduling::prepare_scheduled_dispatch(&pool, SCHEDULING_DEVICE, &now)
            .await
            .unwrap();
        assert_eq!(
            scheduled_available_at(&pool).await,
            "2026-08-06T00:00:00.000Z"
        );

        // A row waiting to retry keeps its later retry time when the delivery moves earlier.
        sqlx::query(
            "UPDATE chat_scheduled_message_dispatch
             SET attempt_count = 1, available_at = '2026-08-06T00:05:00.000Z'",
        )
        .execute(&pool)
        .await
        .unwrap();
        move_to("2026-08-05T00:00:00.000Z").await;
        scheduling::prepare_scheduled_dispatch(&pool, SCHEDULING_DEVICE, &now)
            .await
            .unwrap();
        assert_eq!(
            scheduled_available_at(&pool).await,
            "2026-08-06T00:05:00.000Z"
        );
        move_to("2026-08-07T00:00:00.000Z").await;
        scheduling::prepare_scheduled_dispatch(&pool, SCHEDULING_DEVICE, &now)
            .await
            .unwrap();
        assert_eq!(
            scheduled_available_at(&pool).await,
            "2026-08-07T00:00:00.000Z"
        );
    });
}

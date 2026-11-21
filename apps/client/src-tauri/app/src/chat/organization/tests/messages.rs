//! Message validation: participant references, mentions, thread eligibility, and search terms.

use super::*;

#[test]
fn plain_at_text_is_valid_but_does_not_invoke_a_agent() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        let request = message("Please ask @atlas to review this.");
        validate_message_request(&request).unwrap();
        let invocation = resolve_invoked_agent(
            &pool,
            &ChatConversationId::new("conversation:test").unwrap(),
            None,
            &request.references,
        )
        .await
        .unwrap();
        assert!(invocation.is_none());
    });
}

#[test]
fn only_structured_participant_references_make_top_level_messages_thread_eligible() {
    let mut request = message("Ordinary channel message");
    assert!(!has_thread_eligible_mention(&request));

    request.normalized_markdown = "@Atlas Please review this.".to_string();
    request.references = vec![participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiAgent,
        "Atlas",
        "@Atlas",
        0,
        6,
    )];

    assert!(has_thread_eligible_mention(&request));
}

#[test]
fn mentioning_a_person_or_yourself_opens_a_thread_but_a_folder_alone_does_not() {
    for kind in [ChatParticipantKind::LocalUser, ChatParticipantKind::Human] {
        let mut request = message("@Me Remember this.");
        request.references = vec![participant_reference(
            "reference:me",
            ChatParticipantId::new("participant:me").unwrap(),
            kind,
            "Me",
            "@Me",
            0,
            3,
        )];
        assert!(has_thread_eligible_mention(&request));
    }

    let folder = ChatMessageReference::WorkingFolder {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new("reference:folder").unwrap(),
            label_snapshot: "app".to_string(),
            start_offset: 0,
            end_offset: 4,
            plain_text_projection: "#app".to_string(),
        },
        working_folder_id: ProjectWorkingFolderId::new("folder:app").unwrap(),
    };
    let mut request = message("#app builds again.");
    request.references = vec![folder];
    assert!(!has_thread_eligible_mention(&request));
}

#[test]
fn active_continuations_reject_new_authority_bearing_references() {
    let participant = participant_reference(
        "reference:atlas",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiAgent,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    assert!(!has_authority_bearing_references(std::slice::from_ref(
        &participant
    )));
    require_continuation_scope_is_unchanged(true, &[participant]).unwrap();

    let channel = ChatMessageReference::Channel {
        metadata: ChatReferenceMetadata {
            reference_id: ChatMessageReferenceId::new("reference:source-channel").unwrap(),
            label_snapshot: "source".to_string(),
            start_offset: 0,
            end_offset: 7,
            plain_text_projection: "#source".to_string(),
        },
        channel_id: ChatChannelId::new("channel:source").unwrap(),
    };
    assert!(has_authority_bearing_references(std::slice::from_ref(
        &channel
    )));
    let error =
        require_continuation_scope_is_unchanged(true, std::slice::from_ref(&channel)).unwrap_err();
    assert_eq!(error.code, ChatErrorCode::Conflict);
    require_continuation_scope_is_unchanged(false, &[channel]).unwrap();
}

#[test]
fn two_structured_ai_mentions_are_rejected_before_dispatch() {
    tauri::async_runtime::block_on(async {
        let pool = migrated_pool().await;
        for index in 1..=2 {
            sqlx::query(
                "INSERT INTO chat_participants
                    (id, participant_kind, display_name, created_at, updated_at)
                 VALUES (?, 'ai_agent', ?, ?, ?)",
            )
            .bind(format!("participant:agent-{index}"))
            .bind(format!("Agent {index}"))
            .bind("2026-08-01T00:00:00.000Z")
            .bind("2026-08-01T00:00:00.000Z")
            .execute(&pool)
            .await
            .unwrap();
        }
        let references = (1..=2)
            .map(|index| {
                participant_reference(
                    &format!("reference:agent-{index}"),
                    ChatParticipantId::new(format!("participant:agent-{index}")).unwrap(),
                    ChatParticipantKind::AiAgent,
                    &format!("Agent {index}"),
                    &format!("@Agent {index}"),
                    u64::try_from((index - 1) * 9).unwrap(),
                    u64::try_from(index * 9).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let error = resolve_invoked_agent(
            &pool,
            &ChatConversationId::new("conversation:test").unwrap(),
            None,
            &references,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Validation);
        assert_eq!(error.field.as_deref(), Some("references"));
    });
}

#[test]
fn message_validation_rejects_duplicate_atomic_mention_ranges() {
    let mut request = message("@Atlas");
    let reference = participant_reference(
        "reference:duplicate",
        ChatParticipantId::new("participant:atlas").unwrap(),
        ChatParticipantKind::AiAgent,
        "Atlas",
        "@Atlas",
        0,
        6,
    );
    request.references = vec![reference.clone(), reference];
    let error = validate_message_request(&request).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("references"));
}

#[test]
fn fts_queries_are_bounded_and_quote_each_term() {
    assert_eq!(
        normalized_fts_query("review exact-folder").unwrap(),
        "\"review\" AND \"exact-folder\""
    );
    assert!(normalized_fts_query("   ").is_err());
    assert!(normalized_fts_query(&"x".repeat(501)).is_err());
}

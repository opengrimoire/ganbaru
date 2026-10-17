use super::persistence::{
    PersistUserTurnContext, TurnPersistenceTarget, persist_user_turn, read_thread_runtime_data,
};
use super::session::{normalize_changed_file_paths, reuse_existing_session};
use super::validation::{
    validate_approval_decision, validate_model_options, validate_pending_request,
    validate_send_mentions,
};
use crate::chat::send_commands::SendChatTurnCommand;
use crate::chat::tests::repository::pool_with_thread;
use crate::chat::workspace::AuthorizedWorkingFolder;
use ganbaru_chat::agent_runs::TurnOrigin;
use ganbaru_chat::repository::workspaces;
use ganbaru_chat_contracts::events::{CanonicalEvent, ChangedFileSummary, DiffUpdatedEvent};
use ganbaru_chat_contracts::models::*;

#[test]
fn provider_changed_files_become_relative_bounded_and_deduplicated() {
    let workspace = std::env::temp_dir().join("ganbaru-provider-path-workspace");
    let absolute_file = workspace.join("src").join("hello.py");
    let outside_file = std::env::temp_dir().join("ganbaru-provider-path-outside.py");
    let mut event = CanonicalEvent::DiffUpdated(DiffUpdatedEvent {
        source: "fixture".to_string(),
        files: vec![
            ChangedFileSummary {
                relative_path: absolute_file.to_string_lossy().into_owned(),
                previous_relative_path: None,
                additions: Some(0),
                deletions: Some(0),
                binary: false,
                status: "modified".to_string(),
            },
            ChangedFileSummary {
                relative_path: "src/hello.py".to_string(),
                previous_relative_path: Some(
                    workspace.join("old.py").to_string_lossy().into_owned(),
                ),
                additions: Some(10),
                deletions: Some(1),
                binary: false,
                status: "renamed".to_string(),
            },
            ChangedFileSummary {
                relative_path: outside_file.to_string_lossy().into_owned(),
                previous_relative_path: None,
                additions: None,
                deletions: None,
                binary: false,
                status: "modified".to_string(),
            },
            ChangedFileSummary {
                relative_path: "../escape.py".to_string(),
                previous_relative_path: None,
                additions: None,
                deletions: None,
                binary: false,
                status: "modified".to_string(),
            },
        ],
        provider_diff: None,
    });

    normalize_changed_file_paths(&mut event, &workspace);

    let CanonicalEvent::DiffUpdated(event) = event else {
        panic!("expected diff event");
    };
    assert_eq!(event.files.len(), 1);
    assert_eq!(event.files[0].relative_path, "src/hello.py");
    assert_eq!(
        event.files[0].previous_relative_path.as_deref(),
        Some("old.py")
    );
    assert_eq!(event.files[0].additions, Some(10));
    assert_eq!(event.files[0].status, "renamed");
}

#[test]
fn stopped_and_failed_sessions_restart_even_if_a_stale_identity_remains() {
    assert!(!reuse_existing_session(true, ProviderSessionState::Stopped).unwrap());
    assert!(!reuse_existing_session(true, ProviderSessionState::Failed).unwrap());
    assert!(reuse_existing_session(true, ProviderSessionState::Ready).unwrap());
    assert!(reuse_existing_session(true, ProviderSessionState::Stopping).is_err());
}

#[cfg(unix)]
struct TestDirectory(std::path::PathBuf);

#[cfg(unix)]
impl TestDirectory {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "ganbaru-chat-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

#[cfg(unix)]
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn user_intent_and_receipt_commit_before_provider_dispatch() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let working_folder_id = ProjectWorkingFolderId::new("workspace-1").unwrap();
        let attachment_id = ChatAttachmentId::new("attachment-before-dispatch").unwrap();
        sqlx::query(
            "INSERT INTO chat_attachments
                    (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                     sha256, managed_relative_path, signature_kind, created_at)
                 VALUES (?, ?, 'image', 'prompt.png', 'image/png', 14, ?, ?, 'png', ?)",
        )
        .bind(attachment_id.as_str())
        .bind(working_folder_id.as_str())
        .bind("0".repeat(64))
        .bind("assets/chat/attachments/prompt.png")
        .bind("2026-07-21T12:00:00Z")
        .execute(&pool)
        .await
        .unwrap();
        let workspace = workspaces::read_workspace(&pool, &working_folder_id)
            .await
            .unwrap();
        let existing = read_thread_runtime_data(&pool, &thread_id).await.unwrap();
        let request = SendChatTurnCommand {
            command: ChatCommandContext {
                client_command_id: ChatCommandId::new("send-before-dispatch").unwrap(),
                expected_thread_revision: Some(existing.revision),
            },
            working_folder_id: Some(working_folder_id.clone()),
            scratch_generation_id: None,
            thread_id: Some(thread_id.clone()),
            new_thread_id: None,
            execution_environment_id: None,
            turn_id: ChatTurnId::new("turn-before-dispatch").unwrap(),
            message_id: ChatMessageId::new("message-before-dispatch").unwrap(),
            provider_instance_id: ProviderInstanceId::new("codex-personal").unwrap(),
            provider_managed_model: true,
            model_id: None,
            model_options: Vec::new(),
            modes: TurnModeSnapshot {
                safety_mode: SafetyMode::AskForApproval,
                interaction_mode: InteractionMode::Build,
            },
            prompt: "  Preserve this exact prompt\n".to_string(),
            attachment_ids: vec![attachment_id.clone()],
            mentions: Vec::new(),
        };
        let attachments = [PromptAttachmentReference {
            attachment_id,
            kind: "image".to_string(),
            display_name: "prompt.png".to_string(),
            managed_relative_path: "assets/chat/attachments/prompt.png".to_string(),
            resource_uri: "ganbaru://chat/resource/attachment:image".to_string(),
            mime_type: Some("image/png".to_string()),
            byte_size: 14,
            local_path: Some("/vault/assets/chat/attachments/prompt.png".to_string()),
            text_content: None,
        }];
        let family_id = ProviderFamilyId::new("codex").unwrap();
        let persisted_at = UtcTimestamp::new("2026-07-21T12:00:00Z").unwrap();
        let target = TurnPersistenceTarget {
            project_id: workspace.project_id.clone(),
            working_folder_id: Some(working_folder_id.clone()),
            scratch_generation_id: None,
            execution_environment_id: Some(format!(
                "current-folder:{}",
                working_folder_id.as_str()
            )),
        };
        persist_user_turn(PersistUserTurnContext {
            pool: &pool,
            target: &target,
            thread_id: &thread_id,
            existing: Some(&existing),
            continuation_group_id: &existing.continuation_group_id,
            provider_family_id: &family_id,
            request: &request,
            origin: &TurnOrigin::Direct,
            attachments: &attachments,
            now: &persisted_at,
        })
        .await
        .unwrap();

        let stored: String = sqlx::query_scalar(
            "SELECT normalized_markdown FROM chat_messages WHERE id = 'message-before-dispatch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let context: String = sqlx::query_scalar(
            "SELECT content_metadata_data FROM chat_messages
                 WHERE id = 'message-before-dispatch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let receipt_state: String = sqlx::query_scalar(
                "SELECT state FROM chat_command_receipts WHERE client_command_id = 'send-before-dispatch'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        let turn_state: String =
            sqlx::query_scalar("SELECT state FROM chat_turns WHERE id = 'turn-before-dispatch'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stored, "  Preserve this exact prompt\n");
        let context = serde_json::from_str::<serde_json::Value>(&context).unwrap();
        assert_eq!(
            context["attachments"][0]["attachmentId"],
            "attachment-before-dispatch"
        );
        assert_eq!(context["attachments"][0]["displayName"], "prompt.png");
        assert_eq!(context["attachments"][0]["kind"], "image");
        assert_eq!(context["attachments"][0]["byteSize"], 14);
        assert_eq!(context["attachments"][0]["status"], "managed");
        assert!(context["attachments"][0].get("id").is_none());
        assert!(context["attachments"][0].get("filename").is_none());
        assert_eq!(receipt_state, "accepted");
        assert_eq!(turn_state, "pending");
    });
}

#[test]
fn model_options_require_declared_types_values_and_unique_keys() {
    let definitions = vec![
        ModelOptionDefinition::Choice {
            key: "effort".to_string(),
            label: "Effort".to_string(),
            description: None,
            options: vec![ModelChoiceOption {
                value: "high".to_string(),
                label: "High".to_string(),
                description: None,
            }],
            default_value: None,
        },
        ModelOptionDefinition::IntegerRange {
            key: "budget".to_string(),
            label: "Budget".to_string(),
            description: None,
            minimum: 2,
            maximum: 10,
            step: 2,
            default_value: None,
        },
    ];
    assert!(
        validate_model_options(
            &definitions,
            &[
                ModelOptionSelection {
                    key: "effort".to_string(),
                    value: ModelOptionValue::Choice("high".to_string()),
                },
                ModelOptionSelection {
                    key: "budget".to_string(),
                    value: ModelOptionValue::Integer(6),
                },
            ],
        )
        .is_ok()
    );
    for invalid in [
        vec![ModelOptionSelection {
            key: "effort".to_string(),
            value: ModelOptionValue::Choice("undeclared".to_string()),
        }],
        vec![ModelOptionSelection {
            key: "budget".to_string(),
            value: ModelOptionValue::Integer(5),
        }],
        vec![
            ModelOptionSelection {
                key: "effort".to_string(),
                value: ModelOptionValue::Choice("high".to_string()),
            },
            ModelOptionSelection {
                key: "effort".to_string(),
                value: ModelOptionValue::Choice("high".to_string()),
            },
        ],
    ] {
        assert!(validate_model_options(&definitions, &invalid).is_err());
    }
}

#[test]
fn approval_validation_rejects_cross_thread_expired_and_fabricated_requests() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query(
            "INSERT INTO chat_pending_requests
                    (id, thread_id, provider_request_id, request_kind,
                     safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
                 VALUES ('approval-1', 'thread-1', 'provider-approval-1', 'approval',
                         '{}', ?, 1, '2026-07-21T12:00:00Z')",
        )
        .bind(
            serde_json::json!([{
                "id": "allow-once",
                "label": "Allow once",
                "decisionKind": "allow_once",
                "description": null
            }])
            .to_string(),
        )
        .execute(&pool)
        .await
        .unwrap();
        let thread = ChatThreadId::new("thread-1").unwrap();
        let request = ChatRequestId::new("approval-1").unwrap();
        let provider_request = ProviderRequestId::new("provider-approval-1").unwrap();

        validate_pending_request(&pool, &thread, &request, &provider_request, "approval")
            .await
            .unwrap();
        for (candidate_thread, candidate_request, candidate_provider) in [
            (
                ChatThreadId::new("thread-other").unwrap(),
                request.clone(),
                provider_request.clone(),
            ),
            (
                thread.clone(),
                ChatRequestId::new("fabricated-request").unwrap(),
                provider_request.clone(),
            ),
            (
                thread.clone(),
                request.clone(),
                ProviderRequestId::new("fabricated-provider-request").unwrap(),
            ),
        ] {
            let error = validate_pending_request(
                &pool,
                &candidate_thread,
                &candidate_request,
                &candidate_provider,
                "approval",
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Conflict);
        }

        validate_approval_decision(
            &pool,
            &request,
            &ApprovalDecision {
                kind: ApprovalDecisionKind::AllowOnce,
                provider_option_id: Some("allow-once".to_string()),
                updated_tool_input: None,
            },
        )
        .await
        .unwrap();
        for decision in [
            ApprovalDecision {
                kind: ApprovalDecisionKind::AllowSession,
                provider_option_id: Some("allow-once".to_string()),
                updated_tool_input: None,
            },
            ApprovalDecision {
                kind: ApprovalDecisionKind::AllowOnce,
                provider_option_id: Some("fabricated-option".to_string()),
                updated_tool_input: None,
            },
        ] {
            let error = validate_approval_decision(&pool, &request, &decision)
                .await
                .unwrap_err();
            assert_eq!(error.code, ChatErrorCode::Permission);
        }

        sqlx::query(
            "UPDATE chat_pending_requests
                 SET resolution_state = 'stale', resolved_at = '2026-07-21T12:01:00Z'
                 WHERE id = 'approval-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let expired =
            validate_pending_request(&pool, &thread, &request, &provider_request, "approval")
                .await
                .unwrap_err();
        assert_eq!(expired.code, ChatErrorCode::Conflict);
    });
}

#[cfg(unix)]
#[test]
fn send_time_validation_rejects_file_and_directory_symlink_swaps() {
    use std::os::unix::fs::symlink;

    let workspace = TestDirectory::new("mention-swap-workspace");
    let outside = TestDirectory::new("mention-swap-outside");
    std::fs::write(workspace.0.join("selected.txt"), "inside").unwrap();
    std::fs::write(outside.0.join("secret.txt"), "outside").unwrap();
    std::fs::create_dir(workspace.0.join("selected-directory")).unwrap();
    std::fs::create_dir(outside.0.join("outside-directory")).unwrap();
    let authorized = AuthorizedWorkingFolder {
        working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
        canonical_path: workspace.0.clone(),
        repository_kind: RepositoryKind::None,
        repository_identity: None,
        repository_storage_identity: None,
    };
    let file_mention = WorkspaceMentionReference {
        relative_path: "selected.txt".to_string(),
        kind: "file".to_string(),
    };
    let directory_mention = WorkspaceMentionReference {
        relative_path: "selected-directory".to_string(),
        kind: "directory".to_string(),
    };

    validate_send_mentions(&authorized, std::slice::from_ref(&file_mention)).unwrap();
    validate_send_mentions(&authorized, std::slice::from_ref(&directory_mention)).unwrap();

    std::fs::remove_file(workspace.0.join("selected.txt")).unwrap();
    symlink(
        outside.0.join("secret.txt"),
        workspace.0.join("selected.txt"),
    )
    .unwrap();
    std::fs::remove_dir(workspace.0.join("selected-directory")).unwrap();
    symlink(
        outside.0.join("outside-directory"),
        workspace.0.join("selected-directory"),
    )
    .unwrap();

    for mention in [file_mention, directory_mention] {
        let error = validate_send_mentions(&authorized, &[mention]).unwrap_err();
        assert_eq!(error.code, ChatErrorCode::Permission);
        assert!(!error.message.contains("secret.txt"));
    }
}

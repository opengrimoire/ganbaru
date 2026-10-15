use super::*;

#[test]
fn durable_draft_preserves_unknown_json_and_attachment_references() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let vault_root = std::env::temp_dir().join(format!(
            "ganbaru-chat-draft-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&vault_root).unwrap();
        let source = vault_root.join("source.txt");
        fs::write(&source, b"explicit context").unwrap();
        let attachment_id = ChatAttachmentId::new("attachment-draft").unwrap();
        import_attachment(
            &pool,
            &vault_root,
            &ProjectWorkingFolderId::new("workspace-1").unwrap(),
            attachment_id.clone(),
            &source,
            ChatAttachmentKind::TextSnippet,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        let draft = ChatDraftWrite {
            id: "draft-1".to_string(),
            working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
            thread_id: Some(ChatThreadId::new("thread-1").unwrap()),
            text: "Keep this".to_string(),
            rich_content: Some(VersionedJson {
                schema_version: 1,
                value: serde_json::json!({
                    "lines": [{ "runs": [{ "text": "Keep this", "marks": ["bold"] }] }]
                }),
            }),
            attachment_ids: vec![attachment_id],
            mentions: VersionedJson {
                schema_version: 88,
                value: serde_json::json!([{ "futureMention": true }]),
            },
            provider_instance_id: Some(ProviderInstanceId::new("codex-personal").unwrap()),
            model_selection: Some(VersionedJson {
                schema_version: 91,
                value: serde_json::json!({ "futureModel": "x" }),
            }),
            safety_mode: Some(SafetyMode::AskForApproval),
            interaction_mode: Some(InteractionMode::Build),
            sent_snapshot: None,
            updated_at: UtcTimestamp::new(NOW).unwrap(),
        };
        assert_eq!(
            save_draft(&pool, &draft, &ChatDraftBase::None)
                .await
                .unwrap(),
            draft
        );
        assert_eq!(read_draft(&pool, "draft-1").await.unwrap(), Some(draft));
        delete_draft(&pool, "draft-1", &UtcTimestamp::new(NOW).unwrap())
            .await
            .unwrap();
        let unreferenced: String = sqlx::query_scalar(
            "SELECT unreferenced_at FROM chat_attachments WHERE id = 'attachment-draft'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unreferenced, NOW);
        fs::remove_dir_all(vault_root).unwrap();
    });
}

#[test]
fn draft_save_refuses_a_base_that_no_longer_matches_without_writing() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let draft_at = |text: &str, updated_at: &str| ChatDraftWrite {
            id: "draft-guarded".to_string(),
            working_folder_id: ProjectWorkingFolderId::new("workspace-1").unwrap(),
            thread_id: None,
            text: text.to_string(),
            rich_content: None,
            attachment_ids: Vec::new(),
            mentions: VersionedJson {
                schema_version: 1,
                value: serde_json::json!([]),
            },
            provider_instance_id: None,
            model_selection: None,
            safety_mode: None,
            interaction_mode: None,
            sent_snapshot: None,
            updated_at: UtcTimestamp::new(updated_at).unwrap(),
        };
        let first = draft_at("first window", "2026-01-01T00:00:01.000Z");
        let first_saved = UtcTimestamp::new("2026-01-01T00:00:01.000Z").unwrap();
        save_draft(&pool, &first, &ChatDraftBase::None)
            .await
            .unwrap();

        let stale = |error: ChatError| {
            assert_eq!(
                error.code,
                ganbaru_chat_contracts::models::ChatErrorCode::StaleRevision
            );
        };
        stale(
            save_draft(
                &pool,
                &draft_at("second window", "2026-01-01T00:00:02.000Z"),
                &ChatDraftBase::None,
            )
            .await
            .unwrap_err(),
        );
        stale(
            save_draft(
                &pool,
                &draft_at("second window", "2026-01-01T00:00:02.000Z"),
                &ChatDraftBase::Saved {
                    updated_at: UtcTimestamp::new("2026-01-01T00:00:00.500Z").unwrap(),
                },
            )
            .await
            .unwrap_err(),
        );
        assert_eq!(
            read_draft(&pool, "draft-guarded").await.unwrap(),
            Some(first.clone())
        );

        let second = draft_at("second window", "2026-01-01T00:00:02.000Z");
        save_draft(
            &pool,
            &second,
            &ChatDraftBase::Saved {
                updated_at: first_saved,
            },
        )
        .await
        .unwrap();
        let sent = draft_at("sent", "2026-01-01T00:00:03.000Z");
        assert_eq!(
            save_draft(&pool, &sent, &ChatDraftBase::Any).await.unwrap(),
            sent
        );

        delete_draft(&pool, "draft-guarded", &UtcTimestamp::new(NOW).unwrap())
            .await
            .unwrap();
        stale(
            save_draft(
                &pool,
                &second,
                &ChatDraftBase::Saved {
                    updated_at: UtcTimestamp::new("2026-01-01T00:00:03.000Z").unwrap(),
                },
            )
            .await
            .unwrap_err(),
        );
    });
}

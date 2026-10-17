use super::*;

#[test]
fn browser_artifacts_are_managed_thread_resources() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let vault_root = std::env::temp_dir().join(format!(
            "ganbaru-chat-browser-artifact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&vault_root).unwrap();
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let working_folder_id = ProjectWorkingFolderId::new("workspace-1").unwrap();
        sqlx::query(
            "INSERT INTO chat_preview_tabs
                (id, thread_id, position, current_url, title, viewport_kind, viewport_width,
                 viewport_height, visible, loading_state, created_at, updated_at)
             VALUES ('preview-1', 'thread-1', 0, 'http://localhost:5173', 'Preview',
                 'freeform', 1280, 720, 1, 'loaded', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let bytes = b"bounded-png-fixture";
        let stored = store_browser_artifact(
            &pool,
            &vault_root,
            StoreBrowserArtifact {
                resource_id: "browser-artifact-1",
                thread_id: &thread_id,
                working_folder_id: &working_folder_id,
                preview_tab_id: "preview-1",
                kind: ChatResourceKind::BrowserScreenshot,
                display_name: "Browser screenshot",
                mime_type: "image/png",
                source_url: "http://localhost:5173",
                viewport_width: 1280,
                viewport_height: 720,
                duration_milliseconds: None,
                frame_count: Some(1),
                created_at: &UtcTimestamp::new(NOW).unwrap(),
                bytes,
            },
        )
        .await
        .unwrap();

        assert_eq!(stored.kind, ChatResourceKind::BrowserScreenshot);
        assert_eq!(
            list_thread_resources(&pool, &thread_id).await.unwrap(),
            vec![stored.clone()]
        );
        assert_eq!(
            read_thread_resource_bytes(&pool, &vault_root, &thread_id, &stored.id)
                .await
                .unwrap(),
            (stored, bytes.to_vec())
        );
        let artifact_path = vault_root.join("assets/chat/browser-artifacts/browser-artifact-1.png");
        assert_eq!(fs::read(&artifact_path).unwrap(), bytes);
        let revision =
            sqlx::query_scalar::<_, i64>("SELECT revision FROM chat_threads WHERE id = 'thread-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        permanently_delete_thread(
            &pool,
            &thread_id,
            u64::try_from(revision).unwrap(),
            &UtcTimestamp::new(NOW).unwrap(),
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            run_due_attachment_cleanup(&pool, &vault_root, &UtcTimestamp::new(NOW).unwrap())
                .await
                .unwrap(),
            1
        );
        assert!(!artifact_path.exists());
        fs::remove_dir_all(vault_root).unwrap();
    });
}

#[test]
fn attachment_cleanup_retries_and_never_removes_referenced_files() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let vault_root = std::env::temp_dir().join(format!(
            "ganbaru-chat-cleanup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&vault_root).unwrap();
        let source = vault_root.join("source.txt");
        fs::write(&source, b"retained context").unwrap();
        let attachment_id = ChatAttachmentId::new("attachment-cleanup").unwrap();
        let imported = import_attachment(
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
        sqlx::query(
            "INSERT INTO chat_cleanup_jobs
                (id, cleanup_kind, exact_target, state, available_at, created_at, updated_at,
                 working_folder_id)
             VALUES ('cleanup-retained', 'attachment_file', ?, 'failed', ?, ?, ?, 'workspace-1')",
        )
        .bind(&imported.managed_relative_path)
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_drafts
                (id, working_folder_id, text, mentions_data, updated_at)
             VALUES ('draft-retained', 'workspace-1', '', '[]', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_attachment_references
                (id, attachment_id, draft_id, created_at)
             VALUES ('reference-retained', ?, 'draft-retained', ?)",
        )
        .bind(attachment_id.as_str())
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            run_due_attachment_cleanup(&pool, &vault_root, &UtcTimestamp::new(NOW).unwrap())
                .await
                .unwrap(),
            1
        );
        let managed_path = vault_root.join(&imported.managed_relative_path);
        assert!(
            managed_path.exists(),
            "referenced attachment files must be retained"
        );
        let state: String = sqlx::query_scalar(
            "SELECT deletion_state FROM chat_attachments WHERE id = 'attachment-cleanup'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "active", "referenced metadata must remain active");
        fs::remove_dir_all(vault_root).unwrap();
    });
}

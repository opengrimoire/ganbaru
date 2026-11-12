use super::*;

#[test]
fn command_receipts_replay_every_idempotent_command_kind() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let now = UtcTimestamp::new(NOW).unwrap();
        for command_kind in [
            "send",
            "approval",
            "answer",
            "interrupt",
            "restore",
            "delete",
        ] {
            let command_id = ChatCommandId::new(format!("command-{command_kind}")).unwrap();
            let claim =
                claim_command_receipt(&pool, &command_id, &thread_id, command_kind, Some(1), &now)
                    .await
                    .unwrap();
            assert!(matches!(claim, CommandReceiptClaim::Claimed(_)));
            let result = VersionedJson {
                schema_version: 7,
                value: serde_json::json!({ "commandKind": command_kind, "future": true }),
            };
            complete_command_receipt(
                &pool,
                &command_id,
                CommandReceiptState::Completed,
                Some(&result),
                None,
                &now,
            )
            .await
            .unwrap();
            let replay =
                claim_command_receipt(&pool, &command_id, &thread_id, command_kind, Some(1), &now)
                    .await
                    .unwrap();
            let CommandReceiptClaim::Replay(receipt) = replay else {
                panic!("duplicate command should replay its original receipt");
            };
            assert_eq!(receipt.result, Some(result));
        }
    });
}

#[test]
fn command_receipts_reject_cross_thread_and_changed_command_reuse() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query(
            "INSERT INTO chat_threads
                (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
                 continuation_group_id, safety_mode, interaction_mode, state,
                 last_activity_at, created_at, updated_at)
             VALUES ('thread-2', 'project-chat', 'workspace-1', 'Other', 'codex', 'codex-personal',
                     'continuation-1', 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let command_id = ChatCommandId::new("command-boundary").unwrap();
        let thread_1 = ChatThreadId::new("thread-1").unwrap();
        let thread_2 = ChatThreadId::new("thread-2").unwrap();
        let now = UtcTimestamp::new(NOW).unwrap();
        claim_command_receipt(&pool, &command_id, &thread_1, "approval", Some(1), &now)
            .await
            .unwrap();

        assert!(
            claim_command_receipt(&pool, &command_id, &thread_2, "approval", Some(1), &now,)
                .await
                .is_err()
        );
        assert!(
            claim_command_receipt(&pool, &command_id, &thread_1, "answer", Some(1), &now,)
                .await
                .is_err()
        );
    });
}

/// A file-backed pool with several connections, so concurrent claims really race.
async fn racing_pool_with_thread(name: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "ganbaru-receipts-{name}-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    ganbaru_db::test_support::write_migrated_database(&path)
        .await
        .unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&path)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-chat', 'Chat', 'a1');
         INSERT INTO projects (id, group_id, name, order_key)
         VALUES ('project-chat', 'group-chat', 'Chat', 'a1');",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, managed_relative_path,
             repository_kind, sort_order, created_at, updated_at)
         VALUES ('workspace-1', 'project-chat', 'Project files', 'managed',
                 'projects/project-chat', 'none', 0, ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    ganbaru_chat::project_defaults::ensure_project_chat_defaults(
        &mut connection,
        "project-chat",
        &UtcTimestamp::new(NOW).unwrap(),
    )
    .await
    .unwrap();
    drop(connection);
    sqlx::query(
        "INSERT INTO chat_threads
            (id, project_id, working_folder_id, title, provider_family_id, provider_instance_id,
             continuation_group_id, safety_mode, interaction_mode, state,
             last_activity_at, created_at, updated_at)
         VALUES ('thread-1', 'project-chat', 'workspace-1', 'Chat', 'codex', 'codex-personal',
                 'continuation-1', 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
    )
    .bind(NOW)
    .bind(NOW)
    .bind(NOW)
    .execute(&pool)
    .await
    .unwrap();
    (pool, path)
}

#[test]
fn concurrent_duplicate_commands_claim_once_and_replay_once() {
    tauri::async_runtime::block_on(async {
        let (pool, path) = racing_pool_with_thread("race").await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let now = UtcTimestamp::new(NOW).unwrap();
        for attempt in 0..20 {
            let command_id = ChatCommandId::new(format!("command-race-{attempt}")).unwrap();
            let (first, second) = tokio::join!(
                claim_command_receipt(&pool, &command_id, &thread_id, "send", Some(1), &now),
                claim_command_receipt(&pool, &command_id, &thread_id, "send", Some(1), &now),
            );
            let claims = [first.unwrap(), second.unwrap()];
            let claimed = claims
                .iter()
                .filter(|claim| matches!(claim, CommandReceiptClaim::Claimed(_)))
                .count();
            let replayed = claims
                .iter()
                .filter(|claim| matches!(claim, CommandReceiptClaim::Replay(_)))
                .count();
            assert_eq!((claimed, replayed), (1, 1));
        }
        pool.close().await;
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    });
}

#[test]
fn command_receipt_claims_report_stale_revisions_and_missing_threads() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let now = UtcTimestamp::new(NOW).unwrap();
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let revision: i64 = sqlx::query_scalar("SELECT revision FROM chat_threads WHERE id = ?")
            .bind("thread-1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let stale = u64::try_from(revision).unwrap() + 1;
        let error = claim_command_receipt(
            &pool,
            &ChatCommandId::new("command-stale").unwrap(),
            &thread_id,
            "send",
            Some(stale),
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::StaleRevision);
        let missing = claim_command_receipt(
            &pool,
            &ChatCommandId::new("command-missing").unwrap(),
            &ChatThreadId::new("thread-missing").unwrap(),
            "send",
            Some(1),
            &now,
        )
        .await
        .unwrap_err();
        assert_eq!(missing.code, ChatErrorCode::NotFound);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chat_command_receipts")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    });
}

#[test]
fn command_receipt_pruning_removes_only_old_settled_receipts() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let old = UtcTimestamp::new("2026-01-01T00:00:00.000Z").unwrap();
        let recent = UtcTimestamp::new("2026-03-01T00:00:00.000Z").unwrap();
        let cutoff = UtcTimestamp::new("2026-02-01T00:00:00.000Z").unwrap();
        let result = VersionedJson {
            schema_version: 1,
            value: serde_json::json!({}),
        };
        for (name, at, settle) in [
            ("old-completed", &old, true),
            ("old-accepted", &old, false),
            ("recent-completed", &recent, true),
        ] {
            let command_id = ChatCommandId::new(name).unwrap();
            claim_command_receipt(&pool, &command_id, &thread_id, "send", None, at)
                .await
                .unwrap();
            if settle {
                complete_command_receipt(
                    &pool,
                    &command_id,
                    CommandReceiptState::Completed,
                    Some(&result),
                    None,
                    at,
                )
                .await
                .unwrap();
            }
        }
        for (name, state, at) in [
            ("org-old-failed", "failed", &old),
            ("org-old-accepted", "accepted", &old),
        ] {
            sqlx::query(
                "INSERT INTO chat_organizational_command_receipts
                    (client_command_id, command_kind, state, created_at, updated_at)
                 VALUES (?, 'post_message', ?, ?, ?)",
            )
            .bind(name)
            .bind(state)
            .bind(at.as_str())
            .bind(at.as_str())
            .execute(&pool)
            .await
            .unwrap();
        }

        assert_eq!(prune_command_receipts(&pool, &cutoff).await.unwrap(), 2);
        let thread_receipts: Vec<String> = sqlx::query_scalar(
            "SELECT client_command_id FROM chat_command_receipts ORDER BY client_command_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(thread_receipts, ["old-accepted", "recent-completed"]);
        let organizational: Vec<String> = sqlx::query_scalar(
            "SELECT client_command_id FROM chat_organizational_command_receipts",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(organizational, ["org-old-accepted"]);
    });
}

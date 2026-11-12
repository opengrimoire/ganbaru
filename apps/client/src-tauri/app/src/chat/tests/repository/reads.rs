use super::*;

#[test]
fn shell_search_timeline_and_archive_reads_stay_lightweight() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, content_event("event-shell", "Searchable response"))
            .await
            .unwrap();
        let projects = read_project_shells(&pool).await.unwrap();
        let project = projects
            .iter()
            .find(|project| project.project_id == "project-chat")
            .expect("Chat project shell");
        assert_eq!(project.active_thread_count, 1);
        let shells = read_thread_shells(&pool, None, false).await.unwrap();
        assert_eq!(shells.len(), 1);
        assert_eq!(shells[0].message_count, 1);
        let search = search_thread_titles(&pool, "cha", Some(false), 20)
            .await
            .unwrap();
        assert_eq!(search.len(), 1);
        let page = read_timeline_page(&pool, &ChatThreadId::new("thread-1").unwrap(), None, 20)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);

        let revision = set_thread_read(
            &pool,
            &ChatThreadId::new("thread-1").unwrap(),
            false,
            2,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        let revision = set_thread_archived(
            &pool,
            &ChatThreadId::new("thread-1").unwrap(),
            true,
            revision,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        assert!(
            read_thread_shells(&pool, None, false)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            read_thread_shells(&pool, None, true).await.unwrap().len(),
            1
        );
        set_thread_archived(
            &pool,
            &ChatThreadId::new("thread-1").unwrap(),
            false,
            revision,
            &UtcTimestamp::new(NOW).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            read_thread_shells(&pool, None, false).await.unwrap().len(),
            1
        );
    });
}

#[test]
fn thread_shell_window_limits_recent_navigation_rows() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::query(
            "INSERT INTO chat_threads
                (id, project_id, working_folder_id, title, provider_family_id,
                 provider_instance_id, continuation_group_id, safety_mode, interaction_mode, state,
                 last_activity_at, created_at, updated_at)
             VALUES ('thread-recent', 'project-chat', 'workspace-1', 'Recent', 'codex',
                     'codex-personal', 'continuation-1', 'ask_for_approval', 'build', 'idle',
                     '2026-07-21T00:00:00.000Z', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let threads = read_thread_shell_window(&pool, None, false, 1)
            .await
            .unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id.as_str(), "thread-recent");
    });
}

#[test]
fn scratch_thread_shells_are_exactly_readable_but_hidden_from_direct_navigation() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO chat_scratch_generations
                (id, scratch_scope_id, generation, created_at, updated_at)
             VALUES ('scratch-generation:shell', 'scratch-scope:shell', 1, ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_execution_environments
                (id, scratch_generation_id, kind, display_name,
                 lifecycle_state, created_at, updated_at)
             VALUES ('scratch-environment:shell', 'scratch-generation:shell',
                     'scratch', 'Private scratch', 'available', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_threads
                (id, project_id, execution_environment_id, scratch_generation_id,
                 title, provider_family_id, provider_instance_id,
                 continuation_group_id, safety_mode, interaction_mode, state,
                 last_activity_at, created_at, updated_at)
             VALUES ('thread-scratch', 'project-chat', 'scratch-environment:shell',
                     'scratch-generation:shell', 'Private scratch execution',
                     'codex', 'codex-personal', 'continuation-scratch',
                     'ask_for_approval', 'build', 'idle', ?, ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&pool)
            .await
            .unwrap();

        let scratch = read_thread_shell(&pool, &ChatThreadId::new("thread-scratch").unwrap())
            .await
            .unwrap();
        assert!(scratch.working_folder_id.is_none());
        assert_eq!(
            scratch.execution_environment_id.as_str(),
            "scratch-environment:shell"
        );
        assert_eq!(
            scratch.scratch_generation_id.as_ref().map(|id| id.as_str()),
            Some("scratch-generation:shell")
        );

        let listed = read_thread_shells(&pool, None, false).await.unwrap();
        assert!(
            listed
                .iter()
                .all(|thread| thread.id.as_str() != "thread-scratch")
        );
        let searched = search_thread_titles(&pool, "scratch", Some(false), 20)
            .await
            .unwrap();
        assert!(searched.is_empty());
    });
}

#[test]
fn timeline_cursor_does_not_skip_rows_that_share_a_sequence() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        append_canonical_event(&pool, content_event("event-page", "Answer"))
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO chat_activities
                (id, thread_id, sequence_anchor, item_kind, status, title,
                 source_event_type, created_at, updated_at)
             VALUES ('activity-same-sequence', 'thread-1', 1, 'web_search',
                     'completed', 'Search', 'item_completed', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let newest = read_timeline_page(&pool, &thread_id, None, 1)
            .await
            .unwrap();
        let cursor = parse_timeline_cursor(newest.previous_cursor.as_deref().unwrap()).unwrap();
        let older = read_timeline_page(&pool, &thread_id, Some(&cursor), 1)
            .await
            .unwrap();

        let ids = [
            newest.items[0].activity_id.as_str(),
            older.items[0].activity_id.as_str(),
        ]
        .into_iter()
        .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 2);
        assert!(older.previous_cursor.is_none());
    });
}

#[test]
fn exact_turn_timeline_read_does_not_depend_on_the_latest_thread_page() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        for (suffix, answer) in [("older", "Older answer"), ("newer", "Newer answer")] {
            let turn_id = format!("turn-{suffix}");
            append_canonical_event(
                &pool,
                canonical_request(
                    &format!("event-{suffix}-start"),
                    Some(&turn_id),
                    CanonicalEvent::TurnStarted(TurnStartedEvent {
                        provider_turn_id: None,
                        state: ChatTurnState::Active,
                        modes: TurnModeSnapshot {
                            safety_mode: SafetyMode::AskForApproval,
                            interaction_mode: InteractionMode::Build,
                        },
                        model_id: None,
                        model_options: Vec::new(),
                    }),
                ),
            )
            .await
            .unwrap();
            append_canonical_event(
                &pool,
                canonical_request(
                    &format!("event-{suffix}-answer"),
                    Some(&turn_id),
                    CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                        item_id: format!("answer-{suffix}"),
                        kind: CanonicalItemKind::AssistantMessage,
                        status: ActivityStatus::Completed,
                        title: Some("Assistant message".to_string()),
                        detail: Some(answer.to_string()),
                        safe_metadata: Some(VersionedJson {
                            schema_version: 1,
                            value: serde_json::json!({ "phase": "final_answer" }),
                        }),
                    }),
                ),
            )
            .await
            .unwrap();
            append_canonical_event(
                &pool,
                canonical_request(
                    &format!("event-{suffix}-complete"),
                    Some(&turn_id),
                    CanonicalEvent::TurnCompleted(TurnCompletedEvent {
                        state: ChatTurnState::Completed,
                        stop_reason: None,
                        usage: None,
                        changed_files: Vec::new(),
                    }),
                ),
            )
            .await
            .unwrap();
        }

        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let turn_id = ChatTurnId::new("turn-older").unwrap();
        let page = read_timeline_turn(&pool, &thread_id, &turn_id)
            .await
            .unwrap();

        assert_eq!(page.thread_id, thread_id);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].activity_id.as_str(), "answer-older");
        assert_eq!(page.items[0].source_thread_id.as_ref(), Some(&thread_id));
        assert_eq!(page.turns.len(), 1);
        assert_eq!(page.turns[0].turn_id, turn_id);
        assert!(page.previous_cursor.is_none());
        assert!(page.next_cursor.is_none());
    });
}

async fn insert_activity(pool: &sqlx::SqlitePool, id: &str, sequence: i64, detail: Option<&str>) {
    sqlx::query(
        "INSERT INTO chat_activities
            (id, thread_id, sequence_anchor, item_kind, status, title, detail,
             source_event_type, created_at, updated_at)
         VALUES (?, 'thread-1', ?, 'command_execution', 'completed', 'Command', ?,
                 'item_completed', ?, ?)",
    )
    .bind(id)
    .bind(sequence)
    .bind(detail)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

fn item_ids(items: &[ganbaru_chat_contracts::models::ChatTimelineItemRead]) -> Vec<&str> {
    items.iter().map(|item| item.activity_id.as_str()).collect()
}

#[test]
fn timeline_since_read_returns_only_rows_changed_after_the_page_cursor() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        append_canonical_event(&pool, content_event("event-since-1", "Hello"))
            .await
            .unwrap();
        insert_activity(&pool, "activity-old", 1, None).await;
        let page = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap();
        assert_eq!(page.items.len(), 2);

        let unchanged = read_timeline_since(&pool, &thread_id, page.change_cursor, 20)
            .await
            .unwrap();
        assert!(unchanged.items.is_empty());
        assert_eq!(unchanged.change_cursor, page.change_cursor);

        append_canonical_event(&pool, content_event("event-since-2", " again"))
            .await
            .unwrap();
        let delta = read_timeline_since(&pool, &thread_id, page.change_cursor, 20)
            .await
            .unwrap();
        assert_eq!(item_ids(&delta.items), ["assistant-message-1"]);
        assert!(!delta.has_more && !delta.reset_required);
        assert!(delta.removed_item_ids.is_empty());
        let reread = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap();
        assert_eq!(delta.change_cursor, reread.change_cursor);
        assert!(delta.items[0].change_sequence > page.change_cursor);
    });
}

#[test]
fn timeline_since_read_reports_deleted_and_invalidated_rows_as_removed() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-removal-start",
                Some("turn-removal"),
                CanonicalEvent::TurnStarted(TurnStartedEvent {
                    provider_turn_id: None,
                    state: ChatTurnState::Active,
                    modes: TurnModeSnapshot {
                        safety_mode: SafetyMode::AskForApproval,
                        interaction_mode: InteractionMode::Build,
                    },
                    model_id: None,
                    model_options: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        append_canonical_event(
            &pool,
            canonical_request(
                "event-removal-answer",
                Some("turn-removal"),
                CanonicalEvent::ItemCompleted(ItemLifecycleEvent {
                    item_id: "answer-removal".to_string(),
                    kind: CanonicalItemKind::AssistantMessage,
                    status: ActivityStatus::Completed,
                    title: None,
                    detail: Some("Answer".to_string()),
                    safe_metadata: None,
                }),
            ),
        )
        .await
        .unwrap();
        insert_activity(&pool, "activity-deleted", 1, None).await;
        let page = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap();

        sqlx::query("DELETE FROM chat_activities WHERE id = 'activity-deleted'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE chat_turns SET invalidated_at = ? WHERE id = 'turn-removal'")
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
        let delta = read_timeline_since(&pool, &thread_id, page.change_cursor, 20)
            .await
            .unwrap();

        let removed = delta
            .removed_item_ids
            .iter()
            .map(|id| id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            removed,
            HashSet::from(["activity-deleted", "answer-removal"])
        );
        assert!(delta.items.is_empty());
        assert!(delta.turns.is_empty());
    });
}

#[test]
fn timeline_since_read_caps_bytes_but_always_returns_one_large_row() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let start = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap()
            .change_cursor;
        let large = "x".repeat(2 * 1024 * 1024);
        insert_activity(&pool, "activity-large-1", 1, Some(&large)).await;
        insert_activity(&pool, "activity-large-2", 2, Some(&large)).await;

        let first = read_timeline_since(&pool, &thread_id, start, 20)
            .await
            .unwrap();
        assert_eq!(item_ids(&first.items), ["activity-large-1"]);
        assert!(first.has_more);
        let second = read_timeline_since(&pool, &thread_id, first.change_cursor, 20)
            .await
            .unwrap();
        assert_eq!(item_ids(&second.items), ["activity-large-2"]);
        assert!(!second.has_more);
    });
}

#[test]
fn timeline_since_read_asks_for_a_reload_below_the_pruned_removals() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        insert_activity(&pool, "activity-pruned", 1, None).await;
        let page = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_activities WHERE id = 'activity-pruned'")
            .execute(&pool)
            .await
            .unwrap();
        let after_delete = read_timeline_since(&pool, &thread_id, page.change_cursor, 20)
            .await
            .unwrap();
        assert_eq!(after_delete.removed_item_ids.len(), 1);

        let later = UtcTimestamp::new("2999-01-01T00:00:00Z").unwrap();
        assert_eq!(prune_timeline_removals(&pool, &later).await.unwrap(), 1);
        let stale = read_timeline_since(&pool, &thread_id, page.change_cursor, 20)
            .await
            .unwrap();
        assert!(stale.reset_required);
        let current = read_timeline_since(&pool, &thread_id, after_delete.change_cursor, 20)
            .await
            .unwrap();
        assert!(!current.reset_required);
    });
}

#[test]
fn timeline_reads_carry_a_shortened_detail_that_loads_whole_for_its_own_visible_thread() {
    tauri::async_runtime::block_on(async {
        let pool = pool_with_thread().await;
        let thread_id = ChatThreadId::new("thread-1").unwrap();
        let long = format!("{}✓end", "x".repeat(ACTIVITY_DETAIL_PREVIEW_BYTES));
        append_canonical_event(&pool, content_event("event-detail-1", "Hello"))
            .await
            .unwrap();
        insert_activity(&pool, "activity-long", 1, Some(&long)).await;
        insert_activity(&pool, "activity-short", 1, Some("done")).await;

        let page = read_timeline_page(&pool, &thread_id, None, 20)
            .await
            .unwrap();
        let long_item = page
            .items
            .iter()
            .find(|item| item.activity_id.as_str() == "activity-long")
            .unwrap();
        let value = &long_item.data.value;
        assert!(value.get("detail").is_none());
        assert_eq!(value["detailBytes"], long.len());
        assert_eq!(value["detailTruncated"], true);
        let preview = value["detailPreview"].as_str().unwrap();
        assert!(
            preview.ends_with("✓end"),
            "a command keeps its newest output"
        );
        assert!(preview.len() <= ACTIVITY_DETAIL_PREVIEW_BYTES);
        let short_item = page
            .items
            .iter()
            .find(|item| item.activity_id.as_str() == "activity-short")
            .unwrap();
        assert_eq!(short_item.data.value["detailPreview"], "done");
        assert_eq!(short_item.data.value["detailTruncated"], false);

        let delta = read_timeline_since(&pool, &thread_id, 0, 20).await.unwrap();
        assert!(
            delta
                .items
                .iter()
                .all(|item| item.data.value.get("detail").is_none())
        );

        let activity_id = ChatActivityId::new("activity-long").unwrap();
        let whole = read_activity_detail(&pool, &thread_id, &activity_id)
            .await
            .unwrap();
        assert_eq!(whole.activity_id, activity_id);
        assert_eq!(whole.detail.as_deref(), Some(long.as_str()));
        assert_eq!(whole.updated_at.as_str(), NOW);

        let other_thread = ChatThreadId::new("thread-other").unwrap();
        let error = read_activity_detail(&pool, &other_thread, &activity_id)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::NotFound);
        let missing = ChatActivityId::new("activity-missing").unwrap();
        let error = read_activity_detail(&pool, &thread_id, &missing)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::NotFound);

        append_canonical_event(
            &pool,
            canonical_request(
                "event-hidden-start",
                Some("turn-hidden"),
                CanonicalEvent::TurnStarted(TurnStartedEvent {
                    provider_turn_id: None,
                    state: ChatTurnState::Active,
                    modes: TurnModeSnapshot {
                        safety_mode: SafetyMode::AskForApproval,
                        interaction_mode: InteractionMode::Build,
                    },
                    model_id: None,
                    model_options: Vec::new(),
                }),
            ),
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_activities SET turn_id = 'turn-hidden' WHERE id = 'activity-short'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let hidden = ChatActivityId::new("activity-short").unwrap();
        assert_eq!(
            read_activity_detail(&pool, &thread_id, &hidden)
                .await
                .unwrap()
                .detail
                .as_deref(),
            Some("done")
        );
        sqlx::query("UPDATE chat_turns SET invalidated_at = ? WHERE id = 'turn-hidden'")
            .bind(NOW)
            .execute(&pool)
            .await
            .unwrap();
        let error = read_activity_detail(&pool, &thread_id, &hidden)
            .await
            .unwrap_err();
        assert_eq!(error.code, ChatErrorCode::NotFound);
    });
}

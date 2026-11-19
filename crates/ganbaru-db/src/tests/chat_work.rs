//! Chat work schema: assignments, their reviews and delegation, semantic update kinds, runs,
//! delivered sources, and usage.

use super::helpers::{load_project_chat_graph, migrated_memory_pool};
use sqlx::AssertSqlSafe;

const NOW: &str = "2026-07-20T12:00:00Z";

#[test]
fn assignment_review_columns_accept_only_known_outcomes_and_full_timestamps() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let review = |outcome: &str, reviewed_at: &str, reviewer: &str| {
            sqlx::query(
                "UPDATE chat_work_assignments
                 SET state = 'completed', settled_at = ?, review_outcome = ?, reviewed_at = ?,
                     reviewed_by_participant_id = ?
                 WHERE id = 'assignment-1'",
            )
            .bind(NOW)
            .bind(outcome.to_owned())
            .bind(reviewed_at.to_owned())
            .bind(reviewer.to_owned())
            .execute(&pool)
        };

        assert!(
            review("approved", NOW, "participant:local-owner")
                .await
                .is_err()
        );
        assert!(
            review("accepted", "2026-07-20", "participant:local-owner")
                .await
                .is_err()
        );
        assert!(review("accepted", NOW, "").await.is_err());
        for reviewer in [
            "p".repeat(129),
            "é".repeat(65),
            "participant:\0local".to_owned(),
            "participant:\tlocal".to_owned(),
            "participant:local\u{7f}".to_owned(),
            "participant:\u{85}local".to_owned(),
        ] {
            assert!(
                review("accepted", NOW, &reviewer).await.is_err(),
                "{reviewer:?}"
            );
        }
        for reviewer in ["p".repeat(128), "participant:ñandú".to_owned()] {
            review("accepted", NOW, &reviewer).await.unwrap();
        }
        for outcome in ["accepted", "changes_requested"] {
            review(outcome, NOW, "participant:local-owner")
                .await
                .unwrap();
        }
        let stored: (String, String, String) = sqlx::query_as(
            "SELECT review_outcome, reviewed_at, reviewed_by_participant_id
             FROM chat_work_assignments WHERE id = 'assignment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            stored,
            (
                "changes_requested".to_owned(),
                NOW.to_owned(),
                "participant:local-owner".to_owned()
            )
        );
    });
}

#[test]
fn assignment_reviews_are_whole_and_only_on_completed_assignments() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let set = |state: &str,
                   outcome: Option<&str>,
                   reviewed_at: Option<&str>,
                   reviewer: Option<&str>| {
            sqlx::query(
                "UPDATE chat_work_assignments
                 SET state = ?, settled_at = ?, review_outcome = ?, reviewed_at = ?,
                     reviewed_by_participant_id = ?
                 WHERE id = 'assignment-1'",
            )
            .bind(state.to_owned())
            .bind(NOW)
            .bind(outcome.map(str::to_owned))
            .bind(reviewed_at.map(str::to_owned))
            .bind(reviewer.map(str::to_owned))
            .execute(&pool)
        };
        let owner = Some("participant:local-owner");
        let refused = |result: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>| {
            let error = result.expect_err("the table constraint refuses the review");
            assert!(
                error.to_string().contains("CHECK constraint failed"),
                "{error}"
            );
        };

        for state in ["working", "failed", "cancelled"] {
            refused(set(state, Some("accepted"), Some(NOW), owner).await);
        }
        for (outcome, reviewed_at, reviewer) in [
            (Some("accepted"), None, None),
            (Some("accepted"), Some(NOW), None),
            (None, Some(NOW), owner),
            (None, None, owner),
        ] {
            refused(set("completed", outcome, reviewed_at, reviewer).await);
        }

        set("completed", Some("accepted"), Some(NOW), owner)
            .await
            .unwrap();
        refused(
            sqlx::query(
                "UPDATE chat_work_assignments SET state = 'working' WHERE id = 'assignment-1'",
            )
            .execute(&pool)
            .await,
        );
        set("completed", None, None, None).await.unwrap();
    });
}

#[test]
fn delegated_assignments_sit_one_level_below_a_bounded_parent() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let ceiling = r#"[{"workingFolderId":"folder-managed","capability":"read"}]"#;
        let delegate = |id: &str,
                        parent: Option<&str>,
                        depth: i64,
                        objective: Option<String>,
                        ceiling: Option<&str>| {
            sqlx::query(
                "INSERT INTO chat_work_assignments
                     (id, reply_thread_id, agent_id, triggering_message_item_id, state,
                      model_selection_schema_version, model_selection_data,
                      parent_assignment_id, delegation_depth, delegation_objective,
                      delegation_expected_result, delegation_folder_ceiling, created_at,
                      updated_at)
                 VALUES (?, 'reply-thread', 'participant-agent', 'item-root', 'queued', 1, '{}',
                         ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_owned())
            .bind(parent.map(str::to_owned))
            .bind(depth)
            .bind(objective)
            .bind(parent.map(|_| "A short report".to_owned()))
            .bind(ceiling.map(str::to_owned))
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
        };
        let objective = || Some("Read the logs".to_owned());

        let stored: (Option<String>, i64) = sqlx::query_as(
            "SELECT parent_assignment_id, delegation_depth
             FROM chat_work_assignments WHERE id = 'assignment-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, (None, 0), "undelegated work defaults to depth 0");

        assert!(
            delegate("skip", Some("assignment-1"), 2, objective(), Some(ceiling))
                .await
                .is_err(),
            "a child sits exactly one level below its parent"
        );
        assert!(
            delegate("orphan-fields", None, 0, objective(), None)
                .await
                .is_err(),
            "undelegated work carries no delegation fields"
        );
        assert!(
            delegate("partial", Some("assignment-1"), 1, None, Some(ceiling))
                .await
                .is_err(),
            "delegation fields are all set together"
        );
        assert!(
            delegate("self", Some("self"), 1, objective(), Some(ceiling))
                .await
                .is_err(),
            "an assignment is never its own parent"
        );
        assert!(
            delegate("missing", Some("absent"), 1, objective(), Some(ceiling))
                .await
                .is_err(),
            "the parent must exist"
        );
        for invalid in ["{}", "not json"] {
            assert!(
                delegate(
                    "ceiling",
                    Some("assignment-1"),
                    1,
                    objective(),
                    Some(invalid)
                )
                .await
                .is_err(),
                "{invalid}"
            );
        }
        assert!(
            delegate(
                "long",
                Some("assignment-1"),
                1,
                Some("o".repeat(4001)),
                Some(ceiling)
            )
            .await
            .is_err()
        );

        delegate("child", Some("assignment-1"), 1, objective(), Some(ceiling))
            .await
            .unwrap();
        delegate("grandchild", Some("child"), 2, objective(), Some("[]"))
            .await
            .unwrap();
        assert!(
            delegate("too-deep", Some("grandchild"), 3, objective(), Some("[]"))
                .await
                .is_err(),
            "delegation stops at depth 2"
        );
        assert!(
            sqlx::query(
                "UPDATE chat_work_assignments SET delegation_objective = 'Other'
                 WHERE id = 'child'"
            )
            .execute(&pool)
            .await
            .is_err(),
            "delegation fields are fixed at creation"
        );

        sqlx::query("DELETE FROM chat_work_assignments WHERE id = 'assignment-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_work_assignments WHERE id IN ('child', 'grandchild')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "children are deleted with their parent");
    });
}

#[test]
fn assignments_require_a_frozen_model_and_bound_instructions_and_model_label() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let insert = |id: &str,
                      version: Option<i64>,
                      data: Option<&str>,
                      instructions: String,
                      label: Option<String>| {
            sqlx::query(
                "INSERT INTO chat_work_assignments
                    (id, reply_thread_id, agent_id, triggering_message_item_id,
                     previous_assignment_id, state, created_at, updated_at,
                     model_selection_schema_version, model_selection_data, instructions,
                     model_label)
                 VALUES (?, 'reply-thread', 'participant-agent', 'item-root',
                         'assignment-1', 'queued', ?, ?, ?, ?, ?, ?)",
            )
            .bind(id.to_owned())
            .bind(NOW)
            .bind(NOW)
            .bind(version)
            .bind(data.map(str::to_owned))
            .bind(instructions)
            .bind(label)
            .execute(&pool)
        };
        let selection = r#"{"modelId":"gpt-5","modelOptions":[]}"#;
        let frozen = |id: &str| insert(id, Some(1), Some(selection), String::new(), None);

        frozen("assignment-frozen").await.unwrap();
        insert(
            "assignment-long-instructions",
            Some(1),
            Some(selection),
            "i".repeat(50_176),
            Some("m".repeat(256)),
        )
        .await
        .unwrap();
        for (id, version, data) in [
            ("assignment-no-model", None, None),
            ("assignment-version-only", Some(1), None),
            ("assignment-data-only", None, Some(selection)),
            ("assignment-version-zero", Some(0), Some(selection)),
            ("assignment-array", Some(1), Some("[]")),
            ("assignment-text", Some(1), Some("model")),
        ] {
            assert!(
                insert(id, version, data, String::new(), None)
                    .await
                    .is_err(),
                "{id}"
            );
        }
        let oversized = format!(r#"{{"modelId":"{}"}}"#, "m".repeat(16_384));
        assert!(
            insert(
                "assignment-oversized",
                Some(1),
                Some(&oversized),
                String::new(),
                None
            )
            .await
            .is_err()
        );
        assert!(
            insert(
                "assignment-instructions-oversized",
                Some(1),
                Some(selection),
                "i".repeat(50_177),
                None
            )
            .await
            .is_err(),
            "the joined instructions stay within their bound"
        );
        for label in [String::new(), "m".repeat(257)] {
            assert!(
                insert(
                    "assignment-label",
                    Some(1),
                    Some(selection),
                    String::new(),
                    Some(label.clone())
                )
                .await
                .is_err(),
                "a model label holds 1 to 256 characters: {}",
                label.len()
            );
        }

        sqlx::query(
            "UPDATE chat_work_assignments SET model_label = 'GPT-5'
             WHERE id = 'assignment-frozen'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let label: Option<String> = sqlx::query_scalar(
            "SELECT model_label FROM chat_work_assignments WHERE id = 'assignment-frozen'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            label.as_deref(),
            Some("GPT-5"),
            "the executing device fills the label"
        );
    });
}

#[test]
fn semantic_updates_accept_the_delegation_and_proposal_kinds() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let insert = |item: &str, kind: &str, order_key: &str| {
            let item = item.to_owned();
            let order_key = order_key.to_owned();
            let kind = kind.to_owned();
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO chat_conversation_items
                         (id, conversation_id, item_kind, order_key, created_at)
                     VALUES (?, 'conversation-general', 'work_update', ?, ?)",
                )
                .bind(item.clone())
                .bind(order_key)
                .bind(NOW)
                .execute(&pool)
                .await
                .unwrap();
                sqlx::query(
                    "INSERT INTO chat_work_semantic_updates
                         (item_id, assignment_id, update_kind, payload_data, created_at)
                     VALUES (?, 'assignment-1', ?, '{}', ?)",
                )
                .bind(item)
                .bind(kind)
                .bind(NOW)
                .execute(&pool)
                .await
            }
        };
        for (index, kind) in ["review", "delegation", "schedule_proposal", "note_proposal"]
            .into_iter()
            .enumerate()
        {
            insert(&format!("item-{kind}"), kind, &format!("c10{index}"))
                .await
                .unwrap();
        }
        assert!(insert("item-summary", "summary", "c109").await.is_err());
    });
}

#[test]
fn assignment_input_schema_bounds_the_delivered_revision() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let insert = |id: &str, revision: &str| {
            sqlx::query(
                "INSERT INTO chat_work_assignment_inputs
                    (id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state,
                     delivered_at, delivered_revision_id, created_at)
                 VALUES (?, 'assignment-1', 'item-root', ?, 'trigger', 'delivered', ?, ?, ?)",
            )
            .bind(id.to_owned())
            .bind(if id == "input-long" { 1_i64 } else { 2_i64 })
            .bind(NOW)
            .bind(revision.to_owned())
            .bind(NOW)
            .execute(&pool)
        };

        assert!(insert("input-empty", "").await.is_err());
        assert!(insert("input-oversized", &"r".repeat(1025)).await.is_err());
        insert("input-long", &"r".repeat(128)).await.unwrap();
    });
}

#[test]
fn run_attempts_are_fixed_once_the_run_leaves_starting() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let stored: (String, Option<String>, String, i64) = sqlx::query_as(
            "SELECT attempt_reason, handoff_reason, context_delivery, delivered_context_bytes
             FROM chat_agent_runs WHERE id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, ("fresh".to_owned(), None, "full".to_owned(), 0));

        let digest = "a".repeat(64);
        for (column, value) in [
            ("attempt_reason", "'continuation'".to_owned()),
            ("handoff_reason", "'key_changed'".to_owned()),
            ("instructions_sha256", format!("'{digest}'")),
            ("model_selection_sha256", format!("'{digest}'")),
            ("context_delivery", "'delta'".to_owned()),
            ("delivered_context_bytes", "12".to_owned()),
        ] {
            let error = sqlx::query(AssertSqlSafe(format!(
                "UPDATE chat_agent_runs SET {column} = {value} WHERE id = 'run-1'"
            )))
            .execute(&pool)
            .await
            .unwrap_err();
            assert!(
                error.to_string().contains("Agent run attempt is immutable"),
                "{column}: {error}"
            );
        }

        sqlx::query("UPDATE chat_agent_runs SET state = 'starting' WHERE id = 'run-1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_agent_runs
             SET attempt_reason = 'handoff', handoff_reason = 'native_session_lost',
                 delivered_context_bytes = 131072
             WHERE id = 'run-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        for invalid in [
            "attempt_reason = 'resumed'",
            "handoff_reason = 'expired'",
            "instructions_sha256 = 'short'",
            "model_selection_sha256 = 'short'",
            "context_delivery = 'partial'",
            "delivered_context_bytes = 131073",
            "delivered_context_bytes = -1",
        ] {
            assert!(
                sqlx::query(AssertSqlSafe(format!(
                    "UPDATE chat_agent_runs SET {invalid} WHERE id = 'run-1'"
                )))
                .execute(&pool)
                .await
                .is_err(),
                "{invalid}"
            );
        }
    });
}

#[test]
fn delivered_sources_follow_their_run_and_accept_only_known_kinds() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let digest = "b".repeat(64);
        let deliver = |source_id: &str, content_sha256: &str, kind: &str, at: &str| {
            sqlx::query(
                "INSERT INTO chat_agent_run_delivered_sources
                    (run_id, source_id, content_sha256, delivery_kind, delivered_at)
                 VALUES ('run-1', ?, ?, ?, ?)",
            )
            .bind(source_id.to_owned())
            .bind(content_sha256.to_owned())
            .bind(kind.to_owned())
            .bind(at.to_owned())
            .execute(&pool)
        };
        for kind in [
            "package",
            "pointer",
            "steer",
            "queued_continuation",
            "removal",
        ] {
            deliver(&format!("source-{kind}"), &digest, kind, NOW)
                .await
                .unwrap();
        }
        assert!(
            deliver("source-package", &digest, "steer", NOW)
                .await
                .is_err()
        );
        deliver("source-package", &"c".repeat(64), "package", NOW)
            .await
            .unwrap();
        assert!(deliver("source-x", &digest, "summary", NOW).await.is_err());
        assert!(deliver("source-x", "short", "package", NOW).await.is_err());
        assert!(deliver("", &digest, "package", NOW).await.is_err());
        assert!(
            deliver("source-x", &digest, "package", "2026-07-20")
                .await
                .is_err()
        );

        sqlx::query("DELETE FROM chat_agent_runs WHERE id = 'run-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_agent_run_delivered_sources")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn run_usage_schema_starts_with_the_run_checks_counts_and_follows_its_run() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let (counted, since): (i64, Option<String>) = sqlx::query_as(
            "SELECT counted_tokens, working_since FROM chat_agent_run_usage
             WHERE run_id = 'run-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(counted, 0);
        assert_eq!(since.as_deref(), Some("2026-10-23T00:00:00.000Z"));

        for invalid in [
            "UPDATE chat_agent_run_usage SET input_tokens = 5 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET input_tokens = -1, counted_tokens = -1
             WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET cost_amount = 0.5 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET cost_amount = 0.5, cost_currency = ''
             WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET thread_input_tokens_last = 3 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET scope_unknown_logged = 2 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET working_seconds = -1 WHERE run_id = 'run-1'",
        ] {
            assert!(
                sqlx::query(AssertSqlSafe(invalid))
                    .execute(&pool)
                    .await
                    .is_err(),
                "{invalid}"
            );
        }
        sqlx::query(
            "UPDATE chat_agent_run_usage
             SET input_tokens = 10, cached_input_tokens = 90, cache_write_input_tokens = 2,
                 output_tokens = 5, counted_tokens = 17, cost_amount = 0.5, cost_currency = 'USD',
                 thread_input_tokens_last = 10, thread_cached_input_tokens_last = 90,
                 thread_cache_write_input_tokens_last = 2, thread_output_tokens_last = 5
             WHERE run_id = 'run-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM chat_agent_runs WHERE id = 'run-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_agent_run_usage WHERE run_id = 'run-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    });
}

#[test]
fn chat_work_carries_no_token_or_working_time_limits() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let budget_tables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE name = 'chat_assignment_budgets'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(budget_tables, 0);
        let limit_columns: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info('chat_agent_policy_revisions')
             WHERE name = 'default_assignment_token_limit'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(limit_columns, 0);
        let budget_bookkeeping: i64 = sqlx::query_scalar(
            "SELECT (SELECT count(*) FROM sync_rows WHERE table_id = 398)
                  + (SELECT count(*) FROM sync_seeded_tables WHERE table_id = 398)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(budget_bookkeeping, 0);
    });
}

#[test]
fn agent_usage_days_check_dates_and_counts_and_follow_their_agent() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let insert = |id: &str, date: &str, tokens: i64, seconds: i64, runs: i64| {
            sqlx::query(
                "INSERT INTO chat_agent_usage_daily
                     (id, device_id, agent_id, local_date, counted_tokens, working_seconds,
                      run_count)
                 VALUES (?, 'device-a', 'participant-agent', ?, ?, ?, ?)",
            )
            .bind(id.to_owned())
            .bind(date.to_owned())
            .bind(tokens)
            .bind(seconds)
            .bind(runs)
            .execute(&pool)
        };
        insert("usage-1", "2026-11-12", 10, 60, 1).await.unwrap();
        assert!(insert("usage-2", "2026/11/12", 0, 0, 0).await.is_err());
        assert!(insert("usage-3", "2026-11-1", 0, 0, 0).await.is_err());
        assert!(insert("usage-4", "2026-11-13", -1, 0, 0).await.is_err());
        assert!(
            insert("usage-5", "2026-11-13", 0, 8_640_001, 0)
                .await
                .is_err()
        );
        assert!(
            insert("usage-6", "2026-11-13", 0, 0, 100_001)
                .await
                .is_err()
        );

        for invalid in [
            "UPDATE chat_agent_run_usage SET summarized_counted_tokens = -1 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET summarized_working_seconds = -1 WHERE run_id = 'run-1'",
            "UPDATE chat_agent_run_usage SET summarized_run = 2 WHERE run_id = 'run-1'",
        ] {
            assert!(
                sqlx::query(AssertSqlSafe(invalid))
                    .execute(&pool)
                    .await
                    .is_err(),
                "{invalid}"
            );
        }

        for statement in [
            "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant-idle', 'ai_agent', 'Idle',
                     '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z')",
            "INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
             VALUES ('participant-idle', 'codex', '2026-10-23T00:00:00.000Z',
                     '2026-10-23T00:00:00.000Z')",
            "INSERT INTO chat_agent_usage_daily
                 (id, device_id, agent_id, local_date, counted_tokens, working_seconds, run_count)
             VALUES ('usage-idle', 'device-a', 'participant-idle', '2026-11-12', 1, 1, 1)",
            "DELETE FROM chat_agents WHERE participant_id = 'participant-idle'",
        ] {
            sqlx::query(AssertSqlSafe(statement))
                .execute(&pool)
                .await
                .unwrap();
        }
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT id FROM chat_agent_usage_daily ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, ["usage-1"]);
    });
}

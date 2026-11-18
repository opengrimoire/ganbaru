use super::helpers::{insert_default_channel, load_project_chat_graph, migrated_memory_pool};
use sqlx::{AssertSqlSafe, Row};

const NOW: &str = "2026-07-20T12:00:00Z";

async fn insert_project(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO project_groups (id, name, order_key) VALUES ('group-1', 'Engineering', 'a0')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name, order_key) VALUES ('project-1', 'group-1', 'Ganbaru', 'a0')",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_working_folder(pool: &sqlx::SqlitePool, id: &str, project_id: &str) {
    sqlx::query(
        "INSERT INTO project_working_folders
            (id, project_id, display_name, kind, repository_kind, created_at, updated_at)
         VALUES (?, ?, ?, 'external', 'none', ?, ?)",
    )
    .bind(id)
    .bind(project_id)
    .bind(format!("Workspace {id}"))
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chat_execution_environments
            (id, working_folder_id, kind, display_name, lifecycle_state, created_at, updated_at)
         VALUES (?, ?, 'current_folder', ?, 'available', ?, ?)",
    )
    .bind(format!("current-folder:{id}"))
    .bind(id)
    .bind(format!("Workspace {id}"))
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_thread(
    pool: &sqlx::SqlitePool,
    id: &str,
    working_folder_id: &str,
    project_id: &str,
) {
    sqlx::query(
        "INSERT INTO chat_threads
            (id, working_folder_id, project_id, title, provider_family_id,
             provider_instance_id, continuation_group_id, safety_mode,
             interaction_mode, state, last_activity_at, created_at, updated_at)
         VALUES (?, ?, ?, 'Implement Chat', 'codex', 'codex-personal',
                 'continuation-1', 'ask_for_approval', 'build', 'idle', ?, ?, ?)",
    )
    .bind(id)
    .bind(working_folder_id)
    .bind(project_id)
    .bind(NOW)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
    .unwrap();
}

#[test]
fn built_in_projects_have_one_protected_managed_working_folder() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let managed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_working_folders WHERE kind = 'managed' AND archived_at IS NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(managed, projects);
        let invalid_paths: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_working_folders AS folders
             LEFT JOIN project_managed_folder_locations AS locations
                 ON locations.working_folder_id = folders.id
             WHERE folders.kind = 'managed'
               AND (locations.folder_name IS NULL
                    OR folders.managed_relative_path != ('projects/' || locations.folder_name))",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(invalid_paths, 0);
        let archive = sqlx::query(
            "UPDATE project_working_folders SET archived_at = ? WHERE id = 'working-folder-routine-learning'",
        )
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(archive.is_err());
        let delete = sqlx::query(
            "DELETE FROM project_working_folders WHERE id = 'working-folder-routine-learning'",
        )
        .execute(&pool)
        .await;
        assert!(delete.is_err());
    });
}

#[test]
fn schema_creates_chat_tables_indexes_and_no_device_paths() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for object in [
            "project_working_folders",
            "chat_threads",
            "chat_turns",
            "chat_messages",
            "chat_activities",
            "chat_pending_requests",
            "chat_plans",
            "chat_drafts",
            "chat_attachments",
            "chat_attachment_references",
            "chat_queued_followups",
            "chat_queued_attachment_references",
            "chat_user_input_drafts",
            "chat_events",
            "chat_command_receipts",
            "chat_checkpoints",
            "chat_cleanup_jobs",
            "chat_checkpoint_failures",
            "chat_restore_previews",
            "chat_restore_operations",
            "chat_terminal_attachment_contexts",
            "chat_execution_environments",
            "chat_thread_relations",
            "chat_worktrees",
            "chat_review_comments",
            "chat_resources",
            "chat_resource_thread_references",
            "chat_preview_tabs",
            "chat_browser_artifacts",
            "chat_provider_cleanup_jobs",
            "chat_terminal_layouts",
            "idx_chat_terminal_layouts_working_folder",
            "chat_participants",
            "chat_agents",
            "chat_agent_policy_revisions",
            "chat_conversations",
            "chat_channels",
            "chat_conversation_memberships",
            "chat_agent_app_settings",
            "chat_project_agent_settings",
            "chat_push_approvals",
            "chat_agent_channel_memberships",
            "chat_conversation_audience_state",
            "chat_message_references",
            "chat_participant_reference_targets",
            "chat_channel_reference_targets",
            "chat_working_folder_reference_targets",
            "chat_workspace_path_reference_targets",
            "chat_execution_environment_reference_targets",
            "chat_assignment_authorization_revisions",
            "chat_assignment_authorized_channel_sources",
            "chat_assignment_authorized_folder_sources",
            "chat_host_tool_invocations",
            "chat_host_tool_returned_message_revisions",
            "chat_preview_tool_invocations",
            "chat_scratch_scopes",
            "chat_scratch_generations",
            "chat_scratch_generation_sources",
            "chat_scratch_promotions",
            "chat_scratch_cleanup_jobs",
            "chat_access_revocation_jobs",
            "chat_reply_threads",
            "chat_conversation_items",
            "chat_communication_messages",
            "chat_communication_message_revisions",
            "chat_communication_message_revisions_numbered",
            "chat_communication_attachment_references",
            "chat_conversation_read_cursors",
            "chat_reply_thread_read_cursors",
            "chat_organizational_command_receipts",
            "chat_work_assignments",
            "chat_work_assignment_inputs",
            "chat_work_semantic_updates",
            "chat_assignment_context_packages",
            "chat_assignment_context_sources",
            "chat_agent_runs",
            "chat_agent_run_delivered_sources",
            "chat_agent_run_usage",
            "chat_agent_usage_daily",
            "chat_secret_requests",
            "chat_agent_run_attempt_immutable",
            "chat_assignment_dispatch_jobs",
            "chat_assignment_requested_targets",
            "chat_assignment_input_requested_targets",
            "chat_scheduled_messages",
            "chat_push_request_claims",
            "chat_scheduled_message_attachment_references",
            "chat_scheduled_message_references",
            "project_primary_working_folders",
            "chat_communication_search_fts",
            "idx_chat_threads_active_project",
            "idx_chat_threads_active_working_folder",
            "idx_chat_threads_archived",
            "idx_chat_threads_title_search",
            "idx_chat_events_thread_sequence",
            "idx_chat_messages_thread_sequence",
            "idx_chat_activities_thread_sequence",
            "idx_chat_pending_requests_unresolved",
            "idx_chat_turns_thread_ordinal",
            "idx_chat_checkpoints_thread_turn",
            "idx_chat_attachment_references_message",
            "idx_chat_queued_followups_active",
            "idx_chat_queued_attachment_references_attachment",
            "idx_chat_checkpoint_failures_thread",
            "idx_chat_restore_previews_thread",
            "idx_chat_restore_operations_thread",
            "idx_chat_events_valid_thread_sequence",
            "idx_chat_threads_execution_environment",
            "idx_chat_threads_scratch_generation",
            "idx_chat_execution_environments_current_folder",
            "idx_chat_execution_environments_scratch",
            "idx_chat_worktrees_cleanup",
            "idx_chat_review_comments_thread_path",
            "idx_chat_review_comments_thread_queue",
            "idx_chat_resources_workspace_kind",
            "idx_chat_browser_artifacts_thread",
            "idx_chat_provider_cleanup_jobs_retry",
            "idx_chat_channels_project_default",
            "idx_chat_channels_active_project",
            "idx_chat_channels_conversation",
            "chat_participants_local_user_identity",
            "idx_chat_conversation_memberships_participant",
            "idx_chat_push_approvals_folder",
            "idx_chat_agent_channel_memberships_agent",
            "idx_chat_assignment_authorization_revisions_active",
            "idx_chat_host_tool_invocations_authorization",
            "idx_chat_secret_requests_run",
            "idx_chat_secret_requests_assignment",
            "idx_chat_preview_tool_invocations_turn",
            "idx_chat_scratch_generations_active",
            "idx_chat_scratch_promotions_generation",
            "idx_chat_scratch_promotions_destination",
            "idx_chat_scratch_cleanup_jobs_ready",
            "idx_chat_access_revocation_jobs_ready",
            "idx_chat_conversation_items_order",
            "idx_chat_reply_threads_conversation",
            "idx_chat_message_references_revision",
            "idx_chat_work_assignments_agent",
            "chat_reply_threads_root_item_insert",
            "chat_conversation_items_reply_thread_insert",
            "chat_communication_message_current_revision_update",
            "chat_communication_search_delete",
            "chat_communication_search_insert",
            "chat_communication_message_revision_immutable_update",
            "chat_authorized_channel_source_immutable_delete",
            "chat_authorized_channel_source_immutable_update",
            "chat_scratch_generation_source_delete_denied",
            "chat_scratch_generation_source_monotonic",
            "idx_chat_work_assignments_thread",
            "idx_chat_assignment_dispatch_jobs_ready",
            "idx_chat_scheduled_messages_device",
            "idx_chat_scheduled_messages_destination",
            "idx_chat_scheduled_message_attachment_references_attachment",
            "idx_chat_scheduled_message_attachment_references_digest",
            "idx_chat_communication_attachment_references_attachment",
            "idx_chat_communication_attachment_references_digest",
            "idx_chat_attachments_path",
            "idx_chat_resources_path",
            "idx_chat_scheduled_message_references_folder",
            "chat_timeline_change_clocks",
            "chat_timeline_removals",
            "idx_chat_timeline_removals_removed_at",
            "idx_chat_messages_thread_change",
            "idx_chat_activities_thread_change",
            "idx_chat_plans_thread_change",
            "idx_chat_turns_thread_change",
            "chat_messages_timeline_change_insert",
            "chat_messages_timeline_change_update",
            "chat_messages_timeline_change_delete",
            "chat_activities_timeline_change_insert",
            "chat_activities_timeline_change_update",
            "chat_activities_timeline_change_delete",
            "chat_plans_timeline_change_insert",
            "chat_plans_timeline_change_update",
            "chat_plans_timeline_change_delete",
            "chat_turns_timeline_change_insert",
            "chat_turns_timeline_change_update",
        ] {
            let exists: Option<i64> =
                sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE name = ?")
                    .bind(object)
                    .fetch_optional(&pool)
                    .await
                    .unwrap();
            assert_eq!(exists, Some(1), "{object} should exist");
        }

        for table in [
            "project_working_folders",
            "chat_threads",
            "chat_turns",
            "chat_messages",
            "chat_events",
            "chat_execution_environments",
            "chat_worktrees",
            "chat_resources",
            "chat_browser_artifacts",
            "chat_scratch_scopes",
            "chat_scratch_generations",
            "chat_scratch_promotions",
            "chat_scratch_cleanup_jobs",
        ] {
            let columns = sqlx::query(AssertSqlSafe(format!(
                "SELECT name FROM pragma_table_info('{table}')"
            )))
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
            assert!(!columns.iter().any(|column| {
                column.contains("absolute_path")
                    || column.contains("canonical_path")
                    || column.contains("credential")
                    || column.contains("secret")
            }));
        }

        let channel_columns = sqlx::query("SELECT name FROM pragma_table_info('chat_channels')")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
        for removed in [
            "working_folder_id",
            "provider_instance_id",
            "model_selection_data",
        ] {
            assert!(
                !channel_columns.iter().any(|column| column == removed),
                "chat_channels.{removed} should not exist"
            );
        }
        let legacy_sessions: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM sqlite_schema WHERE name = 'chat_channel_sessions'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert_eq!(legacy_sessions, None);

        let promotion_columns = sqlx::query(
            "SELECT name, \"notnull\" AS required
             FROM pragma_table_info('chat_scratch_promotions')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let destination_channel = promotion_columns
            .iter()
            .find(|row| row.get::<String, _>("name") == "destination_channel_id")
            .expect("scratch promotions should retain their exact destination channel");
        let destination_folder = promotion_columns
            .iter()
            .find(|row| row.get::<String, _>("name") == "destination_working_folder_id")
            .expect("scratch promotions should record working-folder destinations");
        assert_eq!(destination_channel.get::<i64, _>("required"), 1);
        assert_eq!(destination_folder.get::<i64, _>("required"), 0);
    });
}

#[test]
fn fresh_vaults_seed_primary_folders_but_no_chat_conversations_or_pending_keys() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let primary_folders: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_primary_working_folders")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(projects > 0);
        assert_eq!(primary_folders, projects);
        // Default channels are created on demand with derived ids, so a fresh vault holds no
        // conversation rows whose random ids would duplicate across devices.
        for table in [
            "chat_conversations",
            "chat_channels",
            "chat_conversation_memberships",
            "chat_conversation_audience_state",
        ] {
            let rows: i64 =
                sqlx::query_scalar(AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(rows, 0, "{table}");
        }
        for table in [
            "chat_conversation_memberships",
            "chat_agent_channel_memberships",
            "chat_push_approvals",
            "chat_conversation_read_cursors",
            "chat_reply_thread_read_cursors",
        ] {
            let pending: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT count(*) FROM {table} WHERE sync_key GLOB 'pending:*'"
            )))
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(pending, 0, "{table}");
        }
    });
}

#[test]
fn fresh_chat_has_only_the_local_owner_and_unprivileged_profile_recipes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let participants: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_participants")
            .fetch_one(&pool)
            .await
            .unwrap();
        let local_owners: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_participants
             WHERE id = 'participant:local-owner' AND participant_kind = 'local_user'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let ai_agents: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_participants WHERE participant_kind = 'ai_agent'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(participants, 1);
        assert_eq!(local_owners, 1);
        assert_eq!(ai_agents, 0);
    });
}

#[test]
fn standalone_agent_identity_is_inert_until_a_channel_adds_it() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('agent:inert', 'ai_agent', 'Inert agent', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_agents (participant_id, provider_family_id, created_at, updated_at)
             VALUES ('agent:inert', 'codex', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        for table in [
            "chat_conversation_memberships",
            "chat_agent_channel_memberships",
            "chat_push_approvals",
        ] {
            let column = if table == "chat_conversation_memberships" {
                "participant_id"
            } else {
                "agent_id"
            };
            let rows: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
                "SELECT count(*) FROM {table} WHERE {column} = 'agent:inert'"
            )))
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(rows, 0, "{table}");
        }

        let agent_columns = sqlx::query("SELECT name FROM pragma_table_info('chat_agents')")
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
        assert_eq!(
            agent_columns,
            [
                "participant_id",
                "provider_family_id",
                "current_policy_revision_id",
                "created_at",
                "updated_at"
            ],
            "access and instructions live on the vault, project, and channel, not the agent"
        );
        let access_state: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_schema WHERE name IN (
                 'chat_agent_access_state', 'chat_agent_working_folder_grants'
             )",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(access_state, None);
    });
}

#[test]
fn organizational_access_schema_has_one_authority_path_and_optional_native_targets() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let legacy_authorization: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_schema
             WHERE type = 'table' AND name = 'chat_assignment_authorization_decisions'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        let legacy_reference_tables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_schema
             WHERE type = 'table' AND name IN (
               'chat_participant_mentions',
               'chat_communication_resource_references',
               'chat_scheduled_message_resource_references'
             )",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let membership_columns =
            sqlx::query("SELECT name FROM pragma_table_info('chat_conversation_memberships')")
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .map(|row| row.get::<String, _>("name"))
                .collect::<Vec<_>>();
        let thread_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'chat_threads'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let run_sql: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'chat_agent_runs'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(legacy_authorization, None);
        assert_eq!(legacy_reference_tables, 0);
        assert!(
            !membership_columns
                .iter()
                .any(|column| column == "addressable")
        );
        assert!(
            !membership_columns
                .iter()
                .any(|column| column == "approval_policy")
        );
        assert!(thread_sql.contains("scratch_generation_id"));
        assert!(
            thread_sql.contains("working_folder_id IS NULL AND execution_environment_id IS NULL")
        );
        assert!(run_sql.contains("authorization_revision_id"));
        assert!(run_sql.contains("scratch_generation_id"));
        assert!(run_sql.contains("working_folder_id IS NULL AND execution_environment_id IS NULL"));
    });
}

#[test]
fn authorization_source_snapshots_are_immutable_and_folder_policy_is_required() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let mut connection = pool.acquire().await.unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=OFF")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO project_working_folders
                (id, project_id, display_name, kind, created_at, updated_at)
             VALUES ('working-folder:test', 'project:test', 'Test', 'external', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_assignment_authorization_revisions
                (id, assignment_id, revision, requester_participant_id,
                 destination_conversation_id,
                 resolved_runtime_approval_policy, scope_digest, decision_state, created_at)
             VALUES ('authorization:test', 'assignment:test', 1, 'participant:local-owner',
                     'conversation:test', 'ask', ?, 'allowed', ?)",
        )
        .bind("d".repeat(64))
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();

        let missing_policy = sqlx::query(
            "INSERT INTO chat_assignment_authorized_folder_sources
                (authorization_revision_id, root_handle, working_folder_id,
                 capability, is_execution_target, created_at)
             VALUES ('authorization:test', ?, 'working-folder:test', 'read', 0, ?)",
        )
        .bind("r".repeat(32))
        .bind(NOW)
        .execute(&mut *connection)
        .await;
        assert!(missing_policy.is_err());

        sqlx::query(
            "INSERT INTO chat_assignment_authorized_folder_sources
                (authorization_revision_id, root_handle, working_folder_id,
                 capability, is_execution_target, created_at,
                 resolved_runtime_approval_policy)
             VALUES ('authorization:test', ?, 'working-folder:test', 'read', 0, ?, 'ask')",
        )
        .bind("r".repeat(32))
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_assignment_authorized_channel_sources
                (authorization_revision_id, source_handle, message_reference_id,
                 conversation_id, label_snapshot, lower_order_key, lower_item_id,
                 high_order_key, high_item_id, source_revision_cutoff_id,
                 destination_audience_revision, created_at)
             VALUES ('authorization:test', ?, 'reference:test', 'conversation:test',
                     'Source', 'c000', '', 'c000', 'item:test', 'revision:test', 1, ?)",
        )
        .bind("s".repeat(32))
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();
        // Delete guards stand down once any parent is gone, so every parent must exist here.
        sqlx::query(
            "INSERT INTO chat_conversations
                (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
             VALUES ('conversation:test', 'project:test', 'channel', ?1, ?1, ?1)",
        )
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_communication_message_revisions
                (id, message_item_id, normalized_markdown, created_at)
             VALUES ('revision:test', 'item:test', 'Source', ?)",
        )
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_message_references
                (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
                 start_offset, end_offset, ordinal, created_at)
             VALUES ('reference:test', 'revision:test', 'working_folder', 'Test', 'Test',
                     0, 4, 0, ?)",
        )
        .bind(NOW)
        .execute(&mut *connection)
        .await
        .unwrap();

        for statement in [
            "UPDATE chat_assignment_authorized_folder_sources SET capability = 'edit'",
            "DELETE FROM chat_assignment_authorized_folder_sources",
            "UPDATE chat_assignment_authorized_channel_sources SET label_snapshot = 'Changed'",
            "DELETE FROM chat_assignment_authorized_channel_sources",
        ] {
            let result = sqlx::query(statement).execute(&mut *connection).await;
            assert!(
                result.is_err(),
                "source snapshot mutation should fail: {statement}"
            );
        }

        sqlx::raw_sql("PRAGMA foreign_keys=ON")
            .execute(&mut *connection)
            .await
            .unwrap();
    });
}

#[test]
fn scheduled_message_attachments_are_held_by_value_until_the_schedule_is_removed() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_default_channel(&pool).await;
        let (channel_id, working_folder_id): (String, String) = sqlx::query_as(
            "SELECT channel.id, folder.working_folder_id
             FROM chat_channels channel
             JOIN project_primary_working_folders folder
               ON folder.project_id = channel.project_id
             WHERE channel.is_default = 1 LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_attachments
                (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                 sha256, managed_relative_path, signature_kind, created_at)
             VALUES ('scheduled-attachment', ?, 'image', 'scheduled.png', 'image/png', 8,
                     'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
                     'assets/chat/attachments/scheduled.png', 'png', ?)",
        )
        .bind(&working_folder_id)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_scheduled_messages
                (id, client_command_id, channel_id, request_data, scheduled_for,
                 created_at, updated_at)
             VALUES ('scheduled-message-1', 'scheduled-command-1', ?, '{}', ?, ?, ?)",
        )
        .bind(&channel_id)
        .bind("2026-08-03T15:00:00Z")
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_scheduled_message_attachment_references
                (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
                 original_display_name, mime_type, byte_size, sha256)
             SELECT 'scheduled-message-1', 0, id, working_folder_id, kind,
                    original_display_name, mime_type, byte_size, sha256
             FROM chat_attachments WHERE id = 'scheduled-attachment'",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM chat_attachments WHERE id = 'scheduled-attachment'")
            .execute(&pool)
            .await
            .unwrap();
        let kept: (String, String, i64) = sqlx::query_as(
            "SELECT original_display_name, mime_type, byte_size
             FROM chat_scheduled_message_attachment_references
             WHERE scheduled_message_id = 'scheduled-message-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            kept,
            ("scheduled.png".to_string(), "image/png".to_string(), 8),
            "the reference keeps the attachment by value without its row"
        );
        assert!(
            sqlx::query(
                "INSERT INTO chat_scheduled_message_attachment_references
                    (scheduled_message_id, ordinal, attachment_id, working_folder_id, kind,
                     original_display_name, mime_type, byte_size, sha256)
                 VALUES ('scheduled-message-1', 20, 'too-many', 'folder', 'image', 'x.png',
                         'image/png', 1, ?)",
            )
            .bind("b".repeat(64))
            .execute(&pool)
            .await
            .is_err(),
            "a scheduled message names at most 20 attachments"
        );
        sqlx::query("DELETE FROM chat_scheduled_messages WHERE id = 'scheduled-message-1'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_scheduled_message_attachment_references")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0, "removing the schedule removes its references");
    });
}

#[test]
fn chat_workspace_schema_preserves_attachments_and_scopes_resources_to_threads() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "folder-1", "project-1").await;
        insert_thread(&pool, "thread-1", "folder-1", "project-1").await;
        sqlx::query(
            "INSERT INTO chat_messages
                (id, thread_id, sequence_anchor, role, normalized_markdown, streaming_state, created_at, updated_at)
             VALUES ('message-1', 'thread-1', 0, 'user', 'Inspect this image', 'complete', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_attachments
                (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                 sha256, managed_relative_path, signature_kind, created_at)
             VALUES ('resource-1', 'folder-1', 'image', 'screen.png', 'image/png', 8,
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     'assets/chat/attachments/resource-1.png', 'png', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_attachment_references
                (id, attachment_id, message_id, created_at)
             VALUES ('reference-1', 'resource-1', 'message-1', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let resource: (String, String, String) = sqlx::query_as(
            "SELECT resource_uri, managed_relative_path, integrity_state
             FROM chat_resources WHERE id = 'resource-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(resource.0, "ganbaru://chat/resource/resource-1");
        assert_eq!(resource.1, "assets/chat/attachments/resource-1.png");
        assert_eq!(resource.2, "verified");
        let visible: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_resource_thread_references
             WHERE resource_id = 'resource-1' AND thread_id = 'thread-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(visible, 1);

        sqlx::query("DELETE FROM chat_threads WHERE id = 'thread-1'")
            .execute(&pool)
            .await
            .unwrap();
        let resource_remains: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM chat_resources WHERE id = 'resource-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let references_remain: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM chat_resource_thread_references WHERE resource_id = 'resource-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(resource_remains, 1);
        assert_eq!(references_remain, 0);
    });
}

#[test]
fn chat_workspace_schema_records_forks_worktrees_reviews_and_cleanup_failures() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "folder-1", "project-1").await;
        insert_thread(&pool, "thread-parent", "folder-1", "project-1").await;
        insert_thread(&pool, "thread-child", "folder-1", "project-1").await;
        sqlx::query(
            "INSERT INTO chat_thread_relations
                (child_thread_id, parent_thread_id, relation_kind, created_at)
             VALUES ('thread-child', 'thread-parent', 'fork', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_execution_environments
                (id, working_folder_id, kind, display_name, lifecycle_state, created_at, updated_at)
             VALUES ('worktree-1', 'folder-1', 'worktree', 'Feature worktree', 'cleanup_failed', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_worktrees
                (execution_environment_id, branch_name, base_reference, cleanup_state,
                 cleanup_error_code, cleanup_error_detail, created_at, updated_at)
             VALUES ('worktree-1', 'feat/workspace', 'origin/dev', 'failed',
                     'dirty_worktree', 'Worktree has local changes', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_review_comments
                (id, thread_id, relative_path, content_revision, start_line, end_line,
                 selected_text, comment_text, created_at, updated_at)
             VALUES ('review-1', 'thread-child', 'src/main.rs',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     4, 5, 'unsafe block', 'Can this stay safe?', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let environment: String = sqlx::query_scalar(
            "SELECT execution_environment_id FROM chat_threads WHERE id = 'thread-child'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let cleanup: (String, String) = sqlx::query_as(
            "SELECT cleanup_state, cleanup_error_code FROM chat_worktrees WHERE execution_environment_id = 'worktree-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(environment, "current-folder:folder-1");
        assert_eq!(
            cleanup,
            ("failed".to_string(), "dirty_worktree".to_string())
        );
    });
}

#[test]
fn chat_draft_schema_supports_plain_text_and_validates_rich_content() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO chat_drafts (id, working_folder_id, text, updated_at)
             VALUES ('draft-1', 'working-folder-routine-learning', 'Keep this', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let plain_text = sqlx::query(
            "SELECT text, rich_content_schema_version, rich_content_data
             FROM chat_drafts WHERE id = 'draft-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(plain_text.get::<String, _>("text"), "Keep this");
        assert_eq!(
            plain_text.get::<Option<i64>, _>("rich_content_schema_version"),
            None
        );
        assert_eq!(
            plain_text.get::<Option<String>, _>("rich_content_data"),
            None
        );

        sqlx::query(
            "UPDATE chat_drafts
             SET rich_content_schema_version = 1,
                 rich_content_data = '{\"type\":\"document\"}'
             WHERE id = 'draft-1'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let invalid_data = sqlx::query(
            "UPDATE chat_drafts SET rich_content_data = 'not-json' WHERE id = 'draft-1'",
        )
        .execute(&pool)
        .await;
        let invalid_version = sqlx::query(
            "UPDATE chat_drafts SET rich_content_schema_version = 0 WHERE id = 'draft-1'",
        )
        .execute(&pool)
        .await;
        assert!(invalid_data.is_err());
        assert!(invalid_version.is_err());
    });
}

#[test]
fn chat_review_comment_schema_applies_source_defaults_and_constraints() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "folder-1", "project-1").await;
        insert_thread(&pool, "thread-1", "folder-1", "project-1").await;
        sqlx::query(
            "INSERT INTO chat_review_comments
                (id, thread_id, relative_path, content_revision, start_line, end_line,
                 selected_text, comment_text, created_at, updated_at)
             VALUES ('review-1', 'thread-1', 'src/main.rs',
                     'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                     4, 5, 'unsafe block', 'Can this stay safe?', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT selected_text, comment_text, source_kind, source_data,
                    review_revision, snapshot_id, file_id, selection_side,
                    previous_relative_path, applicability, queued_for_send
             FROM chat_review_comments WHERE id = 'review-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("selected_text"), "unsafe block");
        assert_eq!(row.get::<String, _>("comment_text"), "Can this stay safe?");
        assert_eq!(row.get::<String, _>("source_kind"), "file");
        assert_eq!(row.get::<Option<String>, _>("source_data"), None);
        assert_eq!(row.get::<Option<String>, _>("review_revision"), None);
        assert_eq!(row.get::<Option<String>, _>("snapshot_id"), None);
        assert_eq!(row.get::<Option<String>, _>("file_id"), None);
        assert_eq!(row.get::<String, _>("selection_side"), "file");
        assert_eq!(row.get::<Option<String>, _>("previous_relative_path"), None);
        assert_eq!(row.get::<String, _>("applicability"), "current");
        assert_eq!(row.get::<i64, _>("queued_for_send"), 0);

        for invalid_update in [
            "UPDATE chat_review_comments SET source_kind = 'unsupported' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET source_data = 'not-json' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET review_revision = 'short' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET snapshot_id = '' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET file_id = '' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET selection_side = 'both' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET previous_relative_path = '../secret' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET applicability = 'unknown' WHERE id = 'review-1'",
            "UPDATE chat_review_comments SET queued_for_send = 2 WHERE id = 'review-1'",
        ] {
            assert!(
                sqlx::query(invalid_update).execute(&pool).await.is_err(),
                "constraint should reject: {invalid_update}"
            );
        }

        let queue_index: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sqlite_schema
             WHERE type = 'index' AND name = 'idx_chat_review_comments_thread_queue'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(queue_index, Some(1));
    });
}

#[test]
fn chat_permission_columns_accept_only_current_modes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for table in ["chat_threads", "chat_turns", "chat_drafts"] {
            let sql: String = sqlx::query_scalar(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            for mode in [
                "ask_for_approval",
                "approve_for_me",
                "full_access",
                "custom",
            ] {
                assert!(sql.contains(mode), "{table} should accept {mode}");
            }
            assert!(
                !sql.contains("supervised"),
                "{table} should reject legacy modes"
            );
            assert!(
                !sql.contains("auto_accept_edits"),
                "{table} should reject legacy modes"
            );
        }
    });
}

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
fn channel_membership_model_keeps_version_and_selection_together() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let selection = r#"{"modelId":"gpt-5","modelOptions":[]}"#;
        let oversized = format!(r#"{{"modelId":"{}"}}"#, "m".repeat(16_384));
        for (case, version, data, accepted) in [
            ("override", Some(1_i64), Some(selection), true),
            ("default", None, None, true),
            ("version-only", Some(1), None, false),
            ("data-only", None, Some(selection), false),
            ("version-zero", Some(0), Some(selection), false),
            ("array", Some(1), Some("[]"), false),
            ("oversized", Some(1), Some(oversized.as_str()), false),
        ] {
            let result = sqlx::query(
                "UPDATE chat_agent_channel_memberships
                 SET model_selection_schema_version = ?, model_selection_data = ?
                 WHERE conversation_id = 'conversation-general'
                   AND agent_id = 'participant-agent'",
            )
            .bind(version)
            .bind(data)
            .execute(&pool)
            .await;
            if accepted {
                result.unwrap();
            } else {
                assert!(result.is_err(), "{case} is refused");
            }
        }

        let reference_columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info('chat_participant_reference_targets') ORDER BY cid",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            reference_columns,
            ["reference_id", "participant_id"],
            "a mention names only its participant; the channel chooses the model"
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

#[test]
fn working_folder_tool_schema_tracks_restore_invalidation_and_exact_cleanup() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        for (table, expected) in [
            (
                "chat_checkpoints",
                vec![
                    "checkpoint_kind",
                    "turn_id",
                    "index_commit_oid",
                    "index_tree_oid",
                    "worktree_tree_oid",
                    "head_oid",
                    "head_ref",
                    "index_fingerprint",
                    "invalidated_at",
                    "invalidated_by_checkpoint_id",
                ],
            ),
            ("chat_turns", vec!["invalidated_at", "invalidation_reason"]),
            ("chat_events", vec!["invalidated_at", "invalidation_reason"]),
            (
                "chat_cleanup_jobs",
                vec!["working_folder_id", "expected_object_id"],
            ),
        ] {
            let columns = sqlx::query(AssertSqlSafe(format!(
                "SELECT name FROM pragma_table_info('{table}')"
            )))
            .fetch_all(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect::<Vec<_>>();
            for column in expected {
                assert!(
                    columns.iter().any(|value| value == column),
                    "{table}.{column} should exist"
                );
            }
        }
    });
}

#[test]
fn working_folder_project_identity_and_delete_policies_are_explicit() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "working-folder-project", "project-1").await;
        insert_thread(
            &pool,
            "thread-project",
            "working-folder-project",
            "project-1",
        )
        .await;

        let mismatched = sqlx::query(
            "INSERT INTO chat_threads
                (id, working_folder_id, project_id, title, provider_family_id,
                 provider_instance_id, continuation_group_id, safety_mode,
                 interaction_mode, state, last_activity_at, created_at, updated_at)
             VALUES ('thread-bad', 'working-folder-project', NULL, 'Bad', 'codex',
                     'codex-personal', 'continuation-1', 'ask_for_approval', 'build',
                     'idle', ?, ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(mismatched.is_err());

        insert_working_folder(&pool, "working-folder-other", "project-1").await;
        insert_thread(&pool, "thread-other", "working-folder-other", "project-1").await;
        sqlx::query("DELETE FROM project_working_folders WHERE id = 'working-folder-project'")
            .execute(&pool)
            .await
            .unwrap();
        let remaining: Vec<String> = sqlx::query_scalar("SELECT id FROM chat_threads ORDER BY id")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, vec!["thread-other".to_string()]);

        sqlx::query("DELETE FROM projects WHERE id = 'project-1'")
            .execute(&pool)
            .await
            .unwrap();
        let threads: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_threads")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(threads, 0);
    });
}

#[test]
fn event_sequences_and_unresolved_requests_are_unique() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "working-folder-1", "project-1").await;
        insert_thread(&pool, "thread-1", "working-folder-1", "project-1").await;

        let skipped_sequence = sqlx::query(
            "INSERT INTO chat_events
                (id, thread_id, sequence, provider_family_id, provider_instance_id,
                 event_type, payload_schema_version, payload_data, created_at, ingested_at)
             VALUES ('event-2', 'thread-1', 2, 'codex', 'codex-personal',
                     'turn_started', 1, '{}', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(skipped_sequence.is_err());

        sqlx::query(
            "INSERT INTO chat_events
                (id, thread_id, sequence, provider_family_id, provider_instance_id,
                 event_type, payload_schema_version, payload_data, created_at, ingested_at)
             VALUES ('event-1', 'thread-1', 1, 'codex', 'codex-personal',
                     'approval_requested', 999, '{\"future\":true}', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chat_threads
             SET last_event_sequence = 1, last_projected_sequence = 1
             WHERE id = 'thread-1'",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO chat_pending_requests
                (id, thread_id, provider_request_id, request_kind,
                 safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
             VALUES ('request-1', 'thread-1', 'provider-request-1', 'approval',
                     '{}', '[\"deny\"]', 1, ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let duplicate_open = sqlx::query(
            "INSERT INTO chat_pending_requests
                (id, thread_id, provider_request_id, request_kind,
                 safe_display_data, allowed_decisions_data, opened_sequence, opened_at)
             VALUES ('request-2', 'thread-1', 'provider-request-1', 'approval',
                     '{}', '[\"deny\"]', 1, ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await;
        assert!(duplicate_open.is_err());

        let payload: String =
            sqlx::query_scalar("SELECT payload_data FROM chat_events WHERE id = 'event-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(payload, "{\"future\":true}");
    });
}

#[test]
fn cleanup_queue_survives_thread_deletion_and_requires_exact_refs() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "working-folder-1", "project-1").await;
        insert_thread(&pool, "thread-1", "working-folder-1", "project-1").await;
        sqlx::query(
            "INSERT INTO chat_cleanup_jobs
                (id, working_folder_id, source_thread_id, cleanup_kind, exact_target,
                 repository_identity, available_at, created_at, updated_at)
             VALUES ('cleanup-1', 'working-folder-1', 'thread-1', 'checkpoint_ref',
                     'refs/ganbaru/chat/thread-1/checkpoint-1',
                     'git-sha256:repository', ?, ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("DELETE FROM chat_threads WHERE id = 'thread-1'")
            .execute(&pool)
            .await
            .unwrap();

        let target: String =
            sqlx::query_scalar("SELECT exact_target FROM chat_cleanup_jobs WHERE id = 'cleanup-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(target, "refs/ganbaru/chat/thread-1/checkpoint-1");
    });
}

#[test]
fn stored_relative_paths_reject_any_backslash_and_trailing_parent_segments() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        insert_project(&pool).await;
        insert_working_folder(&pool, "folder-1", "project-1").await;
        insert_thread(&pool, "thread-1", "folder-1", "project-1").await;
        let shape_error = "Stored relative paths cannot contain a backslash or a parent segment";

        let insert_attachment = |id: &'static str, path: &'static str| {
            sqlx::query(
                "INSERT INTO chat_attachments
                    (id, working_folder_id, kind, original_display_name, mime_type, byte_size,
                     sha256, managed_relative_path, signature_kind, created_at)
                 VALUES (?, 'folder-1', 'image', 'screen.png', 'image/png', 8,
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                         ?, 'png', ?)",
            )
            .bind(id)
            .bind(path)
            .bind(NOW)
            .execute(&pool)
        };
        for path in ["assets\\chat\\a.png", "assets/chat\\..\\a.png", ".."] {
            let error = insert_attachment("rejected", path).await.unwrap_err();
            assert!(error.to_string().contains(shape_error), "{path}: {error}");
        }
        insert_attachment("accepted", "assets/chat/attachments/a.png")
            .await
            .unwrap();
        let error = sqlx::query(
            "UPDATE chat_resources SET managed_relative_path = 'assets\\b.png'
             WHERE id = 'accepted'",
        )
        .execute(&pool)
        .await
        .unwrap_err();
        assert!(error.to_string().contains(shape_error), "{error}");

        let insert_comment = |id: &'static str, path: &'static str| {
            sqlx::query(
                "INSERT INTO chat_review_comments
                    (id, thread_id, relative_path, content_revision, start_line, end_line,
                     comment_text, created_at, updated_at)
                 VALUES (?, 'thread-1', ?,
                         'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                         1, 1, 'Check this', ?, ?)",
            )
            .bind(id)
            .bind(path)
            .bind(NOW)
            .bind(NOW)
            .execute(&pool)
        };
        for path in ["src\\main.rs", "src/..", ".."] {
            let error = insert_comment("rejected", path).await.unwrap_err();
            assert!(error.to_string().contains(shape_error), "{path}: {error}");
        }
        insert_comment("accepted", "src/main..rs").await.unwrap();

        // A folder path is immutable locally, so only an applied change reaches the shape check.
        sqlx::query("UPDATE sync_apply_state SET applying = 1")
            .execute(&pool)
            .await
            .unwrap();
        let error = sqlx::query(
            "UPDATE project_working_folders SET managed_relative_path = 'projects\\x'
             WHERE id = 'folder-1'",
        )
        .execute(&pool)
        .await
        .unwrap_err();
        assert!(error.to_string().contains(shape_error), "{error}");
    });
}

#[test]
fn host_tool_invocations_record_only_known_tool_classes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let request_hash = "a".repeat(64);
        let record = |id: &str, tool_class: &str| {
            sqlx::query(
                "INSERT INTO chat_host_tool_invocations
                    (id, authorization_revision_id, tool_name, tool_class, request_hash,
                     decision_state, created_at)
                 VALUES (?, 'authorization-1', 'chat_tool', ?, ?, 'allowed', ?)",
            )
            .bind(id.to_owned())
            .bind(tool_class.to_owned())
            .bind(request_hash.clone())
            .bind(NOW)
            .execute(&pool)
        };
        for tool_class in [
            "read",
            "edit",
            "destructive",
            "delegate",
            "ask",
            "report",
            "secret",
            "schedule",
        ] {
            record(&format!("invocation-{tool_class}"), tool_class)
                .await
                .unwrap();
        }
        assert!(record("invocation-unknown", "write").await.is_err());
        assert!(record("invocation-upper", "Read").await.is_err());

        sqlx::query(
            "INSERT INTO chat_host_tool_invocations
                (id, authorization_revision_id, tool_name, request_hash, decision_state,
                 created_at)
             VALUES ('invocation-default', 'authorization-1', 'chat_tool', ?, 'allowed', ?)",
        )
        .bind(&request_hash)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let defaulted: String = sqlx::query_scalar(
            "SELECT tool_class FROM chat_host_tool_invocations WHERE id = 'invocation-default'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(defaulted, "read");
    });
}

/// Inserts channel note `id` in the general channel with `body` written by `editor`.
async fn insert_channel_note(
    pool: &sqlx::SqlitePool,
    id: &str,
    body: &str,
    editor: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO chat_channel_notes
             (id, channel_id, body, edited_by_participant_id, created_at, updated_at)
         VALUES (?, 'channel-general', ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(body)
    .bind(editor)
    .bind(NOW)
    .bind(NOW)
    .execute(pool)
    .await
}

async fn set_sync_applying(pool: &sqlx::SqlitePool, applying: bool) {
    sqlx::query("UPDATE sync_apply_state SET applying = ? WHERE singleton = 1")
        .bind(i64::from(applying))
        .execute(pool)
        .await
        .unwrap();
}

/// Whether `error` is a refusal of a stored value, by the table's CHECK or by the sync value
/// trigger that runs before it.
fn is_value_refusal(error: &str) -> bool {
    error.contains("CHECK constraint failed") || error.contains("Replicated row values are invalid")
}

#[test]
fn channel_notes_are_indexed_by_channel_and_bound_their_body() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let index: String = sqlx::query_scalar(
            "SELECT group_concat(name, ',') FROM pragma_index_info('idx_chat_channel_notes_channel')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(index, "channel_id,deleted_at,updated_at");

        let owner = Some("participant:local-owner");
        insert_channel_note(&pool, "note-longest", &"n".repeat(1000), owner)
            .await
            .unwrap();
        for (id, body) in [
            ("note-empty", "   ".to_owned()),
            ("note-long", "n".repeat(1001)),
        ] {
            let error = insert_channel_note(&pool, id, &body, owner)
                .await
                .unwrap_err()
                .to_string();
            assert!(is_value_refusal(&error), "{error}");
        }
        let error = insert_channel_note(&pool, &"k".repeat(129), "Fact", owner)
            .await
            .unwrap_err()
            .to_string();
        assert!(is_value_refusal(&error), "{error}");

        sqlx::query("DELETE FROM chat_channels WHERE id = 'channel-general'")
            .execute(&pool)
            .await
            .unwrap();
        let notes: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_channel_notes")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(notes, 0, "notes are deleted with their channel");
    });
}

#[test]
fn channel_notes_are_written_only_by_people_outside_the_sync_applier() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        sqlx::query(
            "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant-person', 'human', 'Ana', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let refused = "Channel notes are edited by people";

        for editor in [Some("participant-agent"), None, Some("participant-missing")] {
            let error = insert_channel_note(&pool, "note-agent", "Fact", editor)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(refused), "{editor:?}: {error}");
        }
        insert_channel_note(&pool, "note-owner", "Fact", Some("participant:local-owner"))
            .await
            .unwrap();
        insert_channel_note(&pool, "note-person", "Fact", Some("participant-person"))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_push_approvals SET approved_by_participant_id = NULL
             WHERE destination_branch = 'release'",
        )
        .execute(&pool)
        .await
        .unwrap();

        for update in [
            "UPDATE chat_channel_notes SET body = 'Changed',
                 edited_by_participant_id = 'participant-agent' WHERE id = 'note-owner'",
            "UPDATE chat_channel_notes SET deleted_at = '2026-07-20T12:05:00.000Z',
                 edited_by_participant_id = 'participant-agent' WHERE id = 'note-owner'",
            "UPDATE chat_channel_notes SET edited_by_participant_id = NULL, body = 'Changed'
             WHERE id = 'note-owner'",
        ] {
            let error = sqlx::query(update)
                .execute(&pool)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(refused), "{error}");
        }
        sqlx::query(
            "UPDATE chat_channel_notes SET body = 'Changed', updated_at = '2026-07-20T12:05:00.000Z'
             WHERE id = 'note-owner'",
        )
        .execute(&pool)
        .await
        .unwrap();

        set_sync_applying(&pool, true).await;
        insert_channel_note(&pool, "note-applied", "Fact", Some("participant-agent"))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_channel_notes SET body = 'Applied', edited_by_participant_id = NULL
             WHERE id = 'note-owner'",
        )
        .execute(&pool)
        .await
        .unwrap();
        set_sync_applying(&pool, false).await;

        sqlx::query("DELETE FROM chat_participants WHERE id = 'participant-person'")
            .execute(&pool)
            .await
            .unwrap();
        let kept: Option<String> = sqlx::query_scalar(
            "SELECT body FROM chat_channel_notes
             WHERE id = 'note-person' AND edited_by_participant_id IS NULL",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            kept.as_deref(),
            Some("Fact"),
            "removing a person keeps their notes"
        );
    });
}

/// Inserts a standing push approval of the fixture agent in the fixture channel for the managed
/// folder, made by `approver`.
async fn insert_push_approval(
    pool: &sqlx::SqlitePool,
    branch: &str,
    approver: Option<&str>,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO chat_push_approvals
             (conversation_id, agent_id, working_folder_id, push_url, destination_branch,
              sync_key, approved_by_participant_id, approved_at)
         VALUES ('conversation-general', 'participant-agent', 'folder-managed',
                 'git@example.com:team/app.git', ?, 'push-approval-' || ?, ?, ?)",
    )
    .bind(branch)
    .bind(branch)
    .bind(approver)
    .bind(NOW)
    .execute(pool)
    .await
}

/// The revocation time and revision of the approval for `branch`.
async fn push_approval_state(pool: &sqlx::SqlitePool, branch: &str) -> (Option<String>, i64) {
    let row = sqlx::query(
        "SELECT revoked_at, revision FROM chat_push_approvals WHERE destination_branch = ?",
    )
    .bind(branch)
    .fetch_one(pool)
    .await
    .unwrap();
    (row.get("revoked_at"), row.get("revision"))
}

#[test]
fn push_approvals_are_made_only_by_people_outside_the_sync_applier() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        sqlx::query(
            "INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('participant-person', 'human', 'Ana', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let refused = "Push approvals are made by people";

        for approver in [Some("participant-agent"), None, Some("participant-missing")] {
            let error = insert_push_approval(&pool, "main", approver)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(refused), "{approver:?}: {error}");
        }
        insert_push_approval(&pool, "main", Some("participant:local-owner"))
            .await
            .unwrap();
        insert_push_approval(&pool, "release", Some("participant-person"))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE chat_push_approvals SET approved_by_participant_id = NULL
             WHERE destination_branch = 'release'",
        )
        .execute(&pool)
        .await
        .unwrap();

        for update in [
            "UPDATE chat_push_approvals SET approved_by_participant_id = 'participant-agent'
             WHERE destination_branch = 'main'",
            "UPDATE chat_push_approvals SET approved_by_participant_id = NULL,
                 approved_at = '2026-07-20T12:05:00.000Z' WHERE destination_branch = 'main'",
            "UPDATE chat_push_approvals SET approved_at = '2026-07-20T12:05:00.000Z'
             WHERE destination_branch = 'release' AND approved_by_participant_id IS NULL",
        ] {
            let error = sqlx::query(update)
                .execute(&pool)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(refused), "{error}");
        }

        set_sync_applying(&pool, true).await;
        insert_push_approval(&pool, "applied", Some("participant-agent"))
            .await
            .unwrap();
        set_sync_applying(&pool, false).await;

        insert_push_approval(&pool, "person", Some("participant-person"))
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_participants WHERE id = 'participant-person'")
            .execute(&pool)
            .await
            .unwrap();
        let kept: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_push_approvals
             WHERE revoked_at IS NULL AND sync_key GLOB 'push-approval-*'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept, 4, "removing the approver keeps the approvals");
    });
}

#[test]
fn push_approvals_are_revoked_when_the_agent_leaves_the_channel() {
    let removals: [(&str, &str); 3] = [
        (
            "membership removed",
            "UPDATE chat_agent_channel_memberships SET removed_at = '2026-07-20T12:05:00.000Z'
             WHERE sync_key = 'channel-membership-agent'",
        ),
        (
            "membership deleted",
            "DELETE FROM chat_agent_channel_memberships
             WHERE sync_key = 'channel-membership-agent'",
        ),
        (
            "agent archived",
            "UPDATE chat_participants SET archived_at = '2026-07-20T12:05:00.000Z'
             WHERE id = 'participant-agent'",
        ),
    ];
    super::block_on(async {
        for (case, statement) in removals {
            let pool = migrated_memory_pool().await;
            load_project_chat_graph(&pool).await;
            insert_push_approval(&pool, "main", Some("participant:local-owner"))
                .await
                .unwrap();
            sqlx::query(AssertSqlSafe(statement))
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{case}: {error}"));
            let rows =
                sqlx::query("SELECT sync_key, revoked_at, revision FROM chat_push_approvals")
                    .fetch_all(&pool)
                    .await
                    .unwrap();
            assert_eq!(rows.len(), 3, "{case}");
            for row in rows {
                let key: String = row.get("sync_key");
                let revoked_at: Option<String> = row.get("revoked_at");
                let revision: i64 = row.get("revision");
                assert!(revoked_at.is_some(), "{case} revokes {key}");
                assert_eq!(revision, 2, "{case} bumps the {key} revision once");
            }
        }
    });
}

#[test]
fn push_approvals_survive_unrelated_and_applied_changes() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        insert_push_approval(&pool, "main", Some("participant:local-owner"))
            .await
            .unwrap();
        for statement in [
            "UPDATE chat_channels SET agent_access = 'read' WHERE id = 'channel-general'",
            "UPDATE chat_agent_channel_memberships
             SET model_selection_schema_version = 1, model_selection_data = '{}'
             WHERE sync_key = 'channel-membership-agent'",
            "UPDATE chat_participants SET display_name = 'Renamed' WHERE id = 'participant-agent'",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        set_sync_applying(&pool, true).await;
        sqlx::query(
            "UPDATE chat_agent_channel_memberships SET removed_at = '2026-07-20T12:05:00.000Z'
             WHERE sync_key = 'channel-membership-agent'",
        )
        .execute(&pool)
        .await
        .unwrap();
        set_sync_applying(&pool, false).await;
        assert_eq!(
            push_approval_state(&pool, "main").await,
            (None, 1),
            "an applied removal leaves the revocation to the device that removed the agent"
        );

        sqlx::query("DELETE FROM projects WHERE id = 'project-chat'")
            .execute(&pool)
            .await
            .unwrap();
        let approvals: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_push_approvals")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(approvals, 0, "approvals are deleted with their folder");
    });
}

async fn terminal_layout_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM chat_terminal_layouts")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[test]
fn terminal_layouts_are_keyed_by_channel_and_folder_and_follow_both() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let (_, channel_id) = insert_default_channel(&pool).await;
        insert_working_folder(&pool, "folder-docs", "project-routine-learning").await;
        let insert = |folder: &'static str| {
            sqlx::query(
                "INSERT INTO chat_terminal_layouts (channel_id, working_folder_id, updated_at)
                 VALUES (?, ?, ?)",
            )
            .bind(channel_id.clone())
            .bind(folder)
            .bind(NOW)
        };
        insert("working-folder-routine-learning")
            .execute(&pool)
            .await
            .unwrap();
        insert("folder-docs").execute(&pool).await.unwrap();
        assert!(
            insert("folder-docs").execute(&pool).await.is_err(),
            "one layout per channel and folder"
        );
        assert_eq!(terminal_layout_count(&pool).await, 2);

        sqlx::query("DELETE FROM project_working_folders WHERE id = 'folder-docs'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            terminal_layout_count(&pool).await,
            1,
            "layouts are deleted with their folder"
        );
        sqlx::query("DELETE FROM chat_channels WHERE id = ?")
            .bind(&channel_id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            terminal_layout_count(&pool).await,
            0,
            "layouts are deleted with their channel"
        );
    });
}

#[test]
fn app_agent_settings_are_one_seeded_row_asking_with_empty_instructions() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let row = sqlx::query(
            "SELECT id, agent_access, agent_instructions, revision FROM chat_agent_app_settings",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.len(),
            1,
            "a fresh vault seeds exactly one app settings row"
        );
        assert_eq!(row[0].get::<String, _>("id"), "vault");
        assert_eq!(row[0].get::<String, _>("agent_access"), "ask");
        assert_eq!(row[0].get::<String, _>("agent_instructions"), "");
        assert_eq!(row[0].get::<i64, _>("revision"), 1);

        assert!(
            sqlx::query(
                "INSERT INTO chat_agent_app_settings (id, updated_at)
                 VALUES ('second', '2026-10-23T00:00:00.000Z')",
            )
            .execute(&pool)
            .await
            .is_err(),
            "the app settings row is a singleton"
        );
        assert!(
            sqlx::query("UPDATE chat_agent_app_settings SET agent_access = NULL")
                .execute(&pool)
                .await
                .is_err(),
            "the app scope always names a level"
        );
    });
}

/// The statement that sets `agent_access` to the bound value at `scope` in the fixture graph.
fn access_update(scope: &str) -> &'static str {
    match scope {
        "app" => "UPDATE chat_agent_app_settings SET agent_access = ? WHERE id = 'vault'",
        "project" => {
            "UPDATE chat_project_agent_settings SET agent_access = ?
             WHERE project_id = 'project-chat'"
        }
        "channel" => "UPDATE chat_channels SET agent_access = ? WHERE id = 'channel-general'",
        _ => unreachable!("unknown scope {scope}"),
    }
}

/// The statement that sets `agent_instructions` to the bound value at `scope`.
fn instructions_update(scope: &str) -> &'static str {
    match scope {
        "app" => "UPDATE chat_agent_app_settings SET agent_instructions = ? WHERE id = 'vault'",
        "project" => {
            "UPDATE chat_project_agent_settings SET agent_instructions = ?
             WHERE project_id = 'project-chat'"
        }
        "channel" => "UPDATE chat_channels SET agent_instructions = ? WHERE id = 'channel-general'",
        _ => unreachable!("unknown scope {scope}"),
    }
}

#[test]
fn agent_access_names_one_of_four_levels_and_only_inner_scopes_inherit() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        for scope in ["app", "project", "channel"] {
            for level in ["talk", "read", "ask", "own"] {
                let changed = sqlx::query(AssertSqlSafe(access_update(scope)))
                    .bind(level)
                    .execute(&pool)
                    .await
                    .unwrap_or_else(|error| panic!("{scope} {level}: {error}"));
                assert_eq!(changed.rows_affected(), 1, "{scope} {level}");
            }
            for level in ["", "write", "Own", "none"] {
                assert!(
                    sqlx::query(AssertSqlSafe(access_update(scope)))
                        .bind(level)
                        .execute(&pool)
                        .await
                        .is_err(),
                    "{scope} refuses {level:?}"
                );
            }
            let inherit = sqlx::query(AssertSqlSafe(access_update(scope)))
                .bind(None::<String>)
                .execute(&pool)
                .await;
            assert_eq!(
                inherit.is_ok(),
                scope != "app",
                "{scope}: null inherits from the enclosing scope, and the app scope has none"
            );
        }
    });
}

#[test]
fn agent_instructions_hold_at_most_16384_characters_at_every_scope() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        for scope in ["app", "project", "channel"] {
            for (length, accepted) in [(0, true), (16_384, true), (16_385, false)] {
                let result = sqlx::query(AssertSqlSafe(instructions_update(scope)))
                    .bind("i".repeat(length))
                    .execute(&pool)
                    .await;
                assert_eq!(result.is_ok(), accepted, "{scope} with {length} characters");
            }
            assert!(
                sqlx::query(AssertSqlSafe(instructions_update(scope)))
                    .bind(None::<String>)
                    .execute(&pool)
                    .await
                    .is_err(),
                "{scope} keeps empty text, not null, for no instructions"
            );
        }
    });
}

#[test]
fn project_agent_settings_are_deleted_with_their_project() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        load_project_chat_graph(&pool).await;
        let count = |pool: sqlx::SqlitePool| async move {
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM chat_project_agent_settings WHERE project_id = 'project-chat'",
            )
            .fetch_one(&pool)
            .await
            .unwrap()
        };
        assert_eq!(count(pool.clone()).await, 1);
        assert!(
            sqlx::query(
                "INSERT INTO chat_project_agent_settings (project_id, created_at, updated_at)
                 VALUES ('project-missing', '2026-10-23T00:00:00.000Z',
                         '2026-10-23T00:00:00.000Z')",
            )
            .execute(&pool)
            .await
            .is_err(),
            "project settings need their project"
        );

        sqlx::query("DELETE FROM projects WHERE id = 'project-chat'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(count(pool.clone()).await, 0);
        let app: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_agent_app_settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(app, 1, "deleting a project keeps the app settings");
    });
}

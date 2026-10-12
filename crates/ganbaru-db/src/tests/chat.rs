use super::helpers::migrated_memory_pool;
use sqlx::Row;

const NOW: &str = "2026-07-20T12:00:00Z";

async fn insert_project(pool: &sqlx::SqlitePool) {
    sqlx::query("INSERT INTO project_groups (id, name) VALUES ('group-1', 'Engineering')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO projects (id, group_id, name) VALUES ('project-1', 'group-1', 'Ganbaru')",
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
            "SELECT COUNT(*) FROM project_working_folders
             WHERE kind = 'managed' AND managed_relative_path != ('projects/' || project_id)",
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
            "chat_participants",
            "chat_teammates",
            "chat_teammate_policy_revisions",
            "chat_conversations",
            "chat_channels",
            "chat_conversation_memberships",
            "chat_teammate_working_folder_grants",
            "chat_access_profiles",
            "chat_access_profile_revisions",
            "chat_teammate_access_state",
            "chat_teammate_channel_memberships",
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
            "chat_communication_message_revision_ordinals",
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
            "chat_assignment_dispatch_jobs",
            "chat_scheduled_messages",
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
            "idx_chat_channels_project_name",
            "idx_chat_channels_project_default",
            "idx_chat_channels_active_project",
            "idx_chat_participants_local_user",
            "idx_chat_participants_teammate_display_name",
            "idx_chat_teammate_working_folder_grants_default",
            "idx_chat_access_profiles_builtin",
            "idx_chat_teammate_channel_memberships_teammate",
            "idx_chat_assignment_authorization_revisions_active",
            "idx_chat_host_tool_invocations_authorization",
            "idx_chat_scratch_generations_active",
            "idx_chat_scratch_promotions_generation",
            "idx_chat_scratch_promotions_destination",
            "idx_chat_scratch_cleanup_jobs_ready",
            "idx_chat_access_revocation_jobs_ready",
            "idx_chat_conversation_items_root_ordinal",
            "idx_chat_conversation_items_reply_ordinal",
            "idx_chat_work_assignments_one_active",
            "idx_chat_assignment_dispatch_jobs_ready",
            "idx_chat_scheduled_messages_due",
            "idx_chat_scheduled_messages_destination",
            "idx_chat_scheduled_message_attachment_references_attachment",
            "idx_chat_scheduled_message_references_target",
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
            let columns = sqlx::query(&format!("SELECT name FROM pragma_table_info('{table}')"))
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
fn fresh_projects_create_general_owner_membership_and_primary_folder() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
            .fetch_one(&pool)
            .await
            .unwrap();
        let general_channels: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_channels WHERE is_default = 1 AND name = 'general'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let owner_memberships: i64 = sqlx::query_scalar(
            "SELECT count(*)
             FROM chat_channels channel
             JOIN chat_conversation_memberships membership
               ON membership.conversation_id = channel.conversation_id
             WHERE channel.is_default = 1
               AND membership.participant_id = 'participant:local-owner'
               AND membership.membership_role = 'owner'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let primary_folders: i64 =
            sqlx::query_scalar("SELECT count(*) FROM project_primary_working_folders")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(general_channels, projects);
        assert_eq!(owner_memberships, projects);
        assert_eq!(primary_folders, projects);
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
        let ai_teammates: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_participants WHERE participant_kind = 'ai_teammate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let profiles: i64 = sqlx::query_scalar("SELECT count(*) FROM chat_access_profiles")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(participants, 1);
        assert_eq!(local_owners, 1);
        assert_eq!(ai_teammates, 0);
        assert_eq!(profiles, 5);
    });
}

#[test]
fn standalone_teammate_identity_is_inert_until_access_is_replaced() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('teammate:inert', 'ai_teammate', 'Inert teammate', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_teammates
                (participant_id, role, instructions, created_at, updated_at)
             VALUES ('teammate:inert', 'General support', '', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        let access_revision: i64 = sqlx::query_scalar(
            "SELECT access_revision FROM chat_teammate_access_state
             WHERE teammate_id = 'teammate:inert'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let memberships: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_conversation_memberships
             WHERE participant_id = 'teammate:inert'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let ai_access: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_teammate_channel_memberships
             WHERE teammate_id = 'teammate:inert'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let folder_grants: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chat_teammate_working_folder_grants
             WHERE teammate_id = 'teammate:inert'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(access_revision, 0);
        assert_eq!(memberships, 0);
        assert_eq!(ai_access, 0);
        assert_eq!(folder_grants, 0);
    });
}

#[test]
fn access_profile_revisions_are_immutable_and_do_not_create_memberships() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let update = sqlx::query(
            "UPDATE chat_access_profile_revisions
             SET maximum_folder_capability = 'publish'
             WHERE id = 'access-profile-revision:conversation-only:1'",
        )
        .execute(&pool)
        .await;
        let delete = sqlx::query(
            "DELETE FROM chat_access_profile_revisions
             WHERE id = 'access-profile-revision:conversation-only:1'",
        )
        .execute(&pool)
        .await;
        let extend_builtin = sqlx::query(
            "INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, revision, default_read_history,
                 default_participate, default_history_boundary,
                 maximum_folder_capability, created_at)
             VALUES ('forbidden-built-in-revision', 'access-profile:conversation-only',
                     2, 1, 1, 'entire', 'read', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await;
        let rewrite_builtin = sqlx::query(
            "UPDATE chat_access_profiles
             SET latest_revision = 2, revision = 2
             WHERE id = 'access-profile:conversation-only'",
        )
        .execute(&pool)
        .await;
        let memberships: i64 =
            sqlx::query_scalar("SELECT count(*) FROM chat_teammate_channel_memberships")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(update.is_err());
        assert!(delete.is_err());
        assert!(extend_builtin.is_err());
        assert!(rewrite_builtin.is_err());
        assert_eq!(memberships, 0);
    });
}

#[test]
fn live_profile_expansion_preserves_active_access_revision_and_membership_intent() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
        let conversation_id: String = sqlx::query_scalar(
            "SELECT conversation_id FROM chat_channels WHERE is_default = 1 LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_participants
                (id, participant_kind, display_name, created_at, updated_at)
             VALUES ('teammate:profile-test', 'ai_teammate', 'Profile test', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_teammates
                (participant_id, role, instructions, created_at, updated_at)
             VALUES ('teammate:profile-test', 'Tester', '', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_access_profiles
                (id, display_name, created_at, updated_at)
             VALUES ('access-profile:test', 'Test profile', ?, ?)",
        )
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, revision, default_read_history,
                 default_participate, default_history_boundary,
                 maximum_folder_capability, created_at)
             VALUES ('access-profile-revision:test:1', 'access-profile:test',
                     1, 1, 1, 'entire', 'read', ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_conversation_memberships
                (conversation_id, participant_id, membership_role, created_at, updated_at)
             VALUES (?, 'teammate:profile-test', 'member', ?, ?)",
        )
        .bind(&conversation_id)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_teammate_channel_memberships
                (conversation_id, teammate_id, access_profile_id,
                 read_history, read_history_inherits_profile,
                 participate, participate_inherits_profile,
                 history_boundary, history_boundary_inherits_profile,
                 created_at, updated_at)
             VALUES (?, 'teammate:profile-test', 'access-profile:test',
                     1, 1, 1, 1, 'entire', 1, ?, ?)",
        )
        .bind(&conversation_id)
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let (project_id, working_folder_id): (String, String) = sqlx::query_as(
            "SELECT channel.project_id, primary_folder.working_folder_id
             FROM chat_channels channel
             JOIN project_primary_working_folders primary_folder
               ON primary_folder.project_id = channel.project_id
             WHERE channel.conversation_id = ?",
        )
        .bind(&conversation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_teammate_working_folder_grants
                (conversation_id, teammate_id, project_id, working_folder_id,
                 capability, capability_inherits_profile, created_at)
             VALUES (?, 'teammate:profile-test', ?, ?, 'read', 1, ?)",
        )
        .bind(&conversation_id)
        .bind(project_id)
        .bind(working_folder_id)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        let audience_revision_before: i64 = sqlx::query_scalar(
            "SELECT revision FROM chat_conversation_audience_state
             WHERE conversation_id = ?",
        )
        .bind(&conversation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_access_profile_revisions
                (id, access_profile_id, revision, default_read_history,
                 default_participate, default_history_boundary,
                 maximum_folder_capability, created_at)
             VALUES ('access-profile-revision:test:2', 'access-profile:test',
                     2, 1, 1, 'entire', 'edit', ?)",
        )
        .bind("2026-07-20T12:01:00Z")
        .execute(&pool)
        .await
        .unwrap();

        let access_revision: i64 = sqlx::query_scalar(
            "SELECT access_revision FROM chat_teammate_access_state
             WHERE teammate_id = 'teammate:profile-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let latest_profile_revision: i64 = sqlx::query_scalar(
            "SELECT profile.latest_revision
             FROM chat_teammate_channel_memberships membership
             JOIN chat_access_profiles profile ON profile.id = membership.access_profile_id
             WHERE membership.teammate_id = 'teammate:profile-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let audience_revision_after: i64 = sqlx::query_scalar(
            "SELECT revision FROM chat_conversation_audience_state
             WHERE conversation_id = ?",
        )
        .bind(&conversation_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let pinned_column: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pragma_table_info('chat_teammate_channel_memberships')
             WHERE name = 'access_profile_revision'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let effective_folder_capability: String = sqlx::query_scalar(
            "SELECT CASE WHEN grant_row.capability_inherits_profile = 1
                    THEN revision.maximum_folder_capability
                    ELSE grant_row.capability END
             FROM chat_teammate_working_folder_grants grant_row
             JOIN chat_teammate_channel_memberships membership
               ON membership.conversation_id = grant_row.conversation_id
              AND membership.teammate_id = grant_row.teammate_id
             JOIN chat_access_profiles profile ON profile.id = membership.access_profile_id
             JOIN chat_access_profile_revisions revision
               ON revision.access_profile_id = profile.id
              AND revision.revision = profile.latest_revision
             WHERE grant_row.teammate_id = 'teammate:profile-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(access_revision, 0);
        assert_eq!(latest_profile_revision, 2);
        assert!(audience_revision_after > audience_revision_before);
        assert_eq!(pinned_column, 0);
        assert_eq!(effective_folder_capability, "edit");
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
                 conversation_id, label_snapshot, lower_ordinal, high_ordinal,
                 source_revision_cutoff_id, destination_audience_revision, created_at)
             VALUES ('authorization:test', ?, 'reference:test', 'conversation:test',
                     'Source', 1, 1, 'revision:test', 1, ?)",
        )
        .bind("s".repeat(32))
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
fn scheduled_message_schema_retains_attachments_until_the_schedule_is_removed() {
    super::block_on(async {
        let pool = migrated_memory_pool().await;
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
                 available_at, created_at, updated_at)
             VALUES ('scheduled-message-1', 'scheduled-command-1', ?, '{}', ?, ?, ?, ?)",
        )
        .bind(&channel_id)
        .bind("2026-08-03T15:00:00Z")
        .bind("2026-08-03T15:00:00Z")
        .bind(NOW)
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chat_scheduled_message_attachment_references
                (scheduled_message_id, attachment_id, ordinal, created_at)
             VALUES ('scheduled-message-1', 'scheduled-attachment', 0, ?)",
        )
        .bind(NOW)
        .execute(&pool)
        .await
        .unwrap();

        assert!(
            sqlx::query("DELETE FROM chat_attachments WHERE id = 'scheduled-attachment'")
                .execute(&pool)
                .await
                .is_err()
        );
        sqlx::query("DELETE FROM chat_scheduled_messages WHERE id = 'scheduled-message-1'")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM chat_attachments WHERE id = 'scheduled-attachment'")
            .execute(&pool)
            .await
            .unwrap();
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
            let columns = sqlx::query(&format!("SELECT name FROM pragma_table_info('{table}')"))
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

        let project_delete = sqlx::query("DELETE FROM projects WHERE id = 'project-1'")
            .execute(&pool)
            .await;
        assert!(project_delete.is_err());
        let folder_delete =
            sqlx::query("DELETE FROM project_working_folders WHERE id = 'working-folder-project'")
                .execute(&pool)
                .await;
        assert!(folder_delete.is_err());
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
                     'refs/ganbaru-ai/chat/thread-1/checkpoint-1',
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
        assert_eq!(target, "refs/ganbaru-ai/chat/thread-1/checkpoint-1");
    });
}

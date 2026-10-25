INSERT INTO project_groups (id, name, icon, order_key)
VALUES ('group-chat', 'Chat work', 'lucide:folder', 'a0');
INSERT INTO projects (id, group_id, name, icon, order_key)
VALUES ('project-chat', 'group-chat', 'Chat project', 'lucide:folder', 'a0');
INSERT INTO project_working_folders
    (id, project_id, display_name, kind, managed_relative_path, created_at, updated_at)
VALUES ('folder-managed', 'project-chat', 'Managed', 'managed', 'projects/project-chat',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO project_working_folders
    (id, project_id, display_name, kind, created_at, updated_at, archived_at)
VALUES ('folder-archived', 'project-chat', 'Archived', 'external',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z', '2026-10-23T01:00:00.000Z');
INSERT INTO chat_execution_environments
    (id, working_folder_id, kind, display_name, created_at, updated_at)
VALUES ('current-folder:folder-managed', 'folder-managed', 'current_folder', 'Managed',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z'),
       ('current-folder:folder-archived', 'folder-archived', 'current_folder', 'Archived',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');

INSERT INTO project_sections (id, project_id, name, order_key)
VALUES ('section-now', 'project-chat', 'Now', 'a0'),
       ('section-later', 'project-chat', 'Later', 'a1');
INSERT INTO project_statuses (id, project_id, name, category, order_key, deleted_at)
VALUES ('status-todo', 'project-chat', 'To do', 'not_started', 'a0', NULL),
       ('status-retired', 'project-chat', 'Retired', 'not_started', 'a1', '2026-10-23T02:00:00.000Z');
INSERT INTO project_priorities (id, project_id, name, color, order_key, deleted_at, sync_key)
VALUES ('priority-urgent', 'project-chat', 'Urgent', 4, 'a0', NULL, 'priority-key-urgent'),
       ('priority-retired', 'project-chat', 'Retired', 5, 'a1', '2026-10-23T02:00:00.000Z',
        'priority-key-retired');
INSERT INTO project_tasks
    (id, project_id, section_id, status_id, title, priority, section_order_key, status_order_key)
VALUES ('task-now', 'project-chat', 'section-now', 'status-todo', 'Plan', 'priority-urgent', 'a0', 'a0'),
       ('task-retired', 'project-chat', 'section-later', 'status-retired', 'Old', 'priority-retired',
        'a1', 'a0');
INSERT INTO project_tasks
    (id, project_id, section_id, status_id, parent_task_id, title, section_order_key, status_order_key)
VALUES ('task-child', 'project-chat', 'section-later', 'status-todo', 'task-now', 'Child', 'a2', 'a1');

INSERT INTO chat_participants (id, participant_kind, display_name, created_at, updated_at)
VALUES ('participant-teammate', 'ai_teammate', 'Helper',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_teammates (participant_id, role, created_at, updated_at)
VALUES ('participant-teammate', 'Builds things', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_teammate_policy_revisions
    (id, teammate_id, provider_instance_id, model_selection_data, created_at)
VALUES ('policy-1', 'participant-teammate', 'codex-personal', '{}', '2026-10-23T00:00:00.000Z');

INSERT INTO chat_conversations (id, project_id, conversation_kind, last_activity_at, created_at, updated_at)
VALUES ('conversation-general', 'project-chat', 'channel', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_channels (id, project_id, conversation_id, name, is_default, created_at, updated_at)
VALUES ('channel-general', 'project-chat', 'conversation-general', 'general', 1,
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_conversation_memberships
    (conversation_id, participant_id, sync_key, membership_role, created_at, updated_at)
VALUES ('conversation-general', 'participant:local-owner', 'membership-owner', 'owner',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z'),
       ('conversation-general', 'participant-teammate', 'membership-teammate', 'member',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_teammate_channel_memberships
    (conversation_id, teammate_id, sync_key, access_profile_id, created_at, updated_at)
VALUES ('conversation-general', 'participant-teammate', 'channel-membership-teammate',
        'access-profile:edit-files', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_teammate_working_folder_grants
    (conversation_id, teammate_id, project_id, working_folder_id, sync_key, capability, created_at)
VALUES ('conversation-general', 'participant-teammate', 'project-chat', 'folder-managed',
        'grant-managed', 'edit', '2026-10-23T00:00:00.000Z'),
       ('conversation-general', 'participant-teammate', 'project-chat', 'folder-archived',
        'grant-archived', 'read', '2026-10-23T00:00:00.000Z');

INSERT INTO chat_conversation_items (id, conversation_id, item_kind, ordinal, created_at)
VALUES ('item-root', 'conversation-general', 'message', 1, '2026-10-23T00:00:00.000Z');
INSERT INTO chat_communication_messages (item_id, author_participant_id, current_revision_id, created_at)
VALUES ('item-root', 'participant:local-owner', 'message-revision-root', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_communication_message_revisions (id, message_item_id, revision, normalized_markdown, created_at)
VALUES ('message-revision-root', 'item-root', 1, 'Look at the folder', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_message_references
    (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
     start_offset, end_offset, ordinal, created_at)
VALUES ('reference-folder', 'message-revision-root', 'working_folder', 'Managed', 'Managed', 0, 4, 0,
        '2026-10-23T00:00:00.000Z'),
       ('reference-path', 'message-revision-root', 'workspace_path', 'src', 'src', 5, 8, 1,
        '2026-10-23T00:00:00.000Z'),
       ('reference-archived', 'message-revision-root', 'working_folder', 'Archived', 'Archived', 9, 12, 2,
        '2026-10-23T00:00:00.000Z');
INSERT INTO chat_working_folder_reference_targets (reference_id, working_folder_id)
VALUES ('reference-folder', 'folder-managed'),
       ('reference-archived', 'folder-archived');
INSERT INTO chat_workspace_path_reference_targets (reference_id, working_folder_id, path_kind, relative_path)
VALUES ('reference-path', 'folder-managed', 'folder', 'src');
INSERT INTO chat_reply_threads (id, conversation_id, root_item_id, last_activity_at, created_at, updated_at)
VALUES ('reply-thread', 'conversation-general', 'item-root', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');

INSERT INTO chat_work_assignments
    (id, reply_thread_id, teammate_id, triggering_message_item_id, state, created_at, updated_at)
VALUES ('assignment-1', 'reply-thread', 'participant-teammate', 'item-root', 'working',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_assignment_authorization_revisions
    (id, assignment_id, revision, requester_participant_id, teammate_policy_revision_id,
     teammate_access_revision, destination_conversation_id, access_profile_revision_id,
     execution_environment_id, working_folder_id, resolved_runtime_approval_policy, scope_digest,
     decision_state, created_at)
VALUES ('authorization-1', 'assignment-1', 1, 'participant:local-owner', 'policy-1', 1,
        'conversation-general', 'access-profile-revision:edit-files:1',
        'current-folder:folder-managed', 'folder-managed', 'ask',
        '1111111111111111111111111111111111111111111111111111111111111111', 'allowed',
        '2026-10-23T00:00:00.000Z');
INSERT INTO chat_assignment_authorized_folder_sources
    (authorization_revision_id, root_handle, working_folder_id, capability, is_execution_target,
     created_at, resolved_runtime_approval_policy)
VALUES ('authorization-1', 'root-handle-managed-0000000000000000', 'folder-managed', 'edit', 1,
        '2026-10-23T00:00:00.000Z', 'ask'),
       ('authorization-1', 'root-handle-archived-000000000000000', 'folder-archived', 'read', 0,
        '2026-10-23T00:00:00.000Z', 'ask');
INSERT INTO chat_assignment_authorized_channel_sources
    (authorization_revision_id, source_handle, message_reference_id, conversation_id, label_snapshot,
     lower_ordinal, high_ordinal, source_revision_cutoff_id, destination_audience_revision, created_at)
VALUES ('authorization-1', 'source-handle-general-0000000000000', 'reference-folder',
        'conversation-general', 'general', 1, 1, 'message-revision-root', 1, '2026-10-23T00:00:00.000Z');

INSERT INTO chat_threads
    (id, working_folder_id, project_id, title, provider_family_id, provider_instance_id,
     continuation_group_id, safety_mode, interaction_mode, state, last_activity_at, created_at, updated_at)
VALUES ('thread-managed', 'folder-managed', 'project-chat', 'Implement', 'codex', 'codex-personal',
        'continuation-1', 'ask_for_approval', 'build', 'idle', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z'),
       ('thread-archived', 'folder-archived', 'project-chat', 'Old work', 'codex', 'codex-personal',
        'continuation-2', 'ask_for_approval', 'build', 'archived', '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_turns (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
VALUES ('turn-1', 'thread-managed', 0, 'active', 'ask_for_approval', 'build',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_agent_runs
    (id, assignment_id, project_id, working_folder_id, execution_environment_id,
     teammate_policy_revision_id, authorization_revision_id, authorization_scope_digest,
     provider_turn_id, provider_thread_id, state, run_ordinal, created_at, updated_at)
VALUES ('run-1', 'assignment-1', 'project-chat', 'folder-managed', 'current-folder:folder-managed',
        'policy-1', 'authorization-1',
        '1111111111111111111111111111111111111111111111111111111111111111', 'turn-1',
        'thread-managed', 'working', 1, '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');

INSERT INTO chat_attachments
    (id, working_folder_id, kind, original_display_name, mime_type, byte_size, sha256,
     managed_relative_path, signature_kind, created_at)
VALUES ('attachment-managed', 'folder-managed', 'image', 'screen.png', 'image/png', 4,
        'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
        'assets/chat/attachments/aa/attachment-managed.png', 'png', '2026-10-23T00:00:00.000Z'),
       ('attachment-archived', 'folder-archived', 'text_snippet', 'notes.txt', 'text/plain', 3,
        'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
        'assets/chat/attachments/bb/attachment-archived.txt', 'text', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_resources
    (id, working_folder_id, resource_kind, display_name, mime_type, byte_size, sha256,
     managed_relative_path, resource_uri, created_at)
VALUES ('resource-screenshot', 'folder-managed', 'browser_screenshot', 'page.png', 'image/png', 5,
        'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
        'assets/chat/browser-artifacts/cc/resource-screenshot.png',
        'ganbaru://chat/resource/resource-screenshot', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_cleanup_jobs
    (id, working_folder_id, source_thread_id, cleanup_kind, exact_target, available_at, created_at, updated_at)
VALUES ('cleanup-1', 'folder-archived', 'thread-archived', 'diagnostic_event', 'event-1',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');

INSERT INTO chat_scheduled_messages
    (id, client_command_id, channel_id, request_data, scheduled_for, available_at, created_at, updated_at)
VALUES ('scheduled-1', 'command-1', 'channel-general', '{}', '2026-10-24T00:00:00.000Z',
        '2026-10-24T00:00:00.000Z', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_scheduled_message_references
    (id, scheduled_message_id, reference_kind, label_snapshot, plain_text_projection, start_offset,
     end_offset, working_folder_id, path_kind, relative_path, ordinal)
VALUES ('scheduled-reference-folder', 'scheduled-1', 'working_folder', 'Archived', 'Archived', 0, 4,
        'folder-archived', NULL, NULL, 0),
       ('scheduled-reference-path', 'scheduled-1', 'workspace_path', 'src', 'src', 5, 8,
        'folder-managed', 'file', 'src/main.rs', 1);

INSERT INTO chat_scratch_scopes (id, reply_thread_id, teammate_id, created_at, updated_at)
VALUES ('scratch-scope', 'reply-thread', 'participant-teammate',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_scratch_generations (id, scratch_scope_id, generation, created_at, updated_at)
VALUES ('scratch-generation', 'scratch-scope', 1, '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_execution_environments
    (id, scratch_generation_id, kind, display_name, created_at, updated_at)
VALUES ('scratch-environment', 'scratch-generation', 'scratch', 'Scratch',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_message_references
    (id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
     start_offset, end_offset, ordinal, created_at)
VALUES ('reference-environment', 'message-revision-root', 'execution_environment', 'Scratch', 'Scratch',
        13, 16, 3, '2026-10-23T00:00:00.000Z');
INSERT INTO chat_execution_environment_reference_targets (reference_id, execution_environment_id)
VALUES ('reference-environment', 'scratch-environment');
INSERT INTO chat_scheduled_message_references
    (id, scheduled_message_id, reference_kind, label_snapshot, plain_text_projection, start_offset,
     end_offset, execution_environment_id, ordinal)
VALUES ('scheduled-reference-environment', 'scheduled-1', 'execution_environment', 'Scratch', 'Scratch',
        9, 12, 'scratch-environment', 2);
INSERT INTO chat_assignment_authorization_revisions
    (id, assignment_id, revision, requester_participant_id, teammate_policy_revision_id,
     teammate_access_revision, destination_conversation_id, access_profile_revision_id,
     execution_environment_id, working_folder_id, resolved_runtime_approval_policy, scope_digest,
     decision_state, created_at)
VALUES ('authorization-scratch', 'assignment-1', 2, 'participant:local-owner', 'policy-1', 1,
        'conversation-general', 'access-profile-revision:edit-files:1', 'scratch-environment', NULL,
        'ask', '2222222222222222222222222222222222222222222222222222222222222222', 'allowed',
        '2026-10-23T00:00:00.000Z');
INSERT INTO chat_threads
    (id, project_id, execution_environment_id, scratch_generation_id, title, provider_family_id,
     provider_instance_id, continuation_group_id, safety_mode, interaction_mode, state,
     last_activity_at, created_at, updated_at)
VALUES ('thread-scratch', 'project-chat', 'scratch-environment', 'scratch-generation', 'Scratch work',
        'codex', 'codex-personal', 'continuation-3', 'ask_for_approval', 'build', 'idle',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_turns (id, thread_id, ordinal, state, safety_mode, interaction_mode, created_at, updated_at)
VALUES ('turn-scratch', 'thread-scratch', 0, 'active', 'ask_for_approval', 'build',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_agent_runs
    (id, assignment_id, project_id, execution_environment_id, scratch_generation_id,
     teammate_policy_revision_id, authorization_revision_id, authorization_scope_digest,
     provider_turn_id, provider_thread_id, state, run_ordinal, created_at, updated_at)
VALUES ('run-scratch', 'assignment-1', 'project-chat', 'scratch-environment', 'scratch-generation',
        'policy-1', 'authorization-scratch',
        '2222222222222222222222222222222222222222222222222222222222222222', 'turn-scratch',
        'thread-scratch', 'working', 2, '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_scratch_generation_sources
    (scratch_generation_id, conversation_id, lower_ordinal, high_ordinal, audience_revision, created_at)
VALUES ('scratch-generation', 'conversation-general', 1, 1, 1, '2026-10-23T00:00:00.000Z');
INSERT INTO chat_scratch_promotions
    (id, scratch_generation_id, source_relative_path, source_content_revision, request_digest,
     destination_kind, destination_channel_id, destination_working_folder_id, destination_relative_path,
     attachment_id, created_at, updated_at)
VALUES ('promotion-folder', 'scratch-generation', 'out/report.md',
        'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
        'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
        'working_folder', 'channel-general', 'folder-managed', 'docs/report.md', NULL,
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z'),
       ('promotion-attachment', 'scratch-generation', 'out/chart.png',
        'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
        'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
        'managed_attachment', 'channel-general', NULL, NULL, 'attachment-from-scratch',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');
INSERT INTO chat_scratch_cleanup_jobs
    (id, scratch_scope_id, scratch_generation_id, expected_scope_revision, confirmed_at, available_at,
     created_at, updated_at)
VALUES ('scratch-cleanup', 'scratch-scope', 'scratch-generation', 1, '2026-10-23T00:00:00.000Z',
        '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z', '2026-10-23T00:00:00.000Z');

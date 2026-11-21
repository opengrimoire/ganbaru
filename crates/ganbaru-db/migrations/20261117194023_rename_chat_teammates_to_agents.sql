-- no-transaction
-- Chat names its AI participants agents everywhere: tables, columns, indexes, views, triggers,
-- the `ai_agent` participant kind, and the `publish_agent` command kind. Sync tables keep their
-- numeric ids, so replicated rows stay addressed the same way.
--
-- Rebuilding `chat_participants` drops its rows while other tables cascade from them, so foreign
-- keys are off for this connection and the whole change commits as one transaction.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

-- Objects whose names or bodies use the old names, and every trigger of the rebuilt table.
DROP INDEX idx_chat_teammate_channel_memberships_teammate;
DROP INDEX idx_chat_teammate_policy_revisions_teammate;
DROP INDEX idx_chat_teammate_usage_daily_teammate_date;
DROP INDEX idx_chat_teammate_working_folder_grants_folder;
DROP INDEX idx_chat_work_assignments_teammate;
DROP TRIGGER chat_access_profile_audience_publish;
DROP TRIGGER chat_access_profile_push_approvals_update;
DROP TRIGGER chat_agent_run_authority_immutable;
DROP TRIGGER chat_assignment_authorization_delete_denied;
DROP TRIGGER chat_assignment_authorization_scope_immutable;
DROP TRIGGER chat_participant_push_approvals_archive;
DROP TRIGGER chat_participants_local_user_identity;
DROP TRIGGER chat_scratch_scope_identity_immutable;
DROP TRIGGER chat_teammate_access_revision_monotonic;
DROP TRIGGER chat_teammate_channel_membership_audience_delete;
DROP TRIGGER chat_teammate_channel_membership_audience_insert;
DROP TRIGGER chat_teammate_channel_membership_audience_update;
DROP TRIGGER chat_teammate_channel_membership_identity_insert;
DROP TRIGGER chat_teammate_channel_membership_push_approvals_delete;
DROP TRIGGER chat_teammate_channel_membership_push_approvals_update;
DROP TRIGGER chat_teammate_channel_membership_requires_membership_insert;
DROP TRIGGER chat_teammate_create_access_state;
DROP TRIGGER chat_teammate_folder_grant_identity_insert;
DROP TRIGGER chat_teammate_folder_grant_project_insert;
DROP TRIGGER chat_teammate_folder_grant_project_update;
DROP TRIGGER chat_teammate_folder_grant_push_approvals_delete;
DROP TRIGGER chat_teammate_folder_grant_push_approvals_update;
DROP TRIGGER chat_teammate_folder_grant_single_default_insert;
DROP TRIGGER chat_teammate_folder_grant_single_default_update;
DROP TRIGGER chat_teammate_policy_identity_insert;
DROP TRIGGER chat_teammate_policy_immutable_delete;
DROP TRIGGER chat_teammate_policy_immutable_update;
DROP TRIGGER chat_teammate_push_approvals_safety_mode;
DROP TRIGGER sync_capture_chat_participants_delete;
DROP TRIGGER sync_capture_chat_participants_insert;
DROP TRIGGER sync_capture_chat_participants_update;
DROP TRIGGER sync_capture_chat_teammate_channel_memberships_delete;
DROP TRIGGER sync_capture_chat_teammate_channel_memberships_insert;
DROP TRIGGER sync_capture_chat_teammate_channel_memberships_update;
DROP TRIGGER sync_capture_chat_teammate_policy_revisions_delete;
DROP TRIGGER sync_capture_chat_teammate_policy_revisions_insert;
DROP TRIGGER sync_capture_chat_teammate_policy_revisions_update;
DROP TRIGGER sync_capture_chat_teammate_usage_daily_delete;
DROP TRIGGER sync_capture_chat_teammate_usage_daily_insert;
DROP TRIGGER sync_capture_chat_teammate_usage_daily_update;
DROP TRIGGER sync_capture_chat_teammate_working_folder_grants_delete;
DROP TRIGGER sync_capture_chat_teammate_working_folder_grants_insert;
DROP TRIGGER sync_capture_chat_teammate_working_folder_grants_update;
DROP TRIGGER sync_capture_chat_teammates_delete;
DROP TRIGGER sync_capture_chat_teammates_insert;
DROP TRIGGER sync_capture_chat_teammates_update;
DROP TRIGGER sync_immutable_chat_participants;
DROP TRIGGER sync_immutable_chat_push_approvals;
DROP TRIGGER sync_immutable_chat_teammate_channel_memberships;
DROP TRIGGER sync_immutable_chat_teammate_policy_revisions;
DROP TRIGGER sync_immutable_chat_teammate_usage_daily;
DROP TRIGGER sync_immutable_chat_teammate_working_folder_grants;
DROP TRIGGER sync_immutable_chat_teammates;
DROP TRIGGER sync_immutable_chat_work_assignments;
DROP TRIGGER sync_recreate_chat_participants;
DROP TRIGGER sync_recreate_chat_teammate_policy_revisions;
DROP TRIGGER sync_recreate_chat_teammate_usage_daily;
DROP TRIGGER sync_recreate_chat_teammates;
DROP TRIGGER sync_values_chat_participants_insert;
DROP TRIGGER sync_values_chat_participants_update;
DROP TRIGGER sync_values_chat_push_approvals_insert;
DROP TRIGGER sync_values_chat_push_approvals_update;
DROP TRIGGER sync_values_chat_teammate_channel_memberships_insert;
DROP TRIGGER sync_values_chat_teammate_channel_memberships_update;
DROP TRIGGER sync_values_chat_teammate_policy_revisions_insert;
DROP TRIGGER sync_values_chat_teammate_policy_revisions_update;
DROP TRIGGER sync_values_chat_teammate_usage_daily_insert;
DROP TRIGGER sync_values_chat_teammate_usage_daily_update;
DROP TRIGGER sync_values_chat_teammate_working_folder_grants_insert;
DROP TRIGGER sync_values_chat_teammate_working_folder_grants_update;
DROP TRIGGER sync_values_chat_teammates_insert;
DROP TRIGGER sync_values_chat_teammates_update;
DROP TRIGGER sync_values_chat_work_assignments_insert;
DROP TRIGGER sync_values_chat_work_assignments_update;
DROP VIEW chat_teammate_effective_policies;
DROP VIEW chat_teammate_policy_revisions_numbered;

CREATE TABLE chat_participants_previous AS SELECT * FROM chat_participants;
DROP TABLE chat_participants;
CREATE TABLE chat_participants (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
  participant_kind TEXT NOT NULL CHECK (
      participant_kind IN ('local_user', 'ai_agent', 'human')
  ),
  display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
  avatar_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (avatar_schema_version >= 1),
  avatar_data TEXT NOT NULL DEFAULT '{"kind":"initials"}' CHECK (
      length(avatar_data) <= 16384
      AND json_valid(avatar_data) AND json_type(avatar_data) = 'object'
  ),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO chat_participants (
  id, participant_kind, display_name, avatar_schema_version, avatar_data, revision, archived_at,
  created_at, updated_at
)
SELECT
  id,
  CASE participant_kind WHEN 'ai_teammate' THEN 'ai_agent' ELSE participant_kind END,
  display_name, avatar_schema_version, avatar_data, revision, archived_at, created_at, updated_at
FROM chat_participants_previous;
DROP TABLE chat_participants_previous;

-- Finished receipts only answer retries of their command for a few days, so the ones whose
-- stored result or error still uses the old names are dropped instead of rewritten.
CREATE TABLE chat_organizational_command_receipts_previous AS
SELECT * FROM chat_organizational_command_receipts;
DROP TABLE chat_organizational_command_receipts;
CREATE TABLE chat_organizational_command_receipts (
  client_command_id TEXT PRIMARY KEY NOT NULL CHECK (length(client_command_id) BETWEEN 1 AND 1024),
  command_kind TEXT NOT NULL CHECK (
      command_kind IN (
          'post_message', 'publish_agent', 'update_membership', 'assignment_action',
          'channel_note'
      )
  ),
  state TEXT NOT NULL CHECK (state IN ('accepted', 'completed', 'failed')),
  result_schema_version INTEGER CHECK (result_schema_version IS NULL OR result_schema_version >= 1),
  result_data TEXT CHECK (result_data IS NULL OR json_valid(result_data)),
  error_data TEXT CHECK (error_data IS NULL OR json_valid(error_data)),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO chat_organizational_command_receipts (
  client_command_id, command_kind, state, result_schema_version, result_data, error_data,
  created_at, updated_at
)
SELECT
  client_command_id,
  CASE command_kind WHEN 'publish_teammate' THEN 'publish_agent' ELSE command_kind END,
  state, result_schema_version, result_data, error_data, created_at, updated_at
FROM chat_organizational_command_receipts_previous
WHERE NOT (
  state IN ('completed', 'failed')
  AND (coalesce(result_data, '') LIKE '%teammate%' OR coalesce(error_data, '') LIKE '%teammate%')
);
DROP TABLE chat_organizational_command_receipts_previous;

ALTER TABLE chat_teammates RENAME TO chat_agents;
ALTER TABLE chat_teammate_access_state RENAME TO chat_agent_access_state;
ALTER TABLE chat_teammate_channel_memberships RENAME TO chat_agent_channel_memberships;
ALTER TABLE chat_teammate_policy_revisions RENAME TO chat_agent_policy_revisions;
ALTER TABLE chat_teammate_usage_daily RENAME TO chat_agent_usage_daily;
ALTER TABLE chat_teammate_working_folder_grants RENAME TO chat_agent_working_folder_grants;

ALTER TABLE chat_access_revocation_jobs RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_runs RENAME COLUMN teammate_policy_revision_id TO agent_policy_revision_id;
ALTER TABLE chat_assignment_authorization_revisions
RENAME COLUMN teammate_policy_revision_id TO agent_policy_revision_id;
ALTER TABLE chat_assignment_authorization_revisions
RENAME COLUMN teammate_access_revision TO agent_access_revision;
ALTER TABLE chat_push_approvals RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_scratch_scopes RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_access_state RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_channel_memberships RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_policy_revisions RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_usage_daily RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_agent_working_folder_grants RENAME COLUMN teammate_id TO agent_id;
ALTER TABLE chat_work_assignments RENAME COLUMN teammate_id TO agent_id;

-- The dropped objects again, under the new names.
CREATE INDEX idx_chat_agent_channel_memberships_agent
ON chat_agent_channel_memberships(agent_id, conversation_id);

CREATE INDEX idx_chat_agent_policy_revisions_agent
ON chat_agent_policy_revisions(agent_id, created_at, id);

CREATE INDEX idx_chat_agent_usage_daily_agent_date
ON chat_agent_usage_daily(agent_id, local_date);

CREATE INDEX idx_chat_agent_working_folder_grants_folder
ON chat_agent_working_folder_grants(working_folder_id);

CREATE INDEX idx_chat_work_assignments_agent
ON chat_work_assignments(agent_id, state, updated_at DESC, id);

CREATE TRIGGER chat_access_profile_audience_publish
AFTER INSERT ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.created_at
    WHERE conversation_id IN (
        SELECT membership.conversation_id
        FROM chat_agent_channel_memberships membership
        WHERE membership.access_profile_id = NEW.access_profile_id
          AND membership.removed_at IS NULL
    );
END;

CREATE TRIGGER chat_access_profile_push_approvals_update
AFTER UPDATE OF current_maximum_folder_capability, archived_at ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.current_maximum_folder_capability = 'publish'
    AND (
        NEW.current_maximum_folder_capability IS NOT 'publish'
        OR (NEW.archived_at IS NOT NULL AND OLD.archived_at IS NULL)
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE revoked_at IS NULL
      AND agent_id IN (
          SELECT membership.agent_id FROM chat_agent_channel_memberships membership
          WHERE membership.access_profile_id = NEW.id
      );
END;

CREATE TRIGGER chat_agent_run_authority_immutable
BEFORE UPDATE OF
    assignment_id, project_id, working_folder_id,
    execution_environment_id, scratch_generation_id,
    agent_policy_revision_id, authorization_revision_id,
    authorization_scope_digest, provider_turn_id,
    provider_thread_id, run_ordinal, created_at
ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run authority is immutable');
END;

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
 AND EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.requester_participant_id)
 AND EXISTS (SELECT 1 FROM chat_agent_policy_revisions WHERE id = OLD.agent_policy_revision_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.destination_conversation_id)
 AND EXISTS (SELECT 1 FROM chat_access_profile_revisions WHERE id = OLD.access_profile_revision_id)
 AND (OLD.execution_environment_id IS NULL
      OR EXISTS (SELECT 1 FROM chat_execution_environments WHERE id = OLD.execution_environment_id))
 AND (OLD.working_folder_id IS NULL
      OR EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id))
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization revisions are immutable');
END;

CREATE TRIGGER chat_assignment_authorization_scope_immutable
BEFORE UPDATE OF
    assignment_id, revision, requester_participant_id,
    agent_policy_revision_id, agent_access_revision,
    destination_conversation_id, access_profile_revision_id,
    execution_environment_id, working_folder_id,
    resolved_runtime_approval_policy, scope_digest, created_at
ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization scopes are immutable');
END;

CREATE TRIGGER chat_participant_push_approvals_archive
AFTER UPDATE OF archived_at ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = NEW.id AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_participants_local_user_identity
BEFORE INSERT ON chat_participants
WHEN NEW.participant_kind = 'local_user' AND NEW.id != 'participant:local-owner'
BEGIN
    SELECT RAISE(ABORT, 'The local user keeps its seeded participant id');
END;

CREATE TRIGGER chat_scratch_scope_identity_immutable
BEFORE UPDATE OF id, reply_thread_id, agent_id, created_at ON chat_scratch_scopes
BEGIN
    SELECT RAISE(ABORT, 'Scratch scope identity is immutable');
END;

CREATE TRIGGER chat_agent_access_revision_monotonic
BEFORE UPDATE OF access_revision ON chat_agent_access_state
WHEN NEW.access_revision <= OLD.access_revision
BEGIN
    SELECT RAISE(ABORT, 'Agent access revisions must increase');
END;

CREATE TRIGGER chat_agent_channel_membership_audience_delete
AFTER DELETE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_agent_channel_membership_audience_insert
AFTER INSERT ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_agent_channel_membership_audience_update
AFTER UPDATE OF conversation_id, agent_id, access_profile_id, read_history,
    read_history_inherits_profile, participate, participate_inherits_profile, history_boundary,
    history_boundary_inherits_profile, history_from_order_key, runtime_approval_policy,
    scratch_runtime_approval_policy, created_at, updated_at, removed_at
ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

CREATE TRIGGER chat_agent_channel_membership_identity_insert
BEFORE INSERT ON chat_agent_channel_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM chat_agents agent
    WHERE agent.participant_id = NEW.agent_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires an AI agent');
END;

CREATE TRIGGER chat_agent_channel_membership_push_approvals_delete
AFTER DELETE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = OLD.agent_id AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_channel_membership_push_approvals_update
AFTER UPDATE OF access_profile_id, runtime_approval_policy, removed_at
ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
    AND (
        NEW.removed_at IS NOT NULL
        OR NEW.access_profile_id IS NOT OLD.access_profile_id
        OR NEW.runtime_approval_policy IS NOT OLD.runtime_approval_policy
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = NEW.agent_id AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_channel_membership_requires_membership_insert
BEFORE INSERT ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1 FROM chat_conversation_memberships membership
    WHERE membership.conversation_id = NEW.conversation_id
      AND membership.participant_id = NEW.agent_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires a conversation membership');
END;

CREATE TRIGGER chat_agent_create_access_state
AFTER INSERT ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_agent_access_state (agent_id, updated_at)
    VALUES (NEW.participant_id, NEW.updated_at);
END;

CREATE TRIGGER chat_agent_folder_grant_identity_insert
BEFORE INSERT ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1
    FROM chat_agents agent
    JOIN chat_conversation_memberships membership
      ON membership.conversation_id = NEW.conversation_id
     AND membership.participant_id = NEW.agent_id
     AND membership.removed_at IS NULL
    JOIN chat_agent_channel_memberships ai_membership
      ON ai_membership.conversation_id = membership.conversation_id
     AND ai_membership.agent_id = membership.participant_id
     AND ai_membership.removed_at IS NULL
    WHERE agent.participant_id = NEW.agent_id
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grants require active AI channel access');
END;

CREATE TRIGGER chat_agent_folder_grant_project_insert
BEFORE INSERT ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND (
    NOT EXISTS (
        SELECT 1 FROM chat_conversations conversation
        WHERE conversation.id = NEW.conversation_id
          AND conversation.project_id = NEW.project_id
    )
    OR NOT EXISTS (
        SELECT 1 FROM project_working_folders folder
        WHERE folder.id = NEW.working_folder_id AND folder.project_id = NEW.project_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grant project must match its conversation and folder');
END;

CREATE TRIGGER chat_agent_folder_grant_project_update
BEFORE UPDATE OF project_id ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND (
    NOT EXISTS (
        SELECT 1 FROM chat_conversations conversation
        WHERE conversation.id = NEW.conversation_id
          AND conversation.project_id = NEW.project_id
    )
    OR NOT EXISTS (
        SELECT 1 FROM project_working_folders folder
        WHERE folder.id = NEW.working_folder_id AND folder.project_id = NEW.project_id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grant project must match its conversation and folder');
END;

CREATE TRIGGER chat_agent_folder_grant_push_approvals_delete
AFTER DELETE ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.revoked_at IS NULL
    AND (OLD.capability = 'publish' OR OLD.capability_inherits_profile = 1)
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = OLD.agent_id AND working_folder_id = OLD.working_folder_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_folder_grant_push_approvals_update
AFTER UPDATE OF capability, capability_inherits_profile, runtime_approval_policy, revoked_at
ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.revoked_at IS NULL
    AND (OLD.capability = 'publish' OR OLD.capability_inherits_profile = 1)
    AND (
        NEW.revoked_at IS NOT NULL
        OR NEW.capability IS NOT OLD.capability
        OR NEW.capability_inherits_profile IS NOT OLD.capability_inherits_profile
        OR NEW.runtime_approval_policy IS NOT OLD.runtime_approval_policy
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = NEW.agent_id AND working_folder_id = NEW.working_folder_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_folder_grant_single_default_insert
BEFORE INSERT ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.revoked_at IS NULL
 AND EXISTS (
    SELECT 1 FROM chat_agent_working_folder_grants other
    WHERE other.conversation_id = NEW.conversation_id
      AND other.agent_id = NEW.agent_id
      AND other.working_folder_id <> NEW.working_folder_id
      AND other.is_default = 1 AND other.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'An AI channel membership has one default working folder');
END;

CREATE TRIGGER chat_agent_folder_grant_single_default_update
BEFORE UPDATE OF is_default, revoked_at ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.revoked_at IS NULL
 AND EXISTS (
    SELECT 1 FROM chat_agent_working_folder_grants other
    WHERE other.conversation_id = NEW.conversation_id
      AND other.agent_id = NEW.agent_id
      AND other.working_folder_id <> NEW.working_folder_id
      AND other.is_default = 1 AND other.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'An AI channel membership has one default working folder');
END;

CREATE TRIGGER chat_agent_policy_identity_insert
BEFORE INSERT ON chat_agent_policy_revisions
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_participants participant
    WHERE participant.id = NEW.agent_id
      AND participant.participant_kind = 'ai_agent'
)
BEGIN
    SELECT RAISE(ABORT, 'Agent policies require an AI agent participant');
END;

CREATE TRIGGER chat_agent_policy_immutable_delete
BEFORE DELETE ON chat_agent_policy_revisions
BEGIN
    SELECT RAISE(ABORT, 'Agent policy revisions are immutable');
END;

CREATE TRIGGER chat_agent_policy_immutable_update
BEFORE UPDATE ON chat_agent_policy_revisions
BEGIN
    SELECT RAISE(ABORT, 'Agent policy revisions are immutable');
END;

CREATE TRIGGER chat_agent_push_approvals_safety_mode
AFTER UPDATE OF current_policy_safety_mode ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.current_policy_safety_mode IS NOT OLD.current_policy_safety_mode
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = NEW.participant_id AND revoked_at IS NULL;
END;

CREATE TRIGGER sync_capture_chat_participants_delete
AFTER DELETE ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (352, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_participants_insert
AFTER INSERT ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (352, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_participants_update
AFTER UPDATE ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 352, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.avatar_schema_version IS NOT NEW.avatar_schema_version OR OLD.avatar_data IS NOT NEW.avatar_data COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_agent_channel_memberships_delete
AFTER DELETE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_agent_channel_memberships_insert
AFTER INSERT ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_channel_memberships_update
AFTER UPDATE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.access_profile_id IS NOT NEW.access_profile_id COLLATE BINARY
                AND NOT (NEW.access_profile_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_access_profiles WHERE id = OLD.access_profile_id))) OR OLD.read_history IS NOT NEW.read_history OR OLD.read_history_inherits_profile IS NOT NEW.read_history_inherits_profile OR OLD.participate IS NOT NEW.participate OR OLD.participate_inherits_profile IS NOT NEW.participate_inherits_profile OR OLD.history_boundary IS NOT NEW.history_boundary COLLATE BINARY OR OLD.history_boundary_inherits_profile IS NOT NEW.history_boundary_inherits_profile OR OLD.history_from_order_key IS NOT NEW.history_from_order_key COLLATE BINARY OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.scratch_runtime_approval_policy IS NOT NEW.scratch_runtime_approval_policy COLLATE BINARY OR OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_policy_revisions_delete
AFTER DELETE ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (354, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_agent_policy_revisions_insert
AFTER INSERT ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (354, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_policy_revisions_update
AFTER UPDATE ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 354, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_agent_usage_daily_delete
AFTER DELETE ON chat_agent_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (399, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_agent_usage_daily_insert
AFTER INSERT ON chat_agent_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (399, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_usage_daily_update
AFTER UPDATE ON chat_agent_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 399, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.counted_tokens IS NOT NEW.counted_tokens
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.working_seconds IS NOT NEW.working_seconds
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.run_count IS NOT NEW.run_count
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_agent_working_folder_grants_delete
AFTER DELETE ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (363, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_agent_working_folder_grants_insert
AFTER INSERT ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (363, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_working_folder_grants_update
AFTER UPDATE ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 363, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.capability IS NOT NEW.capability COLLATE BINARY OR OLD.capability_inherits_profile IS NOT NEW.capability_inherits_profile OR OLD.is_default IS NOT NEW.is_default OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.revoked_at IS NOT NEW.revoked_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 363, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agents_delete
AFTER DELETE ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, OLD.participant_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_agents_insert
AFTER INSERT ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, NEW.participant_id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agents_update
AFTER UPDATE ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 353, NEW.participant_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.role IS NOT NEW.role COLLATE BINARY OR OLD.instructions IS NOT NEW.instructions COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.current_policy_revision_id IS NOT NEW.current_policy_revision_id COLLATE BINARY OR OLD.current_policy_safety_mode IS NOT NEW.current_policy_safety_mode COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.home_device_id IS NOT NEW.home_device_id COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_immutable_chat_participants
BEFORE UPDATE OF id, participant_kind, created_at ON chat_participants
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.participant_kind IS NOT NEW.participant_kind COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_push_approvals
BEFORE UPDATE OF sync_key, agent_id, working_folder_id, push_url, destination_branch ON chat_push_approvals
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY OR OLD.push_url IS NOT NEW.push_url COLLATE BINARY OR OLD.destination_branch IS NOT NEW.destination_branch COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_agent_channel_memberships
BEFORE UPDATE OF sync_key, conversation_id, agent_id ON chat_agent_channel_memberships
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_agent_policy_revisions
BEFORE UPDATE OF id, agent_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode ON chat_agent_policy_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.provider_instance_id IS NOT NEW.provider_instance_id COLLATE BINARY OR OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY OR OLD.effort IS NOT NEW.effort COLLATE BINARY OR OLD.speed IS NOT NEW.speed COLLATE BINARY OR OLD.provider_options_schema_version IS NOT NEW.provider_options_schema_version OR OLD.provider_options_data IS NOT NEW.provider_options_data COLLATE BINARY OR OLD.interaction_mode IS NOT NEW.interaction_mode COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.safety_mode IS NOT NEW.safety_mode COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_agent_usage_daily
BEFORE UPDATE OF id, device_id, agent_id, local_date ON chat_agent_usage_daily
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.device_id IS NOT NEW.device_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.local_date IS NOT NEW.local_date COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_agent_working_folder_grants
BEFORE UPDATE OF sync_key, conversation_id, agent_id, working_folder_id ON chat_agent_working_folder_grants
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_agents
BEFORE UPDATE OF participant_id, created_at ON chat_agents
WHEN OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_immutable_chat_work_assignments
BEFORE UPDATE OF id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling ON chat_work_assignments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.triggering_message_item_id IS NOT NEW.triggering_message_item_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.override_model_selection_schema_version IS NOT NEW.override_model_selection_schema_version OR OLD.override_model_selection_data IS NOT NEW.override_model_selection_data COLLATE BINARY OR OLD.parent_assignment_id IS NOT NEW.parent_assignment_id COLLATE BINARY OR OLD.delegation_depth IS NOT NEW.delegation_depth OR OLD.delegation_objective IS NOT NEW.delegation_objective COLLATE BINARY OR OLD.delegation_expected_result IS NOT NEW.delegation_expected_result COLLATE BINARY OR OLD.delegation_folder_ceiling IS NOT NEW.delegation_folder_ceiling COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_participants
BEFORE INSERT ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 352 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_recreate_chat_agent_policy_revisions
BEFORE INSERT ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 354 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_agent_policy_revisions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_recreate_chat_agent_usage_daily
BEFORE INSERT ON chat_agent_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 399 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_agent_usage_daily WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_recreate_chat_agents
BEFORE INSERT ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 353 AND row_key = NEW.participant_id)
    AND NOT EXISTS (SELECT 1 FROM chat_agents WHERE participant_id = NEW.participant_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_values_chat_participants_insert
BEFORE INSERT ON chat_participants
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.avatar_schema_version) = 'integer' AND NEW.avatar_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.avatar_data) = 'text' AND instr(CAST(NEW.avatar_data AS BLOB), x'00') = 0 AND length(NEW.avatar_data) <= 16384 AND length(CAST(NEW.avatar_data AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_participants_update
BEFORE UPDATE OF participant_kind, created_at, display_name, avatar_schema_version, avatar_data, archived_at, updated_at ON chat_participants
WHEN ((typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.avatar_schema_version) = 'integer' AND NEW.avatar_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.avatar_data) = 'text' AND instr(CAST(NEW.avatar_data AS BLOB), x'00') = 0 AND length(NEW.avatar_data) <= 16384 AND length(CAST(NEW.avatar_data AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_push_approvals_insert
BEFORE INSERT ON chat_push_approvals
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_push_approvals_update
BEFORE UPDATE OF agent_id, working_folder_id, push_url, destination_branch, runtime_approval_policy, safety_mode, approved_by_participant_id, approved_at, revoked_at ON chat_push_approvals
WHEN ((NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_channel_memberships_insert
BEFORE INSERT ON chat_agent_channel_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_order_key IS NULL OR (typeof(NEW.history_from_order_key) = 'text' AND instr(CAST(NEW.history_from_order_key AS BLOB), x'00') = 0 AND length(NEW.history_from_order_key) <= 128 AND length(CAST(NEW.history_from_order_key AS BLOB)) <= 262144))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_channel_memberships_update
BEFORE UPDATE OF conversation_id, agent_id, access_profile_id, read_history, read_history_inherits_profile, participate, participate_inherits_profile, history_boundary, history_boundary_inherits_profile, history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, removed_at, created_at, updated_at ON chat_agent_channel_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_order_key IS NULL OR (typeof(NEW.history_from_order_key) = 'text' AND instr(CAST(NEW.history_from_order_key AS BLOB), x'00') = 0 AND length(NEW.history_from_order_key) <= 128 AND length(CAST(NEW.history_from_order_key AS BLOB)) <= 262144))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_policy_revisions_insert
BEFORE INSERT ON chat_agent_policy_revisions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.provider_instance_id) = 'text' AND instr(CAST(NEW.provider_instance_id AS BLOB), x'00') = 0 AND length(NEW.provider_instance_id) <= 1024 AND length(CAST(NEW.provider_instance_id AS BLOB)) <= 262144)
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (NEW.effort IS NULL OR (typeof(NEW.effort) = 'text' AND instr(CAST(NEW.effort AS BLOB), x'00') = 0 AND length(NEW.effort) <= 32 AND length(CAST(NEW.effort AS BLOB)) <= 262144))
    AND (NEW.speed IS NULL OR (typeof(NEW.speed) = 'text' AND instr(CAST(NEW.speed AS BLOB), x'00') = 0 AND length(NEW.speed) <= 32 AND length(CAST(NEW.speed AS BLOB)) <= 262144))
    AND (typeof(NEW.provider_options_schema_version) = 'integer' AND NEW.provider_options_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.provider_options_data) = 'text' AND instr(CAST(NEW.provider_options_data AS BLOB), x'00') = 0 AND length(NEW.provider_options_data) <= 16384 AND length(CAST(NEW.provider_options_data AS BLOB)) <= 262144)
    AND (typeof(NEW.interaction_mode) = 'text' AND instr(CAST(NEW.interaction_mode AS BLOB), x'00') = 0 AND length(NEW.interaction_mode) <= 32 AND length(CAST(NEW.interaction_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_policy_revisions_update
BEFORE UPDATE OF agent_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode ON chat_agent_policy_revisions
WHEN ((NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.provider_instance_id) = 'text' AND instr(CAST(NEW.provider_instance_id AS BLOB), x'00') = 0 AND length(NEW.provider_instance_id) <= 1024 AND length(CAST(NEW.provider_instance_id AS BLOB)) <= 262144)
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (NEW.effort IS NULL OR (typeof(NEW.effort) = 'text' AND instr(CAST(NEW.effort AS BLOB), x'00') = 0 AND length(NEW.effort) <= 32 AND length(CAST(NEW.effort AS BLOB)) <= 262144))
    AND (NEW.speed IS NULL OR (typeof(NEW.speed) = 'text' AND instr(CAST(NEW.speed AS BLOB), x'00') = 0 AND length(NEW.speed) <= 32 AND length(CAST(NEW.speed AS BLOB)) <= 262144))
    AND (typeof(NEW.provider_options_schema_version) = 'integer' AND NEW.provider_options_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.provider_options_data) = 'text' AND instr(CAST(NEW.provider_options_data AS BLOB), x'00') = 0 AND length(NEW.provider_options_data) <= 16384 AND length(CAST(NEW.provider_options_data AS BLOB)) <= 262144)
    AND (typeof(NEW.interaction_mode) = 'text' AND instr(CAST(NEW.interaction_mode AS BLOB), x'00') = 0 AND length(NEW.interaction_mode) <= 32 AND length(CAST(NEW.interaction_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_usage_daily_insert
BEFORE INSERT ON chat_agent_usage_daily
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.counted_tokens) = 'integer' AND NEW.counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.working_seconds) = 'integer' AND NEW.working_seconds BETWEEN 0 AND 8640000)
    AND (typeof(NEW.run_count) = 'integer' AND NEW.run_count BETWEEN 0 AND 100000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_usage_daily_update
BEFORE UPDATE OF device_id, agent_id, local_date, counted_tokens, working_seconds, run_count ON chat_agent_usage_daily
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.counted_tokens) = 'integer' AND NEW.counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.working_seconds) = 'integer' AND NEW.working_seconds BETWEEN 0 AND 8640000)
    AND (typeof(NEW.run_count) = 'integer' AND NEW.run_count BETWEEN 0 AND 100000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_working_folder_grants_insert
BEFORE INSERT ON chat_agent_working_folder_grants
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.capability) = 'text' AND instr(CAST(NEW.capability AS BLOB), x'00') = 0 AND length(NEW.capability) <= 32 AND length(CAST(NEW.capability AS BLOB)) <= 262144)
    AND (typeof(NEW.capability_inherits_profile) = 'integer' AND NEW.capability_inherits_profile IN (0, 1))
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_working_folder_grants_update
BEFORE UPDATE OF conversation_id, agent_id, working_folder_id, capability, capability_inherits_profile, is_default, runtime_approval_policy, revoked_at, project_id, created_at ON chat_agent_working_folder_grants
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.capability) = 'text' AND instr(CAST(NEW.capability AS BLOB), x'00') = 0 AND length(NEW.capability) <= 32 AND length(CAST(NEW.capability AS BLOB)) <= 262144)
    AND (typeof(NEW.capability_inherits_profile) = 'integer' AND NEW.capability_inherits_profile IN (0, 1))
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agents_insert
BEFORE INSERT ON chat_agents
WHEN ((typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.role) = 'text' AND instr(CAST(NEW.role AS BLOB), x'00') = 0 AND length(trim(NEW.role)) BETWEEN 1 AND 1000 AND length(CAST(NEW.role AS BLOB)) <= 262144)
    AND (typeof(NEW.instructions) = 'text' AND instr(CAST(NEW.instructions AS BLOB), x'00') = 0 AND length(NEW.instructions) <= 65536 AND length(CAST(NEW.instructions AS BLOB)) <= 262144)
    AND (NEW.current_policy_revision_id IS NULL OR (typeof(NEW.current_policy_revision_id) = 'text' AND instr(CAST(NEW.current_policy_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_policy_revision_id) <> '' AND length(CAST(NEW.current_policy_revision_id AS BLOB)) <= 128))
    AND (NEW.current_policy_safety_mode IS NULL OR (typeof(NEW.current_policy_safety_mode) = 'text' AND instr(CAST(NEW.current_policy_safety_mode AS BLOB), x'00') = 0 AND length(NEW.current_policy_safety_mode) <= 32 AND length(CAST(NEW.current_policy_safety_mode AS BLOB)) <= 262144))
    AND (NEW.home_device_id IS NULL OR (typeof(NEW.home_device_id) = 'text' AND instr(CAST(NEW.home_device_id AS BLOB), x'00') = 0 AND length(NEW.home_device_id) <= 160 AND length(CAST(NEW.home_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agents_update
BEFORE UPDATE OF created_at, role, instructions, current_policy_revision_id, current_policy_safety_mode, home_device_id, updated_at ON chat_agents
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.role) = 'text' AND instr(CAST(NEW.role AS BLOB), x'00') = 0 AND length(trim(NEW.role)) BETWEEN 1 AND 1000 AND length(CAST(NEW.role AS BLOB)) <= 262144)
    AND (typeof(NEW.instructions) = 'text' AND instr(CAST(NEW.instructions AS BLOB), x'00') = 0 AND length(NEW.instructions) <= 65536 AND length(CAST(NEW.instructions AS BLOB)) <= 262144)
    AND (NEW.current_policy_revision_id IS NULL OR (typeof(NEW.current_policy_revision_id) = 'text' AND instr(CAST(NEW.current_policy_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_policy_revision_id) <> '' AND length(CAST(NEW.current_policy_revision_id AS BLOB)) <= 128))
    AND (NEW.current_policy_safety_mode IS NULL OR (typeof(NEW.current_policy_safety_mode) = 'text' AND instr(CAST(NEW.current_policy_safety_mode AS BLOB), x'00') = 0 AND length(NEW.current_policy_safety_mode) <= 32 AND length(CAST(NEW.current_policy_safety_mode AS BLOB)) <= 262144))
    AND (NEW.home_device_id IS NULL OR (typeof(NEW.home_device_id) = 'text' AND instr(CAST(NEW.home_device_id AS BLOB), x'00') = 0 AND length(NEW.home_device_id) <= 160 AND length(CAST(NEW.home_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_insert
BEFORE INSERT ON chat_work_assignments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.override_model_selection_schema_version IS NULL OR (typeof(NEW.override_model_selection_schema_version) = 'integer' AND NEW.override_model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.override_model_selection_data IS NULL OR (typeof(NEW.override_model_selection_data) = 'text' AND instr(CAST(NEW.override_model_selection_data AS BLOB), x'00') = 0 AND length(NEW.override_model_selection_data) <= 16384 AND length(CAST(NEW.override_model_selection_data AS BLOB)) <= 262144))
    AND (NEW.previous_assignment_id IS NULL OR (typeof(NEW.previous_assignment_id) = 'text' AND instr(CAST(NEW.previous_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.previous_assignment_id) <> '' AND length(CAST(NEW.previous_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.state_reason IS NULL OR (typeof(NEW.state_reason) = 'text' AND instr(CAST(NEW.state_reason AS BLOB), x'00') = 0 AND length(NEW.state_reason) <= 4000 AND length(CAST(NEW.state_reason AS BLOB)) <= 262144))
    AND (NEW.settled_at IS NULL OR (typeof(NEW.settled_at) = 'text' AND instr(CAST(NEW.settled_at AS BLOB), x'00') = 0 AND trim(NEW.settled_at) <> '' AND length(CAST(NEW.settled_at AS BLOB)) <= 64))
    AND (NEW.review_outcome IS NULL OR (typeof(NEW.review_outcome) = 'text' AND instr(CAST(NEW.review_outcome AS BLOB), x'00') = 0 AND length(NEW.review_outcome) <= 32 AND length(CAST(NEW.review_outcome AS BLOB)) <= 262144))
    AND (NEW.reviewed_at IS NULL OR (typeof(NEW.reviewed_at) = 'text' AND instr(CAST(NEW.reviewed_at AS BLOB), x'00') = 0 AND trim(NEW.reviewed_at) <> '' AND length(CAST(NEW.reviewed_at AS BLOB)) <= 64))
    AND (NEW.reviewed_by_participant_id IS NULL OR (typeof(NEW.reviewed_by_participant_id) = 'text' AND instr(CAST(NEW.reviewed_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.reviewed_by_participant_id) <> '' AND length(CAST(NEW.reviewed_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.parent_assignment_id IS NULL OR (typeof(NEW.parent_assignment_id) = 'text' AND instr(CAST(NEW.parent_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.parent_assignment_id) <> '' AND length(CAST(NEW.parent_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.delegation_depth) = 'integer' AND NEW.delegation_depth BETWEEN 0 AND 2)
    AND (NEW.delegation_objective IS NULL OR (typeof(NEW.delegation_objective) = 'text' AND instr(CAST(NEW.delegation_objective AS BLOB), x'00') = 0 AND length(NEW.delegation_objective) <= 4000 AND length(CAST(NEW.delegation_objective AS BLOB)) <= 262144))
    AND (NEW.delegation_expected_result IS NULL OR (typeof(NEW.delegation_expected_result) = 'text' AND instr(CAST(NEW.delegation_expected_result AS BLOB), x'00') = 0 AND length(NEW.delegation_expected_result) <= 2000 AND length(CAST(NEW.delegation_expected_result AS BLOB)) <= 262144))
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, agent_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data, previous_assignment_id, state, state_reason, settled_at, review_outcome, reviewed_at, reviewed_by_participant_id, updated_at, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling ON chat_work_assignments
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.override_model_selection_schema_version IS NULL OR (typeof(NEW.override_model_selection_schema_version) = 'integer' AND NEW.override_model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.override_model_selection_data IS NULL OR (typeof(NEW.override_model_selection_data) = 'text' AND instr(CAST(NEW.override_model_selection_data AS BLOB), x'00') = 0 AND length(NEW.override_model_selection_data) <= 16384 AND length(CAST(NEW.override_model_selection_data AS BLOB)) <= 262144))
    AND (NEW.previous_assignment_id IS NULL OR (typeof(NEW.previous_assignment_id) = 'text' AND instr(CAST(NEW.previous_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.previous_assignment_id) <> '' AND length(CAST(NEW.previous_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.state_reason IS NULL OR (typeof(NEW.state_reason) = 'text' AND instr(CAST(NEW.state_reason AS BLOB), x'00') = 0 AND length(NEW.state_reason) <= 4000 AND length(CAST(NEW.state_reason AS BLOB)) <= 262144))
    AND (NEW.settled_at IS NULL OR (typeof(NEW.settled_at) = 'text' AND instr(CAST(NEW.settled_at AS BLOB), x'00') = 0 AND trim(NEW.settled_at) <> '' AND length(CAST(NEW.settled_at AS BLOB)) <= 64))
    AND (NEW.review_outcome IS NULL OR (typeof(NEW.review_outcome) = 'text' AND instr(CAST(NEW.review_outcome AS BLOB), x'00') = 0 AND length(NEW.review_outcome) <= 32 AND length(CAST(NEW.review_outcome AS BLOB)) <= 262144))
    AND (NEW.reviewed_at IS NULL OR (typeof(NEW.reviewed_at) = 'text' AND instr(CAST(NEW.reviewed_at AS BLOB), x'00') = 0 AND trim(NEW.reviewed_at) <> '' AND length(CAST(NEW.reviewed_at AS BLOB)) <= 64))
    AND (NEW.reviewed_by_participant_id IS NULL OR (typeof(NEW.reviewed_by_participant_id) = 'text' AND instr(CAST(NEW.reviewed_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.reviewed_by_participant_id) <> '' AND length(CAST(NEW.reviewed_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.parent_assignment_id IS NULL OR (typeof(NEW.parent_assignment_id) = 'text' AND instr(CAST(NEW.parent_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.parent_assignment_id) <> '' AND length(CAST(NEW.parent_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.delegation_depth) = 'integer' AND NEW.delegation_depth BETWEEN 0 AND 2)
    AND (NEW.delegation_objective IS NULL OR (typeof(NEW.delegation_objective) = 'text' AND instr(CAST(NEW.delegation_objective AS BLOB), x'00') = 0 AND length(NEW.delegation_objective) <= 4000 AND length(CAST(NEW.delegation_objective AS BLOB)) <= 262144))
    AND (NEW.delegation_expected_result IS NULL OR (typeof(NEW.delegation_expected_result) = 'text' AND instr(CAST(NEW.delegation_expected_result AS BLOB), x'00') = 0 AND length(NEW.delegation_expected_result) <= 2000 AND length(CAST(NEW.delegation_expected_result AS BLOB)) <= 262144))
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE VIEW chat_agent_effective_policies AS
SELECT policy.*
FROM chat_agents agent
JOIN chat_agent_policy_revisions_numbered policy
  ON policy.id = CASE
      WHEN EXISTS (
          SELECT 1 FROM chat_agent_policy_revisions named
          WHERE named.id = agent.current_policy_revision_id
      ) THEN (
          SELECT named.id FROM chat_agent_policy_revisions named
          WHERE named.id = agent.current_policy_revision_id
            AND named.agent_id = agent.participant_id
            AND named.safety_mode IS agent.current_policy_safety_mode
      )
      ELSE (
          SELECT newest.id FROM chat_agent_policy_revisions newest
          WHERE newest.agent_id = agent.participant_id
          ORDER BY newest.created_at DESC, newest.id DESC
          LIMIT 1
      )
  END;

CREATE VIEW chat_agent_policy_revisions_numbered AS
SELECT
    policy.id,
    policy.agent_id,
    policy.provider_instance_id,
    policy.model_selection_schema_version,
    policy.model_selection_data,
    policy.effort,
    policy.speed,
    policy.provider_options_schema_version,
    policy.provider_options_data,
    policy.interaction_mode,
    policy.created_at,
    policy.safety_mode,
    (
        SELECT count(*) FROM chat_agent_policy_revisions earlier
        WHERE earlier.agent_id = policy.agent_id
          AND (earlier.created_at, earlier.id) <= (policy.created_at, policy.id)
    ) AS revision
FROM chat_agent_policy_revisions policy;

COMMIT;

PRAGMA foreign_keys = ON;

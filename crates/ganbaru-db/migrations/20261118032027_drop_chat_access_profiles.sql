-- no-transaction
-- Chat drops access profiles: an AI channel membership's own capabilities and its working folder
-- grants are the whole authority, with no ceiling and nothing inherited. The profile tables
-- (sync ids 355 and 356), their views, and the profile columns of memberships, grants, and
-- assignment authorization revisions go away, with the sync bookkeeping of the retired tables,
-- since the engine treats rows of a table it does not know as corrupt.
--
-- Rebuilding tables that other tables cascade from would drop their rows, so foreign keys are off
-- for this connection and the whole change commits as one transaction.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP VIEW chat_access_profile_effective_revisions;
DROP VIEW chat_access_profile_revisions_numbered;

DROP TRIGGER chat_access_profile_audience_publish;
DROP TRIGGER chat_access_profile_builtin_protected;
DROP TRIGGER chat_access_profile_builtin_revision_protected;
DROP TRIGGER chat_access_profile_push_approvals_update;
DROP TRIGGER chat_access_profile_revision_counter_monotonic;
DROP TRIGGER chat_access_profile_revision_immutable_delete;
DROP TRIGGER chat_access_profile_revision_immutable_update;
DROP TRIGGER sync_capture_chat_access_profile_revisions_delete;
DROP TRIGGER sync_capture_chat_access_profile_revisions_insert;
DROP TRIGGER sync_capture_chat_access_profile_revisions_update;
DROP TRIGGER sync_capture_chat_access_profiles_delete;
DROP TRIGGER sync_capture_chat_access_profiles_insert;
DROP TRIGGER sync_capture_chat_access_profiles_update;
DROP TRIGGER sync_immutable_chat_access_profile_revisions;
DROP TRIGGER sync_immutable_chat_access_profiles;
DROP TRIGGER sync_recreate_chat_access_profile_revisions;
DROP TRIGGER sync_recreate_chat_access_profiles;
DROP TRIGGER sync_values_chat_access_profile_revisions_insert;
DROP TRIGGER sync_values_chat_access_profile_revisions_update;
DROP TRIGGER sync_values_chat_access_profiles_insert;
DROP TRIGGER sync_values_chat_access_profiles_update;

DROP TRIGGER chat_agent_channel_membership_audience_delete;
DROP TRIGGER chat_agent_channel_membership_audience_insert;
DROP TRIGGER chat_agent_channel_membership_audience_update;
DROP TRIGGER chat_agent_channel_membership_identity_insert;
DROP TRIGGER chat_agent_channel_membership_push_approvals_delete;
DROP TRIGGER chat_agent_channel_membership_push_approvals_update;
DROP TRIGGER chat_agent_channel_membership_requires_membership_insert;
DROP TRIGGER sync_capture_chat_agent_channel_memberships_delete;
DROP TRIGGER sync_capture_chat_agent_channel_memberships_insert;
DROP TRIGGER sync_capture_chat_agent_channel_memberships_update;
DROP TRIGGER sync_immutable_chat_agent_channel_memberships;
DROP TRIGGER sync_values_chat_agent_channel_memberships_insert;
DROP TRIGGER sync_values_chat_agent_channel_memberships_update;

DROP TRIGGER chat_agent_folder_grant_identity_insert;
DROP TRIGGER chat_agent_folder_grant_project_insert;
DROP TRIGGER chat_agent_folder_grant_project_update;
DROP TRIGGER chat_agent_folder_grant_push_approvals_delete;
DROP TRIGGER chat_agent_folder_grant_push_approvals_update;
DROP TRIGGER chat_agent_folder_grant_single_default_insert;
DROP TRIGGER chat_agent_folder_grant_single_default_update;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_delete;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_insert;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_update;
DROP TRIGGER sync_immutable_chat_agent_working_folder_grants;
DROP TRIGGER sync_values_chat_agent_working_folder_grants_insert;
DROP TRIGGER sync_values_chat_agent_working_folder_grants_update;

DROP TRIGGER chat_assignment_authorization_delete_denied;
DROP TRIGGER chat_assignment_authorization_id_immutable;
DROP TRIGGER chat_assignment_authorization_reason_immutable;
DROP TRIGGER chat_assignment_authorization_revocation_insert;
DROP TRIGGER chat_assignment_authorization_revocation_update;
DROP TRIGGER chat_assignment_authorization_scope_immutable;
DROP TRIGGER chat_assignment_authorization_state_transition;
DROP TRIGGER chat_assignment_authorization_target_required_insert;
DROP TRIGGER chat_assignment_authorization_terminal_immutable;

DROP INDEX idx_chat_agent_channel_memberships_agent;
DROP INDEX idx_chat_assignment_authorization_revisions_active;

CREATE TABLE chat_agent_channel_memberships_previous AS
SELECT * FROM chat_agent_channel_memberships;
DROP TABLE chat_agent_channel_memberships;
CREATE TABLE chat_agent_channel_memberships (
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
  read_history INTEGER NOT NULL DEFAULT 0 CHECK (read_history IN (0, 1)),
  participate INTEGER NOT NULL DEFAULT 0 CHECK (participate IN (0, 1)),
  history_boundary TEXT NOT NULL DEFAULT 'entire' CHECK (
      history_boundary IN ('entire', 'from_grant')
  ),
  history_from_order_key TEXT CHECK (
      history_from_order_key IS NULL OR length(history_from_order_key) BETWEEN 1 AND 128
  ),
  runtime_approval_policy TEXT CHECK (
      runtime_approval_policy IS NULL OR runtime_approval_policy IN (
          'ask', 'auto_approve', 'unattended', 'provider_custom'
      )
  ),
  scratch_runtime_approval_policy TEXT CHECK (
      scratch_runtime_approval_policy IS NULL OR scratch_runtime_approval_policy IN (
          'ask', 'auto_approve', 'unattended', 'provider_custom'
      )
  ),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
  PRIMARY KEY (conversation_id, agent_id),
  CHECK (
      (history_boundary = 'entire' AND history_from_order_key IS NULL)
      OR (history_boundary = 'from_grant' AND history_from_order_key IS NOT NULL)
  )
) STRICT, WITHOUT ROWID;
INSERT INTO chat_agent_channel_memberships (
    conversation_id, agent_id, sync_key, read_history, participate, history_boundary,
    history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, created_at,
    updated_at, removed_at
)
SELECT
    conversation_id, agent_id, sync_key, read_history, participate, history_boundary,
    history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, created_at,
    updated_at, removed_at
FROM chat_agent_channel_memberships_previous;
DROP TABLE chat_agent_channel_memberships_previous;

CREATE INDEX idx_chat_agent_channel_memberships_agent
ON chat_agent_channel_memberships(agent_id, conversation_id);

ALTER TABLE chat_agent_working_folder_grants DROP COLUMN capability_inherits_profile;

CREATE TABLE chat_assignment_authorization_revisions_previous AS
SELECT * FROM chat_assignment_authorization_revisions;
DROP TABLE chat_assignment_authorization_revisions;
CREATE TABLE chat_assignment_authorization_revisions (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
  assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  revision INTEGER NOT NULL CHECK (revision >= 1),
  requester_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_policy_revision_id TEXT NOT NULL REFERENCES chat_agent_policy_revisions(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_access_revision INTEGER NOT NULL CHECK (agent_access_revision >= 1),
  destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  execution_environment_id TEXT REFERENCES chat_execution_environments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  working_folder_id TEXT REFERENCES project_working_folders(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  resolved_runtime_approval_policy TEXT NOT NULL CHECK (
      resolved_runtime_approval_policy IN (
          'ask', 'auto_approve', 'unattended', 'provider_custom'
      )
  ),
  scope_digest TEXT NOT NULL CHECK (length(scope_digest) = 64),
  decision_state TEXT NOT NULL CHECK (decision_state IN ('allowed', 'blocked', 'revoked')),
  reason TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 4000),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
  UNIQUE (assignment_id, revision),
  CHECK (
      (execution_environment_id IS NULL AND working_folder_id IS NULL)
      OR execution_environment_id IS NOT NULL
  )
) STRICT;
INSERT INTO chat_assignment_authorization_revisions (
    id, assignment_id, revision, requester_participant_id, agent_policy_revision_id,
    agent_access_revision, destination_conversation_id, execution_environment_id,
    working_folder_id, resolved_runtime_approval_policy, scope_digest, decision_state, reason,
    created_at, revoked_at
)
SELECT
    id, assignment_id, revision, requester_participant_id, agent_policy_revision_id,
    agent_access_revision, destination_conversation_id, execution_environment_id,
    working_folder_id, resolved_runtime_approval_policy, scope_digest, decision_state, reason,
    created_at, revoked_at
FROM chat_assignment_authorization_revisions_previous;
DROP TABLE chat_assignment_authorization_revisions_previous;

CREATE INDEX idx_chat_assignment_authorization_revisions_active
ON chat_assignment_authorization_revisions(assignment_id, decision_state, revision DESC);

DROP INDEX idx_chat_access_profile_revisions_profile;
DROP INDEX idx_chat_access_profiles_builtin;
DROP TABLE chat_access_profile_revisions;
DROP TABLE chat_access_profiles;

DELETE FROM sync_capture WHERE table_id IN (355, 356);
DELETE FROM sync_version_refs WHERE table_id IN (355, 356);
DELETE FROM sync_register_versions WHERE table_id IN (355, 356);
DELETE FROM sync_row_creations WHERE table_id IN (355, 356);
DELETE FROM sync_tombstones WHERE table_id IN (355, 356);
DELETE FROM sync_contested_classes WHERE table_id IN (355, 356);
DELETE FROM sync_displaced_versions WHERE table_id IN (355, 356);
DELETE FROM sync_late_changes WHERE table_id IN (355, 356);
DELETE FROM sync_refused_rows WHERE table_id IN (355, 356);
DELETE FROM sync_restated_groups WHERE table_id IN (355, 356);
DELETE FROM sync_stale_rows WHERE table_id IN (355, 356);
DELETE FROM sync_unique_classes WHERE table_id IN (355, 356);
DELETE FROM sync_seeded_tables WHERE table_id IN (355, 356);
DELETE FROM sync_rows WHERE table_id IN (355, 356);

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
AFTER UPDATE OF conversation_id, agent_id, read_history, participate, history_boundary,
    history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, created_at,
    updated_at, removed_at
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
AFTER UPDATE OF runtime_approval_policy, removed_at
ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
    AND (
        NEW.removed_at IS NOT NULL
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
    AND OLD.capability = 'publish'
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = OLD.agent_id AND working_folder_id = OLD.working_folder_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_folder_grant_push_approvals_update
AFTER UPDATE OF capability, runtime_approval_policy, revoked_at
ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.revoked_at IS NULL
    AND OLD.capability = 'publish'
    AND (
        NEW.revoked_at IS NOT NULL
        OR NEW.capability IS NOT OLD.capability
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

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
 AND EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.requester_participant_id)
 AND EXISTS (SELECT 1 FROM chat_agent_policy_revisions WHERE id = OLD.agent_policy_revision_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.destination_conversation_id)
 AND (OLD.execution_environment_id IS NULL
      OR EXISTS (SELECT 1 FROM chat_execution_environments WHERE id = OLD.execution_environment_id))
 AND (OLD.working_folder_id IS NULL
      OR EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id))
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization revisions are immutable');
END;

CREATE TRIGGER chat_assignment_authorization_id_immutable
BEFORE UPDATE OF id ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization identity is immutable');
END;

CREATE TRIGGER chat_assignment_authorization_reason_immutable
BEFORE UPDATE OF reason ON chat_assignment_authorization_revisions
WHEN NEW.reason IS NOT OLD.reason
 AND NOT (OLD.decision_state = 'allowed' AND NEW.decision_state = 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'Authorization reasons are immutable before revocation');
END;

CREATE TRIGGER chat_assignment_authorization_revocation_insert
BEFORE INSERT ON chat_assignment_authorization_revisions
WHEN (NEW.decision_state = 'revoked') != (NEW.revoked_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Authorization revocation state is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_revocation_update
BEFORE UPDATE OF decision_state, revoked_at ON chat_assignment_authorization_revisions
WHEN (NEW.decision_state = 'revoked') != (NEW.revoked_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Authorization revocation state is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_scope_immutable
BEFORE UPDATE OF
    assignment_id, revision, requester_participant_id,
    agent_policy_revision_id, agent_access_revision,
    destination_conversation_id,
    execution_environment_id, working_folder_id,
    resolved_runtime_approval_policy, scope_digest, created_at
ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization scopes are immutable');
END;

CREATE TRIGGER chat_assignment_authorization_state_transition
BEFORE UPDATE OF decision_state ON chat_assignment_authorization_revisions
WHEN NEW.decision_state IS NOT OLD.decision_state
 AND NOT (OLD.decision_state = 'allowed' AND NEW.decision_state = 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'Authorization decisions require a new revision');
END;

CREATE TRIGGER chat_assignment_authorization_target_required_insert
BEFORE INSERT ON chat_assignment_authorization_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.kind IN ('current_folder', 'worktree')
                AND environment.working_folder_id = NEW.working_folder_id
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND environment.kind = 'scratch'
                AND environment.working_folder_id IS NULL
                AND environment.scratch_generation_id IS NOT NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization target is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_terminal_immutable
BEFORE UPDATE ON chat_assignment_authorization_revisions
WHEN OLD.decision_state = 'revoked' OR OLD.revoked_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'Revoked authorization revisions are immutable');
END;

CREATE TRIGGER sync_values_chat_agent_channel_memberships_insert
BEFORE INSERT ON chat_agent_channel_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
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
BEFORE UPDATE OF conversation_id, agent_id, read_history, participate, history_boundary, history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, removed_at, created_at, updated_at ON chat_agent_channel_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (NEW.history_from_order_key IS NULL OR (typeof(NEW.history_from_order_key) = 'text' AND instr(CAST(NEW.history_from_order_key AS BLOB), x'00') = 0 AND length(NEW.history_from_order_key) <= 128 AND length(CAST(NEW.history_from_order_key AS BLOB)) <= 262144))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_agent_channel_memberships
BEFORE UPDATE OF sync_key, conversation_id, agent_id ON chat_agent_channel_memberships
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
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
            (CASE WHEN OLD.read_history IS NOT NEW.read_history OR OLD.participate IS NOT NEW.participate OR OLD.history_boundary IS NOT NEW.history_boundary COLLATE BINARY OR OLD.history_from_order_key IS NOT NEW.history_from_order_key COLLATE BINARY OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.scratch_runtime_approval_policy IS NOT NEW.scratch_runtime_approval_policy COLLATE BINARY OR OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
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

CREATE TRIGGER sync_capture_chat_agent_channel_memberships_delete
AFTER DELETE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_agent_working_folder_grants_insert
BEFORE INSERT ON chat_agent_working_folder_grants
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.capability) = 'text' AND instr(CAST(NEW.capability AS BLOB), x'00') = 0 AND length(NEW.capability) <= 32 AND length(CAST(NEW.capability AS BLOB)) <= 262144)
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_working_folder_grants_update
BEFORE UPDATE OF conversation_id, agent_id, working_folder_id, capability, is_default, runtime_approval_policy, revoked_at, project_id, created_at ON chat_agent_working_folder_grants
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.capability) = 'text' AND instr(CAST(NEW.capability AS BLOB), x'00') = 0 AND length(NEW.capability) <= 32 AND length(CAST(NEW.capability AS BLOB)) <= 262144)
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_agent_working_folder_grants
BEFORE UPDATE OF sync_key, conversation_id, agent_id, working_folder_id ON chat_agent_working_folder_grants
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
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
            (CASE WHEN OLD.capability IS NOT NEW.capability COLLATE BINARY OR OLD.is_default IS NOT NEW.is_default OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.revoked_at IS NOT NEW.revoked_at COLLATE BINARY
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

CREATE TRIGGER sync_capture_chat_agent_working_folder_grants_delete
AFTER DELETE ON chat_agent_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (363, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA foreign_keys = ON;

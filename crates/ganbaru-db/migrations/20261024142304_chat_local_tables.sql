-- no-transaction
-- Device-local Chat rows that point at organizational Chat rows cascade when that organizational row is deleted, so
-- a delete that arrives through sync removes the local evidence that depends on it instead of failing. Audit
-- triggers that deny deletion keep denying it while every parent row exists, so foreign key cascades can remove
-- them with any parent. Agent runs cascade with their authorization revision for the same reason.
--
-- Triggers that validate or derive state from organizational rows step aside while the sync engine applies remote
-- operations, because remote rows can arrive in any order and the derived state is rebuilt after materialization.
-- Teammate channel memberships keep a removal timestamp instead of being deleted, and a device-local dispatch row
-- records claim state for each scheduled message.
--
-- SQLite applies a changed foreign key action only by rebuilding the table. Foreign keys are disabled for the
-- rebuild and every copy keeps its columns and values. Legacy rename keeps SQLite from reparsing triggers on
-- other tables while a rebuilt table is briefly absent. The rebuild commits only when the foreign key check reports
-- no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- chat_access_revocation_jobs
CREATE TABLE chat_access_revocation_jobs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    authorization_revision_id TEXT REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (
        state IN ('queued', 'claimed', 'completed', 'failed', 'cancelled')
    ),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 4000),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    claimed_at TEXT CHECK (claimed_at IS NULL OR length(claimed_at) >= 20),
    claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 1024),
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_access_revocation_jobs_next SELECT * FROM chat_access_revocation_jobs;

DROP TABLE chat_access_revocation_jobs;
ALTER TABLE chat_access_revocation_jobs_next RENAME TO chat_access_revocation_jobs;

CREATE INDEX idx_chat_access_revocation_jobs_ready
ON chat_access_revocation_jobs(state, available_at, created_at, id);

-- chat_assignment_authorization_revisions
CREATE TABLE chat_assignment_authorization_revisions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    requester_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_policy_revision_id TEXT NOT NULL REFERENCES chat_teammate_policy_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_access_revision INTEGER NOT NULL CHECK (teammate_access_revision >= 1),
    destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    access_profile_revision_id TEXT NOT NULL REFERENCES chat_access_profile_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
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

INSERT INTO chat_assignment_authorization_revisions_next
SELECT * FROM chat_assignment_authorization_revisions;

DROP TABLE chat_assignment_authorization_revisions;
ALTER TABLE chat_assignment_authorization_revisions_next RENAME TO chat_assignment_authorization_revisions;

CREATE INDEX idx_chat_assignment_authorization_revisions_active
ON chat_assignment_authorization_revisions(assignment_id, decision_state, revision DESC);

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
 AND EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.requester_participant_id)
 AND EXISTS (SELECT 1 FROM chat_teammate_policy_revisions WHERE id = OLD.teammate_policy_revision_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.destination_conversation_id)
 AND EXISTS (SELECT 1 FROM chat_access_profile_revisions WHERE id = OLD.access_profile_revision_id)
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
    teammate_policy_revision_id, teammate_access_revision,
    destination_conversation_id, access_profile_revision_id,
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
WHEN NOT (
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

-- chat_agent_runs
CREATE TABLE chat_agent_runs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_policy_revision_id TEXT NOT NULL REFERENCES chat_teammate_policy_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    authorization_scope_digest TEXT NOT NULL CHECK (length(authorization_scope_digest) = 64),
    provider_turn_id TEXT NOT NULL UNIQUE REFERENCES chat_turns(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    provider_thread_id TEXT REFERENCES chat_threads(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'starting', 'working', 'waiting', 'completed', 'failed', 'cancelled')
    ),
    run_ordinal INTEGER NOT NULL CHECK (run_ordinal >= 1),
    started_at TEXT CHECK (started_at IS NULL OR length(started_at) >= 20),
    settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    UNIQUE (assignment_id, run_ordinal),
    CHECK (
        (working_folder_id IS NULL AND execution_environment_id IS NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NOT NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NOT NULL)
    )
) STRICT;

INSERT INTO chat_agent_runs_next SELECT * FROM chat_agent_runs;

DROP TABLE chat_agent_runs;
ALTER TABLE chat_agent_runs_next RENAME TO chat_agent_runs;

CREATE INDEX idx_chat_agent_runs_authorization
ON chat_agent_runs(authorization_revision_id, state, id);
CREATE INDEX idx_chat_agent_runs_provider_thread
ON chat_agent_runs(provider_thread_id, assignment_id);

CREATE TRIGGER chat_agent_run_authority_immutable
BEFORE UPDATE OF
    assignment_id, project_id, working_folder_id,
    execution_environment_id, scratch_generation_id,
    teammate_policy_revision_id, authorization_revision_id,
    authorization_scope_digest, provider_turn_id,
    provider_thread_id, run_ordinal, created_at
ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run authority is immutable');
END;

CREATE TRIGGER chat_agent_run_authorization_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions authorization
    WHERE authorization.id = NEW.authorization_revision_id
      AND authorization.assignment_id = NEW.assignment_id
      AND authorization.scope_digest = NEW.authorization_scope_digest
      AND authorization.decision_state = 'allowed'
      AND authorization.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run authorization is invalid');
END;

CREATE TRIGGER chat_agent_run_authorization_target_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions authorization
    WHERE authorization.id = NEW.authorization_revision_id
      AND authorization.assignment_id = NEW.assignment_id
      AND authorization.scope_digest = NEW.authorization_scope_digest
      AND authorization.execution_environment_id IS NEW.execution_environment_id
      AND authorization.working_folder_id IS NEW.working_folder_id
      AND authorization.decision_state = 'allowed'
      AND authorization.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run target does not match its authorization');
END;

CREATE TRIGGER chat_agent_run_execution_target_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.working_folder_id = NEW.working_folder_id
                AND NEW.scratch_generation_id IS NULL
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND NEW.scratch_generation_id IS NOT NULL
                AND environment.scratch_generation_id = NEW.scratch_generation_id
                AND environment.working_folder_id IS NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run execution target is inconsistent');
END;

CREATE TRIGGER chat_agent_run_id_immutable
BEFORE UPDATE OF id ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run identity is immutable');
END;

-- chat_assignment_authorized_channel_sources
CREATE TABLE chat_assignment_authorized_channel_sources_next (
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_handle TEXT NOT NULL UNIQUE CHECK (length(source_handle) BETWEEN 32 AND 1024),
    message_reference_id TEXT NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    lower_ordinal INTEGER NOT NULL CHECK (lower_ordinal >= 1),
    high_ordinal INTEGER NOT NULL CHECK (high_ordinal >= lower_ordinal),
    source_revision_cutoff_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    destination_audience_revision INTEGER NOT NULL CHECK (destination_audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (authorization_revision_id, conversation_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_assignment_authorized_channel_sources_next
SELECT * FROM chat_assignment_authorized_channel_sources;

DROP TABLE chat_assignment_authorized_channel_sources;
ALTER TABLE chat_assignment_authorized_channel_sources_next RENAME TO chat_assignment_authorized_channel_sources;

CREATE TRIGGER chat_authorized_channel_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_channel_sources
WHEN EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions
    WHERE id = OLD.authorization_revision_id
)
 AND EXISTS (SELECT 1 FROM chat_message_references WHERE id = OLD.message_reference_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id)
 AND EXISTS (
    SELECT 1 FROM chat_communication_message_revisions
    WHERE id = OLD.source_revision_cutoff_id
)
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

CREATE TRIGGER chat_authorized_channel_source_immutable_update
BEFORE UPDATE ON chat_assignment_authorized_channel_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

-- chat_assignment_context_packages
CREATE TABLE chat_assignment_context_packages_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    serialized_text TEXT NOT NULL CHECK (length(serialized_text) <= 131072),
    serialized_bytes INTEGER NOT NULL CHECK (serialized_bytes BETWEEN 0 AND 131072),
    excluded_thread_reply_count INTEGER NOT NULL DEFAULT 0 CHECK (excluded_thread_reply_count >= 0),
    excluded_channel_message_count INTEGER NOT NULL DEFAULT 0 CHECK (excluded_channel_message_count >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (assignment_id, revision)
) STRICT;

INSERT INTO chat_assignment_context_packages_next SELECT * FROM chat_assignment_context_packages;

DROP TABLE chat_assignment_context_packages;
ALTER TABLE chat_assignment_context_packages_next RENAME TO chat_assignment_context_packages;

-- chat_host_tool_returned_message_revisions
CREATE TABLE chat_host_tool_returned_message_revisions_next (
    invocation_id TEXT NOT NULL REFERENCES chat_host_tool_invocations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (invocation_id, message_revision_id),
    UNIQUE (invocation_id, ordinal)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_host_tool_returned_message_revisions_next
SELECT * FROM chat_host_tool_returned_message_revisions;

DROP TABLE chat_host_tool_returned_message_revisions;
ALTER TABLE chat_host_tool_returned_message_revisions_next RENAME TO chat_host_tool_returned_message_revisions;

-- chat_scratch_generation_sources
CREATE TABLE chat_scratch_generation_sources_next (
    scratch_generation_id TEXT NOT NULL REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    lower_ordinal INTEGER NOT NULL CHECK (lower_ordinal >= 1),
    high_ordinal INTEGER NOT NULL CHECK (high_ordinal >= lower_ordinal),
    audience_revision INTEGER NOT NULL CHECK (audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (scratch_generation_id, conversation_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_scratch_generation_sources_next SELECT * FROM chat_scratch_generation_sources;

DROP TABLE chat_scratch_generation_sources;
ALTER TABLE chat_scratch_generation_sources_next RENAME TO chat_scratch_generation_sources;

CREATE TRIGGER chat_scratch_generation_source_delete_denied
BEFORE DELETE ON chat_scratch_generation_sources
WHEN EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id)
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints are immutable');
END;

CREATE TRIGGER chat_scratch_generation_source_monotonic
BEFORE UPDATE ON chat_scratch_generation_sources
WHEN NEW.scratch_generation_id IS NOT OLD.scratch_generation_id
  OR NEW.conversation_id IS NOT OLD.conversation_id
  OR NEW.lower_ordinal > OLD.lower_ordinal
  OR NEW.high_ordinal < OLD.high_ordinal
  OR NEW.audience_revision < OLD.audience_revision
  OR NEW.created_at IS NOT OLD.created_at
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints can only expand');
END;

-- chat_scratch_scopes
CREATE TABLE chat_scratch_scopes_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    lifecycle_state TEXT NOT NULL DEFAULT 'active' CHECK (
        lifecycle_state IN ('active', 'archived', 'cleanup_pending', 'cleanup_failed', 'removed')
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    UNIQUE (reply_thread_id, teammate_id)
) STRICT;

INSERT INTO chat_scratch_scopes_next SELECT * FROM chat_scratch_scopes;

DROP TABLE chat_scratch_scopes;
ALTER TABLE chat_scratch_scopes_next RENAME TO chat_scratch_scopes;

CREATE TRIGGER chat_scratch_scope_identity_immutable
BEFORE UPDATE OF id, reply_thread_id, teammate_id, created_at ON chat_scratch_scopes
BEGIN
    SELECT RAISE(ABORT, 'Scratch scope identity is immutable');
END;

-- Teammate channel memberships end with a removal timestamp.
ALTER TABLE chat_teammate_channel_memberships
ADD COLUMN removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20);

-- Device-local claim state for scheduled messages.
CREATE TABLE chat_scheduled_message_dispatch (
    scheduled_message_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    claimed_at TEXT CHECK (claimed_at IS NULL OR length(claimed_at) >= 20),
    claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 1024),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    CHECK ((claimed_at IS NULL) = (claim_token IS NULL))
) STRICT;

CREATE INDEX idx_chat_scheduled_message_dispatch_ready
ON chat_scheduled_message_dispatch(claimed_at, available_at, scheduled_message_id);

-- Triggers that step aside while the sync engine applies remote operations.
DROP TRIGGER chat_reply_threads_root_item_insert;
CREATE TRIGGER chat_reply_threads_root_item_insert
BEFORE INSERT ON chat_reply_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1 FROM chat_conversation_items item
    WHERE item.id = NEW.root_item_id
      AND item.conversation_id = NEW.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.item_kind = 'message'
)
BEGIN
    SELECT RAISE(ABORT, 'Reply thread root must be a root communication message');
END;

DROP TRIGGER chat_communication_message_current_revision_update;
CREATE TRIGGER chat_communication_message_current_revision_update
BEFORE UPDATE OF current_revision_id ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM chat_communication_message_revisions revision
    WHERE revision.id = NEW.current_revision_id
      AND revision.message_item_id = NEW.item_id
)
BEGIN
    SELECT RAISE(ABORT, 'Current message revision belongs to another message');
END;

DROP TRIGGER chat_teammate_folder_grant_identity_insert;
CREATE TRIGGER chat_teammate_folder_grant_identity_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1
    FROM chat_teammates teammate
    JOIN chat_conversation_memberships membership
      ON membership.conversation_id = NEW.conversation_id
     AND membership.participant_id = NEW.teammate_id
     AND membership.removed_at IS NULL
    JOIN chat_teammate_channel_memberships ai_membership
      ON ai_membership.conversation_id = membership.conversation_id
     AND ai_membership.teammate_id = membership.participant_id
     AND ai_membership.removed_at IS NULL
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grants require active AI channel access');
END;

DROP TRIGGER chat_channels_default_name_insert;
CREATE TRIGGER chat_channels_default_name_insert
BEFORE INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.name != 'general'
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel must be named general');
END;

DROP TRIGGER chat_channels_default_protected_update;
CREATE TRIGGER chat_channels_default_protected_update
BEFORE UPDATE OF name, is_default, archived_at ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND OLD.is_default = 1 AND (
    NEW.name != OLD.name OR NEW.is_default != 1 OR NEW.archived_at IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel is protected');
END;

DROP TRIGGER chat_teammate_create_access_state;
CREATE TRIGGER chat_teammate_create_access_state
AFTER INSERT ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_teammate_access_state (teammate_id, updated_at)
    VALUES (NEW.participant_id, NEW.updated_at);
END;

DROP TRIGGER chat_conversation_create_audience_state;
CREATE TRIGGER chat_conversation_create_audience_state
AFTER INSERT ON chat_conversations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_conversation_audience_state (conversation_id, updated_at)
    VALUES (NEW.id, NEW.updated_at);
END;

DROP TRIGGER chat_access_profile_audience_publish;
CREATE TRIGGER chat_access_profile_audience_publish
AFTER INSERT ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.created_at
    WHERE conversation_id IN (
        SELECT membership.conversation_id
        FROM chat_teammate_channel_memberships membership
        WHERE membership.access_profile_id = NEW.access_profile_id
          AND membership.removed_at IS NULL
    );
END;

DROP TRIGGER chat_generic_membership_audience_insert;
CREATE TRIGGER chat_generic_membership_audience_insert
AFTER INSERT ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

DROP TRIGGER chat_generic_membership_audience_update;
CREATE TRIGGER chat_generic_membership_audience_update
AFTER UPDATE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

DROP TRIGGER chat_generic_membership_audience_delete;
CREATE TRIGGER chat_generic_membership_audience_delete
AFTER DELETE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

DROP TRIGGER chat_teammate_channel_membership_audience_insert;
CREATE TRIGGER chat_teammate_channel_membership_audience_insert
AFTER INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

DROP TRIGGER chat_teammate_channel_membership_audience_update;
CREATE TRIGGER chat_teammate_channel_membership_audience_update
AFTER UPDATE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

DROP TRIGGER chat_teammate_channel_membership_audience_delete;
CREATE TRIGGER chat_teammate_channel_membership_audience_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

DROP TRIGGER chat_communication_search_insert;
CREATE TRIGGER chat_communication_search_insert
AFTER UPDATE OF current_revision_id ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.current_revision_id IS NOT NULL
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = NEW.item_id;
    INSERT INTO chat_communication_search_fts (
        message_item_id, conversation_id, reply_thread_id,
        author_display_name, normalized_markdown
    )
    SELECT
        NEW.item_id,
        item.conversation_id,
        item.reply_thread_id,
        participant.display_name,
        revision.normalized_markdown
    FROM chat_conversation_items item
    JOIN chat_participants participant ON participant.id = NEW.author_participant_id
    JOIN chat_communication_message_revisions revision
      ON revision.id = NEW.current_revision_id
    WHERE item.id = NEW.item_id;
END;

DROP TRIGGER chat_communication_search_delete;
CREATE TRIGGER chat_communication_search_delete
AFTER DELETE ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = OLD.item_id;
END;

CREATE TEMP TABLE chat_local_tables_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_local_tables_guard (violations)
SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_local_tables_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

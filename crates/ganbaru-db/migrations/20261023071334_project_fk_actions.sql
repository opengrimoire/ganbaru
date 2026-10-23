-- no-transaction
-- Deleting a project, working folder, task section, or task status removes the rows that depend on it on every
-- replica, so a delete that arrives through sync cannot fail on one device and succeed on another. Chat rows that
-- reference a project or one of its working folders, the scratch promotion and cleanup rows reached through those
-- cascades, and tasks that reference a section or status cascade. Audit triggers that deny deletion keep
-- denying it while the parent row exists, so foreign key cascades can remove them with their parent.
--
-- SQLite applies a changed foreign key action only by rebuilding the table. Foreign keys are disabled for the
-- rebuild and every copy keeps its columns and values. Legacy rename keeps SQLite from reparsing triggers on
-- other tables while a rebuilt table is briefly absent. The rebuild commits only when the foreign key check reports
-- no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- project_tasks
CREATE TABLE project_tasks_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    section_id TEXT NOT NULL REFERENCES project_sections(id) ON DELETE CASCADE,
    status_id TEXT NOT NULL REFERENCES project_statuses(id) ON DELETE CASCADE,
    parent_task_id TEXT REFERENCES project_tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    priority TEXT NOT NULL DEFAULT 'normal' CHECK (trim(priority) <> ''),
    task_type TEXT NOT NULL DEFAULT 'task' CHECK (task_type IN ('task', 'milestone', 'bug', 'habit')),
    estimate_minutes INTEGER CHECK (estimate_minutes IS NULL OR estimate_minutes > 0),
    due_date TEXT,
    start_date TEXT,
    target_end_date TEXT,
    completed_at TEXT,
    archived_at TEXT,
    blocker_reason TEXT,
    milestone INTEGER NOT NULL DEFAULT 0 CHECK (milestone IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    start_time TEXT CHECK (
        start_time IS NULL OR (
            start_date IS NOT NULL
            AND length(start_time) = 5
            AND start_time GLOB '[0-9][0-9]:[0-9][0-9]'
            AND substr(start_time, 3, 1) = ':'
            AND substr(start_time, 1, 2) BETWEEN '00' AND '23'
            AND substr(start_time, 4, 2) BETWEEN '00' AND '59'
        )
    ),
    due_time TEXT CHECK (
        due_time IS NULL OR (
            due_date IS NOT NULL
            AND length(due_time) = 5
            AND due_time GLOB '[0-9][0-9]:[0-9][0-9]'
            AND substr(due_time, 3, 1) = ':'
            AND substr(due_time, 1, 2) BETWEEN '00' AND '23'
            AND substr(due_time, 4, 2) BETWEEN '00' AND '59'
        )
    ),
    revision INTEGER NOT NULL DEFAULT 0
        CHECK (revision >= 0 AND revision <= 9007199254740991), start_reminder_minutes INTEGER
        CHECK (start_reminder_minutes IS NULL OR (start_date IS NOT NULL AND start_reminder_minutes BETWEEN 0 AND 40320)), due_reminder_minutes INTEGER
        CHECK (due_reminder_minutes IS NULL OR (due_date IS NOT NULL AND due_reminder_minutes BETWEEN 0 AND 40320)), section_order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(section_order_key) <= 128), status_order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(status_order_key) <= 128),
    CHECK (parent_task_id IS NULL OR parent_task_id <> id),
    CHECK (start_date IS NULL OR target_end_date IS NULL OR start_date <= target_end_date)
);

INSERT INTO project_tasks_next (
    id,
    project_id,
    section_id,
    status_id,
    parent_task_id,
    title,
    description,
    priority,
    task_type,
    estimate_minutes,
    due_date,
    start_date,
    target_end_date,
    completed_at,
    archived_at,
    blocker_reason,
    milestone,
    created_at,
    updated_at,
    start_time,
    due_time,
    revision,
    start_reminder_minutes,
    due_reminder_minutes,
    section_order_key,
    status_order_key
)
SELECT
    id,
    project_id,
    section_id,
    status_id,
    parent_task_id,
    title,
    description,
    priority,
    task_type,
    estimate_minutes,
    due_date,
    start_date,
    target_end_date,
    completed_at,
    archived_at,
    blocker_reason,
    milestone,
    created_at,
    updated_at,
    start_time,
    due_time,
    revision,
    start_reminder_minutes,
    due_reminder_minutes,
    section_order_key,
    status_order_key
FROM project_tasks;

DROP TABLE project_tasks;
ALTER TABLE project_tasks_next RENAME TO project_tasks;

CREATE INDEX idx_project_tasks_archived ON project_tasks(project_id, archived_at);
CREATE INDEX idx_project_tasks_due ON project_tasks(project_id, due_date);
CREATE INDEX idx_project_tasks_parent ON project_tasks(parent_task_id, section_order_key, id);
CREATE INDEX idx_project_tasks_project_section
    ON project_tasks(project_id, section_id, section_order_key, id);
CREATE INDEX idx_project_tasks_project_status ON project_tasks(project_id, status_id, status_order_key, id);

CREATE TRIGGER project_tasks_order_key_insert
BEFORE INSERT ON project_tasks
WHEN NEW.section_order_key = '' OR NEW.status_order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task order keys are required');
END;

CREATE TRIGGER project_tasks_order_key_update
BEFORE UPDATE OF section_order_key, status_order_key ON project_tasks
WHEN NEW.section_order_key = '' OR NEW.status_order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task order keys are required');
END;

-- chat_threads
CREATE TABLE chat_threads_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT,
    project_id TEXT NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 1000),
    title_search TEXT GENERATED ALWAYS AS (lower(trim(title))) STORED,
    title_source TEXT NOT NULL DEFAULT 'user' CHECK (title_source IN ('user', 'provider')),
    provider_family_id TEXT NOT NULL CHECK (length(provider_family_id) BETWEEN 1 AND 1024),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    continuation_group_id TEXT NOT NULL CHECK (length(continuation_group_id) BETWEEN 1 AND 1024),
    provider_thread_id TEXT CHECK (
        provider_thread_id IS NULL OR length(provider_thread_id) BETWEEN 1 AND 1024
    ),
    resume_cursor_schema_version INTEGER CHECK (
        resume_cursor_schema_version IS NULL OR resume_cursor_schema_version >= 1
    ),
    resume_cursor_data TEXT CHECK (resume_cursor_data IS NULL OR json_valid(resume_cursor_data)),
    model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        model_selection_schema_version >= 1
    ),
    model_selection_data TEXT NOT NULL DEFAULT '{"modelId":null,"options":[]}' CHECK (
        json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
    ),
    safety_mode TEXT NOT NULL CHECK (
        safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
    ),
    interaction_mode TEXT NOT NULL CHECK (interaction_mode IN ('build', 'plan')),
    state TEXT NOT NULL CHECK (
        state IN ('draft', 'active', 'waiting', 'idle', 'error', 'archived', 'closed')
    ),
    latest_turn_state TEXT CHECK (
        latest_turn_state IS NULL OR latest_turn_state IN (
            'pending', 'dispatching', 'active', 'waiting_for_approval',
            'waiting_for_user_input', 'completed', 'interrupted', 'failed'
        )
    ),
    latest_preview TEXT CHECK (latest_preview IS NULL OR length(latest_preview) <= 2000),
    changed_file_summary_schema_version INTEGER CHECK (
        changed_file_summary_schema_version IS NULL OR changed_file_summary_schema_version >= 1
    ),
    changed_file_summary_data TEXT CHECK (
        changed_file_summary_data IS NULL OR (
            json_valid(changed_file_summary_data)
            AND json_type(changed_file_summary_data) = 'object'
        )
    ),
    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    last_event_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_event_sequence >= 0),
    last_projected_sequence INTEGER NOT NULL DEFAULT 0 CHECK (
        last_projected_sequence >= 0 AND last_projected_sequence <= last_event_sequence
    ),
    read_revision INTEGER NOT NULL DEFAULT 0 CHECK (read_revision >= 0),
    unread_at TEXT CHECK (unread_at IS NULL OR length(unread_at) >= 20),
    last_activity_at TEXT NOT NULL CHECK (length(last_activity_at) >= 20),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    CHECK (
        (working_folder_id IS NULL AND execution_environment_id IS NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NOT NULL AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NOT NULL)
    ),
    CHECK (
        (resume_cursor_schema_version IS NULL AND resume_cursor_data IS NULL)
        OR (resume_cursor_schema_version IS NOT NULL AND resume_cursor_data IS NOT NULL)
    )
) STRICT;

INSERT INTO chat_threads_next (
    id,
    working_folder_id,
    project_id,
    execution_environment_id,
    scratch_generation_id,
    title,
    title_source,
    provider_family_id,
    provider_instance_id,
    continuation_group_id,
    provider_thread_id,
    resume_cursor_schema_version,
    resume_cursor_data,
    model_selection_schema_version,
    model_selection_data,
    safety_mode,
    interaction_mode,
    state,
    latest_turn_state,
    latest_preview,
    changed_file_summary_schema_version,
    changed_file_summary_data,
    message_count,
    revision,
    last_event_sequence,
    last_projected_sequence,
    read_revision,
    unread_at,
    last_activity_at,
    archived_at,
    created_at,
    updated_at
)
SELECT
    id,
    working_folder_id,
    project_id,
    execution_environment_id,
    scratch_generation_id,
    title,
    title_source,
    provider_family_id,
    provider_instance_id,
    continuation_group_id,
    provider_thread_id,
    resume_cursor_schema_version,
    resume_cursor_data,
    model_selection_schema_version,
    model_selection_data,
    safety_mode,
    interaction_mode,
    state,
    latest_turn_state,
    latest_preview,
    changed_file_summary_schema_version,
    changed_file_summary_data,
    message_count,
    revision,
    last_event_sequence,
    last_projected_sequence,
    read_revision,
    unread_at,
    last_activity_at,
    archived_at,
    created_at,
    updated_at
FROM chat_threads;

DROP TABLE chat_threads;
ALTER TABLE chat_threads_next RENAME TO chat_threads;

CREATE INDEX idx_chat_threads_active_project
ON chat_threads(project_id, last_activity_at DESC, id)
WHERE archived_at IS NULL AND state != 'closed';
CREATE INDEX idx_chat_threads_active_working_folder
ON chat_threads(working_folder_id, last_activity_at DESC, id)
WHERE working_folder_id IS NOT NULL AND archived_at IS NULL AND state != 'closed';
CREATE INDEX idx_chat_threads_archived
ON chat_threads(archived_at DESC, id)
WHERE archived_at IS NOT NULL;
CREATE INDEX idx_chat_threads_execution_environment
ON chat_threads(execution_environment_id, last_activity_at DESC, id)
WHERE execution_environment_id IS NOT NULL;
CREATE INDEX idx_chat_threads_scratch_generation
ON chat_threads(scratch_generation_id, last_activity_at DESC, id)
WHERE scratch_generation_id IS NOT NULL;
CREATE INDEX idx_chat_threads_title_search
ON chat_threads(title_search, last_activity_at DESC, id);

CREATE TRIGGER chat_threads_assign_current_environment
AFTER INSERT ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NEW.execution_environment_id IS NULL
BEGIN
    UPDATE chat_threads
    SET execution_environment_id = 'current-folder:' || NEW.working_folder_id
    WHERE id = NEW.id;
END;

CREATE TRIGGER chat_threads_execution_target_insert
BEFORE INSERT ON chat_threads
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR (NEW.working_folder_id IS NOT NULL
        AND NEW.execution_environment_id IS NULL
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
    SELECT RAISE(ABORT, 'Chat thread execution target is inconsistent');
END;

CREATE TRIGGER chat_threads_execution_target_update
BEFORE UPDATE OF working_folder_id, execution_environment_id, scratch_generation_id ON chat_threads
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
    SELECT RAISE(ABORT, 'Chat thread execution target is inconsistent');
END;

CREATE TRIGGER chat_threads_project_matches_working_folder_insert
BEFORE INSERT ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM project_working_folders
    WHERE id = NEW.working_folder_id AND project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread project must match its working folder');
END;

CREATE TRIGGER chat_threads_project_matches_working_folder_update
BEFORE UPDATE OF working_folder_id, project_id ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM project_working_folders
    WHERE id = NEW.working_folder_id AND project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread project must match its working folder');
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
        ON UPDATE CASCADE ON DELETE NO ACTION,
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
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

INSERT INTO chat_agent_runs_next (
    id,
    assignment_id,
    project_id,
    working_folder_id,
    execution_environment_id,
    scratch_generation_id,
    teammate_policy_revision_id,
    authorization_revision_id,
    authorization_scope_digest,
    provider_turn_id,
    provider_thread_id,
    state,
    run_ordinal,
    started_at,
    settled_at,
    created_at,
    updated_at
)
SELECT
    id,
    assignment_id,
    project_id,
    working_folder_id,
    execution_environment_id,
    scratch_generation_id,
    teammate_policy_revision_id,
    authorization_revision_id,
    authorization_scope_digest,
    provider_turn_id,
    provider_thread_id,
    state,
    run_ordinal,
    started_at,
    settled_at,
    created_at,
    updated_at
FROM chat_agent_runs;

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

-- chat_assignment_authorization_revisions
CREATE TABLE chat_assignment_authorization_revisions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    requester_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_policy_revision_id TEXT NOT NULL REFERENCES chat_teammate_policy_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_access_revision INTEGER NOT NULL CHECK (teammate_access_revision >= 1),
    destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    access_profile_revision_id TEXT NOT NULL REFERENCES chat_access_profile_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
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

INSERT INTO chat_assignment_authorization_revisions_next (
    id,
    assignment_id,
    revision,
    requester_participant_id,
    teammate_policy_revision_id,
    teammate_access_revision,
    destination_conversation_id,
    access_profile_revision_id,
    execution_environment_id,
    working_folder_id,
    resolved_runtime_approval_policy,
    scope_digest,
    decision_state,
    reason,
    created_at,
    revoked_at
)
SELECT
    id,
    assignment_id,
    revision,
    requester_participant_id,
    teammate_policy_revision_id,
    teammate_access_revision,
    destination_conversation_id,
    access_profile_revision_id,
    execution_environment_id,
    working_folder_id,
    resolved_runtime_approval_policy,
    scope_digest,
    decision_state,
    reason,
    created_at,
    revoked_at
FROM chat_assignment_authorization_revisions;

DROP TABLE chat_assignment_authorization_revisions;
ALTER TABLE chat_assignment_authorization_revisions_next RENAME TO chat_assignment_authorization_revisions;

CREATE INDEX idx_chat_assignment_authorization_revisions_active
ON chat_assignment_authorization_revisions(assignment_id, decision_state, revision DESC);

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
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

-- chat_assignment_authorized_folder_sources
CREATE TABLE chat_assignment_authorized_folder_sources_next (
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    root_handle TEXT NOT NULL UNIQUE CHECK (length(root_handle) BETWEEN 32 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    capability TEXT NOT NULL CHECK (
        capability IN ('read', 'edit', 'execute', 'publish')
    ),
    is_execution_target INTEGER NOT NULL DEFAULT 0 CHECK (is_execution_target IN (0, 1)),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    resolved_runtime_approval_policy TEXT CHECK (
        resolved_runtime_approval_policy IS NULL OR resolved_runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    PRIMARY KEY (authorization_revision_id, working_folder_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_assignment_authorized_folder_sources_next (
    authorization_revision_id,
    root_handle,
    working_folder_id,
    capability,
    is_execution_target,
    created_at,
    resolved_runtime_approval_policy
)
SELECT
    authorization_revision_id,
    root_handle,
    working_folder_id,
    capability,
    is_execution_target,
    created_at,
    resolved_runtime_approval_policy
FROM chat_assignment_authorized_folder_sources;

DROP TABLE chat_assignment_authorized_folder_sources;
ALTER TABLE chat_assignment_authorized_folder_sources_next RENAME TO chat_assignment_authorized_folder_sources;

CREATE UNIQUE INDEX idx_chat_assignment_authorized_folder_sources_execution_target
ON chat_assignment_authorized_folder_sources(authorization_revision_id)
WHERE is_execution_target = 1;

CREATE TRIGGER chat_authorized_folder_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_folder_sources
WHEN EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions
    WHERE id = OLD.authorization_revision_id
)
 AND EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id)
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources are immutable');
END;

CREATE TRIGGER chat_authorized_folder_source_immutable_update
BEFORE UPDATE ON chat_assignment_authorized_folder_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources are immutable');
END;

CREATE TRIGGER chat_authorized_folder_source_requires_approval
BEFORE INSERT ON chat_assignment_authorized_folder_sources
WHEN NEW.resolved_runtime_approval_policy IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources require a frozen approval policy');
END;

-- chat_attachments
CREATE TABLE chat_attachments_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'text_snippet')),
    original_display_name TEXT NOT NULL CHECK (length(trim(original_display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 52428800),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL UNIQUE CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/..'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    signature_kind TEXT NOT NULL CHECK (length(signature_kind) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deletion_state TEXT NOT NULL DEFAULT 'active' CHECK (
        deletion_state IN ('active', 'pending_delete', 'deleted', 'cleanup_failed')
    ),
    unreferenced_at TEXT CHECK (unreferenced_at IS NULL OR length(unreferenced_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;

INSERT INTO chat_attachments_next (
    id,
    working_folder_id,
    kind,
    original_display_name,
    mime_type,
    byte_size,
    sha256,
    managed_relative_path,
    signature_kind,
    created_at,
    deletion_state,
    unreferenced_at,
    deleted_at
)
SELECT
    id,
    working_folder_id,
    kind,
    original_display_name,
    mime_type,
    byte_size,
    sha256,
    managed_relative_path,
    signature_kind,
    created_at,
    deletion_state,
    unreferenced_at,
    deleted_at
FROM chat_attachments;

DROP TABLE chat_attachments;
ALTER TABLE chat_attachments_next RENAME TO chat_attachments;

CREATE INDEX idx_chat_attachments_cleanup
ON chat_attachments(deletion_state, unreferenced_at, id);
CREATE INDEX idx_chat_attachments_hash
ON chat_attachments(sha256, byte_size, id);

CREATE TRIGGER chat_attachments_create_resource
AFTER INSERT ON chat_attachments
BEGIN
    INSERT INTO chat_resources (
        id,
        working_folder_id,
        attachment_id,
        resource_kind,
        display_name,
        mime_type,
        byte_size,
        sha256,
        managed_relative_path,
        resource_uri,
        integrity_state,
        created_at
    ) VALUES (
        NEW.id,
        NEW.working_folder_id,
        NEW.id,
        NEW.kind,
        NEW.original_display_name,
        NEW.mime_type,
        NEW.byte_size,
        NEW.sha256,
        NEW.managed_relative_path,
        'ganbaru://chat/resource/' || NEW.id,
        'verified',
        NEW.created_at
    );
END;

CREATE TRIGGER chat_attachments_managed_relative_path_shape_insert
BEFORE INSERT ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_attachments_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_attachments_update_resource_integrity
AFTER UPDATE OF deletion_state, deleted_at ON chat_attachments
BEGIN
    UPDATE chat_resources
    SET integrity_state = CASE NEW.deletion_state
            WHEN 'deleted' THEN 'deleted'
            ELSE integrity_state
        END,
        deleted_at = NEW.deleted_at
    WHERE attachment_id = NEW.id;
END;

-- chat_cleanup_jobs
CREATE TABLE chat_cleanup_jobs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    source_thread_id TEXT CHECK (source_thread_id IS NULL OR length(source_thread_id) BETWEEN 1 AND 1024),
    cleanup_kind TEXT NOT NULL CHECK (
        cleanup_kind IN ('checkpoint_ref', 'attachment_file', 'diagnostic_event')
    ),
    exact_target TEXT NOT NULL CHECK (length(exact_target) BETWEEN 1 AND 4096),
    repository_identity TEXT CHECK (
        repository_identity IS NULL OR length(repository_identity) BETWEEN 1 AND 1024
    ),
    expected_object_id TEXT CHECK (
        expected_object_id IS NULL OR length(expected_object_id) BETWEEN 40 AND 128
    ),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'running', 'failed', 'completed')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    UNIQUE (cleanup_kind, exact_target)
) STRICT;

INSERT INTO chat_cleanup_jobs_next (
    id,
    working_folder_id,
    source_thread_id,
    cleanup_kind,
    exact_target,
    repository_identity,
    expected_object_id,
    state,
    attempt_count,
    available_at,
    last_error_code,
    created_at,
    updated_at
)
SELECT
    id,
    working_folder_id,
    source_thread_id,
    cleanup_kind,
    exact_target,
    repository_identity,
    expected_object_id,
    state,
    attempt_count,
    available_at,
    last_error_code,
    created_at,
    updated_at
FROM chat_cleanup_jobs;

DROP TABLE chat_cleanup_jobs;
ALTER TABLE chat_cleanup_jobs_next RENAME TO chat_cleanup_jobs;

CREATE INDEX idx_chat_cleanup_jobs_due
ON chat_cleanup_jobs(state, available_at, id);

-- chat_resources
CREATE TABLE chat_resources_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    attachment_id TEXT UNIQUE REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('image', 'text_snippet', 'browser_screenshot', 'browser_recording')),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 1073741824),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL UNIQUE CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    resource_uri TEXT NOT NULL UNIQUE CHECK (resource_uri GLOB 'ganbaru://chat/resource/*'),
    integrity_state TEXT NOT NULL DEFAULT 'verified' CHECK (
        integrity_state IN ('verified', 'missing', 'hash_mismatch', 'unsafe_path', 'deleted')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;

INSERT INTO chat_resources_next (
    id,
    working_folder_id,
    attachment_id,
    resource_kind,
    display_name,
    mime_type,
    byte_size,
    sha256,
    managed_relative_path,
    resource_uri,
    integrity_state,
    created_at,
    deleted_at
)
SELECT
    id,
    working_folder_id,
    attachment_id,
    resource_kind,
    display_name,
    mime_type,
    byte_size,
    sha256,
    managed_relative_path,
    resource_uri,
    integrity_state,
    created_at,
    deleted_at
FROM chat_resources;

DROP TABLE chat_resources;
ALTER TABLE chat_resources_next RENAME TO chat_resources;

CREATE INDEX idx_chat_resources_workspace_kind
ON chat_resources(working_folder_id, resource_kind, created_at DESC, id);

CREATE TRIGGER chat_resources_managed_relative_path_shape_insert
BEFORE INSERT ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_resources_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

-- chat_scheduled_message_references
CREATE TABLE chat_scheduled_message_references_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reference_kind TEXT NOT NULL CHECK (
        reference_kind IN (
            'participant', 'channel', 'working_folder',
            'workspace_path', 'execution_environment'
        )
    ),
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    plain_text_projection TEXT NOT NULL CHECK (length(plain_text_projection) BETWEEN 1 AND 4096),
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    participant_id TEXT REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    participant_kind TEXT CHECK (
        participant_kind IS NULL OR participant_kind IN ('local_user', 'ai_teammate', 'human')
    ),
    channel_id TEXT REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    path_kind TEXT CHECK (path_kind IS NULL OR path_kind IN ('file', 'folder')),
    relative_path TEXT CHECK (
        relative_path IS NULL OR (
            length(relative_path) BETWEEN 1 AND 4096
            AND relative_path NOT LIKE '/%'
            AND relative_path NOT LIKE '../%'
            AND relative_path NOT LIKE '%/../%'
            AND relative_path NOT LIKE '%/..'
            AND relative_path NOT LIKE '%\%'
        )
    ),
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    UNIQUE (scheduled_message_id, start_offset, end_offset),
    UNIQUE (scheduled_message_id, ordinal),
    CHECK (
        (reference_kind = 'participant' AND participant_id IS NOT NULL
            AND participant_kind IS NOT NULL AND channel_id IS NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'channel' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NOT NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'working_folder' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NOT NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'workspace_path' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NOT NULL AND path_kind IS NOT NULL
            AND relative_path IS NOT NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'execution_environment' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NOT NULL)
    )
) STRICT;

INSERT INTO chat_scheduled_message_references_next (
    id,
    scheduled_message_id,
    reference_kind,
    label_snapshot,
    plain_text_projection,
    start_offset,
    end_offset,
    participant_id,
    participant_kind,
    channel_id,
    working_folder_id,
    path_kind,
    relative_path,
    execution_environment_id,
    ordinal
)
SELECT
    id,
    scheduled_message_id,
    reference_kind,
    label_snapshot,
    plain_text_projection,
    start_offset,
    end_offset,
    participant_id,
    participant_kind,
    channel_id,
    working_folder_id,
    path_kind,
    relative_path,
    execution_environment_id,
    ordinal
FROM chat_scheduled_message_references;

DROP TABLE chat_scheduled_message_references;
ALTER TABLE chat_scheduled_message_references_next RENAME TO chat_scheduled_message_references;

CREATE INDEX idx_chat_scheduled_message_references_target
ON chat_scheduled_message_references(scheduled_message_id, reference_kind, ordinal);

-- chat_scratch_promotions
CREATE TABLE chat_scratch_promotions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scratch_generation_id TEXT NOT NULL REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_relative_path TEXT NOT NULL CHECK (
        length(source_relative_path) BETWEEN 1 AND 4096
        AND source_relative_path NOT LIKE '/%'
        AND source_relative_path NOT LIKE '../%'
        AND source_relative_path NOT LIKE '%/../%'
        AND source_relative_path NOT LIKE '%/..'
        AND source_relative_path NOT LIKE '%\%'
    ),
    source_content_revision TEXT NOT NULL CHECK (length(source_content_revision) = 64),
    source_sha256 TEXT CHECK (source_sha256 IS NULL OR length(source_sha256) = 64),
    request_digest TEXT CHECK (
        request_digest IS NULL OR (
            length(request_digest) = 64
            AND request_digest NOT GLOB '*[^0-9a-f]*'
        )
    ),
    destination_kind TEXT NOT NULL CHECK (
        destination_kind IN ('working_folder', 'managed_attachment')
    ),
    destination_channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    destination_working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    destination_relative_path TEXT CHECK (
        destination_relative_path IS NULL OR (
            length(destination_relative_path) BETWEEN 1 AND 4096
            AND destination_relative_path NOT LIKE '/%'
            AND destination_relative_path NOT LIKE '../%'
            AND destination_relative_path NOT LIKE '%/../%'
            AND destination_relative_path NOT LIKE '%/..'
            AND destination_relative_path NOT LIKE '%\%'
        )
    ),
    attachment_id TEXT CHECK (
        attachment_id IS NULL OR length(attachment_id) BETWEEN 1 AND 1024
    ),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'completed', 'failed')
    ),
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20),
    CHECK (
        (destination_kind = 'working_folder'
            AND destination_working_folder_id IS NOT NULL
            AND destination_relative_path IS NOT NULL
            AND attachment_id IS NULL)
        OR (destination_kind = 'managed_attachment'
            AND destination_working_folder_id IS NULL
            AND destination_relative_path IS NULL
            AND attachment_id IS NOT NULL)
    )
) STRICT;

INSERT INTO chat_scratch_promotions_next (
    id,
    scratch_generation_id,
    source_relative_path,
    source_content_revision,
    source_sha256,
    request_digest,
    destination_kind,
    destination_channel_id,
    destination_working_folder_id,
    destination_relative_path,
    attachment_id,
    state,
    last_error_code,
    created_at,
    updated_at,
    completed_at
)
SELECT
    id,
    scratch_generation_id,
    source_relative_path,
    source_content_revision,
    source_sha256,
    request_digest,
    destination_kind,
    destination_channel_id,
    destination_working_folder_id,
    destination_relative_path,
    attachment_id,
    state,
    last_error_code,
    created_at,
    updated_at,
    completed_at
FROM chat_scratch_promotions;

DROP TABLE chat_scratch_promotions;
ALTER TABLE chat_scratch_promotions_next RENAME TO chat_scratch_promotions;

CREATE INDEX idx_chat_scratch_promotions_destination
ON chat_scratch_promotions(destination_channel_id, created_at DESC, id);
CREATE INDEX idx_chat_scratch_promotions_generation
ON chat_scratch_promotions(scratch_generation_id, created_at DESC, id);

CREATE TRIGGER chat_scratch_attachment_promotion_completed
BEFORE UPDATE OF state ON chat_scratch_promotions
WHEN NEW.state = 'completed'
 AND NEW.destination_kind = 'managed_attachment'
 AND NOT EXISTS (
    SELECT 1
    FROM chat_attachments attachment
    JOIN project_working_folders storage_folder
      ON storage_folder.id = attachment.working_folder_id
     AND storage_folder.kind = 'managed'
     AND storage_folder.archived_at IS NULL
    JOIN chat_channels channel
      ON channel.id = NEW.destination_channel_id
     AND channel.project_id = storage_folder.project_id
    WHERE attachment.id = NEW.attachment_id
 )
BEGIN
    SELECT RAISE(ABORT, 'Completed scratch attachment promotion requires its channel attachment');
END;

CREATE TRIGGER chat_scratch_promotion_audit_delete_denied
BEFORE DELETE ON chat_scratch_promotions
WHEN EXISTS (SELECT 1 FROM chat_channels WHERE id = OLD.destination_channel_id)
 AND EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
 AND (OLD.destination_working_folder_id IS NULL
      OR EXISTS (
          SELECT 1 FROM project_working_folders
          WHERE id = OLD.destination_working_folder_id
      ))
BEGIN
    SELECT RAISE(ABORT, 'Scratch promotion audit rows cannot be deleted');
END;

CREATE TRIGGER chat_scratch_promotion_authority_immutable
BEFORE UPDATE OF scratch_generation_id, source_relative_path, source_content_revision,
    request_digest, destination_kind, destination_channel_id,
    destination_working_folder_id, destination_relative_path, attachment_id, created_at
ON chat_scratch_promotions
BEGIN
    SELECT RAISE(ABORT, 'Scratch promotion authority is immutable');
END;

CREATE TRIGGER chat_scratch_promotion_completion_provenance
BEFORE UPDATE OF state ON chat_scratch_promotions
WHEN NEW.state = 'completed' AND NEW.source_sha256 IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Completed scratch promotion requires source provenance');
END;

CREATE TRIGGER chat_scratch_promotion_request_digest_required
BEFORE INSERT ON chat_scratch_promotions
WHEN NEW.request_digest IS NULL
BEGIN
    SELECT RAISE(ABORT, 'New scratch promotions require an idempotency digest');
END;

CREATE TRIGGER chat_scratch_promotion_terminal_immutable
BEFORE UPDATE ON chat_scratch_promotions
WHEN OLD.state != 'pending'
BEGIN
    SELECT RAISE(ABORT, 'Terminal scratch promotion is immutable');
END;

-- chat_scratch_cleanup_jobs
CREATE TABLE chat_scratch_cleanup_jobs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scratch_scope_id TEXT NOT NULL REFERENCES chat_scratch_scopes(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    scratch_generation_id TEXT NOT NULL UNIQUE REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    expected_scope_revision INTEGER NOT NULL CHECK (expected_scope_revision >= 1),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'running', 'completed', 'failed', 'unavailable_on_device')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    removed_bytes INTEGER NOT NULL DEFAULT 0 CHECK (removed_bytes >= 0),
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128
    ),
    confirmed_at TEXT NOT NULL CHECK (length(confirmed_at) >= 20),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20)
) STRICT;

INSERT INTO chat_scratch_cleanup_jobs_next (
    id,
    scratch_scope_id,
    scratch_generation_id,
    expected_scope_revision,
    state,
    attempt_count,
    removed_bytes,
    last_error_code,
    confirmed_at,
    available_at,
    created_at,
    updated_at,
    completed_at
)
SELECT
    id,
    scratch_scope_id,
    scratch_generation_id,
    expected_scope_revision,
    state,
    attempt_count,
    removed_bytes,
    last_error_code,
    confirmed_at,
    available_at,
    created_at,
    updated_at,
    completed_at
FROM chat_scratch_cleanup_jobs;

DROP TABLE chat_scratch_cleanup_jobs;
ALTER TABLE chat_scratch_cleanup_jobs_next RENAME TO chat_scratch_cleanup_jobs;

CREATE INDEX idx_chat_scratch_cleanup_jobs_ready
ON chat_scratch_cleanup_jobs(state, available_at, created_at, id);

-- chat_teammate_working_folder_grants
CREATE TABLE chat_teammate_working_folder_grants_next (
    conversation_id TEXT NOT NULL,
    teammate_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    working_folder_id TEXT NOT NULL,
    capability TEXT NOT NULL CHECK (
        capability IN ('none', 'read', 'edit', 'execute', 'publish')
    ),
    capability_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        capability_inherits_profile IN (0, 1)
    ),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    runtime_approval_policy TEXT CHECK (
        runtime_approval_policy IS NULL OR runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
    PRIMARY KEY (conversation_id, teammate_id, working_folder_id),
    FOREIGN KEY (conversation_id, teammate_id)
        REFERENCES chat_conversation_memberships(conversation_id, participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (conversation_id, project_id)
        REFERENCES chat_conversations(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

INSERT INTO chat_teammate_working_folder_grants_next (
    conversation_id,
    teammate_id,
    project_id,
    working_folder_id,
    capability,
    capability_inherits_profile,
    is_default,
    runtime_approval_policy,
    revision,
    created_at,
    revoked_at
)
SELECT
    conversation_id,
    teammate_id,
    project_id,
    working_folder_id,
    capability,
    capability_inherits_profile,
    is_default,
    runtime_approval_policy,
    revision,
    created_at,
    revoked_at
FROM chat_teammate_working_folder_grants;

DROP TABLE chat_teammate_working_folder_grants;
ALTER TABLE chat_teammate_working_folder_grants_next RENAME TO chat_teammate_working_folder_grants;

CREATE UNIQUE INDEX idx_chat_teammate_working_folder_grants_default
ON chat_teammate_working_folder_grants(conversation_id, teammate_id)
WHERE is_default = 1 AND revoked_at IS NULL;

CREATE TRIGGER chat_teammate_folder_grant_identity_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_teammates teammate
    JOIN chat_conversation_memberships membership
      ON membership.conversation_id = NEW.conversation_id
     AND membership.participant_id = NEW.teammate_id
     AND membership.removed_at IS NULL
    JOIN chat_teammate_channel_memberships ai_membership
      ON ai_membership.conversation_id = membership.conversation_id
     AND ai_membership.teammate_id = membership.participant_id
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grants require active AI channel access');
END;

-- chat_working_folder_reference_targets
CREATE TABLE chat_working_folder_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;

INSERT INTO chat_working_folder_reference_targets_next (
    reference_id,
    working_folder_id
)
SELECT
    reference_id,
    working_folder_id
FROM chat_working_folder_reference_targets;

DROP TABLE chat_working_folder_reference_targets;
ALTER TABLE chat_working_folder_reference_targets_next RENAME TO chat_working_folder_reference_targets;


-- chat_workspace_path_reference_targets
CREATE TABLE chat_workspace_path_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    path_kind TEXT NOT NULL CHECK (path_kind IN ('file', 'folder')),
    relative_path TEXT NOT NULL CHECK (
        length(relative_path) BETWEEN 1 AND 4096
        AND relative_path NOT LIKE '/%'
        AND relative_path NOT LIKE '../%'
        AND relative_path NOT LIKE '%/../%'
        AND relative_path NOT LIKE '%/..'
        AND relative_path NOT LIKE '%\%'
    )
) STRICT;

INSERT INTO chat_workspace_path_reference_targets_next (
    reference_id,
    working_folder_id,
    path_kind,
    relative_path
)
SELECT
    reference_id,
    working_folder_id,
    path_kind,
    relative_path
FROM chat_workspace_path_reference_targets;

DROP TABLE chat_workspace_path_reference_targets;
ALTER TABLE chat_workspace_path_reference_targets_next RENAME TO chat_workspace_path_reference_targets;


-- Delete guards on tables whose foreign key actions already cascade

DROP TRIGGER chat_authorized_channel_source_immutable_delete;

CREATE TRIGGER chat_authorized_channel_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_channel_sources
WHEN EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions
    WHERE id = OLD.authorization_revision_id
)
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

DROP TRIGGER chat_scratch_generation_source_delete_denied;

CREATE TRIGGER chat_scratch_generation_source_delete_denied
BEFORE DELETE ON chat_scratch_generation_sources
WHEN EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints are immutable');
END;

CREATE TEMP TABLE project_fk_actions_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO project_fk_actions_guard (violations)
SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE project_fk_actions_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

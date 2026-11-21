-- no-transaction
-- Chat configures the channel instead of the agent. One access level (talk, read, ask, or own)
-- and instructions are set for the vault, a project, and a channel, each lower scope inheriting
-- when it leaves the level null: the vault settings singleton (sync id 357), the project agent
-- settings (sync id 358), and two new channel columns. An agent is a provider: it keeps its
-- provider family and an optional default model revision, and loses its role, instructions,
-- safety mode copy, and home device. A channel membership keeps only presence and an optional
-- model override, working folder grants (sync id 363) and the agent access counter go away, and
-- push approvals name their channel. Work assignments freeze the model and instructions they
-- start with and carry the model label that ran.
--
-- The rebuilt replicated tables drop their sync bookkeeping and seeded marks, so the next vault
-- open republishes their rows in the new shape. Rebuilding tables that other tables cascade from
-- would drop their rows, so foreign keys are off for this connection and the whole change
-- commits as one transaction.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP VIEW chat_agent_effective_policies;
DROP VIEW chat_agent_policy_revisions_numbered;

DROP TRIGGER chat_agent_access_revision_monotonic;
DROP TRIGGER chat_agent_create_access_state;
DROP TRIGGER chat_agent_push_approvals_safety_mode;
DROP TRIGGER chat_agent_policy_identity_insert;
DROP TRIGGER chat_agent_policy_immutable_delete;
DROP TRIGGER chat_agent_policy_immutable_update;
DROP TRIGGER chat_agent_channel_membership_audience_delete;
DROP TRIGGER chat_agent_channel_membership_audience_insert;
DROP TRIGGER chat_agent_channel_membership_audience_update;
DROP TRIGGER chat_agent_channel_membership_identity_insert;
DROP TRIGGER chat_agent_channel_membership_push_approvals_delete;
DROP TRIGGER chat_agent_channel_membership_push_approvals_update;
DROP TRIGGER chat_agent_channel_membership_requires_membership_insert;
DROP TRIGGER chat_agent_folder_grant_identity_insert;
DROP TRIGGER chat_agent_folder_grant_project_insert;
DROP TRIGGER chat_agent_folder_grant_project_update;
DROP TRIGGER chat_agent_folder_grant_push_approvals_delete;
DROP TRIGGER chat_agent_folder_grant_push_approvals_update;
DROP TRIGGER chat_agent_folder_grant_single_default_insert;
DROP TRIGGER chat_agent_folder_grant_single_default_update;
DROP TRIGGER chat_push_approvals_approver_is_person_insert;
DROP TRIGGER chat_push_approvals_approver_is_person_update;
DROP TRIGGER chat_participant_push_approvals_archive;
DROP TRIGGER chat_work_assignment_delegation_depth_insert;
DROP TRIGGER chat_work_assignment_delegation_insert;
DROP TRIGGER chat_assignment_authorization_delete_denied;
DROP TRIGGER chat_assignment_authorization_id_immutable;
DROP TRIGGER chat_assignment_authorization_reason_immutable;
DROP TRIGGER chat_assignment_authorization_revocation_insert;
DROP TRIGGER chat_assignment_authorization_revocation_update;
DROP TRIGGER chat_assignment_authorization_scope_immutable;
DROP TRIGGER chat_assignment_authorization_state_transition;
DROP TRIGGER chat_assignment_authorization_target_required_insert;
DROP TRIGGER chat_assignment_authorization_terminal_immutable;
DROP TRIGGER chat_agent_run_attempt_immutable;
DROP TRIGGER chat_agent_run_authority_immutable;
DROP TRIGGER chat_agent_run_authorization_insert;
DROP TRIGGER chat_agent_run_authorization_target_insert;
DROP TRIGGER chat_agent_run_execution_target_insert;
DROP TRIGGER chat_agent_run_id_immutable;
DROP TRIGGER chat_agent_run_usage_working_insert;
DROP TRIGGER chat_agent_run_usage_working_update;

DROP TRIGGER sync_capture_chat_agent_channel_memberships_delete;
DROP TRIGGER sync_capture_chat_agent_channel_memberships_insert;
DROP TRIGGER sync_capture_chat_agent_channel_memberships_update;
DROP TRIGGER sync_capture_chat_agent_policy_revisions_delete;
DROP TRIGGER sync_capture_chat_agent_policy_revisions_insert;
DROP TRIGGER sync_capture_chat_agent_policy_revisions_update;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_delete;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_insert;
DROP TRIGGER sync_capture_chat_agent_working_folder_grants_update;
DROP TRIGGER sync_capture_chat_agents_delete;
DROP TRIGGER sync_capture_chat_agents_insert;
DROP TRIGGER sync_capture_chat_agents_update;
DROP TRIGGER sync_capture_chat_channels_delete;
DROP TRIGGER sync_capture_chat_channels_insert;
DROP TRIGGER sync_capture_chat_channels_update;
DROP TRIGGER sync_capture_chat_participant_reference_targets_delete;
DROP TRIGGER sync_capture_chat_participant_reference_targets_insert;
DROP TRIGGER sync_capture_chat_participant_reference_targets_update;
DROP TRIGGER sync_capture_chat_push_approvals_delete;
DROP TRIGGER sync_capture_chat_push_approvals_insert;
DROP TRIGGER sync_capture_chat_push_approvals_update;
DROP TRIGGER sync_capture_chat_work_assignments_delete;
DROP TRIGGER sync_capture_chat_work_assignments_insert;
DROP TRIGGER sync_capture_chat_work_assignments_update;
DROP TRIGGER sync_immutable_chat_agent_channel_memberships;
DROP TRIGGER sync_immutable_chat_agent_policy_revisions;
DROP TRIGGER sync_immutable_chat_agent_working_folder_grants;
DROP TRIGGER sync_immutable_chat_agents;
DROP TRIGGER sync_immutable_chat_channels;
DROP TRIGGER sync_immutable_chat_participant_reference_targets;
DROP TRIGGER sync_immutable_chat_push_approvals;
DROP TRIGGER sync_immutable_chat_work_assignments;
DROP TRIGGER sync_recreate_chat_agent_policy_revisions;
DROP TRIGGER sync_recreate_chat_agents;
DROP TRIGGER sync_recreate_chat_channels;
DROP TRIGGER sync_recreate_chat_participant_reference_targets;
DROP TRIGGER sync_recreate_chat_work_assignments;
DROP TRIGGER sync_values_chat_agent_channel_memberships_insert;
DROP TRIGGER sync_values_chat_agent_channel_memberships_update;
DROP TRIGGER sync_values_chat_agent_policy_revisions_insert;
DROP TRIGGER sync_values_chat_agent_policy_revisions_update;
DROP TRIGGER sync_values_chat_agent_working_folder_grants_insert;
DROP TRIGGER sync_values_chat_agent_working_folder_grants_update;
DROP TRIGGER sync_values_chat_agents_insert;
DROP TRIGGER sync_values_chat_agents_update;
DROP TRIGGER sync_values_chat_channels_insert;
DROP TRIGGER sync_values_chat_channels_update;
DROP TRIGGER sync_values_chat_participant_reference_targets_insert;
DROP TRIGGER sync_values_chat_participant_reference_targets_update;
DROP TRIGGER sync_values_chat_push_approvals_insert;
DROP TRIGGER sync_values_chat_push_approvals_update;
DROP TRIGGER sync_values_chat_work_assignments_insert;
DROP TRIGGER sync_values_chat_work_assignments_update;

DROP TABLE chat_agent_working_folder_grants;
DROP TABLE chat_agent_access_state;

CREATE TABLE chat_agent_app_settings (
  id TEXT PRIMARY KEY NOT NULL CHECK (id = 'vault'),
  agent_access TEXT NOT NULL DEFAULT 'ask' CHECK (agent_access IN ('talk', 'read', 'ask', 'own')),
  agent_instructions TEXT NOT NULL DEFAULT '' CHECK (length(agent_instructions) <= 16384),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_agent_app_settings (id, updated_at)
VALUES ('vault', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE chat_project_agent_settings (
  project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_access TEXT CHECK (
      agent_access IS NULL OR agent_access IN ('talk', 'read', 'ask', 'own')
  ),
  agent_instructions TEXT NOT NULL DEFAULT '' CHECK (length(agent_instructions) <= 16384),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

ALTER TABLE chat_channels ADD COLUMN agent_access TEXT CHECK (
    agent_access IS NULL OR agent_access IN ('talk', 'read', 'ask', 'own')
);
ALTER TABLE chat_channels ADD COLUMN agent_instructions TEXT NOT NULL DEFAULT '' CHECK (
    length(agent_instructions) <= 16384
);

CREATE TABLE chat_agents_previous AS SELECT * FROM chat_agents;
DROP TABLE chat_agents;
CREATE TABLE chat_agents (
  participant_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  provider_family_id TEXT NOT NULL CHECK (length(provider_family_id) BETWEEN 1 AND 1024),
  current_policy_revision_id TEXT CHECK (
      current_policy_revision_id IS NULL
      OR length(current_policy_revision_id) BETWEEN 1 AND 1024
  ),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO chat_agents (
    participant_id, provider_family_id, current_policy_revision_id, created_at, updated_at
)
SELECT
    agent.participant_id,
    coalesce(
        (SELECT policy.provider_instance_id FROM chat_agent_policy_revisions policy
         WHERE policy.id = agent.current_policy_revision_id),
        (SELECT policy.provider_instance_id FROM chat_agent_policy_revisions policy
         WHERE policy.agent_id = agent.participant_id
         ORDER BY policy.created_at DESC, policy.id DESC
         LIMIT 1),
        'codex'
    ),
    agent.current_policy_revision_id,
    agent.created_at,
    agent.updated_at
FROM chat_agents_previous agent;
DROP TABLE chat_agents_previous;

CREATE TABLE chat_agent_policy_revisions_previous AS SELECT * FROM chat_agent_policy_revisions;
DROP TABLE chat_agent_policy_revisions;
CREATE TABLE chat_agent_policy_revisions (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
      model_selection_schema_version >= 1
  ),
  model_selection_data TEXT NOT NULL CHECK (
      length(model_selection_data) <= 16384
      AND json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
  ),
  provider_options_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
      provider_options_schema_version >= 1
  ),
  provider_options_data TEXT NOT NULL DEFAULT '{}' CHECK (
      length(provider_options_data) <= 16384
      AND json_valid(provider_options_data) AND json_type(provider_options_data) = 'object'
  ),
  interaction_mode TEXT NOT NULL DEFAULT 'build' CHECK (interaction_mode = 'build'),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;
INSERT INTO chat_agent_policy_revisions (
    id, agent_id, model_selection_schema_version, model_selection_data,
    provider_options_schema_version, provider_options_data, interaction_mode, created_at
)
SELECT
    id, agent_id, model_selection_schema_version, model_selection_data,
    provider_options_schema_version, provider_options_data, interaction_mode, created_at
FROM chat_agent_policy_revisions_previous;
DROP TABLE chat_agent_policy_revisions_previous;

CREATE INDEX idx_chat_agent_policy_revisions_agent
ON chat_agent_policy_revisions(agent_id, created_at, id);

CREATE TABLE chat_agent_channel_memberships_previous AS
SELECT * FROM chat_agent_channel_memberships;
DROP TABLE chat_agent_channel_memberships;
CREATE TABLE chat_agent_channel_memberships (
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
  -- The model this channel starts the agent's work on: its selection schema version and data
  -- together, or neither when the channel uses the agent's default model.
  model_selection_schema_version INTEGER CHECK (
      model_selection_schema_version IS NULL OR model_selection_schema_version >= 1
  ),
  model_selection_data TEXT CHECK (
      model_selection_data IS NULL OR (
          length(model_selection_data) <= 16384
          AND json_valid(model_selection_data)
          AND json_type(model_selection_data) = 'object'
      )
  ),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
  PRIMARY KEY (conversation_id, agent_id),
  CHECK ((model_selection_schema_version IS NULL) = (model_selection_data IS NULL))
) STRICT, WITHOUT ROWID;
INSERT INTO chat_agent_channel_memberships (
    conversation_id, agent_id, sync_key, created_at, updated_at, removed_at
)
SELECT conversation_id, agent_id, sync_key, created_at, updated_at, removed_at
FROM chat_agent_channel_memberships_previous;
DROP TABLE chat_agent_channel_memberships_previous;

CREATE INDEX idx_chat_agent_channel_memberships_agent
ON chat_agent_channel_memberships(agent_id, conversation_id);

-- A standing approval names the channel it was given in, so it cannot be copied from a row that
-- does not; approvals are re-given in the channel.
DROP TABLE chat_push_approvals;
CREATE TABLE chat_push_approvals (
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  push_url TEXT NOT NULL CHECK (length(CAST(push_url AS BLOB)) BETWEEN 1 AND 2048),
  destination_branch TEXT NOT NULL CHECK (
      length(CAST(destination_branch AS BLOB)) BETWEEN 1 AND 1024
  ),
  sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
  approved_by_participant_id TEXT REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  approved_at TEXT NOT NULL CHECK (length(approved_at) >= 20),
  revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  PRIMARY KEY (conversation_id, agent_id, working_folder_id, push_url, destination_branch)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_chat_push_approvals_folder
ON chat_push_approvals(working_folder_id);

CREATE INDEX idx_chat_push_approvals_approved_by
ON chat_push_approvals(approved_by_participant_id);

CREATE INDEX idx_chat_push_approvals_agent
ON chat_push_approvals(agent_id, conversation_id);

CREATE TABLE chat_participant_reference_targets_previous AS
SELECT * FROM chat_participant_reference_targets;
DROP TABLE chat_participant_reference_targets;
CREATE TABLE chat_participant_reference_targets (
  reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  participant_id TEXT NOT NULL CHECK (length(participant_id) BETWEEN 1 AND 1024)
) STRICT;
INSERT INTO chat_participant_reference_targets (reference_id, participant_id)
SELECT reference_id, participant_id FROM chat_participant_reference_targets_previous;
DROP TABLE chat_participant_reference_targets_previous;

CREATE TABLE chat_work_assignments_previous AS SELECT * FROM chat_work_assignments;
DROP TABLE chat_work_assignments;
CREATE TABLE chat_work_assignments (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
  reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  executing_device_id TEXT CHECK (
      executing_device_id IS NULL
      OR length(CAST(executing_device_id AS BLOB)) BETWEEN 1 AND 160
  ),
  -- The model and the joined vault, project, and channel instructions the work starts with,
  -- resolved once by the device that creates it and copied by every continuation, so editing
  -- either later changes new work and leaves this thread alone.
  model_selection_schema_version INTEGER NOT NULL CHECK (model_selection_schema_version >= 1),
  model_selection_data TEXT NOT NULL CHECK (
      length(model_selection_data) <= 16384
      AND json_valid(model_selection_data)
      AND json_type(model_selection_data) = 'object'
  ),
  instructions TEXT NOT NULL DEFAULT '' CHECK (length(instructions) <= 50176),
  -- The display name of the model that actually ran, written by the executing device, so every
  -- reply names its model on every device.
  model_label TEXT CHECK (model_label IS NULL OR length(model_label) BETWEEN 1 AND 256),
  previous_assignment_id TEXT REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  state TEXT NOT NULL CHECK (
      state IN (
          'queued', 'working', 'waiting_for_answer', 'waiting_for_approval',
          'ready_for_review', 'completed', 'failed', 'cancelled'
      )
  ),
  state_reason TEXT CHECK (state_reason IS NULL OR length(state_reason) <= 4000),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  -- The outcome of a review of ready-for-review work, beside the completion it judges. The
  -- reviewer has no foreign key, since a participant row may arrive through sync after the
  -- review, and takes the participant id bounds: at most 128 bytes and no control characters.
  review_outcome TEXT CHECK (
      review_outcome IS NULL OR review_outcome IN ('accepted', 'changes_requested')
  ),
  reviewed_at TEXT CHECK (
      reviewed_at IS NULL OR length(reviewed_at) >= 20
  ),
  reviewed_by_participant_id TEXT CHECK (
      reviewed_by_participant_id IS NULL OR (
          length(CAST(reviewed_by_participant_id AS BLOB)) BETWEEN 1 AND 128
          AND instr(CAST(reviewed_by_participant_id AS BLOB), x'00') = 0
          AND reviewed_by_participant_id NOT GLOB
              '*[' || char(1) || '-' || char(31) || char(127) || '-' || char(159) || ']*'
      )
  ),
  -- Delegated work names its parent, its depth below the root (at most 2), the objective and
  -- expected result the parent wrote, and the folder ceiling it may never exceed, as a JSON
  -- array of working folder ids with a capability. All five are null with depth 0 on an
  -- assignment that nobody delegated. They are fixed at creation and the child is deleted with
  -- its parent.
  parent_assignment_id TEXT
      REFERENCES chat_work_assignments(id) ON UPDATE CASCADE ON DELETE CASCADE,
  delegation_depth INTEGER NOT NULL DEFAULT 0 CHECK (
      delegation_depth BETWEEN 0 AND 2
  ),
  delegation_objective TEXT CHECK (
      delegation_objective IS NULL OR length(delegation_objective) <= 4000
  ),
  delegation_expected_result TEXT CHECK (
      delegation_expected_result IS NULL OR length(delegation_expected_result) <= 2000
  ),
  delegation_folder_ceiling TEXT CHECK (
      delegation_folder_ceiling IS NULL OR (
          json_valid(delegation_folder_ceiling) AND json_type(delegation_folder_ceiling) = 'array'
      )
  ),
  CHECK (
      (review_outcome IS NULL AND reviewed_at IS NULL AND reviewed_by_participant_id IS NULL)
      OR (
          review_outcome IS NOT NULL AND reviewed_at IS NOT NULL
          AND reviewed_by_participant_id IS NOT NULL AND state = 'completed'
      )
  )
) STRICT;
INSERT INTO chat_work_assignments (
    id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id,
    model_selection_schema_version, model_selection_data, instructions, model_label,
    previous_assignment_id, state, state_reason, revision, settled_at, created_at, updated_at,
    review_outcome, reviewed_at, reviewed_by_participant_id, parent_assignment_id,
    delegation_depth, delegation_objective, delegation_expected_result,
    delegation_folder_ceiling
)
SELECT
    id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id,
    coalesce(override_model_selection_schema_version, 1),
    coalesce(
        override_model_selection_data,
        '{"providerManagedModel":true,"modelId":null,"modelOptions":[]}'
    ),
    '', NULL,
    previous_assignment_id, state, state_reason, revision, settled_at, created_at, updated_at,
    review_outcome, reviewed_at, reviewed_by_participant_id, parent_assignment_id,
    delegation_depth, delegation_objective, delegation_expected_result,
    delegation_folder_ceiling
FROM chat_work_assignments_previous;
DROP TABLE chat_work_assignments_previous;

CREATE INDEX idx_chat_work_assignments_thread
ON chat_work_assignments(reply_thread_id, state, created_at, id);

CREATE INDEX idx_chat_work_assignments_trigger
ON chat_work_assignments(triggering_message_item_id);

CREATE INDEX idx_chat_work_assignments_parent
ON chat_work_assignments(parent_assignment_id)
WHERE parent_assignment_id IS NOT NULL;

CREATE INDEX idx_chat_work_assignments_attention
ON chat_work_assignments(state, updated_at DESC, id)
WHERE state IN ('waiting_for_answer', 'waiting_for_approval', 'ready_for_review', 'failed');

CREATE INDEX idx_chat_work_assignments_previous
ON chat_work_assignments(previous_assignment_id)
WHERE previous_assignment_id IS NOT NULL;

CREATE INDEX idx_chat_work_assignments_agent
ON chat_work_assignments(agent_id, state, updated_at DESC, id);

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
    id, assignment_id, revision, requester_participant_id, destination_conversation_id,
    execution_environment_id, working_folder_id, resolved_runtime_approval_policy,
    scope_digest, decision_state, reason, created_at, revoked_at
)
SELECT
    id, assignment_id, revision, requester_participant_id, destination_conversation_id,
    execution_environment_id, working_folder_id, resolved_runtime_approval_policy,
    scope_digest, decision_state, reason, created_at, revoked_at
FROM chat_assignment_authorization_revisions_previous;
DROP TABLE chat_assignment_authorization_revisions_previous;

CREATE INDEX idx_chat_assignment_authorization_revisions_active
ON chat_assignment_authorization_revisions(assignment_id, decision_state, revision DESC);

CREATE TABLE chat_agent_runs_previous AS SELECT * FROM chat_agent_runs;
DROP TABLE chat_agent_runs;
CREATE TABLE chat_agent_runs (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
  assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  working_folder_id TEXT,
  execution_environment_id TEXT REFERENCES chat_execution_environments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  authorization_scope_digest TEXT NOT NULL CHECK (length(authorization_scope_digest) = 64),
  provider_turn_id TEXT NOT NULL UNIQUE REFERENCES chat_turns(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  provider_thread_id TEXT REFERENCES chat_threads(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (
      state IN ('queued', 'starting', 'working', 'waiting', 'completed', 'failed', 'cancelled')
  ),
  run_ordinal INTEGER NOT NULL CHECK (run_ordinal >= 1),
  started_at TEXT CHECK (started_at IS NULL OR length(started_at) >= 20),
  settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  -- Why the run started and what context it delivered, so a later run in the same reply thread
  -- can decide whether to continue the native session and which sources it still has to send.
  attempt_reason TEXT NOT NULL DEFAULT 'fresh' CHECK (
      attempt_reason IN ('fresh', 'continuation', 'handoff', 'retry')
  ),
  handoff_reason TEXT CHECK (
      handoff_reason IS NULL
      OR handoff_reason IN ('key_changed', 'authority_revoked', 'native_session_lost')
  ),
  instructions_sha256 TEXT CHECK (
      instructions_sha256 IS NULL OR length(instructions_sha256) = 64
  ),
  model_selection_sha256 TEXT CHECK (
      model_selection_sha256 IS NULL OR length(model_selection_sha256) = 64
  ),
  context_delivery TEXT NOT NULL DEFAULT 'full' CHECK (
      context_delivery IN ('full', 'delta')
  ),
  delivered_context_bytes INTEGER NOT NULL DEFAULT 0 CHECK (
      delivered_context_bytes BETWEEN 0 AND 131072
  ),
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
INSERT INTO chat_agent_runs (
    id, assignment_id, project_id, working_folder_id, execution_environment_id,
    scratch_generation_id, authorization_revision_id, authorization_scope_digest,
    provider_turn_id, provider_thread_id, state, run_ordinal, started_at, settled_at,
    created_at, updated_at, attempt_reason, handoff_reason, instructions_sha256,
    model_selection_sha256, context_delivery, delivered_context_bytes
)
SELECT
    id, assignment_id, project_id, working_folder_id, execution_environment_id,
    scratch_generation_id, authorization_revision_id, authorization_scope_digest,
    provider_turn_id, provider_thread_id, state, run_ordinal, started_at, settled_at,
    created_at, updated_at, attempt_reason, handoff_reason, instructions_sha256,
    model_selection_sha256, context_delivery, delivered_context_bytes
FROM chat_agent_runs_previous;
DROP TABLE chat_agent_runs_previous;

CREATE INDEX idx_chat_agent_runs_authorization
ON chat_agent_runs(authorization_revision_id, state, id);

CREATE INDEX idx_chat_agent_runs_provider_thread
ON chat_agent_runs(provider_thread_id, assignment_id);

DELETE FROM sync_capture WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_version_refs WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_register_versions WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_row_creations WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_tombstones WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_contested_classes WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_displaced_versions WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_late_changes WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_refused_rows WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_restated_groups WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_stale_rows WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_unique_classes WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_seeded_tables WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);
DELETE FROM sync_rows WHERE table_id IN (353, 354, 362, 363, 364, 365, 377, 395);

CREATE VIEW chat_agent_policy_revisions_numbered AS
SELECT
    policy.id,
    policy.agent_id,
    policy.model_selection_schema_version,
    policy.model_selection_data,
    policy.provider_options_schema_version,
    policy.provider_options_data,
    policy.interaction_mode,
    policy.created_at,
    (
        SELECT count(*) FROM chat_agent_policy_revisions earlier
        WHERE earlier.agent_id = policy.agent_id
          AND (earlier.created_at, earlier.id) <= (policy.created_at, policy.id)
    ) AS revision
FROM chat_agent_policy_revisions policy;

-- The agent's named current revision while it belongs to the agent, else its newest revision,
-- else no row: an agent without a revision uses its provider's default model.
CREATE VIEW chat_agent_effective_policies AS
SELECT policy.*
FROM chat_agents agent
JOIN chat_agent_policy_revisions_numbered policy
  ON policy.id = coalesce(
      (
          SELECT named.id FROM chat_agent_policy_revisions named
          WHERE named.id = agent.current_policy_revision_id
            AND named.agent_id = agent.participant_id
      ),
      (
          SELECT newest.id FROM chat_agent_policy_revisions newest
          WHERE newest.agent_id = agent.participant_id
          ORDER BY newest.created_at DESC, newest.id DESC
          LIMIT 1
      )
  );

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
AFTER UPDATE OF conversation_id, agent_id, created_at, removed_at
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
    WHERE conversation_id = OLD.conversation_id AND agent_id = OLD.agent_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_channel_membership_push_approvals_update
AFTER UPDATE OF removed_at
ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
    AND NEW.removed_at IS NOT NULL
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE conversation_id = NEW.conversation_id AND agent_id = NEW.agent_id
      AND revoked_at IS NULL;
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

CREATE TRIGGER chat_push_approvals_approver_is_person_insert
BEFORE INSERT ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Push approvals are made by people');
END;

CREATE TRIGGER chat_push_approvals_approver_is_person_update
BEFORE UPDATE OF approved_by_participant_id, approved_at
ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        NEW.approved_by_participant_id IS NULL
        AND OLD.approved_by_participant_id IS NOT NULL
        AND NEW.approved_at IS OLD.approved_at
    )
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Push approvals are made by people');
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

CREATE TRIGGER chat_work_assignment_delegation_insert
BEFORE INSERT ON chat_work_assignments
WHEN (
    NEW.delegation_depth = 0
    AND (
        NEW.parent_assignment_id IS NOT NULL
        OR NEW.delegation_objective IS NOT NULL
        OR NEW.delegation_expected_result IS NOT NULL
        OR NEW.delegation_folder_ceiling IS NOT NULL
    )
) OR (
    NEW.delegation_depth <> 0
    AND (
        NEW.parent_assignment_id IS NULL
        OR NEW.delegation_objective IS NULL
        OR NEW.delegation_expected_result IS NULL
        OR NEW.delegation_folder_ceiling IS NULL
        OR NEW.parent_assignment_id = NEW.id
    )
)
BEGIN
    SELECT RAISE(ABORT, 'A delegated assignment needs every delegation field and another parent');
END;

CREATE TRIGGER chat_work_assignment_delegation_depth_insert
BEFORE INSERT ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.delegation_depth <> 0
    AND EXISTS (
        SELECT 1 FROM chat_work_assignments parent
        WHERE parent.id = NEW.parent_assignment_id
          AND parent.delegation_depth + 1 <> NEW.delegation_depth
    )
BEGIN
    SELECT RAISE(ABORT, 'A delegated assignment must sit one level below its parent');
END;

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
 AND EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.requester_participant_id)
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
    assignment_id, revision, requester_participant_id, destination_conversation_id,
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

CREATE TRIGGER chat_agent_run_attempt_immutable
BEFORE UPDATE OF
    attempt_reason, handoff_reason, instructions_sha256,
    model_selection_sha256, context_delivery, delivered_context_bytes
ON chat_agent_runs
WHEN OLD.state <> 'starting'
BEGIN
    SELECT RAISE(ABORT, 'Agent run attempt is immutable');
END;

CREATE TRIGGER chat_agent_run_authority_immutable
BEFORE UPDATE OF
    assignment_id, project_id, working_folder_id,
    execution_environment_id, scratch_generation_id,
    authorization_revision_id, authorization_scope_digest, provider_turn_id,
    provider_thread_id, run_ordinal, created_at
ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run authority is immutable');
END;

CREATE TRIGGER chat_agent_run_authorization_insert
BEFORE INSERT ON chat_agent_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
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
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
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
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT (
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

CREATE TRIGGER chat_agent_run_usage_working_insert
AFTER INSERT ON chat_agent_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_agent_run_usage (run_id, working_since, updated_at)
    VALUES (
        NEW.id,
        CASE WHEN NEW.state = 'working' THEN NEW.updated_at ELSE NULL END,
        NEW.updated_at
    )
    ON CONFLICT(run_id) DO NOTHING;
END;

CREATE TRIGGER chat_agent_run_usage_working_update
AFTER UPDATE OF state ON chat_agent_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND OLD.state IS NOT NEW.state
BEGIN
    INSERT INTO chat_agent_run_usage (run_id, updated_at)
    VALUES (NEW.id, NEW.updated_at)
    ON CONFLICT(run_id) DO NOTHING;
    UPDATE chat_agent_run_usage
    SET working_seconds = working_seconds + CASE
            WHEN working_since IS NOT NULL AND NEW.state <> 'working' THEN coalesce(max(0,
                CAST(round((julianday(NEW.updated_at) - julianday(working_since)) * 86400)
                    AS INTEGER)), 0)
            ELSE 0
        END,
        working_since = CASE
            WHEN NEW.state = 'working' THEN coalesce(working_since, NEW.updated_at)
            ELSE NULL
        END,
        updated_at = NEW.updated_at
    WHERE run_id = NEW.id;
END;

CREATE TRIGGER sync_values_chat_agents_insert
BEFORE INSERT ON chat_agents
WHEN ((typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128)
    AND (typeof(NEW.provider_family_id) = 'text' AND instr(CAST(NEW.provider_family_id AS BLOB), x'00') = 0 AND length(trim(NEW.provider_family_id)) BETWEEN 1 AND 1024 AND length(CAST(NEW.provider_family_id AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.current_policy_revision_id IS NULL OR (typeof(NEW.current_policy_revision_id) = 'text' AND instr(CAST(NEW.current_policy_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_policy_revision_id) <> '' AND length(CAST(NEW.current_policy_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agents_update
BEFORE UPDATE OF provider_family_id, created_at, current_policy_revision_id, updated_at ON chat_agents
WHEN ((typeof(NEW.provider_family_id) = 'text' AND instr(CAST(NEW.provider_family_id AS BLOB), x'00') = 0 AND length(trim(NEW.provider_family_id)) BETWEEN 1 AND 1024 AND length(CAST(NEW.provider_family_id AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.current_policy_revision_id IS NULL OR (typeof(NEW.current_policy_revision_id) = 'text' AND instr(CAST(NEW.current_policy_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_policy_revision_id) <> '' AND length(CAST(NEW.current_policy_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_agents
BEFORE UPDATE OF participant_id, provider_family_id, created_at ON chat_agents
WHEN OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY
    OR ((OLD.provider_family_id IS NOT NEW.provider_family_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_agents
BEFORE INSERT ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 353 AND row_key = NEW.participant_id)
    AND NOT EXISTS (SELECT 1 FROM chat_agents WHERE participant_id = NEW.participant_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_agents_insert
AFTER INSERT ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, NEW.participant_id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
            (CASE WHEN OLD.current_policy_revision_id IS NOT NEW.current_policy_revision_id COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_agents_delete
AFTER DELETE ON chat_agents
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, OLD.participant_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_agent_policy_revisions_insert
BEFORE INSERT ON chat_agent_policy_revisions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (typeof(NEW.provider_options_schema_version) = 'integer' AND NEW.provider_options_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.provider_options_data) = 'text' AND instr(CAST(NEW.provider_options_data AS BLOB), x'00') = 0 AND length(NEW.provider_options_data) <= 16384 AND length(CAST(NEW.provider_options_data AS BLOB)) <= 262144)
    AND (typeof(NEW.interaction_mode) = 'text' AND instr(CAST(NEW.interaction_mode AS BLOB), x'00') = 0 AND length(NEW.interaction_mode) <= 32 AND length(CAST(NEW.interaction_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_policy_revisions_update
BEFORE UPDATE OF agent_id, model_selection_schema_version, model_selection_data, provider_options_schema_version, provider_options_data, interaction_mode, created_at ON chat_agent_policy_revisions
WHEN ((NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (typeof(NEW.provider_options_schema_version) = 'integer' AND NEW.provider_options_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.provider_options_data) = 'text' AND instr(CAST(NEW.provider_options_data AS BLOB), x'00') = 0 AND length(NEW.provider_options_data) <= 16384 AND length(CAST(NEW.provider_options_data AS BLOB)) <= 262144)
    AND (typeof(NEW.interaction_mode) = 'text' AND instr(CAST(NEW.interaction_mode AS BLOB), x'00') = 0 AND length(NEW.interaction_mode) <= 32 AND length(CAST(NEW.interaction_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_agent_policy_revisions
BEFORE UPDATE OF id, agent_id, model_selection_schema_version, model_selection_data, provider_options_schema_version, provider_options_data, interaction_mode, created_at ON chat_agent_policy_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY OR OLD.provider_options_schema_version IS NOT NEW.provider_options_schema_version OR OLD.provider_options_data IS NOT NEW.provider_options_data COLLATE BINARY OR OLD.interaction_mode IS NOT NEW.interaction_mode COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_agent_policy_revisions
BEFORE INSERT ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 354 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_agent_policy_revisions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
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

CREATE TRIGGER sync_capture_chat_agent_policy_revisions_delete
AFTER DELETE ON chat_agent_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (354, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_agent_app_settings_insert
BEFORE INSERT ON chat_agent_app_settings
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144)
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_app_settings_update
BEFORE UPDATE OF agent_access, agent_instructions, updated_at ON chat_agent_app_settings
WHEN ((typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144)
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_agent_app_settings
BEFORE UPDATE OF id ON chat_agent_app_settings
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_agent_app_settings
BEFORE INSERT ON chat_agent_app_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 357 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_agent_app_settings WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_agent_app_settings_insert
AFTER INSERT ON chat_agent_app_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (357, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_agent_app_settings_update
AFTER UPDATE ON chat_agent_app_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 357, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.agent_access IS NOT NEW.agent_access COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.agent_instructions IS NOT NEW.agent_instructions COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_agent_app_settings_delete
AFTER DELETE ON chat_agent_app_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (357, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_project_agent_settings_insert
BEFORE INSERT ON chat_project_agent_settings
WHEN ((typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.agent_access IS NULL OR (typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144))
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_project_agent_settings_update
BEFORE UPDATE OF created_at, agent_access, agent_instructions, updated_at ON chat_project_agent_settings
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.agent_access IS NULL OR (typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144))
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_project_agent_settings
BEFORE UPDATE OF project_id, created_at ON chat_project_agent_settings
WHEN OLD.project_id IS NOT NEW.project_id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_project_agent_settings
BEFORE INSERT ON chat_project_agent_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 358 AND row_key = NEW.project_id)
    AND NOT EXISTS (SELECT 1 FROM chat_project_agent_settings WHERE project_id = NEW.project_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_project_agent_settings_insert
AFTER INSERT ON chat_project_agent_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (358, NEW.project_id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_project_agent_settings_update
AFTER UPDATE ON chat_project_agent_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 358, NEW.project_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.agent_access IS NOT NEW.agent_access COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.agent_instructions IS NOT NEW.agent_instructions COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_project_agent_settings_delete
AFTER DELETE ON chat_project_agent_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (358, OLD.project_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_agent_channel_memberships_insert
BEFORE INSERT ON chat_agent_channel_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.model_selection_schema_version IS NULL OR (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.model_selection_data IS NULL OR (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_agent_channel_memberships_update
BEFORE UPDATE OF conversation_id, agent_id, removed_at, created_at, updated_at, model_selection_schema_version, model_selection_data ON chat_agent_channel_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.model_selection_schema_version IS NULL OR (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.model_selection_data IS NULL OR (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144))) IS NOT 1
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
    VALUES (362, NEW.sync_key, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
            (CASE WHEN OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
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

CREATE TRIGGER sync_values_chat_push_approvals_insert
BEFORE INSERT ON chat_push_approvals
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_push_approvals_update
BEFORE UPDATE OF conversation_id, agent_id, working_folder_id, push_url, destination_branch, approved_by_participant_id, approved_at, revoked_at ON chat_push_approvals
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_push_approvals
BEFORE UPDATE OF sync_key, conversation_id, agent_id, working_folder_id, push_url, destination_branch ON chat_push_approvals
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY OR OLD.push_url IS NOT NEW.push_url COLLATE BINARY OR OLD.destination_branch IS NOT NEW.destination_branch COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_push_approvals_insert
AFTER INSERT ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (365, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_push_approvals_update
AFTER UPDATE ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 365, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.approved_by_participant_id IS NOT NEW.approved_by_participant_id COLLATE BINARY
                AND NOT (NEW.approved_by_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.approved_by_participant_id))) OR OLD.approved_at IS NOT NEW.approved_at COLLATE BINARY OR OLD.revoked_at IS NOT NEW.revoked_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 365, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_push_approvals_delete
AFTER DELETE ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (365, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_channels_insert
BEFORE INSERT ON chat_channels
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.topic) = 'text' AND instr(CAST(NEW.topic AS BLOB), x'00') = 0 AND length(NEW.topic) <= 250 AND length(CAST(NEW.topic AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.agent_access IS NULL OR (typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144))
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_channels_update
BEFORE UPDATE OF project_id, conversation_id, is_default, created_at, name, topic, archived_at, updated_at, agent_access, agent_instructions ON chat_channels
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.topic) = 'text' AND instr(CAST(NEW.topic AS BLOB), x'00') = 0 AND length(NEW.topic) <= 250 AND length(CAST(NEW.topic AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.agent_access IS NULL OR (typeof(NEW.agent_access) = 'text' AND instr(CAST(NEW.agent_access AS BLOB), x'00') = 0 AND length(NEW.agent_access) <= 32 AND length(CAST(NEW.agent_access AS BLOB)) <= 262144))
    AND (typeof(NEW.agent_instructions) = 'text' AND instr(CAST(NEW.agent_instructions AS BLOB), x'00') = 0 AND length(NEW.agent_instructions) <= 16384 AND length(CAST(NEW.agent_instructions AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_channels
BEFORE UPDATE OF id, project_id, conversation_id, is_default, created_at ON chat_channels
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.is_default IS NOT NEW.is_default OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_channels
BEFORE INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 364 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_channels WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_channels_insert
AFTER INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (364, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_channels_update
AFTER UPDATE ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 364, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY OR OLD.topic IS NOT NEW.topic COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.agent_access IS NOT NEW.agent_access COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.agent_instructions IS NOT NEW.agent_instructions COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_channels_delete
AFTER DELETE ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (364, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_participant_reference_targets_insert
BEFORE INSERT ON chat_participant_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_participant_reference_targets_update
BEFORE UPDATE OF participant_id ON chat_participant_reference_targets
WHEN ((NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_participant_reference_targets
BEFORE UPDATE OF reference_id, participant_id ON chat_participant_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_participant_reference_targets
BEFORE INSERT ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 377 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_participant_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_insert
AFTER INSERT ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (377, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_update
AFTER UPDATE ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 377, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_delete
AFTER DELETE ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (377, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_work_assignments_insert
BEFORE INSERT ON chat_work_assignments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (typeof(NEW.instructions) = 'text' AND instr(CAST(NEW.instructions AS BLOB), x'00') = 0 AND length(NEW.instructions) <= 50176 AND length(CAST(NEW.instructions AS BLOB)) <= 262144)
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
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))
    AND (NEW.model_label IS NULL OR (typeof(NEW.model_label) = 'text' AND instr(CAST(NEW.model_label AS BLOB), x'00') = 0 AND length(NEW.model_label) <= 256 AND length(CAST(NEW.model_label AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, agent_id, triggering_message_item_id, executing_device_id, created_at, model_selection_schema_version, model_selection_data, instructions, previous_assignment_id, state, state_reason, settled_at, review_outcome, reviewed_at, reviewed_by_participant_id, updated_at, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling, model_label ON chat_work_assignments
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.model_selection_schema_version) = 'integer' AND NEW.model_selection_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.model_selection_data) = 'text' AND instr(CAST(NEW.model_selection_data AS BLOB), x'00') = 0 AND length(NEW.model_selection_data) <= 16384 AND length(CAST(NEW.model_selection_data AS BLOB)) <= 262144)
    AND (typeof(NEW.instructions) = 'text' AND instr(CAST(NEW.instructions AS BLOB), x'00') = 0 AND length(NEW.instructions) <= 50176 AND length(CAST(NEW.instructions AS BLOB)) <= 262144)
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
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))
    AND (NEW.model_label IS NULL OR (typeof(NEW.model_label) = 'text' AND instr(CAST(NEW.model_label AS BLOB), x'00') = 0 AND length(NEW.model_label) <= 256 AND length(CAST(NEW.model_label AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_assignments
BEFORE UPDATE OF id, reply_thread_id, agent_id, triggering_message_item_id, executing_device_id, created_at, model_selection_schema_version, model_selection_data, instructions, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling ON chat_work_assignments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.triggering_message_item_id IS NOT NEW.triggering_message_item_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY OR OLD.instructions IS NOT NEW.instructions COLLATE BINARY OR OLD.parent_assignment_id IS NOT NEW.parent_assignment_id COLLATE BINARY OR OLD.delegation_depth IS NOT NEW.delegation_depth OR OLD.delegation_objective IS NOT NEW.delegation_objective COLLATE BINARY OR OLD.delegation_expected_result IS NOT NEW.delegation_expected_result COLLATE BINARY OR OLD.delegation_folder_ceiling IS NOT NEW.delegation_folder_ceiling COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_work_assignments
BEFORE INSERT ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 395 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_work_assignments_insert
AFTER INSERT ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (395, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_work_assignments_update
AFTER UPDATE ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 395, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.previous_assignment_id IS NOT NEW.previous_assignment_id COLLATE BINARY
                AND NOT (NEW.previous_assignment_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.previous_assignment_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.state_reason IS NOT NEW.state_reason COLLATE BINARY OR OLD.settled_at IS NOT NEW.settled_at COLLATE BINARY OR OLD.review_outcome IS NOT NEW.review_outcome COLLATE BINARY OR OLD.reviewed_at IS NOT NEW.reviewed_at COLLATE BINARY OR OLD.reviewed_by_participant_id IS NOT NEW.reviewed_by_participant_id COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.model_label IS NOT NEW.model_label COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_work_assignments_delete
AFTER DELETE ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (395, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;


COMMIT;

PRAGMA foreign_keys = ON;

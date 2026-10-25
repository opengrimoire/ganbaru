-- no-transaction
-- Chat identity and policy replication. Participants, AI teammates, teammate policy revisions,
-- access profiles, and access profile revisions replicate. Revisions no longer carry integer
-- numbers: a teammate names its current policy revision and an access profile its current
-- revision, each with a copy of the revision's restrictiveness so concurrent edits keep the
-- stricter one, and display numbers come from `(created_at, id)` order. A teammate records the
-- device its assignments run on in `home_device_id`.
--
-- Uniqueness of the local user and of teammate and custom profile names cannot hold across
-- devices, so the partial unique indexes go; the local user keeps its seeded id through an
-- insert trigger. Foreign keys into the replicated tables cascade or clear: tables that keep
-- history set null on columns that cannot hold null, so deleting a participant, teammate, or
-- profile stays refused. Device-local Chat triggers that a snapshot copy or local state restore
-- would trip are gated on the sync applier.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- chat_participants
CREATE TABLE chat_participants_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    participant_kind TEXT NOT NULL CHECK (
        participant_kind IN ('local_user', 'ai_teammate', 'human')
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

INSERT INTO chat_participants_next (
    id, participant_kind, display_name, avatar_schema_version, avatar_data, revision,
    archived_at, created_at, updated_at
)
SELECT
    id, participant_kind, display_name, avatar_schema_version, avatar_data, revision,
    archived_at, created_at, updated_at
FROM chat_participants;

DROP TABLE chat_participants;
ALTER TABLE chat_participants_next RENAME TO chat_participants;

CREATE TRIGGER chat_participants_local_user_identity
BEFORE INSERT ON chat_participants
WHEN NEW.participant_kind = 'local_user' AND NEW.id != 'participant:local-owner'
BEGIN
    SELECT RAISE(ABORT, 'The local user keeps its seeded participant id');
END;

-- chat_teammates
CREATE TABLE chat_teammates_next (
    participant_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (length(trim(role)) BETWEEN 1 AND 1000),
    instructions TEXT NOT NULL DEFAULT '' CHECK (length(instructions) <= 65536),
    current_policy_revision_id TEXT CHECK (
        current_policy_revision_id IS NULL
        OR length(current_policy_revision_id) BETWEEN 1 AND 1024
    ),
    current_policy_safety_mode TEXT CHECK (
        current_policy_safety_mode IS NULL OR current_policy_safety_mode IN (
            'ask_for_approval', 'approve_for_me', 'full_access', 'custom'
        )
    ),
    home_device_id TEXT CHECK (
        home_device_id IS NULL OR length(CAST(home_device_id AS BLOB)) BETWEEN 1 AND 160
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    CHECK ((current_policy_revision_id IS NULL) = (current_policy_safety_mode IS NULL))
) STRICT;

INSERT INTO chat_teammates_next (
    participant_id, role, instructions, current_policy_revision_id,
    current_policy_safety_mode, home_device_id, created_at, updated_at
)
SELECT
    teammate.participant_id,
    teammate.role,
    teammate.instructions,
    policy.id,
    policy.safety_mode,
    NULL,
    teammate.created_at,
    teammate.updated_at
FROM chat_teammates teammate
LEFT JOIN chat_teammate_policy_revisions policy
  ON policy.teammate_id = teammate.participant_id
 AND policy.revision = teammate.latest_policy_revision;

DROP TABLE chat_teammates;
ALTER TABLE chat_teammates_next RENAME TO chat_teammates;

CREATE TRIGGER chat_teammate_create_access_state
AFTER INSERT ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_teammate_access_state (teammate_id, updated_at)
    VALUES (NEW.participant_id, NEW.updated_at);
END;

-- chat_teammate_policy_revisions
CREATE TABLE chat_teammate_policy_revisions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        model_selection_schema_version >= 1
    ),
    model_selection_data TEXT NOT NULL CHECK (
        length(model_selection_data) <= 16384
        AND json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
    ),
    effort TEXT CHECK (
        effort IS NULL OR effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh')
    ),
    speed TEXT CHECK (speed IS NULL OR speed IN ('standard', 'fast')),
    provider_options_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        provider_options_schema_version >= 1
    ),
    provider_options_data TEXT NOT NULL DEFAULT '{}' CHECK (
        length(provider_options_data) <= 16384
        AND json_valid(provider_options_data) AND json_type(provider_options_data) = 'object'
    ),
    interaction_mode TEXT NOT NULL DEFAULT 'build' CHECK (interaction_mode = 'build'),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    safety_mode TEXT NOT NULL DEFAULT 'ask_for_approval' CHECK (
        safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
    )
) STRICT;

INSERT INTO chat_teammate_policy_revisions_next (
    id, teammate_id, provider_instance_id, model_selection_schema_version,
    model_selection_data, effort, speed, provider_options_schema_version,
    provider_options_data, interaction_mode, created_at, safety_mode
)
SELECT
    id, teammate_id, provider_instance_id, model_selection_schema_version,
    model_selection_data, effort, speed, provider_options_schema_version,
    provider_options_data, interaction_mode, created_at, safety_mode
FROM chat_teammate_policy_revisions;

DROP TABLE chat_teammate_policy_revisions;
ALTER TABLE chat_teammate_policy_revisions_next RENAME TO chat_teammate_policy_revisions;

CREATE INDEX idx_chat_teammate_policy_revisions_teammate
ON chat_teammate_policy_revisions(teammate_id, created_at, id);

CREATE TRIGGER chat_teammate_policy_identity_insert
BEFORE INSERT ON chat_teammate_policy_revisions
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_participants participant
    WHERE participant.id = NEW.teammate_id
      AND participant.participant_kind = 'ai_teammate'
)
BEGIN
    SELECT RAISE(ABORT, 'Teammate policies require an AI teammate participant');
END;

CREATE TRIGGER chat_teammate_policy_immutable_delete
BEFORE DELETE ON chat_teammate_policy_revisions
BEGIN
    SELECT RAISE(ABORT, 'Teammate policy revisions are immutable');
END;

CREATE TRIGGER chat_teammate_policy_immutable_update
BEFORE UPDATE ON chat_teammate_policy_revisions
BEGIN
    SELECT RAISE(ABORT, 'Teammate policy revisions are immutable');
END;

-- chat_access_profiles
CREATE TABLE chat_access_profiles_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    builtin_key TEXT CHECK (
        builtin_key IS NULL OR builtin_key IN (
            'conversation_only', 'read_only', 'edit_files',
            'build_and_test', 'publish_changes'
        )
    ),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
    current_revision_id TEXT CHECK (
        current_revision_id IS NULL OR length(current_revision_id) BETWEEN 1 AND 1024
    ),
    current_maximum_folder_capability TEXT CHECK (
        current_maximum_folder_capability IS NULL OR current_maximum_folder_capability IN (
            'none', 'read', 'edit', 'execute', 'publish'
        )
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    CHECK ((current_revision_id IS NULL) = (current_maximum_folder_capability IS NULL))
) STRICT;

INSERT INTO chat_access_profiles_next (
    id, builtin_key, display_name, current_revision_id, current_maximum_folder_capability,
    revision, archived_at, created_at, updated_at
)
SELECT
    profile.id,
    profile.builtin_key,
    profile.display_name,
    current.id,
    current.maximum_folder_capability,
    profile.revision,
    profile.archived_at,
    profile.created_at,
    profile.updated_at
FROM chat_access_profiles profile
LEFT JOIN chat_access_profile_revisions current
  ON current.access_profile_id = profile.id
 AND current.revision = profile.latest_revision;

DROP TABLE chat_access_profiles;
ALTER TABLE chat_access_profiles_next RENAME TO chat_access_profiles;

CREATE UNIQUE INDEX idx_chat_access_profiles_builtin
ON chat_access_profiles(builtin_key);

CREATE TRIGGER chat_access_profile_builtin_protected
BEFORE UPDATE ON chat_access_profiles
WHEN OLD.builtin_key IS NOT NULL AND (
    NEW.id IS NOT OLD.id
    OR NEW.builtin_key IS NOT OLD.builtin_key
    OR NEW.display_name IS NOT OLD.display_name
    OR NEW.current_revision_id IS NOT OLD.current_revision_id
    OR NEW.current_maximum_folder_capability IS NOT OLD.current_maximum_folder_capability
    OR NEW.archived_at IS NOT OLD.archived_at
    OR NEW.created_at IS NOT OLD.created_at
    OR NEW.updated_at IS NOT OLD.updated_at
)
BEGIN
    SELECT RAISE(ABORT, 'Built-in access profiles are protected');
END;

CREATE TRIGGER chat_access_profile_revision_counter_monotonic
BEFORE UPDATE OF revision ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.revision <= OLD.revision
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions must increase');
END;

-- chat_access_profile_revisions
CREATE TABLE chat_access_profile_revisions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    access_profile_id TEXT NOT NULL REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    default_read_history INTEGER NOT NULL CHECK (default_read_history IN (0, 1)),
    default_participate INTEGER NOT NULL CHECK (default_participate IN (0, 1)),
    default_history_boundary TEXT NOT NULL CHECK (
        default_history_boundary IN ('entire', 'from_grant')
    ),
    maximum_folder_capability TEXT NOT NULL CHECK (
        maximum_folder_capability IN ('none', 'read', 'edit', 'execute', 'publish')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

INSERT INTO chat_access_profile_revisions_next (
    id, access_profile_id, default_read_history, default_participate,
    default_history_boundary, maximum_folder_capability, created_at
)
SELECT
    id, access_profile_id, default_read_history, default_participate,
    default_history_boundary, maximum_folder_capability, created_at
FROM chat_access_profile_revisions;

DROP TABLE chat_access_profile_revisions;
ALTER TABLE chat_access_profile_revisions_next RENAME TO chat_access_profile_revisions;

CREATE INDEX idx_chat_access_profile_revisions_profile
ON chat_access_profile_revisions(access_profile_id, created_at, id);

-- A built-in profile keeps only the seeded revision its protected current pointer names. The
-- check reads the profile alone, which materializes before its revisions.
CREATE TRIGGER chat_access_profile_builtin_revision_protected
BEFORE INSERT ON chat_access_profile_revisions
WHEN EXISTS (
    SELECT 1 FROM chat_access_profiles profile
    WHERE profile.id = NEW.access_profile_id
      AND profile.builtin_key IS NOT NULL
      AND profile.current_revision_id IS NOT NEW.id
)
BEGIN
    SELECT RAISE(ABORT, 'Built-in access profiles are immutable');
END;

CREATE TRIGGER chat_access_profile_revision_immutable_delete
BEFORE DELETE ON chat_access_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions are immutable');
END;

CREATE TRIGGER chat_access_profile_revision_immutable_update
BEFORE UPDATE ON chat_access_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions are immutable');
END;

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

-- Revisions with display numbers. A display number counts the parent's revisions at or before
-- the revision in `(created_at, id)` order, so a lookup reads only that parent's revisions
-- through the parent index.
CREATE VIEW chat_teammate_policy_revisions_numbered AS
SELECT
    policy.id,
    policy.teammate_id,
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
        SELECT count(*) FROM chat_teammate_policy_revisions earlier
        WHERE earlier.teammate_id = policy.teammate_id
          AND (earlier.created_at, earlier.id) <= (policy.created_at, policy.id)
    ) AS revision
FROM chat_teammate_policy_revisions policy;

CREATE VIEW chat_access_profile_revisions_numbered AS
SELECT
    revision.id,
    revision.access_profile_id,
    revision.default_read_history,
    revision.default_participate,
    revision.default_history_boundary,
    revision.maximum_folder_capability,
    revision.created_at,
    (
        SELECT count(*) FROM chat_access_profile_revisions earlier
        WHERE earlier.access_profile_id = revision.access_profile_id
          AND (earlier.created_at, earlier.id) <= (revision.created_at, revision.id)
    ) AS revision
FROM chat_access_profile_revisions revision;

-- The effective revision of each teammate and access profile. The current pointer names it
-- once that revision has arrived; until then the newest arrived revision stands in, and a
-- teammate without any policy revision has no row.
CREATE VIEW chat_teammate_effective_policies AS
SELECT policy.*
FROM chat_teammates teammate
JOIN chat_teammate_policy_revisions_numbered policy
  ON policy.id = COALESCE(
      (
          SELECT current.id FROM chat_teammate_policy_revisions current
          WHERE current.id = teammate.current_policy_revision_id
            AND current.teammate_id = teammate.participant_id
      ),
      (
          SELECT newest.id FROM chat_teammate_policy_revisions newest
          WHERE newest.teammate_id = teammate.participant_id
          ORDER BY newest.created_at DESC, newest.id DESC
          LIMIT 1
      )
  );

CREATE VIEW chat_access_profile_effective_revisions AS
SELECT revision.*
FROM chat_access_profiles profile
JOIN chat_access_profile_revisions_numbered revision
  ON revision.id = COALESCE(
      (
          SELECT current.id FROM chat_access_profile_revisions current
          WHERE current.id = profile.current_revision_id
            AND current.access_profile_id = profile.id
      ),
      (
          SELECT newest.id FROM chat_access_profile_revisions newest
          WHERE newest.access_profile_id = profile.id
          ORDER BY newest.created_at DESC, newest.id DESC
          LIMIT 1
      )
  );

-- chat_communication_messages: the author can no longer be deleted out from under a message.
CREATE TABLE chat_communication_messages_next (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    author_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    current_revision_id TEXT CHECK (
        current_revision_id IS NULL OR length(current_revision_id) BETWEEN 1 AND 1024
    ),
    edited_at TEXT CHECK (edited_at IS NULL OR length(edited_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    author_label_snapshot TEXT NOT NULL DEFAULT 'Unknown participant' CHECK (
        length(trim(author_label_snapshot)) BETWEEN 1 AND 160
    )
) STRICT;

INSERT INTO chat_communication_messages_next (
    item_id, author_participant_id, current_revision_id, edited_at, deleted_at, created_at,
    author_label_snapshot
)
SELECT
    item_id, author_participant_id, current_revision_id, edited_at, deleted_at, created_at,
    author_label_snapshot
FROM chat_communication_messages;

DROP TABLE chat_communication_messages;
ALTER TABLE chat_communication_messages_next RENAME TO chat_communication_messages;

CREATE TRIGGER chat_communication_author_snapshot_immutable
BEFORE UPDATE OF author_label_snapshot ON chat_communication_messages
WHEN NEW.author_label_snapshot IS NOT OLD.author_label_snapshot
BEGIN
    SELECT RAISE(ABORT, 'Communication message author snapshots are immutable');
END;

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

CREATE TRIGGER chat_communication_search_delete
AFTER DELETE ON chat_communication_messages
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = OLD.item_id;
END;

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

-- chat_conversation_memberships: the participant column is part of the key, so clearing it
-- refuses the participant's deletion.
CREATE TABLE chat_conversation_memberships_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    membership_role TEXT NOT NULL DEFAULT 'member' CHECK (
        membership_role IN ('owner', 'member')
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (conversation_id, participant_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_conversation_memberships_next (
    conversation_id, participant_id, membership_role, revision, removed_at, created_at,
    updated_at
)
SELECT
    conversation_id, participant_id, membership_role, revision, removed_at, created_at,
    updated_at
FROM chat_conversation_memberships;

DROP TABLE chat_conversation_memberships;
ALTER TABLE chat_conversation_memberships_next RENAME TO chat_conversation_memberships;

CREATE INDEX idx_chat_conversation_memberships_participant
ON chat_conversation_memberships(participant_id, removed_at, conversation_id);

CREATE TRIGGER chat_generic_membership_audience_delete
AFTER DELETE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_generic_membership_audience_insert
AFTER INSERT ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_generic_membership_audience_update
AFTER UPDATE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

-- chat_participant_reference_targets
CREATE TABLE chat_participant_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE SET NULL
) STRICT;

INSERT INTO chat_participant_reference_targets_next (reference_id, participant_id)
SELECT reference_id, participant_id FROM chat_participant_reference_targets;

DROP TABLE chat_participant_reference_targets;
ALTER TABLE chat_participant_reference_targets_next RENAME TO chat_participant_reference_targets;

-- chat_scheduled_message_references: a participant reference requires its participant, so
-- clearing it refuses the participant's deletion.
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
        ON UPDATE CASCADE ON DELETE SET NULL,
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
    execution_environment_id TEXT,
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
    id, scheduled_message_id, reference_kind, label_snapshot, plain_text_projection,
    start_offset, end_offset, participant_id, participant_kind, channel_id, working_folder_id,
    path_kind, relative_path, execution_environment_id, ordinal
)
SELECT
    id, scheduled_message_id, reference_kind, label_snapshot, plain_text_projection,
    start_offset, end_offset, participant_id, participant_kind, channel_id, working_folder_id,
    path_kind, relative_path, execution_environment_id, ordinal
FROM chat_scheduled_message_references;

DROP TABLE chat_scheduled_message_references;
ALTER TABLE chat_scheduled_message_references_next RENAME TO chat_scheduled_message_references;

CREATE INDEX idx_chat_scheduled_message_references_target
ON chat_scheduled_message_references(scheduled_message_id, reference_kind, ordinal);

-- chat_teammate_channel_memberships: the access profile column cannot hold null, so clearing
-- it refuses the profile's deletion.
CREATE TABLE chat_teammate_channel_memberships_next (
    conversation_id TEXT NOT NULL,
    teammate_id TEXT NOT NULL,
    access_profile_id TEXT NOT NULL,
    read_history INTEGER NOT NULL DEFAULT 0 CHECK (read_history IN (0, 1)),
    read_history_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        read_history_inherits_profile IN (0, 1)
    ),
    participate INTEGER NOT NULL DEFAULT 0 CHECK (participate IN (0, 1)),
    participate_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        participate_inherits_profile IN (0, 1)
    ),
    history_boundary TEXT NOT NULL DEFAULT 'entire' CHECK (
        history_boundary IN ('entire', 'from_grant')
    ),
    history_boundary_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        history_boundary_inherits_profile IN (0, 1)
    ),
    history_from_ordinal INTEGER CHECK (
        history_from_ordinal IS NULL OR history_from_ordinal >= 1
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
    PRIMARY KEY (conversation_id, teammate_id),
    FOREIGN KEY (conversation_id, teammate_id)
        REFERENCES chat_conversation_memberships(conversation_id, participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (access_profile_id)
        REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    CHECK (
        (history_boundary = 'entire' AND history_from_ordinal IS NULL)
        OR (history_boundary = 'from_grant' AND history_from_ordinal IS NOT NULL)
    )
) STRICT, WITHOUT ROWID;

INSERT INTO chat_teammate_channel_memberships_next (
    conversation_id, teammate_id, access_profile_id, read_history,
    read_history_inherits_profile, participate, participate_inherits_profile,
    history_boundary, history_boundary_inherits_profile, history_from_ordinal,
    runtime_approval_policy, scratch_runtime_approval_policy, created_at, updated_at,
    removed_at
)
SELECT
    conversation_id, teammate_id, access_profile_id, read_history,
    read_history_inherits_profile, participate, participate_inherits_profile,
    history_boundary, history_boundary_inherits_profile, history_from_ordinal,
    runtime_approval_policy, scratch_runtime_approval_policy, created_at, updated_at,
    removed_at
FROM chat_teammate_channel_memberships;

DROP TABLE chat_teammate_channel_memberships;
ALTER TABLE chat_teammate_channel_memberships_next RENAME TO chat_teammate_channel_memberships;

CREATE INDEX idx_chat_teammate_channel_memberships_teammate
ON chat_teammate_channel_memberships(teammate_id, conversation_id);

CREATE TRIGGER chat_teammate_channel_membership_audience_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_insert
AFTER INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_update
AFTER UPDATE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

CREATE TRIGGER chat_teammate_channel_membership_identity_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM chat_teammates teammate
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires an AI teammate');
END;

-- chat_work_assignments: the teammate column cannot hold null, so clearing it refuses the
-- teammate's deletion.
CREATE TABLE chat_work_assignments_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
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
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_work_assignments_next (
    id, reply_thread_id, teammate_id, triggering_message_item_id, previous_assignment_id,
    state, state_reason, revision, settled_at, created_at, updated_at
)
SELECT
    id, reply_thread_id, teammate_id, triggering_message_item_id, previous_assignment_id,
    state, state_reason, revision, settled_at, created_at, updated_at
FROM chat_work_assignments;

DROP TABLE chat_work_assignments;
ALTER TABLE chat_work_assignments_next RENAME TO chat_work_assignments;

CREATE UNIQUE INDEX idx_chat_work_assignments_one_active
ON chat_work_assignments(reply_thread_id)
WHERE state IN (
    'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'
);

CREATE INDEX idx_chat_work_assignments_teammate
ON chat_work_assignments(teammate_id, state, updated_at DESC, id);

-- Device-local Chat triggers gated on the sync applier: a snapshot copy clears the device
-- tables and a local state restore puts their rows back, either of which these would refuse.
DROP TRIGGER chat_agent_run_authorization_insert;
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

DROP TRIGGER chat_agent_run_authorization_target_insert;
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

DROP TRIGGER chat_agent_run_execution_target_insert;
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

DROP TRIGGER chat_assignment_authorization_delete_denied;
CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id)
 AND EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.requester_participant_id)
 AND EXISTS (SELECT 1 FROM chat_teammate_policy_revisions WHERE id = OLD.teammate_policy_revision_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.destination_conversation_id)
 AND EXISTS (SELECT 1 FROM chat_access_profile_revisions WHERE id = OLD.access_profile_revision_id)
 AND (OLD.execution_environment_id IS NULL
      OR EXISTS (SELECT 1 FROM chat_execution_environments WHERE id = OLD.execution_environment_id))
 AND (OLD.working_folder_id IS NULL
      OR EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id))
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization revisions are immutable');
END;

DROP TRIGGER chat_assignment_authorization_target_required_insert;
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

DROP TRIGGER chat_authorized_channel_source_immutable_delete;
CREATE TRIGGER chat_authorized_channel_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_channel_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (
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

DROP TRIGGER chat_authorized_folder_source_immutable_delete;
CREATE TRIGGER chat_authorized_folder_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_folder_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions
    WHERE id = OLD.authorization_revision_id
)
 AND EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id)
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources are immutable');
END;

DROP TRIGGER chat_authorized_folder_source_requires_approval;
CREATE TRIGGER chat_authorized_folder_source_requires_approval
BEFORE INSERT ON chat_assignment_authorized_folder_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.resolved_runtime_approval_policy IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources require a frozen approval policy');
END;

DROP TRIGGER chat_attachment_messages_expose_resource;
CREATE TRIGGER chat_attachment_messages_expose_resource
AFTER INSERT ON chat_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.message_id IS NOT NULL
BEGIN
    INSERT OR IGNORE INTO chat_resource_thread_references (
        resource_id,
        thread_id,
        first_message_id,
        created_at
    )
    SELECT NEW.attachment_id, thread_id, id, NEW.created_at
    FROM chat_messages
    WHERE id = NEW.message_id;
END;

DROP TRIGGER chat_events_require_next_sequence;
CREATE TRIGGER chat_events_require_next_sequence
BEFORE INSERT ON chat_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.sequence != (
    SELECT last_event_sequence + 1 FROM chat_threads WHERE id = NEW.thread_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat event sequence must be contiguous');
END;

DROP TRIGGER chat_scratch_generation_source_delete_denied;
CREATE TRIGGER chat_scratch_generation_source_delete_denied
BEFORE DELETE ON chat_scratch_generation_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id)
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints are immutable');
END;

DROP TRIGGER chat_scratch_promotion_audit_delete_denied;
CREATE TRIGGER chat_scratch_promotion_audit_delete_denied
BEFORE DELETE ON chat_scratch_promotions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_channels WHERE id = OLD.destination_channel_id)
 AND EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
 AND (OLD.destination_working_folder_id IS NULL
      OR EXISTS (
          SELECT 1 FROM project_working_folders
          WHERE id = OLD.destination_working_folder_id
      ))
BEGIN
    SELECT RAISE(ABORT, 'Scratch promotion audit rows cannot be deleted');
END;

DROP TRIGGER chat_scratch_promotion_request_digest_required;
CREATE TRIGGER chat_scratch_promotion_request_digest_required
BEFORE INSERT ON chat_scratch_promotions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.request_digest IS NULL
BEGIN
    SELECT RAISE(ABORT, 'New scratch promotions require an idempotency digest');
END;

DROP TRIGGER chat_threads_execution_target_insert;
CREATE TRIGGER chat_threads_execution_target_insert
BEFORE INSERT ON chat_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT (
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

DROP TRIGGER chat_threads_project_matches_working_folder_insert;
CREATE TRIGGER chat_threads_project_matches_working_folder_insert
BEFORE INSERT ON chat_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.working_folder_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM project_working_folders
    WHERE id = NEW.working_folder_id AND project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread project must match its working folder');
END;

CREATE TEMP TABLE chat_identity_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_identity_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_identity_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

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

CREATE TRIGGER sync_immutable_chat_participants
BEFORE UPDATE OF id, participant_kind, created_at ON chat_participants
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.participant_kind IS NOT NEW.participant_kind COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
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

CREATE TRIGGER sync_capture_chat_participants_delete
AFTER DELETE ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (352, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_teammates_insert
BEFORE INSERT ON chat_teammates
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

CREATE TRIGGER sync_values_chat_teammates_update
BEFORE UPDATE OF created_at, role, instructions, current_policy_revision_id, current_policy_safety_mode, home_device_id, updated_at ON chat_teammates
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

CREATE TRIGGER sync_immutable_chat_teammates
BEFORE UPDATE OF participant_id, created_at ON chat_teammates
WHEN OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_teammates
BEFORE INSERT ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 353 AND row_key = NEW.participant_id)
    AND NOT EXISTS (SELECT 1 FROM chat_teammates WHERE participant_id = NEW.participant_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_teammates_insert
AFTER INSERT ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, NEW.participant_id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammates_update
AFTER UPDATE ON chat_teammates
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

CREATE TRIGGER sync_capture_chat_teammates_delete
AFTER DELETE ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (353, OLD.participant_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_teammate_policy_revisions_insert
BEFORE INSERT ON chat_teammate_policy_revisions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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

CREATE TRIGGER sync_values_chat_teammate_policy_revisions_update
BEFORE UPDATE OF teammate_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode ON chat_teammate_policy_revisions
WHEN ((NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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

CREATE TRIGGER sync_immutable_chat_teammate_policy_revisions
BEFORE UPDATE OF id, teammate_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode ON chat_teammate_policy_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.provider_instance_id IS NOT NEW.provider_instance_id COLLATE BINARY OR OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY OR OLD.effort IS NOT NEW.effort COLLATE BINARY OR OLD.speed IS NOT NEW.speed COLLATE BINARY OR OLD.provider_options_schema_version IS NOT NEW.provider_options_schema_version OR OLD.provider_options_data IS NOT NEW.provider_options_data COLLATE BINARY OR OLD.interaction_mode IS NOT NEW.interaction_mode COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.safety_mode IS NOT NEW.safety_mode COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_teammate_policy_revisions
BEFORE INSERT ON chat_teammate_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 354 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_teammate_policy_revisions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_teammate_policy_revisions_insert
AFTER INSERT ON chat_teammate_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (354, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_policy_revisions_update
AFTER UPDATE ON chat_teammate_policy_revisions
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

CREATE TRIGGER sync_capture_chat_teammate_policy_revisions_delete
AFTER DELETE ON chat_teammate_policy_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (354, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_access_profiles_insert
BEFORE INSERT ON chat_access_profiles
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.builtin_key IS NULL OR (typeof(NEW.builtin_key) = 'text' AND instr(CAST(NEW.builtin_key AS BLOB), x'00') = 0 AND length(NEW.builtin_key) <= 32 AND length(CAST(NEW.builtin_key AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (NEW.current_revision_id IS NULL OR (typeof(NEW.current_revision_id) = 'text' AND instr(CAST(NEW.current_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_revision_id) <> '' AND length(CAST(NEW.current_revision_id AS BLOB)) <= 128))
    AND (NEW.current_maximum_folder_capability IS NULL OR (typeof(NEW.current_maximum_folder_capability) = 'text' AND instr(CAST(NEW.current_maximum_folder_capability AS BLOB), x'00') = 0 AND length(NEW.current_maximum_folder_capability) <= 32 AND length(CAST(NEW.current_maximum_folder_capability AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_access_profiles_update
BEFORE UPDATE OF builtin_key, created_at, display_name, current_revision_id, current_maximum_folder_capability, archived_at, updated_at ON chat_access_profiles
WHEN ((NEW.builtin_key IS NULL OR (typeof(NEW.builtin_key) = 'text' AND instr(CAST(NEW.builtin_key AS BLOB), x'00') = 0 AND length(NEW.builtin_key) <= 32 AND length(CAST(NEW.builtin_key AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (NEW.current_revision_id IS NULL OR (typeof(NEW.current_revision_id) = 'text' AND instr(CAST(NEW.current_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_revision_id) <> '' AND length(CAST(NEW.current_revision_id AS BLOB)) <= 128))
    AND (NEW.current_maximum_folder_capability IS NULL OR (typeof(NEW.current_maximum_folder_capability) = 'text' AND instr(CAST(NEW.current_maximum_folder_capability AS BLOB), x'00') = 0 AND length(NEW.current_maximum_folder_capability) <= 32 AND length(CAST(NEW.current_maximum_folder_capability AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_access_profiles
BEFORE UPDATE OF id, builtin_key, created_at ON chat_access_profiles
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.builtin_key IS NOT NEW.builtin_key COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_access_profiles
BEFORE INSERT ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 355 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_access_profiles WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_access_profiles_insert
AFTER INSERT ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (355, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_access_profiles_update
AFTER UPDATE ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 355, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.current_revision_id IS NOT NEW.current_revision_id COLLATE BINARY OR OLD.current_maximum_folder_capability IS NOT NEW.current_maximum_folder_capability COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_access_profiles_delete
AFTER DELETE ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (355, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_access_profile_revisions_insert
BEFORE INSERT ON chat_access_profile_revisions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.default_read_history) = 'integer' AND NEW.default_read_history IN (0, 1))
    AND (typeof(NEW.default_participate) = 'integer' AND NEW.default_participate IN (0, 1))
    AND (typeof(NEW.default_history_boundary) = 'text' AND instr(CAST(NEW.default_history_boundary AS BLOB), x'00') = 0 AND length(NEW.default_history_boundary) <= 32 AND length(CAST(NEW.default_history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.maximum_folder_capability) = 'text' AND instr(CAST(NEW.maximum_folder_capability AS BLOB), x'00') = 0 AND length(NEW.maximum_folder_capability) <= 32 AND length(CAST(NEW.maximum_folder_capability AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_access_profile_revisions_update
BEFORE UPDATE OF access_profile_id, default_read_history, default_participate, default_history_boundary, maximum_folder_capability, created_at ON chat_access_profile_revisions
WHEN ((NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.default_read_history) = 'integer' AND NEW.default_read_history IN (0, 1))
    AND (typeof(NEW.default_participate) = 'integer' AND NEW.default_participate IN (0, 1))
    AND (typeof(NEW.default_history_boundary) = 'text' AND instr(CAST(NEW.default_history_boundary AS BLOB), x'00') = 0 AND length(NEW.default_history_boundary) <= 32 AND length(CAST(NEW.default_history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.maximum_folder_capability) = 'text' AND instr(CAST(NEW.maximum_folder_capability AS BLOB), x'00') = 0 AND length(NEW.maximum_folder_capability) <= 32 AND length(CAST(NEW.maximum_folder_capability AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_access_profile_revisions
BEFORE UPDATE OF id, access_profile_id, default_read_history, default_participate, default_history_boundary, maximum_folder_capability, created_at ON chat_access_profile_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.access_profile_id IS NOT NEW.access_profile_id COLLATE BINARY OR OLD.default_read_history IS NOT NEW.default_read_history OR OLD.default_participate IS NOT NEW.default_participate OR OLD.default_history_boundary IS NOT NEW.default_history_boundary COLLATE BINARY OR OLD.maximum_folder_capability IS NOT NEW.maximum_folder_capability COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_access_profile_revisions
BEFORE INSERT ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 356 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_access_profile_revisions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_access_profile_revisions_insert
AFTER INSERT ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (356, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_access_profile_revisions_update
AFTER UPDATE ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 356, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_access_profile_revisions_delete
AFTER DELETE ON chat_access_profile_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (356, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

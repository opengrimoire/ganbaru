-- no-transaction
-- Chat conversations replication: conversations, conversation memberships, AI channel
-- memberships, working folder grants, and channels join the vault manifest.
--
-- The three tables keyed by several columns gain a `sync_key` that holds a placeholder
-- 'pending:' || ROW_NUMBER() OVER (ORDER BY key columns) until the Chat conversion pass rewrites
-- it at vault open to the key every device derives from the key columns. Composite foreign keys
-- are replaced by one foreign key per replicated reference, and the guarantees they carried are
-- kept for local writes by triggers. Constraints the merge cannot keep unique (channel names,
-- one channel per conversation, one default channel per project, and one default folder grant
-- per AI membership) become local write triggers, and readers break ties deterministically.
-- Foreign keys from other tables into these tables cascade.
--
-- Default channels created before their ids were derived from the project id carry random ids
-- that would duplicate on every device. Those that hold nothing beyond the channel, the local
-- owner membership, and the audience state are removed here, and Chat recreates them on demand
-- with derived ids. Default channels that hold anything else keep their ids.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TEMP TABLE chat_empty_random_defaults AS
SELECT channel.id AS channel_id, channel.conversation_id
FROM chat_channels channel
WHERE channel.is_default = 1
    AND channel.conversation_id GLOB 'conversation:*'
    AND NOT EXISTS (
        SELECT 1 FROM chat_conversation_memberships membership
        WHERE membership.conversation_id = channel.conversation_id
            AND membership.participant_id <> 'participant:local-owner'
    )
    AND NOT EXISTS (SELECT 1 FROM chat_conversation_items item WHERE item.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_conversation_read_cursors cursor WHERE cursor.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_reply_threads thread WHERE thread.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_teammate_channel_memberships member WHERE member.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_teammate_working_folder_grants grant_row WHERE grant_row.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_access_revocation_jobs job WHERE job.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_assignment_authorization_revisions revision WHERE revision.destination_conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_assignment_authorized_channel_sources source WHERE source.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_scratch_generation_sources source WHERE source.conversation_id = channel.conversation_id)
    AND NOT EXISTS (SELECT 1 FROM chat_scratch_promotions promotion WHERE promotion.destination_channel_id = channel.id)
    AND NOT EXISTS (SELECT 1 FROM chat_scheduled_messages scheduled WHERE scheduled.channel_id = channel.id)
    AND NOT EXISTS (SELECT 1 FROM chat_scheduled_message_references reference WHERE reference.channel_id = channel.id)
    AND NOT EXISTS (
        SELECT 1 FROM chat_channel_reference_targets target
        WHERE target.channel_id = channel.id
            OR target.source_conversation_id = channel.conversation_id
            OR target.destination_conversation_id = channel.conversation_id
    )
    AND NOT EXISTS (
        SELECT 1 FROM chat_channels other
        WHERE other.conversation_id = channel.conversation_id AND other.id <> channel.id
    );

DELETE FROM chat_conversation_memberships
WHERE conversation_id IN (SELECT conversation_id FROM chat_empty_random_defaults);
DELETE FROM chat_conversation_audience_state
WHERE conversation_id IN (SELECT conversation_id FROM chat_empty_random_defaults);
DELETE FROM chat_channels
WHERE id IN (SELECT channel_id FROM chat_empty_random_defaults);
DELETE FROM chat_conversations
WHERE id IN (SELECT conversation_id FROM chat_empty_random_defaults);
DROP TABLE chat_empty_random_defaults;

CREATE TABLE chat_conversation_memberships_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
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
    conversation_id, participant_id, sync_key, membership_role, revision, removed_at,
    created_at, updated_at
)
SELECT
    conversation_id,
    participant_id,
    'pending:' || ROW_NUMBER() OVER (ORDER BY conversation_id, participant_id),
    membership_role,
    revision,
    removed_at,
    created_at,
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

CREATE TABLE chat_teammate_channel_memberships_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    access_profile_id TEXT NOT NULL REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
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
    CHECK (
        (history_boundary = 'entire' AND history_from_ordinal IS NULL)
        OR (history_boundary = 'from_grant' AND history_from_ordinal IS NOT NULL)
    )
) STRICT, WITHOUT ROWID;

INSERT INTO chat_teammate_channel_memberships_next (
    conversation_id, teammate_id, sync_key, access_profile_id,
    read_history, read_history_inherits_profile, participate, participate_inherits_profile,
    history_boundary, history_boundary_inherits_profile, history_from_ordinal,
    runtime_approval_policy, scratch_runtime_approval_policy, created_at, updated_at, removed_at
)
SELECT
    conversation_id,
    teammate_id,
    'pending:' || ROW_NUMBER() OVER (ORDER BY conversation_id, teammate_id),
    access_profile_id,
    read_history,
    read_history_inherits_profile,
    participate,
    participate_inherits_profile,
    history_boundary,
    history_boundary_inherits_profile,
    history_from_ordinal,
    runtime_approval_policy,
    scratch_runtime_approval_policy,
    created_at,
    updated_at,
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

CREATE TRIGGER chat_teammate_channel_membership_requires_membership_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1 FROM chat_conversation_memberships membership
    WHERE membership.conversation_id = NEW.conversation_id
      AND membership.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires a conversation membership');
END;

CREATE TABLE chat_teammate_working_folder_grants_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 1024),
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
    PRIMARY KEY (conversation_id, teammate_id, working_folder_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_teammate_working_folder_grants_next (
    conversation_id, teammate_id, working_folder_id, sync_key, project_id, capability,
    capability_inherits_profile, is_default, runtime_approval_policy, revision, created_at,
    revoked_at
)
SELECT
    conversation_id,
    teammate_id,
    working_folder_id,
    'pending:' || ROW_NUMBER() OVER (
        ORDER BY conversation_id, teammate_id, working_folder_id
    ),
    project_id,
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

CREATE INDEX idx_chat_teammate_working_folder_grants_folder
ON chat_teammate_working_folder_grants(working_folder_id);

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

CREATE TRIGGER chat_teammate_folder_grant_project_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
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

CREATE TRIGGER chat_teammate_folder_grant_project_update
BEFORE UPDATE OF project_id ON chat_teammate_working_folder_grants
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

CREATE TRIGGER chat_teammate_folder_grant_single_default_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.revoked_at IS NULL
 AND EXISTS (
    SELECT 1 FROM chat_teammate_working_folder_grants other
    WHERE other.conversation_id = NEW.conversation_id
      AND other.teammate_id = NEW.teammate_id
      AND other.working_folder_id <> NEW.working_folder_id
      AND other.is_default = 1 AND other.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'An AI channel membership has one default working folder');
END;

CREATE TRIGGER chat_teammate_folder_grant_single_default_update
BEFORE UPDATE OF is_default, revoked_at ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.revoked_at IS NULL
 AND EXISTS (
    SELECT 1 FROM chat_teammate_working_folder_grants other
    WHERE other.conversation_id = NEW.conversation_id
      AND other.teammate_id = NEW.teammate_id
      AND other.working_folder_id <> NEW.working_folder_id
      AND other.is_default = 1 AND other.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'An AI channel membership has one default working folder');
END;

CREATE TABLE chat_channels_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    project_id TEXT NOT NULL REFERENCES projects(id) ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    topic TEXT NOT NULL DEFAULT '' CHECK (length(topic) <= 250),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_channels_next (
    id, project_id, conversation_id, name, topic, is_default, revision, archived_at,
    created_at, updated_at
)
SELECT
    id, project_id, conversation_id, name, topic, is_default, revision, archived_at,
    created_at, updated_at
FROM chat_channels;

DROP TABLE chat_channels;
ALTER TABLE chat_channels_next RENAME TO chat_channels;

CREATE INDEX idx_chat_channels_active_project
ON chat_channels(project_id, archived_at, updated_at DESC, id);

CREATE INDEX idx_chat_channels_project_default
ON chat_channels(project_id, is_default, id);

CREATE INDEX idx_chat_channels_conversation
ON chat_channels(conversation_id, id);

CREATE TRIGGER chat_channels_single_conversation_insert
BEFORE INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (
    SELECT 1 FROM chat_channels other WHERE other.conversation_id = NEW.conversation_id
)
BEGIN
    SELECT RAISE(ABORT, 'A Chat conversation has one channel');
END;

CREATE TRIGGER chat_channels_conversation_kind_insert
BEFORE INSERT ON chat_channels
WHEN NOT EXISTS (
    SELECT 1 FROM chat_conversations conversation
    WHERE conversation.id = NEW.conversation_id
      AND conversation.project_id = NEW.project_id
      AND conversation.conversation_kind = 'channel'
)
BEGIN
    SELECT RAISE(ABORT, 'Chat channel conversation is invalid');
END;

CREATE TRIGGER chat_channels_default_name_insert
BEFORE INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND NEW.name != 'general'
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel must be named general');
END;

CREATE TRIGGER chat_channels_single_default_insert
BEFORE INSERT ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.is_default = 1 AND EXISTS (
    SELECT 1 FROM chat_channels other
    WHERE other.project_id = NEW.project_id AND other.is_default = 1
)
BEGIN
    SELECT RAISE(ABORT, 'A project has one default Chat channel');
END;

CREATE TRIGGER chat_channels_default_protected_update
BEFORE UPDATE OF name, is_default, archived_at ON chat_channels
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND OLD.is_default = 1 AND (
    NEW.name != OLD.name OR NEW.is_default != 1 OR NEW.archived_at IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel is protected');
END;

CREATE TABLE chat_channel_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_lower_ordinal INTEGER NOT NULL CHECK (source_lower_ordinal >= 0),
    source_high_ordinal INTEGER NOT NULL CHECK (source_high_ordinal >= source_lower_ordinal),
    source_revision_cutoff_id TEXT REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_audience_revision INTEGER NOT NULL CHECK (destination_audience_revision >= 1),
    CHECK (
        (source_high_ordinal = 0 AND source_revision_cutoff_id IS NULL)
        OR (source_high_ordinal > 0 AND source_revision_cutoff_id IS NOT NULL)
    )
) STRICT;

INSERT INTO chat_channel_reference_targets_next (
    reference_id, channel_id, source_conversation_id, destination_conversation_id,
    source_lower_ordinal, source_high_ordinal, source_revision_cutoff_id,
    destination_audience_revision
)
SELECT
    reference_id, channel_id, source_conversation_id, destination_conversation_id,
    source_lower_ordinal, source_high_ordinal, source_revision_cutoff_id,
    destination_audience_revision
FROM chat_channel_reference_targets;

DROP TABLE chat_channel_reference_targets;
ALTER TABLE chat_channel_reference_targets_next RENAME TO chat_channel_reference_targets;

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
        ON UPDATE CASCADE ON DELETE CASCADE,
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

CREATE TEMP TABLE chat_conversations_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_conversations_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_conversations_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.


CREATE TRIGGER sync_values_chat_conversations_insert
BEFORE INSERT ON chat_conversations
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.conversation_kind) = 'text' AND instr(CAST(NEW.conversation_kind AS BLOB), x'00') = 0 AND length(NEW.conversation_kind) <= 32 AND length(CAST(NEW.conversation_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.last_activity_at) = 'text' AND instr(CAST(NEW.last_activity_at AS BLOB), x'00') = 0 AND trim(NEW.last_activity_at) <> '' AND length(CAST(NEW.last_activity_at AS BLOB)) <= 64)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_conversations_update
BEFORE UPDATE OF project_id, conversation_kind, created_at, last_activity_at, archived_at, updated_at ON chat_conversations
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.conversation_kind) = 'text' AND instr(CAST(NEW.conversation_kind AS BLOB), x'00') = 0 AND length(NEW.conversation_kind) <= 32 AND length(CAST(NEW.conversation_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.last_activity_at) = 'text' AND instr(CAST(NEW.last_activity_at AS BLOB), x'00') = 0 AND trim(NEW.last_activity_at) <> '' AND length(CAST(NEW.last_activity_at AS BLOB)) <= 64)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_conversations
BEFORE UPDATE OF id, project_id, conversation_kind, created_at ON chat_conversations
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.conversation_kind IS NOT NEW.conversation_kind COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_conversations
BEFORE INSERT ON chat_conversations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 360 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_conversations WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_conversations_insert
AFTER INSERT ON chat_conversations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (360, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversations_update
AFTER UPDATE ON chat_conversations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 360, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.last_activity_at IS NOT NEW.last_activity_at COLLATE BINARY
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

CREATE TRIGGER sync_capture_chat_conversations_delete
AFTER DELETE ON chat_conversations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (360, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_conversation_memberships_insert
BEFORE INSERT ON chat_conversation_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.membership_role) = 'text' AND instr(CAST(NEW.membership_role AS BLOB), x'00') = 0 AND length(NEW.membership_role) <= 32 AND length(CAST(NEW.membership_role AS BLOB)) <= 262144)
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_conversation_memberships_update
BEFORE UPDATE OF conversation_id, participant_id, membership_role, removed_at, created_at, updated_at ON chat_conversation_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.membership_role) = 'text' AND instr(CAST(NEW.membership_role AS BLOB), x'00') = 0 AND length(NEW.membership_role) <= 32 AND length(CAST(NEW.membership_role AS BLOB)) <= 262144)
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_conversation_memberships
BEFORE UPDATE OF sync_key, conversation_id, participant_id ON chat_conversation_memberships
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_conversation_memberships_insert
AFTER INSERT ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (361, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversation_memberships_update
AFTER UPDATE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 361, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.membership_role IS NOT NEW.membership_role COLLATE BINARY OR OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
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
    SELECT 361, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversation_memberships_delete
AFTER DELETE ON chat_conversation_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (361, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_teammate_channel_memberships_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_ordinal IS NULL OR (typeof(NEW.history_from_ordinal) = 'integer' AND NEW.history_from_ordinal BETWEEN 1 AND 9223372036854775807))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_teammate_channel_memberships_update
BEFORE UPDATE OF conversation_id, teammate_id, access_profile_id, read_history, read_history_inherits_profile, participate, participate_inherits_profile, history_boundary, history_boundary_inherits_profile, history_from_ordinal, runtime_approval_policy, scratch_runtime_approval_policy, removed_at, created_at, updated_at ON chat_teammate_channel_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_ordinal IS NULL OR (typeof(NEW.history_from_ordinal) = 'integer' AND NEW.history_from_ordinal BETWEEN 1 AND 9223372036854775807))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_teammate_channel_memberships
BEFORE UPDATE OF sync_key, conversation_id, teammate_id ON chat_teammate_channel_memberships
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_insert
AFTER INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_update
AFTER UPDATE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.access_profile_id IS NOT NEW.access_profile_id COLLATE BINARY
                AND NOT (NEW.access_profile_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_access_profiles WHERE id = OLD.access_profile_id))) OR OLD.read_history IS NOT NEW.read_history OR OLD.read_history_inherits_profile IS NOT NEW.read_history_inherits_profile OR OLD.participate IS NOT NEW.participate OR OLD.participate_inherits_profile IS NOT NEW.participate_inherits_profile OR OLD.history_boundary IS NOT NEW.history_boundary COLLATE BINARY OR OLD.history_boundary_inherits_profile IS NOT NEW.history_boundary_inherits_profile OR OLD.history_from_ordinal IS NOT NEW.history_from_ordinal OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.scratch_runtime_approval_policy IS NOT NEW.scratch_runtime_approval_policy COLLATE BINARY OR OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
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

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_teammate_working_folder_grants_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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

CREATE TRIGGER sync_values_chat_teammate_working_folder_grants_update
BEFORE UPDATE OF conversation_id, teammate_id, working_folder_id, capability, capability_inherits_profile, is_default, runtime_approval_policy, revoked_at, project_id, created_at ON chat_teammate_working_folder_grants
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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

CREATE TRIGGER sync_immutable_chat_teammate_working_folder_grants
BEFORE UPDATE OF sync_key, conversation_id, teammate_id, working_folder_id ON chat_teammate_working_folder_grants
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_teammate_working_folder_grants_insert
AFTER INSERT ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (363, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_working_folder_grants_update
AFTER UPDATE ON chat_teammate_working_folder_grants
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

CREATE TRIGGER sync_capture_chat_teammate_working_folder_grants_delete
AFTER DELETE ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (363, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_channels_update
BEFORE UPDATE OF project_id, conversation_id, is_default, created_at, name, topic, archived_at, updated_at ON chat_channels
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (typeof(NEW.is_default) = 'integer' AND NEW.is_default IN (0, 1))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.topic) = 'text' AND instr(CAST(NEW.topic AS BLOB), x'00') = 0 AND length(NEW.topic) <= 250 AND length(CAST(NEW.topic AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
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
    VALUES (364, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

-- no-transaction
-- Chat assignments replication: scheduled messages, work assignments, assignment inputs, and
-- semantic updates join the vault manifest.
--
-- Scheduled messages name the device that sends them (`executing_device_id`); a scheduled
-- message without one is sent by the device holding its local dispatch row. Claims, attempts,
-- and the retry time move to the local `chat_scheduled_message_dispatch` table, and a failure
-- keeps only `failed_at` and `last_error`. Scheduled message references become rows owned by
-- their scheduled message, keyed by ordinal like its attachment references, and a removed
-- participant now removes the reference instead of clearing it. Assignments name their
-- executing device as well, and a removed teammate removes its assignments. Existing scheduled
-- messages and assignments take the device of the vault's local sync writer when one is
-- recorded, so a vault that already syncs never has two devices sending the same row; a vault
-- that never synced has only the migrating device. The uniques two
-- devices could break concurrently (one active assignment per reply thread, one input per
-- message and per ordinal) become plain indexes: input ids derive from their message, and
-- readers order concurrent rows deterministically. The scheduled client command id stays
-- unique and is repaired as a duplicate, which random command ids make unreachable.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- chat_scheduled_message_dispatch: every scheduled message on this device keeps its claim and
-- retry state here.
INSERT INTO chat_scheduled_message_dispatch (
    scheduled_message_id, attempt_count, available_at, claimed_at, claim_token, updated_at
)
SELECT
    id, attempt_count, available_at,
    CASE WHEN claim_token IS NULL THEN NULL ELSE coalesce(claimed_at, updated_at) END,
    claim_token,
    updated_at
FROM chat_scheduled_messages
WHERE true
ON CONFLICT (scheduled_message_id) DO NOTHING;

-- chat_scheduled_messages
CREATE TABLE chat_scheduled_messages_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    client_command_id TEXT NOT NULL UNIQUE CHECK (length(client_command_id) BETWEEN 1 AND 1024),
    channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reply_thread_id TEXT REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    executing_device_id TEXT CHECK (
        executing_device_id IS NULL
        OR length(CAST(executing_device_id AS BLOB)) BETWEEN 1 AND 160
    ),
    request_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (request_schema_version = 1),
    request_data TEXT NOT NULL CHECK (
        json_valid(request_data) AND json_type(request_data) = 'object'
    ),
    scheduled_for TEXT NOT NULL CHECK (length(scheduled_for) >= 20),
    failed_at TEXT CHECK (failed_at IS NULL OR length(failed_at) >= 20),
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_scheduled_messages_next (
    id, client_command_id, channel_id, reply_thread_id, executing_device_id,
    request_schema_version, request_data, scheduled_for, failed_at, last_error, created_at,
    updated_at
)
SELECT
    id, client_command_id, channel_id, reply_thread_id,
    (SELECT local_device_id FROM sync_spaces WHERE local_device_id IS NOT NULL
     ORDER BY space_id LIMIT 1),
    request_schema_version, request_data, scheduled_for,
    CASE WHEN state = 'failed' THEN updated_at END, last_error, created_at, updated_at
FROM chat_scheduled_messages;

DROP TABLE chat_scheduled_messages;
ALTER TABLE chat_scheduled_messages_next RENAME TO chat_scheduled_messages;

CREATE INDEX idx_chat_scheduled_messages_destination
ON chat_scheduled_messages(channel_id, reply_thread_id, scheduled_for, id);
CREATE INDEX idx_chat_scheduled_messages_device
ON chat_scheduled_messages(executing_device_id, scheduled_for, id);

CREATE TRIGGER chat_scheduled_messages_reply_thread_insert
BEFORE INSERT ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.reply_thread_id IS NOT NULL AND NOT EXISTS (
    SELECT 1
    FROM chat_reply_threads thread
    JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
    WHERE thread.id = NEW.reply_thread_id
      AND channel.id = NEW.channel_id
)
BEGIN
    SELECT RAISE(ABORT, 'Scheduled reply belongs to another channel');
END;

-- chat_scheduled_message_references: rows owned by their scheduled message. The Chat adapter
-- checks the reference kinds, labels, and the target each kind needs before a value applies.
CREATE TABLE chat_scheduled_message_references_next (
    scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 255),
    reference_kind TEXT NOT NULL,
    label_snapshot TEXT NOT NULL,
    plain_text_projection TEXT NOT NULL,
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset >= 1),
    participant_id TEXT REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_kind TEXT,
    channel_id TEXT REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    path_kind TEXT,
    relative_path TEXT CHECK (
        relative_path IS NULL OR (
            length(relative_path) <= 4096
            AND relative_path NOT LIKE '/%'
            AND relative_path NOT LIKE '../%'
            AND relative_path NOT LIKE '%/../%'
            AND relative_path NOT LIKE '%/..'
            AND relative_path NOT LIKE '%\%'
        )
    ),
    execution_environment_id TEXT CHECK (
        execution_environment_id IS NULL OR length(execution_environment_id) <= 2048
    ),
    PRIMARY KEY (scheduled_message_id, ordinal)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_scheduled_message_references_next (
    scheduled_message_id, ordinal, reference_kind, label_snapshot, plain_text_projection,
    start_offset, end_offset, participant_id, participant_kind, channel_id, working_folder_id,
    path_kind, relative_path, execution_environment_id
)
SELECT
    scheduled_message_id,
    row_number() OVER (PARTITION BY scheduled_message_id ORDER BY ordinal, id) - 1,
    reference_kind, label_snapshot, plain_text_projection, start_offset, end_offset,
    participant_id, participant_kind, channel_id, working_folder_id, path_kind, relative_path,
    execution_environment_id
FROM chat_scheduled_message_references;

DROP TABLE chat_scheduled_message_references;
ALTER TABLE chat_scheduled_message_references_next
RENAME TO chat_scheduled_message_references;

CREATE INDEX idx_chat_scheduled_message_references_folder
ON chat_scheduled_message_references(working_folder_id);

-- chat_work_assignments
CREATE TABLE chat_work_assignments_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    executing_device_id TEXT CHECK (
        executing_device_id IS NULL
        OR length(CAST(executing_device_id AS BLOB)) BETWEEN 1 AND 160
    ),
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
    id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id,
    previous_assignment_id, state, state_reason, revision, settled_at, created_at, updated_at
)
SELECT
    id, reply_thread_id, teammate_id, triggering_message_item_id,
    (SELECT local_device_id FROM sync_spaces WHERE local_device_id IS NOT NULL
     ORDER BY space_id LIMIT 1),
    previous_assignment_id, state, state_reason, revision, settled_at, created_at, updated_at
FROM chat_work_assignments;

DROP TABLE chat_work_assignments;
ALTER TABLE chat_work_assignments_next RENAME TO chat_work_assignments;

CREATE INDEX idx_chat_work_assignments_thread
ON chat_work_assignments(reply_thread_id, state, created_at, id);
CREATE INDEX idx_chat_work_assignments_teammate
ON chat_work_assignments(teammate_id, state, updated_at DESC, id);
CREATE INDEX idx_chat_work_assignments_trigger
ON chat_work_assignments(triggering_message_item_id);

-- chat_work_assignment_inputs
CREATE TABLE chat_work_assignment_inputs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    routing_kind TEXT NOT NULL CHECK (
        routing_kind IN ('trigger', 'steer', 'queued_continuation', 'follow_up')
    ),
    delivery_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        delivery_state IN ('pending', 'delivered', 'failed', 'ignored')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    delivered_at TEXT CHECK (delivered_at IS NULL OR length(delivered_at) >= 20)
) STRICT;

INSERT INTO chat_work_assignment_inputs_next (
    id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state, created_at,
    delivered_at
)
SELECT
    id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state, created_at,
    delivered_at
FROM chat_work_assignment_inputs;

DROP TABLE chat_work_assignment_inputs;
ALTER TABLE chat_work_assignment_inputs_next RENAME TO chat_work_assignment_inputs;

CREATE INDEX idx_chat_work_assignment_inputs_assignment
ON chat_work_assignment_inputs(assignment_id, ordinal, id);
CREATE INDEX idx_chat_work_assignment_inputs_message
ON chat_work_assignment_inputs(message_item_id);

CREATE INDEX idx_chat_work_semantic_updates_assignment
ON chat_work_semantic_updates(assignment_id);

CREATE TEMP TABLE chat_assignments_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_assignments_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_assignments_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.

CREATE TRIGGER sync_values_chat_scheduled_messages_insert
BEFORE INSERT ON chat_scheduled_messages
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.client_command_id) = 'text' AND instr(CAST(NEW.client_command_id AS BLOB), x'00') = 0 AND length(NEW.client_command_id) <= 1024 AND length(CAST(NEW.client_command_id AS BLOB)) <= 262144)
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.request_schema_version) = 'integer' AND NEW.request_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.request_data) = 'text' AND instr(CAST(NEW.request_data AS BLOB), x'00') = 0 AND length(NEW.request_data) <= 262144 AND length(CAST(NEW.request_data AS BLOB)) <= 262144)
    AND (typeof(NEW.scheduled_for) = 'text' AND instr(CAST(NEW.scheduled_for AS BLOB), x'00') = 0 AND trim(NEW.scheduled_for) <> '' AND length(CAST(NEW.scheduled_for AS BLOB)) <= 64)
    AND (NEW.failed_at IS NULL OR (typeof(NEW.failed_at) = 'text' AND instr(CAST(NEW.failed_at AS BLOB), x'00') = 0 AND trim(NEW.failed_at) <> '' AND length(CAST(NEW.failed_at AS BLOB)) <= 64))
    AND (NEW.last_error IS NULL OR (typeof(NEW.last_error) = 'text' AND instr(CAST(NEW.last_error AS BLOB), x'00') = 0 AND length(NEW.last_error) <= 4000 AND length(CAST(NEW.last_error AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_scheduled_messages_update
BEFORE UPDATE OF client_command_id, channel_id, reply_thread_id, executing_device_id, created_at, request_schema_version, request_data, scheduled_for, failed_at, last_error, updated_at ON chat_scheduled_messages
WHEN ((typeof(NEW.client_command_id) = 'text' AND instr(CAST(NEW.client_command_id AS BLOB), x'00') = 0 AND length(NEW.client_command_id) <= 1024 AND length(CAST(NEW.client_command_id AS BLOB)) <= 262144)
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.request_schema_version) = 'integer' AND NEW.request_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.request_data) = 'text' AND instr(CAST(NEW.request_data AS BLOB), x'00') = 0 AND length(NEW.request_data) <= 262144 AND length(CAST(NEW.request_data AS BLOB)) <= 262144)
    AND (typeof(NEW.scheduled_for) = 'text' AND instr(CAST(NEW.scheduled_for AS BLOB), x'00') = 0 AND trim(NEW.scheduled_for) <> '' AND length(CAST(NEW.scheduled_for AS BLOB)) <= 64)
    AND (NEW.failed_at IS NULL OR (typeof(NEW.failed_at) = 'text' AND instr(CAST(NEW.failed_at AS BLOB), x'00') = 0 AND trim(NEW.failed_at) <> '' AND length(CAST(NEW.failed_at AS BLOB)) <= 64))
    AND (NEW.last_error IS NULL OR (typeof(NEW.last_error) = 'text' AND instr(CAST(NEW.last_error AS BLOB), x'00') = 0 AND length(NEW.last_error) <= 4000 AND length(CAST(NEW.last_error AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_scheduled_messages
BEFORE UPDATE OF id, client_command_id, channel_id, reply_thread_id, executing_device_id, created_at ON chat_scheduled_messages
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.client_command_id IS NOT NEW.client_command_id COLLATE BINARY OR OLD.channel_id IS NOT NEW.channel_id COLLATE BINARY OR OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_scheduled_messages
BEFORE INSERT ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 390 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_scheduled_messages WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_scheduled_messages_insert
AFTER INSERT ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_scheduled_messages_update
AFTER UPDATE ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 390, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.request_schema_version IS NOT NEW.request_schema_version OR OLD.request_data IS NOT NEW.request_data COLLATE BINARY OR OLD.scheduled_for IS NOT NEW.scheduled_for COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.failed_at IS NOT NEW.failed_at COLLATE BINARY OR OLD.last_error IS NOT NEW.last_error COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_messages_delete
AFTER DELETE ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_attachment_references_insert
AFTER INSERT ON chat_scheduled_message_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, NEW.scheduled_message_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_attachment_references_update
AFTER UPDATE ON chat_scheduled_message_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 390, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.scheduled_message_id AS key UNION SELECT OLD.scheduled_message_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_attachment_references_delete
AFTER DELETE ON chat_scheduled_message_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, OLD.scheduled_message_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_chat_scheduled_message_references_insert
BEFORE INSERT ON chat_scheduled_message_references
WHEN ((typeof(NEW.reference_kind) = 'text' AND instr(CAST(NEW.reference_kind AS BLOB), x'00') = 0 AND length(NEW.reference_kind) <= 32 AND length(CAST(NEW.reference_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.label_snapshot) = 'text' AND instr(CAST(NEW.label_snapshot AS BLOB), x'00') = 0 AND length(NEW.label_snapshot) <= 4096 AND length(CAST(NEW.label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text_projection) = 'text' AND instr(CAST(NEW.plain_text_projection AS BLOB), x'00') = 0 AND length(NEW.plain_text_projection) <= 4096 AND length(CAST(NEW.plain_text_projection AS BLOB)) <= 262144)
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (NEW.participant_kind IS NULL OR (typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144))
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (NEW.path_kind IS NULL OR (typeof(NEW.path_kind) = 'text' AND instr(CAST(NEW.path_kind AS BLOB), x'00') = 0 AND length(NEW.path_kind) <= 32 AND length(CAST(NEW.path_kind AS BLOB)) <= 262144))
    AND (NEW.relative_path IS NULL OR (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(NEW.relative_path) <= 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144))
    AND (NEW.execution_environment_id IS NULL OR (typeof(NEW.execution_environment_id) = 'text' AND instr(CAST(NEW.execution_environment_id AS BLOB), x'00') = 0 AND length(NEW.execution_environment_id) <= 2048 AND length(CAST(NEW.execution_environment_id AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_scheduled_message_references_update
BEFORE UPDATE OF reference_kind, label_snapshot, plain_text_projection, start_offset, end_offset, participant_id, participant_kind, channel_id, working_folder_id, path_kind, relative_path, execution_environment_id ON chat_scheduled_message_references
WHEN ((typeof(NEW.reference_kind) = 'text' AND instr(CAST(NEW.reference_kind AS BLOB), x'00') = 0 AND length(NEW.reference_kind) <= 32 AND length(CAST(NEW.reference_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.label_snapshot) = 'text' AND instr(CAST(NEW.label_snapshot AS BLOB), x'00') = 0 AND length(NEW.label_snapshot) <= 4096 AND length(CAST(NEW.label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text_projection) = 'text' AND instr(CAST(NEW.plain_text_projection AS BLOB), x'00') = 0 AND length(NEW.plain_text_projection) <= 4096 AND length(CAST(NEW.plain_text_projection AS BLOB)) <= 262144)
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (NEW.participant_kind IS NULL OR (typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144))
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (NEW.path_kind IS NULL OR (typeof(NEW.path_kind) = 'text' AND instr(CAST(NEW.path_kind AS BLOB), x'00') = 0 AND length(NEW.path_kind) <= 32 AND length(CAST(NEW.path_kind AS BLOB)) <= 262144))
    AND (NEW.relative_path IS NULL OR (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(NEW.relative_path) <= 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144))
    AND (NEW.execution_environment_id IS NULL OR (typeof(NEW.execution_environment_id) = 'text' AND instr(CAST(NEW.execution_environment_id AS BLOB), x'00') = 0 AND length(NEW.execution_environment_id) <= 2048 AND length(CAST(NEW.execution_environment_id AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_references_insert
AFTER INSERT ON chat_scheduled_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, NEW.scheduled_message_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_references_update
AFTER UPDATE ON chat_scheduled_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 390, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.scheduled_message_id AS key UNION SELECT OLD.scheduled_message_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_references_delete
AFTER DELETE ON chat_scheduled_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (OLD.participant_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.participant_id))
    AND NOT (OLD.channel_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM chat_channels WHERE id = OLD.channel_id))
    AND NOT (OLD.working_folder_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM project_working_folders WHERE id = OLD.working_folder_id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (390, OLD.scheduled_message_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_chat_work_assignments_insert
BEFORE INSERT ON chat_work_assignments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.previous_assignment_id IS NULL OR (typeof(NEW.previous_assignment_id) = 'text' AND instr(CAST(NEW.previous_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.previous_assignment_id) <> '' AND length(CAST(NEW.previous_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.state_reason IS NULL OR (typeof(NEW.state_reason) = 'text' AND instr(CAST(NEW.state_reason AS BLOB), x'00') = 0 AND length(NEW.state_reason) <= 4000 AND length(CAST(NEW.state_reason AS BLOB)) <= 262144))
    AND (NEW.settled_at IS NULL OR (typeof(NEW.settled_at) = 'text' AND instr(CAST(NEW.settled_at AS BLOB), x'00') = 0 AND trim(NEW.settled_at) <> '' AND length(CAST(NEW.settled_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, previous_assignment_id, state, state_reason, settled_at, updated_at ON chat_work_assignments
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.previous_assignment_id IS NULL OR (typeof(NEW.previous_assignment_id) = 'text' AND instr(CAST(NEW.previous_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.previous_assignment_id) <> '' AND length(CAST(NEW.previous_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.state_reason IS NULL OR (typeof(NEW.state_reason) = 'text' AND instr(CAST(NEW.state_reason AS BLOB), x'00') = 0 AND length(NEW.state_reason) <= 4000 AND length(CAST(NEW.state_reason AS BLOB)) <= 262144))
    AND (NEW.settled_at IS NULL OR (typeof(NEW.settled_at) = 'text' AND instr(CAST(NEW.settled_at AS BLOB), x'00') = 0 AND trim(NEW.settled_at) <> '' AND length(CAST(NEW.settled_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_assignments
BEFORE UPDATE OF id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at ON chat_work_assignments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.triggering_message_item_id IS NOT NEW.triggering_message_item_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
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
    VALUES (395, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
            | (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.state_reason IS NOT NEW.state_reason COLLATE BINARY OR OLD.settled_at IS NOT NEW.settled_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
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

CREATE TRIGGER sync_values_chat_work_assignment_inputs_insert
BEFORE INSERT ON chat_work_assignment_inputs
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.routing_kind) = 'text' AND instr(CAST(NEW.routing_kind AS BLOB), x'00') = 0 AND length(NEW.routing_kind) <= 32 AND length(CAST(NEW.routing_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.delivery_state) = 'text' AND instr(CAST(NEW.delivery_state AS BLOB), x'00') = 0 AND length(NEW.delivery_state) <= 32 AND length(CAST(NEW.delivery_state AS BLOB)) <= 262144)
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignment_inputs_update
BEFORE UPDATE OF message_item_id, created_at, assignment_id, ordinal, routing_kind, delivery_state, delivered_at ON chat_work_assignment_inputs
WHEN ((NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.routing_kind) = 'text' AND instr(CAST(NEW.routing_kind AS BLOB), x'00') = 0 AND length(NEW.routing_kind) <= 32 AND length(CAST(NEW.routing_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.delivery_state) = 'text' AND instr(CAST(NEW.delivery_state AS BLOB), x'00') = 0 AND length(NEW.delivery_state) <= 32 AND length(CAST(NEW.delivery_state AS BLOB)) <= 262144)
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_assignment_inputs
BEFORE UPDATE OF id, message_item_id ON chat_work_assignment_inputs
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.message_item_id IS NOT NEW.message_item_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_work_assignment_inputs_insert
AFTER INSERT ON chat_work_assignment_inputs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (396, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_work_assignment_inputs_update
AFTER UPDATE ON chat_work_assignment_inputs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 396, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN (OLD.assignment_id IS NOT NEW.assignment_id COLLATE BINARY
                AND NOT (NEW.assignment_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id))) OR OLD.ordinal IS NOT NEW.ordinal OR OLD.routing_kind IS NOT NEW.routing_kind COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.delivery_state IS NOT NEW.delivery_state COLLATE BINARY OR OLD.delivered_at IS NOT NEW.delivered_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_work_assignment_inputs_delete
AFTER DELETE ON chat_work_assignment_inputs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (396, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_work_semantic_updates_insert
BEFORE INSERT ON chat_work_semantic_updates
WHEN ((typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.update_kind) = 'text' AND instr(CAST(NEW.update_kind AS BLOB), x'00') = 0 AND length(NEW.update_kind) <= 32 AND length(CAST(NEW.update_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.payload_schema_version) = 'integer' AND NEW.payload_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.payload_data) = 'text' AND instr(CAST(NEW.payload_data AS BLOB), x'00') = 0 AND length(NEW.payload_data) <= 196608 AND length(CAST(NEW.payload_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_semantic_updates_update
BEFORE UPDATE OF assignment_id, update_kind, payload_schema_version, payload_data, created_at ON chat_work_semantic_updates
WHEN ((NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.update_kind) = 'text' AND instr(CAST(NEW.update_kind AS BLOB), x'00') = 0 AND length(NEW.update_kind) <= 32 AND length(CAST(NEW.update_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.payload_schema_version) = 'integer' AND NEW.payload_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.payload_data) = 'text' AND instr(CAST(NEW.payload_data AS BLOB), x'00') = 0 AND length(NEW.payload_data) <= 196608 AND length(CAST(NEW.payload_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_semantic_updates
BEFORE UPDATE OF item_id, assignment_id, update_kind, payload_schema_version, payload_data, created_at ON chat_work_semantic_updates
WHEN OLD.item_id IS NOT NEW.item_id COLLATE BINARY
    OR ((OLD.assignment_id IS NOT NEW.assignment_id COLLATE BINARY OR OLD.update_kind IS NOT NEW.update_kind COLLATE BINARY OR OLD.payload_schema_version IS NOT NEW.payload_schema_version OR OLD.payload_data IS NOT NEW.payload_data COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_work_semantic_updates
BEFORE INSERT ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 397 AND row_key = NEW.item_id)
    AND NOT EXISTS (SELECT 1 FROM chat_work_semantic_updates WHERE item_id = NEW.item_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_insert
AFTER INSERT ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (397, NEW.item_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_update
AFTER UPDATE ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 397, NEW.item_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_delete
AFTER DELETE ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (397, OLD.item_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

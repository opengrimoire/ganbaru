-- Fix the model a person asked for with the assignment it starts, and record which revision of
-- an input message reached the run. The override is its selection schema version and data
-- together, or neither when the assignment runs with its teammate's model; the data column's
-- CHECK holds the pairing, since it is added after the version column it names.
-- The delivered revision has no foreign key, like the message's current revision, since a
-- revision row may arrive through sync after the delivery that names it.
ALTER TABLE chat_work_assignments ADD COLUMN override_model_selection_schema_version INTEGER CHECK (
    override_model_selection_schema_version IS NULL OR override_model_selection_schema_version >= 1
);
ALTER TABLE chat_work_assignments ADD COLUMN override_model_selection_data TEXT CHECK (
    (override_model_selection_data IS NULL) = (override_model_selection_schema_version IS NULL)
    AND (
        override_model_selection_data IS NULL OR (
            length(override_model_selection_data) <= 16384
            AND json_valid(override_model_selection_data)
            AND json_type(override_model_selection_data) = 'object'
        )
    )
);
ALTER TABLE chat_work_assignment_inputs ADD COLUMN delivered_revision_id TEXT CHECK (
    delivered_revision_id IS NULL OR length(delivered_revision_id) BETWEEN 1 AND 1024
);
-- The mention of the teammate a post invokes keeps the override the post asked for, so a Start
-- of a mention that recorded no work, on a device other than the poster, runs with it.
ALTER TABLE chat_participant_reference_targets ADD COLUMN override_model_selection_schema_version INTEGER CHECK (
    override_model_selection_schema_version IS NULL OR override_model_selection_schema_version >= 1
);
ALTER TABLE chat_participant_reference_targets ADD COLUMN override_model_selection_data TEXT CHECK (
    (override_model_selection_data IS NULL) = (override_model_selection_schema_version IS NULL)
    AND (
        override_model_selection_data IS NULL OR (
            length(override_model_selection_data) <= 16384
            AND json_valid(override_model_selection_data)
            AND json_type(override_model_selection_data) = 'object'
        )
    )
);

DROP TRIGGER sync_values_chat_participant_reference_targets_insert;
DROP TRIGGER sync_values_chat_participant_reference_targets_update;
DROP TRIGGER sync_immutable_chat_participant_reference_targets;
DROP TRIGGER sync_recreate_chat_participant_reference_targets;
DROP TRIGGER sync_capture_chat_participant_reference_targets_insert;
DROP TRIGGER sync_capture_chat_participant_reference_targets_update;
DROP TRIGGER sync_capture_chat_participant_reference_targets_delete;
DROP TRIGGER sync_values_chat_work_assignments_insert;
DROP TRIGGER sync_values_chat_work_assignments_update;
DROP TRIGGER sync_immutable_chat_work_assignments;
DROP TRIGGER sync_recreate_chat_work_assignments;
DROP TRIGGER sync_capture_chat_work_assignments_insert;
DROP TRIGGER sync_capture_chat_work_assignments_update;
DROP TRIGGER sync_capture_chat_work_assignments_delete;
DROP TRIGGER sync_values_chat_work_assignment_inputs_insert;
DROP TRIGGER sync_values_chat_work_assignment_inputs_update;
DROP TRIGGER sync_immutable_chat_work_assignment_inputs;
DROP TRIGGER sync_capture_chat_work_assignment_inputs_insert;
DROP TRIGGER sync_capture_chat_work_assignment_inputs_update;
DROP TRIGGER sync_capture_chat_work_assignment_inputs_delete;

CREATE TRIGGER sync_values_chat_work_assignments_insert
BEFORE INSERT ON chat_work_assignments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data, previous_assignment_id, state, state_reason, settled_at, review_outcome, reviewed_at, reviewed_by_participant_id, updated_at ON chat_work_assignments
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
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
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_assignments
BEFORE UPDATE OF id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data ON chat_work_assignments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.triggering_message_item_id IS NOT NEW.triggering_message_item_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.override_model_selection_schema_version IS NOT NEW.override_model_selection_schema_version OR OLD.override_model_selection_data IS NOT NEW.override_model_selection_data COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
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
            | (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.state_reason IS NOT NEW.state_reason COLLATE BINARY OR OLD.settled_at IS NOT NEW.settled_at COLLATE BINARY OR OLD.review_outcome IS NOT NEW.review_outcome COLLATE BINARY OR OLD.reviewed_at IS NOT NEW.reviewed_at COLLATE BINARY OR OLD.reviewed_by_participant_id IS NOT NEW.reviewed_by_participant_id COLLATE BINARY
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
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))
    AND (NEW.delivered_revision_id IS NULL OR (typeof(NEW.delivered_revision_id) = 'text' AND instr(CAST(NEW.delivered_revision_id AS BLOB), x'00') = 0 AND trim(NEW.delivered_revision_id) <> '' AND length(CAST(NEW.delivered_revision_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignment_inputs_update
BEFORE UPDATE OF message_item_id, created_at, assignment_id, ordinal, routing_kind, delivery_state, delivered_at, delivered_revision_id ON chat_work_assignment_inputs
WHEN ((NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.routing_kind) = 'text' AND instr(CAST(NEW.routing_kind AS BLOB), x'00') = 0 AND length(NEW.routing_kind) <= 32 AND length(CAST(NEW.routing_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.delivery_state) = 'text' AND instr(CAST(NEW.delivery_state AS BLOB), x'00') = 0 AND length(NEW.delivery_state) <= 32 AND length(CAST(NEW.delivery_state AS BLOB)) <= 262144)
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))
    AND (NEW.delivered_revision_id IS NULL OR (typeof(NEW.delivered_revision_id) = 'text' AND instr(CAST(NEW.delivered_revision_id AS BLOB), x'00') = 0 AND trim(NEW.delivered_revision_id) <> '' AND length(CAST(NEW.delivered_revision_id AS BLOB)) <= 128))) IS NOT 1
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
            | (CASE WHEN OLD.delivery_state IS NOT NEW.delivery_state COLLATE BINARY OR OLD.delivered_at IS NOT NEW.delivered_at COLLATE BINARY OR OLD.delivered_revision_id IS NOT NEW.delivered_revision_id COLLATE BINARY
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

CREATE TRIGGER sync_values_chat_participant_reference_targets_insert
BEFORE INSERT ON chat_participant_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (NEW.override_model_selection_schema_version IS NULL OR (typeof(NEW.override_model_selection_schema_version) = 'integer' AND NEW.override_model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.override_model_selection_data IS NULL OR (typeof(NEW.override_model_selection_data) = 'text' AND instr(CAST(NEW.override_model_selection_data AS BLOB), x'00') = 0 AND length(NEW.override_model_selection_data) <= 16384 AND length(CAST(NEW.override_model_selection_data AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_participant_reference_targets_update
BEFORE UPDATE OF participant_id, override_model_selection_schema_version, override_model_selection_data ON chat_participant_reference_targets
WHEN ((NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (NEW.override_model_selection_schema_version IS NULL OR (typeof(NEW.override_model_selection_schema_version) = 'integer' AND NEW.override_model_selection_schema_version BETWEEN 1 AND 9223372036854775807))
    AND (NEW.override_model_selection_data IS NULL OR (typeof(NEW.override_model_selection_data) = 'text' AND instr(CAST(NEW.override_model_selection_data AS BLOB), x'00') = 0 AND length(NEW.override_model_selection_data) <= 16384 AND length(CAST(NEW.override_model_selection_data AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_participant_reference_targets
BEFORE UPDATE OF reference_id, participant_id, override_model_selection_schema_version, override_model_selection_data ON chat_participant_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY OR OLD.override_model_selection_schema_version IS NOT NEW.override_model_selection_schema_version OR OLD.override_model_selection_data IS NOT NEW.override_model_selection_data COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
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

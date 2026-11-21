-- A waiting reply a person holds stays in the queue of its work and is never delivered until
-- they resume it. `held` replicates as its own register, and `ordinal` leaves the routing
-- register for its own, so a reorder on one device never undoes a move of the reply onto
-- another assignment on another.
ALTER TABLE chat_work_assignment_inputs
  ADD COLUMN held INTEGER NOT NULL DEFAULT 0 CHECK (held IN (0, 1));

DROP TRIGGER sync_values_chat_work_assignment_inputs_insert;
DROP TRIGGER sync_values_chat_work_assignment_inputs_update;
DROP TRIGGER sync_capture_chat_work_assignment_inputs_insert;
DROP TRIGGER sync_capture_chat_work_assignment_inputs_update;

CREATE TRIGGER sync_values_chat_work_assignment_inputs_insert
BEFORE INSERT ON chat_work_assignment_inputs
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.routing_kind) = 'text' AND instr(CAST(NEW.routing_kind AS BLOB), x'00') = 0 AND length(NEW.routing_kind) <= 32 AND length(CAST(NEW.routing_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.delivery_state) = 'text' AND instr(CAST(NEW.delivery_state AS BLOB), x'00') = 0 AND length(NEW.delivery_state) <= 32 AND length(CAST(NEW.delivery_state AS BLOB)) <= 262144)
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))
    AND (NEW.delivered_revision_id IS NULL OR (typeof(NEW.delivered_revision_id) = 'text' AND instr(CAST(NEW.delivered_revision_id AS BLOB), x'00') = 0 AND trim(NEW.delivered_revision_id) <> '' AND length(CAST(NEW.delivered_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.held) = 'integer' AND NEW.held IN (0, 1))
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignment_inputs_update
BEFORE UPDATE OF message_item_id, created_at, assignment_id, routing_kind, delivery_state, delivered_at, delivered_revision_id, held, ordinal ON chat_work_assignment_inputs
WHEN ((NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.routing_kind) = 'text' AND instr(CAST(NEW.routing_kind AS BLOB), x'00') = 0 AND length(NEW.routing_kind) <= 32 AND length(CAST(NEW.routing_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.delivery_state) = 'text' AND instr(CAST(NEW.delivery_state AS BLOB), x'00') = 0 AND length(NEW.delivery_state) <= 32 AND length(CAST(NEW.delivery_state AS BLOB)) <= 262144)
    AND (NEW.delivered_at IS NULL OR (typeof(NEW.delivered_at) = 'text' AND instr(CAST(NEW.delivered_at AS BLOB), x'00') = 0 AND trim(NEW.delivered_at) <> '' AND length(CAST(NEW.delivered_at AS BLOB)) <= 64))
    AND (NEW.delivered_revision_id IS NULL OR (typeof(NEW.delivered_revision_id) = 'text' AND instr(CAST(NEW.delivered_revision_id AS BLOB), x'00') = 0 AND trim(NEW.delivered_revision_id) <> '' AND length(CAST(NEW.delivered_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.held) = 'integer' AND NEW.held IN (0, 1))
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_chat_work_assignment_inputs_insert
AFTER INSERT ON chat_work_assignment_inputs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (396, NEW.id, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
                AND NOT (NEW.assignment_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_work_assignments WHERE id = OLD.assignment_id))) OR OLD.routing_kind IS NOT NEW.routing_kind COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.delivery_state IS NOT NEW.delivery_state COLLATE BINARY OR OLD.delivered_at IS NOT NEW.delivered_at COLLATE BINARY OR OLD.delivered_revision_id IS NOT NEW.delivered_revision_id COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.held IS NOT NEW.held
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.ordinal IS NOT NEW.ordinal
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

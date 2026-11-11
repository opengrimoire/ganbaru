-- Record the outcome of a review of ready-for-review work beside the completion it judges. The
-- three columns are all null, or all set on completed work; the sync adapter and the writers
-- enforce that, because SQLite cannot add a table CHECK to an existing table. The reviewer has
-- no foreign key, since a participant row may arrive through sync after the review.
ALTER TABLE chat_work_assignments ADD COLUMN review_outcome TEXT CHECK (
    review_outcome IS NULL OR review_outcome IN ('accepted', 'changes_requested')
);
ALTER TABLE chat_work_assignments ADD COLUMN reviewed_at TEXT CHECK (
    reviewed_at IS NULL OR length(reviewed_at) >= 20
);
ALTER TABLE chat_work_assignments ADD COLUMN reviewed_by_participant_id TEXT CHECK (
    reviewed_by_participant_id IS NULL OR length(reviewed_by_participant_id) BETWEEN 1 AND 1024
);

DROP TRIGGER sync_values_chat_work_assignments_insert;
DROP TRIGGER sync_values_chat_work_assignments_update;
DROP TRIGGER sync_capture_chat_work_assignments_update;

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
    AND (NEW.review_outcome IS NULL OR (typeof(NEW.review_outcome) = 'text' AND instr(CAST(NEW.review_outcome AS BLOB), x'00') = 0 AND length(NEW.review_outcome) <= 32 AND length(CAST(NEW.review_outcome AS BLOB)) <= 262144))
    AND (NEW.reviewed_at IS NULL OR (typeof(NEW.reviewed_at) = 'text' AND instr(CAST(NEW.reviewed_at AS BLOB), x'00') = 0 AND trim(NEW.reviewed_at) <> '' AND length(CAST(NEW.reviewed_at AS BLOB)) <= 64))
    AND (NEW.reviewed_by_participant_id IS NULL OR (typeof(NEW.reviewed_by_participant_id) = 'text' AND instr(CAST(NEW.reviewed_by_participant_id AS BLOB), x'00') = 0 AND length(NEW.reviewed_by_participant_id) <= 1024 AND length(CAST(NEW.reviewed_by_participant_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, previous_assignment_id, state, state_reason, settled_at, review_outcome, reviewed_at, reviewed_by_participant_id, updated_at ON chat_work_assignments
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.triggering_message_item_id IS NULL OR (typeof(NEW.triggering_message_item_id) = 'text' AND instr(CAST(NEW.triggering_message_item_id AS BLOB), x'00') = 0 AND trim(NEW.triggering_message_item_id) <> '' AND length(CAST(NEW.triggering_message_item_id AS BLOB)) <= 128))
    AND (NEW.executing_device_id IS NULL OR (typeof(NEW.executing_device_id) = 'text' AND instr(CAST(NEW.executing_device_id AS BLOB), x'00') = 0 AND length(NEW.executing_device_id) <= 160 AND length(CAST(NEW.executing_device_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.previous_assignment_id IS NULL OR (typeof(NEW.previous_assignment_id) = 'text' AND instr(CAST(NEW.previous_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.previous_assignment_id) <> '' AND length(CAST(NEW.previous_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.state_reason IS NULL OR (typeof(NEW.state_reason) = 'text' AND instr(CAST(NEW.state_reason AS BLOB), x'00') = 0 AND length(NEW.state_reason) <= 4000 AND length(CAST(NEW.state_reason AS BLOB)) <= 262144))
    AND (NEW.settled_at IS NULL OR (typeof(NEW.settled_at) = 'text' AND instr(CAST(NEW.settled_at AS BLOB), x'00') = 0 AND trim(NEW.settled_at) <> '' AND length(CAST(NEW.settled_at AS BLOB)) <= 64))
    AND (NEW.review_outcome IS NULL OR (typeof(NEW.review_outcome) = 'text' AND instr(CAST(NEW.review_outcome AS BLOB), x'00') = 0 AND length(NEW.review_outcome) <= 32 AND length(CAST(NEW.review_outcome AS BLOB)) <= 262144))
    AND (NEW.reviewed_at IS NULL OR (typeof(NEW.reviewed_at) = 'text' AND instr(CAST(NEW.reviewed_at AS BLOB), x'00') = 0 AND trim(NEW.reviewed_at) <> '' AND length(CAST(NEW.reviewed_at AS BLOB)) <= 64))
    AND (NEW.reviewed_by_participant_id IS NULL OR (typeof(NEW.reviewed_by_participant_id) = 'text' AND instr(CAST(NEW.reviewed_by_participant_id AS BLOB), x'00') = 0 AND length(NEW.reviewed_by_participant_id) <= 1024 AND length(CAST(NEW.reviewed_by_participant_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
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

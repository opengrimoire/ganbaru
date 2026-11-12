-- A teammate may delegate bounded work to another teammate. The child assignment names its
-- parent, its depth below the root (at most 2), the objective and expected result the parent
-- wrote, and the folder ceiling it may never exceed, as a JSON array of working folder ids with
-- a capability. All five are null with depth 0 on an assignment that nobody delegated. They are
-- fixed at creation and the child is deleted with its parent.
ALTER TABLE chat_work_assignments ADD COLUMN parent_assignment_id TEXT
    REFERENCES chat_work_assignments(id) ON UPDATE CASCADE ON DELETE CASCADE;
ALTER TABLE chat_work_assignments ADD COLUMN delegation_depth INTEGER NOT NULL DEFAULT 0 CHECK (
    delegation_depth BETWEEN 0 AND 2
);
ALTER TABLE chat_work_assignments ADD COLUMN delegation_objective TEXT CHECK (
    delegation_objective IS NULL OR length(delegation_objective) <= 4000
);
ALTER TABLE chat_work_assignments ADD COLUMN delegation_expected_result TEXT CHECK (
    delegation_expected_result IS NULL OR length(delegation_expected_result) <= 2000
);
ALTER TABLE chat_work_assignments ADD COLUMN delegation_folder_ceiling TEXT CHECK (
    delegation_folder_ceiling IS NULL OR (
        json_valid(delegation_folder_ceiling) AND json_type(delegation_folder_ceiling) = 'array'
    )
);

CREATE INDEX idx_chat_work_assignments_parent
ON chat_work_assignments(parent_assignment_id)
WHERE parent_assignment_id IS NOT NULL;

-- The fields are all set or all absent, and a child is never its own parent. This rule reads
-- only the new row, so it also runs while sync applies remote rows.
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

-- A local child sits exactly one level below its parent. Apply cannot order a child after its
-- parent within one table, so a replica instead checks the whole chain before it delegates
-- from an assignment.
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

DROP TRIGGER sync_values_chat_work_assignments_insert;
DROP TRIGGER sync_values_chat_work_assignments_update;
DROP TRIGGER sync_immutable_chat_work_assignments;
DROP TRIGGER sync_capture_chat_work_assignments_insert;

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
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.parent_assignment_id IS NULL OR (typeof(NEW.parent_assignment_id) = 'text' AND instr(CAST(NEW.parent_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.parent_assignment_id) <> '' AND length(CAST(NEW.parent_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.delegation_depth) = 'integer' AND NEW.delegation_depth BETWEEN 0 AND 2)
    AND (NEW.delegation_objective IS NULL OR (typeof(NEW.delegation_objective) = 'text' AND instr(CAST(NEW.delegation_objective AS BLOB), x'00') = 0 AND length(NEW.delegation_objective) <= 4000 AND length(CAST(NEW.delegation_objective AS BLOB)) <= 262144))
    AND (NEW.delegation_expected_result IS NULL OR (typeof(NEW.delegation_expected_result) = 'text' AND instr(CAST(NEW.delegation_expected_result AS BLOB), x'00') = 0 AND length(NEW.delegation_expected_result) <= 2000 AND length(CAST(NEW.delegation_expected_result AS BLOB)) <= 262144))
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_assignments_update
BEFORE UPDATE OF reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data, previous_assignment_id, state, state_reason, settled_at, review_outcome, reviewed_at, reviewed_by_participant_id, updated_at, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling ON chat_work_assignments
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
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (NEW.parent_assignment_id IS NULL OR (typeof(NEW.parent_assignment_id) = 'text' AND instr(CAST(NEW.parent_assignment_id AS BLOB), x'00') = 0 AND trim(NEW.parent_assignment_id) <> '' AND length(CAST(NEW.parent_assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.delegation_depth) = 'integer' AND NEW.delegation_depth BETWEEN 0 AND 2)
    AND (NEW.delegation_objective IS NULL OR (typeof(NEW.delegation_objective) = 'text' AND instr(CAST(NEW.delegation_objective AS BLOB), x'00') = 0 AND length(NEW.delegation_objective) <= 4000 AND length(CAST(NEW.delegation_objective AS BLOB)) <= 262144))
    AND (NEW.delegation_expected_result IS NULL OR (typeof(NEW.delegation_expected_result) = 'text' AND instr(CAST(NEW.delegation_expected_result AS BLOB), x'00') = 0 AND length(NEW.delegation_expected_result) <= 2000 AND length(CAST(NEW.delegation_expected_result AS BLOB)) <= 262144))
    AND (NEW.delegation_folder_ceiling IS NULL OR (typeof(NEW.delegation_folder_ceiling) = 'text' AND instr(CAST(NEW.delegation_folder_ceiling AS BLOB), x'00') = 0 AND length(NEW.delegation_folder_ceiling) <= 16384 AND length(CAST(NEW.delegation_folder_ceiling AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_assignments
BEFORE UPDATE OF id, reply_thread_id, teammate_id, triggering_message_item_id, executing_device_id, created_at, override_model_selection_schema_version, override_model_selection_data, parent_assignment_id, delegation_depth, delegation_objective, delegation_expected_result, delegation_folder_ceiling ON chat_work_assignments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.triggering_message_item_id IS NOT NEW.triggering_message_item_id COLLATE BINARY OR OLD.executing_device_id IS NOT NEW.executing_device_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.override_model_selection_schema_version IS NOT NEW.override_model_selection_schema_version OR OLD.override_model_selection_data IS NOT NEW.override_model_selection_data COLLATE BINARY OR OLD.parent_assignment_id IS NOT NEW.parent_assignment_id COLLATE BINARY OR OLD.delegation_depth IS NOT NEW.delegation_depth OR OLD.delegation_objective IS NOT NEW.delegation_objective COLLATE BINARY OR OLD.delegation_expected_result IS NOT NEW.delegation_expected_result COLLATE BINARY OR OLD.delegation_folder_ceiling IS NOT NEW.delegation_folder_ceiling COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_work_assignments_insert
AFTER INSERT ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (395, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

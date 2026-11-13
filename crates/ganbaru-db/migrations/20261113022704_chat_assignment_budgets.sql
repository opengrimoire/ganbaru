-- Assignment budgets: the token and working time limits a person set on one assignment, and what
-- its executing device recorded as used. The id derives from the assignment, so devices that set
-- a budget concurrently create one row. Limits are one register; used amounts only grow.
CREATE TABLE chat_assignment_budgets (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  assignment_id TEXT NOT NULL UNIQUE REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  token_limit INTEGER CHECK (
      token_limit IS NULL OR token_limit BETWEEN 1000 AND 1000000000000
  ),
  working_seconds_limit INTEGER CHECK (
      working_seconds_limit IS NULL OR working_seconds_limit BETWEEN 60 AND 8640000
  ),
  set_by_participant_id TEXT CHECK (
      set_by_participant_id IS NULL OR length(set_by_participant_id) BETWEEN 1 AND 1024
  ),
  set_at TEXT NOT NULL CHECK (length(set_at) >= 20),
  used_counted_tokens INTEGER NOT NULL DEFAULT 0 CHECK (
      used_counted_tokens BETWEEN 0 AND 1000000000000
  ),
  used_working_seconds INTEGER NOT NULL DEFAULT 0 CHECK (
      used_working_seconds BETWEEN 0 AND 8640000
  ),
  CHECK (token_limit IS NOT NULL OR working_seconds_limit IS NOT NULL)
) STRICT;

-- The default token limit a new assignment of the teammate starts with; null sets none.
ALTER TABLE chat_teammate_policy_revisions ADD COLUMN default_assignment_token_limit INTEGER
CHECK (
    default_assignment_token_limit IS NULL
    OR default_assignment_token_limit BETWEEN 1000 AND 1000000000000
);

-- Sync triggers that check the policy revision columns, rendered again with the new column.
DROP TRIGGER sync_values_chat_teammate_policy_revisions_insert;
DROP TRIGGER sync_values_chat_teammate_policy_revisions_update;
DROP TRIGGER sync_immutable_chat_teammate_policy_revisions;

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
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.default_assignment_token_limit IS NULL OR (typeof(NEW.default_assignment_token_limit) = 'integer' AND NEW.default_assignment_token_limit BETWEEN 1000 AND 1000000000000))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_teammate_policy_revisions_update
BEFORE UPDATE OF teammate_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode, default_assignment_token_limit ON chat_teammate_policy_revisions
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
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.default_assignment_token_limit IS NULL OR (typeof(NEW.default_assignment_token_limit) = 'integer' AND NEW.default_assignment_token_limit BETWEEN 1000 AND 1000000000000))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_teammate_policy_revisions
BEFORE UPDATE OF id, teammate_id, provider_instance_id, model_selection_schema_version, model_selection_data, effort, speed, provider_options_schema_version, provider_options_data, interaction_mode, created_at, safety_mode, default_assignment_token_limit ON chat_teammate_policy_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.provider_instance_id IS NOT NEW.provider_instance_id COLLATE BINARY OR OLD.model_selection_schema_version IS NOT NEW.model_selection_schema_version OR OLD.model_selection_data IS NOT NEW.model_selection_data COLLATE BINARY OR OLD.effort IS NOT NEW.effort COLLATE BINARY OR OLD.speed IS NOT NEW.speed COLLATE BINARY OR OLD.provider_options_schema_version IS NOT NEW.provider_options_schema_version OR OLD.provider_options_data IS NOT NEW.provider_options_data COLLATE BINARY OR OLD.interaction_mode IS NOT NEW.interaction_mode COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.safety_mode IS NOT NEW.safety_mode COLLATE BINARY OR OLD.default_assignment_token_limit IS NOT NEW.default_assignment_token_limit) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

-- Sync triggers of the budgets.
CREATE TRIGGER sync_values_chat_assignment_budgets_insert
BEFORE INSERT ON chat_assignment_budgets
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (NEW.token_limit IS NULL OR (typeof(NEW.token_limit) = 'integer' AND NEW.token_limit BETWEEN 1000 AND 1000000000000))
    AND (NEW.working_seconds_limit IS NULL OR (typeof(NEW.working_seconds_limit) = 'integer' AND NEW.working_seconds_limit BETWEEN 60 AND 8640000))
    AND (NEW.set_by_participant_id IS NULL OR (typeof(NEW.set_by_participant_id) = 'text' AND instr(CAST(NEW.set_by_participant_id AS BLOB), x'00') = 0 AND length(NEW.set_by_participant_id) <= 1024 AND length(CAST(NEW.set_by_participant_id AS BLOB)) <= 262144))
    AND (typeof(NEW.set_at) = 'text' AND instr(CAST(NEW.set_at AS BLOB), x'00') = 0 AND trim(NEW.set_at) <> '' AND length(CAST(NEW.set_at AS BLOB)) <= 64)
    AND (typeof(NEW.used_counted_tokens) = 'integer' AND NEW.used_counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.used_working_seconds) = 'integer' AND NEW.used_working_seconds BETWEEN 0 AND 8640000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_assignment_budgets_update
BEFORE UPDATE OF assignment_id, token_limit, working_seconds_limit, set_by_participant_id, set_at, used_counted_tokens, used_working_seconds ON chat_assignment_budgets
WHEN ((NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (NEW.token_limit IS NULL OR (typeof(NEW.token_limit) = 'integer' AND NEW.token_limit BETWEEN 1000 AND 1000000000000))
    AND (NEW.working_seconds_limit IS NULL OR (typeof(NEW.working_seconds_limit) = 'integer' AND NEW.working_seconds_limit BETWEEN 60 AND 8640000))
    AND (NEW.set_by_participant_id IS NULL OR (typeof(NEW.set_by_participant_id) = 'text' AND instr(CAST(NEW.set_by_participant_id AS BLOB), x'00') = 0 AND length(NEW.set_by_participant_id) <= 1024 AND length(CAST(NEW.set_by_participant_id AS BLOB)) <= 262144))
    AND (typeof(NEW.set_at) = 'text' AND instr(CAST(NEW.set_at AS BLOB), x'00') = 0 AND trim(NEW.set_at) <> '' AND length(CAST(NEW.set_at AS BLOB)) <= 64)
    AND (typeof(NEW.used_counted_tokens) = 'integer' AND NEW.used_counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.used_working_seconds) = 'integer' AND NEW.used_working_seconds BETWEEN 0 AND 8640000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_assignment_budgets
BEFORE UPDATE OF id, assignment_id ON chat_assignment_budgets
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.assignment_id IS NOT NEW.assignment_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_assignment_budgets_insert
AFTER INSERT ON chat_assignment_budgets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (398, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_assignment_budgets_update
AFTER UPDATE ON chat_assignment_budgets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 398, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.token_limit IS NOT NEW.token_limit OR OLD.working_seconds_limit IS NOT NEW.working_seconds_limit OR OLD.set_by_participant_id IS NOT NEW.set_by_participant_id COLLATE BINARY OR OLD.set_at IS NOT NEW.set_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.used_counted_tokens IS NOT NEW.used_counted_tokens
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.used_working_seconds IS NOT NEW.used_working_seconds
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_assignment_budgets_delete
AFTER DELETE ON chat_assignment_budgets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (398, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

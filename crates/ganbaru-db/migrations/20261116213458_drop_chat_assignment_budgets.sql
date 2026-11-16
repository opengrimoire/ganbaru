-- Chat work carries no token or working time limits: assignment budgets and the default token
-- limit of teammate policies go away, while run usage and the daily teammate usage summaries
-- stay. Sync bookkeeping of the budget table (id 398) goes with it, since the engine treats rows
-- of a table it does not know as corrupt.
DROP TRIGGER sync_values_chat_assignment_budgets_insert;
DROP TRIGGER sync_values_chat_assignment_budgets_update;
DROP TRIGGER sync_immutable_chat_assignment_budgets;
DROP TRIGGER sync_capture_chat_assignment_budgets_insert;
DROP TRIGGER sync_capture_chat_assignment_budgets_update;
DROP TRIGGER sync_capture_chat_assignment_budgets_delete;
DROP TABLE chat_assignment_budgets;

DELETE FROM sync_capture WHERE table_id = 398;
DELETE FROM sync_version_refs WHERE table_id = 398;
DELETE FROM sync_register_versions WHERE table_id = 398;
DELETE FROM sync_row_creations WHERE table_id = 398;
DELETE FROM sync_tombstones WHERE table_id = 398;
DELETE FROM sync_contested_classes WHERE table_id = 398;
DELETE FROM sync_displaced_versions WHERE table_id = 398;
DELETE FROM sync_late_changes WHERE table_id = 398;
DELETE FROM sync_refused_rows WHERE table_id = 398;
DELETE FROM sync_restated_groups WHERE table_id = 398;
DELETE FROM sync_stale_rows WHERE table_id = 398;
DELETE FROM sync_unique_classes WHERE table_id = 398;
DELETE FROM sync_seeded_tables WHERE table_id = 398;
DELETE FROM sync_rows WHERE table_id = 398;

DROP TRIGGER sync_values_chat_teammate_policy_revisions_insert;
DROP TRIGGER sync_values_chat_teammate_policy_revisions_update;
DROP TRIGGER sync_immutable_chat_teammate_policy_revisions;
ALTER TABLE chat_teammate_policy_revisions DROP COLUMN default_assignment_token_limit;

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

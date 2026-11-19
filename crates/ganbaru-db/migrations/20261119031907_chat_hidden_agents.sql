-- An archived custom agent is off and hidden from the Settings agent list until a person
-- restores it. `hidden_at` replicates in the participant lifecycle register beside
-- `archived_at`, so a concurrent toggle and archive settle as one value, and a hidden agent is
-- always off.
ALTER TABLE chat_participants
  ADD COLUMN hidden_at TEXT CHECK (
      hidden_at IS NULL OR (length(hidden_at) >= 20 AND archived_at IS NOT NULL)
  );

DROP TRIGGER sync_values_chat_participants_insert;
DROP TRIGGER sync_values_chat_participants_update;
DROP TRIGGER sync_capture_chat_participants_update;

CREATE TRIGGER sync_values_chat_participants_insert
BEFORE INSERT ON chat_participants
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.avatar_schema_version) = 'integer' AND NEW.avatar_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.avatar_data) = 'text' AND instr(CAST(NEW.avatar_data AS BLOB), x'00') = 0 AND length(NEW.avatar_data) <= 16384 AND length(CAST(NEW.avatar_data AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_participants_update
BEFORE UPDATE OF participant_kind, created_at, display_name, avatar_schema_version, avatar_data, archived_at, hidden_at, updated_at ON chat_participants
WHEN ((typeof(NEW.participant_kind) = 'text' AND instr(CAST(NEW.participant_kind AS BLOB), x'00') = 0 AND length(NEW.participant_kind) <= 32 AND length(CAST(NEW.participant_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.avatar_schema_version) = 'integer' AND NEW.avatar_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.avatar_data) = 'text' AND instr(CAST(NEW.avatar_data AS BLOB), x'00') = 0 AND length(NEW.avatar_data) <= 16384 AND length(CAST(NEW.avatar_data AS BLOB)) <= 262144)
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
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
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY OR OLD.hidden_at IS NOT NEW.hidden_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

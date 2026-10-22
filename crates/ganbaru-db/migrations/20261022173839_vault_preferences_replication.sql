-- Portable preferences replicate one row per preference key: the value merges as a register,
-- updated_at as the latest timestamp, and a hidden existence register settles concurrent
-- creation of the same key. Sync triggers rendered by ganbaru-sync from the vault manifest; the
-- conformance test fails when these differ from the current rendering.
CREATE TRIGGER sync_values_vault_preferences_insert
BEFORE INSERT ON vault_preferences
WHEN ((typeof(NEW.key) = 'text' AND instr(CAST(NEW.key AS BLOB), x'00') = 0 AND trim(NEW.key) <> '' AND length(CAST(NEW.key AS BLOB)) <= 128)
    AND (typeof(NEW.value) = 'text' AND instr(CAST(NEW.value AS BLOB), x'00') = 0 AND length(NEW.value) <= 65536 AND length(CAST(NEW.value AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_vault_preferences_update
BEFORE UPDATE OF value, updated_at ON vault_preferences
WHEN ((typeof(NEW.value) = 'text' AND instr(CAST(NEW.value AS BLOB), x'00') = 0 AND length(NEW.value) <= 65536 AND length(CAST(NEW.value AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_vault_preferences
BEFORE UPDATE OF key ON vault_preferences
WHEN OLD.key IS NOT NEW.key COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_vault_preferences_insert
AFTER INSERT ON vault_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (16, NEW.key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_vault_preferences_update
AFTER UPDATE ON vault_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 16, NEW.key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.value IS NOT NEW.value COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_vault_preferences_delete
AFTER DELETE ON vault_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (16, OLD.key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

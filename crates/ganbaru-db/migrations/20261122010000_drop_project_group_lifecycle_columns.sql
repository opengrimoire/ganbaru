-- Project groups have no hidden or archived lifecycle: nothing hid or archived a group, deleting a
-- group keeps its projects, and every project carries its own lifecycle. The unused `hidden_at`
-- and `archived_at` columns go away, and the capture trigger marks `updated_at` with the bit the
-- removed lifecycle group used.
DROP TRIGGER sync_capture_project_groups_insert;
DROP TRIGGER sync_capture_project_groups_update;
DROP TRIGGER sync_values_project_groups_insert;
DROP TRIGGER sync_values_project_groups_update;
ALTER TABLE project_groups DROP COLUMN hidden_at;
ALTER TABLE project_groups DROP COLUMN archived_at;

CREATE TRIGGER sync_capture_project_groups_insert
AFTER INSERT ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (96, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_groups_update
AFTER UPDATE ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 96, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.icon_byte_size IS NOT NEW.icon_byte_size
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_project_groups_insert
BEFORE INSERT ON project_groups
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_groups_update
BEFORE UPDATE OF created_at, name, icon, icon_byte_size, color, order_key, updated_at ON project_groups
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

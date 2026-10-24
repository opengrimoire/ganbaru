-- Daily distraction usage replicates as one total per device, source, and local day, keyed by
-- those four values. Only the device a row names writes it and its total only grows, so rows
-- merge by maximum; rows are never deleted, and the retention window is a read filter.
-- Usage samples stay on the device as its accounting journal and no longer replicate. Samples
-- already stored mixed this device's usage with usage imported from linked devices under ids
-- that cannot be told apart, so they are cleared: development vaults start counting again.

CREATE TABLE distractions_usage_totals (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    device_id TEXT NOT NULL CHECK (trim(device_id) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('website', 'desktop-app', 'mobile-app')),
    source_key TEXT NOT NULL CHECK (trim(source_key) <> ''),
    local_date TEXT NOT NULL CHECK (
        length(local_date) = 10
        AND substr(local_date, 5, 1) = '-'
        AND substr(local_date, 8, 1) = '-'
    ),
    elapsed_seconds INTEGER NOT NULL CHECK (elapsed_seconds > 0 AND elapsed_seconds <= 90000),
    display_name TEXT
);
CREATE INDEX idx_distractions_usage_totals_date_device ON distractions_usage_totals(local_date, device_id);

DELETE FROM distractions_usage_samples;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_distractions_usage_totals_insert
BEFORE INSERT ON distractions_usage_totals
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 16 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (typeof(NEW.source_key) = 'text' AND instr(CAST(NEW.source_key AS BLOB), x'00') = 0 AND length(trim(NEW.source_key)) BETWEEN 1 AND 255 AND length(CAST(NEW.source_key AS BLOB)) <= 262144)
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.elapsed_seconds) = 'integer' AND NEW.elapsed_seconds BETWEEN 1 AND 90000)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 120 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_usage_totals_update
BEFORE UPDATE OF device_id, source_type, source_key, local_date, elapsed_seconds, display_name ON distractions_usage_totals
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 16 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (typeof(NEW.source_key) = 'text' AND instr(CAST(NEW.source_key AS BLOB), x'00') = 0 AND length(trim(NEW.source_key)) BETWEEN 1 AND 255 AND length(CAST(NEW.source_key AS BLOB)) <= 262144)
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.elapsed_seconds) = 'integer' AND NEW.elapsed_seconds BETWEEN 1 AND 90000)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 120 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_usage_totals
BEFORE UPDATE OF id, device_id, source_type, source_key, local_date ON distractions_usage_totals
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.device_id IS NOT NEW.device_id COLLATE BINARY OR OLD.source_type IS NOT NEW.source_type COLLATE BINARY OR OLD.source_key IS NOT NEW.source_key COLLATE BINARY OR OLD.local_date IS NOT NEW.local_date COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_distractions_usage_totals
BEFORE INSERT ON distractions_usage_totals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 278 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM distractions_usage_totals WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_distractions_usage_totals_insert
AFTER INSERT ON distractions_usage_totals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (278, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_usage_totals_update
AFTER UPDATE ON distractions_usage_totals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 278, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.elapsed_seconds IS NOT NEW.elapsed_seconds
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_usage_totals_delete
AFTER DELETE ON distractions_usage_totals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (278, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;


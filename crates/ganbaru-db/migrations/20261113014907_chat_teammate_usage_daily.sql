-- Daily teammate usage: what one device's runs of one teammate used during one local day. Each
-- device writes only its own rows, keyed by its owner tag, and the counts only grow, so devices
-- converge on per-device rows that readers sum. Rows hold counts and dates only.
CREATE TABLE chat_teammate_usage_daily (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  device_id TEXT NOT NULL CHECK (length(trim(device_id)) BETWEEN 1 AND 160),
  teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  local_date TEXT NOT NULL CHECK (
      length(local_date) = 10
      AND substr(local_date, 5, 1) = '-'
      AND substr(local_date, 8, 1) = '-'
  ),
  counted_tokens INTEGER NOT NULL CHECK (counted_tokens BETWEEN 0 AND 1000000000000),
  working_seconds INTEGER NOT NULL CHECK (working_seconds BETWEEN 0 AND 8640000),
  run_count INTEGER NOT NULL CHECK (run_count BETWEEN 0 AND 100000)
) STRICT;
CREATE INDEX idx_chat_teammate_usage_daily_teammate_date
ON chat_teammate_usage_daily(teammate_id, local_date);

CREATE TRIGGER sync_values_chat_teammate_usage_daily_insert
BEFORE INSERT ON chat_teammate_usage_daily
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.counted_tokens) = 'integer' AND NEW.counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.working_seconds) = 'integer' AND NEW.working_seconds BETWEEN 0 AND 8640000)
    AND (typeof(NEW.run_count) = 'integer' AND NEW.run_count BETWEEN 0 AND 100000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_teammate_usage_daily_update
BEFORE UPDATE OF device_id, teammate_id, local_date, counted_tokens, working_seconds, run_count ON chat_teammate_usage_daily
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (typeof(NEW.local_date) = 'text' AND instr(CAST(NEW.local_date AS BLOB), x'00') = 0 AND length(NEW.local_date) <= 10 AND length(CAST(NEW.local_date AS BLOB)) <= 262144)
    AND (typeof(NEW.counted_tokens) = 'integer' AND NEW.counted_tokens BETWEEN 0 AND 1000000000000)
    AND (typeof(NEW.working_seconds) = 'integer' AND NEW.working_seconds BETWEEN 0 AND 8640000)
    AND (typeof(NEW.run_count) = 'integer' AND NEW.run_count BETWEEN 0 AND 100000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_teammate_usage_daily
BEFORE UPDATE OF id, device_id, teammate_id, local_date ON chat_teammate_usage_daily
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.device_id IS NOT NEW.device_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.local_date IS NOT NEW.local_date COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_teammate_usage_daily
BEFORE INSERT ON chat_teammate_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 399 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_teammate_usage_daily WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_teammate_usage_daily_insert
AFTER INSERT ON chat_teammate_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (399, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_usage_daily_update
AFTER UPDATE ON chat_teammate_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 399, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.counted_tokens IS NOT NEW.counted_tokens
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.working_seconds IS NOT NEW.working_seconds
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.run_count IS NOT NEW.run_count
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_teammate_usage_daily_delete
AFTER DELETE ON chat_teammate_usage_daily
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (399, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

-- What this device already added to its daily summaries from each run, so a flush adds only the
-- usage since the last one and counts each run once.
ALTER TABLE chat_agent_run_usage
ADD COLUMN summarized_counted_tokens INTEGER NOT NULL DEFAULT 0
    CHECK (summarized_counted_tokens >= 0);
ALTER TABLE chat_agent_run_usage
ADD COLUMN summarized_working_seconds INTEGER NOT NULL DEFAULT 0
    CHECK (summarized_working_seconds >= 0);
ALTER TABLE chat_agent_run_usage
ADD COLUMN summarized_run INTEGER NOT NULL DEFAULT 0 CHECK (summarized_run IN (0, 1));

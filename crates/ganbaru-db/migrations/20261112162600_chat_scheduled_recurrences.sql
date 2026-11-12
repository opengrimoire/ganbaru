-- Repeating scheduled messages. A recurrence row holds the rule of one scheduled message and
-- the progress its executing device advances after each occurrence; it replicates under an id
-- derived from the scheduled message and is deleted with it. The device-local dispatch row
-- counts skipped occurrences and holds the end of a quiet period an occurrence waits for.
CREATE TABLE IF NOT EXISTS chat_scheduled_message_recurrences (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  rule_kind TEXT NOT NULL CHECK (rule_kind IN ('interval', 'fixed_time')),
  interval_minutes INTEGER CHECK (
      interval_minutes IS NULL OR interval_minutes BETWEEN 60 AND 43200
  ),
  local_minute INTEGER CHECK (local_minute IS NULL OR local_minute BETWEEN 0 AND 1439),
  weekdays_mask INTEGER CHECK (weekdays_mask IS NULL OR weekdays_mask BETWEEN 1 AND 127),
  time_zone TEXT NOT NULL CHECK (length(trim(time_zone)) BETWEEN 1 AND 255),
  quiet_policy TEXT NOT NULL CHECK (quiet_policy IN ('defer', 'ignore')),
  next_occurrence_at TEXT NOT NULL CHECK (length(next_occurrence_at) >= 20),
  fired_count INTEGER NOT NULL DEFAULT 0 CHECK (fired_count >= 0),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  CHECK ((rule_kind = 'interval') = (interval_minutes IS NOT NULL)),
  CHECK ((rule_kind = 'fixed_time') = (local_minute IS NOT NULL AND weekdays_mask IS NOT NULL))
) STRICT;
CREATE INDEX IF NOT EXISTS idx_chat_scheduled_message_recurrences_message
ON chat_scheduled_message_recurrences(scheduled_message_id);
CREATE INDEX IF NOT EXISTS idx_chat_scheduled_message_recurrences_next
ON chat_scheduled_message_recurrences(next_occurrence_at, scheduled_message_id);

ALTER TABLE chat_scheduled_message_dispatch
ADD COLUMN skipped_occurrences INTEGER NOT NULL DEFAULT 0 CHECK (skipped_occurrences >= 0);
ALTER TABLE chat_scheduled_message_dispatch
ADD COLUMN deferred_until TEXT CHECK (deferred_until IS NULL OR length(deferred_until) >= 20);

CREATE TRIGGER sync_values_chat_scheduled_message_recurrences_insert
BEFORE INSERT ON chat_scheduled_message_recurrences
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.scheduled_message_id IS NULL OR (typeof(NEW.scheduled_message_id) = 'text' AND instr(CAST(NEW.scheduled_message_id AS BLOB), x'00') = 0 AND trim(NEW.scheduled_message_id) <> '' AND length(CAST(NEW.scheduled_message_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.rule_kind) = 'text' AND instr(CAST(NEW.rule_kind AS BLOB), x'00') = 0 AND length(NEW.rule_kind) <= 32 AND length(CAST(NEW.rule_kind AS BLOB)) <= 262144)
    AND (NEW.interval_minutes IS NULL OR (typeof(NEW.interval_minutes) = 'integer' AND NEW.interval_minutes BETWEEN 60 AND 43200))
    AND (NEW.local_minute IS NULL OR (typeof(NEW.local_minute) = 'integer' AND NEW.local_minute BETWEEN 0 AND 1439))
    AND (NEW.weekdays_mask IS NULL OR (typeof(NEW.weekdays_mask) = 'integer' AND NEW.weekdays_mask BETWEEN 1 AND 127))
    AND (typeof(NEW.time_zone) = 'text' AND instr(CAST(NEW.time_zone AS BLOB), x'00') = 0 AND length(trim(NEW.time_zone)) BETWEEN 1 AND 255 AND length(CAST(NEW.time_zone AS BLOB)) <= 262144)
    AND (typeof(NEW.quiet_policy) = 'text' AND instr(CAST(NEW.quiet_policy AS BLOB), x'00') = 0 AND length(NEW.quiet_policy) <= 32 AND length(CAST(NEW.quiet_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.next_occurrence_at) = 'text' AND instr(CAST(NEW.next_occurrence_at AS BLOB), x'00') = 0 AND trim(NEW.next_occurrence_at) <> '' AND length(CAST(NEW.next_occurrence_at AS BLOB)) <= 64)
    AND (typeof(NEW.fired_count) = 'integer' AND NEW.fired_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_scheduled_message_recurrences_update
BEFORE UPDATE OF scheduled_message_id, created_at, rule_kind, interval_minutes, local_minute, weekdays_mask, time_zone, quiet_policy, next_occurrence_at, fired_count, updated_at ON chat_scheduled_message_recurrences
WHEN ((NEW.scheduled_message_id IS NULL OR (typeof(NEW.scheduled_message_id) = 'text' AND instr(CAST(NEW.scheduled_message_id AS BLOB), x'00') = 0 AND trim(NEW.scheduled_message_id) <> '' AND length(CAST(NEW.scheduled_message_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.rule_kind) = 'text' AND instr(CAST(NEW.rule_kind AS BLOB), x'00') = 0 AND length(NEW.rule_kind) <= 32 AND length(CAST(NEW.rule_kind AS BLOB)) <= 262144)
    AND (NEW.interval_minutes IS NULL OR (typeof(NEW.interval_minutes) = 'integer' AND NEW.interval_minutes BETWEEN 60 AND 43200))
    AND (NEW.local_minute IS NULL OR (typeof(NEW.local_minute) = 'integer' AND NEW.local_minute BETWEEN 0 AND 1439))
    AND (NEW.weekdays_mask IS NULL OR (typeof(NEW.weekdays_mask) = 'integer' AND NEW.weekdays_mask BETWEEN 1 AND 127))
    AND (typeof(NEW.time_zone) = 'text' AND instr(CAST(NEW.time_zone AS BLOB), x'00') = 0 AND length(trim(NEW.time_zone)) BETWEEN 1 AND 255 AND length(CAST(NEW.time_zone AS BLOB)) <= 262144)
    AND (typeof(NEW.quiet_policy) = 'text' AND instr(CAST(NEW.quiet_policy AS BLOB), x'00') = 0 AND length(NEW.quiet_policy) <= 32 AND length(CAST(NEW.quiet_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.next_occurrence_at) = 'text' AND instr(CAST(NEW.next_occurrence_at AS BLOB), x'00') = 0 AND trim(NEW.next_occurrence_at) <> '' AND length(CAST(NEW.next_occurrence_at AS BLOB)) <= 64)
    AND (typeof(NEW.fired_count) = 'integer' AND NEW.fired_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_scheduled_message_recurrences
BEFORE UPDATE OF id, scheduled_message_id ON chat_scheduled_message_recurrences
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.scheduled_message_id IS NOT NEW.scheduled_message_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_recurrences_insert
AFTER INSERT ON chat_scheduled_message_recurrences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (401, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_recurrences_update
AFTER UPDATE ON chat_scheduled_message_recurrences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 401, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.rule_kind IS NOT NEW.rule_kind COLLATE BINARY OR OLD.interval_minutes IS NOT NEW.interval_minutes OR OLD.local_minute IS NOT NEW.local_minute OR OLD.weekdays_mask IS NOT NEW.weekdays_mask OR OLD.time_zone IS NOT NEW.time_zone COLLATE BINARY OR OLD.quiet_policy IS NOT NEW.quiet_policy COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.next_occurrence_at IS NOT NEW.next_occurrence_at COLLATE BINARY OR OLD.fired_count IS NOT NEW.fired_count
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_scheduled_message_recurrences_delete
AFTER DELETE ON chat_scheduled_message_recurrences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (401, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

-- no-transaction
-- Semantic updates gain the delegation, schedule proposal, and note proposal kinds. SQLite
-- cannot change a CHECK in place, so the table is rebuilt once with every kind. No table
-- references semantic updates; foreign keys are still off for the rebuild so dropping the old
-- table never runs a cascade, and the rebuild repeats safely if it stops before sqlx records it.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP TABLE IF EXISTS chat_work_semantic_updates_rebuild;

CREATE TABLE chat_work_semantic_updates_rebuild (
  item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  update_kind TEXT NOT NULL CHECK (
      update_kind IN (
          'plan', 'replan', 'question', 'approval', 'failure', 'result', 'review',
          'delegation', 'schedule_proposal', 'note_proposal'
      )
  ),
  payload_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (payload_schema_version >= 1),
  payload_data TEXT NOT NULL CHECK (json_valid(payload_data) AND json_type(payload_data) = 'object'),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

INSERT INTO chat_work_semantic_updates_rebuild SELECT * FROM chat_work_semantic_updates;

DROP TABLE chat_work_semantic_updates;

PRAGMA legacy_alter_table = ON;
ALTER TABLE chat_work_semantic_updates_rebuild RENAME TO chat_work_semantic_updates;
PRAGMA legacy_alter_table = OFF;

CREATE INDEX idx_chat_work_semantic_updates_assignment
ON chat_work_semantic_updates(assignment_id);

CREATE TRIGGER sync_values_chat_work_semantic_updates_insert
BEFORE INSERT ON chat_work_semantic_updates
WHEN ((typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128)
    AND (NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.update_kind) = 'text' AND instr(CAST(NEW.update_kind AS BLOB), x'00') = 0 AND length(NEW.update_kind) <= 32 AND length(CAST(NEW.update_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.payload_schema_version) = 'integer' AND NEW.payload_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.payload_data) = 'text' AND instr(CAST(NEW.payload_data AS BLOB), x'00') = 0 AND length(NEW.payload_data) <= 196608 AND length(CAST(NEW.payload_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_work_semantic_updates_update
BEFORE UPDATE OF assignment_id, update_kind, payload_schema_version, payload_data, created_at ON chat_work_semantic_updates
WHEN ((NEW.assignment_id IS NULL OR (typeof(NEW.assignment_id) = 'text' AND instr(CAST(NEW.assignment_id AS BLOB), x'00') = 0 AND trim(NEW.assignment_id) <> '' AND length(CAST(NEW.assignment_id AS BLOB)) <= 128))
    AND (typeof(NEW.update_kind) = 'text' AND instr(CAST(NEW.update_kind AS BLOB), x'00') = 0 AND length(NEW.update_kind) <= 32 AND length(CAST(NEW.update_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.payload_schema_version) = 'integer' AND NEW.payload_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.payload_data) = 'text' AND instr(CAST(NEW.payload_data AS BLOB), x'00') = 0 AND length(NEW.payload_data) <= 196608 AND length(CAST(NEW.payload_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_work_semantic_updates
BEFORE UPDATE OF item_id, assignment_id, update_kind, payload_schema_version, payload_data, created_at ON chat_work_semantic_updates
WHEN OLD.item_id IS NOT NEW.item_id COLLATE BINARY
    OR ((OLD.assignment_id IS NOT NEW.assignment_id COLLATE BINARY OR OLD.update_kind IS NOT NEW.update_kind COLLATE BINARY OR OLD.payload_schema_version IS NOT NEW.payload_schema_version OR OLD.payload_data IS NOT NEW.payload_data COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_work_semantic_updates
BEFORE INSERT ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 397 AND row_key = NEW.item_id)
    AND NOT EXISTS (SELECT 1 FROM chat_work_semantic_updates WHERE item_id = NEW.item_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_insert
AFTER INSERT ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (397, NEW.item_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_update
AFTER UPDATE ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 397, NEW.item_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_work_semantic_updates_delete
AFTER DELETE ON chat_work_semantic_updates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (397, OLD.item_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA foreign_keys = ON;

-- no-transaction
-- A project status is terminal exactly when its category is done, and task completion follows
-- the terminal flag. The category and the flag replicate as one coupled value, so the CHECK ties
-- them on every replica. Existing rows that disagree take the flag their category implies.
--
-- Rebuilding project_statuses would cascade into every task that references it, so foreign keys
-- are off for this connection and the change commits as one transaction. Rows are copied back
-- before the sync triggers are recreated, so the copy is not captured as local edits. The update
-- capture trigger marks the category and the flag with one group bit.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE project_statuses_previous AS SELECT * FROM project_statuses;
DROP TABLE project_statuses;

CREATE TABLE project_statuses (
  id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (trim(name) <> ''),
  category TEXT NOT NULL CHECK (category IN ('not_started', 'active', 'blocked', 'done')),
  terminal INTEGER NOT NULL DEFAULT 0 CHECK (terminal IN (0, 1)),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
  color INTEGER NOT NULL DEFAULT 30 CHECK (color >= 0 AND color < 32),
  order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128),
  deleted_at TEXT CHECK (deleted_at IS NULL OR trim(deleted_at) <> ''),
  CHECK (terminal = (category = 'done'))
);

INSERT INTO project_statuses (id, project_id, name, category, terminal, created_at, updated_at, color, order_key, deleted_at)
SELECT id, project_id, name, category, category = 'done', created_at, updated_at, color, order_key, deleted_at
FROM project_statuses_previous;

DROP TABLE project_statuses_previous;

CREATE INDEX idx_project_statuses_project_order ON project_statuses(project_id, order_key, id);

CREATE TRIGGER project_statuses_order_key_insert
BEFORE INSERT ON project_statuses
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project status order key is required');
END;

CREATE TRIGGER project_statuses_order_key_update
BEFORE UPDATE OF order_key ON project_statuses
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project status order key is required');
END;

CREATE TRIGGER sync_immutable_project_statuses
BEFORE UPDATE OF id, project_id, created_at ON project_statuses
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_statuses
BEFORE INSERT ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 101 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_statuses WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_values_project_statuses_insert
BEFORE INSERT ON project_statuses
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.category) = 'text' AND instr(CAST(NEW.category AS BLOB), x'00') = 0 AND length(NEW.category) <= 32 AND length(CAST(NEW.category AS BLOB)) <= 262144)
    AND (typeof(NEW.terminal) = 'integer' AND NEW.terminal IN (0, 1))
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_statuses_update
BEFORE UPDATE OF project_id, created_at, name, category, terminal, color, order_key, deleted_at, updated_at ON project_statuses
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.category) = 'text' AND instr(CAST(NEW.category AS BLOB), x'00') = 0 AND length(NEW.category) <= 32 AND length(CAST(NEW.category AS BLOB)) <= 262144)
    AND (typeof(NEW.terminal) = 'integer' AND NEW.terminal IN (0, 1))
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_project_statuses_insert
AFTER INSERT ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (101, NEW.id, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_statuses_update
AFTER UPDATE ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 101, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.category IS NOT NEW.category COLLATE BINARY OR OLD.terminal IS NOT NEW.terminal
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_statuses_delete
AFTER DELETE ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (101, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA foreign_keys = ON;

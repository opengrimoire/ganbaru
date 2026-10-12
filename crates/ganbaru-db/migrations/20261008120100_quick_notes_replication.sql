-- Quick notes replicate through the sync engine. Tags and notes order by fractional order keys,
-- and triggers rendered from the sync manifest guard values and capture every local change.

-- Existing rows get rank keys: 'c' and three base 62 digits, which covers 238,328 rows.
CREATE TEMP TABLE quick_notes_rank_capacity (
    row_count INTEGER NOT NULL CHECK (row_count <= 238328)
);
INSERT INTO quick_notes_rank_capacity (row_count)
SELECT count(*) FROM quick_note_tags
UNION ALL
SELECT count(*) FROM quick_notes;
DROP TABLE quick_notes_rank_capacity;

-- Tags are rebuilt without sort_order and its tag count limit. Dropping the old table sets
-- every note's tag_id to null through the foreign key action, so assignments are restored.
CREATE TEMP TABLE quick_notes_tag_assignments AS
SELECT id, tag_id FROM quick_notes WHERE tag_id IS NOT NULL;

CREATE TABLE quick_note_tags_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (trim(id) <> ''),
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 40),
    order_key TEXT NOT NULL CHECK (length(order_key) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    UNIQUE (name)
);

INSERT INTO quick_note_tags_next (id, name, order_key, created_at, updated_at)
SELECT
    id,
    name,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1),
    created_at,
    updated_at
FROM (
    SELECT *, ROW_NUMBER() OVER (ORDER BY sort_order, id) - 1 AS rank
    FROM quick_note_tags
);

DROP TABLE quick_note_tags;
ALTER TABLE quick_note_tags_next RENAME TO quick_note_tags;

UPDATE quick_notes
SET tag_id = saved.tag_id
FROM quick_notes_tag_assignments AS saved
WHERE quick_notes.id = saved.id;
DROP TABLE quick_notes_tag_assignments;

-- Notes are converted in place, because rebuilding the parent of the text runs would cascade.
DROP INDEX idx_quick_notes_active;
DROP INDEX idx_quick_notes_tag_active;

ALTER TABLE quick_notes ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);

UPDATE quick_notes
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (ORDER BY manual_order, id) - 1 AS rank
        FROM quick_notes
    )
) AS ranked
WHERE quick_notes.id = ranked.id;

ALTER TABLE quick_notes DROP COLUMN manual_order;

CREATE INDEX idx_quick_notes_active
    ON quick_notes(pinned DESC, order_key ASC, id ASC)
    WHERE archived = 0 AND trashed_at IS NULL;
CREATE INDEX idx_quick_notes_tag_active
    ON quick_notes(tag_id, pinned DESC, order_key ASC, id ASC)
    WHERE archived = 0 AND trashed_at IS NULL;

-- Sync triggers, rendered by ganbaru-sync from the vault manifest. The conformance test fails
-- when these differ from the current rendering.

CREATE TRIGGER sync_values_quick_note_tags_insert
BEFORE INSERT ON quick_note_tags
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 40 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_quick_note_tags_update
BEFORE UPDATE OF created_at, name, order_key, updated_at ON quick_note_tags
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 40 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_quick_note_tags
BEFORE UPDATE OF id, created_at ON quick_note_tags
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_quick_note_tags
BEFORE INSERT ON quick_note_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 1 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM quick_note_tags WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_quick_note_tags_insert
AFTER INSERT ON quick_note_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (1, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_quick_note_tags_update
AFTER UPDATE ON quick_note_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 1, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_note_tags_delete
AFTER DELETE ON quick_note_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (1, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_quick_notes_insert
BEFORE INSERT ON quick_notes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 200 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (NEW.tag_id IS NULL OR (typeof(NEW.tag_id) = 'text' AND instr(CAST(NEW.tag_id AS BLOB), x'00') = 0 AND trim(NEW.tag_id) <> '' AND length(CAST(NEW.tag_id AS BLOB)) <= 128))
    AND (typeof(NEW.pinned) = 'integer' AND NEW.pinned IN (0, 1))
    AND (typeof(NEW.archived) = 'integer' AND NEW.archived IN (0, 1))
    AND (NEW.trashed_at IS NULL OR (typeof(NEW.trashed_at) = 'text' AND instr(CAST(NEW.trashed_at AS BLOB), x'00') = 0 AND trim(NEW.trashed_at) <> '' AND length(CAST(NEW.trashed_at AS BLOB)) <= 64))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_quick_notes_update
BEFORE UPDATE OF created_at, title, color, tag_id, pinned, archived, trashed_at, order_key, updated_at ON quick_notes
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 200 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (NEW.tag_id IS NULL OR (typeof(NEW.tag_id) = 'text' AND instr(CAST(NEW.tag_id AS BLOB), x'00') = 0 AND trim(NEW.tag_id) <> '' AND length(CAST(NEW.tag_id AS BLOB)) <= 128))
    AND (typeof(NEW.pinned) = 'integer' AND NEW.pinned IN (0, 1))
    AND (typeof(NEW.archived) = 'integer' AND NEW.archived IN (0, 1))
    AND (NEW.trashed_at IS NULL OR (typeof(NEW.trashed_at) = 'text' AND instr(CAST(NEW.trashed_at AS BLOB), x'00') = 0 AND trim(NEW.trashed_at) <> '' AND length(CAST(NEW.trashed_at AS BLOB)) <= 64))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_quick_notes
BEFORE UPDATE OF id, created_at ON quick_notes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_quick_notes
BEFORE INSERT ON quick_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 2 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM quick_notes WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_quick_notes_insert
AFTER INSERT ON quick_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, NEW.id, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_quick_notes_update
AFTER UPDATE ON quick_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 2, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.tag_id IS NOT NEW.tag_id COLLATE BINARY
                AND NOT (NEW.tag_id IS NULL AND NOT EXISTS (SELECT 1 FROM quick_note_tags WHERE id = OLD.tag_id))
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.pinned IS NOT NEW.pinned OR OLD.archived IS NOT NEW.archived OR OLD.trashed_at IS NOT NEW.trashed_at COLLATE BINARY THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY THEN 128 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_notes_delete
AFTER DELETE ON quick_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_quick_note_text_runs_insert
AFTER INSERT ON quick_note_text_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, NEW.note_id, 4, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_note_text_runs_update
AFTER UPDATE ON quick_note_text_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 2, parent.key, 4, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.note_id AS key UNION SELECT OLD.note_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_note_text_runs_delete
AFTER DELETE ON quick_note_text_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, OLD.note_id, 4, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

-- Every existing row is unpublished: the first seal creates it with all of its groups.
INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
SELECT 1, id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM quick_note_tags;
INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
SELECT 2, id, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM quick_notes;

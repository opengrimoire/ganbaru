-- no-transaction
-- A Notes page in Trash always has a trash time, so retention can purge it, and a page outside
-- Trash has neither a trash time nor a trash owner. Trash, its time, and its owner replicate as
-- one coupled value, so the CHECK ties only those three columns; archive replicates separately
-- and a trashed page may keep it as the way back into Archive. Existing rows that disagree are
-- normalized: a trashed page without a time takes its last edit time, and a page outside Trash
-- drops a leftover time and owner.
--
-- Rebuilding notes_pages would cascade into every table that references it, so foreign keys
-- are off for this connection and the change commits as one transaction. Rows are copied back
-- before the sync triggers are recreated, so the copy is not captured as local edits.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE notes_pages_previous AS SELECT * FROM notes_pages;
DROP TABLE notes_pages;

CREATE TABLE notes_pages (
  id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
  parent_type TEXT NOT NULL DEFAULT 'workspace'
      CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
  parent_page_id TEXT,
  parent_block_id TEXT,
  parent_data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE
      DEFERRABLE INITIALLY DEFERRED,
  child_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
  project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
  title TEXT NOT NULL DEFAULT '',
  icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
  cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
  in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
  trash_owner TEXT CHECK (trash_owner IS NULL OR trim(trash_owner) <> ''),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
  source_provider TEXT,
  source_object_id TEXT,
  source_workspace_id TEXT,
  source_last_edited_time TEXT,
  url TEXT,
  public_url TEXT,
  created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
  last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
  trashed_time TEXT CHECK (trashed_time IS NULL OR trim(trashed_time) <> ''),
  folder_id TEXT REFERENCES notes_folders(id) ON DELETE SET NULL,
  parent_row_page_id TEXT REFERENCES notes_pages(id) ON DELETE SET NULL
      CHECK (parent_row_page_id IS NULL OR parent_row_page_id <> id),
  CHECK (child_block_id IS NULL OR parent_data_source_id IS NULL),
  CHECK (folder_id IS NULL OR (child_block_id IS NULL AND parent_data_source_id IS NULL)),
  CHECK (
      (in_trash = 1 AND trashed_time IS NOT NULL)
      OR (in_trash = 0 AND trashed_time IS NULL AND trash_owner IS NULL)
  )
);

INSERT INTO notes_pages (id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, child_block_id, project_id, title, icon, cover, in_trash, trash_owner, archived, source_provider, source_object_id, source_workspace_id, source_last_edited_time, url, public_url, created_time, last_edited_time, trashed_time, folder_id, parent_row_page_id)
SELECT id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, child_block_id,
    project_id, title, icon, cover, in_trash,
    CASE WHEN in_trash = 1 THEN trash_owner END,
    archived, source_provider, source_object_id, source_workspace_id, source_last_edited_time,
    url, public_url, created_time, last_edited_time,
    CASE WHEN in_trash = 1 THEN COALESCE(trashed_time, last_edited_time) END,
    folder_id, parent_row_page_id
FROM notes_pages_previous;

DROP TABLE notes_pages_previous;

CREATE INDEX idx_notes_pages_active
    ON notes_pages(in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_child_block ON notes_pages(child_block_id);
CREATE INDEX idx_notes_pages_data_source
    ON notes_pages(parent_data_source_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_folder
    ON notes_pages(folder_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_parent
    ON notes_pages(parent_type, parent_page_id, parent_block_id, parent_data_source_id);
CREATE INDEX idx_notes_pages_parent_row
    ON notes_pages(parent_row_page_id)
    WHERE parent_row_page_id IS NOT NULL;
CREATE INDEX idx_notes_pages_project
    ON notes_pages(project_id, in_trash, archived);
CREATE INDEX idx_notes_pages_source
    ON notes_pages(source_provider, source_workspace_id, source_object_id);
CREATE INDEX idx_notes_pages_trash_retention
    ON notes_pages(in_trash, trashed_time);
CREATE INDEX idx_notes_pages_visible
    ON notes_pages(in_trash, last_edited_time DESC, title);

CREATE TRIGGER notes_pages_local_nesting_insert
BEFORE INSERT ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT ((NEW.child_block_id IS NULL OR NEW.child_block_id = NEW.id)
        AND (NEW.child_block_id IS NOT NULL) = (NEW.parent_type IN ('page_id', 'block_id')))
BEGIN
    SELECT RAISE(ABORT, 'A nested Notes page must name its own child_page block');
END;

CREATE TRIGGER notes_pages_local_nesting_update
BEFORE UPDATE OF id, parent_type, child_block_id ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT ((NEW.child_block_id IS NULL OR NEW.child_block_id = NEW.id)
        AND (NEW.child_block_id IS NOT NULL) = (NEW.parent_type IN ('page_id', 'block_id')))
BEGIN
    SELECT RAISE(ABORT, 'A nested Notes page must name its own child_page block');
END;

CREATE TRIGGER notes_pages_park_child_page_blocks
BEFORE DELETE ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE notes_blocks
    SET page_id = id,
        parent_type = 'page_id',
        parent_page_id = id,
        parent_block_id = NULL,
        in_trash = 1,
        trash_owner = NULL
    WHERE page_id = OLD.id
        AND type = 'child_page'
        AND id <> OLD.id
        AND EXISTS (
            SELECT 1 FROM notes_pages AS page
            WHERE page.id = notes_blocks.id AND page.child_block_id IS NULL
        );
END;

CREATE TRIGGER sync_values_notes_pages_insert
BEFORE INSERT ON notes_pages
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (NEW.public_url IS NULL OR (typeof(NEW.public_url) = 'text' AND instr(CAST(NEW.public_url AS BLOB), x'00') = 0 AND length(NEW.public_url) <= 262144 AND length(CAST(NEW.public_url AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (NEW.child_block_id IS NULL OR (typeof(NEW.child_block_id) = 'text' AND instr(CAST(NEW.child_block_id AS BLOB), x'00') = 0 AND trim(NEW.child_block_id) <> '' AND length(CAST(NEW.child_block_id AS BLOB)) <= 128))
    AND (NEW.parent_data_source_id IS NULL OR (typeof(NEW.parent_data_source_id) = 'text' AND instr(CAST(NEW.parent_data_source_id AS BLOB), x'00') = 0 AND trim(NEW.parent_data_source_id) <> '' AND length(CAST(NEW.parent_data_source_id AS BLOB)) <= 128))
    AND (NEW.folder_id IS NULL OR (typeof(NEW.folder_id) = 'text' AND instr(CAST(NEW.folder_id AS BLOB), x'00') = 0 AND trim(NEW.folder_id) <> '' AND length(CAST(NEW.folder_id AS BLOB)) <= 128))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (NEW.parent_row_page_id IS NULL OR (typeof(NEW.parent_row_page_id) = 'text' AND instr(CAST(NEW.parent_row_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_row_page_id) <> '' AND length(CAST(NEW.parent_row_page_id AS BLOB)) <= 128))
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.trashed_time IS NULL OR (typeof(NEW.trashed_time) = 'text' AND instr(CAST(NEW.trashed_time AS BLOB), x'00') = 0 AND trim(NEW.trashed_time) <> '' AND length(CAST(NEW.trashed_time AS BLOB)) <= 64))
    AND (NEW.trash_owner IS NULL OR (typeof(NEW.trash_owner) = 'text' AND instr(CAST(NEW.trash_owner AS BLOB), x'00') = 0 AND length(NEW.trash_owner) <= 262144 AND length(CAST(NEW.trash_owner AS BLOB)) <= 262144))
    AND (typeof(NEW.archived) = 'integer' AND NEW.archived IN (0, 1))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_pages_update
BEFORE UPDATE OF created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, url, public_url, title, child_block_id, parent_data_source_id, folder_id, project_id, parent_row_page_id, icon, cover, in_trash, trashed_time, trash_owner, archived, last_edited_time ON notes_pages
WHEN ((typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (NEW.public_url IS NULL OR (typeof(NEW.public_url) = 'text' AND instr(CAST(NEW.public_url AS BLOB), x'00') = 0 AND length(NEW.public_url) <= 262144 AND length(CAST(NEW.public_url AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (NEW.child_block_id IS NULL OR (typeof(NEW.child_block_id) = 'text' AND instr(CAST(NEW.child_block_id AS BLOB), x'00') = 0 AND trim(NEW.child_block_id) <> '' AND length(CAST(NEW.child_block_id AS BLOB)) <= 128))
    AND (NEW.parent_data_source_id IS NULL OR (typeof(NEW.parent_data_source_id) = 'text' AND instr(CAST(NEW.parent_data_source_id AS BLOB), x'00') = 0 AND trim(NEW.parent_data_source_id) <> '' AND length(CAST(NEW.parent_data_source_id AS BLOB)) <= 128))
    AND (NEW.folder_id IS NULL OR (typeof(NEW.folder_id) = 'text' AND instr(CAST(NEW.folder_id AS BLOB), x'00') = 0 AND trim(NEW.folder_id) <> '' AND length(CAST(NEW.folder_id AS BLOB)) <= 128))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (NEW.parent_row_page_id IS NULL OR (typeof(NEW.parent_row_page_id) = 'text' AND instr(CAST(NEW.parent_row_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_row_page_id) <> '' AND length(CAST(NEW.parent_row_page_id AS BLOB)) <= 128))
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.trashed_time IS NULL OR (typeof(NEW.trashed_time) = 'text' AND instr(CAST(NEW.trashed_time AS BLOB), x'00') = 0 AND trim(NEW.trashed_time) <> '' AND length(CAST(NEW.trashed_time AS BLOB)) <= 64))
    AND (NEW.trash_owner IS NULL OR (typeof(NEW.trash_owner) = 'text' AND instr(CAST(NEW.trash_owner AS BLOB), x'00') = 0 AND length(NEW.trash_owner) <= 262144 AND length(CAST(NEW.trash_owner AS BLOB)) <= 262144))
    AND (typeof(NEW.archived) = 'integer' AND NEW.archived IN (0, 1))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_pages
BEFORE UPDATE OF id, created_time ON notes_pages
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_pages
BEFORE INSERT ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 290 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_pages WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_pages_insert
AFTER INSERT ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (290, NEW.id, 1023, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_pages_update
AFTER UPDATE ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 290, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY OR OLD.url IS NOT NEW.url COLLATE BINARY OR OLD.public_url IS NOT NEW.public_url COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN (OLD.child_block_id IS NOT NEW.child_block_id COLLATE BINARY
                AND NOT (NEW.child_block_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_blocks WHERE id = OLD.child_block_id))) OR (OLD.parent_data_source_id IS NOT NEW.parent_data_source_id COLLATE BINARY
                AND NOT (NEW.parent_data_source_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_data_sources WHERE id = OLD.parent_data_source_id))) OR (OLD.folder_id IS NOT NEW.folder_id COLLATE BINARY
                AND NOT (NEW.folder_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_folders WHERE id = OLD.folder_id)))
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.project_id IS NOT NEW.project_id COLLATE BINARY
                AND NOT (NEW.project_id IS NULL AND NOT EXISTS (SELECT 1 FROM projects WHERE id = OLD.project_id))
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.parent_row_page_id IS NOT NEW.parent_row_page_id COLLATE BINARY
                AND NOT (NEW.parent_row_page_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_pages WHERE id = OLD.parent_row_page_id))
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.cover IS NOT NEW.cover COLLATE BINARY
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.in_trash IS NOT NEW.in_trash OR OLD.trashed_time IS NOT NEW.trashed_time COLLATE BINARY OR OLD.trash_owner IS NOT NEW.trash_owner COLLATE BINARY
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.archived IS NOT NEW.archived
                THEN 256 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 512 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_pages_delete
AFTER DELETE ON notes_pages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (290, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA foreign_keys = ON;

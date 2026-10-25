-- no-transaction
-- Notes structure replication. The local user, folders, pages, page aliases, and blocks become
-- replicated tables. A page's `parent_type`, `parent_page_id`, and `parent_block_id` and a
-- block's `page_id` and `plain_text` are maintained by Notes code and the Notes sync hook from
-- replicated placement, so they lose their foreign keys and the checks that tie them to
-- replicated columns. `parent_data_source_id` loses its foreign key while data sources stay on
-- the vault owner; Notes deletes row pages with their data source. Remote changes queue page
-- history snapshots in the device-local `notes_page_history_pending`.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE notes_pages_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL DEFAULT 'workspace'
        CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT,
    parent_block_id TEXT,
    parent_data_source_id TEXT,
    child_block_id TEXT REFERENCES "notes_blocks"(id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
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
    CHECK (folder_id IS NULL OR (child_block_id IS NULL AND parent_data_source_id IS NULL))
);

INSERT INTO notes_pages_next (
    id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, child_block_id,
    project_id, title, icon, cover, in_trash, trash_owner, archived, source_provider,
    source_object_id, source_workspace_id, source_last_edited_time, url, public_url,
    created_time, last_edited_time, trashed_time, folder_id, parent_row_page_id
)
SELECT
    id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, child_block_id,
    project_id, title, icon, cover, in_trash, trash_owner, archived, source_provider,
    source_object_id, source_workspace_id, source_last_edited_time, url, public_url,
    created_time, last_edited_time, trashed_time, folder_id, parent_row_page_id
FROM notes_pages;

DROP TABLE notes_pages;
ALTER TABLE notes_pages_next RENAME TO notes_pages;

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

-- `page_id` is empty only while a received block waits for the Notes hook to find its page.
CREATE TABLE notes_blocks_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL DEFAULT '',
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES "notes_blocks"(id) ON DELETE CASCADE,
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    trash_owner TEXT CHECK (trash_owner IS NULL OR trim(trash_owner) <> ''),
    type TEXT NOT NULL DEFAULT 'paragraph' CHECK (
        type IN (
            'paragraph',
            'heading_1',
            'heading_2',
            'heading_3',
            'heading_4',
            'heading_5',
            'heading_6',
            'bulleted_list_item',
            'numbered_list_item',
            'to_do',
            'toggle',
            'callout',
            'quote',
            'child_page',
            'child_database',
            'breadcrumb',
            'table_of_contents',
            'column_list',
            'column',
            'table',
            'table_row',
            'tab',
            'image',
            'video',
            'audio',
            'file',
            'pdf',
            'bookmark',
            'link_preview',
            'synced_block',
            'template',
            'button',
            'embed',
            'equation',
            'divider',
            'code',
            'unsupported'
        )
    ),
    attrs TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(attrs) AND json_type(attrs) = 'object'),
    text TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(text) AND json_type(text) = 'object'),
    plain_text TEXT NOT NULL DEFAULT '',
    order_key TEXT NOT NULL CHECK (length(order_key) BETWEEN 1 AND 128),
    source_provider TEXT,
    source_object_id TEXT,
    source_last_edited_time TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    CHECK ((parent_page_id IS NULL) <> (parent_block_id IS NULL))
);

INSERT INTO notes_blocks_next (
    id, page_id, parent_type, parent_page_id, parent_block_id, in_trash, trash_owner, type,
    attrs, text, plain_text, order_key, source_provider, source_object_id,
    source_last_edited_time, created_time, last_edited_time
)
SELECT
    id, page_id, parent_type, parent_page_id, parent_block_id, in_trash, trash_owner, type,
    attrs, text, plain_text, order_key, source_provider, source_object_id,
    source_last_edited_time, created_time, last_edited_time
FROM notes_blocks;

DROP TABLE notes_blocks;
ALTER TABLE notes_blocks_next RENAME TO notes_blocks;

CREATE INDEX idx_notes_blocks_page ON notes_blocks(page_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_parent_block ON notes_blocks(parent_block_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_parent_page ON notes_blocks(parent_page_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_source ON notes_blocks(source_provider, source_object_id);

-- Pages whose replicated structure changed remotely and still need a history snapshot.
CREATE TABLE notes_page_history_pending (
    page_id TEXT PRIMARY KEY REFERENCES notes_pages(id) ON DELETE CASCADE,
    queued_at TEXT NOT NULL CHECK (trim(queued_at) <> '')
);

-- Row pages leave with their data source on every path that removes it, including foreign key
-- cascades from a purged block or database. The walk collects the whole content subtree in
-- this one statement because SQLite does not fire this trigger again for data sources that
-- the deleted pages cascade away while it runs.
CREATE TRIGGER notes_data_sources_delete_row_pages
AFTER DELETE ON notes_data_sources
BEGIN
    DELETE FROM notes_pages
    WHERE id IN (
        WITH RECURSIVE subtree(id) AS (
            SELECT id FROM notes_pages WHERE parent_data_source_id = OLD.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN subtree AS parent ON child.parent_page_id = parent.id
            UNION
            SELECT child.id
            FROM notes_pages AS child
            JOIN notes_blocks AS parent_block ON child.parent_block_id = parent_block.id
            JOIN subtree AS parent ON parent_block.page_id = parent.id
            UNION
            SELECT row_page.id
            FROM notes_pages AS row_page
            JOIN notes_data_sources AS source ON source.id = row_page.parent_data_source_id
            JOIN notes_databases AS db ON db.id = source.database_id
            JOIN notes_blocks AS db_block ON db_block.id = db.block_id
            JOIN subtree AS parent ON db_block.page_id = parent.id
        )
        SELECT id FROM subtree
    );
END;

CREATE TEMP TABLE notes_structure_replication_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_structure_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_structure_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_notes_local_users_insert
BEFORE INSERT ON notes_local_users
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_local_users_update
BEFORE UPDATE OF created_time ON notes_local_users
WHEN ((typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_local_users
BEFORE UPDATE OF id, created_time ON notes_local_users
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_local_users
BEFORE INSERT ON notes_local_users
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 288 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_local_users WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_local_users_insert
AFTER INSERT ON notes_local_users
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (288, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_local_users_update
AFTER UPDATE ON notes_local_users
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 288, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_local_users_delete
AFTER DELETE ON notes_local_users
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (288, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_folders_insert
BEFORE INSERT ON notes_folders
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.parent_folder_id IS NULL OR (typeof(NEW.parent_folder_id) = 'text' AND instr(CAST(NEW.parent_folder_id AS BLOB), x'00') = 0 AND trim(NEW.parent_folder_id) <> '' AND length(CAST(NEW.parent_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_folders_update
BEFORE UPDATE OF project_id, created_time, name, parent_folder_id, last_edited_time ON notes_folders
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.parent_folder_id IS NULL OR (typeof(NEW.parent_folder_id) = 'text' AND instr(CAST(NEW.parent_folder_id AS BLOB), x'00') = 0 AND trim(NEW.parent_folder_id) <> '' AND length(CAST(NEW.parent_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_folders
BEFORE UPDATE OF id, project_id, created_time ON notes_folders
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_folders
BEFORE INSERT ON notes_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 289 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_folders WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_folders_insert
AFTER INSERT ON notes_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (289, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_folders_update
AFTER UPDATE ON notes_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 289, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.parent_folder_id IS NOT NEW.parent_folder_id COLLATE BINARY
                AND NOT (NEW.parent_folder_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_folders WHERE id = OLD.parent_folder_id))
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_folders_delete
AFTER DELETE ON notes_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (289, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
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
                AND NOT (NEW.child_block_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_blocks WHERE id = OLD.child_block_id))) OR OLD.parent_data_source_id IS NOT NEW.parent_data_source_id COLLATE BINARY OR (OLD.folder_id IS NOT NEW.folder_id COLLATE BINARY
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

CREATE TRIGGER sync_values_notes_blocks_insert
BEFORE INSERT ON notes_blocks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.parent_page_id IS NULL OR (typeof(NEW.parent_page_id) = 'text' AND instr(CAST(NEW.parent_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_page_id) <> '' AND length(CAST(NEW.parent_page_id AS BLOB)) <= 128))
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND trim(NEW.parent_block_id) <> '' AND length(CAST(NEW.parent_block_id AS BLOB)) <= 128))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.attrs) = 'text' AND instr(CAST(NEW.attrs AS BLOB), x'00') = 0 AND length(NEW.attrs) <= 65536 AND length(CAST(NEW.attrs AS BLOB)) <= 262144)
    AND (typeof(NEW.text) = 'text' AND instr(CAST(NEW.text AS BLOB), x'00') = 0 AND length(NEW.text) <= 262144 AND length(CAST(NEW.text AS BLOB)) <= 262144)
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.trash_owner IS NULL OR (typeof(NEW.trash_owner) = 'text' AND instr(CAST(NEW.trash_owner AS BLOB), x'00') = 0 AND length(NEW.trash_owner) <= 262144 AND length(CAST(NEW.trash_owner AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_blocks_update
BEFORE UPDATE OF created_time, source_provider, source_object_id, source_last_edited_time, parent_page_id, parent_block_id, order_key, type, attrs, text, in_trash, trash_owner, last_edited_time ON notes_blocks
WHEN ((typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.parent_page_id IS NULL OR (typeof(NEW.parent_page_id) = 'text' AND instr(CAST(NEW.parent_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_page_id) <> '' AND length(CAST(NEW.parent_page_id AS BLOB)) <= 128))
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND trim(NEW.parent_block_id) <> '' AND length(CAST(NEW.parent_block_id AS BLOB)) <= 128))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.attrs) = 'text' AND instr(CAST(NEW.attrs AS BLOB), x'00') = 0 AND length(NEW.attrs) <= 65536 AND length(CAST(NEW.attrs AS BLOB)) <= 262144)
    AND (typeof(NEW.text) = 'text' AND instr(CAST(NEW.text AS BLOB), x'00') = 0 AND length(NEW.text) <= 262144 AND length(CAST(NEW.text AS BLOB)) <= 262144)
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.trash_owner IS NULL OR (typeof(NEW.trash_owner) = 'text' AND instr(CAST(NEW.trash_owner AS BLOB), x'00') = 0 AND length(NEW.trash_owner) <= 262144 AND length(CAST(NEW.trash_owner AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_blocks
BEFORE UPDATE OF id ON notes_blocks
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_blocks
BEFORE INSERT ON notes_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 292 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_blocks WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_blocks_insert
AFTER INSERT ON notes_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (292, NEW.id, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_blocks_update
AFTER UPDATE ON notes_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 292, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_time IS NOT NEW.created_time COLLATE BINARY OR OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN (OLD.parent_page_id IS NOT NEW.parent_page_id COLLATE BINARY
                AND NOT (NEW.parent_page_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_pages WHERE id = OLD.parent_page_id))) OR (OLD.parent_block_id IS NOT NEW.parent_block_id COLLATE BINARY
                AND NOT (NEW.parent_block_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_blocks WHERE id = OLD.parent_block_id)))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.type IS NOT NEW.type COLLATE BINARY OR OLD.attrs IS NOT NEW.attrs COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.text IS NOT NEW.text COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.in_trash IS NOT NEW.in_trash OR OLD.trash_owner IS NOT NEW.trash_owner COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_blocks_delete
AFTER DELETE ON notes_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (292, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_page_aliases_insert
BEFORE INSERT ON notes_page_aliases
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.page_id IS NULL OR (typeof(NEW.page_id) = 'text' AND instr(CAST(NEW.page_id AS BLOB), x'00') = 0 AND trim(NEW.page_id) <> '' AND length(CAST(NEW.page_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.alias) = 'text' AND instr(CAST(NEW.alias AS BLOB), x'00') = 0 AND length(NEW.alias) <= 200 AND length(CAST(NEW.alias AS BLOB)) <= 262144)
    AND (typeof(NEW.normalized_alias) = 'text' AND instr(CAST(NEW.normalized_alias AS BLOB), x'00') = 0 AND length(NEW.normalized_alias) <= 262144 AND length(CAST(NEW.normalized_alias AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_page_aliases_update
BEFORE UPDATE OF page_id, created_time, alias, normalized_alias, last_edited_time ON notes_page_aliases
WHEN ((NEW.page_id IS NULL OR (typeof(NEW.page_id) = 'text' AND instr(CAST(NEW.page_id AS BLOB), x'00') = 0 AND trim(NEW.page_id) <> '' AND length(CAST(NEW.page_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.alias) = 'text' AND instr(CAST(NEW.alias AS BLOB), x'00') = 0 AND length(NEW.alias) <= 200 AND length(CAST(NEW.alias AS BLOB)) <= 262144)
    AND (typeof(NEW.normalized_alias) = 'text' AND instr(CAST(NEW.normalized_alias AS BLOB), x'00') = 0 AND length(NEW.normalized_alias) <= 262144 AND length(CAST(NEW.normalized_alias AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_page_aliases
BEFORE UPDATE OF id, page_id, created_time ON notes_page_aliases
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.page_id IS NOT NEW.page_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_page_aliases
BEFORE INSERT ON notes_page_aliases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 291 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_page_aliases WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_page_aliases_insert
AFTER INSERT ON notes_page_aliases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (291, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_page_aliases_update
AFTER UPDATE ON notes_page_aliases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 291, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.alias IS NOT NEW.alias COLLATE BINARY OR OLD.normalized_alias IS NOT NEW.normalized_alias COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_page_aliases_delete
AFTER DELETE ON notes_page_aliases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (291, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

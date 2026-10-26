-- no-transaction
-- Notes databases and templates replication. Databases, data sources, properties, options,
-- property values, relation edges, views, and page and data source templates become replicated
-- tables, with template blocks as owned rows of their template's content.
--
-- `notes_pages.parent_data_source_id` becomes a foreign key that cascades, so removing a data
-- source removes its row pages through foreign keys on every path, and the trigger that did it
-- while data sources stayed on the vault owner is dropped. Row pages whose data source is absent
-- (possible only on replicas before this step) become workspace pages instead of being lost.
-- A database's parent columns are maintained by Notes code and the Notes sync hook from its
-- block, so they lose their foreign keys and the check that ties them together. Views order by
-- an order key instead of a REAL sort order. An option names its property by the property's
-- `sync_key`, so it cascades with the property on every device. Template blocks lose their
-- per-template id index, self foreign key, and value checks (parent kind, block type, payload
-- JSON): owned rows are replaced as a whole and hold any value their column types allow, so
-- Notes writes and the sync adapter validate them instead. A database's `block_id` loses its
-- unique index and its check against the key: it is a replicated reference that Notes writes
-- equal to the id, like a nested page's `child_block_id`.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

DROP TRIGGER notes_data_sources_delete_row_pages;

UPDATE notes_pages
SET parent_type = 'workspace',
    parent_data_source_id = NULL,
    parent_row_page_id = NULL
WHERE parent_data_source_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1 FROM notes_data_sources WHERE notes_data_sources.id = notes_pages.parent_data_source_id
  );

CREATE TABLE notes_pages_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL DEFAULT 'workspace'
        CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT,
    parent_block_id TEXT,
    parent_data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
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

-- Local writes keep a nested page paired with its `child_page` block: a page nested under a
-- page or a block names the block that shares its id, and no other page names a block. Remote
-- placement is paired by the Notes structure repair, because apply derives the parent columns
-- only after the rows land.
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

-- A page moved out of its parent keeps its trashed `child_page` block, whose id is the page id.
-- Before a local delete of the page that holds such a block, the block moves under its own
-- page, still trashed, so its published key stays live and nesting the page again rewrites the
-- block in place instead of creating a deleted key again. Remote deletes come from the writer
-- that already parked its blocks, so apply skips this.
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

-- `parent_type`, `parent_page_id`, and `parent_block_id` follow the database's block.
CREATE TABLE notes_databases_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    block_id TEXT NOT NULL REFERENCES "notes_blocks"(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    parent_type TEXT NOT NULL DEFAULT 'page_id' CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT,
    parent_block_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    title_rich_text TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(title_rich_text)),
    description TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(description)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    is_inline INTEGER NOT NULL DEFAULT 1 CHECK (is_inline IN (0, 1)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    url TEXT,
    public_url TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);

INSERT INTO notes_databases_next (
    id, block_id, parent_type, parent_page_id, parent_block_id, title, title_rich_text,
    description, icon, cover, is_inline, in_trash, source_provider, source_object_id,
    source_workspace_id, source_last_edited_time, url, public_url, created_time,
    last_edited_time
)
SELECT
    id, block_id, parent_type, parent_page_id, parent_block_id, title, title_rich_text,
    description, icon, cover, is_inline, in_trash, source_provider, source_object_id,
    source_workspace_id, source_last_edited_time, url, public_url, created_time,
    last_edited_time
FROM notes_databases;

DROP TABLE notes_databases;
ALTER TABLE notes_databases_next RENAME TO notes_databases;

CREATE INDEX idx_notes_databases_parent ON notes_databases(parent_type, parent_page_id, parent_block_id);
CREATE INDEX idx_notes_databases_source ON notes_databases(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_database_views_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    database_id TEXT NOT NULL REFERENCES notes_databases(id) ON DELETE CASCADE,
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    type TEXT NOT NULL CHECK (
        type IN (
            'table',
            'board',
            'list',
            'calendar',
            'timeline',
            'gallery',
            'form',
            'chart',
            'map',
            'dashboard'
        )
    ),
    filter TEXT CHECK (filter IS NULL OR json_valid(filter)),
    sorts TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sorts)),
    configuration TEXT CHECK (configuration IS NULL OR json_valid(configuration)),
    order_key TEXT NOT NULL DEFAULT 'a0' CHECK (trim(order_key) <> ''),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    url TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);

-- Views keep their order within their database as rank keys.
INSERT INTO notes_database_views_next (
    id, database_id, data_source_id, name, type, filter, sorts, configuration, order_key,
    source_provider, source_object_id, source_workspace_id, source_last_edited_time, url,
    created_time, last_edited_time
)
SELECT
    view.id, view.database_id, view.data_source_id, view.name, view.type, view.filter,
    view.sorts, view.configuration,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank % 62 + 1, 1),
    view.source_provider, view.source_object_id, view.source_workspace_id,
    view.source_last_edited_time, view.url, view.created_time, view.last_edited_time
FROM notes_database_views AS view
JOIN (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY database_id ORDER BY sort_order, created_time, id
        ) - 1 AS rank
    FROM notes_database_views
) AS ranked ON ranked.id = view.id;

DROP TABLE notes_database_views;
ALTER TABLE notes_database_views_next RENAME TO notes_database_views;

CREATE INDEX idx_notes_database_views_data_source ON notes_database_views(data_source_id, order_key, id);
CREATE INDEX idx_notes_database_views_database ON notes_database_views(database_id, order_key, id);
CREATE INDEX idx_notes_database_views_source ON notes_database_views(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_data_source_property_options_next (
    data_source_id TEXT NOT NULL,
    property_id TEXT NOT NULL,
    option_id TEXT NOT NULL CHECK (trim(option_id) <> ''),
    sync_key TEXT NOT NULL CHECK (sync_key <> '' AND length(sync_key) <= 128),
    property_key TEXT NOT NULL REFERENCES notes_data_source_properties(sync_key)
        ON DELETE CASCADE ON UPDATE CASCADE,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT 'default',
    status_group TEXT,
    position TEXT NOT NULL CHECK (trim(position) <> ''),
    PRIMARY KEY (data_source_id, property_id, option_id)
);

INSERT INTO notes_data_source_property_options_next (
    data_source_id, property_id, option_id, sync_key, property_key, name, color, status_group,
    position
)
SELECT
    option.data_source_id, option.property_id, option.option_id, option.sync_key,
    property.sync_key, option.name, option.color, option.status_group, option.position
FROM notes_data_source_property_options AS option
JOIN notes_data_source_properties AS property
  ON property.data_source_id = option.data_source_id
 AND property.property_id = option.property_id;

DROP TABLE notes_data_source_property_options;
ALTER TABLE notes_data_source_property_options_next RENAME TO notes_data_source_property_options;

CREATE UNIQUE INDEX idx_notes_data_source_property_options_sync_key
    ON notes_data_source_property_options(sync_key);
CREATE INDEX idx_notes_data_source_property_options_property
    ON notes_data_source_property_options(property_key);

CREATE TABLE notes_page_template_blocks_next (
    template_id TEXT NOT NULL REFERENCES notes_page_templates(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL,
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    payload TEXT NOT NULL,
    plain_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    type TEXT NOT NULL DEFAULT 'paragraph',
    PRIMARY KEY (template_id, ordinal)
);

INSERT INTO notes_page_template_blocks_next (
    template_id, ordinal, id, parent_type, parent_block_id, has_children, payload, plain_text,
    created_time, last_edited_time, type
)
SELECT
    template_id, ordinal, id, parent_type, parent_block_id, has_children, payload, plain_text,
    created_time, last_edited_time, type
FROM notes_page_template_blocks;

DROP TABLE notes_page_template_blocks;
ALTER TABLE notes_page_template_blocks_next RENAME TO notes_page_template_blocks;

CREATE INDEX idx_notes_page_template_blocks_parent
    ON notes_page_template_blocks(template_id, parent_block_id, ordinal);

CREATE TABLE notes_data_source_template_blocks_next (
    template_id TEXT NOT NULL REFERENCES notes_data_source_templates(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL,
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    type TEXT NOT NULL,
    payload TEXT NOT NULL,
    plain_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (template_id, ordinal)
);

INSERT INTO notes_data_source_template_blocks_next (
    template_id, ordinal, id, parent_type, parent_block_id, has_children, type, payload,
    plain_text, created_time, last_edited_time
)
SELECT
    template_id, ordinal, id, parent_type, parent_block_id, has_children, type, payload,
    plain_text, created_time, last_edited_time
FROM notes_data_source_template_blocks;

DROP TABLE notes_data_source_template_blocks;
ALTER TABLE notes_data_source_template_blocks_next RENAME TO notes_data_source_template_blocks;

CREATE INDEX idx_notes_data_source_template_blocks_parent
    ON notes_data_source_template_blocks(template_id, parent_block_id, ordinal);

CREATE TEMP TABLE notes_databases_replication_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_databases_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_databases_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

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

CREATE TRIGGER sync_values_notes_databases_insert
BEFORE INSERT ON notes_databases
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (NEW.public_url IS NULL OR (typeof(NEW.public_url) = 'text' AND instr(CAST(NEW.public_url AS BLOB), x'00') = 0 AND length(NEW.public_url) <= 262144 AND length(CAST(NEW.public_url AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.title_rich_text) = 'text' AND instr(CAST(NEW.title_rich_text AS BLOB), x'00') = 0 AND length(NEW.title_rich_text) <= 262144 AND length(CAST(NEW.title_rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.is_inline) = 'integer' AND NEW.is_inline IN (0, 1))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_databases_update
BEFORE UPDATE OF block_id, created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, url, public_url, title, title_rich_text, description, icon, cover, is_inline, in_trash, last_edited_time ON notes_databases
WHEN ((NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (NEW.public_url IS NULL OR (typeof(NEW.public_url) = 'text' AND instr(CAST(NEW.public_url AS BLOB), x'00') = 0 AND length(NEW.public_url) <= 262144 AND length(CAST(NEW.public_url AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.title_rich_text) = 'text' AND instr(CAST(NEW.title_rich_text AS BLOB), x'00') = 0 AND length(NEW.title_rich_text) <= 262144 AND length(CAST(NEW.title_rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.is_inline) = 'integer' AND NEW.is_inline IN (0, 1))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_databases
BEFORE UPDATE OF id, block_id, created_time ON notes_databases
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.block_id IS NOT NEW.block_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_databases
BEFORE INSERT ON notes_databases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 293 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_databases WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_databases_insert
AFTER INSERT ON notes_databases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (293, NEW.id, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_databases_update
AFTER UPDATE ON notes_databases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 293, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY OR OLD.url IS NOT NEW.url COLLATE BINARY OR OLD.public_url IS NOT NEW.public_url COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY OR OLD.title_rich_text IS NOT NEW.title_rich_text COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.description IS NOT NEW.description COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.cover IS NOT NEW.cover COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.is_inline IS NOT NEW.is_inline
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.in_trash IS NOT NEW.in_trash
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 128 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_databases_delete
AFTER DELETE ON notes_databases
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (293, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_data_sources_insert
BEFORE INSERT ON notes_data_sources
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.database_id IS NULL OR (typeof(NEW.database_id) = 'text' AND instr(CAST(NEW.database_id AS BLOB), x'00') = 0 AND trim(NEW.database_id) <> '' AND length(CAST(NEW.database_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.title_rich_text) = 'text' AND instr(CAST(NEW.title_rich_text AS BLOB), x'00') = 0 AND length(NEW.title_rich_text) <= 262144 AND length(CAST(NEW.title_rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.default_template_id IS NULL OR (typeof(NEW.default_template_id) = 'text' AND instr(CAST(NEW.default_template_id AS BLOB), x'00') = 0 AND trim(NEW.default_template_id) <> '' AND length(CAST(NEW.default_template_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_data_sources_update
BEFORE UPDATE OF database_id, created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, title, title_rich_text, description, icon, in_trash, default_template_id, last_edited_time ON notes_data_sources
WHEN ((NEW.database_id IS NULL OR (typeof(NEW.database_id) = 'text' AND instr(CAST(NEW.database_id AS BLOB), x'00') = 0 AND trim(NEW.database_id) <> '' AND length(CAST(NEW.database_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.title_rich_text) = 'text' AND instr(CAST(NEW.title_rich_text AS BLOB), x'00') = 0 AND length(NEW.title_rich_text) <= 262144 AND length(CAST(NEW.title_rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (typeof(NEW.in_trash) = 'integer' AND NEW.in_trash IN (0, 1))
    AND (NEW.default_template_id IS NULL OR (typeof(NEW.default_template_id) = 'text' AND instr(CAST(NEW.default_template_id AS BLOB), x'00') = 0 AND trim(NEW.default_template_id) <> '' AND length(CAST(NEW.default_template_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_data_sources
BEFORE UPDATE OF id, database_id, created_time ON notes_data_sources
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.database_id IS NOT NEW.database_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_data_sources
BEFORE INSERT ON notes_data_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 294 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_data_sources WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_data_sources_insert
AFTER INSERT ON notes_data_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (294, NEW.id, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_sources_update
AFTER UPDATE ON notes_data_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 294, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY OR OLD.title_rich_text IS NOT NEW.title_rich_text COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.description IS NOT NEW.description COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.in_trash IS NOT NEW.in_trash
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.default_template_id IS NOT NEW.default_template_id COLLATE BINARY
                AND NOT (NEW.default_template_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_data_source_templates WHERE id = OLD.default_template_id))
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 128 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_data_sources_delete
AFTER DELETE ON notes_data_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (294, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_data_source_templates_insert
BEFORE INSERT ON notes_data_source_templates
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.properties) = 'text' AND instr(CAST(NEW.properties AS BLOB), x'00') = 0 AND length(NEW.properties) <= 262144 AND length(CAST(NEW.properties AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_data_source_templates_update
BEFORE UPDATE OF data_source_id, created_time, source_page_id, name, properties, last_edited_time ON notes_data_source_templates
WHEN ((NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.properties) = 'text' AND instr(CAST(NEW.properties AS BLOB), x'00') = 0 AND length(NEW.properties) <= 262144 AND length(CAST(NEW.properties AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_data_source_templates
BEFORE UPDATE OF id, data_source_id, created_time ON notes_data_source_templates
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.data_source_id IS NOT NEW.data_source_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_data_source_templates
BEFORE INSERT ON notes_data_source_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 302 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_data_source_templates WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_data_source_templates_insert
AFTER INSERT ON notes_data_source_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (302, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_source_templates_update
AFTER UPDATE ON notes_data_source_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 302, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_page_id IS NOT NEW.source_page_id COLLATE BINARY
                AND NOT (NEW.source_page_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_pages WHERE id = OLD.source_page_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.properties IS NOT NEW.properties COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_data_source_templates_delete
AFTER DELETE ON notes_data_source_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (302, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_data_source_template_blocks_insert
BEFORE INSERT ON notes_data_source_template_blocks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 200 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND length(NEW.parent_block_id) <= 200 AND length(CAST(NEW.parent_block_id AS BLOB)) <= 262144))
    AND (typeof(NEW.has_children) = 'integer' AND NEW.has_children IN (0, 1))
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.payload) = 'text' AND instr(CAST(NEW.payload AS BLOB), x'00') = 0 AND length(NEW.payload) <= 262144 AND length(CAST(NEW.payload AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_data_source_template_blocks_update
BEFORE UPDATE OF id, parent_type, parent_block_id, has_children, type, payload, plain_text, created_time, last_edited_time ON notes_data_source_template_blocks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 200 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND length(NEW.parent_block_id) <= 200 AND length(CAST(NEW.parent_block_id AS BLOB)) <= 262144))
    AND (typeof(NEW.has_children) = 'integer' AND NEW.has_children IN (0, 1))
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.payload) = 'text' AND instr(CAST(NEW.payload AS BLOB), x'00') = 0 AND length(NEW.payload) <= 262144 AND length(CAST(NEW.payload AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_notes_data_source_template_blocks_insert
AFTER INSERT ON notes_data_source_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (302, NEW.template_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_data_source_template_blocks_update
AFTER UPDATE ON notes_data_source_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 302, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.template_id AS key UNION SELECT OLD.template_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_data_source_template_blocks_delete
AFTER DELETE ON notes_data_source_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (302, OLD.template_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_notes_data_source_properties_insert
BEFORE INSERT ON notes_data_source_properties
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(trim(NEW.property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(trim(NEW.type)) BETWEEN 1 AND 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.config) = 'text' AND instr(CAST(NEW.config AS BLOB), x'00') = 0 AND length(NEW.config) <= 262144 AND length(CAST(NEW.config AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_data_source_properties_update
BEFORE UPDATE OF data_source_id, property_id, name, type, config, order_key ON notes_data_source_properties
WHEN ((NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(trim(NEW.property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(trim(NEW.type)) BETWEEN 1 AND 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.config) = 'text' AND instr(CAST(NEW.config AS BLOB), x'00') = 0 AND length(NEW.config) <= 262144 AND length(CAST(NEW.config AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_data_source_properties
BEFORE UPDATE OF sync_key, data_source_id, property_id ON notes_data_source_properties
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.data_source_id IS NOT NEW.data_source_id COLLATE BINARY OR OLD.property_id IS NOT NEW.property_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_notes_data_source_properties_insert
AFTER INSERT ON notes_data_source_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (295, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_source_properties_update
AFTER UPDATE ON notes_data_source_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 295, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.type IS NOT NEW.type COLLATE BINARY OR OLD.config IS NOT NEW.config COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 295, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_source_properties_delete
AFTER DELETE ON notes_data_source_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (295, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_data_source_property_options_insert
BEFORE INSERT ON notes_data_source_property_options
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND length(NEW.data_source_id) <= 200 AND length(CAST(NEW.data_source_id AS BLOB)) <= 262144)
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(NEW.property_id) <= 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.option_id) = 'text' AND instr(CAST(NEW.option_id AS BLOB), x'00') = 0 AND length(trim(NEW.option_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.option_id AS BLOB)) <= 262144)
    AND (NEW.property_key IS NULL OR (typeof(NEW.property_key) = 'text' AND instr(CAST(NEW.property_key AS BLOB), x'00') = 0 AND trim(NEW.property_key) <> '' AND length(CAST(NEW.property_key AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'text' AND instr(CAST(NEW.color AS BLOB), x'00') = 0 AND length(NEW.color) <= 200 AND length(CAST(NEW.color AS BLOB)) <= 262144)
    AND (NEW.status_group IS NULL OR (typeof(NEW.status_group) = 'text' AND instr(CAST(NEW.status_group AS BLOB), x'00') = 0 AND length(NEW.status_group) <= 200 AND length(CAST(NEW.status_group AS BLOB)) <= 262144))
    AND (typeof(NEW.position) = 'text' AND instr(CAST(NEW.position AS BLOB), x'00') = 0 AND NEW.position NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.position) <= 128 AND (CASE WHEN unicode(NEW.position) BETWEEN 97 AND 122 THEN unicode(NEW.position) - 95 WHEN unicode(NEW.position) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.position) ELSE 129 END) <= length(NEW.position) AND ((CASE WHEN unicode(NEW.position) BETWEEN 97 AND 122 THEN unicode(NEW.position) - 95 WHEN unicode(NEW.position) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.position) ELSE 129 END) = length(NEW.position) OR substr(NEW.position, -1) <> '0') AND NEW.position <> 'A00000000000000000000000000')) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_data_source_property_options_update
BEFORE UPDATE OF data_source_id, property_id, option_id, property_key, name, color, status_group, position ON notes_data_source_property_options
WHEN ((typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND length(NEW.data_source_id) <= 200 AND length(CAST(NEW.data_source_id AS BLOB)) <= 262144)
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(NEW.property_id) <= 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.option_id) = 'text' AND instr(CAST(NEW.option_id AS BLOB), x'00') = 0 AND length(trim(NEW.option_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.option_id AS BLOB)) <= 262144)
    AND (NEW.property_key IS NULL OR (typeof(NEW.property_key) = 'text' AND instr(CAST(NEW.property_key AS BLOB), x'00') = 0 AND trim(NEW.property_key) <> '' AND length(CAST(NEW.property_key AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'text' AND instr(CAST(NEW.color AS BLOB), x'00') = 0 AND length(NEW.color) <= 200 AND length(CAST(NEW.color AS BLOB)) <= 262144)
    AND (NEW.status_group IS NULL OR (typeof(NEW.status_group) = 'text' AND instr(CAST(NEW.status_group AS BLOB), x'00') = 0 AND length(NEW.status_group) <= 200 AND length(CAST(NEW.status_group AS BLOB)) <= 262144))
    AND (typeof(NEW.position) = 'text' AND instr(CAST(NEW.position AS BLOB), x'00') = 0 AND NEW.position NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.position) <= 128 AND (CASE WHEN unicode(NEW.position) BETWEEN 97 AND 122 THEN unicode(NEW.position) - 95 WHEN unicode(NEW.position) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.position) ELSE 129 END) <= length(NEW.position) AND ((CASE WHEN unicode(NEW.position) BETWEEN 97 AND 122 THEN unicode(NEW.position) - 95 WHEN unicode(NEW.position) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.position) ELSE 129 END) = length(NEW.position) OR substr(NEW.position, -1) <> '0') AND NEW.position <> 'A00000000000000000000000000')) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_data_source_property_options
BEFORE UPDATE OF sync_key, data_source_id, property_id, option_id ON notes_data_source_property_options
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.data_source_id IS NOT NEW.data_source_id COLLATE BINARY OR OLD.property_id IS NOT NEW.property_id COLLATE BINARY OR OLD.option_id IS NOT NEW.option_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_notes_data_source_property_options_insert
AFTER INSERT ON notes_data_source_property_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (296, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_source_property_options_update
AFTER UPDATE ON notes_data_source_property_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 296, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.property_key IS NOT NEW.property_key COLLATE BINARY
                AND NOT (NEW.property_key IS NULL AND NOT EXISTS (SELECT 1 FROM notes_data_source_properties WHERE sync_key = OLD.property_key))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.status_group IS NOT NEW.status_group COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.position IS NOT NEW.position COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 296, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_data_source_property_options_delete
AFTER DELETE ON notes_data_source_property_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (296, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_page_property_values_insert
BEFORE INSERT ON notes_page_property_values
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.page_id IS NULL OR (typeof(NEW.page_id) = 'text' AND instr(CAST(NEW.page_id AS BLOB), x'00') = 0 AND trim(NEW.page_id) <> '' AND length(CAST(NEW.page_id AS BLOB)) <= 128))
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(trim(NEW.property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.value_type) = 'text' AND instr(CAST(NEW.value_type AS BLOB), x'00') = 0 AND length(trim(NEW.value_type)) BETWEEN 1 AND 200 AND length(CAST(NEW.value_type AS BLOB)) <= 262144)
    AND (typeof(NEW.value) = 'text' AND instr(CAST(NEW.value AS BLOB), x'00') = 0 AND length(NEW.value) <= 262144 AND length(CAST(NEW.value AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_page_property_values_update
BEFORE UPDATE OF page_id, property_id, value_type, value ON notes_page_property_values
WHEN ((NEW.page_id IS NULL OR (typeof(NEW.page_id) = 'text' AND instr(CAST(NEW.page_id AS BLOB), x'00') = 0 AND trim(NEW.page_id) <> '' AND length(CAST(NEW.page_id AS BLOB)) <= 128))
    AND (typeof(NEW.property_id) = 'text' AND instr(CAST(NEW.property_id AS BLOB), x'00') = 0 AND length(trim(NEW.property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.property_id AS BLOB)) <= 262144)
    AND (typeof(NEW.value_type) = 'text' AND instr(CAST(NEW.value_type AS BLOB), x'00') = 0 AND length(trim(NEW.value_type)) BETWEEN 1 AND 200 AND length(CAST(NEW.value_type AS BLOB)) <= 262144)
    AND (typeof(NEW.value) = 'text' AND instr(CAST(NEW.value AS BLOB), x'00') = 0 AND length(NEW.value) <= 262144 AND length(CAST(NEW.value AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_page_property_values
BEFORE UPDATE OF sync_key, page_id, property_id ON notes_page_property_values
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.page_id IS NOT NEW.page_id COLLATE BINARY OR OLD.property_id IS NOT NEW.property_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_notes_page_property_values_insert
AFTER INSERT ON notes_page_property_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (297, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_page_property_values_update
AFTER UPDATE ON notes_page_property_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 297, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.value_type IS NOT NEW.value_type COLLATE BINARY OR OLD.value IS NOT NEW.value COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 297, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_page_property_values_delete
AFTER DELETE ON notes_page_property_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (297, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_relation_edges_insert
BEFORE INSERT ON notes_relation_edges
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.source_property_id) = 'text' AND instr(CAST(NEW.source_property_id AS BLOB), x'00') = 0 AND length(trim(NEW.source_property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.source_property_id AS BLOB)) <= 262144)
    AND (NEW.target_page_id IS NULL OR (typeof(NEW.target_page_id) = 'text' AND instr(CAST(NEW.target_page_id AS BLOB), x'00') = 0 AND trim(NEW.target_page_id) <> '' AND length(CAST(NEW.target_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_relation_edges_update
BEFORE UPDATE OF source_page_id, source_property_id, target_page_id, created_time ON notes_relation_edges
WHEN ((NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.source_property_id) = 'text' AND instr(CAST(NEW.source_property_id AS BLOB), x'00') = 0 AND length(trim(NEW.source_property_id)) BETWEEN 1 AND 200 AND length(CAST(NEW.source_property_id AS BLOB)) <= 262144)
    AND (NEW.target_page_id IS NULL OR (typeof(NEW.target_page_id) = 'text' AND instr(CAST(NEW.target_page_id AS BLOB), x'00') = 0 AND trim(NEW.target_page_id) <> '' AND length(CAST(NEW.target_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_relation_edges
BEFORE UPDATE OF sync_key, source_page_id, source_property_id, target_page_id ON notes_relation_edges
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.source_page_id IS NOT NEW.source_page_id COLLATE BINARY OR OLD.source_property_id IS NOT NEW.source_property_id COLLATE BINARY OR OLD.target_page_id IS NOT NEW.target_page_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_notes_relation_edges_insert
AFTER INSERT ON notes_relation_edges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (298, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_relation_edges_update
AFTER UPDATE ON notes_relation_edges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 298, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_time IS NOT NEW.created_time COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 298, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_relation_edges_delete
AFTER DELETE ON notes_relation_edges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (298, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_database_views_insert
BEFORE INSERT ON notes_database_views
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.database_id IS NULL OR (typeof(NEW.database_id) = 'text' AND instr(CAST(NEW.database_id AS BLOB), x'00') = 0 AND trim(NEW.database_id) <> '' AND length(CAST(NEW.database_id AS BLOB)) <= 128))
    AND (NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (NEW.filter IS NULL OR (typeof(NEW.filter) = 'text' AND instr(CAST(NEW.filter AS BLOB), x'00') = 0 AND length(NEW.filter) <= 262144 AND length(CAST(NEW.filter AS BLOB)) <= 262144))
    AND (typeof(NEW.sorts) = 'text' AND instr(CAST(NEW.sorts AS BLOB), x'00') = 0 AND length(NEW.sorts) <= 262144 AND length(CAST(NEW.sorts AS BLOB)) <= 262144)
    AND (NEW.configuration IS NULL OR (typeof(NEW.configuration) = 'text' AND instr(CAST(NEW.configuration AS BLOB), x'00') = 0 AND length(NEW.configuration) <= 262144 AND length(CAST(NEW.configuration AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_database_views_update
BEFORE UPDATE OF database_id, data_source_id, created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, url, name, type, filter, sorts, configuration, order_key, last_edited_time ON notes_database_views
WHEN ((NEW.database_id IS NULL OR (typeof(NEW.database_id) = 'text' AND instr(CAST(NEW.database_id AS BLOB), x'00') = 0 AND trim(NEW.database_id) <> '' AND length(CAST(NEW.database_id AS BLOB)) <= 128))
    AND (NEW.data_source_id IS NULL OR (typeof(NEW.data_source_id) = 'text' AND instr(CAST(NEW.data_source_id AS BLOB), x'00') = 0 AND trim(NEW.data_source_id) <> '' AND length(CAST(NEW.data_source_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (NEW.url IS NULL OR (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (NEW.filter IS NULL OR (typeof(NEW.filter) = 'text' AND instr(CAST(NEW.filter AS BLOB), x'00') = 0 AND length(NEW.filter) <= 262144 AND length(CAST(NEW.filter AS BLOB)) <= 262144))
    AND (typeof(NEW.sorts) = 'text' AND instr(CAST(NEW.sorts AS BLOB), x'00') = 0 AND length(NEW.sorts) <= 262144 AND length(CAST(NEW.sorts AS BLOB)) <= 262144)
    AND (NEW.configuration IS NULL OR (typeof(NEW.configuration) = 'text' AND instr(CAST(NEW.configuration AS BLOB), x'00') = 0 AND length(NEW.configuration) <= 262144 AND length(CAST(NEW.configuration AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_database_views
BEFORE UPDATE OF id, database_id, data_source_id, created_time ON notes_database_views
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.database_id IS NOT NEW.database_id COLLATE BINARY OR OLD.data_source_id IS NOT NEW.data_source_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_database_views
BEFORE INSERT ON notes_database_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 299 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_database_views WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_database_views_insert
AFTER INSERT ON notes_database_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (299, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_database_views_update
AFTER UPDATE ON notes_database_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 299, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY OR OLD.url IS NOT NEW.url COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.type IS NOT NEW.type COLLATE BINARY OR OLD.filter IS NOT NEW.filter COLLATE BINARY OR OLD.sorts IS NOT NEW.sorts COLLATE BINARY OR OLD.configuration IS NOT NEW.configuration COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_database_views_delete
AFTER DELETE ON notes_database_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (299, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_page_templates_insert
BEFORE INSERT ON notes_page_templates
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.properties) = 'text' AND instr(CAST(NEW.properties AS BLOB), x'00') = 0 AND length(NEW.properties) <= 262144 AND length(CAST(NEW.properties AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_page_templates_update
BEFORE UPDATE OF created_time, source_page_id, name, properties, icon, cover, last_edited_time ON notes_page_templates
WHEN ((typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_page_id IS NULL OR (typeof(NEW.source_page_id) = 'text' AND instr(CAST(NEW.source_page_id AS BLOB), x'00') = 0 AND trim(NEW.source_page_id) <> '' AND length(CAST(NEW.source_page_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.properties) = 'text' AND instr(CAST(NEW.properties AS BLOB), x'00') = 0 AND length(NEW.properties) <= 262144 AND length(CAST(NEW.properties AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 262144 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (NEW.cover IS NULL OR (typeof(NEW.cover) = 'text' AND instr(CAST(NEW.cover AS BLOB), x'00') = 0 AND length(NEW.cover) <= 262144 AND length(CAST(NEW.cover AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_page_templates
BEFORE UPDATE OF id, created_time ON notes_page_templates
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_page_templates
BEFORE INSERT ON notes_page_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 300 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_page_templates WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_page_templates_insert
AFTER INSERT ON notes_page_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (300, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_page_templates_update
AFTER UPDATE ON notes_page_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 300, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_page_id IS NOT NEW.source_page_id COLLATE BINARY
                AND NOT (NEW.source_page_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_pages WHERE id = OLD.source_page_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.properties IS NOT NEW.properties COLLATE BINARY OR OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.cover IS NOT NEW.cover COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_page_templates_delete
AFTER DELETE ON notes_page_templates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (300, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_page_template_blocks_insert
BEFORE INSERT ON notes_page_template_blocks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 200 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND length(NEW.parent_block_id) <= 200 AND length(CAST(NEW.parent_block_id AS BLOB)) <= 262144))
    AND (typeof(NEW.has_children) = 'integer' AND NEW.has_children IN (0, 1))
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.payload) = 'text' AND instr(CAST(NEW.payload AS BLOB), x'00') = 0 AND length(NEW.payload) <= 262144 AND length(CAST(NEW.payload AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_page_template_blocks_update
BEFORE UPDATE OF id, parent_type, parent_block_id, has_children, type, payload, plain_text, created_time, last_edited_time ON notes_page_template_blocks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 200 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND length(NEW.parent_block_id) <= 200 AND length(CAST(NEW.parent_block_id AS BLOB)) <= 262144))
    AND (typeof(NEW.has_children) = 'integer' AND NEW.has_children IN (0, 1))
    AND (typeof(NEW.type) = 'text' AND instr(CAST(NEW.type AS BLOB), x'00') = 0 AND length(NEW.type) <= 200 AND length(CAST(NEW.type AS BLOB)) <= 262144)
    AND (typeof(NEW.payload) = 'text' AND instr(CAST(NEW.payload AS BLOB), x'00') = 0 AND length(NEW.payload) <= 262144 AND length(CAST(NEW.payload AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_notes_page_template_blocks_insert
AFTER INSERT ON notes_page_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (300, NEW.template_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_page_template_blocks_update
AFTER UPDATE ON notes_page_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 300, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.template_id AS key UNION SELECT OLD.template_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_page_template_blocks_delete
AFTER DELETE ON notes_page_template_blocks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (300, OLD.template_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

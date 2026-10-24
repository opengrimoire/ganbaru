-- no-transaction
-- Notes databases preparation for concurrent sync. A database names its child_database block in
-- `block_id`, so deleting the block deletes the database. Template blocks are keyed by their
-- depth-first ordinal inside the template instead of a REAL sort order, and a data source names
-- its default template in `default_template_id` instead of a flag on each template.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- notes_databases: the database shares its id with its child_database block; `block_id` carries
-- the reference so the id itself has no foreign key.
CREATE TABLE notes_databases_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    block_id TEXT NOT NULL UNIQUE REFERENCES "notes_blocks"(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED CHECK (block_id = id),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES "notes_blocks"(id) ON DELETE CASCADE,
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
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    CHECK (
        (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
        )
    )
);
INSERT INTO notes_databases_next (
    id, block_id, parent_type, parent_page_id, parent_block_id, title, title_rich_text,
    description, icon, cover, is_inline, in_trash, source_provider, source_object_id,
    source_workspace_id, source_last_edited_time, url, public_url, created_time, last_edited_time
)
SELECT
    id, id, parent_type, parent_page_id, parent_block_id, title, title_rich_text, description,
    icon, cover, is_inline, in_trash, source_provider, source_object_id, source_workspace_id,
    source_last_edited_time, url, public_url, created_time, last_edited_time
FROM notes_databases;
DROP TABLE notes_databases;
ALTER TABLE notes_databases_next RENAME TO notes_databases;
CREATE INDEX idx_notes_databases_parent ON notes_databases(parent_type, parent_page_id, parent_block_id);
CREATE INDEX idx_notes_databases_source ON notes_databases(source_provider, source_workspace_id, source_object_id);

-- Template blocks: a template is one owned snapshot, so its blocks are keyed by their depth-first
-- ordinal (parents before children, siblings in their previous `sort_order`). The block id stays
-- unique inside the template for the parent link, which is deferred so a whole snapshot can be
-- written in any order. Blocks unreachable from a template root are dropped.
CREATE TEMP TABLE notes_page_template_block_ordinals AS
WITH RECURSIVE walk(template_id, id, path, depth) AS (
    SELECT template_id, id, printf('%020.6f:%s', sort_order, id), 1
    FROM notes_page_template_blocks
    WHERE parent_type = 'template'
    UNION
    SELECT child.template_id, child.id,
           walk.path || '/' || printf('%020.6f:%s', child.sort_order, child.id), walk.depth + 1
    FROM notes_page_template_blocks AS child
    JOIN walk ON child.template_id = walk.template_id AND child.parent_block_id = walk.id
    WHERE walk.depth < 64
)
SELECT template_id, id,
       ROW_NUMBER() OVER (PARTITION BY template_id ORDER BY path) - 1 AS ordinal
FROM walk;

CREATE TABLE notes_page_template_blocks_next (
    template_id TEXT NOT NULL REFERENCES notes_page_templates(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('template', 'block_id')),
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    plain_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
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
    PRIMARY KEY (template_id, ordinal),
    UNIQUE (template_id, id),
    FOREIGN KEY (template_id, parent_block_id)
        REFERENCES "notes_page_template_blocks"(template_id, id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CHECK (
        (
            parent_type = 'template'
            AND parent_block_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_block_id IS NOT NULL
        )
    )
);
INSERT INTO notes_page_template_blocks_next (
    template_id, ordinal, id, parent_type, parent_block_id, has_children, payload, plain_text,
    created_time, last_edited_time, type
)
SELECT
    block.template_id, ordinals.ordinal, block.id, block.parent_type, block.parent_block_id,
    block.has_children, block.payload, block.plain_text, block.created_time,
    block.last_edited_time, block.type
FROM notes_page_template_blocks AS block
JOIN notes_page_template_block_ordinals AS ordinals
  ON ordinals.template_id = block.template_id AND ordinals.id = block.id;
DROP TABLE notes_page_template_block_ordinals;
DROP TABLE notes_page_template_blocks;
ALTER TABLE notes_page_template_blocks_next RENAME TO notes_page_template_blocks;
CREATE INDEX idx_notes_page_template_blocks_parent
    ON notes_page_template_blocks(template_id, parent_block_id, ordinal);

-- notes_data_source_templates: the default moves to the data source.
ALTER TABLE notes_data_sources ADD COLUMN default_template_id TEXT
    REFERENCES notes_data_source_templates(id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED;
UPDATE notes_data_sources
SET default_template_id = (
    SELECT template.id FROM notes_data_source_templates AS template
    WHERE template.data_source_id = notes_data_sources.id AND template.is_default = 1
    ORDER BY template.id
    LIMIT 1
);
CREATE INDEX idx_notes_data_sources_default_template
    ON notes_data_sources(default_template_id);

CREATE TABLE notes_data_source_templates_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    source_page_id TEXT REFERENCES notes_pages(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    properties TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(properties)),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO notes_data_source_templates_next (
    id, data_source_id, source_page_id, name, properties, created_time, last_edited_time
)
SELECT id, data_source_id, source_page_id, name, properties, created_time, last_edited_time
FROM notes_data_source_templates;
DROP TABLE notes_data_source_templates;
ALTER TABLE notes_data_source_templates_next RENAME TO notes_data_source_templates;
CREATE INDEX idx_notes_data_source_templates_source
    ON notes_data_source_templates(data_source_id, last_edited_time DESC, name);
CREATE INDEX idx_notes_data_source_templates_source_page
    ON notes_data_source_templates(source_page_id);

CREATE TEMP TABLE notes_data_source_template_block_ordinals AS
WITH RECURSIVE walk(template_id, id, path, depth) AS (
    SELECT template_id, id, printf('%020.6f:%s', sort_order, id), 1
    FROM notes_data_source_template_blocks
    WHERE parent_type = 'template'
    UNION
    SELECT child.template_id, child.id,
           walk.path || '/' || printf('%020.6f:%s', child.sort_order, child.id), walk.depth + 1
    FROM notes_data_source_template_blocks AS child
    JOIN walk ON child.template_id = walk.template_id AND child.parent_block_id = walk.id
    WHERE walk.depth < 64
)
SELECT template_id, id,
       ROW_NUMBER() OVER (PARTITION BY template_id ORDER BY path) - 1 AS ordinal
FROM walk;

CREATE TABLE notes_data_source_template_blocks_next (
    template_id TEXT NOT NULL REFERENCES notes_data_source_templates(id) ON DELETE CASCADE
        DEFERRABLE INITIALLY DEFERRED,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('template', 'block_id')),
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    type TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    plain_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (template_id, ordinal),
    UNIQUE (template_id, id),
    FOREIGN KEY (template_id, parent_block_id)
        REFERENCES "notes_data_source_template_blocks"(template_id, id)
        ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    CHECK (
        (
            parent_type = 'template'
            AND parent_block_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_block_id IS NOT NULL
        )
    )
);
INSERT INTO notes_data_source_template_blocks_next (
    template_id, ordinal, id, parent_type, parent_block_id, has_children, type, payload,
    plain_text, created_time, last_edited_time
)
SELECT
    block.template_id, ordinals.ordinal, block.id, block.parent_type, block.parent_block_id,
    block.has_children, block.type, block.payload, block.plain_text, block.created_time,
    block.last_edited_time
FROM notes_data_source_template_blocks AS block
JOIN notes_data_source_template_block_ordinals AS ordinals
  ON ordinals.template_id = block.template_id AND ordinals.id = block.id;
DROP TABLE notes_data_source_template_block_ordinals;
DROP TABLE notes_data_source_template_blocks;
ALTER TABLE notes_data_source_template_blocks_next RENAME TO notes_data_source_template_blocks;
CREATE INDEX idx_notes_data_source_template_blocks_parent
    ON notes_data_source_template_blocks(template_id, parent_block_id, ordinal);

-- Data source properties and select, multi-select, and status options move out of the schema JSON
-- into rows. The schema object is keyed by property name, so the name is the object key; the
-- property row keeps every other field in `config` except the options and the status group
-- membership, which the option rows carry. Properties and options keep their JSON order as rank
-- order keys. Rows get a placeholder sync key that the Notes conversion pass replaces at vault
-- open.
CREATE TABLE notes_data_source_properties (
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL CHECK (trim(property_id) <> ''),
    sync_key TEXT NOT NULL CHECK (sync_key <> '' AND length(sync_key) <= 128),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    type TEXT NOT NULL CHECK (trim(type) <> ''),
    config TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config) AND json_type(config) = 'object'),
    order_key TEXT NOT NULL CHECK (trim(order_key) <> ''),
    PRIMARY KEY (data_source_id, property_id)
);
CREATE UNIQUE INDEX idx_notes_data_source_properties_sync_key
    ON notes_data_source_properties(sync_key);
CREATE INDEX idx_notes_data_source_properties_order
    ON notes_data_source_properties(data_source_id, order_key, property_id);

CREATE TABLE notes_data_source_property_options (
    data_source_id TEXT NOT NULL,
    property_id TEXT NOT NULL,
    option_id TEXT NOT NULL CHECK (trim(option_id) <> ''),
    sync_key TEXT NOT NULL CHECK (sync_key <> '' AND length(sync_key) <= 128),
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT 'default',
    status_group TEXT,
    position TEXT NOT NULL CHECK (trim(position) <> ''),
    PRIMARY KEY (data_source_id, property_id, option_id),
    FOREIGN KEY (data_source_id, property_id)
        REFERENCES notes_data_source_properties(data_source_id, property_id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX idx_notes_data_source_property_options_sync_key
    ON notes_data_source_property_options(sync_key);

CREATE TEMP TABLE notes_data_source_property_sources AS
SELECT
    data_source.id AS data_source_id,
    property.key AS name,
    COALESCE(
        json_extract(property.value, '$.id'),
        CASE WHEN json_extract(property.value, '$.type') = 'title' THEN 'title' END
    ) AS property_id,
    json_extract(property.value, '$.type') AS type,
    property.value AS value,
    ROW_NUMBER() OVER (PARTITION BY data_source.id ORDER BY property.id) - 1 AS rank
FROM notes_data_sources AS data_source, json_each(data_source.properties) AS property
WHERE json_valid(data_source.properties)
  AND CASE WHEN property.type = 'object' THEN json_type(property.value, '$.type') END = 'text'
  AND trim(property.key) <> '';

INSERT OR IGNORE INTO notes_data_source_properties (
    data_source_id, property_id, sync_key, name, type, config, order_key
)
SELECT
    source.data_source_id,
    source.property_id,
    'pending:p' || ROW_NUMBER() OVER (ORDER BY source.data_source_id, source.rank),
    source.name,
    source.type,
    CASE
        WHEN source.type IN ('select', 'multi_select') THEN
            json_remove(source.value, '$.id', '$.name', '$.type', '$.' || source.type || '.options')
        WHEN source.type = 'status' AND json_type(source.value, '$.status.groups') = 'array' THEN
            json_set(
                json_remove(source.value, '$.id', '$.name', '$.type', '$.status.options'),
                '$.status.groups',
                json((
                    SELECT json_group_array(json(json_remove(grp.value, '$.option_ids')) ORDER BY grp.id)
                    FROM json_each(source.value, '$.status.groups') AS grp
                    WHERE grp.type = 'object'
                ))
            )
        WHEN source.type = 'status' THEN
            json_remove(source.value, '$.id', '$.name', '$.type', '$.status.options')
        ELSE json_remove(source.value, '$.id', '$.name', '$.type')
    END,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', source.rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', source.rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', source.rank % 62 + 1, 1)
FROM notes_data_source_property_sources AS source
WHERE typeof(source.property_id) = 'text' AND trim(source.property_id) <> '';

INSERT OR IGNORE INTO notes_data_source_property_options (
    data_source_id, property_id, option_id, sync_key, name, color, status_group, position
)
SELECT
    ranked.data_source_id,
    ranked.property_id,
    ranked.option_id,
    'pending:o' || ROW_NUMBER() OVER (ORDER BY ranked.data_source_id, ranked.property_id, ranked.rank),
    ranked.name,
    ranked.color,
    ranked.status_group,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank % 62 + 1, 1)
FROM (
    SELECT
        source.data_source_id,
        source.property_id,
        json_extract(option.value, '$.id') AS option_id,
        COALESCE(json_extract(option.value, '$.name'), '') AS name,
        COALESCE(json_extract(option.value, '$.color'), 'default') AS color,
        CASE WHEN source.type = 'status' THEN (
            SELECT json_extract(grp.value, '$.name')
            FROM json_each(source.value, '$.status.groups') AS grp,
                 json_each(CASE WHEN grp.type = 'object' THEN grp.value ELSE '{}' END, '$.option_ids') AS member
            WHERE member.value = json_extract(option.value, '$.id')
            ORDER BY grp.id
            LIMIT 1
        ) END AS status_group,
        ROW_NUMBER() OVER (
            PARTITION BY source.data_source_id, source.property_id ORDER BY option.id
        ) - 1 AS rank
    FROM notes_data_source_property_sources AS source,
         json_each(source.value, '$.' || source.type || '.options') AS option
    WHERE source.type IN ('select', 'multi_select', 'status')
      AND CASE WHEN option.type = 'object' THEN
          json_type(option.value, '$.id') = 'text' AND trim(json_extract(option.value, '$.id')) <> ''
      ELSE 0 END
) AS ranked
JOIN notes_data_source_properties AS property
  ON property.data_source_id = ranked.data_source_id AND property.property_id = ranked.property_id;

DROP TABLE notes_data_source_property_sources;
ALTER TABLE notes_data_sources DROP COLUMN properties;

CREATE TEMP TABLE notes_sync_databases_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_sync_databases_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_sync_databases_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

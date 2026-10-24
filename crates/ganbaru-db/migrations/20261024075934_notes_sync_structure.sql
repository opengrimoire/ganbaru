-- no-transaction
-- Notes structure preparation for concurrent sync. The local user keeps only its id and creation
-- time; local comment, suggestion, and reaction authors store no name and readers take the
-- replicated profile.displayName preference, while imported authors keep their stored name.
-- Collaboration operation records and sync_version counters are dropped because sync and history
-- replace them. Foreign keys that blocked deleting a local user become actions, and page history
-- keeps its author id without a foreign key because history is device-local. Managed assets are
-- named `{class}:{sha256}` so devices that record the same bytes agree on the id; rows that shared
-- bytes merge into the first recorded path, and the per-page icon and cover tables give way to
-- asset references.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

DROP TABLE notes_collaboration_operations;

-- notes_local_users
DROP INDEX idx_notes_local_users_display_name;
ALTER TABLE notes_local_users DROP COLUMN display_name;
ALTER TABLE notes_local_users DROP COLUMN last_edited_time;

-- notes_comment_threads
ALTER TABLE notes_comment_threads DROP COLUMN sync_version;

-- notes_comments
CREATE TABLE notes_comments_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    thread_id TEXT NOT NULL REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    rich_text TEXT NOT NULL CHECK (json_valid(rich_text)),
    plain_text TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    display_name TEXT CHECK (display_name IS NULL OR json_valid(display_name)),
    attachments TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(attachments)),
    deleted_at TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT
);
INSERT INTO notes_comments_next (
    id, thread_id, rich_text, plain_text, created_by, display_name, attachments, deleted_at,
    created_time, last_edited_time, source_provider, source_object_id, source_workspace_id,
    source_last_edited_time
)
SELECT
    id, thread_id, rich_text, plain_text, created_by,
    CASE
        WHEN created_by IN (SELECT id FROM notes_local_users)
            AND json_extract(display_name, '$.type') = 'user' THEN NULL
        ELSE display_name
    END,
    attachments, deleted_at, created_time, last_edited_time, source_provider, source_object_id,
    source_workspace_id, source_last_edited_time
FROM notes_comments;
DROP TABLE notes_comments;
ALTER TABLE notes_comments_next RENAME TO notes_comments;
CREATE INDEX idx_notes_comments_source
    ON notes_comments(source_provider, source_workspace_id, source_object_id);
CREATE INDEX idx_notes_comments_thread ON notes_comments(thread_id, deleted_at, created_time, id);

-- notes_comment_reactions
CREATE TABLE notes_comment_reactions_next (
    comment_id TEXT NOT NULL REFERENCES notes_comments(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    display_name TEXT CHECK (display_name IS NULL OR json_valid(display_name)),
    emoji TEXT NOT NULL CHECK (trim(emoji) <> '' AND length(emoji) <= 16),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_time) <> ''),
    PRIMARY KEY (comment_id, created_by, emoji)
);
INSERT INTO notes_comment_reactions_next (comment_id, created_by, display_name, emoji, created_time)
SELECT
    comment_id, created_by,
    CASE
        WHEN created_by IN (SELECT id FROM notes_local_users)
            AND json_extract(display_name, '$.type') = 'user' THEN NULL
        ELSE display_name
    END,
    emoji, created_time
FROM notes_comment_reactions;
DROP TABLE notes_comment_reactions;
ALTER TABLE notes_comment_reactions_next RENAME TO notes_comment_reactions;

-- notes_suggestions
CREATE TABLE notes_suggestions_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL REFERENCES notes_blocks(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL REFERENCES notes_local_users(id) ON DELETE CASCADE,
    display_name TEXT CHECK (display_name IS NULL OR json_valid(display_name)),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'accepted', 'rejected')),
    range_start INTEGER NOT NULL CHECK (range_start >= 0),
    range_end INTEGER NOT NULL CHECK (range_end > range_start),
    original_text TEXT NOT NULL CHECK (trim(original_text) <> '' AND length(original_text) <= 2000),
    proposed_text TEXT NOT NULL DEFAULT '' CHECK (length(proposed_text) <= 2000),
    prefix_text TEXT NOT NULL DEFAULT '' CHECK (length(prefix_text) <= 120),
    suffix_text TEXT NOT NULL DEFAULT '' CHECK (length(suffix_text) <= 120),
    accepted_at TEXT,
    accepted_by TEXT REFERENCES notes_local_users(id) ON DELETE SET NULL,
    rejected_at TEXT,
    rejected_by TEXT REFERENCES notes_local_users(id) ON DELETE SET NULL,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    CHECK (
        (
            status = 'open'
            AND accepted_at IS NULL
            AND accepted_by IS NULL
            AND rejected_at IS NULL
            AND rejected_by IS NULL
        )
        OR (
            status = 'accepted'
            AND accepted_at IS NOT NULL
            AND rejected_at IS NULL
            AND rejected_by IS NULL
        )
        OR (
            status = 'rejected'
            AND rejected_at IS NOT NULL
            AND accepted_at IS NULL
            AND accepted_by IS NULL
        )
    )
);
INSERT INTO notes_suggestions_next (
    id, page_id, block_id, created_by, display_name, status, range_start, range_end, original_text,
    proposed_text, prefix_text, suffix_text, accepted_at, accepted_by, rejected_at, rejected_by,
    created_time, last_edited_time
)
SELECT
    id, page_id, block_id, created_by,
    CASE WHEN json_extract(display_name, '$.type') = 'user' THEN NULL ELSE display_name END,
    status, range_start, range_end, original_text, proposed_text, prefix_text, suffix_text,
    accepted_at, accepted_by, rejected_at, rejected_by, created_time, last_edited_time
FROM notes_suggestions;
DROP TABLE notes_suggestions;
ALTER TABLE notes_suggestions_next RENAME TO notes_suggestions;
CREATE INDEX idx_notes_suggestions_block_range
    ON notes_suggestions(block_id, range_start, range_end, status);
CREATE INDEX idx_notes_suggestions_page
    ON notes_suggestions(page_id, status, created_time, id);

-- notes_page_history_snapshots
CREATE TABLE notes_page_history_snapshots_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_type TEXT NOT NULL CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT,
    parent_block_id TEXT,
    parent_data_source_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    properties TEXT NOT NULL CHECK (json_valid(properties)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    blocks TEXT NOT NULL CHECK (json_valid(blocks)),
    block_count INTEGER NOT NULL DEFAULT 0 CHECK (block_count >= 0),
    reason TEXT NOT NULL CHECK (trim(reason) <> ''),
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    page_created_time TEXT NOT NULL CHECK (trim(page_created_time) <> ''),
    page_last_edited_time TEXT NOT NULL CHECK (trim(page_last_edited_time) <> ''),
    block_bundle_hash TEXT REFERENCES notes_history_bundles(hash) ON DELETE NO ACTION,
    folder_id TEXT,
    CHECK (
        (
            parent_type = 'workspace'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'data_source_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NOT NULL
        )
    )
);
INSERT INTO notes_page_history_snapshots_next (
    id, page_id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, title,
    properties, icon, cover, in_trash, archived, blocks, block_count, reason, created_by,
    created_time, page_created_time, page_last_edited_time, block_bundle_hash, folder_id
)
SELECT
    id, page_id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, title,
    properties, icon, cover, in_trash, archived, blocks, block_count, reason, created_by,
    created_time, page_created_time, page_last_edited_time, block_bundle_hash, folder_id
FROM notes_page_history_snapshots;
DROP TABLE notes_page_history_snapshots;
ALTER TABLE notes_page_history_snapshots_next RENAME TO notes_page_history_snapshots;
CREATE INDEX idx_notes_page_history_snapshots_bundle
    ON notes_page_history_snapshots(block_bundle_hash)
    WHERE block_bundle_hash IS NOT NULL;
CREATE INDEX idx_notes_page_history_snapshots_created
    ON notes_page_history_snapshots(created_time);
CREATE INDEX idx_notes_page_history_snapshots_created_by
    ON notes_page_history_snapshots(created_by, created_time DESC, id);
CREATE INDEX idx_notes_page_history_snapshots_page
    ON notes_page_history_snapshots(page_id, created_time DESC, id);

-- notes_assets: each row is named by its class (the managed directory) and content digest, so the
-- same bytes added twice are one row. Rows that collide keep the earliest path, and stored JSON
-- that named a dropped path is rewritten to the kept one. Asset references, link facts, and history
-- pins follow the new ids, and the derived link and search indexes rebuild on next use.
CREATE TEMP TABLE notes_asset_id_map AS
WITH classified AS (
    SELECT
        id AS old_id,
        asset_path AS old_path,
        CASE
            WHEN asset_path GLOB 'notes/page-icons/*' THEN 'notes/page-icons:'
            WHEN asset_path GLOB 'notes/page-covers/*' THEN 'notes/page-covers:'
            ELSE 'notes/files:'
        END || sha256 AS new_id,
        created_at
    FROM notes_assets
)
SELECT
    old_id,
    old_path,
    new_id,
    first_value(old_path) OVER (PARTITION BY new_id ORDER BY created_at, old_path) AS new_path
FROM classified;

CREATE TEMP TABLE notes_asset_path_rewrites AS
SELECT
    row_number() OVER (ORDER BY old_path) AS step,
    old_path || '"' AS old_text,
    new_path || '"' AS new_text
FROM notes_asset_id_map
WHERE old_path <> new_path;

CREATE TABLE notes_assets_next (
    id TEXT PRIMARY KEY,
    asset_path TEXT NOT NULL UNIQUE CHECK (
        trim(asset_path) <> ''
        AND instr(asset_path, '..') = 0
        AND instr(asset_path, '\') = 0
        AND (
            (
                asset_path GLOB 'notes/page-icons/*'
                AND instr(substr(asset_path, length('notes/page-icons/') + 1), '/') = 0
                AND (
                    lower(asset_path) GLOB '*.png'
                    OR lower(asset_path) GLOB '*.jpg'
                    OR lower(asset_path) GLOB '*.jpeg'
                    OR lower(asset_path) GLOB '*.webp'
                )
            )
            OR (
                asset_path GLOB 'notes/page-covers/*'
                AND instr(substr(asset_path, length('notes/page-covers/') + 1), '/') = 0
                AND (
                    lower(asset_path) GLOB '*.png'
                    OR lower(asset_path) GLOB '*.jpg'
                    OR lower(asset_path) GLOB '*.jpeg'
                    OR lower(asset_path) GLOB '*.webp'
                )
            )
            OR (
                asset_path GLOB 'notes/files/*'
                AND instr(substr(asset_path, length('notes/files/') + 1), '/') = 0
            )
        )
    ),
    kind TEXT NOT NULL CHECK (kind IN ('image', 'video', 'audio', 'pdf', 'file')),
    source_type TEXT NOT NULL CHECK (
        source_type IN ('local_upload', 'generated', 'imported', 'external_reference')
    ),
    original_name TEXT,
    content_type TEXT NOT NULL CHECK (
        trim(content_type) <> ''
        AND instr(content_type, '/') > 1
        AND instr(content_type, ' ') = 0
    ),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    sha256 TEXT NOT NULL CHECK (
        length(sha256) = 64
        AND sha256 = lower(sha256)
        AND sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    storage_state TEXT NOT NULL DEFAULT 'available' CHECK (storage_state IN ('available', 'missing')),
    missing_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    CHECK (
        id = CASE
            WHEN asset_path GLOB 'notes/page-icons/*' THEN 'notes/page-icons:'
            WHEN asset_path GLOB 'notes/page-covers/*' THEN 'notes/page-covers:'
            ELSE 'notes/files:'
        END || sha256
    ),
    CHECK (
        (
            storage_state = 'available'
            AND missing_at IS NULL
        )
        OR (
            storage_state = 'missing'
            AND missing_at IS NOT NULL
            AND trim(missing_at) <> ''
        )
    ),
    CHECK (
        (
            kind = 'image'
            AND content_type IN ('image/png', 'image/jpeg', 'image/webp')
        )
        OR (
            kind = 'video'
            AND content_type GLOB 'video/*'
        )
        OR (
            kind = 'audio'
            AND content_type GLOB 'audio/*'
        )
        OR (
            kind = 'pdf'
            AND content_type = 'application/pdf'
        )
        OR kind = 'file'
    )
);
INSERT INTO notes_assets_next (
    id, asset_path, kind, source_type, original_name, content_type, byte_size, sha256,
    storage_state, missing_at, created_at, updated_at
)
SELECT
    map.new_id, asset.asset_path, asset.kind, asset.source_type,
    COALESCE(
        asset.original_name,
        (
            SELECT other.original_name
            FROM notes_asset_id_map AS other_map
            JOIN notes_assets AS other ON other.id = other_map.old_id
            WHERE other_map.new_id = map.new_id AND other.original_name IS NOT NULL
            ORDER BY other.created_at, other.asset_path
            LIMIT 1
        )
    ),
    asset.content_type, asset.byte_size, asset.sha256, asset.storage_state, asset.missing_at,
    asset.created_at, asset.updated_at
FROM notes_assets AS asset
JOIN notes_asset_id_map AS map ON map.old_id = asset.id
WHERE map.old_path = map.new_path;
DROP TABLE notes_assets;
ALTER TABLE notes_assets_next RENAME TO notes_assets;
CREATE INDEX idx_notes_assets_hash ON notes_assets(sha256, byte_size, content_type);
CREATE INDEX idx_notes_assets_path ON notes_assets(asset_path);
CREATE INDEX idx_notes_assets_state ON notes_assets(storage_state, updated_at DESC, id);

UPDATE OR REPLACE notes_asset_references
SET asset_id = (SELECT new_id FROM notes_asset_id_map WHERE old_id = notes_asset_references.asset_id)
WHERE asset_id IN (SELECT old_id FROM notes_asset_id_map);
DELETE FROM notes_link_facts WHERE target_asset_id IS NOT NULL;
DELETE FROM notes_link_facts_state;
DELETE FROM notes_search_index_state;

DROP TABLE notes_page_cover_assets;
DROP TABLE notes_page_icon_assets;

-- notes_project_history_asset_pins
CREATE TABLE notes_project_history_asset_pins_next (
    version_id TEXT NOT NULL REFERENCES notes_project_history_versions(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES notes_assets(id) ON DELETE NO ACTION,
    PRIMARY KEY (version_id, asset_id)
);
INSERT OR IGNORE INTO notes_project_history_asset_pins_next (version_id, asset_id)
SELECT pin.version_id, COALESCE(map.new_id, pin.asset_id)
FROM notes_project_history_asset_pins AS pin
LEFT JOIN notes_asset_id_map AS map ON map.old_id = pin.asset_id;
DROP TABLE notes_project_history_asset_pins;
ALTER TABLE notes_project_history_asset_pins_next RENAME TO notes_project_history_asset_pins;
CREATE INDEX idx_notes_project_history_asset_pins_asset
    ON notes_project_history_asset_pins(asset_id, version_id);

-- Stored JSON that named a collided path moves to the kept path. Each rewrite step applies one
-- path, so a value naming several collided paths ends with all of them rewritten.
WITH RECURSIVE steps(row_key, step, properties, icon, cover) AS (
    SELECT rowid, 0, properties, icon, cover
    FROM notes_pages
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite
        WHERE instr(properties || COALESCE(icon, '') || COALESCE(cover, ''), rewrite.old_text) > 0
    )
    UNION ALL
    SELECT
        steps.row_key, steps.step + 1,
        replace(steps.properties, rewrite.old_text, rewrite.new_text),
        replace(steps.icon, rewrite.old_text, rewrite.new_text),
        replace(steps.cover, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_pages
SET properties = (SELECT properties FROM rewritten WHERE row_key = notes_pages.rowid),
    icon = (SELECT icon FROM rewritten WHERE row_key = notes_pages.rowid),
    cover = (SELECT cover FROM rewritten WHERE row_key = notes_pages.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, properties, icon, cover) AS (
    SELECT rowid, 0, properties, icon, cover
    FROM notes_page_templates
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite
        WHERE instr(properties || COALESCE(icon, '') || COALESCE(cover, ''), rewrite.old_text) > 0
    )
    UNION ALL
    SELECT
        steps.row_key, steps.step + 1,
        replace(steps.properties, rewrite.old_text, rewrite.new_text),
        replace(steps.icon, rewrite.old_text, rewrite.new_text),
        replace(steps.cover, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_page_templates
SET properties = (SELECT properties FROM rewritten WHERE row_key = notes_page_templates.rowid),
    icon = (SELECT icon FROM rewritten WHERE row_key = notes_page_templates.rowid),
    cover = (SELECT cover FROM rewritten WHERE row_key = notes_page_templates.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, icon, cover) AS (
    SELECT rowid, 0, icon, cover
    FROM notes_databases
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite
        WHERE instr(COALESCE(icon, '') || COALESCE(cover, ''), rewrite.old_text) > 0
    )
    UNION ALL
    SELECT
        steps.row_key, steps.step + 1,
        replace(steps.icon, rewrite.old_text, rewrite.new_text),
        replace(steps.cover, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_databases
SET icon = (SELECT icon FROM rewritten WHERE row_key = notes_databases.rowid),
    cover = (SELECT cover FROM rewritten WHERE row_key = notes_databases.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, icon
    FROM notes_data_sources
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(icon, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_data_sources
SET icon = (SELECT value FROM rewritten WHERE row_key = notes_data_sources.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, properties
    FROM notes_data_source_templates
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(properties, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_data_source_templates
SET properties = (SELECT value FROM rewritten WHERE row_key = notes_data_source_templates.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, payload
    FROM notes_blocks
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(payload, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_blocks
SET payload = (SELECT value FROM rewritten WHERE row_key = notes_blocks.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, payload
    FROM notes_page_template_blocks
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(payload, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_page_template_blocks
SET payload = (SELECT value FROM rewritten WHERE row_key = notes_page_template_blocks.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, payload
    FROM notes_data_source_template_blocks
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(payload, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_data_source_template_blocks
SET payload = (SELECT value FROM rewritten WHERE row_key = notes_data_source_template_blocks.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

WITH RECURSIVE steps(row_key, step, value) AS (
    SELECT rowid, 0, attachments
    FROM notes_comments
    WHERE EXISTS (
        SELECT 1 FROM notes_asset_path_rewrites AS rewrite WHERE instr(attachments, rewrite.old_text) > 0
    )
    UNION ALL
    SELECT steps.row_key, steps.step + 1, replace(steps.value, rewrite.old_text, rewrite.new_text)
    FROM steps JOIN notes_asset_path_rewrites AS rewrite ON rewrite.step = steps.step + 1
),
rewritten AS (SELECT * FROM steps WHERE step = (SELECT COUNT(*) FROM notes_asset_path_rewrites))
UPDATE notes_comments
SET attachments = (SELECT value FROM rewritten WHERE row_key = notes_comments.rowid)
WHERE rowid IN (SELECT row_key FROM rewritten);

DROP TABLE notes_asset_path_rewrites;
DROP TABLE notes_asset_id_map;

-- Folder parents, folder and page project membership, and row hierarchy detachment are checked by
-- Notes writes instead of triggers, so applying a replicated row never aborts on a rule that a
-- concurrent edit broke; the Notes repair pass restores the rule instead.
DROP TRIGGER notes_folders_validate_parent_insert;
DROP TRIGGER notes_folders_validate_parent_update;
DROP TRIGGER notes_folders_validate_project_update;
DROP TRIGGER notes_pages_validate_folder_insert;
DROP TRIGGER notes_pages_validate_folder_update;
DROP TRIGGER notes_row_hierarchy_detach_moved_page;

-- notes_pages: the project and the trash owner move out of the properties JSON into columns. A
-- project id that names no project is dropped.
CREATE TABLE notes_pages_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT,
    parent_data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    properties TEXT NOT NULL DEFAULT '{"title":{"id":"title","type":"title","title":[]}}' CHECK (json_valid(properties)),
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
    CHECK (
        (
            parent_type = 'workspace'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'data_source_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NOT NULL
        )
    )
);
INSERT INTO notes_pages_next (
    id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, project_id, title,
    properties, icon, cover, in_trash, trash_owner, archived, source_provider, source_object_id,
    source_workspace_id, source_last_edited_time, url, public_url, created_time, last_edited_time,
    trashed_time, folder_id
)
SELECT
    page.id, page.parent_type, page.parent_page_id, page.parent_block_id,
    page.parent_data_source_id,
    (SELECT project.id FROM projects AS project
     WHERE json_type(page.properties, '$.__ganbaru_project_id') = 'text'
       AND project.id = trim(json_extract(page.properties, '$.__ganbaru_project_id'))),
    page.title,
    json_remove(page.properties, '$.__ganbaru_project_id', '$.__ganbaru_trash_owner'),
    page.icon, page.cover, page.in_trash,
    CASE WHEN json_type(page.properties, '$.__ganbaru_trash_owner') = 'text'
        THEN NULLIF(trim(json_extract(page.properties, '$.__ganbaru_trash_owner')), '') END,
    page.archived, page.source_provider, page.source_object_id, page.source_workspace_id,
    page.source_last_edited_time, page.url, page.public_url, page.created_time,
    page.last_edited_time, page.trashed_time, page.folder_id
FROM notes_pages AS page;
DROP TABLE notes_pages;
ALTER TABLE notes_pages_next RENAME TO notes_pages;
CREATE INDEX idx_notes_pages_active
    ON notes_pages(in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_data_source
    ON notes_pages(parent_data_source_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_folder
    ON notes_pages(folder_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_parent
    ON notes_pages(parent_type, parent_page_id, parent_block_id, parent_data_source_id);
CREATE INDEX idx_notes_pages_project
    ON notes_pages(project_id, in_trash, archived);
CREATE INDEX idx_notes_pages_source
    ON notes_pages(source_provider, source_workspace_id, source_object_id);
CREATE INDEX idx_notes_pages_trash_retention
    ON notes_pages(in_trash, trashed_time);

-- notes_blocks: the payload splits into `text` (the type's text keys) and `attrs` (every other
-- key); toggle open state and the database editing lock move to the device-local
-- notes_block_view_state; a child_page title is read from its page; `has_children` is derived;
-- the trash owner moves into a column; and siblings order by fractional order keys. Existing
-- blocks get rank keys ('c' and three base 62 digits, 238,328 per sibling group) from their
-- previous order, trashed blocks included so a restore returns them where they were. `page_id`
-- is written by Rust from the placement and has no foreign key; the parent references cascade.
CREATE TEMP TABLE notes_block_rank_capacity (
    row_count INTEGER NOT NULL CHECK (row_count <= 238328)
);
INSERT INTO notes_block_rank_capacity (row_count)
SELECT count(*) FROM notes_blocks GROUP BY parent_page_id, parent_block_id;
DROP TABLE notes_block_rank_capacity;

CREATE TABLE notes_blocks_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL CHECK (trim(page_id) <> ''),
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
    CHECK (
        (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_page_id = page_id
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
        )
    )
);
INSERT INTO notes_blocks_next (
    id, page_id, parent_type, parent_page_id, parent_block_id, in_trash, trash_owner, type,
    attrs, text, plain_text, order_key, source_provider, source_object_id,
    source_last_edited_time, created_time, last_edited_time
)
SELECT
    block.id,
    block.page_id,
    block.parent_type,
    block.parent_page_id,
    block.parent_block_id,
    block.in_trash,
    NULLIF(trim(json_extract(block.payload, '$.__ganbaru_trash_owner')), ''),
    block.type,
    CASE
        WHEN block.type = 'child_page' THEN json_remove(
            block.payload, '$.rich_text', '$.cells', '$.caption', '$.ganbaru_open',
            '$.editing_locked', '$.__ganbaru_trash_owner', '$.title'
        )
        ELSE json_remove(
            block.payload, '$.rich_text', '$.cells', '$.caption', '$.ganbaru_open',
            '$.editing_locked', '$.__ganbaru_trash_owner'
        )
    END,
    COALESCE(
        (
            SELECT json_group_object(
                item.key,
                CASE WHEN item.type IN ('array', 'object') THEN json(item.value) ELSE item.value END
            )
            FROM json_each(block.payload) AS item
            WHERE item.key IN ('rich_text', 'cells', 'caption')
        ),
        '{}'
    ),
    block.plain_text,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', ranked.rank % 62 + 1, 1),
    block.source_provider,
    block.source_object_id,
    block.source_last_edited_time,
    block.created_time,
    block.last_edited_time
FROM notes_blocks AS block
JOIN (
    SELECT
        id,
        ROW_NUMBER() OVER (
            PARTITION BY parent_page_id, parent_block_id ORDER BY sort_order, id
        ) - 1 AS rank
    FROM notes_blocks
) AS ranked ON ranked.id = block.id;

CREATE TABLE notes_block_view_state (
    block_id TEXT PRIMARY KEY REFERENCES "notes_blocks"(id) ON DELETE CASCADE,
    open INTEGER CHECK (open IS NULL OR open IN (0, 1)),
    editing_locked INTEGER CHECK (editing_locked IS NULL OR editing_locked IN (0, 1)),
    CHECK (open IS NOT NULL OR editing_locked IS NOT NULL)
);
INSERT INTO notes_block_view_state (block_id, open, editing_locked)
SELECT
    id,
    CASE json_type(payload, '$.ganbaru_open') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END,
    CASE json_type(payload, '$.editing_locked') WHEN 'true' THEN 1 WHEN 'false' THEN 0 END
FROM notes_blocks
WHERE json_type(payload, '$.ganbaru_open') IN ('true', 'false')
   OR json_type(payload, '$.editing_locked') IN ('true', 'false');

DROP TABLE notes_blocks;
ALTER TABLE notes_blocks_next RENAME TO notes_blocks;
CREATE INDEX idx_notes_blocks_page ON notes_blocks(page_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_parent_block ON notes_blocks(parent_block_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_parent_page ON notes_blocks(parent_page_id, in_trash, order_key, id);
CREATE INDEX idx_notes_blocks_source ON notes_blocks(source_provider, source_object_id);

-- Page history keeps page properties in the current shape; the page's project is not versioned.
UPDATE notes_page_history_snapshots
SET properties = json_remove(properties, '$.__ganbaru_project_id', '$.__ganbaru_trash_owner')
WHERE json_type(properties, '$.__ganbaru_project_id') IS NOT NULL
   OR json_type(properties, '$.__ganbaru_trash_owner') IS NOT NULL;

CREATE TEMP TABLE notes_sync_structure_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_sync_structure_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_sync_structure_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

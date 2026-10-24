-- no-transaction
-- Notes structure preparation for concurrent sync. The local user keeps only its id and creation
-- time; local comment, suggestion, and reaction authors store no name and readers take the
-- replicated profile.displayName preference, while imported authors keep their stored name.
-- Collaboration operation records and sync_version counters are dropped because sync and history
-- replace them. Foreign keys that blocked deleting a local user become actions, and page history
-- keeps its author id without a foreign key because history is device-local.
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

CREATE TEMP TABLE notes_sync_structure_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_sync_structure_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_sync_structure_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

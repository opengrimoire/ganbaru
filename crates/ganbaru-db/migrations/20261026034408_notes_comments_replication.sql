-- no-transaction
-- Notes comments replication. Comment threads, their inline anchors, comments, reactions, and
-- suggestions become replicated tables.
--
-- The `page_id` of a thread, an anchor, and a suggestion is maintained by Notes code and the
-- Notes sync hook from the parent page or block, so it loses its foreign key and the check that
-- ties a page thread's page to its parent. A reaction gains a `sync_key` derived from its
-- comment, author, and emoji; existing rows get `pending:` placeholders that the Notes
-- conversion pass at vault open rewrites to derived keys.
--
-- SQLite applies changed column constraints and foreign key actions only by rebuilding the table.
-- Foreign keys are disabled for the rebuild and every copy keeps its values. Legacy rename keeps
-- SQLite from reparsing other tables while a rebuilt table is briefly absent. The rebuild commits
-- only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE notes_comment_threads_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL DEFAULT '',
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    resolved_at TEXT,
    resolved_by TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
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
    ),
    CHECK (
        (
            status = 'open'
            AND resolved_at IS NULL
            AND resolved_by IS NULL
        )
        OR (
            status = 'resolved'
            AND resolved_at IS NOT NULL
            AND resolved_by IS NOT NULL
        )
    )
);

INSERT INTO notes_comment_threads_next (
    id, page_id, parent_type, parent_page_id, parent_block_id, status, resolved_at, resolved_by,
    created_time, last_edited_time, source_provider, source_object_id, source_workspace_id,
    source_last_edited_time
)
SELECT
    id, page_id, parent_type, parent_page_id, parent_block_id, status, resolved_at, resolved_by,
    created_time, last_edited_time, source_provider, source_object_id, source_workspace_id,
    source_last_edited_time
FROM notes_comment_threads;

DROP TABLE notes_comment_threads;
ALTER TABLE notes_comment_threads_next RENAME TO notes_comment_threads;

CREATE INDEX idx_notes_comment_threads_page ON notes_comment_threads(page_id, status, created_time, id);
CREATE INDEX idx_notes_comment_threads_parent_block ON notes_comment_threads(parent_block_id, status, created_time, id);
CREATE INDEX idx_notes_comment_threads_source
    ON notes_comment_threads(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_comment_thread_anchors_next (
    thread_id TEXT PRIMARY KEY
        REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL DEFAULT '',
    block_id TEXT NOT NULL
        REFERENCES notes_blocks(id) ON DELETE CASCADE,
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    anchor_text TEXT NOT NULL CHECK (trim(anchor_text) <> ''),
    prefix_text TEXT NOT NULL DEFAULT '',
    suffix_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(anchor_text) <= 2000),
    CHECK (length(prefix_text) <= 120),
    CHECK (length(suffix_text) <= 120)
);

INSERT INTO notes_comment_thread_anchors_next (
    thread_id, page_id, block_id, start_offset, end_offset, anchor_text, prefix_text,
    suffix_text, created_time, last_edited_time
)
SELECT
    thread_id, page_id, block_id, start_offset, end_offset, anchor_text, prefix_text,
    suffix_text, created_time, last_edited_time
FROM notes_comment_thread_anchors;

DROP TABLE notes_comment_thread_anchors;
ALTER TABLE notes_comment_thread_anchors_next RENAME TO notes_comment_thread_anchors;

CREATE INDEX idx_notes_comment_thread_anchors_block
    ON notes_comment_thread_anchors(block_id, start_offset, end_offset);
CREATE INDEX idx_notes_comment_thread_anchors_page
    ON notes_comment_thread_anchors(page_id, block_id);

CREATE TABLE notes_comment_reactions_next (
    comment_id TEXT NOT NULL REFERENCES notes_comments(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    display_name TEXT CHECK (display_name IS NULL OR json_valid(display_name)),
    emoji TEXT NOT NULL CHECK (trim(emoji) <> '' AND length(emoji) <= 16),
    sync_key TEXT NOT NULL CHECK (sync_key <> '' AND length(sync_key) <= 128),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_time) <> ''),
    PRIMARY KEY (comment_id, created_by, emoji)
);

INSERT INTO notes_comment_reactions_next (
    comment_id, created_by, display_name, emoji, sync_key, created_time
)
SELECT comment_id, created_by, display_name, emoji, 'pending:' || rowid, created_time
FROM notes_comment_reactions;

DROP TABLE notes_comment_reactions;
ALTER TABLE notes_comment_reactions_next RENAME TO notes_comment_reactions;

CREATE UNIQUE INDEX idx_notes_comment_reactions_sync_key ON notes_comment_reactions(sync_key);

CREATE TABLE notes_suggestions_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL DEFAULT '',
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
    id, page_id, block_id, created_by, display_name, status, range_start, range_end,
    original_text, proposed_text, prefix_text, suffix_text, accepted_at, accepted_by,
    rejected_at, rejected_by, created_time, last_edited_time
)
SELECT
    id, page_id, block_id, created_by, display_name, status, range_start, range_end,
    original_text, proposed_text, prefix_text, suffix_text, accepted_at, accepted_by,
    rejected_at, rejected_by, created_time, last_edited_time
FROM notes_suggestions;

DROP TABLE notes_suggestions;
ALTER TABLE notes_suggestions_next RENAME TO notes_suggestions;

CREATE INDEX idx_notes_suggestions_block_range
    ON notes_suggestions(block_id, range_start, range_end, status);
CREATE INDEX idx_notes_suggestions_page
    ON notes_suggestions(page_id, status, created_time, id);

CREATE TEMP TABLE notes_comments_replication_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_comments_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_comments_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_notes_comment_threads_insert
BEFORE INSERT ON notes_comment_threads
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_page_id IS NULL OR (typeof(NEW.parent_page_id) = 'text' AND instr(CAST(NEW.parent_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_page_id) <> '' AND length(CAST(NEW.parent_page_id AS BLOB)) <= 128))
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND trim(NEW.parent_block_id) <> '' AND length(CAST(NEW.parent_block_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 200 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.resolved_at IS NULL OR (typeof(NEW.resolved_at) = 'text' AND instr(CAST(NEW.resolved_at AS BLOB), x'00') = 0 AND trim(NEW.resolved_at) <> '' AND length(CAST(NEW.resolved_at AS BLOB)) <= 64))
    AND (NEW.resolved_by IS NULL OR (typeof(NEW.resolved_by) = 'text' AND instr(CAST(NEW.resolved_by AS BLOB), x'00') = 0 AND length(NEW.resolved_by) <= 262144 AND length(CAST(NEW.resolved_by AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_comment_threads_update
BEFORE UPDATE OF parent_type, parent_page_id, parent_block_id, created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, status, resolved_at, resolved_by, last_edited_time ON notes_comment_threads
WHEN ((typeof(NEW.parent_type) = 'text' AND instr(CAST(NEW.parent_type AS BLOB), x'00') = 0 AND length(NEW.parent_type) <= 200 AND length(CAST(NEW.parent_type AS BLOB)) <= 262144)
    AND (NEW.parent_page_id IS NULL OR (typeof(NEW.parent_page_id) = 'text' AND instr(CAST(NEW.parent_page_id AS BLOB), x'00') = 0 AND trim(NEW.parent_page_id) <> '' AND length(CAST(NEW.parent_page_id AS BLOB)) <= 128))
    AND (NEW.parent_block_id IS NULL OR (typeof(NEW.parent_block_id) = 'text' AND instr(CAST(NEW.parent_block_id AS BLOB), x'00') = 0 AND trim(NEW.parent_block_id) <> '' AND length(CAST(NEW.parent_block_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 200 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.resolved_at IS NULL OR (typeof(NEW.resolved_at) = 'text' AND instr(CAST(NEW.resolved_at AS BLOB), x'00') = 0 AND trim(NEW.resolved_at) <> '' AND length(CAST(NEW.resolved_at AS BLOB)) <= 64))
    AND (NEW.resolved_by IS NULL OR (typeof(NEW.resolved_by) = 'text' AND instr(CAST(NEW.resolved_by AS BLOB), x'00') = 0 AND length(NEW.resolved_by) <= 262144 AND length(CAST(NEW.resolved_by AS BLOB)) <= 262144))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_comment_threads
BEFORE UPDATE OF id, parent_type, parent_page_id, parent_block_id, created_time ON notes_comment_threads
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.parent_type IS NOT NEW.parent_type COLLATE BINARY OR OLD.parent_page_id IS NOT NEW.parent_page_id COLLATE BINARY OR OLD.parent_block_id IS NOT NEW.parent_block_id COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_comment_threads
BEFORE INSERT ON notes_comment_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 304 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_comment_threads WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_comment_threads_insert
AFTER INSERT ON notes_comment_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (304, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_comment_threads_update
AFTER UPDATE ON notes_comment_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 304, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.status IS NOT NEW.status COLLATE BINARY OR OLD.resolved_at IS NOT NEW.resolved_at COLLATE BINARY OR OLD.resolved_by IS NOT NEW.resolved_by COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_comment_threads_delete
AFTER DELETE ON notes_comment_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (304, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_comment_thread_anchors_insert
BEFORE INSERT ON notes_comment_thread_anchors
WHEN ((typeof(NEW.thread_id) = 'text' AND instr(CAST(NEW.thread_id AS BLOB), x'00') = 0 AND trim(NEW.thread_id) <> '' AND length(CAST(NEW.thread_id AS BLOB)) <= 128)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.anchor_text) = 'text' AND instr(CAST(NEW.anchor_text AS BLOB), x'00') = 0 AND length(NEW.anchor_text) <= 2000 AND length(CAST(NEW.anchor_text AS BLOB)) <= 262144)
    AND (typeof(NEW.prefix_text) = 'text' AND instr(CAST(NEW.prefix_text AS BLOB), x'00') = 0 AND length(NEW.prefix_text) <= 120 AND length(CAST(NEW.prefix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.suffix_text) = 'text' AND instr(CAST(NEW.suffix_text AS BLOB), x'00') = 0 AND length(NEW.suffix_text) <= 120 AND length(CAST(NEW.suffix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_comment_thread_anchors_update
BEFORE UPDATE OF created_time, block_id, start_offset, end_offset, anchor_text, prefix_text, suffix_text, last_edited_time ON notes_comment_thread_anchors
WHEN ((typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.anchor_text) = 'text' AND instr(CAST(NEW.anchor_text AS BLOB), x'00') = 0 AND length(NEW.anchor_text) <= 2000 AND length(CAST(NEW.anchor_text AS BLOB)) <= 262144)
    AND (typeof(NEW.prefix_text) = 'text' AND instr(CAST(NEW.prefix_text AS BLOB), x'00') = 0 AND length(NEW.prefix_text) <= 120 AND length(CAST(NEW.prefix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.suffix_text) = 'text' AND instr(CAST(NEW.suffix_text AS BLOB), x'00') = 0 AND length(NEW.suffix_text) <= 120 AND length(CAST(NEW.suffix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_comment_thread_anchors
BEFORE UPDATE OF thread_id, created_time ON notes_comment_thread_anchors
WHEN OLD.thread_id IS NOT NEW.thread_id COLLATE BINARY
    OR ((OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_comment_thread_anchors
BEFORE INSERT ON notes_comment_thread_anchors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 305 AND row_key = NEW.thread_id)
    AND NOT EXISTS (SELECT 1 FROM notes_comment_thread_anchors WHERE thread_id = NEW.thread_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_comment_thread_anchors_insert
AFTER INSERT ON notes_comment_thread_anchors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (305, NEW.thread_id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_comment_thread_anchors_update
AFTER UPDATE ON notes_comment_thread_anchors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 305, NEW.thread_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.block_id IS NOT NEW.block_id COLLATE BINARY
                AND NOT (NEW.block_id IS NULL AND NOT EXISTS (SELECT 1 FROM notes_blocks WHERE id = OLD.block_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.start_offset IS NOT NEW.start_offset OR OLD.end_offset IS NOT NEW.end_offset OR OLD.anchor_text IS NOT NEW.anchor_text COLLATE BINARY OR OLD.prefix_text IS NOT NEW.prefix_text COLLATE BINARY OR OLD.suffix_text IS NOT NEW.suffix_text COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_comment_thread_anchors_delete
AFTER DELETE ON notes_comment_thread_anchors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (305, OLD.thread_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_comments_insert
BEFORE INSERT ON notes_comments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.thread_id IS NULL OR (typeof(NEW.thread_id) = 'text' AND instr(CAST(NEW.thread_id AS BLOB), x'00') = 0 AND trim(NEW.thread_id) <> '' AND length(CAST(NEW.thread_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND length(NEW.created_by) <= 200 AND length(CAST(NEW.created_by AS BLOB)) <= 262144)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.rich_text) = 'text' AND instr(CAST(NEW.rich_text AS BLOB), x'00') = 0 AND length(NEW.rich_text) <= 262144 AND length(CAST(NEW.rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.attachments) = 'text' AND instr(CAST(NEW.attachments AS BLOB), x'00') = 0 AND length(NEW.attachments) <= 262144 AND length(CAST(NEW.attachments AS BLOB)) <= 262144)
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_comments_update
BEFORE UPDATE OF thread_id, created_by, display_name, created_time, source_provider, source_object_id, source_workspace_id, source_last_edited_time, rich_text, plain_text, attachments, deleted_at, last_edited_time ON notes_comments
WHEN ((NEW.thread_id IS NULL OR (typeof(NEW.thread_id) = 'text' AND instr(CAST(NEW.thread_id AS BLOB), x'00') = 0 AND trim(NEW.thread_id) <> '' AND length(CAST(NEW.thread_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND length(NEW.created_by) <= 200 AND length(CAST(NEW.created_by AS BLOB)) <= 262144)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.source_provider IS NULL OR (typeof(NEW.source_provider) = 'text' AND instr(CAST(NEW.source_provider AS BLOB), x'00') = 0 AND length(NEW.source_provider) <= 262144 AND length(CAST(NEW.source_provider AS BLOB)) <= 262144))
    AND (NEW.source_object_id IS NULL OR (typeof(NEW.source_object_id) = 'text' AND instr(CAST(NEW.source_object_id AS BLOB), x'00') = 0 AND length(NEW.source_object_id) <= 262144 AND length(CAST(NEW.source_object_id AS BLOB)) <= 262144))
    AND (NEW.source_workspace_id IS NULL OR (typeof(NEW.source_workspace_id) = 'text' AND instr(CAST(NEW.source_workspace_id AS BLOB), x'00') = 0 AND length(NEW.source_workspace_id) <= 262144 AND length(CAST(NEW.source_workspace_id AS BLOB)) <= 262144))
    AND (NEW.source_last_edited_time IS NULL OR (typeof(NEW.source_last_edited_time) = 'text' AND instr(CAST(NEW.source_last_edited_time AS BLOB), x'00') = 0 AND length(NEW.source_last_edited_time) <= 262144 AND length(CAST(NEW.source_last_edited_time AS BLOB)) <= 262144))
    AND (typeof(NEW.rich_text) = 'text' AND instr(CAST(NEW.rich_text AS BLOB), x'00') = 0 AND length(NEW.rich_text) <= 262144 AND length(CAST(NEW.rich_text AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text) = 'text' AND instr(CAST(NEW.plain_text AS BLOB), x'00') = 0 AND length(NEW.plain_text) <= 262144 AND length(CAST(NEW.plain_text AS BLOB)) <= 262144)
    AND (typeof(NEW.attachments) = 'text' AND instr(CAST(NEW.attachments AS BLOB), x'00') = 0 AND length(NEW.attachments) <= 262144 AND length(CAST(NEW.attachments AS BLOB)) <= 262144)
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_comments
BEFORE UPDATE OF id, thread_id, created_by, display_name, created_time ON notes_comments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.thread_id IS NOT NEW.thread_id COLLATE BINARY OR OLD.created_by IS NOT NEW.created_by COLLATE BINARY OR OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_comments
BEFORE INSERT ON notes_comments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 306 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_comments WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_comments_insert
AFTER INSERT ON notes_comments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (306, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_comments_update
AFTER UPDATE ON notes_comments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 306, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_provider IS NOT NEW.source_provider COLLATE BINARY OR OLD.source_object_id IS NOT NEW.source_object_id COLLATE BINARY OR OLD.source_workspace_id IS NOT NEW.source_workspace_id COLLATE BINARY OR OLD.source_last_edited_time IS NOT NEW.source_last_edited_time COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.rich_text IS NOT NEW.rich_text COLLATE BINARY OR OLD.plain_text IS NOT NEW.plain_text COLLATE BINARY OR OLD.attachments IS NOT NEW.attachments COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_comments_delete
AFTER DELETE ON notes_comments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (306, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_comment_reactions_insert
BEFORE INSERT ON notes_comment_reactions
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.comment_id IS NULL OR (typeof(NEW.comment_id) = 'text' AND instr(CAST(NEW.comment_id AS BLOB), x'00') = 0 AND trim(NEW.comment_id) <> '' AND length(CAST(NEW.comment_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND length(NEW.created_by) <= 200 AND length(CAST(NEW.created_by AS BLOB)) <= 262144)
    AND (typeof(NEW.emoji) = 'text' AND instr(CAST(NEW.emoji AS BLOB), x'00') = 0 AND length(NEW.emoji) <= 16 AND length(CAST(NEW.emoji AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_comment_reactions_update
BEFORE UPDATE OF comment_id, created_by, emoji, created_time, display_name ON notes_comment_reactions
WHEN ((NEW.comment_id IS NULL OR (typeof(NEW.comment_id) = 'text' AND instr(CAST(NEW.comment_id AS BLOB), x'00') = 0 AND trim(NEW.comment_id) <> '' AND length(CAST(NEW.comment_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND length(NEW.created_by) <= 200 AND length(CAST(NEW.created_by AS BLOB)) <= 262144)
    AND (typeof(NEW.emoji) = 'text' AND instr(CAST(NEW.emoji AS BLOB), x'00') = 0 AND length(NEW.emoji) <= 16 AND length(CAST(NEW.emoji AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_comment_reactions
BEFORE UPDATE OF sync_key, comment_id, created_by, emoji ON notes_comment_reactions
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.comment_id IS NOT NEW.comment_id COLLATE BINARY OR OLD.created_by IS NOT NEW.created_by COLLATE BINARY OR OLD.emoji IS NOT NEW.emoji COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_notes_comment_reactions_insert
AFTER INSERT ON notes_comment_reactions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (309, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_comment_reactions_update
AFTER UPDATE ON notes_comment_reactions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 309, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_time IS NOT NEW.created_time COLLATE BINARY OR OLD.display_name IS NOT NEW.display_name COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 309, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_comment_reactions_delete
AFTER DELETE ON notes_comment_reactions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (309, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_notes_suggestions_insert
BEFORE INSERT ON notes_suggestions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (NEW.created_by IS NULL OR (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND trim(NEW.created_by) <> '' AND length(CAST(NEW.created_by AS BLOB)) <= 128))
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (typeof(NEW.range_start) = 'integer' AND NEW.range_start BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.range_end) = 'integer' AND NEW.range_end BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.original_text) = 'text' AND instr(CAST(NEW.original_text AS BLOB), x'00') = 0 AND length(NEW.original_text) <= 2000 AND length(CAST(NEW.original_text AS BLOB)) <= 262144)
    AND (typeof(NEW.proposed_text) = 'text' AND instr(CAST(NEW.proposed_text AS BLOB), x'00') = 0 AND length(NEW.proposed_text) <= 2000 AND length(CAST(NEW.proposed_text AS BLOB)) <= 262144)
    AND (typeof(NEW.prefix_text) = 'text' AND instr(CAST(NEW.prefix_text AS BLOB), x'00') = 0 AND length(NEW.prefix_text) <= 120 AND length(CAST(NEW.prefix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.suffix_text) = 'text' AND instr(CAST(NEW.suffix_text AS BLOB), x'00') = 0 AND length(NEW.suffix_text) <= 120 AND length(CAST(NEW.suffix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 200 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.accepted_at IS NULL OR (typeof(NEW.accepted_at) = 'text' AND instr(CAST(NEW.accepted_at AS BLOB), x'00') = 0 AND trim(NEW.accepted_at) <> '' AND length(CAST(NEW.accepted_at AS BLOB)) <= 64))
    AND (NEW.accepted_by IS NULL OR (typeof(NEW.accepted_by) = 'text' AND instr(CAST(NEW.accepted_by AS BLOB), x'00') = 0 AND trim(NEW.accepted_by) <> '' AND length(CAST(NEW.accepted_by AS BLOB)) <= 128))
    AND (NEW.rejected_at IS NULL OR (typeof(NEW.rejected_at) = 'text' AND instr(CAST(NEW.rejected_at AS BLOB), x'00') = 0 AND trim(NEW.rejected_at) <> '' AND length(CAST(NEW.rejected_at AS BLOB)) <= 64))
    AND (NEW.rejected_by IS NULL OR (typeof(NEW.rejected_by) = 'text' AND instr(CAST(NEW.rejected_by AS BLOB), x'00') = 0 AND trim(NEW.rejected_by) <> '' AND length(CAST(NEW.rejected_by AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_suggestions_update
BEFORE UPDATE OF block_id, created_by, display_name, range_start, range_end, original_text, proposed_text, prefix_text, suffix_text, created_time, status, accepted_at, accepted_by, rejected_at, rejected_by, last_edited_time ON notes_suggestions
WHEN ((NEW.block_id IS NULL OR (typeof(NEW.block_id) = 'text' AND instr(CAST(NEW.block_id AS BLOB), x'00') = 0 AND trim(NEW.block_id) <> '' AND length(CAST(NEW.block_id AS BLOB)) <= 128))
    AND (NEW.created_by IS NULL OR (typeof(NEW.created_by) = 'text' AND instr(CAST(NEW.created_by AS BLOB), x'00') = 0 AND trim(NEW.created_by) <> '' AND length(CAST(NEW.created_by AS BLOB)) <= 128))
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 262144 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (typeof(NEW.range_start) = 'integer' AND NEW.range_start BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.range_end) = 'integer' AND NEW.range_end BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.original_text) = 'text' AND instr(CAST(NEW.original_text AS BLOB), x'00') = 0 AND length(NEW.original_text) <= 2000 AND length(CAST(NEW.original_text AS BLOB)) <= 262144)
    AND (typeof(NEW.proposed_text) = 'text' AND instr(CAST(NEW.proposed_text AS BLOB), x'00') = 0 AND length(NEW.proposed_text) <= 2000 AND length(CAST(NEW.proposed_text AS BLOB)) <= 262144)
    AND (typeof(NEW.prefix_text) = 'text' AND instr(CAST(NEW.prefix_text AS BLOB), x'00') = 0 AND length(NEW.prefix_text) <= 120 AND length(CAST(NEW.prefix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.suffix_text) = 'text' AND instr(CAST(NEW.suffix_text AS BLOB), x'00') = 0 AND length(NEW.suffix_text) <= 120 AND length(CAST(NEW.suffix_text AS BLOB)) <= 262144)
    AND (typeof(NEW.created_time) = 'text' AND instr(CAST(NEW.created_time AS BLOB), x'00') = 0 AND trim(NEW.created_time) <> '' AND length(CAST(NEW.created_time AS BLOB)) <= 64)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 200 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.accepted_at IS NULL OR (typeof(NEW.accepted_at) = 'text' AND instr(CAST(NEW.accepted_at AS BLOB), x'00') = 0 AND trim(NEW.accepted_at) <> '' AND length(CAST(NEW.accepted_at AS BLOB)) <= 64))
    AND (NEW.accepted_by IS NULL OR (typeof(NEW.accepted_by) = 'text' AND instr(CAST(NEW.accepted_by AS BLOB), x'00') = 0 AND trim(NEW.accepted_by) <> '' AND length(CAST(NEW.accepted_by AS BLOB)) <= 128))
    AND (NEW.rejected_at IS NULL OR (typeof(NEW.rejected_at) = 'text' AND instr(CAST(NEW.rejected_at AS BLOB), x'00') = 0 AND trim(NEW.rejected_at) <> '' AND length(CAST(NEW.rejected_at AS BLOB)) <= 64))
    AND (NEW.rejected_by IS NULL OR (typeof(NEW.rejected_by) = 'text' AND instr(CAST(NEW.rejected_by AS BLOB), x'00') = 0 AND trim(NEW.rejected_by) <> '' AND length(CAST(NEW.rejected_by AS BLOB)) <= 128))
    AND (typeof(NEW.last_edited_time) = 'text' AND instr(CAST(NEW.last_edited_time AS BLOB), x'00') = 0 AND trim(NEW.last_edited_time) <> '' AND length(CAST(NEW.last_edited_time AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_suggestions
BEFORE UPDATE OF id, block_id, created_by, display_name, range_start, range_end, original_text, proposed_text, prefix_text, suffix_text, created_time ON notes_suggestions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.block_id IS NOT NEW.block_id COLLATE BINARY OR OLD.created_by IS NOT NEW.created_by COLLATE BINARY OR OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.range_start IS NOT NEW.range_start OR OLD.range_end IS NOT NEW.range_end OR OLD.original_text IS NOT NEW.original_text COLLATE BINARY OR OLD.proposed_text IS NOT NEW.proposed_text COLLATE BINARY OR OLD.prefix_text IS NOT NEW.prefix_text COLLATE BINARY OR OLD.suffix_text IS NOT NEW.suffix_text COLLATE BINARY OR OLD.created_time IS NOT NEW.created_time COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_suggestions
BEFORE INSERT ON notes_suggestions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 307 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_suggestions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_suggestions_insert
AFTER INSERT ON notes_suggestions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (307, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_suggestions_update
AFTER UPDATE ON notes_suggestions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 307, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.status IS NOT NEW.status COLLATE BINARY OR OLD.accepted_at IS NOT NEW.accepted_at COLLATE BINARY OR (OLD.accepted_by IS NOT NEW.accepted_by COLLATE BINARY
                AND NOT (NEW.accepted_by IS NULL AND NOT EXISTS (SELECT 1 FROM notes_local_users WHERE id = OLD.accepted_by))) OR OLD.rejected_at IS NOT NEW.rejected_at COLLATE BINARY OR (OLD.rejected_by IS NOT NEW.rejected_by COLLATE BINARY
                AND NOT (NEW.rejected_by IS NULL AND NOT EXISTS (SELECT 1 FROM notes_local_users WHERE id = OLD.rejected_by)))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.last_edited_time IS NOT NEW.last_edited_time COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_suggestions_delete
AFTER DELETE ON notes_suggestions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (307, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

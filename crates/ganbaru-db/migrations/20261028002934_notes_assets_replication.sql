-- no-transaction
-- Notes assets replication. `notes_assets` becomes a replicated table keyed by its asset id
-- `{class}:{sha256}`, and its files travel as sync asset classes.
--
-- The check that ties the id to the path and digest names the key, and the check that pairs
-- `storage_state` with `missing_at` names device-local columns; neither can be enforced on a
-- merged row, so both go. The sync adapter checks the path, digest, and kind of every replicated
-- row, and Notes code keeps the storage pair consistent. The path loses its unique constraint:
-- two rows never share a path because the path is the digest name of the row's own bytes, and a
-- merge cannot refuse a row for a path another row holds.
--
-- SQLite applies changed column constraints only by rebuilding the table. Foreign keys are
-- disabled for the rebuild and every copy keeps its values. Legacy rename keeps SQLite from
-- reparsing the tables that reference `notes_assets` while it is briefly absent. The rebuild
-- commits only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE notes_assets_next (
    id TEXT PRIMARY KEY,
    asset_path TEXT NOT NULL CHECK (
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
    id, asset_path, kind, source_type, original_name, content_type, byte_size, sha256,
    storage_state, missing_at, created_at, updated_at
FROM notes_assets;

DROP TABLE notes_assets;
ALTER TABLE notes_assets_next RENAME TO notes_assets;

CREATE INDEX idx_notes_assets_hash ON notes_assets(sha256, byte_size, content_type);
CREATE INDEX idx_notes_assets_path ON notes_assets(asset_path);
CREATE INDEX idx_notes_assets_state ON notes_assets(storage_state, updated_at DESC, id);

CREATE TEMP TABLE notes_assets_replication_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO notes_assets_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE notes_assets_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.
CREATE TRIGGER sync_values_notes_assets_insert
BEFORE INSERT ON notes_assets
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.asset_path) = 'text' AND instr(CAST(NEW.asset_path AS BLOB), x'00') = 0 AND length(NEW.asset_path) <= 200 AND length(CAST(NEW.asset_path AS BLOB)) <= 262144)
    AND (typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 200 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (typeof(NEW.content_type) = 'text' AND instr(CAST(NEW.content_type AS BLOB), x'00') = 0 AND length(NEW.content_type) <= 64 AND length(CAST(NEW.content_type AS BLOB)) <= 262144)
    AND (typeof(NEW.byte_size) = 'integer' AND NEW.byte_size BETWEEN 1 AND 52428800)
    AND (typeof(NEW.sha256) = 'text' AND instr(CAST(NEW.sha256 AS BLOB), x'00') = 0 AND length(NEW.sha256) <= 200 AND length(CAST(NEW.sha256 AS BLOB)) <= 262144)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 200 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (NEW.original_name IS NULL OR (typeof(NEW.original_name) = 'text' AND instr(CAST(NEW.original_name AS BLOB), x'00') = 0 AND length(NEW.original_name) <= 262144 AND length(CAST(NEW.original_name AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_notes_assets_update
BEFORE UPDATE OF asset_path, kind, content_type, byte_size, sha256, source_type, original_name, created_at ON notes_assets
WHEN ((typeof(NEW.asset_path) = 'text' AND instr(CAST(NEW.asset_path AS BLOB), x'00') = 0 AND length(NEW.asset_path) <= 200 AND length(CAST(NEW.asset_path AS BLOB)) <= 262144)
    AND (typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 200 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (typeof(NEW.content_type) = 'text' AND instr(CAST(NEW.content_type AS BLOB), x'00') = 0 AND length(NEW.content_type) <= 64 AND length(CAST(NEW.content_type AS BLOB)) <= 262144)
    AND (typeof(NEW.byte_size) = 'integer' AND NEW.byte_size BETWEEN 1 AND 52428800)
    AND (typeof(NEW.sha256) = 'text' AND instr(CAST(NEW.sha256 AS BLOB), x'00') = 0 AND length(NEW.sha256) <= 200 AND length(CAST(NEW.sha256 AS BLOB)) <= 262144)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 200 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (NEW.original_name IS NULL OR (typeof(NEW.original_name) = 'text' AND instr(CAST(NEW.original_name AS BLOB), x'00') = 0 AND length(NEW.original_name) <= 262144 AND length(CAST(NEW.original_name AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_notes_assets
BEFORE UPDATE OF id, asset_path, kind, content_type, byte_size, sha256 ON notes_assets
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.asset_path IS NOT NEW.asset_path COLLATE BINARY OR OLD.kind IS NOT NEW.kind COLLATE BINARY OR OLD.content_type IS NOT NEW.content_type COLLATE BINARY OR OLD.byte_size IS NOT NEW.byte_size OR OLD.sha256 IS NOT NEW.sha256 COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_notes_assets
BEFORE INSERT ON notes_assets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 308 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM notes_assets WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_notes_assets_insert
AFTER INSERT ON notes_assets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (308, NEW.id, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_notes_assets_update
AFTER UPDATE ON notes_assets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 308, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_type IS NOT NEW.source_type COLLATE BINARY OR OLD.original_name IS NOT NEW.original_name COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_notes_assets_delete
AFTER DELETE ON notes_assets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (308, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

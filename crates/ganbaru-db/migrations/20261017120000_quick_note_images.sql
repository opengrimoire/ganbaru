-- Quick notes reference images stored as content-addressed files under
-- assets/quick-notes/images/{sha256}.{extension}. The ordered list replicates as one owned
-- register group of the note; the file bytes travel separately.
CREATE TABLE quick_note_images (
    note_id TEXT NOT NULL REFERENCES quick_notes(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL CHECK (sort_order >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    content_type TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif')),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    PRIMARY KEY (note_id, sort_order)
);
CREATE INDEX idx_quick_note_images_sha256 ON quick_note_images(sha256);

-- Sync triggers whose rendering covers the new group, rendered by ganbaru-sync from the vault
-- manifest. The conformance test fails when these differ from the current rendering.
DROP TRIGGER sync_capture_quick_notes_insert;

CREATE TRIGGER sync_capture_quick_notes_insert
AFTER INSERT ON quick_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, NEW.id, 511, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_quick_note_images_insert
AFTER INSERT ON quick_note_images
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, NEW.note_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_note_images_update
AFTER UPDATE ON quick_note_images
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 2, parent.key, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.note_id AS key UNION SELECT OLD.note_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_quick_note_images_delete
AFTER DELETE ON quick_note_images
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (2, OLD.note_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

-- Every existing note publishes its empty image list: pending creations widen to the full group
-- mask, and published notes write the new group.
INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
SELECT 2, id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM quick_notes
WHERE true
ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;

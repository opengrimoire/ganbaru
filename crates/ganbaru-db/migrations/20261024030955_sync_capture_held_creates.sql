-- no-transaction
-- A publish hold may keep a captured insert unsealed as well as a captured delete, so a Calendar
-- deletion undone inside its lease publishes nothing for the archive it wrote. SQLite changes a
-- CHECK only by rebuilding the table. Capture triggers on domain tables name `sync_capture` in
-- their bodies, and legacy rename keeps those references by name while the rebuilt table is
-- briefly absent.

PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE sync_capture_next (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    mask INTEGER NOT NULL,
    forced_mask INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL CHECK (created IN (0, 1)),
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    replaced_by TEXT CHECK (replaced_by IS NULL OR length(CAST(replaced_by AS BLOB)) BETWEEN 1 AND 128),
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    -- A local delete or insert held for Undo until this Unix millisecond time. Sealing skips it
    -- until then, unless a remote change touches the row first.
    held_until_ms INTEGER CHECK (held_until_ms IS NULL OR held_until_ms >= 0),
    PRIMARY KEY (table_id, row_key)
) STRICT, WITHOUT ROWID;

INSERT INTO sync_capture_next
    (table_id, row_key, mask, forced_mask, created, deleted, replaced_by, captured_at_ms,
     held_until_ms)
SELECT table_id, row_key, mask, forced_mask, created, deleted, replaced_by, captured_at_ms,
       held_until_ms
FROM sync_capture;

DROP TABLE sync_capture;
ALTER TABLE sync_capture_next RENAME TO sync_capture;

COMMIT;

PRAGMA legacy_alter_table = OFF;

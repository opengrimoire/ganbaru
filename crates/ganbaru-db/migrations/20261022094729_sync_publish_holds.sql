-- A local delete held for Undo until this Unix millisecond time. Sealing skips it until then,
-- unless a remote change touches the row first.
ALTER TABLE sync_capture ADD COLUMN held_until_ms INTEGER
    CHECK (held_until_ms IS NULL OR (held_until_ms >= 0 AND deleted = 1));

-- Replicated tables whose current rows were captured once, so capture seeding runs once per
-- table.
CREATE TABLE sync_seeded_tables (
    table_id INTEGER PRIMARY KEY CHECK (table_id BETWEEN 1 AND 65535)
) STRICT;

-- Quick note tags and notes were seeded when their capture triggers were installed.
INSERT INTO sync_seeded_tables (table_id) VALUES (1), (2);

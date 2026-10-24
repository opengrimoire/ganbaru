-- Row creations and rows that wait for one.
--
-- `sync_row_creations` lists the applied operations that created each published row, so
-- excluding a forked branch can tell whether another operation still creates a row the branch
-- created. `creation_pending` marks a row whose merge state holds honest changes but no
-- creation: changes that saw a row only an excluded forked branch created. The row stays hidden
-- until an operation creates it, as the forked device's re-sealed branch does.

CREATE TABLE sync_row_creations (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    PRIMARY KEY (table_id, row_key, writer_id, seq),
    FOREIGN KEY (table_id, row_key) REFERENCES sync_rows(table_id, row_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_row_creations_writer ON sync_row_creations(writer_id, seq);

ALTER TABLE sync_rows ADD COLUMN creation_pending INTEGER NOT NULL DEFAULT 0
    CHECK (creation_pending IN (0, 1));

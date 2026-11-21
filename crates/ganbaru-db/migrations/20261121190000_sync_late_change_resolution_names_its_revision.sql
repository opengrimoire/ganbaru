-- no-transaction
-- A late change is decided once, by the authorization revision that restores or discards it, so
-- its resolution and resolving revision are set and cleared together. SQLite adds a CHECK only by
-- rebuilding the table. Every writer sets both at once; a row holding only one is copied back as
-- undecided, so the next decision resolves it again.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE sync_late_changes_next (
  removal_revision INTEGER NOT NULL CHECK (removal_revision >= 1),
  table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
  row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
  change TEXT NOT NULL CHECK (change IN ('values', 'tombstone', 'created')),
  mask INTEGER NOT NULL DEFAULT 0,
  groups BLOB,
  resolved_revision INTEGER CHECK (resolved_revision IS NULL OR resolved_revision > removal_revision),
  resolution TEXT
    CHECK (resolution IS NULL OR resolution IN ('restore', 'discard')),
  PRIMARY KEY (removal_revision, table_id, row_key),
  CHECK ((resolution IS NULL) = (resolved_revision IS NULL))
) STRICT, WITHOUT ROWID;

INSERT INTO sync_late_changes_next
    (removal_revision, table_id, row_key, change, mask, groups, resolved_revision, resolution)
SELECT removal_revision, table_id, row_key, change, mask, groups,
    CASE WHEN resolution IS NULL THEN NULL ELSE resolved_revision END,
    CASE WHEN resolved_revision IS NULL THEN NULL ELSE resolution END
FROM sync_late_changes;

DROP TABLE sync_late_changes;
ALTER TABLE sync_late_changes_next RENAME TO sync_late_changes;
CREATE INDEX idx_sync_late_changes_row ON sync_late_changes(table_id, row_key);

COMMIT;

PRAGMA foreign_keys = ON;

-- The deleted row a cascade tombstone follows. A cause tombstone carries no redirect, and its
-- retained versions are judged against the cause's tombstone contexts.
ALTER TABLE sync_tombstones ADD COLUMN cause_table INTEGER
    CHECK (cause_table IS NULL OR (cause_table BETWEEN 1 AND 65535 AND replaced_by IS NULL));
ALTER TABLE sync_tombstones ADD COLUMN cause_row TEXT
    CHECK ((cause_row IS NULL) = (cause_table IS NULL)
        AND (cause_row IS NULL OR length(CAST(cause_row AS BLOB)) BETWEEN 1 AND 128));
CREATE INDEX idx_sync_tombstones_cause ON sync_tombstones(cause_table, cause_row)
    WHERE cause_table IS NOT NULL;

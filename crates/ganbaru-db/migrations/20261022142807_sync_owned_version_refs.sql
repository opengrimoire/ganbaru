-- An owned table blob field references many rows, so a version keeps one row per field and
-- referenced key.
CREATE TABLE sync_version_refs_next (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    group_id INTEGER NOT NULL,
    writer_id BLOB NOT NULL,
    field INTEGER NOT NULL CHECK (field BETWEEN 0 AND 255),
    ref_key TEXT NOT NULL CHECK (length(CAST(ref_key AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (table_id, row_key, group_id, writer_id, field, ref_key),
    FOREIGN KEY (table_id, row_key, group_id, writer_id)
        REFERENCES sync_register_versions(table_id, row_key, group_id, writer_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

INSERT INTO sync_version_refs_next (table_id, row_key, group_id, writer_id, field, ref_key)
SELECT table_id, row_key, group_id, writer_id, field, ref_key
FROM sync_version_refs;

DROP TABLE sync_version_refs;
ALTER TABLE sync_version_refs_next RENAME TO sync_version_refs;
CREATE INDEX idx_sync_version_refs_key ON sync_version_refs(ref_key);

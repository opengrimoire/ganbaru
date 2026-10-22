-- Reference targets of retained sync versions, one row per reference column of the version's
-- group, so redirects and deletions find their referrers even in groups with several
-- references. `field` is the column's index in the group value.
CREATE TABLE sync_version_refs (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    group_id INTEGER NOT NULL,
    writer_id BLOB NOT NULL,
    field INTEGER NOT NULL CHECK (field BETWEEN 0 AND 255),
    ref_key TEXT NOT NULL CHECK (length(CAST(ref_key AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (table_id, row_key, group_id, writer_id, field),
    FOREIGN KEY (table_id, row_key, group_id, writer_id)
        REFERENCES sync_register_versions(table_id, row_key, group_id, writer_id) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_version_refs_key ON sync_version_refs(ref_key);

INSERT INTO sync_version_refs (table_id, row_key, group_id, writer_id, field, ref_key)
SELECT table_id, row_key, group_id, writer_id, 0, ref_key
FROM sync_register_versions
WHERE ref_key IS NOT NULL;

DROP INDEX idx_sync_register_versions_ref;
ALTER TABLE sync_register_versions DROP COLUMN ref_key;

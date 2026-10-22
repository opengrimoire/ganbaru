-- An elided version keeps a null value; readers take it from the domain row or the operation
-- body and check it against `value_hash`. The child table moves with the parent so its foreign
-- key never sees the parent dropped.
CREATE TABLE sync_register_versions_next (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    group_id INTEGER NOT NULL CHECK (group_id BETWEEN 0 AND 63),
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    clock INTEGER NOT NULL CHECK (clock >= 0),
    value_hash BLOB NOT NULL CHECK (length(value_hash) = 32),
    value BLOB,
    PRIMARY KEY (table_id, row_key, group_id, writer_id),
    FOREIGN KEY (table_id, row_key) REFERENCES sync_rows(table_id, row_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

INSERT INTO sync_register_versions_next
    (table_id, row_key, group_id, writer_id, seq, clock, value_hash, value)
SELECT table_id, row_key, group_id, writer_id, seq, clock, value_hash, value
FROM sync_register_versions;

CREATE TABLE sync_version_refs_next (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    group_id INTEGER NOT NULL,
    writer_id BLOB NOT NULL,
    field INTEGER NOT NULL CHECK (field BETWEEN 0 AND 255),
    ref_key TEXT NOT NULL CHECK (length(CAST(ref_key AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (table_id, row_key, group_id, writer_id, field, ref_key),
    FOREIGN KEY (table_id, row_key, group_id, writer_id)
        REFERENCES sync_register_versions_next(table_id, row_key, group_id, writer_id)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;

INSERT INTO sync_version_refs_next (table_id, row_key, group_id, writer_id, field, ref_key)
SELECT table_id, row_key, group_id, writer_id, field, ref_key
FROM sync_version_refs;

DROP TABLE sync_version_refs;
DROP TABLE sync_register_versions;
ALTER TABLE sync_register_versions_next RENAME TO sync_register_versions;
ALTER TABLE sync_version_refs_next RENAME TO sync_version_refs;
CREATE INDEX idx_sync_version_refs_key ON sync_version_refs(ref_key);

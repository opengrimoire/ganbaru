-- Rows whose domain state a stored operation may have made stale.
--
-- Storing an operation a removal excludes records its late changes outside any engine write, and
-- a late change can hide a row already materialized. The next engine write materializes every
-- listed row again and clears the list.

CREATE TABLE sync_stale_rows (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (table_id, row_key)
) STRICT, WITHOUT ROWID;

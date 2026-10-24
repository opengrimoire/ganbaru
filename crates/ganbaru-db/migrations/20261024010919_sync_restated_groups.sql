-- Groups the anchor seals again with the value they already hold, by the hash of that value.
--
-- A restatement gets a new clock without being a newer edit, so restoring a late change skips a
-- version holding the restated value. Rows leave the list once no late change of theirs waits
-- for a decision. Device-local engine state that never replicates.

CREATE TABLE sync_restated_groups (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    group_id INTEGER NOT NULL CHECK (group_id BETWEEN 0 AND 63),
    value_hash BLOB NOT NULL CHECK (length(value_hash) = 32),
    PRIMARY KEY (table_id, row_key, group_id)
) STRICT, WITHOUT ROWID;

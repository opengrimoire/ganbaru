-- Compaction empties the bodies of a writer's operations every device has stored and applied,
-- in sequence order; `compacted_seq` is the highest sequence whose body may be empty.
ALTER TABLE sync_writers
    ADD COLUMN compacted_seq INTEGER NOT NULL DEFAULT 0 CHECK (compacted_seq >= 0);

-- The stability vector a client last received from the coordinator: per writer, the sequence
-- every counted device holds. Device-local engine state that never replicates.
CREATE TABLE sync_stable (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    PRIMARY KEY (space_id, writer_id)
) STRICT, WITHOUT ROWID;

-- Per writer, the highest sequence this replica applied while it was the anchor.
--
-- The anchor keeps none of the versions an operation displaces, so when such an operation is
-- excluded later, even after the anchor moved, this replica seals the groups it wrote again.
-- Device-local engine state that never replicates.

CREATE TABLE sync_released_marks (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    PRIMARY KEY (space_id, writer_id)
) STRICT, WITHOUT ROWID;

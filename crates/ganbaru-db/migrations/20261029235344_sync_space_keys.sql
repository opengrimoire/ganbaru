-- Per key epoch of the selected authorization chain, the key material the local device reads.
--
-- Rebuilt from the applied authorization entries whenever the selection changes: the chain link
-- that wraps the previous epoch's key under this one, and the envelope of this epoch's key to the
-- encryption key the local device published. Plaintext space keys never reach the database.
-- Device-local engine state that never replicates; snapshot copies empty it.

CREATE TABLE sync_space_keys (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    epoch INTEGER NOT NULL CHECK (epoch >= 1 AND epoch <= 4294967295),
    wrapped_previous BLOB CHECK (wrapped_previous IS NULL OR length(wrapped_previous) = 72),
    own_envelope BLOB CHECK (own_envelope IS NULL OR length(own_envelope) = 96),
    sequenced_revision INTEGER NOT NULL CHECK (sequenced_revision >= 1),
    CHECK ((epoch = 1) = (wrapped_previous IS NULL)),
    PRIMARY KEY (space_id, epoch)
) STRICT, WITHOUT ROWID;

-- Whether the selected authorization chain holds an enrolled device for data: a stored branch
-- of the head's frozen epoch instance removes it, so its operations are stored but neither
-- applied nor served past authorization records until a handover resolves the instance.
-- Derived on every selection like the rest of the table.

ALTER TABLE sync_devices ADD COLUMN held INTEGER NOT NULL DEFAULT 0 CHECK (held IN (0, 1));

-- The authorization record store: every authorization record envelope, writer certificate
-- (the genesis envelope that carries it), and fork evidence envelope that passes its intrinsic
-- checks, keyed by envelope hash. Rows are never deleted by validity, selection, load, or
-- arrival order, so dropping a writer's operation slots never loses the records a chain
-- descends from. `verified` is 0 while no certificate of the writer is stored yet. `walked` is
-- 0 until a selection read the row. `stored_at_ms` is local metadata no rule reads.
CREATE TABLE sync_auth_envelopes (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('record', 'certificate', 'evidence')),
    envelope_hash BLOB NOT NULL CHECK (length(envelope_hash) = 32),
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    previous_hash BLOB NOT NULL CHECK (length(previous_hash) = 32),
    stamp INTEGER NOT NULL CHECK (stamp >= 0),
    entry_hash BLOB CHECK (entry_hash IS NULL OR length(entry_hash) = 32),
    previous_entry_hash BLOB CHECK (previous_entry_hash IS NULL OR length(previous_entry_hash) = 32),
    device_id TEXT CHECK (device_id IS NULL OR length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
    device_key BLOB CHECK (device_key IS NULL OR length(device_key) = 32),
    envelope BLOB NOT NULL,
    verified INTEGER NOT NULL CHECK (verified IN (0, 1)),
    walked INTEGER NOT NULL DEFAULT 0 CHECK (walked IN (0, 1)),
    stored_order INTEGER NOT NULL CHECK (stored_order >= 1),
    stored_at_ms INTEGER NOT NULL CHECK (stored_at_ms >= 0),
    PRIMARY KEY (space_id, role, envelope_hash),
    CHECK ((role = 'record') = (entry_hash IS NOT NULL)),
    CHECK ((role = 'record') = (previous_entry_hash IS NOT NULL)),
    CHECK ((role = 'certificate') = (device_id IS NOT NULL AND device_key IS NOT NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_auth_envelopes_writer ON sync_auth_envelopes(space_id, writer_id, seq);
CREATE INDEX idx_sync_auth_envelopes_entry ON sync_auth_envelopes(space_id, entry_hash)
    WHERE entry_hash IS NOT NULL;
CREATE INDEX idx_sync_auth_envelopes_hash ON sync_auth_envelopes(space_id, envelope_hash);
CREATE UNIQUE INDEX idx_sync_auth_envelopes_order ON sync_auth_envelopes(space_id, stored_order);
CREATE INDEX idx_sync_auth_envelopes_unwalked ON sync_auth_envelopes(space_id)
    WHERE walked = 0;

-- References the record store lacks: an entry hash a record's chain names, an envelope hash
-- an evidence envelope continues from, or a writer without a stored certificate. Derived from
-- the record store.
CREATE TABLE sync_auth_wanted (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('entry', 'envelope', 'writer')),
    key BLOB NOT NULL CHECK (length(key) BETWEEN 16 AND 32),
    PRIMARY KEY (space_id, kind, key)
) STRICT, WITHOUT ROWID;

-- The selected chain, one row per revision past the charter, with the stored record envelope
-- of lowest hash that carries each entry. Derived on every selection.
CREATE TABLE sync_auth_selected (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    epoch INTEGER NOT NULL CHECK (epoch BETWEEN 0 AND 4294967295),
    entry_hash BLOB NOT NULL CHECK (length(entry_hash) = 32),
    proposal_hash BLOB NOT NULL CHECK (length(proposal_hash) = 32),
    envelope_hash BLOB NOT NULL CHECK (length(envelope_hash) = 32),
    PRIMARY KEY (space_id, revision)
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_auth_selected_proposal ON sync_auth_selected(space_id, proposal_hash);

DROP TABLE sync_auth_entries;

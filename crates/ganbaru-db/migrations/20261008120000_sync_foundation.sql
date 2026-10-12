-- Sync engine state: the vault's replication space, local capture, writers, stored operations,
-- and per-row merge state. The manifest in the ganbaru-sync crate classifies every other table.

CREATE TABLE sync_spaces (
    space_id BLOB PRIMARY KEY NOT NULL CHECK (typeof(space_id) = 'blob' AND length(space_id) = 16),
    vault_id TEXT NOT NULL UNIQUE CHECK (length(vault_id) BETWEEN 1 AND 160),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;

-- One row. The engine sets `applying` inside its own transactions so capture triggers skip the
-- writes it materializes; vault pools hold one connection, so nothing interleaves.
CREATE TABLE sync_apply_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    applying INTEGER NOT NULL DEFAULT 0 CHECK (applying IN (0, 1))
) STRICT;
INSERT INTO sync_apply_state (singleton, applying) VALUES (1, 0);

-- Local changes not yet sealed, one row per domain row with the changed group mask.
CREATE TABLE sync_capture (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    mask INTEGER NOT NULL,
    forced_mask INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL CHECK (created IN (0, 1)),
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    replaced_by TEXT CHECK (replaced_by IS NULL OR length(CAST(replaced_by AS BLOB)) BETWEEN 1 AND 128),
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    PRIMARY KEY (table_id, row_key)
) STRICT, WITHOUT ROWID;

CREATE TABLE sync_writers (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 32),
    device_id TEXT NOT NULL CHECK (length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
    certificate BLOB NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'retired', 'revoked', 'forked')),
    predecessor BLOB CHECK (predecessor IS NULL OR length(predecessor) = 16),
    cutoff_seq INTEGER CHECK (cutoff_seq IS NULL OR cutoff_seq >= 1),
    stored_seq INTEGER NOT NULL CHECK (stored_seq >= 1),
    applied_seq INTEGER NOT NULL CHECK (applied_seq >= 0 AND applied_seq <= stored_seq),
    head_hash BLOB NOT NULL CHECK (length(head_hash) = 32),
    head_clock INTEGER NOT NULL CHECK (head_clock >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (space_id, writer_id)
) STRICT;

-- Every stored operation, kept whole so peers can be served the exact signed bytes. The rowid
-- orders operations by arrival for incremental pulls.
CREATE TABLE sync_ops (
    space_id BLOB NOT NULL,
    writer_id BLOB NOT NULL,
    seq INTEGER NOT NULL CHECK (seq >= 1),
    kind INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 255),
    hash BLOB NOT NULL CHECK (length(hash) = 32),
    clock INTEGER NOT NULL CHECK (clock >= 0),
    context BLOB NOT NULL,
    envelope BLOB NOT NULL CHECK (length(envelope) <= 524288),
    state TEXT NOT NULL CHECK (state IN ('waiting', 'applied', 'held')),
    hold_reason TEXT CHECK (hold_reason IN ('newer_format', 'newer_manifest', 'invalid')),
    stored_at_ms INTEGER NOT NULL CHECK (stored_at_ms >= 0),
    PRIMARY KEY (space_id, writer_id, seq),
    FOREIGN KEY (space_id, writer_id) REFERENCES sync_writers(space_id, writer_id) ON DELETE CASCADE,
    CHECK ((state = 'held') = (hold_reason IS NOT NULL))
) STRICT;
CREATE INDEX idx_sync_ops_pending ON sync_ops(state, space_id, writer_id, seq)
    WHERE state <> 'applied';

-- Merge state for every published row of a replicated table, live or tombstoned.
CREATE TABLE sync_rows (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    state TEXT NOT NULL CHECK (state IN ('live', 'tombstoned')),
    recovery INTEGER NOT NULL DEFAULT 0 CHECK (recovery IN (0, 1) AND (recovery = 0 OR state = 'tombstoned')),
    conflict_mask INTEGER NOT NULL DEFAULT 0 CHECK (conflict_mask = 0 OR state = 'live'),
    PRIMARY KEY (table_id, row_key)
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_rows_recovery ON sync_rows(table_id, row_key) WHERE recovery = 1;
CREATE INDEX idx_sync_rows_conflicts ON sync_rows(table_id, row_key) WHERE conflict_mask <> 0;

-- Concurrent versions of one group of one row, at most one per writer. The winner is chosen
-- by the group's merge kind. Versions of tombstoned rows are the retained state for recovery.
-- `ref_key` indexes reference values so redirects and deletions find their referrers.
CREATE TABLE sync_register_versions (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    group_id INTEGER NOT NULL CHECK (group_id BETWEEN 0 AND 63),
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    clock INTEGER NOT NULL CHECK (clock >= 0),
    value_hash BLOB NOT NULL CHECK (length(value_hash) = 32),
    value BLOB NOT NULL,
    ref_key TEXT CHECK (ref_key IS NULL OR length(CAST(ref_key AS BLOB)) BETWEEN 1 AND 128),
    PRIMARY KEY (table_id, row_key, group_id, writer_id),
    FOREIGN KEY (table_id, row_key) REFERENCES sync_rows(table_id, row_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_register_versions_ref ON sync_register_versions(ref_key)
    WHERE ref_key IS NOT NULL;

-- Tombstones of published rows; the causal context is the sealing operation's. `replaced_by`
-- redirects references to a lower key that absorbed the row.
CREATE TABLE sync_tombstones (
    table_id INTEGER NOT NULL,
    row_key TEXT NOT NULL,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    replaced_by TEXT CHECK (replaced_by IS NULL OR replaced_by < row_key),
    PRIMARY KEY (table_id, row_key, writer_id, seq),
    FOREIGN KEY (table_id, row_key) REFERENCES sync_rows(table_id, row_key) ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_tombstones_redirect ON sync_tombstones(table_id, replaced_by)
    WHERE replaced_by IS NOT NULL;

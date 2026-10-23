-- no-transaction
-- Sync authorization log: the space charter, the kind 4 entries every replica stores, the device
-- state derived from the selected chain, and the tables removal exclusion, late changes, and fork
-- evidence keep. Writers of a removed device the anchor never stored keep nothing, so a writer
-- cutoff may be zero; SQLite changes that check only by rebuilding `sync_writers`. Foreign keys are
-- disabled for the rebuild so `sync_ops` keeps its rows, and legacy rename keeps its reference to
-- `sync_writers` by name. The rebuild commits only when the foreign key check reports no
-- violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE sync_writers_next (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 32),
    device_id TEXT NOT NULL CHECK (length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
    certificate BLOB NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('active', 'retired', 'revoked', 'forked')),
    predecessor BLOB CHECK (predecessor IS NULL OR length(predecessor) = 16),
    cutoff_seq INTEGER CHECK (cutoff_seq IS NULL OR cutoff_seq >= 0),
    stored_seq INTEGER NOT NULL CHECK (stored_seq >= 1),
    applied_seq INTEGER NOT NULL CHECK (applied_seq >= 0 AND applied_seq <= stored_seq),
    head_hash BLOB NOT NULL CHECK (length(head_hash) = 32),
    head_clock INTEGER NOT NULL CHECK (head_clock >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    compacted_seq INTEGER NOT NULL DEFAULT 0 CHECK (compacted_seq >= 0),
    PRIMARY KEY (space_id, writer_id)
) STRICT;

INSERT INTO sync_writers_next
    (space_id, writer_id, public_key, device_id, certificate, state, predecessor, cutoff_seq,
     stored_seq, applied_seq, head_hash, head_clock, created_at_ms, compacted_seq)
SELECT space_id, writer_id, public_key, device_id, certificate, state, predecessor, cutoff_seq,
    stored_seq, applied_seq, head_hash, head_clock, created_at_ms, compacted_seq
FROM sync_writers;

DROP TABLE sync_writers;
ALTER TABLE sync_writers_next RENAME TO sync_writers;

-- The signed space charter, revision 0 of the authorization log. A space without one cannot
-- store remote writers.
ALTER TABLE sync_spaces ADD COLUMN charter BLOB CHECK (charter IS NULL OR length(charter) >= 1);

-- The authorization revision an operation was sealed against, copied from its signed header so
-- readiness needs no decoding, and the removal revision that excludes it.
ALTER TABLE sync_ops ADD COLUMN authorization_revision INTEGER NOT NULL DEFAULT 0
    CHECK (authorization_revision >= 0);
ALTER TABLE sync_ops ADD COLUMN excluded_revision INTEGER
    CHECK (excluded_revision IS NULL OR excluded_revision >= 1);

-- One row per applied kind 4 operation. `selected` marks the chain selection chose; every
-- other entry stays stored and forwarded.
CREATE TABLE sync_auth_entries (
    space_id BLOB NOT NULL,
    writer_id BLOB NOT NULL,
    seq INTEGER NOT NULL CHECK (seq >= 1),
    epoch INTEGER NOT NULL CHECK (epoch BETWEEN 0 AND 4294967295),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    previous_hash BLOB NOT NULL CHECK (length(previous_hash) = 32),
    entry_hash BLOB NOT NULL CHECK (length(entry_hash) = 32),
    entry_kind INTEGER NOT NULL CHECK (entry_kind BETWEEN 0 AND 255),
    proposal_hash BLOB NOT NULL CHECK (length(proposal_hash) = 32),
    selected INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1)),
    PRIMARY KEY (space_id, writer_id, seq),
    FOREIGN KEY (space_id, writer_id, seq) REFERENCES sync_ops(space_id, writer_id, seq)
        ON DELETE CASCADE
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_sync_auth_entries_previous ON sync_auth_entries(space_id, previous_hash);
CREATE UNIQUE INDEX idx_sync_auth_entries_selected ON sync_auth_entries(space_id, revision)
    WHERE selected = 1;

-- Enrolled devices as the selected chain leaves them. The charter's anchor has no label.
CREATE TABLE sync_devices (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    device_id TEXT NOT NULL CHECK (length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
    device_key BLOB NOT NULL CHECK (length(device_key) = 32),
    tls_fingerprint BLOB NOT NULL CHECK (length(tls_fingerprint) = 32),
    label TEXT CHECK (label IS NULL OR length(CAST(label AS BLOB)) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('desktop', 'mobile')),
    role TEXT NOT NULL CHECK (role IN ('member', 'admin')),
    admin_key BLOB CHECK (admin_key IS NULL OR length(admin_key) = 32),
    enrolled_revision INTEGER NOT NULL CHECK (enrolled_revision >= 0),
    removed_revision INTEGER CHECK (removed_revision IS NULL OR removed_revision > enrolled_revision),
    PRIMARY KEY (space_id, device_id),
    CHECK ((role = 'admin') = (admin_key IS NOT NULL)),
    CHECK (role = 'member' OR kind = 'desktop')
) STRICT;

-- Versions an operation past the anchor mark superseded, kept until the anchor confirms the
-- operation, so excluding it can restore them.
CREATE TABLE sync_displaced_versions (
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    group_id INTEGER NOT NULL CHECK (group_id BETWEEN 0 AND 63),
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    clock INTEGER NOT NULL CHECK (clock >= 0),
    value_hash BLOB NOT NULL CHECK (length(value_hash) = 32),
    value BLOB NOT NULL,
    displaced_by_writer BLOB NOT NULL CHECK (length(displaced_by_writer) = 16),
    displaced_by_seq INTEGER NOT NULL CHECK (displaced_by_seq >= 1),
    PRIMARY KEY (table_id, row_key, group_id, writer_id, displaced_by_writer, displaced_by_seq)
) STRICT, WITHOUT ROWID;

-- Rows a removed device changed past its cutoff, per removal, until an admin restores or
-- discards them.
CREATE TABLE sync_late_changes (
    removal_revision INTEGER NOT NULL CHECK (removal_revision >= 1),
    table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
    row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
    change TEXT NOT NULL CHECK (change IN ('values', 'tombstone', 'created')),
    mask INTEGER NOT NULL DEFAULT 0,
    groups BLOB,
    resolved_revision INTEGER CHECK (resolved_revision IS NULL OR resolved_revision > removal_revision),
    PRIMARY KEY (removal_revision, table_id, row_key)
) STRICT, WITHOUT ROWID;

-- Two signed operations of one writer at one sequence with different hashes.
CREATE TABLE sync_fork_evidence (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    first_envelope BLOB NOT NULL,
    second_envelope BLOB NOT NULL,
    received_at_ms INTEGER NOT NULL CHECK (received_at_ms >= 0),
    PRIMARY KEY (space_id, writer_id)
) STRICT;

-- Per writer, the highest sequence the anchor confirmed storing. Device-local engine state that
-- never replicates.
CREATE TABLE sync_anchor_marks (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    PRIMARY KEY (space_id, writer_id)
) STRICT, WITHOUT ROWID;

CREATE TEMP TABLE sync_authorization_log_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO sync_authorization_log_guard (violations)
SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE sync_authorization_log_guard;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

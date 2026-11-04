-- Relay server storage. Records and asset chunks are sealed; the relay keeps only their clear
-- slot headers, sizes, and times.

CREATE TABLE relay_spaces (
    space_id BLOB PRIMARY KEY NOT NULL CHECK (length(space_id) = 16),
    current_epoch INTEGER NOT NULL CHECK (current_epoch >= 1),
    -- Concatenated 32-byte admin public keys.
    admin_keys BLOB NOT NULL CHECK (length(admin_keys) > 0 AND length(admin_keys) % 32 = 0),
    admin_counter INTEGER NOT NULL CHECK (admin_counter >= 0),
    stored_bytes INTEGER NOT NULL DEFAULT 0 CHECK (stored_bytes >= 0),
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE relay_epochs (
    space_id BLOB NOT NULL REFERENCES relay_spaces (space_id) ON DELETE CASCADE,
    epoch INTEGER NOT NULL CHECK (epoch >= 1),
    access_key BLOB NOT NULL CHECK (length(access_key) = 32),
    -- Key epoch authorization entry; absent for the epoch a space was claimed with.
    entry BLOB,
    registered_at_ms INTEGER NOT NULL,
    PRIMARY KEY (space_id, epoch)
) STRICT;

CREATE TABLE relay_records (
    arrival INTEGER PRIMARY KEY AUTOINCREMENT,
    space_id BLOB NOT NULL REFERENCES relay_spaces (space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    seq INTEGER NOT NULL CHECK (seq >= 1),
    epoch INTEGER NOT NULL CHECK (epoch >= 1),
    record BLOB NOT NULL,
    stored_at_ms INTEGER NOT NULL,
    UNIQUE (space_id, writer_id, seq)
) STRICT;

CREATE INDEX relay_records_space_arrival ON relay_records (space_id, arrival);

-- Each writer's stored records are exactly the sequences floor up to next_seq - 1.
CREATE TABLE relay_floors (
    space_id BLOB NOT NULL REFERENCES relay_spaces (space_id) ON DELETE CASCADE,
    writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
    floor INTEGER NOT NULL CHECK (floor >= 1),
    next_seq INTEGER NOT NULL CHECK (next_seq >= floor),
    PRIMARY KEY (space_id, writer_id)
) STRICT;

CREATE TABLE relay_assets (
    space_id BLOB NOT NULL REFERENCES relay_spaces (space_id) ON DELETE CASCADE,
    asset_id BLOB NOT NULL CHECK (length(asset_id) = 32),
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    chunk_count INTEGER NOT NULL CHECK (chunk_count > chunk_index),
    epoch INTEGER NOT NULL CHECK (epoch >= 1),
    chunk BLOB NOT NULL,
    stored_at_ms INTEGER NOT NULL,
    last_read_at_ms INTEGER NOT NULL,
    PRIMARY KEY (space_id, asset_id, chunk_index)
) STRICT;

CREATE INDEX relay_assets_space_read ON relay_assets (space_id, last_read_at_ms);

-- One-time setup tokens, kept only as SHA-256 digests.
CREATE TABLE relay_setup_tokens (
    token_hash BLOB PRIMARY KEY NOT NULL CHECK (length(token_hash) = 32),
    expires_at_ms INTEGER NOT NULL
) STRICT;

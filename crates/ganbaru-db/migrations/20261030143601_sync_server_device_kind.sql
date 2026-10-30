-- An always-on anchor server enrolls as its own device kind and, like a desktop, may be an admin.
-- The table is derived from the authorization chain; the rebuild keeps its rows.

CREATE TABLE sync_devices_with_servers (
    space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
    device_id TEXT NOT NULL CHECK (length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
    device_key BLOB NOT NULL CHECK (length(device_key) = 32),
    tls_fingerprint BLOB NOT NULL CHECK (length(tls_fingerprint) = 32),
    label TEXT CHECK (label IS NULL OR length(CAST(label AS BLOB)) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('desktop', 'mobile', 'server')),
    role TEXT NOT NULL CHECK (role IN ('member', 'admin')),
    admin_key BLOB CHECK (admin_key IS NULL OR length(admin_key) = 32),
    enrolled_revision INTEGER NOT NULL CHECK (enrolled_revision >= 0),
    removed_revision INTEGER CHECK (removed_revision IS NULL OR removed_revision > enrolled_revision),
    held INTEGER NOT NULL DEFAULT 0 CHECK (held IN (0, 1)),
    PRIMARY KEY (space_id, device_id),
    CHECK ((role = 'admin') = (admin_key IS NOT NULL)),
    CHECK (role = 'member' OR kind IN ('desktop', 'server'))
) STRICT;

INSERT INTO sync_devices_with_servers (
    space_id, device_id, device_key, tls_fingerprint, label, kind, role, admin_key,
    enrolled_revision, removed_revision, held
)
SELECT
    space_id, device_id, device_key, tls_fingerprint, label, kind, role, admin_key,
    enrolled_revision, removed_revision, held
FROM sync_devices;

DROP TABLE sync_devices;
ALTER TABLE sync_devices_with_servers RENAME TO sync_devices;

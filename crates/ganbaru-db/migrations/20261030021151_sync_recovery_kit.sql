-- The public keys of the vault's recovery kit: the recovery admin key a charter names and the
-- recovery encryption key the anchor publishes in the space's first recovery key entry.
--
-- A kit created before the vault links has no charter to name it yet, so founding the space at
-- first link reads both keys from here. Regenerating the kit replaces the row. The secret seed
-- never reaches the database. Reset sync state keeps the row, because the kit the user saved
-- still belongs to the vault.

CREATE TABLE sync_recovery_kit (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    recovery_admin_key BLOB NOT NULL CHECK (length(recovery_admin_key) = 32),
    recovery_encryption_key BLOB NOT NULL CHECK (length(recovery_encryption_key) = 32),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
) STRICT;

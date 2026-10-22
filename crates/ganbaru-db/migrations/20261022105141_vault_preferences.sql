-- Portable preferences, one row per registered preference path or per child of a per-child
-- path. Values are canonical JSON text. Resetting a preference to its default deletes its row.
-- Device preferences live outside the vault, and config.json keeps only distraction rules and
-- Chat settings.
CREATE TABLE vault_preferences (
    key TEXT PRIMARY KEY NOT NULL CHECK (length(CAST(key AS BLOB)) BETWEEN 1 AND 128),
    value TEXT NOT NULL CHECK (json_valid(value) AND length(CAST(value AS BLOB)) <= 65536),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

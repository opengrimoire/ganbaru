-- Distraction rules move into SQLite and replicate through the vault manifest. The settings row
-- is a seeded singleton keyed 'vault'. A missing built-in category row means the category's
-- default. Host and app rules are keyed by their content, so a removed rule can be added again
-- under the same key; a custom category host also references its category and goes with it.
-- Checks spanning columns and JSON shapes are enforced by the rule writers and the sync
-- adapter, since replicated values arrive one group at a time.
-- Usage limit entries belong to their limit as an ordered list keyed by ordinal. Rules written
-- to config.json are not carried over: development vaults start from these defaults.

CREATE TABLE distractions_settings (
    id TEXT PRIMARY KEY CHECK (id = 'vault'),
    mode TEXT NOT NULL DEFAULT 'blacklist' CHECK (mode IN ('blacklist', 'whitelist')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    block_during_focus INTEGER NOT NULL DEFAULT 1 CHECK (block_during_focus IN (0, 1)),
    block_during_short_breaks INTEGER NOT NULL DEFAULT 1 CHECK (block_during_short_breaks IN (0, 1)),
    block_during_long_breaks INTEGER NOT NULL DEFAULT 1 CHECK (block_during_long_breaks IN (0, 1)),
    pause_during_focus_pause INTEGER NOT NULL DEFAULT 1 CHECK (pause_during_focus_pause IN (0, 1)),
    desktop_enabled INTEGER NOT NULL DEFAULT 1 CHECK (desktop_enabled IN (0, 1)),
    desktop_block_during_focus INTEGER NOT NULL DEFAULT 1 CHECK (desktop_block_during_focus IN (0, 1)),
    desktop_block_during_short_breaks INTEGER NOT NULL DEFAULT 1 CHECK (desktop_block_during_short_breaks IN (0, 1)),
    desktop_block_during_long_breaks INTEGER NOT NULL DEFAULT 1 CHECK (desktop_block_during_long_breaks IN (0, 1)),
    desktop_pause_during_focus_pause INTEGER NOT NULL DEFAULT 1 CHECK (desktop_pause_during_focus_pause IN (0, 1)),
    mobile_enabled INTEGER NOT NULL DEFAULT 1 CHECK (mobile_enabled IN (0, 1)),
    mobile_block_during_focus INTEGER NOT NULL DEFAULT 1 CHECK (mobile_block_during_focus IN (0, 1)),
    mobile_block_during_short_breaks INTEGER NOT NULL DEFAULT 1 CHECK (mobile_block_during_short_breaks IN (0, 1)),
    mobile_block_during_long_breaks INTEGER NOT NULL DEFAULT 1 CHECK (mobile_block_during_long_breaks IN (0, 1)),
    mobile_pause_during_focus_pause INTEGER NOT NULL DEFAULT 1 CHECK (mobile_pause_during_focus_pause IN (0, 1)),
    limits_enabled INTEGER NOT NULL DEFAULT 1 CHECK (limits_enabled IN (0, 1))
);

INSERT INTO distractions_settings (id) VALUES ('vault');

CREATE TABLE distractions_category_rules (
    id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1))
);

CREATE TABLE distractions_custom_categories (
    id TEXT PRIMARY KEY,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 60),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
);

CREATE TABLE distractions_host_rules (
    id TEXT PRIMARY KEY,
    list_kind TEXT NOT NULL CHECK (list_kind IN ('blocked', 'exception', 'allowed', 'category')),
    scope TEXT NOT NULL,
    host TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    category_id TEXT REFERENCES distractions_custom_categories(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);

CREATE INDEX idx_distractions_host_rules_category ON distractions_host_rules(category_id);

CREATE TABLE distractions_app_rules (
    id TEXT PRIMARY KEY,
    platform TEXT NOT NULL CHECK (platform IN ('desktop', 'mobile')),
    identity TEXT NOT NULL CHECK (length(trim(identity)) BETWEEN 1 AND 255),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 80),
    match_names_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);

CREATE TABLE distractions_usage_limits (
    id TEXT PRIMARY KEY,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    name TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    minutes_per_day INTEGER CHECK (minutes_per_day IS NULL OR minutes_per_day BETWEEN 1 AND 1440),
    minutes_per_week INTEGER CHECK (minutes_per_week IS NULL OR minutes_per_week BETWEEN 1 AND 10080)
);

CREATE TABLE distractions_usage_limit_entries (
    limit_id TEXT NOT NULL REFERENCES distractions_usage_limits(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 1999),
    id TEXT NOT NULL CHECK (length(trim(id)) BETWEEN 1 AND 80),
    name TEXT,
    color INTEGER CHECK (color IS NULL OR color BETWEEN 0 AND 31),
    website_host TEXT,
    mobile_app_name TEXT,
    mobile_app_package TEXT,
    desktop_app_name TEXT,
    desktop_app_match_names_json TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (limit_id, ordinal)
);

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_distractions_settings_insert
BEFORE INSERT ON distractions_settings
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.mode) = 'text' AND instr(CAST(NEW.mode AS BLOB), x'00') = 0 AND length(NEW.mode) <= 16 AND length(CAST(NEW.mode AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.block_during_focus) = 'integer' AND NEW.block_during_focus IN (0, 1))
    AND (typeof(NEW.block_during_short_breaks) = 'integer' AND NEW.block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.block_during_long_breaks) = 'integer' AND NEW.block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.pause_during_focus_pause) = 'integer' AND NEW.pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.desktop_enabled) = 'integer' AND NEW.desktop_enabled IN (0, 1))
    AND (typeof(NEW.desktop_block_during_focus) = 'integer' AND NEW.desktop_block_during_focus IN (0, 1))
    AND (typeof(NEW.desktop_block_during_short_breaks) = 'integer' AND NEW.desktop_block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.desktop_block_during_long_breaks) = 'integer' AND NEW.desktop_block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.desktop_pause_during_focus_pause) = 'integer' AND NEW.desktop_pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.mobile_enabled) = 'integer' AND NEW.mobile_enabled IN (0, 1))
    AND (typeof(NEW.mobile_block_during_focus) = 'integer' AND NEW.mobile_block_during_focus IN (0, 1))
    AND (typeof(NEW.mobile_block_during_short_breaks) = 'integer' AND NEW.mobile_block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.mobile_block_during_long_breaks) = 'integer' AND NEW.mobile_block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.mobile_pause_during_focus_pause) = 'integer' AND NEW.mobile_pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.limits_enabled) = 'integer' AND NEW.limits_enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_settings_update
BEFORE UPDATE OF mode, enabled, block_during_focus, block_during_short_breaks, block_during_long_breaks, pause_during_focus_pause, desktop_enabled, desktop_block_during_focus, desktop_block_during_short_breaks, desktop_block_during_long_breaks, desktop_pause_during_focus_pause, mobile_enabled, mobile_block_during_focus, mobile_block_during_short_breaks, mobile_block_during_long_breaks, mobile_pause_during_focus_pause, limits_enabled ON distractions_settings
WHEN ((typeof(NEW.mode) = 'text' AND instr(CAST(NEW.mode AS BLOB), x'00') = 0 AND length(NEW.mode) <= 16 AND length(CAST(NEW.mode AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.block_during_focus) = 'integer' AND NEW.block_during_focus IN (0, 1))
    AND (typeof(NEW.block_during_short_breaks) = 'integer' AND NEW.block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.block_during_long_breaks) = 'integer' AND NEW.block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.pause_during_focus_pause) = 'integer' AND NEW.pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.desktop_enabled) = 'integer' AND NEW.desktop_enabled IN (0, 1))
    AND (typeof(NEW.desktop_block_during_focus) = 'integer' AND NEW.desktop_block_during_focus IN (0, 1))
    AND (typeof(NEW.desktop_block_during_short_breaks) = 'integer' AND NEW.desktop_block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.desktop_block_during_long_breaks) = 'integer' AND NEW.desktop_block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.desktop_pause_during_focus_pause) = 'integer' AND NEW.desktop_pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.mobile_enabled) = 'integer' AND NEW.mobile_enabled IN (0, 1))
    AND (typeof(NEW.mobile_block_during_focus) = 'integer' AND NEW.mobile_block_during_focus IN (0, 1))
    AND (typeof(NEW.mobile_block_during_short_breaks) = 'integer' AND NEW.mobile_block_during_short_breaks IN (0, 1))
    AND (typeof(NEW.mobile_block_during_long_breaks) = 'integer' AND NEW.mobile_block_during_long_breaks IN (0, 1))
    AND (typeof(NEW.mobile_pause_during_focus_pause) = 'integer' AND NEW.mobile_pause_during_focus_pause IN (0, 1))
    AND (typeof(NEW.limits_enabled) = 'integer' AND NEW.limits_enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_settings
BEFORE UPDATE OF id ON distractions_settings
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_distractions_settings
BEFORE INSERT ON distractions_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 272 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM distractions_settings WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_distractions_settings_insert
AFTER INSERT ON distractions_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (272, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_settings_update
AFTER UPDATE ON distractions_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 272, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.mode IS NOT NEW.mode COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.block_during_focus IS NOT NEW.block_during_focus OR OLD.block_during_short_breaks IS NOT NEW.block_during_short_breaks OR OLD.block_during_long_breaks IS NOT NEW.block_during_long_breaks OR OLD.pause_during_focus_pause IS NOT NEW.pause_during_focus_pause
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.desktop_enabled IS NOT NEW.desktop_enabled OR OLD.desktop_block_during_focus IS NOT NEW.desktop_block_during_focus OR OLD.desktop_block_during_short_breaks IS NOT NEW.desktop_block_during_short_breaks OR OLD.desktop_block_during_long_breaks IS NOT NEW.desktop_block_during_long_breaks OR OLD.desktop_pause_during_focus_pause IS NOT NEW.desktop_pause_during_focus_pause
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.mobile_enabled IS NOT NEW.mobile_enabled OR OLD.mobile_block_during_focus IS NOT NEW.mobile_block_during_focus OR OLD.mobile_block_during_short_breaks IS NOT NEW.mobile_block_during_short_breaks OR OLD.mobile_block_during_long_breaks IS NOT NEW.mobile_block_during_long_breaks OR OLD.mobile_pause_during_focus_pause IS NOT NEW.mobile_pause_during_focus_pause
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.limits_enabled IS NOT NEW.limits_enabled
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_settings_delete
AFTER DELETE ON distractions_settings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (272, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_category_rules_insert
BEFORE INSERT ON distractions_category_rules
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_category_rules_update
BEFORE UPDATE OF enabled ON distractions_category_rules
WHEN ((typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_category_rules
BEFORE UPDATE OF id ON distractions_category_rules
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_distractions_category_rules_insert
AFTER INSERT ON distractions_category_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (273, NEW.id, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_category_rules_update
AFTER UPDATE ON distractions_category_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 273, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 1 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_category_rules_delete
AFTER DELETE ON distractions_category_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (273, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_custom_categories_insert
BEFORE INSERT ON distractions_custom_categories
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 60 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_custom_categories_update
BEFORE UPDATE OF created_at_ms, name, enabled ON distractions_custom_categories
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 60 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_custom_categories
BEFORE UPDATE OF id, created_at_ms ON distractions_custom_categories
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_distractions_custom_categories
BEFORE INSERT ON distractions_custom_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 274 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM distractions_custom_categories WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_distractions_custom_categories_insert
AFTER INSERT ON distractions_custom_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (274, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_custom_categories_update
AFTER UPDATE ON distractions_custom_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 274, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_custom_categories_delete
AFTER DELETE ON distractions_custom_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (274, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_host_rules_insert
BEFORE INSERT ON distractions_host_rules
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.list_kind) = 'text' AND instr(CAST(NEW.list_kind AS BLOB), x'00') = 0 AND length(NEW.list_kind) <= 16 AND length(CAST(NEW.list_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.scope) = 'text' AND instr(CAST(NEW.scope AS BLOB), x'00') = 0 AND length(NEW.scope) <= 80 AND length(CAST(NEW.scope AS BLOB)) <= 262144)
    AND (typeof(NEW.host) = 'text' AND instr(CAST(NEW.host AS BLOB), x'00') = 0 AND length(trim(NEW.host)) BETWEEN 1 AND 253 AND length(CAST(NEW.host AS BLOB)) <= 262144)
    AND (NEW.category_id IS NULL OR (typeof(NEW.category_id) = 'text' AND instr(CAST(NEW.category_id AS BLOB), x'00') = 0 AND trim(NEW.category_id) <> '' AND length(CAST(NEW.category_id AS BLOB)) <= 128))
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_host_rules_update
BEFORE UPDATE OF list_kind, scope, host, category_id, enabled, created_at_ms ON distractions_host_rules
WHEN ((typeof(NEW.list_kind) = 'text' AND instr(CAST(NEW.list_kind AS BLOB), x'00') = 0 AND length(NEW.list_kind) <= 16 AND length(CAST(NEW.list_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.scope) = 'text' AND instr(CAST(NEW.scope AS BLOB), x'00') = 0 AND length(NEW.scope) <= 80 AND length(CAST(NEW.scope AS BLOB)) <= 262144)
    AND (typeof(NEW.host) = 'text' AND instr(CAST(NEW.host AS BLOB), x'00') = 0 AND length(trim(NEW.host)) BETWEEN 1 AND 253 AND length(CAST(NEW.host AS BLOB)) <= 262144)
    AND (NEW.category_id IS NULL OR (typeof(NEW.category_id) = 'text' AND instr(CAST(NEW.category_id AS BLOB), x'00') = 0 AND trim(NEW.category_id) <> '' AND length(CAST(NEW.category_id AS BLOB)) <= 128))
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_host_rules
BEFORE UPDATE OF id, list_kind, scope, host ON distractions_host_rules
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.list_kind IS NOT NEW.list_kind COLLATE BINARY OR OLD.scope IS NOT NEW.scope COLLATE BINARY OR OLD.host IS NOT NEW.host COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_distractions_host_rules_insert
AFTER INSERT ON distractions_host_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (275, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_host_rules_update
AFTER UPDATE ON distractions_host_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 275, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.category_id IS NOT NEW.category_id COLLATE BINARY
                AND NOT (NEW.category_id IS NULL AND NOT EXISTS (SELECT 1 FROM distractions_custom_categories WHERE id = OLD.category_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_host_rules_delete
AFTER DELETE ON distractions_host_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (275, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_app_rules_insert
BEFORE INSERT ON distractions_app_rules
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.platform) = 'text' AND instr(CAST(NEW.platform AS BLOB), x'00') = 0 AND length(NEW.platform) <= 16 AND length(CAST(NEW.platform AS BLOB)) <= 262144)
    AND (typeof(NEW.identity) = 'text' AND instr(CAST(NEW.identity AS BLOB), x'00') = 0 AND length(trim(NEW.identity)) BETWEEN 1 AND 255 AND length(CAST(NEW.identity AS BLOB)) <= 262144)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 80 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.match_names_json) = 'text' AND instr(CAST(NEW.match_names_json AS BLOB), x'00') = 0 AND length(NEW.match_names_json) <= 10434 AND length(CAST(NEW.match_names_json AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_app_rules_update
BEFORE UPDATE OF platform, identity, display_name, match_names_json, enabled, created_at_ms ON distractions_app_rules
WHEN ((typeof(NEW.platform) = 'text' AND instr(CAST(NEW.platform AS BLOB), x'00') = 0 AND length(NEW.platform) <= 16 AND length(CAST(NEW.platform AS BLOB)) <= 262144)
    AND (typeof(NEW.identity) = 'text' AND instr(CAST(NEW.identity AS BLOB), x'00') = 0 AND length(trim(NEW.identity)) BETWEEN 1 AND 255 AND length(CAST(NEW.identity AS BLOB)) <= 262144)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 80 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.match_names_json) = 'text' AND instr(CAST(NEW.match_names_json AS BLOB), x'00') = 0 AND length(NEW.match_names_json) <= 10434 AND length(CAST(NEW.match_names_json AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_app_rules
BEFORE UPDATE OF id, platform, identity ON distractions_app_rules
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.platform IS NOT NEW.platform COLLATE BINARY OR OLD.identity IS NOT NEW.identity COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_distractions_app_rules_insert
AFTER INSERT ON distractions_app_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (276, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_app_rules_update
AFTER UPDATE ON distractions_app_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 276, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.match_names_json IS NOT NEW.match_names_json COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_app_rules_delete
AFTER DELETE ON distractions_app_rules
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (276, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_usage_limits_insert
BEFORE INSERT ON distractions_usage_limits
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (NEW.minutes_per_day IS NULL OR (typeof(NEW.minutes_per_day) = 'integer' AND NEW.minutes_per_day BETWEEN 1 AND 1440))
    AND (NEW.minutes_per_week IS NULL OR (typeof(NEW.minutes_per_week) = 'integer' AND NEW.minutes_per_week BETWEEN 1 AND 10080))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_usage_limits_update
BEFORE UPDATE OF created_at_ms, name, enabled, minutes_per_day, minutes_per_week ON distractions_usage_limits
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (NEW.minutes_per_day IS NULL OR (typeof(NEW.minutes_per_day) = 'integer' AND NEW.minutes_per_day BETWEEN 1 AND 1440))
    AND (NEW.minutes_per_week IS NULL OR (typeof(NEW.minutes_per_week) = 'integer' AND NEW.minutes_per_week BETWEEN 1 AND 10080))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_usage_limits
BEFORE UPDATE OF id, created_at_ms ON distractions_usage_limits
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_distractions_usage_limits
BEFORE INSERT ON distractions_usage_limits
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 277 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM distractions_usage_limits WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_distractions_usage_limits_insert
AFTER INSERT ON distractions_usage_limits
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (277, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_usage_limits_update
AFTER UPDATE ON distractions_usage_limits
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 277, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.minutes_per_day IS NOT NEW.minutes_per_day OR OLD.minutes_per_week IS NOT NEW.minutes_per_week
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_usage_limits_delete
AFTER DELETE ON distractions_usage_limits
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (277, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_usage_limit_entries_insert
BEFORE INSERT ON distractions_usage_limit_entries
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 80 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (NEW.name IS NULL OR (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 80 AND length(CAST(NEW.name AS BLOB)) <= 262144))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (NEW.website_host IS NULL OR (typeof(NEW.website_host) = 'text' AND instr(CAST(NEW.website_host AS BLOB), x'00') = 0 AND length(NEW.website_host) <= 253 AND length(CAST(NEW.website_host AS BLOB)) <= 262144))
    AND (NEW.mobile_app_name IS NULL OR (typeof(NEW.mobile_app_name) = 'text' AND instr(CAST(NEW.mobile_app_name AS BLOB), x'00') = 0 AND length(NEW.mobile_app_name) <= 80 AND length(CAST(NEW.mobile_app_name AS BLOB)) <= 262144))
    AND (NEW.mobile_app_package IS NULL OR (typeof(NEW.mobile_app_package) = 'text' AND instr(CAST(NEW.mobile_app_package AS BLOB), x'00') = 0 AND length(NEW.mobile_app_package) <= 255 AND length(CAST(NEW.mobile_app_package AS BLOB)) <= 262144))
    AND (NEW.desktop_app_name IS NULL OR (typeof(NEW.desktop_app_name) = 'text' AND instr(CAST(NEW.desktop_app_name AS BLOB), x'00') = 0 AND length(NEW.desktop_app_name) <= 80 AND length(CAST(NEW.desktop_app_name AS BLOB)) <= 262144))
    AND (typeof(NEW.desktop_app_match_names_json) = 'text' AND instr(CAST(NEW.desktop_app_match_names_json AS BLOB), x'00') = 0 AND length(NEW.desktop_app_match_names_json) <= 10434 AND length(CAST(NEW.desktop_app_match_names_json AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_usage_limit_entries_update
BEFORE UPDATE OF id, name, color, website_host, mobile_app_name, mobile_app_package, desktop_app_name, desktop_app_match_names_json ON distractions_usage_limit_entries
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 80 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (NEW.name IS NULL OR (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 80 AND length(CAST(NEW.name AS BLOB)) <= 262144))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (NEW.website_host IS NULL OR (typeof(NEW.website_host) = 'text' AND instr(CAST(NEW.website_host AS BLOB), x'00') = 0 AND length(NEW.website_host) <= 253 AND length(CAST(NEW.website_host AS BLOB)) <= 262144))
    AND (NEW.mobile_app_name IS NULL OR (typeof(NEW.mobile_app_name) = 'text' AND instr(CAST(NEW.mobile_app_name AS BLOB), x'00') = 0 AND length(NEW.mobile_app_name) <= 80 AND length(CAST(NEW.mobile_app_name AS BLOB)) <= 262144))
    AND (NEW.mobile_app_package IS NULL OR (typeof(NEW.mobile_app_package) = 'text' AND instr(CAST(NEW.mobile_app_package AS BLOB), x'00') = 0 AND length(NEW.mobile_app_package) <= 255 AND length(CAST(NEW.mobile_app_package AS BLOB)) <= 262144))
    AND (NEW.desktop_app_name IS NULL OR (typeof(NEW.desktop_app_name) = 'text' AND instr(CAST(NEW.desktop_app_name AS BLOB), x'00') = 0 AND length(NEW.desktop_app_name) <= 80 AND length(CAST(NEW.desktop_app_name AS BLOB)) <= 262144))
    AND (typeof(NEW.desktop_app_match_names_json) = 'text' AND instr(CAST(NEW.desktop_app_match_names_json AS BLOB), x'00') = 0 AND length(NEW.desktop_app_match_names_json) <= 10434 AND length(CAST(NEW.desktop_app_match_names_json AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_distractions_usage_limit_entries_insert
AFTER INSERT ON distractions_usage_limit_entries
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (277, NEW.limit_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_usage_limit_entries_update
AFTER UPDATE ON distractions_usage_limit_entries
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 277, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.limit_id AS key UNION SELECT OLD.limit_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_usage_limit_entries_delete
AFTER DELETE ON distractions_usage_limit_entries
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (277, OLD.limit_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

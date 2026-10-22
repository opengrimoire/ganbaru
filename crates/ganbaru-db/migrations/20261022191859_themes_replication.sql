-- User themes replicate one row per theme. The seed is fixed at creation, the editor's content
-- (blend canvas, engine version, Calendar default, live tokens, and palette) merges as one
-- register, and the highest dismissed engine upgrade merges as a maximum. The dismissal moves
-- from its own table onto the theme row so it replicates with the theme.
ALTER TABLE themes ADD COLUMN upgrade_dismissed_engine_version INTEGER NOT NULL DEFAULT 0
    CHECK (upgrade_dismissed_engine_version >= 0);

UPDATE themes
SET upgrade_dismissed_engine_version = (
    SELECT max(engine_version) FROM theme_upgrade_dismissals
    WHERE theme_upgrade_dismissals.theme_id = themes.id AND engine_version >= 0
)
WHERE EXISTS (
    SELECT 1 FROM theme_upgrade_dismissals
    WHERE theme_upgrade_dismissals.theme_id = themes.id AND engine_version >= 0
);

DROP TABLE theme_upgrade_dismissals;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.
CREATE TRIGGER sync_values_themes_insert
BEFORE INSERT ON themes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.seed_blend_canvas) = 'text' AND instr(CAST(NEW.seed_blend_canvas AS BLOB), x'00') = 0 AND length(NEW.seed_blend_canvas) <= 9 AND length(CAST(NEW.seed_blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_icon_label) = 'text' AND instr(CAST(NEW.seed_icon_label AS BLOB), x'00') = 0 AND length(NEW.seed_icon_label) <= 5 AND length(CAST(NEW.seed_icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_mode) = 'text' AND instr(CAST(NEW.seed_calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_mode) <= 10 AND length(CAST(NEW.seed_calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_custom) = 'text' AND instr(CAST(NEW.seed_calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_custom) <= 9 AND length(CAST(NEW.seed_calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 60 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon_label) = 'text' AND instr(CAST(NEW.icon_label AS BLOB), x'00') = 0 AND length(NEW.icon_label) <= 5 AND length(CAST(NEW.icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.blend_canvas) = 'text' AND instr(CAST(NEW.blend_canvas AS BLOB), x'00') = 0 AND length(NEW.blend_canvas) <= 9 AND length(CAST(NEW.blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.derivation_engine_version) = 'integer' AND NEW.derivation_engine_version BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.calendar_default_mode) = 'text' AND instr(CAST(NEW.calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.calendar_default_mode) <= 10 AND length(CAST(NEW.calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.calendar_default_custom) = 'text' AND instr(CAST(NEW.calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.calendar_default_custom) <= 9 AND length(CAST(NEW.calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.upgrade_dismissed_engine_version) = 'integer' AND NEW.upgrade_dismissed_engine_version BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_themes_update
BEFORE UPDATE OF created_at_ms, seed_blend_canvas, seed_icon_label, seed_calendar_default_mode, seed_calendar_default_custom, display_name, icon_label, blend_canvas, derivation_engine_version, calendar_default_mode, calendar_default_custom, upgrade_dismissed_engine_version, updated_at_ms ON themes
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.seed_blend_canvas) = 'text' AND instr(CAST(NEW.seed_blend_canvas AS BLOB), x'00') = 0 AND length(NEW.seed_blend_canvas) <= 9 AND length(CAST(NEW.seed_blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_icon_label) = 'text' AND instr(CAST(NEW.seed_icon_label AS BLOB), x'00') = 0 AND length(NEW.seed_icon_label) <= 5 AND length(CAST(NEW.seed_icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_mode) = 'text' AND instr(CAST(NEW.seed_calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_mode) <= 10 AND length(CAST(NEW.seed_calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_custom) = 'text' AND instr(CAST(NEW.seed_calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_custom) <= 9 AND length(CAST(NEW.seed_calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 60 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon_label) = 'text' AND instr(CAST(NEW.icon_label AS BLOB), x'00') = 0 AND length(NEW.icon_label) <= 5 AND length(CAST(NEW.icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.blend_canvas) = 'text' AND instr(CAST(NEW.blend_canvas AS BLOB), x'00') = 0 AND length(NEW.blend_canvas) <= 9 AND length(CAST(NEW.blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.derivation_engine_version) = 'integer' AND NEW.derivation_engine_version BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.calendar_default_mode) = 'text' AND instr(CAST(NEW.calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.calendar_default_mode) <= 10 AND length(CAST(NEW.calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.calendar_default_custom) = 'text' AND instr(CAST(NEW.calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.calendar_default_custom) <= 9 AND length(CAST(NEW.calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.upgrade_dismissed_engine_version) = 'integer' AND NEW.upgrade_dismissed_engine_version BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_themes
BEFORE UPDATE OF id, created_at_ms, seed_blend_canvas, seed_icon_label, seed_calendar_default_mode, seed_calendar_default_custom ON themes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms OR OLD.seed_blend_canvas IS NOT NEW.seed_blend_canvas COLLATE BINARY OR OLD.seed_icon_label IS NOT NEW.seed_icon_label COLLATE BINARY OR OLD.seed_calendar_default_mode IS NOT NEW.seed_calendar_default_mode COLLATE BINARY OR OLD.seed_calendar_default_custom IS NOT NEW.seed_calendar_default_custom COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_themes
BEFORE INSERT ON themes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 17 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM themes WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_themes_insert
AFTER INSERT ON themes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, NEW.id, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_themes_update
AFTER UPDATE ON themes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 17, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.icon_label IS NOT NEW.icon_label COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.blend_canvas IS NOT NEW.blend_canvas COLLATE BINARY OR OLD.derivation_engine_version IS NOT NEW.derivation_engine_version OR OLD.calendar_default_mode IS NOT NEW.calendar_default_mode COLLATE BINARY OR OLD.calendar_default_custom IS NOT NEW.calendar_default_custom COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.upgrade_dismissed_engine_version IS NOT NEW.upgrade_dismissed_engine_version
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_themes_delete
AFTER DELETE ON themes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_theme_seed_tokens_insert
AFTER INSERT ON theme_seed_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, NEW.theme_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_seed_tokens_update
AFTER UPDATE ON theme_seed_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 17, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.theme_id AS key UNION SELECT OLD.theme_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_seed_tokens_delete
AFTER DELETE ON theme_seed_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, OLD.theme_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_seed_event_palette_insert
AFTER INSERT ON theme_seed_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, NEW.theme_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_seed_event_palette_update
AFTER UPDATE ON theme_seed_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 17, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.theme_id AS key UNION SELECT OLD.theme_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_seed_event_palette_delete
AFTER DELETE ON theme_seed_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, OLD.theme_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_tokens_insert
AFTER INSERT ON theme_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, NEW.theme_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_tokens_update
AFTER UPDATE ON theme_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 17, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.theme_id AS key UNION SELECT OLD.theme_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_tokens_delete
AFTER DELETE ON theme_tokens
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, OLD.theme_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_event_palette_insert
AFTER INSERT ON theme_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, NEW.theme_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_event_palette_update
AFTER UPDATE ON theme_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 17, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.theme_id AS key UNION SELECT OLD.theme_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_theme_event_palette_delete
AFTER DELETE ON theme_event_palette
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (17, OLD.theme_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

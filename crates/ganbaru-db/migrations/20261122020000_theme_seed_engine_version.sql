-- A theme's seed records the derivation engine version its seed tokens were derived with, so
-- resetting a theme to its seed restores the engine stamp with the tokens and a reset after a
-- rebake offers the rebake again. Existing seeds take the theme's current stamp.
DROP TRIGGER sync_immutable_themes;
DROP TRIGGER sync_values_themes_insert;
DROP TRIGGER sync_values_themes_update;
ALTER TABLE themes ADD COLUMN seed_derivation_engine_version INTEGER NOT NULL DEFAULT 0
  CHECK (seed_derivation_engine_version >= 0);
UPDATE themes SET seed_derivation_engine_version = derivation_engine_version;

CREATE TRIGGER sync_immutable_themes
BEFORE UPDATE OF id, created_at_ms, seed_blend_canvas, seed_icon_label, seed_calendar_default_mode, seed_calendar_default_custom, seed_derivation_engine_version ON themes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms OR OLD.seed_blend_canvas IS NOT NEW.seed_blend_canvas COLLATE BINARY OR OLD.seed_icon_label IS NOT NEW.seed_icon_label COLLATE BINARY OR OLD.seed_calendar_default_mode IS NOT NEW.seed_calendar_default_mode COLLATE BINARY OR OLD.seed_calendar_default_custom IS NOT NEW.seed_calendar_default_custom COLLATE BINARY OR OLD.seed_derivation_engine_version IS NOT NEW.seed_derivation_engine_version) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_values_themes_insert
BEFORE INSERT ON themes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128 AND length(CAST(NEW.id AS BLOB)) <= 64 AND NEW.id GLOB '[a-z0-9]*' AND NEW.id NOT GLOB '*[^a-z0-9-]*' AND NEW.id NOT IN ('light', 'dark'))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.seed_blend_canvas) = 'text' AND instr(CAST(NEW.seed_blend_canvas AS BLOB), x'00') = 0 AND length(NEW.seed_blend_canvas) <= 9 AND length(CAST(NEW.seed_blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_icon_label) = 'text' AND instr(CAST(NEW.seed_icon_label AS BLOB), x'00') = 0 AND length(NEW.seed_icon_label) <= 5 AND length(CAST(NEW.seed_icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_mode) = 'text' AND instr(CAST(NEW.seed_calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_mode) <= 10 AND length(CAST(NEW.seed_calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_custom) = 'text' AND instr(CAST(NEW.seed_calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_custom) <= 9 AND length(CAST(NEW.seed_calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_derivation_engine_version) = 'integer' AND NEW.seed_derivation_engine_version BETWEEN 0 AND 9223372036854775807)
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
BEFORE UPDATE OF created_at_ms, seed_blend_canvas, seed_icon_label, seed_calendar_default_mode, seed_calendar_default_custom, seed_derivation_engine_version, display_name, icon_label, blend_canvas, derivation_engine_version, calendar_default_mode, calendar_default_custom, upgrade_dismissed_engine_version, updated_at_ms ON themes
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.seed_blend_canvas) = 'text' AND instr(CAST(NEW.seed_blend_canvas AS BLOB), x'00') = 0 AND length(NEW.seed_blend_canvas) <= 9 AND length(CAST(NEW.seed_blend_canvas AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_icon_label) = 'text' AND instr(CAST(NEW.seed_icon_label AS BLOB), x'00') = 0 AND length(NEW.seed_icon_label) <= 5 AND length(CAST(NEW.seed_icon_label AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_mode) = 'text' AND instr(CAST(NEW.seed_calendar_default_mode AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_mode) <= 10 AND length(CAST(NEW.seed_calendar_default_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_calendar_default_custom) = 'text' AND instr(CAST(NEW.seed_calendar_default_custom AS BLOB), x'00') = 0 AND length(NEW.seed_calendar_default_custom) <= 9 AND length(CAST(NEW.seed_calendar_default_custom AS BLOB)) <= 262144)
    AND (typeof(NEW.seed_derivation_engine_version) = 'integer' AND NEW.seed_derivation_engine_version BETWEEN 0 AND 9223372036854775807)
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

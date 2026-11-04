-- Hold new theme keys to the theme id rule, so a stored row can always be read back. The CHECK on
-- themes stays as it is, because SQLite cannot change one without rebuilding the table.
DROP TRIGGER sync_values_themes_insert;

CREATE TRIGGER sync_values_themes_insert
BEFORE INSERT ON themes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128 AND length(CAST(NEW.id AS BLOB)) <= 64 AND NEW.id GLOB '[a-z0-9]*' AND NEW.id NOT GLOB '*[^a-z0-9-]*' AND NEW.id NOT IN ('light', 'dark'))
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

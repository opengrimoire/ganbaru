-- no-transaction
-- A project may stand outside every group, and deleting a group leaves its projects ungrouped
-- instead of deleting them. SQLite cannot relax NOT NULL or change a foreign key action in
-- place, so `projects` is rebuilt. Foreign keys are off for the rebuild because dropping the old
-- table with them on would run every ON DELETE CASCADE in the tables that reference projects.
-- The rebuild runs in its own transaction and repeats safely if it stops before sqlx records it.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP TABLE IF EXISTS projects_rebuild;

CREATE TABLE projects_rebuild (
  id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
  group_id TEXT REFERENCES project_groups(id) ON DELETE SET NULL,
  name TEXT NOT NULL CHECK (trim(name) <> ''),
  icon TEXT NOT NULL DEFAULT 'folder' CHECK (trim(icon) <> ''),
  color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'hidden', 'archived')),
  default_event_duration_minutes INTEGER CHECK (
      default_event_duration_minutes IS NULL
      OR (default_event_duration_minutes > 0 AND default_event_duration_minutes <= 1440)
  ),
  default_pomodoro_mode TEXT NOT NULL DEFAULT 'preset' CHECK (
      default_pomodoro_mode IN ('none', 'preset', 'custom')
  ),
  default_pomodoro_preset_key TEXT DEFAULT 'adaptive' CHECK (
      default_pomodoro_preset_key IS NULL
      OR default_pomodoro_preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
  ),
  default_pomodoro_focus_minutes INTEGER CHECK (
      default_pomodoro_focus_minutes IS NULL
      OR (default_pomodoro_focus_minutes >= 1 AND default_pomodoro_focus_minutes <= 120)
  ),
  default_pomodoro_short_break_minutes INTEGER CHECK (
      default_pomodoro_short_break_minutes IS NULL
      OR (default_pomodoro_short_break_minutes >= 1 AND default_pomodoro_short_break_minutes <= 30)
  ),
  default_pomodoro_long_break_minutes INTEGER CHECK (
      default_pomodoro_long_break_minutes IS NULL
      OR (default_pomodoro_long_break_minutes >= 1 AND default_pomodoro_long_break_minutes <= 60)
  ),
  default_pomodoro_long_break_after_focus_count INTEGER CHECK (
      default_pomodoro_long_break_after_focus_count IS NULL
      OR (
          default_pomodoro_long_break_after_focus_count >= 1
          AND default_pomodoro_long_break_after_focus_count <= 12
      )
  ),
  focus_playlist_id TEXT,
  break_playlist_id TEXT,
  work_environment_id TEXT,
  blocker_ruleset_id TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
  default_event_name TEXT CHECK (
      default_event_name IS NULL OR trim(default_event_name) <> ''
  ),
  default_idle_settings_source TEXT NOT NULL DEFAULT 'global'
      CHECK (default_idle_settings_source IN ('global', 'custom')),
  default_idle_pause_enabled INTEGER NOT NULL DEFAULT 1
      CHECK (default_idle_pause_enabled IN (0, 1)),
  default_idle_threshold_minutes INTEGER NOT NULL DEFAULT 3
      CHECK (default_idle_threshold_minutes IN (1, 2, 3, 4, 5, 10, 15)),
  default_event_time_mode TEXT NOT NULL DEFAULT 'timed'
      CHECK (default_event_time_mode IN ('timed', 'all_day')),
  notes_default_open_mode TEXT CHECK (
      notes_default_open_mode IS NULL
      OR notes_default_open_mode IN ('center', 'side', 'full')
  ),
  notes_history_retention_days INTEGER CHECK (
      notes_history_retention_days IS NULL
      OR notes_history_retention_days IN (0, 7, 30, 90, 180, 365)
  ),
  order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128),
  icon_byte_size INTEGER CHECK (
    icon_byte_size IS NULL
    OR (substr(icon, 1, 20) = 'asset:project-icons/' AND icon_byte_size BETWEEN 1 AND 3145728)
),
  CHECK (
      (
          default_pomodoro_mode = 'none'
          AND default_pomodoro_preset_key IS NULL
          AND default_pomodoro_focus_minutes IS NULL
          AND default_pomodoro_short_break_minutes IS NULL
          AND default_pomodoro_long_break_minutes IS NULL
          AND default_pomodoro_long_break_after_focus_count IS NULL
      )
      OR (
          default_pomodoro_mode = 'preset'
          AND default_pomodoro_preset_key IS NOT NULL
          AND default_pomodoro_focus_minutes IS NULL
          AND default_pomodoro_short_break_minutes IS NULL
          AND default_pomodoro_long_break_minutes IS NULL
          AND default_pomodoro_long_break_after_focus_count IS NULL
      )
      OR (
          default_pomodoro_mode = 'custom'
          AND default_pomodoro_preset_key IS NULL
          AND default_pomodoro_focus_minutes IS NOT NULL
          AND default_pomodoro_short_break_minutes IS NOT NULL
          AND default_pomodoro_long_break_minutes IS NOT NULL
          AND default_pomodoro_long_break_after_focus_count IS NOT NULL
      )
  )
);

INSERT INTO projects_rebuild SELECT * FROM projects;

DROP TABLE projects;

-- Triggers on other tables name `projects` while it is briefly absent. The legacy rename leaves
-- them untouched instead of failing to parse them, and they resolve again once the name exists.
PRAGMA legacy_alter_table = ON;
ALTER TABLE projects_rebuild RENAME TO projects;
PRAGMA legacy_alter_table = OFF;

CREATE INDEX idx_projects_status ON projects(status);
CREATE INDEX idx_projects_group_order ON projects(group_id, order_key, id);

CREATE TRIGGER projects_icon_size_insert
BEFORE INSERT ON projects
WHEN substr(NEW.icon, 1, 20) = 'asset:project-icons/' AND NEW.icon_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project icon size is required');
END;

CREATE TRIGGER projects_icon_size_update
BEFORE UPDATE OF icon, icon_byte_size ON projects
WHEN substr(NEW.icon, 1, 20) = 'asset:project-icons/' AND NEW.icon_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project icon size is required');
END;

CREATE TRIGGER projects_order_key_insert
BEFORE INSERT ON projects
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project order key is required');
END;

CREATE TRIGGER projects_order_key_update
BEFORE UPDATE OF order_key ON projects
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project order key is required');
END;

CREATE TRIGGER sync_values_projects_insert
BEFORE INSERT ON projects
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.group_id IS NULL OR (typeof(NEW.group_id) = 'text' AND instr(CAST(NEW.group_id AS BLOB), x'00') = 0 AND trim(NEW.group_id) <> '' AND length(CAST(NEW.group_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.default_event_duration_minutes IS NULL OR (typeof(NEW.default_event_duration_minutes) = 'integer' AND NEW.default_event_duration_minutes BETWEEN 1 AND 1440))
    AND (NEW.default_event_name IS NULL OR (typeof(NEW.default_event_name) = 'text' AND instr(CAST(NEW.default_event_name AS BLOB), x'00') = 0 AND length(NEW.default_event_name) <= 1000 AND length(CAST(NEW.default_event_name AS BLOB)) <= 262144))
    AND (typeof(NEW.default_event_time_mode) = 'text' AND instr(CAST(NEW.default_event_time_mode AS BLOB), x'00') = 0 AND length(NEW.default_event_time_mode) <= 32 AND length(CAST(NEW.default_event_time_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.default_pomodoro_mode) = 'text' AND instr(CAST(NEW.default_pomodoro_mode AS BLOB), x'00') = 0 AND length(NEW.default_pomodoro_mode) <= 32 AND length(CAST(NEW.default_pomodoro_mode AS BLOB)) <= 262144)
    AND (NEW.default_pomodoro_preset_key IS NULL OR (typeof(NEW.default_pomodoro_preset_key) = 'text' AND instr(CAST(NEW.default_pomodoro_preset_key AS BLOB), x'00') = 0 AND length(NEW.default_pomodoro_preset_key) <= 32 AND length(CAST(NEW.default_pomodoro_preset_key AS BLOB)) <= 262144))
    AND (NEW.default_pomodoro_focus_minutes IS NULL OR (typeof(NEW.default_pomodoro_focus_minutes) = 'integer' AND NEW.default_pomodoro_focus_minutes BETWEEN 1 AND 120))
    AND (NEW.default_pomodoro_short_break_minutes IS NULL OR (typeof(NEW.default_pomodoro_short_break_minutes) = 'integer' AND NEW.default_pomodoro_short_break_minutes BETWEEN 1 AND 30))
    AND (NEW.default_pomodoro_long_break_minutes IS NULL OR (typeof(NEW.default_pomodoro_long_break_minutes) = 'integer' AND NEW.default_pomodoro_long_break_minutes BETWEEN 1 AND 60))
    AND (NEW.default_pomodoro_long_break_after_focus_count IS NULL OR (typeof(NEW.default_pomodoro_long_break_after_focus_count) = 'integer' AND NEW.default_pomodoro_long_break_after_focus_count BETWEEN 1 AND 12))
    AND (typeof(NEW.default_idle_settings_source) = 'text' AND instr(CAST(NEW.default_idle_settings_source AS BLOB), x'00') = 0 AND length(NEW.default_idle_settings_source) <= 32 AND length(CAST(NEW.default_idle_settings_source AS BLOB)) <= 262144)
    AND (typeof(NEW.default_idle_pause_enabled) = 'integer' AND NEW.default_idle_pause_enabled IN (0, 1))
    AND (typeof(NEW.default_idle_threshold_minutes) = 'integer' AND NEW.default_idle_threshold_minutes BETWEEN 1 AND 15)
    AND (NEW.focus_playlist_id IS NULL OR (typeof(NEW.focus_playlist_id) = 'text' AND instr(CAST(NEW.focus_playlist_id AS BLOB), x'00') = 0 AND length(NEW.focus_playlist_id) <= 128 AND length(CAST(NEW.focus_playlist_id AS BLOB)) <= 262144))
    AND (NEW.break_playlist_id IS NULL OR (typeof(NEW.break_playlist_id) = 'text' AND instr(CAST(NEW.break_playlist_id AS BLOB), x'00') = 0 AND length(NEW.break_playlist_id) <= 128 AND length(CAST(NEW.break_playlist_id AS BLOB)) <= 262144))
    AND (NEW.work_environment_id IS NULL OR (typeof(NEW.work_environment_id) = 'text' AND instr(CAST(NEW.work_environment_id AS BLOB), x'00') = 0 AND length(NEW.work_environment_id) <= 128 AND length(CAST(NEW.work_environment_id AS BLOB)) <= 262144))
    AND (NEW.blocker_ruleset_id IS NULL OR (typeof(NEW.blocker_ruleset_id) = 'text' AND instr(CAST(NEW.blocker_ruleset_id AS BLOB), x'00') = 0 AND length(NEW.blocker_ruleset_id) <= 128 AND length(CAST(NEW.blocker_ruleset_id AS BLOB)) <= 262144))
    AND (NEW.notes_default_open_mode IS NULL OR (typeof(NEW.notes_default_open_mode) = 'text' AND instr(CAST(NEW.notes_default_open_mode AS BLOB), x'00') = 0 AND length(NEW.notes_default_open_mode) <= 32 AND length(CAST(NEW.notes_default_open_mode AS BLOB)) <= 262144))
    AND (NEW.notes_history_retention_days IS NULL OR (typeof(NEW.notes_history_retention_days) = 'integer' AND NEW.notes_history_retention_days BETWEEN 0 AND 365))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_projects_update
BEFORE UPDATE OF created_at, group_id, name, icon, icon_byte_size, color, order_key, status, default_event_duration_minutes, default_event_name, default_event_time_mode, default_pomodoro_mode, default_pomodoro_preset_key, default_pomodoro_focus_minutes, default_pomodoro_short_break_minutes, default_pomodoro_long_break_minutes, default_pomodoro_long_break_after_focus_count, default_idle_settings_source, default_idle_pause_enabled, default_idle_threshold_minutes, focus_playlist_id, break_playlist_id, work_environment_id, blocker_ruleset_id, notes_default_open_mode, notes_history_retention_days, updated_at ON projects
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.group_id IS NULL OR (typeof(NEW.group_id) = 'text' AND instr(CAST(NEW.group_id AS BLOB), x'00') = 0 AND trim(NEW.group_id) <> '' AND length(CAST(NEW.group_id AS BLOB)) <= 128))
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.default_event_duration_minutes IS NULL OR (typeof(NEW.default_event_duration_minutes) = 'integer' AND NEW.default_event_duration_minutes BETWEEN 1 AND 1440))
    AND (NEW.default_event_name IS NULL OR (typeof(NEW.default_event_name) = 'text' AND instr(CAST(NEW.default_event_name AS BLOB), x'00') = 0 AND length(NEW.default_event_name) <= 1000 AND length(CAST(NEW.default_event_name AS BLOB)) <= 262144))
    AND (typeof(NEW.default_event_time_mode) = 'text' AND instr(CAST(NEW.default_event_time_mode AS BLOB), x'00') = 0 AND length(NEW.default_event_time_mode) <= 32 AND length(CAST(NEW.default_event_time_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.default_pomodoro_mode) = 'text' AND instr(CAST(NEW.default_pomodoro_mode AS BLOB), x'00') = 0 AND length(NEW.default_pomodoro_mode) <= 32 AND length(CAST(NEW.default_pomodoro_mode AS BLOB)) <= 262144)
    AND (NEW.default_pomodoro_preset_key IS NULL OR (typeof(NEW.default_pomodoro_preset_key) = 'text' AND instr(CAST(NEW.default_pomodoro_preset_key AS BLOB), x'00') = 0 AND length(NEW.default_pomodoro_preset_key) <= 32 AND length(CAST(NEW.default_pomodoro_preset_key AS BLOB)) <= 262144))
    AND (NEW.default_pomodoro_focus_minutes IS NULL OR (typeof(NEW.default_pomodoro_focus_minutes) = 'integer' AND NEW.default_pomodoro_focus_minutes BETWEEN 1 AND 120))
    AND (NEW.default_pomodoro_short_break_minutes IS NULL OR (typeof(NEW.default_pomodoro_short_break_minutes) = 'integer' AND NEW.default_pomodoro_short_break_minutes BETWEEN 1 AND 30))
    AND (NEW.default_pomodoro_long_break_minutes IS NULL OR (typeof(NEW.default_pomodoro_long_break_minutes) = 'integer' AND NEW.default_pomodoro_long_break_minutes BETWEEN 1 AND 60))
    AND (NEW.default_pomodoro_long_break_after_focus_count IS NULL OR (typeof(NEW.default_pomodoro_long_break_after_focus_count) = 'integer' AND NEW.default_pomodoro_long_break_after_focus_count BETWEEN 1 AND 12))
    AND (typeof(NEW.default_idle_settings_source) = 'text' AND instr(CAST(NEW.default_idle_settings_source AS BLOB), x'00') = 0 AND length(NEW.default_idle_settings_source) <= 32 AND length(CAST(NEW.default_idle_settings_source AS BLOB)) <= 262144)
    AND (typeof(NEW.default_idle_pause_enabled) = 'integer' AND NEW.default_idle_pause_enabled IN (0, 1))
    AND (typeof(NEW.default_idle_threshold_minutes) = 'integer' AND NEW.default_idle_threshold_minutes BETWEEN 1 AND 15)
    AND (NEW.focus_playlist_id IS NULL OR (typeof(NEW.focus_playlist_id) = 'text' AND instr(CAST(NEW.focus_playlist_id AS BLOB), x'00') = 0 AND length(NEW.focus_playlist_id) <= 128 AND length(CAST(NEW.focus_playlist_id AS BLOB)) <= 262144))
    AND (NEW.break_playlist_id IS NULL OR (typeof(NEW.break_playlist_id) = 'text' AND instr(CAST(NEW.break_playlist_id AS BLOB), x'00') = 0 AND length(NEW.break_playlist_id) <= 128 AND length(CAST(NEW.break_playlist_id AS BLOB)) <= 262144))
    AND (NEW.work_environment_id IS NULL OR (typeof(NEW.work_environment_id) = 'text' AND instr(CAST(NEW.work_environment_id AS BLOB), x'00') = 0 AND length(NEW.work_environment_id) <= 128 AND length(CAST(NEW.work_environment_id AS BLOB)) <= 262144))
    AND (NEW.blocker_ruleset_id IS NULL OR (typeof(NEW.blocker_ruleset_id) = 'text' AND instr(CAST(NEW.blocker_ruleset_id AS BLOB), x'00') = 0 AND length(NEW.blocker_ruleset_id) <= 128 AND length(CAST(NEW.blocker_ruleset_id AS BLOB)) <= 262144))
    AND (NEW.notes_default_open_mode IS NULL OR (typeof(NEW.notes_default_open_mode) = 'text' AND instr(CAST(NEW.notes_default_open_mode AS BLOB), x'00') = 0 AND length(NEW.notes_default_open_mode) <= 32 AND length(CAST(NEW.notes_default_open_mode AS BLOB)) <= 262144))
    AND (NEW.notes_history_retention_days IS NULL OR (typeof(NEW.notes_history_retention_days) = 'integer' AND NEW.notes_history_retention_days BETWEEN 0 AND 365))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_projects
BEFORE UPDATE OF id, created_at ON projects
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_projects
BEFORE INSERT ON projects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 97 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM projects WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_projects_insert
AFTER INSERT ON projects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (97, NEW.id, 65535, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_projects_update
AFTER UPDATE ON projects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 97, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.group_id IS NOT NEW.group_id COLLATE BINARY
                AND NOT (NEW.group_id IS NULL AND NOT EXISTS (SELECT 1 FROM project_groups WHERE id = OLD.group_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.icon_byte_size IS NOT NEW.icon_byte_size
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.status IS NOT NEW.status COLLATE BINARY
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.default_event_duration_minutes IS NOT NEW.default_event_duration_minutes OR OLD.default_event_name IS NOT NEW.default_event_name COLLATE BINARY OR OLD.default_event_time_mode IS NOT NEW.default_event_time_mode COLLATE BINARY
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.default_pomodoro_mode IS NOT NEW.default_pomodoro_mode COLLATE BINARY OR OLD.default_pomodoro_preset_key IS NOT NEW.default_pomodoro_preset_key COLLATE BINARY OR OLD.default_pomodoro_focus_minutes IS NOT NEW.default_pomodoro_focus_minutes OR OLD.default_pomodoro_short_break_minutes IS NOT NEW.default_pomodoro_short_break_minutes OR OLD.default_pomodoro_long_break_minutes IS NOT NEW.default_pomodoro_long_break_minutes OR OLD.default_pomodoro_long_break_after_focus_count IS NOT NEW.default_pomodoro_long_break_after_focus_count
                THEN 256 ELSE 0 END)
            | (CASE WHEN OLD.default_idle_settings_source IS NOT NEW.default_idle_settings_source COLLATE BINARY OR OLD.default_idle_pause_enabled IS NOT NEW.default_idle_pause_enabled OR OLD.default_idle_threshold_minutes IS NOT NEW.default_idle_threshold_minutes
                THEN 512 ELSE 0 END)
            | (CASE WHEN OLD.focus_playlist_id IS NOT NEW.focus_playlist_id COLLATE BINARY
                THEN 1024 ELSE 0 END)
            | (CASE WHEN OLD.break_playlist_id IS NOT NEW.break_playlist_id COLLATE BINARY
                THEN 2048 ELSE 0 END)
            | (CASE WHEN OLD.work_environment_id IS NOT NEW.work_environment_id COLLATE BINARY
                THEN 4096 ELSE 0 END)
            | (CASE WHEN OLD.blocker_ruleset_id IS NOT NEW.blocker_ruleset_id COLLATE BINARY
                THEN 8192 ELSE 0 END)
            | (CASE WHEN OLD.notes_default_open_mode IS NOT NEW.notes_default_open_mode COLLATE BINARY OR OLD.notes_history_retention_days IS NOT NEW.notes_history_retention_days
                THEN 16384 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 32768 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_projects_delete
AFTER DELETE ON projects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (97, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA foreign_keys = ON;

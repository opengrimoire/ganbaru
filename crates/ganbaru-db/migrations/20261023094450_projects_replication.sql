-- no-transaction
-- Projects replicate through the vault manifest. Constraints the engine cannot resolve move into
-- rules it can. A dependency is keyed by the derived key of its two tasks, which `id` and
-- `sync_key` both hold, so the pair is unique and a CHECK pins `sync_key` to the key. A managed
-- working folder is keyed by its project and created with it, so one managed folder per project
-- follows from the key; a managed folder that is never archived stays a local rule of the
-- protect trigger and the commands, because kind and archive time merge in different groups.
--
-- SQLite changes a CHECK or a table constraint only by rebuilding the table. Foreign keys are
-- disabled for the rebuild and every copy keeps its columns and values. Legacy rename keeps
-- SQLite from reparsing triggers on other tables while a rebuilt table is briefly absent. The
-- rebuild commits only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- project_task_dependencies
DROP TRIGGER project_task_dependencies_sync_key_insert;
DROP TRIGGER project_task_dependencies_sync_key_update;

CREATE TABLE project_task_dependencies_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    blocking_task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    blocked_task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    dependency_type TEXT NOT NULL DEFAULT 'blocks' CHECK (dependency_type IN ('blocks')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    sync_key TEXT NOT NULL CHECK (sync_key = id AND length(sync_key) <= 128),
    UNIQUE (blocking_task_id, blocked_task_id),
    CHECK (blocking_task_id <> blocked_task_id)
);

INSERT INTO project_task_dependencies_next (
    id,
    blocking_task_id,
    blocked_task_id,
    dependency_type,
    created_at,
    sync_key
)
SELECT
    id,
    blocking_task_id,
    blocked_task_id,
    dependency_type,
    created_at,
    sync_key
FROM project_task_dependencies;

DROP TABLE project_task_dependencies;
ALTER TABLE project_task_dependencies_next RENAME TO project_task_dependencies;

CREATE INDEX idx_project_task_dependencies_blocked ON project_task_dependencies(blocked_task_id);
CREATE UNIQUE INDEX idx_project_task_dependencies_sync_key ON project_task_dependencies(sync_key);

-- project_working_folders
CREATE TABLE project_working_folders_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    project_id TEXT NOT NULL REFERENCES projects(id) ON UPDATE CASCADE ON DELETE CASCADE,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 240),
    kind TEXT NOT NULL CHECK (kind IN ('managed', 'external')),
    managed_relative_path TEXT CHECK (
        managed_relative_path IS NULL OR (
            length(managed_relative_path) BETWEEN 1 AND 2048
            AND managed_relative_path NOT LIKE '/%'
            AND managed_relative_path NOT LIKE '%/../%'
            AND managed_relative_path NOT LIKE '../%'
            AND managed_relative_path NOT LIKE '%/..'
            AND managed_relative_path NOT LIKE '%\\%'
        )
    ),
    repository_kind TEXT NOT NULL DEFAULT 'none' CHECK (repository_kind IN ('git', 'none')),
    repository_identity TEXT CHECK (
        repository_identity IS NULL OR length(repository_identity) BETWEEN 1 AND 1024
    ),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    UNIQUE (id, project_id),
    CHECK (
        (kind = 'managed' AND managed_relative_path IS NOT NULL)
        OR (kind = 'external' AND managed_relative_path IS NULL)
    ),
    CHECK (
        (repository_kind = 'git' AND repository_identity IS NOT NULL)
        OR (repository_kind = 'none' AND repository_identity IS NULL)
    )
) STRICT;

INSERT INTO project_working_folders_next (
    id,
    project_id,
    display_name,
    kind,
    managed_relative_path,
    repository_kind,
    repository_identity,
    sort_order,
    created_at,
    updated_at,
    archived_at,
    revision
)
SELECT
    id,
    project_id,
    display_name,
    kind,
    managed_relative_path,
    repository_kind,
    repository_identity,
    sort_order,
    created_at,
    updated_at,
    archived_at,
    revision
FROM project_working_folders;

DROP TABLE project_working_folders;
ALTER TABLE project_working_folders_next RENAME TO project_working_folders;

CREATE INDEX idx_project_working_folders_project_active
ON project_working_folders(project_id, archived_at, sort_order, display_name COLLATE NOCASE, id);

CREATE TRIGGER project_working_folders_managed_relative_path_shape_insert
BEFORE INSERT ON project_working_folders
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER project_working_folders_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON project_working_folders
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER project_working_folders_protect_managed_insert
BEFORE INSERT ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.kind = 'managed'
    AND NEW.archived_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'Managed project working folders cannot be archived');
END;

CREATE TRIGGER project_working_folders_protect_managed_delete
BEFORE DELETE ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.kind = 'managed'
    AND EXISTS (SELECT 1 FROM projects WHERE id = OLD.project_id)
BEGIN
    SELECT RAISE(ABORT, 'Managed project working folders cannot be deleted');
END;

CREATE TRIGGER project_working_folders_protect_managed_update
BEFORE UPDATE OF project_id, kind, managed_relative_path, archived_at ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0 AND OLD.kind = 'managed' AND (
    NEW.project_id != OLD.project_id
    OR NEW.kind != 'managed'
    OR NEW.managed_relative_path != OLD.managed_relative_path
    OR NEW.archived_at IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Managed project working folders cannot change ownership or be archived');
END;

CREATE TEMP TABLE projects_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO projects_replication_guard (violations)
SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE projects_replication_guard;

-- The seeded routine priorities take their derived keys here, `derived_key("project-priority",
-- [project_id, id])`, so a fresh vault holds no placeholder. A vault whose conversion pass already
-- keyed them has no pending key left to match.
WITH seeded (project_id, id, sync_key) AS (
    VALUES
    ('project-routine-learning', 'urgent', 'project-priority:f33bb51498b2496a921ac398271a0a34'),
    ('project-routine-learning', 'high', 'project-priority:ca4078e78ecf5598d6a5b2339e31d8c9'),
    ('project-routine-learning', 'normal', 'project-priority:52c2222cd3e4b49a0d24910ecbfe2818'),
    ('project-routine-learning', 'low', 'project-priority:8ef26c5123737bac358a1b5b49f3eff5'),
    ('project-routine-reading', 'urgent', 'project-priority:398f5395d09d666dc3bfba8c262c4db6'),
    ('project-routine-reading', 'high', 'project-priority:6eafbe9e9ce897bcaa0f7c8f278d158d'),
    ('project-routine-reading', 'normal', 'project-priority:093a3e1903b74337397a278edc08b589'),
    ('project-routine-reading', 'low', 'project-priority:98aaac234ca9b7e1015cb4d8d5c419f4'),
    ('project-routine-exercise', 'urgent', 'project-priority:eaaaa358cb48a8606d74875f6375de1f'),
    ('project-routine-exercise', 'high', 'project-priority:1a87e12f376a9202c40e3e421736c320'),
    ('project-routine-exercise', 'normal', 'project-priority:8cd14ad077eef1113b3df6b624e9f69e'),
    ('project-routine-exercise', 'low', 'project-priority:201bfcabbf6747c9e1d066e97d71603f'),
    ('project-routine-hygiene', 'urgent', 'project-priority:c4a0d44693a09e5aab431e9b49c952fe'),
    ('project-routine-hygiene', 'high', 'project-priority:5d4eabd734ade7889336c8c2dc72f374'),
    ('project-routine-hygiene', 'normal', 'project-priority:b2848b821805d0d55e35ad1a8ce2e503'),
    ('project-routine-hygiene', 'low', 'project-priority:9a4ce8cc92c3d91e16f76a356edbcf5b'),
    ('project-routine-eat', 'urgent', 'project-priority:9ba61ca82ef4855d8e671b4c0dca6a73'),
    ('project-routine-eat', 'high', 'project-priority:eecc1e7dfb4d0f4c02aa8f7d3e3d6897'),
    ('project-routine-eat', 'normal', 'project-priority:512b229d13d0adaa93ae90a6635726ea'),
    ('project-routine-eat', 'low', 'project-priority:4b18722f3c3fb50dd2e34390f7842dde'),
    ('project-routine-commute', 'urgent', 'project-priority:255ee1809e9cd15e512678f87d104769'),
    ('project-routine-commute', 'high', 'project-priority:f14b672069a96123c485247146009615'),
    ('project-routine-commute', 'normal', 'project-priority:f2c890c34514d5189914e9f074bd19dd'),
    ('project-routine-commute', 'low', 'project-priority:eeda78e731cdf76df137d68bf2f64e9e'),
    ('project-routine-social', 'urgent', 'project-priority:434678b5f43422c5182740637c11fb1c'),
    ('project-routine-social', 'high', 'project-priority:0c7eade02b1d8b71d9b3842bea479c46'),
    ('project-routine-social', 'normal', 'project-priority:c8205bff62c5137b7d03d57845a7cc8d'),
    ('project-routine-social', 'low', 'project-priority:7474bbf5d5d5111f9e72ccf095f8fbc4'),
    ('project-routine-chores', 'urgent', 'project-priority:aae6c4cacbccdf6f008cb633933700e4'),
    ('project-routine-chores', 'high', 'project-priority:4b7bf097f562f179100b90bf085263de'),
    ('project-routine-chores', 'normal', 'project-priority:199b893f8c4be7a317425ed0ecad6575'),
    ('project-routine-chores', 'low', 'project-priority:b50fe97f6f0e98f85cfd9fb05ced332a'),
    ('project-routine-leisure', 'urgent', 'project-priority:ea509c26b64946e0265a8bffadd95a3a'),
    ('project-routine-leisure', 'high', 'project-priority:e63216ed8ab58d5bbb4ff68371200823'),
    ('project-routine-leisure', 'normal', 'project-priority:454c0b140ae74b8dc573cc9c29f96f84'),
    ('project-routine-leisure', 'low', 'project-priority:b2561bad5101115797ed48aedc371df0'),
    ('project-routine-meditate', 'urgent', 'project-priority:d0fdeddad7d6aafc9b24f0477d46dc36'),
    ('project-routine-meditate', 'high', 'project-priority:b988186f0ccf16b8ace84aac3536378e'),
    ('project-routine-meditate', 'normal', 'project-priority:5979c3569fec0330aa371655841d2dba'),
    ('project-routine-meditate', 'low', 'project-priority:92077c7fa621ada01b52f2d2d04838da'),
    ('project-routine-health', 'urgent', 'project-priority:bb66345ca55b2dd8a12a718cee8fba4c'),
    ('project-routine-health', 'high', 'project-priority:bc642107c9f3d0bb6c19b530a1d20299'),
    ('project-routine-health', 'normal', 'project-priority:c6e821000b5e9fc84ab1aa69892253f0'),
    ('project-routine-health', 'low', 'project-priority:4a215e6353395ba7d3b4c088e46389fe'),
    ('project-routine-sleep', 'urgent', 'project-priority:831e33cf10981f3e7b12a0650b5e6ade'),
    ('project-routine-sleep', 'high', 'project-priority:41a975d56e8f42b53e0fa610f66d4ca3'),
    ('project-routine-sleep', 'normal', 'project-priority:d4f7f71640e81c8a86e093d25efcf24e'),
    ('project-routine-sleep', 'low', 'project-priority:1b295e86e48db485c7ab469d764bae78')
)
UPDATE project_priorities
SET sync_key = seeded.sync_key
FROM seeded
WHERE project_priorities.project_id = seeded.project_id
    AND project_priorities.id = seeded.id
    AND project_priorities.sync_key GLOB 'pending:*';

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.
CREATE TRIGGER sync_values_project_groups_insert
BEFORE INSERT ON project_groups
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_groups_update
BEFORE UPDATE OF created_at, name, icon, icon_byte_size, color, order_key, hidden_at, archived_at, updated_at ON project_groups
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (NEW.icon_byte_size IS NULL OR (typeof(NEW.icon_byte_size) = 'integer' AND NEW.icon_byte_size BETWEEN 1 AND 3145728))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_groups
BEFORE UPDATE OF id, created_at ON project_groups
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_groups
BEFORE INSERT ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 96 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_groups WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_groups_insert
AFTER INSERT ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (96, NEW.id, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_groups_update
AFTER UPDATE ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 96, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY OR OLD.icon_byte_size IS NOT NEW.icon_byte_size
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.hidden_at IS NOT NEW.hidden_at COLLATE BINARY OR OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_groups_delete
AFTER DELETE ON project_groups
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (96, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
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

CREATE TRIGGER sync_values_project_working_folders_insert
BEFORE INSERT ON project_working_folders
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 32 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (NEW.managed_relative_path IS NULL OR (typeof(NEW.managed_relative_path) = 'text' AND instr(CAST(NEW.managed_relative_path AS BLOB), x'00') = 0 AND length(NEW.managed_relative_path) <= 2048 AND length(CAST(NEW.managed_relative_path AS BLOB)) <= 262144))
    AND (typeof(NEW.sort_order) = 'integer' AND NEW.sort_order BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 240 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.repository_kind) = 'text' AND instr(CAST(NEW.repository_kind AS BLOB), x'00') = 0 AND length(NEW.repository_kind) <= 32 AND length(CAST(NEW.repository_kind AS BLOB)) <= 262144)
    AND (NEW.repository_identity IS NULL OR (typeof(NEW.repository_identity) = 'text' AND instr(CAST(NEW.repository_identity AS BLOB), x'00') = 0 AND length(NEW.repository_identity) <= 1024 AND length(CAST(NEW.repository_identity AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_working_folders_update
BEFORE UPDATE OF project_id, kind, managed_relative_path, sort_order, created_at, display_name, repository_kind, repository_identity, archived_at, updated_at ON project_working_folders
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 32 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (NEW.managed_relative_path IS NULL OR (typeof(NEW.managed_relative_path) = 'text' AND instr(CAST(NEW.managed_relative_path AS BLOB), x'00') = 0 AND length(NEW.managed_relative_path) <= 2048 AND length(CAST(NEW.managed_relative_path AS BLOB)) <= 262144))
    AND (typeof(NEW.sort_order) = 'integer' AND NEW.sort_order BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 240 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.repository_kind) = 'text' AND instr(CAST(NEW.repository_kind AS BLOB), x'00') = 0 AND length(NEW.repository_kind) <= 32 AND length(CAST(NEW.repository_kind AS BLOB)) <= 262144)
    AND (NEW.repository_identity IS NULL OR (typeof(NEW.repository_identity) = 'text' AND instr(CAST(NEW.repository_identity AS BLOB), x'00') = 0 AND length(NEW.repository_identity) <= 1024 AND length(CAST(NEW.repository_identity AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_working_folders
BEFORE UPDATE OF id, project_id, kind, managed_relative_path, sort_order, created_at ON project_working_folders
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.kind IS NOT NEW.kind COLLATE BINARY OR OLD.managed_relative_path IS NOT NEW.managed_relative_path COLLATE BINARY OR OLD.sort_order IS NOT NEW.sort_order OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_working_folders
BEFORE INSERT ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 98 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_working_folders WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_working_folders_insert
AFTER INSERT ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (98, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_working_folders_update
AFTER UPDATE ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 98, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.repository_kind IS NOT NEW.repository_kind COLLATE BINARY OR OLD.repository_identity IS NOT NEW.repository_identity COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_working_folders_delete
AFTER DELETE ON project_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (98, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_primary_working_folders_insert
BEFORE INSERT ON project_primary_working_folders
WHEN ((typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND length(NEW.working_folder_id) <= 1024 AND length(CAST(NEW.working_folder_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_primary_working_folders_update
BEFORE UPDATE OF created_at, working_folder_id, updated_at ON project_primary_working_folders
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND length(NEW.working_folder_id) <= 1024 AND length(CAST(NEW.working_folder_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_primary_working_folders
BEFORE UPDATE OF project_id, created_at ON project_primary_working_folders
WHEN OLD.project_id IS NOT NEW.project_id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_primary_working_folders
BEFORE INSERT ON project_primary_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 99 AND row_key = NEW.project_id)
    AND NOT EXISTS (SELECT 1 FROM project_primary_working_folders WHERE project_id = NEW.project_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_primary_working_folders_insert
AFTER INSERT ON project_primary_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (99, NEW.project_id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_primary_working_folders_update
AFTER UPDATE ON project_primary_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 99, NEW.project_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_primary_working_folders_delete
AFTER DELETE ON project_primary_working_folders
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (99, OLD.project_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_sections_insert
BEFORE INSERT ON project_sections
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_sections_update
BEFORE UPDATE OF project_id, created_at, name, order_key, hidden_at, archived_at, updated_at ON project_sections
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.hidden_at IS NULL OR (typeof(NEW.hidden_at) = 'text' AND instr(CAST(NEW.hidden_at AS BLOB), x'00') = 0 AND trim(NEW.hidden_at) <> '' AND length(CAST(NEW.hidden_at AS BLOB)) <= 64))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_sections
BEFORE UPDATE OF id, project_id, created_at ON project_sections
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_sections
BEFORE INSERT ON project_sections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 100 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_sections WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_sections_insert
AFTER INSERT ON project_sections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (100, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_sections_update
AFTER UPDATE ON project_sections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 100, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.hidden_at IS NOT NEW.hidden_at COLLATE BINARY OR OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_sections_delete
AFTER DELETE ON project_sections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (100, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_statuses_insert
BEFORE INSERT ON project_statuses
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.category) = 'text' AND instr(CAST(NEW.category AS BLOB), x'00') = 0 AND length(NEW.category) <= 32 AND length(CAST(NEW.category AS BLOB)) <= 262144)
    AND (typeof(NEW.terminal) = 'integer' AND NEW.terminal IN (0, 1))
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_statuses_update
BEFORE UPDATE OF project_id, created_at, name, category, terminal, color, order_key, deleted_at, updated_at ON project_statuses
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.category) = 'text' AND instr(CAST(NEW.category AS BLOB), x'00') = 0 AND length(NEW.category) <= 32 AND length(CAST(NEW.category AS BLOB)) <= 262144)
    AND (typeof(NEW.terminal) = 'integer' AND NEW.terminal IN (0, 1))
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_statuses
BEFORE UPDATE OF id, project_id, created_at ON project_statuses
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_statuses
BEFORE INSERT ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 101 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_statuses WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_statuses_insert
AFTER INSERT ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (101, NEW.id, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_statuses_update
AFTER UPDATE ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 101, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.category IS NOT NEW.category COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.terminal IS NOT NEW.terminal
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 128 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_statuses_delete
AFTER DELETE ON project_statuses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (101, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_priorities_insert
BEFORE INSERT ON project_priorities
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 128 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_priorities_update
BEFORE UPDATE OF project_id, id, created_at, name, color, order_key, deleted_at, updated_at ON project_priorities
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND length(trim(NEW.id)) BETWEEN 1 AND 128 AND length(CAST(NEW.id AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_priorities
BEFORE UPDATE OF sync_key, project_id, id ON project_priorities
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.id IS NOT NEW.id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_priorities
BEFORE INSERT ON project_priorities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.sync_key NOT GLOB 'pending:*'
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 102 AND row_key = NEW.sync_key)
    AND NOT EXISTS (SELECT 1 FROM project_priorities WHERE sync_key = NEW.sync_key)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_priorities_insert
AFTER INSERT ON project_priorities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (102, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_priorities_update
AFTER UPDATE ON project_priorities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 102, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 102, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_priorities_delete
AFTER DELETE ON project_priorities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (102, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_tags_insert
BEFORE INSERT ON project_tags
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_tags_update
BEFORE UPDATE OF project_id, created_at, name, color, order_key, updated_at ON project_tags
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_tags
BEFORE UPDATE OF id, project_id, created_at ON project_tags
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_tags
BEFORE INSERT ON project_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 103 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_tags WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_tags_insert
AFTER INSERT ON project_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (103, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_tags_update
AFTER UPDATE ON project_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 103, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_tags_delete
AFTER DELETE ON project_tags
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (103, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_tasks_insert
BEFORE INSERT ON project_tasks
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.parent_task_id IS NULL OR (typeof(NEW.parent_task_id) = 'text' AND instr(CAST(NEW.parent_task_id AS BLOB), x'00') = 0 AND trim(NEW.parent_task_id) <> '' AND length(CAST(NEW.parent_task_id AS BLOB)) <= 128))
    AND (NEW.section_id IS NULL OR (typeof(NEW.section_id) = 'text' AND instr(CAST(NEW.section_id AS BLOB), x'00') = 0 AND trim(NEW.section_id) <> '' AND length(CAST(NEW.section_id AS BLOB)) <= 128))
    AND (typeof(NEW.section_order_key) = 'text' AND instr(CAST(NEW.section_order_key AS BLOB), x'00') = 0 AND NEW.section_order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.section_order_key) <= 128 AND (CASE WHEN unicode(NEW.section_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.section_order_key) - 95 WHEN unicode(NEW.section_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.section_order_key) ELSE 129 END) <= length(NEW.section_order_key) AND ((CASE WHEN unicode(NEW.section_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.section_order_key) - 95 WHEN unicode(NEW.section_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.section_order_key) ELSE 129 END) = length(NEW.section_order_key) OR substr(NEW.section_order_key, -1) <> '0') AND NEW.section_order_key <> 'A00000000000000000000000000')
    AND (NEW.status_id IS NULL OR (typeof(NEW.status_id) = 'text' AND instr(CAST(NEW.status_id AS BLOB), x'00') = 0 AND trim(NEW.status_id) <> '' AND length(CAST(NEW.status_id AS BLOB)) <= 128))
    AND (NEW.completed_at IS NULL OR (typeof(NEW.completed_at) = 'text' AND instr(CAST(NEW.completed_at AS BLOB), x'00') = 0 AND trim(NEW.completed_at) <> '' AND length(CAST(NEW.completed_at AS BLOB)) <= 64))
    AND (typeof(NEW.status_order_key) = 'text' AND instr(CAST(NEW.status_order_key AS BLOB), x'00') = 0 AND NEW.status_order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.status_order_key) <= 128 AND (CASE WHEN unicode(NEW.status_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.status_order_key) - 95 WHEN unicode(NEW.status_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.status_order_key) ELSE 129 END) <= length(NEW.status_order_key) AND ((CASE WHEN unicode(NEW.status_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.status_order_key) - 95 WHEN unicode(NEW.status_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.status_order_key) ELSE 129 END) = length(NEW.status_order_key) OR substr(NEW.status_order_key, -1) <> '0') AND NEW.status_order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(trim(NEW.title)) BETWEEN 1 AND 2000 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 65536 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.priority) = 'text' AND instr(CAST(NEW.priority AS BLOB), x'00') = 0 AND length(trim(NEW.priority)) BETWEEN 1 AND 128 AND length(CAST(NEW.priority AS BLOB)) <= 262144)
    AND (typeof(NEW.task_type) = 'text' AND instr(CAST(NEW.task_type AS BLOB), x'00') = 0 AND length(NEW.task_type) <= 32 AND length(CAST(NEW.task_type AS BLOB)) <= 262144)
    AND (typeof(NEW.milestone) = 'integer' AND NEW.milestone IN (0, 1))
    AND (NEW.estimate_minutes IS NULL OR (typeof(NEW.estimate_minutes) = 'integer' AND NEW.estimate_minutes BETWEEN 1 AND 9223372036854775807))
    AND (NEW.start_date IS NULL OR (typeof(NEW.start_date) = 'text' AND instr(CAST(NEW.start_date AS BLOB), x'00') = 0 AND length(NEW.start_date) <= 32 AND length(CAST(NEW.start_date AS BLOB)) <= 262144))
    AND (NEW.start_time IS NULL OR (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(NEW.start_time) <= 5 AND length(CAST(NEW.start_time AS BLOB)) <= 262144))
    AND (NEW.start_reminder_minutes IS NULL OR (typeof(NEW.start_reminder_minutes) = 'integer' AND NEW.start_reminder_minutes BETWEEN 0 AND 40320))
    AND (NEW.due_date IS NULL OR (typeof(NEW.due_date) = 'text' AND instr(CAST(NEW.due_date AS BLOB), x'00') = 0 AND length(NEW.due_date) <= 32 AND length(CAST(NEW.due_date AS BLOB)) <= 262144))
    AND (NEW.due_time IS NULL OR (typeof(NEW.due_time) = 'text' AND instr(CAST(NEW.due_time AS BLOB), x'00') = 0 AND length(NEW.due_time) <= 5 AND length(CAST(NEW.due_time AS BLOB)) <= 262144))
    AND (NEW.due_reminder_minutes IS NULL OR (typeof(NEW.due_reminder_minutes) = 'integer' AND NEW.due_reminder_minutes BETWEEN 0 AND 40320))
    AND (NEW.target_end_date IS NULL OR (typeof(NEW.target_end_date) = 'text' AND instr(CAST(NEW.target_end_date AS BLOB), x'00') = 0 AND length(NEW.target_end_date) <= 32 AND length(CAST(NEW.target_end_date AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (NEW.blocker_reason IS NULL OR (typeof(NEW.blocker_reason) = 'text' AND instr(CAST(NEW.blocker_reason AS BLOB), x'00') = 0 AND length(NEW.blocker_reason) <= 65536 AND length(CAST(NEW.blocker_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_tasks_update
BEFORE UPDATE OF project_id, created_at, parent_task_id, section_id, section_order_key, status_id, completed_at, status_order_key, title, description, priority, task_type, milestone, estimate_minutes, start_date, start_time, start_reminder_minutes, due_date, due_time, due_reminder_minutes, target_end_date, archived_at, blocker_reason, updated_at ON project_tasks
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.parent_task_id IS NULL OR (typeof(NEW.parent_task_id) = 'text' AND instr(CAST(NEW.parent_task_id AS BLOB), x'00') = 0 AND trim(NEW.parent_task_id) <> '' AND length(CAST(NEW.parent_task_id AS BLOB)) <= 128))
    AND (NEW.section_id IS NULL OR (typeof(NEW.section_id) = 'text' AND instr(CAST(NEW.section_id AS BLOB), x'00') = 0 AND trim(NEW.section_id) <> '' AND length(CAST(NEW.section_id AS BLOB)) <= 128))
    AND (typeof(NEW.section_order_key) = 'text' AND instr(CAST(NEW.section_order_key AS BLOB), x'00') = 0 AND NEW.section_order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.section_order_key) <= 128 AND (CASE WHEN unicode(NEW.section_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.section_order_key) - 95 WHEN unicode(NEW.section_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.section_order_key) ELSE 129 END) <= length(NEW.section_order_key) AND ((CASE WHEN unicode(NEW.section_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.section_order_key) - 95 WHEN unicode(NEW.section_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.section_order_key) ELSE 129 END) = length(NEW.section_order_key) OR substr(NEW.section_order_key, -1) <> '0') AND NEW.section_order_key <> 'A00000000000000000000000000')
    AND (NEW.status_id IS NULL OR (typeof(NEW.status_id) = 'text' AND instr(CAST(NEW.status_id AS BLOB), x'00') = 0 AND trim(NEW.status_id) <> '' AND length(CAST(NEW.status_id AS BLOB)) <= 128))
    AND (NEW.completed_at IS NULL OR (typeof(NEW.completed_at) = 'text' AND instr(CAST(NEW.completed_at AS BLOB), x'00') = 0 AND trim(NEW.completed_at) <> '' AND length(CAST(NEW.completed_at AS BLOB)) <= 64))
    AND (typeof(NEW.status_order_key) = 'text' AND instr(CAST(NEW.status_order_key AS BLOB), x'00') = 0 AND NEW.status_order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.status_order_key) <= 128 AND (CASE WHEN unicode(NEW.status_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.status_order_key) - 95 WHEN unicode(NEW.status_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.status_order_key) ELSE 129 END) <= length(NEW.status_order_key) AND ((CASE WHEN unicode(NEW.status_order_key) BETWEEN 97 AND 122 THEN unicode(NEW.status_order_key) - 95 WHEN unicode(NEW.status_order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.status_order_key) ELSE 129 END) = length(NEW.status_order_key) OR substr(NEW.status_order_key, -1) <> '0') AND NEW.status_order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(trim(NEW.title)) BETWEEN 1 AND 2000 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 65536 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.priority) = 'text' AND instr(CAST(NEW.priority AS BLOB), x'00') = 0 AND length(trim(NEW.priority)) BETWEEN 1 AND 128 AND length(CAST(NEW.priority AS BLOB)) <= 262144)
    AND (typeof(NEW.task_type) = 'text' AND instr(CAST(NEW.task_type AS BLOB), x'00') = 0 AND length(NEW.task_type) <= 32 AND length(CAST(NEW.task_type AS BLOB)) <= 262144)
    AND (typeof(NEW.milestone) = 'integer' AND NEW.milestone IN (0, 1))
    AND (NEW.estimate_minutes IS NULL OR (typeof(NEW.estimate_minutes) = 'integer' AND NEW.estimate_minutes BETWEEN 1 AND 9223372036854775807))
    AND (NEW.start_date IS NULL OR (typeof(NEW.start_date) = 'text' AND instr(CAST(NEW.start_date AS BLOB), x'00') = 0 AND length(NEW.start_date) <= 32 AND length(CAST(NEW.start_date AS BLOB)) <= 262144))
    AND (NEW.start_time IS NULL OR (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(NEW.start_time) <= 5 AND length(CAST(NEW.start_time AS BLOB)) <= 262144))
    AND (NEW.start_reminder_minutes IS NULL OR (typeof(NEW.start_reminder_minutes) = 'integer' AND NEW.start_reminder_minutes BETWEEN 0 AND 40320))
    AND (NEW.due_date IS NULL OR (typeof(NEW.due_date) = 'text' AND instr(CAST(NEW.due_date AS BLOB), x'00') = 0 AND length(NEW.due_date) <= 32 AND length(CAST(NEW.due_date AS BLOB)) <= 262144))
    AND (NEW.due_time IS NULL OR (typeof(NEW.due_time) = 'text' AND instr(CAST(NEW.due_time AS BLOB), x'00') = 0 AND length(NEW.due_time) <= 5 AND length(CAST(NEW.due_time AS BLOB)) <= 262144))
    AND (NEW.due_reminder_minutes IS NULL OR (typeof(NEW.due_reminder_minutes) = 'integer' AND NEW.due_reminder_minutes BETWEEN 0 AND 40320))
    AND (NEW.target_end_date IS NULL OR (typeof(NEW.target_end_date) = 'text' AND instr(CAST(NEW.target_end_date AS BLOB), x'00') = 0 AND length(NEW.target_end_date) <= 32 AND length(CAST(NEW.target_end_date AS BLOB)) <= 262144))
    AND (NEW.archived_at IS NULL OR (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64))
    AND (NEW.blocker_reason IS NULL OR (typeof(NEW.blocker_reason) = 'text' AND instr(CAST(NEW.blocker_reason AS BLOB), x'00') = 0 AND length(NEW.blocker_reason) <= 65536 AND length(CAST(NEW.blocker_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_tasks
BEFORE UPDATE OF id, project_id, created_at ON project_tasks
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_tasks
BEFORE INSERT ON project_tasks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 104 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_tasks WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_tasks_insert
AFTER INSERT ON project_tasks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (104, NEW.id, 8191, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_tasks_update
AFTER UPDATE ON project_tasks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 104, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.parent_task_id IS NOT NEW.parent_task_id COLLATE BINARY
                AND NOT (NEW.parent_task_id IS NULL AND NOT EXISTS (SELECT 1 FROM project_tasks WHERE id = OLD.parent_task_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN (OLD.section_id IS NOT NEW.section_id COLLATE BINARY
                AND NOT (NEW.section_id IS NULL AND NOT EXISTS (SELECT 1 FROM project_sections WHERE id = OLD.section_id))) OR OLD.section_order_key IS NOT NEW.section_order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN (OLD.status_id IS NOT NEW.status_id COLLATE BINARY
                AND NOT (NEW.status_id IS NULL AND NOT EXISTS (SELECT 1 FROM project_statuses WHERE id = OLD.status_id))) OR OLD.completed_at IS NOT NEW.completed_at COLLATE BINARY OR OLD.status_order_key IS NOT NEW.status_order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.description IS NOT NEW.description COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.priority IS NOT NEW.priority COLLATE BINARY
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.task_type IS NOT NEW.task_type COLLATE BINARY OR OLD.milestone IS NOT NEW.milestone
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.estimate_minutes IS NOT NEW.estimate_minutes
                THEN 256 ELSE 0 END)
            | (CASE WHEN OLD.start_date IS NOT NEW.start_date COLLATE BINARY OR OLD.start_time IS NOT NEW.start_time COLLATE BINARY OR OLD.start_reminder_minutes IS NOT NEW.start_reminder_minutes OR OLD.due_date IS NOT NEW.due_date COLLATE BINARY OR OLD.due_time IS NOT NEW.due_time COLLATE BINARY OR OLD.due_reminder_minutes IS NOT NEW.due_reminder_minutes OR OLD.target_end_date IS NOT NEW.target_end_date COLLATE BINARY
                THEN 512 ELSE 0 END)
            | (CASE WHEN OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY
                THEN 1024 ELSE 0 END)
            | (CASE WHEN OLD.blocker_reason IS NOT NEW.blocker_reason COLLATE BINARY
                THEN 2048 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4096 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_tasks_delete
AFTER DELETE ON project_tasks
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (104, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_checklist_items_insert
BEFORE INSERT ON project_checklist_items
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(trim(NEW.title)) BETWEEN 1 AND 2000 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (NEW.completed_at IS NULL OR (typeof(NEW.completed_at) = 'text' AND instr(CAST(NEW.completed_at AS BLOB), x'00') = 0 AND trim(NEW.completed_at) <> '' AND length(CAST(NEW.completed_at AS BLOB)) <= 64))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_checklist_items_update
BEFORE UPDATE OF task_id, created_at, title, completed_at, order_key, updated_at ON project_checklist_items
WHEN ((NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(trim(NEW.title)) BETWEEN 1 AND 2000 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (NEW.completed_at IS NULL OR (typeof(NEW.completed_at) = 'text' AND instr(CAST(NEW.completed_at AS BLOB), x'00') = 0 AND trim(NEW.completed_at) <> '' AND length(CAST(NEW.completed_at AS BLOB)) <= 64))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_checklist_items
BEFORE UPDATE OF id, task_id, created_at ON project_checklist_items
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.task_id IS NOT NEW.task_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_checklist_items
BEFORE INSERT ON project_checklist_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 105 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_checklist_items WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_checklist_items_insert
AFTER INSERT ON project_checklist_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (105, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_checklist_items_update
AFTER UPDATE ON project_checklist_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 105, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.completed_at IS NOT NEW.completed_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_checklist_items_delete
AFTER DELETE ON project_checklist_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (105, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_task_dependencies_insert
BEFORE INSERT ON project_task_dependencies
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.blocking_task_id IS NULL OR (typeof(NEW.blocking_task_id) = 'text' AND instr(CAST(NEW.blocking_task_id AS BLOB), x'00') = 0 AND trim(NEW.blocking_task_id) <> '' AND length(CAST(NEW.blocking_task_id AS BLOB)) <= 128))
    AND (NEW.blocked_task_id IS NULL OR (typeof(NEW.blocked_task_id) = 'text' AND instr(CAST(NEW.blocked_task_id AS BLOB), x'00') = 0 AND trim(NEW.blocked_task_id) <> '' AND length(CAST(NEW.blocked_task_id AS BLOB)) <= 128))
    AND (typeof(NEW.dependency_type) = 'text' AND instr(CAST(NEW.dependency_type AS BLOB), x'00') = 0 AND length(NEW.dependency_type) <= 32 AND length(CAST(NEW.dependency_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_task_dependencies_update
BEFORE UPDATE OF blocking_task_id, blocked_task_id, dependency_type, created_at ON project_task_dependencies
WHEN ((NEW.blocking_task_id IS NULL OR (typeof(NEW.blocking_task_id) = 'text' AND instr(CAST(NEW.blocking_task_id AS BLOB), x'00') = 0 AND trim(NEW.blocking_task_id) <> '' AND length(CAST(NEW.blocking_task_id AS BLOB)) <= 128))
    AND (NEW.blocked_task_id IS NULL OR (typeof(NEW.blocked_task_id) = 'text' AND instr(CAST(NEW.blocked_task_id AS BLOB), x'00') = 0 AND trim(NEW.blocked_task_id) <> '' AND length(CAST(NEW.blocked_task_id AS BLOB)) <= 128))
    AND (typeof(NEW.dependency_type) = 'text' AND instr(CAST(NEW.dependency_type AS BLOB), x'00') = 0 AND length(NEW.dependency_type) <= 32 AND length(CAST(NEW.dependency_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_task_dependencies
BEFORE UPDATE OF id, blocking_task_id, blocked_task_id ON project_task_dependencies
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.blocking_task_id IS NOT NEW.blocking_task_id COLLATE BINARY OR OLD.blocked_task_id IS NOT NEW.blocked_task_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_project_task_dependencies_insert
AFTER INSERT ON project_task_dependencies
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (106, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_task_dependencies_update
AFTER UPDATE ON project_task_dependencies
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 106, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.dependency_type IS NOT NEW.dependency_type COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_task_dependencies_delete
AFTER DELETE ON project_task_dependencies
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (106, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_task_tag_links_insert
BEFORE INSERT ON project_task_tag_links
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.tag_id IS NULL OR (typeof(NEW.tag_id) = 'text' AND instr(CAST(NEW.tag_id AS BLOB), x'00') = 0 AND trim(NEW.tag_id) <> '' AND length(CAST(NEW.tag_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_task_tag_links_update
BEFORE UPDATE OF task_id, tag_id, created_at ON project_task_tag_links
WHEN ((NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.tag_id IS NULL OR (typeof(NEW.tag_id) = 'text' AND instr(CAST(NEW.tag_id AS BLOB), x'00') = 0 AND trim(NEW.tag_id) <> '' AND length(CAST(NEW.tag_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_task_tag_links
BEFORE UPDATE OF sync_key, task_id, tag_id ON project_task_tag_links
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.task_id IS NOT NEW.task_id COLLATE BINARY OR OLD.tag_id IS NOT NEW.tag_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_project_task_tag_links_insert
AFTER INSERT ON project_task_tag_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (107, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_task_tag_links_update
AFTER UPDATE ON project_task_tag_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 107, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 107, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_task_tag_links_delete
AFTER DELETE ON project_task_tag_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (107, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_custom_fields_insert
BEFORE INSERT ON project_custom_fields
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.field_type) = 'text' AND instr(CAST(NEW.field_type AS BLOB), x'00') = 0 AND length(NEW.field_type) <= 32 AND length(CAST(NEW.field_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_custom_fields_update
BEFORE UPDATE OF project_id, field_type, created_at, name, icon, order_key, updated_at ON project_custom_fields
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.field_type) = 'text' AND instr(CAST(NEW.field_type AS BLOB), x'00') = 0 AND length(NEW.field_type) <= 32 AND length(CAST(NEW.field_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (NEW.icon IS NULL OR (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_custom_fields
BEFORE UPDATE OF id, project_id, field_type, created_at ON project_custom_fields
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.field_type IS NOT NEW.field_type COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_custom_fields
BEFORE INSERT ON project_custom_fields
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 108 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_custom_fields WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_custom_fields_insert
AFTER INSERT ON project_custom_fields
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (108, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_custom_fields_update
AFTER UPDATE ON project_custom_fields
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 108, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_custom_fields_delete
AFTER DELETE ON project_custom_fields
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (108, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_custom_field_options_insert
BEFORE INSERT ON project_custom_field_options
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.field_id IS NULL OR (typeof(NEW.field_id) = 'text' AND instr(CAST(NEW.field_id AS BLOB), x'00') = 0 AND trim(NEW.field_id) <> '' AND length(CAST(NEW.field_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_custom_field_options_update
BEFORE UPDATE OF field_id, created_at, name, order_key, updated_at ON project_custom_field_options
WHEN ((NEW.field_id IS NULL OR (typeof(NEW.field_id) = 'text' AND instr(CAST(NEW.field_id AS BLOB), x'00') = 0 AND trim(NEW.field_id) <> '' AND length(CAST(NEW.field_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_custom_field_options
BEFORE UPDATE OF id, field_id, created_at ON project_custom_field_options
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.field_id IS NOT NEW.field_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_custom_field_options
BEFORE INSERT ON project_custom_field_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 109 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_custom_field_options WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_custom_field_options_insert
AFTER INSERT ON project_custom_field_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (109, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_custom_field_options_update
AFTER UPDATE ON project_custom_field_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 109, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_custom_field_options_delete
AFTER DELETE ON project_custom_field_options
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (109, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_custom_field_values_insert
BEFORE INSERT ON project_custom_field_values
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.field_id IS NULL OR (typeof(NEW.field_id) = 'text' AND instr(CAST(NEW.field_id AS BLOB), x'00') = 0 AND trim(NEW.field_id) <> '' AND length(CAST(NEW.field_id AS BLOB)) <= 128))
    AND (NEW.text_value IS NULL OR (typeof(NEW.text_value) = 'text' AND instr(CAST(NEW.text_value AS BLOB), x'00') = 0 AND length(NEW.text_value) <= 65536 AND length(CAST(NEW.text_value AS BLOB)) <= 262144))
    AND (NEW.number_value IS NULL OR (typeof(NEW.number_value) = 'integer' AND NEW.number_value BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.date_value IS NULL OR (typeof(NEW.date_value) = 'text' AND instr(CAST(NEW.date_value AS BLOB), x'00') = 0 AND length(NEW.date_value) <= 32 AND length(CAST(NEW.date_value AS BLOB)) <= 262144))
    AND (NEW.checkbox_value IS NULL OR (typeof(NEW.checkbox_value) = 'integer' AND NEW.checkbox_value BETWEEN 0 AND 1))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_custom_field_values_update
BEFORE UPDATE OF task_id, field_id, text_value, number_value, date_value, checkbox_value, updated_at ON project_custom_field_values
WHEN ((NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.field_id IS NULL OR (typeof(NEW.field_id) = 'text' AND instr(CAST(NEW.field_id AS BLOB), x'00') = 0 AND trim(NEW.field_id) <> '' AND length(CAST(NEW.field_id AS BLOB)) <= 128))
    AND (NEW.text_value IS NULL OR (typeof(NEW.text_value) = 'text' AND instr(CAST(NEW.text_value AS BLOB), x'00') = 0 AND length(NEW.text_value) <= 65536 AND length(CAST(NEW.text_value AS BLOB)) <= 262144))
    AND (NEW.number_value IS NULL OR (typeof(NEW.number_value) = 'integer' AND NEW.number_value BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.date_value IS NULL OR (typeof(NEW.date_value) = 'text' AND instr(CAST(NEW.date_value AS BLOB), x'00') = 0 AND length(NEW.date_value) <= 32 AND length(CAST(NEW.date_value AS BLOB)) <= 262144))
    AND (NEW.checkbox_value IS NULL OR (typeof(NEW.checkbox_value) = 'integer' AND NEW.checkbox_value BETWEEN 0 AND 1))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_custom_field_values
BEFORE UPDATE OF sync_key, task_id, field_id ON project_custom_field_values
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.task_id IS NOT NEW.task_id COLLATE BINARY OR OLD.field_id IS NOT NEW.field_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_project_custom_field_values_insert
AFTER INSERT ON project_custom_field_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (110, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_custom_field_values_update
AFTER UPDATE ON project_custom_field_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 110, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.text_value IS NOT NEW.text_value COLLATE BINARY OR OLD.number_value IS NOT NEW.number_value OR OLD.date_value IS NOT NEW.date_value COLLATE BINARY OR OLD.checkbox_value IS NOT NEW.checkbox_value
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 110, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_custom_field_values_delete
AFTER DELETE ON project_custom_field_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (110, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_custom_field_option_values_insert
BEFORE INSERT ON project_custom_field_option_values
WHEN ((NEW.option_id IS NULL OR (typeof(NEW.option_id) = 'text' AND instr(CAST(NEW.option_id AS BLOB), x'00') = 0 AND trim(NEW.option_id) <> '' AND length(CAST(NEW.option_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_custom_field_option_values_update
BEFORE UPDATE OF option_id ON project_custom_field_option_values
WHEN ((NEW.option_id IS NULL OR (typeof(NEW.option_id) = 'text' AND instr(CAST(NEW.option_id AS BLOB), x'00') = 0 AND trim(NEW.option_id) <> '' AND length(CAST(NEW.option_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_project_custom_field_option_values_insert
AFTER INSERT ON project_custom_field_option_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (110, NEW.value_key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_custom_field_option_values_update
AFTER UPDATE ON project_custom_field_option_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 110, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.value_key AS key UNION SELECT OLD.value_key) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_custom_field_option_values_delete
AFTER DELETE ON project_custom_field_option_values
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (OLD.option_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM project_custom_field_options WHERE id = OLD.option_id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (110, OLD.value_key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_project_task_change_events_insert
BEFORE INSERT ON project_task_change_events
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (typeof(NEW.event_type) = 'text' AND instr(CAST(NEW.event_type AS BLOB), x'00') = 0 AND length(NEW.event_type) <= 32 AND length(CAST(NEW.event_type AS BLOB)) <= 262144)
    AND (NEW.field_name IS NULL OR (typeof(NEW.field_name) = 'text' AND instr(CAST(NEW.field_name AS BLOB), x'00') = 0 AND length(NEW.field_name) <= 32 AND length(CAST(NEW.field_name AS BLOB)) <= 262144))
    AND (NEW.old_value IS NULL OR (typeof(NEW.old_value) = 'text' AND instr(CAST(NEW.old_value AS BLOB), x'00') = 0 AND length(NEW.old_value) <= 65536 AND length(CAST(NEW.old_value AS BLOB)) <= 262144))
    AND (NEW.new_value IS NULL OR (typeof(NEW.new_value) = 'text' AND instr(CAST(NEW.new_value AS BLOB), x'00') = 0 AND length(NEW.new_value) <= 65536 AND length(CAST(NEW.new_value AS BLOB)) <= 262144))
    AND (NEW.reason IS NULL OR (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 65536 AND length(CAST(NEW.reason AS BLOB)) <= 262144))
    AND (typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_task_change_events_update
BEFORE UPDATE OF task_id, event_type, field_name, old_value, new_value, reason, occurred_at ON project_task_change_events
WHEN ((NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (typeof(NEW.event_type) = 'text' AND instr(CAST(NEW.event_type AS BLOB), x'00') = 0 AND length(NEW.event_type) <= 32 AND length(CAST(NEW.event_type AS BLOB)) <= 262144)
    AND (NEW.field_name IS NULL OR (typeof(NEW.field_name) = 'text' AND instr(CAST(NEW.field_name AS BLOB), x'00') = 0 AND length(NEW.field_name) <= 32 AND length(CAST(NEW.field_name AS BLOB)) <= 262144))
    AND (NEW.old_value IS NULL OR (typeof(NEW.old_value) = 'text' AND instr(CAST(NEW.old_value AS BLOB), x'00') = 0 AND length(NEW.old_value) <= 65536 AND length(CAST(NEW.old_value AS BLOB)) <= 262144))
    AND (NEW.new_value IS NULL OR (typeof(NEW.new_value) = 'text' AND instr(CAST(NEW.new_value AS BLOB), x'00') = 0 AND length(NEW.new_value) <= 65536 AND length(CAST(NEW.new_value AS BLOB)) <= 262144))
    AND (NEW.reason IS NULL OR (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 65536 AND length(CAST(NEW.reason AS BLOB)) <= 262144))
    AND (typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_task_change_events
BEFORE UPDATE OF id, task_id, event_type, field_name, old_value, new_value, reason, occurred_at ON project_task_change_events
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.task_id IS NOT NEW.task_id COLLATE BINARY OR OLD.event_type IS NOT NEW.event_type COLLATE BINARY OR OLD.field_name IS NOT NEW.field_name COLLATE BINARY OR OLD.old_value IS NOT NEW.old_value COLLATE BINARY OR OLD.new_value IS NOT NEW.new_value COLLATE BINARY OR OLD.reason IS NOT NEW.reason COLLATE BINARY OR OLD.occurred_at IS NOT NEW.occurred_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_task_change_events
BEFORE INSERT ON project_task_change_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 111 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_task_change_events WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_task_change_events_insert
AFTER INSERT ON project_task_change_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (111, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_task_change_events_update
AFTER UPDATE ON project_task_change_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 111, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_task_change_events_delete
AFTER DELETE ON project_task_change_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (111, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_views_insert
BEFORE INSERT ON project_views
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.view_type) = 'text' AND instr(CAST(NEW.view_type AS BLOB), x'00') = 0 AND length(trim(NEW.view_type)) BETWEEN 1 AND 32 AND length(CAST(NEW.view_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.name IS NULL OR (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_views_update
BEFORE UPDATE OF project_id, view_type, created_at, name, order_key, updated_at ON project_views
WHEN ((NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.view_type) = 'text' AND instr(CAST(NEW.view_type AS BLOB), x'00') = 0 AND length(trim(NEW.view_type)) BETWEEN 1 AND 32 AND length(CAST(NEW.view_type AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.name IS NULL OR (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(NEW.name) <= 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_views
BEFORE UPDATE OF id, project_id, view_type, created_at ON project_views
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.view_type IS NOT NEW.view_type COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_views
BEFORE INSERT ON project_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 112 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_views WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_views_insert
AFTER INSERT ON project_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (112, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_views_update
AFTER UPDATE ON project_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 112, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_views_delete
AFTER DELETE ON project_views
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (112, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_view_preferences_insert
BEFORE INSERT ON project_view_preferences
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.view_id IS NULL OR (typeof(NEW.view_id) = 'text' AND instr(CAST(NEW.view_id AS BLOB), x'00') = 0 AND trim(NEW.view_id) <> '' AND length(CAST(NEW.view_id AS BLOB)) <= 128))
    AND (typeof(NEW.preference_key) = 'text' AND instr(CAST(NEW.preference_key AS BLOB), x'00') = 0 AND length(trim(NEW.preference_key)) BETWEEN 1 AND 32 AND length(CAST(NEW.preference_key AS BLOB)) <= 262144)
    AND (typeof(NEW.preference_value) = 'text' AND instr(CAST(NEW.preference_value AS BLOB), x'00') = 0 AND length(NEW.preference_value) <= 20000 AND length(CAST(NEW.preference_value AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_view_preferences_update
BEFORE UPDATE OF view_id, preference_key, preference_value, updated_at ON project_view_preferences
WHEN ((NEW.view_id IS NULL OR (typeof(NEW.view_id) = 'text' AND instr(CAST(NEW.view_id AS BLOB), x'00') = 0 AND trim(NEW.view_id) <> '' AND length(CAST(NEW.view_id AS BLOB)) <= 128))
    AND (typeof(NEW.preference_key) = 'text' AND instr(CAST(NEW.preference_key AS BLOB), x'00') = 0 AND length(trim(NEW.preference_key)) BETWEEN 1 AND 32 AND length(CAST(NEW.preference_key AS BLOB)) <= 262144)
    AND (typeof(NEW.preference_value) = 'text' AND instr(CAST(NEW.preference_value AS BLOB), x'00') = 0 AND length(NEW.preference_value) <= 20000 AND length(CAST(NEW.preference_value AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_view_preferences
BEFORE UPDATE OF sync_key, view_id, preference_key ON project_view_preferences
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.view_id IS NOT NEW.view_id COLLATE BINARY OR OLD.preference_key IS NOT NEW.preference_key COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_project_view_preferences_insert
AFTER INSERT ON project_view_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (113, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_view_preferences_update
AFTER UPDATE ON project_view_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 113, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.preference_value IS NOT NEW.preference_value COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 113, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_view_preferences_delete
AFTER DELETE ON project_view_preferences
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (113, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_custom_emojis_insert
BEFORE INSERT ON project_custom_emojis
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.asset_path) = 'text' AND instr(CAST(NEW.asset_path AS BLOB), x'00') = 0 AND length(trim(NEW.asset_path)) BETWEEN 1 AND 500 AND length(CAST(NEW.asset_path AS BLOB)) <= 262144)
    AND (NEW.asset_byte_size IS NULL OR (typeof(NEW.asset_byte_size) = 'integer' AND NEW.asset_byte_size BETWEEN 1 AND 3145728))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_custom_emojis_update
BEFORE UPDATE OF created_at, name, asset_path, asset_byte_size, order_key, updated_at ON project_custom_emojis
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 1000 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.asset_path) = 'text' AND instr(CAST(NEW.asset_path AS BLOB), x'00') = 0 AND length(trim(NEW.asset_path)) BETWEEN 1 AND 500 AND length(CAST(NEW.asset_path AS BLOB)) <= 262144)
    AND (NEW.asset_byte_size IS NULL OR (typeof(NEW.asset_byte_size) = 'integer' AND NEW.asset_byte_size BETWEEN 1 AND 3145728))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_custom_emojis
BEFORE UPDATE OF id, created_at ON project_custom_emojis
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_project_custom_emojis
BEFORE INSERT ON project_custom_emojis
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 114 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM project_custom_emojis WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_project_custom_emojis_insert
AFTER INSERT ON project_custom_emojis
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (114, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_custom_emojis_update
AFTER UPDATE ON project_custom_emojis
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 114, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.asset_path IS NOT NEW.asset_path COLLATE BINARY OR OLD.asset_byte_size IS NOT NEW.asset_byte_size
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_project_custom_emojis_delete
AFTER DELETE ON project_custom_emojis
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (114, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

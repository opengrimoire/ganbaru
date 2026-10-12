-- Fresh-start schema created before Ganbaru AI had external users.
-- Earlier development databases are intentionally unsupported and must be recreated.
--
-- Layout: one section per domain. Parent tables come before their children, each table is
-- followed by its indexes, and each section ends with its triggers. Seed data comes last.
--
-- Conventions: TEXT ids, TEXT RFC 3339 UTC timestamps named *_at, INTEGER Unix epoch milliseconds named *_ms,
-- INTEGER 0/1 booleans, and idx_<table>_<purpose> index names. Foreign keys that protect history use
-- ON DELETE NO ACTION instead of RESTRICT, so a cascade that removes both rows in one statement
-- succeeds regardless of the order SQLite visits the child tables.


-- Projects: Project groups, projects, tasks, custom fields, working folders, and idempotency receipts.

CREATE TABLE project_groups (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL DEFAULT 'folder' CHECK (trim(icon) <> ''),
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    collapsed INTEGER NOT NULL DEFAULT 0 CHECK (collapsed IN (0, 1)),
    hidden_at TEXT,
    archived_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_project_groups_sort ON project_groups(sort_order, name);

CREATE TABLE projects (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    group_id TEXT NOT NULL REFERENCES project_groups(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL DEFAULT 'folder' CHECK (trim(icon) <> ''),
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
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
CREATE INDEX idx_projects_group_sort ON projects(group_id, status, sort_order, name);
CREATE INDEX idx_projects_status ON projects(status);

CREATE TABLE project_view_preferences (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    view_id TEXT NOT NULL CHECK (view_id IN ('dashboard', 'list', 'kanban', 'calendar', 'gantt')),
    preference_key TEXT NOT NULL CHECK (trim(preference_key) <> ''),
    preference_value TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (project_id, view_id, preference_key)
);

CREATE TABLE project_dependency_cascade_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    request_json TEXT NOT NULL,
    response_json TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_project_dependency_cascade_receipts_project
    ON project_dependency_cascade_receipts(project_id);

CREATE TABLE project_reorder_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    request_json TEXT NOT NULL,
    response_json TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_project_reorder_receipts_project ON project_reorder_receipts(project_id);

CREATE TABLE project_task_bulk_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    request_json TEXT NOT NULL,
    response_json TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_project_task_bulk_receipts_project ON project_task_bulk_receipts(project_id);

CREATE TABLE project_sections (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    collapsed INTEGER NOT NULL DEFAULT 0 CHECK (collapsed IN (0, 1)),
    hidden_at TEXT,
    archived_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_project_sections_project_sort ON project_sections(project_id, sort_order, name);

CREATE TABLE project_statuses (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    category TEXT NOT NULL CHECK (category IN ('not_started', 'active', 'blocked', 'done')),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    terminal INTEGER NOT NULL DEFAULT 0 CHECK (terminal IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    color INTEGER NOT NULL DEFAULT 30 CHECK (color >= 0 AND color < 32)
);
CREATE INDEX idx_project_statuses_project_sort ON project_statuses(project_id, sort_order, name);

CREATE TABLE project_priorities (
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    color INTEGER NOT NULL CHECK (color >= 0 AND color < 32),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (project_id, id)
);
CREATE INDEX idx_project_priorities_project_sort ON project_priorities(project_id, sort_order, name);

CREATE TABLE project_tags (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE UNIQUE INDEX idx_project_tags_project_name ON project_tags(project_id, lower(name));
CREATE INDEX idx_project_tags_project_sort ON project_tags(project_id, sort_order, name);

CREATE TABLE project_tasks (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    section_id TEXT NOT NULL REFERENCES project_sections(id) ON DELETE NO ACTION,
    status_id TEXT NOT NULL REFERENCES project_statuses(id) ON DELETE NO ACTION,
    parent_task_id TEXT REFERENCES project_tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    description TEXT NOT NULL DEFAULT '',
    priority TEXT NOT NULL DEFAULT 'normal' CHECK (trim(priority) <> ''),
    task_type TEXT NOT NULL DEFAULT 'task' CHECK (task_type IN ('task', 'milestone', 'bug', 'habit')),
    section_sort_order REAL NOT NULL DEFAULT 0,
    status_sort_order REAL NOT NULL DEFAULT 0,
    estimate_minutes INTEGER CHECK (estimate_minutes IS NULL OR estimate_minutes > 0),
    due_date TEXT,
    start_date TEXT,
    target_end_date TEXT,
    completed_at TEXT,
    archived_at TEXT,
    blocker_reason TEXT,
    milestone INTEGER NOT NULL DEFAULT 0 CHECK (milestone IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    start_time TEXT CHECK (
        start_time IS NULL OR (
            start_date IS NOT NULL
            AND length(start_time) = 5
            AND start_time GLOB '[0-9][0-9]:[0-9][0-9]'
            AND substr(start_time, 3, 1) = ':'
            AND substr(start_time, 1, 2) BETWEEN '00' AND '23'
            AND substr(start_time, 4, 2) BETWEEN '00' AND '59'
        )
    ),
    due_time TEXT CHECK (
        due_time IS NULL OR (
            due_date IS NOT NULL
            AND length(due_time) = 5
            AND due_time GLOB '[0-9][0-9]:[0-9][0-9]'
            AND substr(due_time, 3, 1) = ':'
            AND substr(due_time, 1, 2) BETWEEN '00' AND '23'
            AND substr(due_time, 4, 2) BETWEEN '00' AND '59'
        )
    ),
    revision INTEGER NOT NULL DEFAULT 0
        CHECK (revision >= 0 AND revision <= 9007199254740991),
    CHECK (parent_task_id IS NULL OR parent_task_id <> id),
    CHECK (start_date IS NULL OR target_end_date IS NULL OR start_date <= target_end_date)
);
CREATE INDEX idx_project_tasks_archived ON project_tasks(project_id, archived_at);
CREATE INDEX idx_project_tasks_due ON project_tasks(project_id, due_date);
CREATE INDEX idx_project_tasks_parent ON project_tasks(parent_task_id, section_sort_order);
CREATE INDEX idx_project_tasks_project_section ON project_tasks(project_id, section_id, section_sort_order);
CREATE INDEX idx_project_tasks_project_status ON project_tasks(project_id, status_id, status_sort_order);

CREATE TABLE project_checklist_items (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (trim(title) <> ''),
    completed_at TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_project_checklist_items_task ON project_checklist_items(task_id, sort_order);

CREATE TABLE project_task_change_events (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (
        event_type IN (
            'created',
            'updated',
            'status_changed',
            'scheduled',
            'completed',
            'reopened',
            'archived',
            'event_unlinked',
            'dependency_added',
            'dependency_removed'
        )
    ),
    field_name TEXT,
    old_value TEXT,
    new_value TEXT,
    reason TEXT,
    occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(occurred_at) <> '')
);
CREATE INDEX idx_project_task_change_events_task ON project_task_change_events(task_id, occurred_at);

CREATE TABLE project_task_dependencies (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    blocking_task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    blocked_task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    dependency_type TEXT NOT NULL DEFAULT 'blocks' CHECK (dependency_type IN ('blocks')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    CHECK (blocking_task_id <> blocked_task_id)
);
CREATE INDEX idx_project_task_dependencies_blocked ON project_task_dependencies(blocked_task_id);
CREATE UNIQUE INDEX idx_project_task_dependencies_unique
    ON project_task_dependencies(blocking_task_id, blocked_task_id, dependency_type);

CREATE TABLE project_task_event_links (
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    link_kind TEXT NOT NULL DEFAULT 'scheduled' CHECK (link_kind IN ('scheduled', 'reference')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    PRIMARY KEY (task_id, event_id)
);
CREATE INDEX idx_project_task_event_links_event ON project_task_event_links(event_id);

CREATE TABLE project_task_tag_links (
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES "project_tags"(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    PRIMARY KEY (task_id, tag_id)
);
CREATE INDEX idx_project_task_tag_links_tag ON project_task_tag_links(tag_id);

CREATE TABLE project_custom_fields (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    field_type TEXT NOT NULL CHECK (
        field_type IN (
            'text',
            'number',
            'select',
            'multi_select',
            'status',
            'date',
            'person',
            'files',
            'checkbox',
            'url',
            'phone',
            'email'
        )
    ),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    revision INTEGER NOT NULL DEFAULT 0
        CHECK (revision BETWEEN 0 AND 9007199254740991)
);
CREATE UNIQUE INDEX idx_project_custom_fields_project_name ON project_custom_fields(project_id, lower(name));
CREATE INDEX idx_project_custom_fields_project_sort ON project_custom_fields(project_id, sort_order, name);

CREATE TABLE project_custom_field_options (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    field_id TEXT NOT NULL REFERENCES project_custom_fields(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    revision INTEGER NOT NULL DEFAULT 0
        CHECK (revision BETWEEN 0 AND 9007199254740991),
    UNIQUE (field_id, id)
);
CREATE UNIQUE INDEX idx_project_custom_field_options_field_name ON project_custom_field_options(field_id, lower(name));
CREATE INDEX idx_project_custom_field_options_field_sort ON project_custom_field_options(field_id, sort_order, name);

CREATE TABLE project_custom_field_option_values (
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL REFERENCES project_custom_fields(id) ON DELETE CASCADE,
    option_id TEXT NOT NULL REFERENCES project_custom_field_options(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    PRIMARY KEY (task_id, field_id, option_id),
    FOREIGN KEY (field_id, option_id) REFERENCES project_custom_field_options(field_id, id) ON DELETE CASCADE
);
CREATE INDEX idx_project_custom_field_option_values_field ON project_custom_field_option_values(field_id, option_id);

CREATE TABLE project_custom_field_values (
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL REFERENCES project_custom_fields(id) ON DELETE CASCADE,
    text_value TEXT,
    number_value REAL,
    date_value TEXT,
    checkbox_value INTEGER CHECK (checkbox_value IS NULL OR checkbox_value IN (0, 1)),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (task_id, field_id),
    CHECK (
        (text_value IS NOT NULL AND number_value IS NULL AND date_value IS NULL AND checkbox_value IS NULL) OR
        (text_value IS NULL AND number_value IS NOT NULL AND date_value IS NULL AND checkbox_value IS NULL) OR
        (text_value IS NULL AND number_value IS NULL AND date_value IS NOT NULL AND checkbox_value IS NULL) OR
        (text_value IS NULL AND number_value IS NULL AND date_value IS NULL AND checkbox_value IS NOT NULL)
    )
);
CREATE INDEX idx_project_custom_field_values_date ON project_custom_field_values(field_id, date_value);
CREATE INDEX idx_project_custom_field_values_field ON project_custom_field_values(field_id);
CREATE INDEX idx_project_custom_field_values_number ON project_custom_field_values(field_id, number_value);

CREATE TABLE project_working_folders (
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
        (kind = 'managed' AND managed_relative_path IS NOT NULL AND archived_at IS NULL)
        OR (kind = 'external' AND managed_relative_path IS NULL)
    ),
    CHECK (
        (repository_kind = 'git' AND repository_identity IS NOT NULL)
        OR (repository_kind = 'none' AND repository_identity IS NULL)
    )
) STRICT;
CREATE UNIQUE INDEX idx_project_working_folders_managed
ON project_working_folders(project_id)
WHERE kind = 'managed';
CREATE INDEX idx_project_working_folders_project_active
ON project_working_folders(project_id, archived_at, sort_order, display_name COLLATE NOCASE, id);

CREATE TABLE project_primary_working_folders (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE NO ACTION
) STRICT;

CREATE TABLE project_custom_emojis (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    asset_path TEXT NOT NULL CHECK (
        trim(asset_path) <> ''
        AND asset_path GLOB 'project-icons/*'
        AND instr(substr(asset_path, length('project-icons/') + 1), '/') = 0
        AND instr(asset_path, '..') = 0
        AND instr(asset_path, '\') = 0
    ),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_project_custom_emojis_sort ON project_custom_emojis(sort_order, name);

-- Projects triggers.

CREATE TRIGGER project_task_revision_after_update
AFTER UPDATE ON project_tasks
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE project_tasks SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

CREATE TRIGGER project_custom_field_revision AFTER UPDATE ON project_custom_fields
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE project_custom_fields SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

CREATE TRIGGER project_custom_field_option_revision AFTER UPDATE ON project_custom_field_options
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE project_custom_field_options SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

CREATE TRIGGER chat_channels_create_project_general
AFTER INSERT ON project_working_folders
WHEN NEW.kind = 'managed' AND NOT EXISTS (
    SELECT 1 FROM chat_channels WHERE project_id = NEW.project_id AND is_default = 1
)
BEGIN
    INSERT INTO chat_conversations (
        id, project_id, conversation_kind, last_activity_at, created_at, updated_at
    ) VALUES (
        'conversation:' || lower(hex(randomblob(16))),
        NEW.project_id,
        'channel',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );

    INSERT INTO chat_channels (
        id, project_id, conversation_id, name, topic, is_default, created_at, updated_at
    ) VALUES (
        'channel:' || lower(hex(randomblob(16))),
        NEW.project_id,
        (SELECT id FROM chat_conversations
         WHERE project_id = NEW.project_id AND conversation_kind = 'channel'
         ORDER BY rowid DESC LIMIT 1),
        'general',
        '',
        1,
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );

    INSERT INTO chat_conversation_memberships (
        conversation_id, participant_id, membership_role, created_at, updated_at
    ) VALUES (
        (SELECT conversation_id FROM chat_channels
         WHERE project_id = NEW.project_id AND is_default = 1),
        'participant:local-owner',
        'owner',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );

    INSERT INTO project_primary_working_folders (
        project_id, working_folder_id, created_at, updated_at
    ) VALUES (
        NEW.project_id,
        NEW.id,
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );
END;

CREATE TRIGGER project_primary_working_folder_prevent_archive
BEFORE UPDATE OF archived_at ON project_working_folders
WHEN NEW.archived_at IS NOT NULL AND EXISTS (
    SELECT 1 FROM project_primary_working_folders primary_folder
    WHERE primary_folder.working_folder_id = OLD.id
)
BEGIN
    SELECT RAISE(ABORT, 'Promote another working folder before archiving the primary folder');
END;

CREATE TRIGGER project_working_folders_create_chat_environment
AFTER INSERT ON project_working_folders
BEGIN
    INSERT INTO chat_execution_environments (
        id, working_folder_id, kind, display_name, repository_identity,
        lifecycle_state, created_at, updated_at
    ) VALUES (
        'current-folder:' || NEW.id,
        NEW.id,
        'current_folder',
        NEW.display_name,
        NEW.repository_identity,
        'available',
        NEW.created_at,
        NEW.updated_at
    );
END;

CREATE TRIGGER project_working_folders_protect_managed_delete
BEFORE DELETE ON project_working_folders
WHEN OLD.kind = 'managed' AND EXISTS (SELECT 1 FROM projects WHERE id = OLD.project_id)
BEGIN
    SELECT RAISE(ABORT, 'Managed project working folders cannot be deleted');
END;

CREATE TRIGGER project_working_folders_protect_managed_update
BEFORE UPDATE OF project_id, kind, managed_relative_path, archived_at ON project_working_folders
WHEN OLD.kind = 'managed' AND (
    NEW.project_id != OLD.project_id
    OR NEW.kind != 'managed'
    OR NEW.managed_relative_path != OLD.managed_relative_path
    OR NEW.archived_at IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Managed project working folders cannot change ownership or be archived');
END;

CREATE TRIGGER project_primary_working_folder_active_insert
BEFORE INSERT ON project_primary_working_folders
WHEN NOT EXISTS (
    SELECT 1 FROM project_working_folders folder
    WHERE folder.id = NEW.working_folder_id
      AND folder.project_id = NEW.project_id
      AND folder.archived_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Primary working folder must be active');
END;

CREATE TRIGGER project_primary_working_folder_active_update
BEFORE UPDATE OF working_folder_id, project_id ON project_primary_working_folders
WHEN NOT EXISTS (
    SELECT 1 FROM project_working_folders folder
    WHERE folder.id = NEW.working_folder_id
      AND folder.project_id = NEW.project_id
      AND folder.archived_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Primary working folder must be active');
END;


-- Calendar: Calendars, preserved iCalendar source objects, events with their per-event Pomodoro configuration, and the event archive used by Undo and history.

CREATE TABLE calendars (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    color TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'local' CHECK (source IN ('local', 'ics')),
    visible INTEGER NOT NULL DEFAULT 1 CHECK (visible IN (0, 1)),
    read_only INTEGER NOT NULL DEFAULT 0 CHECK (read_only IN (0, 1)),
    source_url TEXT,
    last_synced TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);

CREATE TABLE icalendar_objects (
    id TEXT PRIMARY KEY,
    calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('import-file', 'import-zip-entry', 'local-export-base', 'subscription')),
    source_name TEXT NOT NULL DEFAULT '',
    source_fingerprint TEXT NOT NULL,
    prodid TEXT,
    version TEXT,
    method TEXT,
    calendar_scale TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_icalendar_objects_calendar ON icalendar_objects(calendar_id);
CREATE INDEX idx_icalendar_objects_fingerprint ON icalendar_objects(source_fingerprint);
CREATE INDEX idx_icalendar_objects_source ON icalendar_objects(calendar_id, source_kind, source_name);

CREATE TABLE icalendar_object_diagnostics (
    id TEXT PRIMARY KEY,
    object_id TEXT NOT NULL REFERENCES icalendar_objects(id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_icalendar_object_diagnostics_object ON icalendar_object_diagnostics(object_id, sort_order);

CREATE TABLE icalendar_components (
    id TEXT PRIMARY KEY,
    object_id TEXT NOT NULL REFERENCES icalendar_objects(id) ON DELETE CASCADE,
    parent_component_id TEXT REFERENCES icalendar_components(id) ON DELETE CASCADE,
    calendar_id TEXT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    component_type TEXT NOT NULL,
    uid TEXT,
    recurrence_id TEXT,
    recurrence_id_value_type TEXT,
    sequence INTEGER,
    dtstart_key TEXT,
    projected_kind TEXT,
    projected_id TEXT,
    preservation_status TEXT NOT NULL CHECK (preservation_status IN ('lossless', 'partial', 'unsupported', 'needs-review', 'regenerated', 'invalid')),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_icalendar_components_calendar_type ON icalendar_components(calendar_id, component_type);
CREATE INDEX idx_icalendar_components_object ON icalendar_components(object_id);
CREATE INDEX idx_icalendar_components_parent
ON icalendar_components(parent_component_id);
CREATE INDEX idx_icalendar_components_projection ON icalendar_components(projected_kind, projected_id);
CREATE INDEX idx_icalendar_components_status ON icalendar_components(preservation_status);
CREATE INDEX idx_icalendar_components_uid ON icalendar_components(calendar_id, uid);
CREATE INDEX idx_icalendar_components_uid_recurrence ON icalendar_components(calendar_id, uid, recurrence_id);

CREATE TABLE icalendar_component_projection_warnings (
    id TEXT PRIMARY KEY,
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    message TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_icalendar_component_projection_warnings_component ON icalendar_component_projection_warnings(component_id, sort_order);

CREATE TABLE icalendar_component_properties (
    id TEXT PRIMARY KEY,
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    value_type TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_icalendar_component_properties_component ON icalendar_component_properties(component_id, sort_order);

CREATE TABLE icalendar_property_parameters (
    id TEXT PRIMARY KEY,
    property_id TEXT NOT NULL REFERENCES icalendar_component_properties(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_icalendar_property_parameters_property ON icalendar_property_parameters(property_id, sort_order);

CREATE TABLE icalendar_value_nodes (
    id TEXT PRIMARY KEY,
    property_id TEXT REFERENCES icalendar_component_properties(id) ON DELETE CASCADE,
    parameter_id TEXT REFERENCES icalendar_property_parameters(id) ON DELETE CASCADE,
    parent_node_id TEXT REFERENCES icalendar_value_nodes(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    value_kind TEXT NOT NULL CHECK (value_kind IN ('array', 'object', 'text', 'number', 'boolean', 'null')),
    object_key TEXT,
    text_value TEXT,
    number_value REAL,
    boolean_value INTEGER
);
CREATE INDEX idx_icalendar_value_nodes_parameter ON icalendar_value_nodes(parameter_id, parent_node_id, sort_order);
CREATE INDEX idx_icalendar_value_nodes_parent ON icalendar_value_nodes(parent_node_id, sort_order);
CREATE INDEX idx_icalendar_value_nodes_property ON icalendar_value_nodes(property_id, parent_node_id, sort_order);

CREATE TABLE calendar_events (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    title TEXT NOT NULL DEFAULT '',
    start_time TEXT NOT NULL CHECK (trim(start_time) <> ''),
    end_time TEXT NOT NULL CHECK (trim(end_time) <> ''),
    timezone TEXT NOT NULL DEFAULT 'UTC' CHECK (trim(timezone) <> ''),
    calendar_id TEXT NOT NULL DEFAULT 'local' REFERENCES calendars(id) ON DELETE NO ACTION,
    project_id TEXT REFERENCES projects(id) ON DELETE SET NULL,
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    description TEXT NOT NULL DEFAULT '',
    rrule TEXT,
    repeat_until TEXT,
    environment_id TEXT,
    playlist_id TEXT,
    all_day INTEGER NOT NULL DEFAULT 0 CHECK (all_day IN (0, 1)),
    location TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL DEFAULT '',
    transparency TEXT NOT NULL DEFAULT 'opaque' CHECK (transparency IN ('opaque', 'transparent')),
    status TEXT NOT NULL DEFAULT 'confirmed' CHECK (status IN ('confirmed', 'tentative', 'cancelled')),
    source_uid TEXT,
    visibility TEXT NOT NULL DEFAULT 'public' CHECK (visibility IN ('public', 'private')),
    priority INTEGER CHECK (priority IS NULL OR (priority >= 0 AND priority <= 9)),
    geo_lat REAL,
    geo_lng REAL,
    sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    guest_can_modify INTEGER NOT NULL DEFAULT 0 CHECK (guest_can_modify IN (0, 1)),
    guest_can_invite_others INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_invite_others IN (0, 1)),
    guest_can_see_other_guests INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_see_other_guests IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    icalendar_component_id TEXT REFERENCES icalendar_components(id) ON DELETE SET NULL,
    local_rsvp_status TEXT CHECK (local_rsvp_status IS NULL OR local_rsvp_status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    meeting_enabled INTEGER NOT NULL DEFAULT 0 CHECK (meeting_enabled IN (0, 1)),
    CHECK (
        (geo_lat IS NULL AND geo_lng IS NULL)
        OR (
            geo_lat IS NOT NULL
            AND geo_lng IS NOT NULL
            AND geo_lat >= -90
            AND geo_lat <= 90
            AND geo_lng >= -180
            AND geo_lng <= 180
        )
    )
);
CREATE INDEX idx_calendar_events_calendar ON calendar_events(calendar_id);
CREATE INDEX idx_calendar_events_end ON calendar_events(end_time);
CREATE INDEX idx_calendar_events_icalendar_component ON calendar_events(icalendar_component_id);
CREATE INDEX idx_calendar_events_project ON calendar_events(project_id, start_time);
CREATE UNIQUE INDEX idx_calendar_events_source_uid ON calendar_events(calendar_id, source_uid);
CREATE INDEX idx_calendar_events_start ON calendar_events(start_time);

CREATE TABLE calendar_event_alarms (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    action TEXT NOT NULL DEFAULT 'display' CHECK (action IN ('display', 'audio', 'email')),
    trigger_type TEXT NOT NULL DEFAULT 'relative' CHECK (trigger_type IN ('relative', 'absolute')),
    trigger_value TEXT NOT NULL CHECK (trim(trigger_value) <> ''),
    description TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT REFERENCES icalendar_components(id) ON DELETE SET NULL
);
CREATE INDEX idx_calendar_event_alarms_event ON calendar_event_alarms(event_id);
CREATE INDEX idx_calendar_event_alarms_icalendar_component ON calendar_event_alarms(icalendar_component_id);

CREATE TABLE calendar_event_attendees (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    role TEXT NOT NULL DEFAULT 'req-participant' CHECK (role IN ('chair', 'req-participant', 'opt-participant', 'non-participant')),
    status TEXT NOT NULL DEFAULT 'needs-action' CHECK (status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    rsvp INTEGER NOT NULL DEFAULT 0 CHECK (rsvp IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT REFERENCES icalendar_components(id) ON DELETE SET NULL,
    icalendar_property_index INTEGER CHECK (icalendar_property_index IS NULL OR icalendar_property_index >= 0)
);
CREATE INDEX idx_calendar_event_attendees_event ON calendar_event_attendees(event_id);
CREATE INDEX idx_calendar_event_attendees_icalendar_component ON calendar_event_attendees(icalendar_component_id);

CREATE TABLE calendar_event_categories (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (trim(category) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE INDEX idx_calendar_event_categories_event ON calendar_event_categories(event_id, sort_order);

CREATE TABLE calendar_event_exdates (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    occurrence_date TEXT NOT NULL CHECK (trim(occurrence_date) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE UNIQUE INDEX idx_calendar_event_exdates_event_date ON calendar_event_exdates(event_id, occurrence_date);

CREATE TABLE calendar_event_extended_properties (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE UNIQUE INDEX idx_calendar_event_extended_properties_key ON calendar_event_extended_properties(event_id, property_key);

CREATE TABLE calendar_event_notifications (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    offset_minutes INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE INDEX idx_calendar_event_notifications_event ON calendar_event_notifications(event_id, sort_order);

CREATE TABLE calendar_event_organizers (
    event_id TEXT PRIMARY KEY REFERENCES calendar_events(id) ON DELETE CASCADE,
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> '')
);

CREATE TABLE calendar_event_rdates (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    occurrence_start TEXT NOT NULL CHECK (trim(occurrence_start) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE UNIQUE INDEX idx_calendar_event_rdates_event_start ON calendar_event_rdates(event_id, occurrence_start);

CREATE TABLE calendar_event_overrides (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    parent_event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    recurrence_id TEXT NOT NULL CHECK (trim(recurrence_id) <> ''),
    title TEXT,
    start_time TEXT,
    end_time TEXT,
    description TEXT,
    location TEXT,
    url TEXT,
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    status TEXT CHECK (status IS NULL OR status IN ('confirmed', 'tentative', 'cancelled')),
    transparency TEXT CHECK (transparency IS NULL OR transparency IN ('opaque', 'transparent')),
    visibility TEXT CHECK (visibility IS NULL OR visibility IN ('public', 'private')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    icalendar_component_id TEXT REFERENCES icalendar_components(id) ON DELETE SET NULL,
    recurrence_range TEXT CHECK (recurrence_range IS NULL OR recurrence_range = 'this-and-future')
);
CREATE INDEX idx_calendar_event_overrides_icalendar_component ON calendar_event_overrides(icalendar_component_id);
CREATE UNIQUE INDEX idx_calendar_event_overrides_parent_recurrence_id ON calendar_event_overrides(parent_event_id, recurrence_id);

CREATE TABLE calendar_event_override_extended_properties (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    override_id TEXT NOT NULL REFERENCES calendar_event_overrides(id) ON DELETE CASCADE,
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);
CREATE UNIQUE INDEX idx_calendar_event_override_extended_properties_key ON calendar_event_override_extended_properties(override_id, property_key);

CREATE TABLE calendar_event_pomodoro_configs (
    event_id TEXT PRIMARY KEY REFERENCES calendar_events(id) ON DELETE CASCADE,
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER CHECK (idle_timeout_minutes IS NULL OR idle_timeout_minutes > 0)
);

CREATE TABLE calendar_event_pomodoro_config_count_rhythms (
    event_id TEXT PRIMARY KEY REFERENCES calendar_event_pomodoro_configs(event_id) ON DELETE CASCADE,
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    )
);

CREATE TABLE calendar_event_pomodoro_config_sequence_steps (
    event_id TEXT NOT NULL REFERENCES calendar_event_pomodoro_configs(event_id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL CHECK (break_phase IN ('short_break', 'long_break')),
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (event_id, step_index)
);

CREATE TABLE calendar_event_archives (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_event_id TEXT NOT NULL CHECK (trim(source_event_id) <> ''),
    archived_at TEXT NOT NULL CHECK (trim(archived_at) <> ''),
    title TEXT NOT NULL DEFAULT '',
    start_time TEXT NOT NULL CHECK (trim(start_time) <> ''),
    end_time TEXT NOT NULL CHECK (trim(end_time) <> ''),
    timezone TEXT NOT NULL DEFAULT 'UTC' CHECK (trim(timezone) <> ''),
    calendar_id TEXT NOT NULL CHECK (trim(calendar_id) <> ''),
    project_id TEXT,
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    description TEXT NOT NULL DEFAULT '',
    rrule TEXT,
    repeat_until TEXT,
    environment_id TEXT,
    playlist_id TEXT,
    all_day INTEGER NOT NULL DEFAULT 0 CHECK (all_day IN (0, 1)),
    location TEXT NOT NULL DEFAULT '',
    url TEXT NOT NULL DEFAULT '',
    transparency TEXT NOT NULL DEFAULT 'opaque' CHECK (transparency IN ('opaque', 'transparent')),
    status TEXT NOT NULL DEFAULT 'confirmed' CHECK (status IN ('confirmed', 'tentative', 'cancelled')),
    source_uid TEXT,
    visibility TEXT NOT NULL DEFAULT 'public' CHECK (visibility IN ('public', 'private')),
    priority INTEGER CHECK (priority IS NULL OR (priority >= 0 AND priority <= 9)),
    geo_lat REAL,
    geo_lng REAL,
    sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    guest_can_modify INTEGER NOT NULL DEFAULT 0 CHECK (guest_can_modify IN (0, 1)),
    guest_can_invite_others INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_invite_others IN (0, 1)),
    guest_can_see_other_guests INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_see_other_guests IN (0, 1)),
    created_at TEXT NOT NULL CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL CHECK (trim(updated_at) <> ''),
    icalendar_component_id TEXT,
    local_rsvp_status TEXT CHECK (local_rsvp_status IS NULL OR local_rsvp_status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    meeting_enabled INTEGER NOT NULL DEFAULT 0 CHECK (meeting_enabled IN (0, 1)),
    original_occurrence_id TEXT,
    recurrence_date TEXT
        CHECK (recurrence_date IS NULL OR length(recurrence_date) = 10),
    CHECK (
        (geo_lat IS NULL AND geo_lng IS NULL)
        OR (
            geo_lat IS NOT NULL
            AND geo_lng IS NOT NULL
            AND geo_lat >= -90
            AND geo_lat <= 90
            AND geo_lng >= -180
            AND geo_lng <= 180
        )
    )
);
CREATE INDEX idx_calendar_event_archives_calendar ON calendar_event_archives(calendar_id);
CREATE INDEX idx_calendar_event_archives_icalendar_component ON calendar_event_archives(icalendar_component_id);
CREATE INDEX idx_calendar_event_archives_source ON calendar_event_archives(source_event_id);

CREATE TABLE calendar_event_archive_alarms (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    source_alarm_id TEXT NOT NULL CHECK (trim(source_alarm_id) <> ''),
    action TEXT NOT NULL DEFAULT 'display' CHECK (action IN ('display', 'audio', 'email')),
    trigger_type TEXT NOT NULL DEFAULT 'relative' CHECK (trigger_type IN ('relative', 'absolute')),
    trigger_value TEXT NOT NULL CHECK (trim(trigger_value) <> ''),
    description TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT
);
CREATE INDEX idx_calendar_event_archive_alarms_icalendar_component ON calendar_event_archive_alarms(icalendar_component_id);

CREATE TABLE calendar_event_archive_attendees (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    source_attendee_id TEXT NOT NULL CHECK (trim(source_attendee_id) <> ''),
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    role TEXT NOT NULL DEFAULT 'req-participant' CHECK (role IN ('chair', 'req-participant', 'opt-participant', 'non-participant')),
    status TEXT NOT NULL DEFAULT 'needs-action' CHECK (status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    rsvp INTEGER NOT NULL DEFAULT 0 CHECK (rsvp IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT,
    icalendar_property_index INTEGER CHECK (icalendar_property_index IS NULL OR icalendar_property_index >= 0)
);
CREATE INDEX idx_calendar_event_archive_attendees_icalendar_component ON calendar_event_archive_attendees(icalendar_component_id);

CREATE TABLE calendar_event_archive_categories (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (trim(category) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_exdates (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    occurrence_date TEXT NOT NULL CHECK (trim(occurrence_date) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_extended_properties (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_import_objects (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL REFERENCES icalendar_objects(id) ON DELETE CASCADE,
    PRIMARY KEY (archive_event_id, object_id)
);
CREATE INDEX idx_calendar_event_archive_import_objects_object
    ON calendar_event_archive_import_objects(object_id);

CREATE TABLE calendar_event_archive_music_assignments (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('event-snapshot', 'event-override')),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short-break', 'long-break')),
    behavior TEXT NOT NULL CHECK (behavior IN (
            'inherit', 'play-automatically', 'prepare-silently', 'pause-music', 'keep-current-music'
        )),
    playlist_id TEXT,
    soundscape_id TEXT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN ('explicit', 'copied-project', 'work-environment')),
    provenance_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL CHECK (version > 0),
    soundscape_behavior TEXT NOT NULL CHECK (soundscape_behavior IN (
            'inherit', 'play-selected', 'pause-soundscape', 'keep-current-soundscape'
        )),
    PRIMARY KEY (archive_event_id, owner_kind, phase)
);

CREATE TABLE calendar_event_archive_notifications (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    offset_minutes INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_organizers (
    archive_event_id TEXT PRIMARY KEY REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> '')
);

CREATE TABLE calendar_event_archive_overrides (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    source_override_id TEXT NOT NULL CHECK (trim(source_override_id) <> ''),
    recurrence_id TEXT NOT NULL CHECK (trim(recurrence_id) <> ''),
    title TEXT,
    start_time TEXT,
    end_time TEXT,
    description TEXT,
    location TEXT,
    url TEXT,
    color INTEGER CHECK (color IS NULL OR (color >= 0 AND color < 32)),
    status TEXT CHECK (status IS NULL OR status IN ('confirmed', 'tentative', 'cancelled')),
    transparency TEXT CHECK (transparency IS NULL OR transparency IN ('opaque', 'transparent')),
    visibility TEXT CHECK (visibility IS NULL OR visibility IN ('public', 'private')),
    created_at TEXT NOT NULL CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL CHECK (trim(updated_at) <> ''),
    icalendar_component_id TEXT,
    recurrence_range TEXT CHECK (recurrence_range IS NULL OR recurrence_range = 'this-and-future')
);
CREATE INDEX idx_calendar_event_archive_overrides_icalendar_component ON calendar_event_archive_overrides(icalendar_component_id);
CREATE INDEX idx_calendar_event_archive_overrides_source ON calendar_event_archive_overrides(source_override_id);

CREATE TABLE calendar_event_archive_override_extended_properties (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_override_id TEXT NOT NULL REFERENCES calendar_event_archive_overrides(id) ON DELETE CASCADE,
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_pomodoro_configs (
    archive_event_id TEXT PRIMARY KEY REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER CHECK (idle_timeout_minutes IS NULL OR idle_timeout_minutes > 0)
);

CREATE TABLE calendar_event_archive_pomodoro_config_count_rhythms (
    archive_event_id TEXT PRIMARY KEY REFERENCES calendar_event_archive_pomodoro_configs(archive_event_id) ON DELETE CASCADE,
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    )
);

CREATE TABLE calendar_event_archive_pomodoro_config_sequence_steps (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archive_pomodoro_configs(archive_event_id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL CHECK (break_phase IN ('short_break', 'long_break')),
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (archive_event_id, step_index)
);

CREATE TABLE calendar_event_archive_rdates (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    occurrence_start TEXT NOT NULL CHECK (trim(occurrence_start) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0)
);

CREATE TABLE calendar_event_archive_task_links (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL CHECK (trim(task_id) <> ''),
    link_kind TEXT NOT NULL CHECK (link_kind IN ('scheduled', 'reference')),
    created_at TEXT NOT NULL CHECK (trim(created_at) <> ''),
    PRIMARY KEY (archive_event_id, task_id)
);

CREATE TABLE calendar_edit_receipts (
    command_id TEXT PRIMARY KEY NOT NULL,
    intent_hash TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);


-- Pomodoro: Runs, segments, pauses, native execution state, and the adaptive policy model.

CREATE TABLE pomodoro_execution_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    state_json TEXT NOT NULL CHECK (
        json_valid(state_json) AND json_type(state_json) = 'object'
        AND length(CAST(state_json AS BLOB)) <= 65536
    ),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
);

CREATE TABLE pomodoro_execution_receipts (
    command_id TEXT PRIMARY KEY CHECK (length(command_id) BETWEEN 1 AND 128),
    request_json TEXT NOT NULL CHECK (
        json_valid(request_json) AND json_type(request_json) = 'object'
        AND length(CAST(request_json AS BLOB)) <= 65536
    ),
    result_json TEXT NOT NULL CHECK (
        json_valid(result_json) AND json_type(result_json) = 'object'
        AND length(CAST(result_json AS BLOB)) <= 262144
    ),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    committed_at_ms INTEGER NOT NULL CHECK (committed_at_ms >= 0)
);

CREATE TABLE pomodoro_runs (
    id TEXT PRIMARY KEY,
    event_id TEXT REFERENCES calendar_events(id) ON DELETE SET NULL,
    original_event_id TEXT NOT NULL,
    event_date TEXT NOT NULL,
    planned_start TEXT NOT NULL,
    planned_end TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    end_reason TEXT CHECK (end_reason IS NULL OR end_reason IN ('completed', 'stopped', 'interrupted', 'reconfigured', 'block_transition')),
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER,
    last_heartbeat TEXT NOT NULL,
    event_title_snapshot TEXT,
    inherited_focus_minutes INTEGER NOT NULL DEFAULT 0,
    inherited_rhythm_position INTEGER NOT NULL DEFAULT 1,
    inherited_from_run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    start_trigger TEXT NOT NULL DEFAULT 'manual' CHECK (start_trigger IN ('manual', 'block_auto', 'block_transition', 'reconfigure', 'crash_recovery')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    inherited_focus_milliseconds INTEGER
        CHECK (inherited_focus_milliseconds IS NULL OR inherited_focus_milliseconds >= 0),
    inherited_phase_milliseconds INTEGER
        CHECK (inherited_phase_milliseconds IS NULL OR inherited_phase_milliseconds >= 0),
    current_occurrence_id TEXT
        CHECK (current_occurrence_id IS NULL OR trim(current_occurrence_id) <> ''),
    current_event_date TEXT
        CHECK (current_event_date IS NULL OR trim(current_event_date) <> ''),
    current_event_title TEXT,
    calendar_archive_id TEXT
        REFERENCES calendar_event_archives(id) ON DELETE SET NULL
);
CREATE INDEX idx_pomodoro_runs_calendar_archive ON pomodoro_runs(calendar_archive_id);
CREATE INDEX idx_pomodoro_runs_current_occurrence
ON pomodoro_runs(current_occurrence_id)
WHERE current_occurrence_id IS NOT NULL;
CREATE INDEX idx_pomodoro_runs_event_date ON pomodoro_runs(event_id, event_date);
CREATE INDEX idx_pomodoro_runs_open ON pomodoro_runs(ended_at);
CREATE INDEX idx_pomodoro_runs_original_event ON pomodoro_runs(original_event_id);
CREATE UNIQUE INDEX idx_pomodoro_runs_single_open ON pomodoro_runs((1)) WHERE ended_at IS NULL;

CREATE TABLE pomodoro_adaptive_planned_blocks (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    capture_run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    event_date TEXT NOT NULL CHECK (trim(event_date) <> ''),
    event_id TEXT CHECK (event_id IS NULL OR trim(event_id) <> ''),
    original_event_id TEXT NOT NULL CHECK (trim(original_event_id) <> ''),
    planned_start TEXT NOT NULL CHECK (trim(planned_start) <> ''),
    planned_end TEXT NOT NULL CHECK (trim(planned_end) <> ''),
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('live_event', 'archived_event', 'scheduler_snapshot')
    ),
    captured_at TEXT NOT NULL CHECK (trim(captured_at) <> '')
);
CREATE INDEX idx_pomodoro_adaptive_planned_blocks_date
ON pomodoro_adaptive_planned_blocks(event_date);
CREATE UNIQUE INDEX idx_pomodoro_adaptive_planned_blocks_unique
ON pomodoro_adaptive_planned_blocks(event_date, original_event_id, planned_start);

CREATE TABLE pomodoro_run_count_rhythms (
    run_id TEXT PRIMARY KEY REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    )
);

CREATE TABLE pomodoro_run_sequence_steps (
    run_id TEXT NOT NULL REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL CHECK (break_phase IN ('short_break', 'long_break')),
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (run_id, step_index)
);

CREATE TABLE pomodoro_segments (
    id TEXT PRIMARY KEY,
    event_id TEXT REFERENCES calendar_events(id) ON DELETE SET NULL,
    event_date TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    rhythm_position INTEGER NOT NULL CHECK (rhythm_position > 0),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short_break', 'long_break')),
    planned_start TEXT NOT NULL,
    planned_end TEXT NOT NULL,
    actual_start TEXT NOT NULL,
    actual_end TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'completed', 'interrupted')),
    end_reason TEXT CHECK (end_reason IS NULL OR end_reason IN ('completed', 'stopped', 'skipped_by_user', 'event_expired', 'focus_failed', 'reconfigured', 'block_transition', 'crash_recovery')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    chosen_duration_ms INTEGER
        CHECK (chosen_duration_ms IS NULL OR chosen_duration_ms > 0)
);
CREATE INDEX idx_pomodoro_segments_event ON pomodoro_segments(event_id, event_date);
CREATE INDEX idx_pomodoro_segments_run ON pomodoro_segments(run_id);
CREATE INDEX idx_pomodoro_segments_run_actual ON pomodoro_segments(run_id, actual_start);
CREATE UNIQUE INDEX idx_pomodoro_segments_single_active ON pomodoro_segments((1)) WHERE status = 'active';

CREATE TABLE pomodoro_pauses (
    id TEXT PRIMARY KEY,
    segment_id TEXT NOT NULL REFERENCES pomodoro_segments(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    reason TEXT NOT NULL CHECK (reason IN ('idle', 'manual', 'suspend')),
    detected_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pomodoro_pauses_reason ON pomodoro_pauses(reason, started_at);
CREATE INDEX idx_pomodoro_pauses_segment ON pomodoro_pauses(segment_id, started_at);
CREATE UNIQUE INDEX idx_pomodoro_pauses_single_open_per_segment ON pomodoro_pauses(segment_id) WHERE ended_at IS NULL;

CREATE TABLE pomodoro_run_events (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL CHECK (
        event_type IN (
            'start',
            'phase_start',
            'phase_complete',
            'pause_start',
            'pause_end',
            'idle_detected',
            'focus_failed',
            'suspend_detected',
            'skip_break',
            'extend_focus',
            'go_to_break_now',
            'start_focus_now',
            'reconfigure',
            'block_transition',
            'stop',
            'complete',
            'crash_recovery'
        )
    ),
    occurred_at TEXT NOT NULL,
    phase TEXT CHECK (phase IS NULL OR phase IN ('focus', 'short_break', 'long_break')),
    reason TEXT,
    duration_seconds INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pomodoro_run_events_run ON pomodoro_run_events(run_id, occurred_at);

CREATE TABLE pomodoro_adaptive_policies (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'archived')),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    model_version INTEGER NOT NULL CHECK (model_version > 0),
    exploration_budget_per_week INTEGER NOT NULL DEFAULT 2 CHECK (
        exploration_budget_per_week >= 0 AND exploration_budget_per_week <= 20
    ),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX idx_pomodoro_adaptive_policies_single_active
ON pomodoro_adaptive_policies((1))
WHERE status = 'active';

CREATE TABLE pomodoro_adaptive_context_state_history (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    policy_id TEXT NOT NULL REFERENCES pomodoro_adaptive_policies(id) ON DELETE CASCADE,
    context_key TEXT NOT NULL CHECK (trim(context_key) <> ''),
    observed_at TEXT NOT NULL CHECK (trim(observed_at) <> ''),
    readiness REAL NOT NULL CHECK (readiness >= 0.0 AND readiness <= 1.0),
    strain REAL NOT NULL CHECK (strain >= 0.0 AND strain <= 1.0),
    recovery_debt REAL NOT NULL CHECK (recovery_debt >= 0.0 AND recovery_debt <= 1.0),
    avoidance_pressure REAL NOT NULL CHECK (avoidance_pressure >= 0.0 AND avoidance_pressure <= 1.0),
    momentum REAL NOT NULL CHECK (momentum >= 0.0 AND momentum <= 1.0),
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pomodoro_adaptive_context_state_history_policy
ON pomodoro_adaptive_context_state_history(policy_id, context_key, observed_at);

CREATE TABLE pomodoro_adaptive_context_states (
    policy_id TEXT NOT NULL REFERENCES pomodoro_adaptive_policies(id) ON DELETE CASCADE,
    context_key TEXT NOT NULL CHECK (trim(context_key) <> ''),
    readiness REAL NOT NULL CHECK (readiness >= 0.0 AND readiness <= 1.0),
    strain REAL NOT NULL CHECK (strain >= 0.0 AND strain <= 1.0),
    recovery_debt REAL NOT NULL CHECK (recovery_debt >= 0.0 AND recovery_debt <= 1.0),
    avoidance_pressure REAL NOT NULL CHECK (avoidance_pressure >= 0.0 AND avoidance_pressure <= 1.0),
    momentum REAL NOT NULL CHECK (momentum >= 0.0 AND momentum <= 1.0),
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0),
    updated_at TEXT NOT NULL,
    PRIMARY KEY (policy_id, context_key)
);

CREATE TABLE pomodoro_adaptive_policy_bounds (
    policy_id TEXT NOT NULL REFERENCES pomodoro_adaptive_policies(id) ON DELETE CASCADE,
    parameter_key TEXT NOT NULL CHECK (
        parameter_key IN (
            'focus_duration_minutes',
            'short_break_minutes',
            'long_break_minutes',
            'long_break_after_focus_count'
        )
    ),
    min_value REAL NOT NULL,
    max_value REAL NOT NULL,
    PRIMARY KEY (policy_id, parameter_key),
    CHECK (min_value <= max_value)
);

CREATE TABLE pomodoro_adaptive_context_snapshots (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    local_started_at TEXT NOT NULL CHECK (trim(local_started_at) <> ''),
    time_of_day TEXT NOT NULL CHECK (time_of_day IN ('morning', 'midday', 'afternoon', 'evening', 'late')),
    session_position TEXT NOT NULL CHECK (session_position IN ('first', 'middle', 'late')),
    event_length TEXT NOT NULL CHECK (event_length IN ('short', 'medium', 'long')),
    workload TEXT NOT NULL CHECK (workload IN ('low', 'normal', 'high')),
    energy TEXT NOT NULL CHECK (energy IN ('low', 'normal', 'high', 'unknown')),
    environment_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pomodoro_adaptive_context_snapshots_run
ON pomodoro_adaptive_context_snapshots(run_id, created_at);

CREATE TABLE pomodoro_adaptive_context_snapshot_features (
    snapshot_id TEXT NOT NULL REFERENCES pomodoro_adaptive_context_snapshots(id) ON DELETE CASCADE,
    feature_key TEXT NOT NULL CHECK (trim(feature_key) <> ''),
    numeric_value REAL,
    categorical_value TEXT,
    boolean_value INTEGER CHECK (boolean_value IS NULL OR boolean_value IN (0, 1)),
    missing INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0, 1)),
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('pomodoro', 'distractions', 'calendar', 'diary', 'project', 'environment', 'device')
    ),
    PRIMARY KEY (snapshot_id, feature_key),
    CHECK (
        missing = 1 OR
        numeric_value IS NOT NULL OR
        categorical_value IS NOT NULL OR
        boolean_value IS NOT NULL
    )
);

CREATE TABLE pomodoro_adaptive_data_quality_flags (
    snapshot_id TEXT NOT NULL REFERENCES pomodoro_adaptive_context_snapshots(id) ON DELETE CASCADE,
    flag TEXT NOT NULL CHECK (
        flag IN (
            'extension_unavailable',
            'desktop_tracking_unavailable',
            'diary_missing',
            'idle_detection_disabled',
            'crash_recovered',
            'calendar_clipped'
        )
    ),
    PRIMARY KEY (snapshot_id, flag)
);

CREATE TABLE pomodoro_adaptive_decisions (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    policy_id TEXT REFERENCES pomodoro_adaptive_policies(id) ON DELETE SET NULL,
    run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    context_snapshot_id TEXT REFERENCES pomodoro_adaptive_context_snapshots(id) ON DELETE SET NULL,
    opportunity_kind TEXT NOT NULL CHECK (
        opportunity_kind IN (
            'run_start',
            'focus_start',
            'break_start',
            'focus_tick',
            'break_overtime',
            'block_event',
            'idle_failure',
            'run_outcome'
        )
    ),
    candidate_id TEXT CHECK (candidate_id IS NULL OR trim(candidate_id) <> ''),
    decision_mode TEXT NOT NULL CHECK (
        decision_mode IN ('fallback', 'hold', 'recovery', 'guardrail', 'exploit', 'explore')
    ),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    model_version INTEGER NOT NULL CHECK (model_version > 0),
    occurred_at TEXT NOT NULL CHECK (trim(occurred_at) <> ''),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pomodoro_adaptive_decisions_policy
ON pomodoro_adaptive_decisions(policy_id, occurred_at);
CREATE INDEX idx_pomodoro_adaptive_decisions_run
ON pomodoro_adaptive_decisions(run_id, occurred_at);

CREATE TABLE pomodoro_adaptive_decision_reasons (
    decision_id TEXT NOT NULL REFERENCES pomodoro_adaptive_decisions(id) ON DELETE CASCADE,
    reason_code TEXT NOT NULL CHECK (
        reason_code IN (
            'no_history',
            'low_confidence',
            'missing_extension_data',
            'missing_diary_data',
            'high_strain',
            'high_avoidance_pressure',
            'high_recovery_debt',
            'clean_momentum',
            'break_return_drift',
            'break_transition_pressure',
            'skipped_break_recovery',
            'focus_idle_pressure',
            'repeated_blocked_source_pressure',
            'capacity_rebuild',
            'experiment_assignment',
            'experiment_guardrail',
            'guardrail_recovery',
            'replay_candidate',
            'hold_current_rhythm'
        )
    ),
    PRIMARY KEY (decision_id, reason_code)
);

CREATE TABLE pomodoro_adaptive_decision_state_scores (
    decision_id TEXT PRIMARY KEY REFERENCES pomodoro_adaptive_decisions(id) ON DELETE CASCADE,
    readiness REAL NOT NULL CHECK (readiness >= 0.0 AND readiness <= 1.0),
    strain REAL NOT NULL CHECK (strain >= 0.0 AND strain <= 1.0),
    recovery_debt REAL NOT NULL CHECK (recovery_debt >= 0.0 AND recovery_debt <= 1.0),
    avoidance_pressure REAL NOT NULL CHECK (avoidance_pressure >= 0.0 AND avoidance_pressure <= 1.0),
    momentum REAL NOT NULL CHECK (momentum >= 0.0 AND momentum <= 1.0),
    confidence REAL NOT NULL CHECK (confidence >= 0.0 AND confidence <= 1.0)
);

CREATE TABLE pomodoro_adaptive_decision_values (
    decision_id TEXT NOT NULL REFERENCES pomodoro_adaptive_decisions(id) ON DELETE CASCADE,
    value_key TEXT NOT NULL CHECK (
        value_key IN (
            'focus_duration_minutes',
            'short_break_minutes',
            'long_break_minutes',
            'long_break_after_focus_count'
        )
    ),
    previous_numeric_value REAL,
    selected_numeric_value REAL NOT NULL,
    value_unit TEXT NOT NULL CHECK (value_unit IN ('minutes', 'count')),
    PRIMARY KEY (decision_id, value_key)
);

CREATE TABLE pomodoro_run_adaptive_snapshots (
    run_id TEXT PRIMARY KEY REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    policy_id TEXT REFERENCES pomodoro_adaptive_policies(id) ON DELETE SET NULL,
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    model_version INTEGER NOT NULL CHECK (model_version > 0),
    context_snapshot_id TEXT REFERENCES pomodoro_adaptive_context_snapshots(id) ON DELETE SET NULL,
    decision_id TEXT REFERENCES pomodoro_adaptive_decisions(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE pomodoro_adaptive_experiments (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    policy_id TEXT REFERENCES pomodoro_adaptive_policies(id) ON DELETE SET NULL,
    parameter_key TEXT NOT NULL CHECK (
        parameter_key IN (
            'focus_duration_minutes',
            'short_break_minutes',
            'long_break_minutes',
            'long_break_after_focus_count',
            'rhythm_bundle'
        )
    ),
    assignment_unit TEXT NOT NULL CHECK (assignment_unit IN ('phase', 'run', 'day', 'context')),
    status TEXT NOT NULL CHECK (status IN ('draft', 'active', 'paused', 'completed', 'abandoned')),
    started_at TEXT,
    ended_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (ended_at IS NULL OR started_at IS NOT NULL)
);
CREATE INDEX idx_pomodoro_adaptive_experiments_policy
ON pomodoro_adaptive_experiments(policy_id);

CREATE TABLE pomodoro_adaptive_experiment_variants (
    experiment_id TEXT NOT NULL REFERENCES pomodoro_adaptive_experiments(id) ON DELETE CASCADE,
    variant_key TEXT NOT NULL CHECK (trim(variant_key) <> ''),
    numeric_value REAL NOT NULL,
    is_control INTEGER NOT NULL DEFAULT 0 CHECK (is_control IN (0, 1)),
    PRIMARY KEY (experiment_id, variant_key)
);
CREATE UNIQUE INDEX idx_pomodoro_adaptive_experiment_variants_one_control
ON pomodoro_adaptive_experiment_variants(experiment_id)
WHERE is_control = 1;

CREATE TABLE pomodoro_adaptive_assignments (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    experiment_id TEXT NOT NULL REFERENCES pomodoro_adaptive_experiments(id) ON DELETE CASCADE,
    variant_key TEXT NOT NULL,
    run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    context_snapshot_id TEXT REFERENCES pomodoro_adaptive_context_snapshots(id) ON DELETE SET NULL,
    assignment_seed TEXT NOT NULL CHECK (trim(assignment_seed) <> ''),
    assigned_at TEXT NOT NULL CHECK (trim(assigned_at) <> ''),
    FOREIGN KEY (experiment_id, variant_key)
        REFERENCES pomodoro_adaptive_experiment_variants(experiment_id, variant_key)
        ON DELETE CASCADE
);
CREATE INDEX idx_pomodoro_adaptive_assignments_experiment
ON pomodoro_adaptive_assignments(experiment_id, assigned_at);

CREATE TABLE pomodoro_adaptive_outcomes (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    decision_id TEXT REFERENCES pomodoro_adaptive_decisions(id) ON DELETE SET NULL,
    assignment_id TEXT REFERENCES pomodoro_adaptive_assignments(id) ON DELETE SET NULL,
    outcome_window TEXT NOT NULL CHECK (outcome_window IN ('phase', 'run', 'day', 'next_day')),
    outcome_key TEXT NOT NULL CHECK (trim(outcome_key) <> ''),
    numeric_value REAL,
    boolean_value INTEGER CHECK (boolean_value IS NULL OR boolean_value IN (0, 1)),
    categorical_value TEXT,
    measured_at TEXT NOT NULL CHECK (trim(measured_at) <> ''),
    CHECK (
        numeric_value IS NOT NULL OR
        boolean_value IS NOT NULL OR
        categorical_value IS NOT NULL
    )
);
CREATE INDEX idx_pomodoro_adaptive_outcomes_assignment
ON pomodoro_adaptive_outcomes(assignment_id, measured_at);
CREATE INDEX idx_pomodoro_adaptive_outcomes_decision
ON pomodoro_adaptive_outcomes(decision_id, measured_at);


-- Distractions: Block events and usage samples.

CREATE TABLE distractions_block_events (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE SET NULL,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL CHECK (trim(occurred_at) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('browser', 'desktop_app', 'mobile_app')),
    source_key TEXT NOT NULL CHECK (trim(source_key) <> '' AND instr(source_key, '://') = 0),
    display_name TEXT,
    phase TEXT CHECK (
        phase IS NULL OR
        phase IN ('focus', 'short_break', 'long_break', 'manual_pause', 'idle_pause', 'suspend_pause')
    ),
    decision TEXT NOT NULL CHECK (
        decision IN ('blocked', 'temporary_allowed', 'false_positive_reported', 'limit_exhausted')
    ),
    rule_id TEXT,
    category_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_distractions_block_events_run
ON distractions_block_events(run_id, occurred_at);
CREATE INDEX idx_distractions_block_events_source
ON distractions_block_events(source_type, source_key, occurred_at);

CREATE TABLE distractions_block_event_rule_snapshots (
    block_event_id TEXT PRIMARY KEY REFERENCES distractions_block_events(id) ON DELETE CASCADE,
    rule_id TEXT,
    rule_kind TEXT CHECK (
        rule_kind IS NULL OR
        rule_kind IN (
            'domain',
            'url_pattern',
            'category',
            'custom_category',
            'usage_limit',
            'desktop_app',
            'mobile_app'
        )
    ),
    rule_label TEXT,
    environment_id TEXT,
    blocker_mode TEXT CHECK (
        blocker_mode IS NULL OR
        blocker_mode IN ('blacklist', 'whitelist', 'limit')
    )
);

CREATE TABLE distractions_usage_samples (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('website', 'desktop-app', 'mobile-app')),
    source_key TEXT NOT NULL CHECK (trim(source_key) <> ''),
    display_name TEXT,
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    elapsed_seconds INTEGER NOT NULL CHECK (elapsed_seconds > 0 AND elapsed_seconds <= 86400),
    local_date TEXT NOT NULL CHECK (
        length(local_date) = 10
        AND substr(local_date, 5, 1) = '-'
        AND substr(local_date, 8, 1) = '-'
    ),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0)
);
CREATE INDEX idx_distractions_usage_samples_date_source ON distractions_usage_samples(local_date, source_type, source_key);
CREATE INDEX idx_distractions_usage_samples_started ON distractions_usage_samples(started_at_ms);


-- Music: Library, sources, playlists, soundscapes, and the native session.

CREATE TABLE music_library_items (
    id TEXT PRIMARY KEY,
    identity_key TEXT NOT NULL UNIQUE CHECK (trim(identity_key) <> ''),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('local-file', 'youtube-video')),
    media_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (media_kind IN ('audio', 'video', 'unknown')),
    youtube_video_id TEXT UNIQUE,
    original_title TEXT NOT NULL DEFAULT '',
    original_artist TEXT NOT NULL DEFAULT '',
    original_album TEXT NOT NULL DEFAULT '',
    title_override TEXT,
    artist_override TEXT,
    album_override TEXT,
    artwork_override TEXT,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    availability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (availability IN ('available', 'missing', 'unavailable', 'ambiguous', 'unknown')),
    review_state TEXT NOT NULL DEFAULT 'unreviewed'
        CHECK (review_state IN ('unreviewed', 'reviewed', 'deferred', 'ignored')),
    review_changed_at_ms INTEGER,
    discovered_at_ms INTEGER NOT NULL CHECK (discovered_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    original_track_number INTEGER
        CHECK (original_track_number IS NULL OR original_track_number > 0),
    original_artwork_identity TEXT,
    youtube_resolution_state TEXT
        CHECK (
        youtube_resolution_state IS NULL
        OR youtube_resolution_state IN (
            'resolving', 'ready', 'unavailable', 'embedding-blocked', 'timed-out'
        )
    ),
    review_deferred_until_ms INTEGER
        CHECK (review_deferred_until_ms IS NULL OR review_deferred_until_ms > 0),
    CHECK (
        (source_kind = 'local-file' AND youtube_video_id IS NULL)
        OR (source_kind = 'youtube-video' AND youtube_video_id IS NOT NULL AND trim(youtube_video_id) <> '')
    )
);
CREATE INDEX idx_music_library_items_availability
    ON music_library_items(availability, source_kind, id);
CREATE INDEX idx_music_library_items_review
    ON music_library_items(review_state, discovered_at_ms, id);
CREATE INDEX idx_music_library_items_review_deferred_until
ON music_library_items(review_state, review_deferred_until_ms, discovered_at_ms, id);

CREATE TABLE music_item_signals (
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    signal TEXT NOT NULL
        CHECK (signal IN ('lyrics', 'sudden-changes', 'high-intensity', 'calm', 'repetitive', 'energizing')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    PRIMARY KEY (item_id, signal)
);

CREATE TABLE music_listening_statistics (
    item_id TEXT PRIMARY KEY REFERENCES music_library_items(id) ON DELETE CASCADE,
    last_played_at_ms INTEGER,
    play_count INTEGER NOT NULL DEFAULT 0 CHECK (play_count >= 0),
    completion_count INTEGER NOT NULL DEFAULT 0 CHECK (completion_count >= 0),
    skip_count INTEGER NOT NULL DEFAULT 0 CHECK (skip_count >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0)
);
CREATE INDEX idx_music_listening_statistics_last_played
    ON music_listening_statistics(last_played_at_ms DESC, item_id);

CREATE TABLE music_local_roots (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
);

CREATE TABLE music_local_locations (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    root_id TEXT NOT NULL REFERENCES music_local_roots(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL CHECK (trim(relative_path) <> ''),
    file_size_bytes INTEGER CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0),
    modified_at_ms INTEGER,
    lightweight_fingerprint TEXT,
    strong_fingerprint TEXT,
    availability TEXT NOT NULL DEFAULT 'unknown'
        CHECK (availability IN ('available', 'missing', 'ambiguous', 'unsupported', 'unknown')),
    last_seen_generation INTEGER CHECK (last_seen_generation IS NULL OR last_seen_generation >= 0),
    first_seen_at_ms INTEGER NOT NULL CHECK (first_seen_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    UNIQUE (root_id, relative_path),
    UNIQUE (root_id, item_id, relative_path)
);
CREATE INDEX idx_music_local_locations_item
    ON music_local_locations(item_id, availability);
CREATE INDEX idx_music_local_locations_root_availability
    ON music_local_locations(root_id, availability, relative_path);

CREATE TABLE music_relink_plans (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    root_id TEXT NOT NULL REFERENCES music_local_roots(id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('planning', 'ready', 'applied', 'cancelled')),
    exact_count INTEGER NOT NULL DEFAULT 0 CHECK (exact_count >= 0),
    likely_count INTEGER NOT NULL DEFAULT 0 CHECK (likely_count >= 0),
    ambiguous_count INTEGER NOT NULL DEFAULT 0 CHECK (ambiguous_count >= 0),
    missing_count INTEGER NOT NULL DEFAULT 0 CHECK (missing_count >= 0),
    new_count INTEGER NOT NULL DEFAULT 0 CHECK (new_count >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0)
);

CREATE TABLE music_relink_plan_entries (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    plan_id TEXT NOT NULL REFERENCES music_relink_plans(id) ON DELETE CASCADE,
    match_kind TEXT NOT NULL CHECK (match_kind IN ('exact', 'likely', 'ambiguous', 'missing', 'new')),
    old_location_id TEXT,
    suggested_item_id TEXT REFERENCES music_library_items(id) ON DELETE SET NULL,
    candidate_relative_path TEXT,
    candidate_item_ids TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(candidate_item_ids)),
    file_size_bytes INTEGER CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0),
    lightweight_fingerprint TEXT,
    resolved_item_id TEXT REFERENCES music_library_items(id) ON DELETE SET NULL,
    resolved_at_ms INTEGER,
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);
CREATE INDEX idx_music_relink_plan_entries_window
    ON music_relink_plan_entries(plan_id, match_kind, candidate_relative_path, id);

CREATE TABLE music_source_collections (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('local-root', 'youtube-playlist')),
    identity_key TEXT NOT NULL UNIQUE CHECK (trim(identity_key) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    local_root_id TEXT REFERENCES music_local_roots(id) ON DELETE CASCADE,
    youtube_playlist_id TEXT UNIQUE,
    refresh_state TEXT NOT NULL DEFAULT 'idle'
        CHECK (refresh_state IN ('idle', 'queued', 'running', 'partial', 'failed')),
    last_successful_refresh_at_ms INTEGER,
    last_refresh_error_code TEXT,
    snapshot_generation INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_generation >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    discovery_enabled INTEGER NOT NULL DEFAULT 1
        CHECK (discovery_enabled IN (0, 1)),
    removed_at_ms INTEGER,
    previous_successful_refresh_at_ms INTEGER,
    UNIQUE (local_root_id),
    CHECK (
        (kind = 'local-root' AND local_root_id IS NOT NULL AND youtube_playlist_id IS NULL)
        OR (
            kind = 'youtube-playlist'
            AND local_root_id IS NULL
            AND youtube_playlist_id IS NOT NULL
            AND trim(youtube_playlist_id) <> ''
        )
    )
);
CREATE INDEX idx_music_source_collections_kind_state
    ON music_source_collections(kind, refresh_state, updated_at_ms);

CREATE TABLE music_refresh_jobs (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_collection_id TEXT NOT NULL REFERENCES music_source_collections(id) ON DELETE CASCADE,
    local_root_id TEXT REFERENCES music_local_roots(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('local-root', 'youtube-playlist')),
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'running', 'completed', 'partial', 'failed', 'cancelled')
    ),
    generation INTEGER NOT NULL CHECK (generation > 0),
    discovered_count INTEGER NOT NULL DEFAULT 0 CHECK (discovered_count >= 0),
    processed_count INTEGER NOT NULL DEFAULT 0 CHECK (processed_count >= 0),
    skipped_count INTEGER NOT NULL DEFAULT 0 CHECK (skipped_count >= 0),
    issue_count INTEGER NOT NULL DEFAULT 0 CHECK (issue_count >= 0),
    truncated_count INTEGER NOT NULL DEFAULT 0 CHECK (truncated_count >= 0),
    absence_determined INTEGER NOT NULL DEFAULT 0 CHECK (absence_determined IN (0, 1)),
    status_message TEXT NOT NULL DEFAULT '',
    requested_at_ms INTEGER NOT NULL CHECK (requested_at_ms > 0),
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    CHECK (
        (kind = 'local-root' AND local_root_id IS NOT NULL)
        OR (kind = 'youtube-playlist' AND local_root_id IS NULL)
    )
);
CREATE INDEX idx_music_refresh_jobs_source_state
    ON music_refresh_jobs(source_collection_id, state, generation DESC);

CREATE TABLE music_refresh_job_entries (
    job_id TEXT NOT NULL REFERENCES music_refresh_jobs(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    entry_kind TEXT NOT NULL CHECK (entry_kind IN ('directory', 'media')),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'processed', 'skipped')),
    PRIMARY KEY (job_id, relative_path)
);
CREATE INDEX idx_music_refresh_job_entries_pending
    ON music_refresh_job_entries(job_id, entry_kind, state, relative_path);

CREATE TABLE music_refresh_job_issues (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    job_id TEXT NOT NULL REFERENCES music_refresh_jobs(id) ON DELETE CASCADE,
    issue_code TEXT NOT NULL CHECK (trim(issue_code) <> ''),
    relative_path TEXT,
    item_id TEXT REFERENCES music_library_items(id) ON DELETE SET NULL,
    message TEXT NOT NULL CHECK (trim(message) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);
CREATE INDEX idx_music_refresh_job_issues_job
    ON music_refresh_job_issues(job_id, created_at_ms, id);

CREATE TABLE music_source_collection_items (
    collection_id TEXT NOT NULL REFERENCES music_source_collections(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    source_position INTEGER CHECK (source_position IS NULL OR source_position >= 0),
    first_discovered_at_ms INTEGER NOT NULL CHECK (first_discovered_at_ms > 0),
    last_seen_generation INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_generation >= 0),
    missing_from_latest_snapshot INTEGER NOT NULL DEFAULT 0
        CHECK (missing_from_latest_snapshot IN (0, 1)),
    PRIMARY KEY (collection_id, item_id)
);
CREATE INDEX idx_music_source_collection_items_item
    ON music_source_collection_items(item_id, collection_id);
CREATE INDEX idx_music_source_collection_items_order
    ON music_source_collection_items(collection_id, source_position, item_id);

CREATE TABLE music_playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    icon TEXT NOT NULL DEFAULT 'lucide:list-music',
    shuffle_enabled INTEGER NOT NULL DEFAULT 0 CHECK (shuffle_enabled IN (0, 1)),
    repeat_mode TEXT NOT NULL DEFAULT 'all' CHECK (repeat_mode IN ('off', 'all', 'one')),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    mix_enabled INTEGER NOT NULL DEFAULT 0 CHECK (mix_enabled IN (0, 1))
);
CREATE INDEX idx_music_playlists_sort
ON music_playlists(sort_order, name COLLATE NOCASE, id);

CREATE TABLE music_playlist_intended_uses (
    playlist_id TEXT NOT NULL REFERENCES music_playlists(id) ON DELETE CASCADE,
    intended_use TEXT NOT NULL
        CHECK (intended_use IN ('general', 'focus', 'reading', 'relaxation', 'energizing')),
    PRIMARY KEY (playlist_id, intended_use)
);

CREATE TABLE music_playlist_memberships (
    id TEXT PRIMARY KEY,
    playlist_id TEXT NOT NULL REFERENCES music_playlists(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    weight TEXT NOT NULL DEFAULT 'normal'
        CHECK (weight IN ('rarely', 'less-often', 'normal', 'more-often', 'much-more-often')),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    start_ms INTEGER CHECK (start_ms IS NULL OR start_ms >= 0),
    end_ms INTEGER CHECK (end_ms IS NULL OR end_ms >= 0),
    volume REAL CHECK (volume IS NULL OR (volume >= 0 AND volume <= 1)),
    rate REAL CHECK (rate IS NULL OR (rate >= 0.25 AND rate <= 2)),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    UNIQUE (playlist_id, item_id),
    CHECK (start_ms IS NULL OR end_ms IS NULL OR end_ms >= start_ms)
);
CREATE INDEX idx_music_playlist_memberships_eligibility
    ON music_playlist_memberships(playlist_id, enabled, weight, position);
CREATE INDEX idx_music_playlist_memberships_item
    ON music_playlist_memberships(item_id, playlist_id);
CREATE INDEX idx_music_playlist_memberships_order
    ON music_playlist_memberships(playlist_id, position, id);

CREATE TABLE music_membership_break_items (
    membership_id TEXT PRIMARY KEY REFERENCES music_playlist_memberships(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE NO ACTION,
    start_ms INTEGER CHECK (start_ms IS NULL OR start_ms >= 0),
    end_ms INTEGER CHECK (end_ms IS NULL OR end_ms >= 0),
    volume REAL CHECK (volume IS NULL OR (volume >= 0 AND volume <= 1)),
    rate REAL CHECK (rate IS NULL OR (rate >= 0.25 AND rate <= 2)),
    CHECK (start_ms IS NULL OR end_ms IS NULL OR end_ms >= start_ms)
);
CREATE INDEX idx_music_membership_break_items_item
    ON music_membership_break_items(item_id, membership_id);

CREATE TABLE music_membership_skip_ranges (
    id TEXT PRIMARY KEY,
    membership_id TEXT NOT NULL REFERENCES music_playlist_memberships(id) ON DELETE CASCADE,
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms >= start_ms),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    UNIQUE (membership_id, sort_order)
);

CREATE TABLE music_recent_selections (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    playlist_id TEXT REFERENCES music_playlists(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    selection_kind TEXT NOT NULL CHECK (selection_kind IN ('automatic', 'manual')),
    selected_at_ms INTEGER NOT NULL CHECK (selected_at_ms > 0)
);
CREATE INDEX idx_music_recent_selections_item
    ON music_recent_selections(item_id, selected_at_ms DESC, id DESC);
CREATE INDEX idx_music_recent_selections_playlist
    ON music_recent_selections(playlist_id, selected_at_ms DESC, id DESC);

CREATE TABLE music_snoozes (
    id TEXT PRIMARY KEY,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('playlist', 'all-playlists')),
    playlist_id TEXT REFERENCES music_playlists(id) ON DELETE CASCADE,
    starts_at_ms INTEGER NOT NULL CHECK (starts_at_ms > 0),
    ends_at_ms INTEGER CHECK (ends_at_ms IS NULL OR ends_at_ms > starts_at_ms),
    reason TEXT NOT NULL DEFAULT '',
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    CHECK (
        (scope = 'playlist' AND playlist_id IS NOT NULL)
        OR (scope = 'all-playlists' AND playlist_id IS NULL)
    )
);
CREATE INDEX idx_music_snoozes_active_item
    ON music_snoozes(item_id, ends_at_ms, starts_at_ms);
CREATE INDEX idx_music_snoozes_playlist
    ON music_snoozes(playlist_id, item_id, ends_at_ms);

CREATE TABLE music_soundscape_groups (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL CHECK (trim(icon) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
);

CREATE TABLE music_soundscapes (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_kind TEXT NOT NULL CHECK (source_kind IN (
            'generated-noise',
            'local-loop',
            'bundled-loop'
        )),
    generated_kind TEXT CHECK (generated_kind IN ('white', 'pink', 'brown')),
    bundled_identity TEXT,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL DEFAULT 'lucide:audio-lines' CHECK (trim(icon) <> ''),
    group_id TEXT REFERENCES music_soundscape_groups(id) ON DELETE SET NULL,
    availability TEXT NOT NULL CHECK (availability IN ('available', 'missing', 'unsupported')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    CHECK (
        (source_kind = 'generated-noise' AND generated_kind IS NOT NULL AND bundled_identity IS NULL)
        OR (source_kind = 'local-loop' AND generated_kind IS NULL AND bundled_identity IS NULL)
        OR (source_kind = 'bundled-loop' AND generated_kind IS NULL AND trim(bundled_identity) <> '')
    )
);
CREATE UNIQUE INDEX idx_music_soundscapes_generated_kind
    ON music_soundscapes(generated_kind)
    WHERE generated_kind IS NOT NULL;

CREATE TABLE music_soundscape_active_selections (
    position INTEGER PRIMARY KEY CHECK (position >= 0),
    soundscape_id TEXT NOT NULL UNIQUE REFERENCES music_soundscapes(id) ON DELETE CASCADE
);

CREATE TABLE music_soundscape_locations (
    soundscape_id TEXT NOT NULL REFERENCES music_soundscapes(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL CHECK (trim(device_id) <> ''),
    absolute_path TEXT NOT NULL CHECK (trim(absolute_path) <> ''),
    availability TEXT NOT NULL CHECK (availability IN ('available', 'missing', 'unsupported')),
    file_size_bytes INTEGER CHECK (file_size_bytes IS NULL OR file_size_bytes >= 0),
    modified_at_ms INTEGER,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    PRIMARY KEY (soundscape_id, device_id)
);
CREATE INDEX idx_music_soundscape_locations_device
    ON music_soundscape_locations(device_id, availability, soundscape_id);

CREATE TABLE music_soundscape_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_soundscape_id TEXT REFERENCES music_soundscapes(id) ON DELETE SET NULL,
    multiple_enabled INTEGER NOT NULL DEFAULT 0 CHECK (multiple_enabled IN (0, 1)),
    generated_level REAL CHECK (generated_level IS NULL OR (generated_level >= 0.0 AND generated_level <= 2.0)),
    local_level REAL CHECK (local_level IS NULL OR (local_level >= 0.0 AND local_level <= 2.0)),
    desired_playing INTEGER NOT NULL DEFAULT 0 CHECK (desired_playing IN (0, 1)),
    volume REAL NOT NULL DEFAULT 0.1 CHECK (volume >= 0.0 AND volume <= 1.0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    automatic_intent INTEGER NOT NULL DEFAULT 0 CHECK (automatic_intent IN (0, 1))
);

CREATE TABLE music_context_assignments (
    owner_kind TEXT NOT NULL CHECK (owner_kind IN (
            'project-default',
            'event-snapshot',
            'event-override',
            'work-environment'
        )),
    owner_id TEXT NOT NULL CHECK (trim(owner_id) <> ''),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short-break', 'long-break')),
    behavior TEXT NOT NULL CHECK (behavior IN (
            'inherit',
            'play-automatically',
            'prepare-silently',
            'pause-music',
            'keep-current-music'
        )),
    playlist_id TEXT,
    soundscape_id TEXT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN (
            'explicit',
            'copied-project',
            'work-environment'
        )),
    provenance_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    soundscape_behavior TEXT NOT NULL DEFAULT 'inherit'
        CHECK (soundscape_behavior IN (
            'inherit',
            'play-selected',
            'pause-soundscape',
            'keep-current-soundscape'
        )),
    PRIMARY KEY (owner_kind, owner_id, phase),
    CHECK (playlist_id IS NULL OR trim(playlist_id) <> ''),
    CHECK (soundscape_id IS NULL OR trim(soundscape_id) <> ''),
    CHECK (provenance_id IS NULL OR trim(provenance_id) <> '')
);
CREATE INDEX idx_music_context_assignments_owner
    ON music_context_assignments(owner_kind, owner_id, phase);
CREATE INDEX idx_music_context_assignments_playlist
    ON music_context_assignments(playlist_id, owner_kind, owner_id);

CREATE TABLE music_session_checkpoints (
    device_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    checkpoint_json TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE music_session_receipts (
    device_id TEXT NOT NULL,
    action_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    result_json TEXT NOT NULL,
    committed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (device_id, action_id)
);

CREATE TABLE music_transfer_receipts (
    action_id TEXT PRIMARY KEY NOT NULL,
    request_hash TEXT NOT NULL,
    result_json TEXT NOT NULL,
    committed_at_ms INTEGER NOT NULL
);

CREATE TABLE music_search_index_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    fingerprint TEXT NOT NULL,
    rebuilt_at_ms INTEGER NOT NULL CHECK (rebuilt_at_ms > 0)
);

CREATE VIRTUAL TABLE music_search_fts USING fts5(
    item_id UNINDEXED,
    title,
    artist,
    album,
    source_collections,
    relative_paths,
    signals,
    playlist_metadata,
    tokenize = 'unicode61 remove_diacritics 2'
);


-- Notes: Pages, blocks, databases, comments, assets, history, and derived indexes. Notes rows mirror the Notion object model, so content times are created_time and last_edited_time; index rows use updated_at for the time they were written.

CREATE TABLE notes_local_users (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    display_name TEXT NOT NULL CHECK (
        trim(display_name) <> ''
        AND length(display_name) <= 80
    ),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);
CREATE INDEX idx_notes_local_users_display_name
    ON notes_local_users(display_name);

CREATE TABLE notes_collaboration_operations (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE CHECK (trim(id) <> ''),
    entity_type TEXT NOT NULL CHECK (entity_type IN ('comment_thread', 'comment', 'suggestion')),
    entity_id TEXT NOT NULL CHECK (trim(entity_id) <> ''),
    operation_type TEXT NOT NULL CHECK (
        operation_type IN (
            'comment_thread_create',
            'comment_thread_resolve',
            'comment_thread_reopen',
            'comment_create',
            'comment_update',
            'comment_delete',
            'suggestion_create',
            'suggestion_accept',
            'suggestion_reject'
        )
    ),
    page_id TEXT NOT NULL CHECK (trim(page_id) <> ''),
    block_id TEXT,
    actor_id TEXT NOT NULL REFERENCES notes_local_users(id),
    actor_display_name TEXT NOT NULL CHECK (json_valid(actor_display_name)),
    base_version INTEGER NOT NULL CHECK (base_version >= 0),
    entity_version INTEGER NOT NULL CHECK (entity_version > base_version),
    conflict_policy TEXT NOT NULL CHECK (
        conflict_policy IN ('append_only', 'last_writer_wins', 'state_transition')
    ),
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    sync_state TEXT NOT NULL DEFAULT 'local' CHECK (
        sync_state IN ('local', 'exported', 'acknowledged')
    ),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_time) <> ''),
    CHECK (
        (
            entity_type = 'comment_thread'
            AND operation_type IN (
                'comment_thread_create',
                'comment_thread_resolve',
                'comment_thread_reopen'
            )
        )
        OR (
            entity_type = 'comment'
            AND operation_type IN ('comment_create', 'comment_update', 'comment_delete')
        )
        OR (
            entity_type = 'suggestion'
            AND operation_type IN (
                'suggestion_create',
                'suggestion_accept',
                'suggestion_reject'
            )
        )
    ),
    CHECK (
        (
            conflict_policy = 'append_only'
            AND operation_type IN (
                'comment_thread_create',
                'comment_create',
                'suggestion_create'
            )
        )
        OR (
            conflict_policy = 'last_writer_wins'
            AND operation_type = 'comment_update'
        )
        OR (
            conflict_policy = 'state_transition'
            AND operation_type IN (
                'comment_thread_resolve',
                'comment_thread_reopen',
                'comment_delete',
                'suggestion_accept',
                'suggestion_reject'
            )
        )
    )
);
CREATE INDEX idx_notes_collaboration_operations_entity
    ON notes_collaboration_operations(entity_type, entity_id, sequence);
CREATE INDEX idx_notes_collaboration_operations_page
    ON notes_collaboration_operations(page_id, sequence);
CREATE INDEX idx_notes_collaboration_operations_sync_state
    ON notes_collaboration_operations(sync_state, sequence);

CREATE TABLE notes_folders (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_folder_id TEXT REFERENCES notes_folders(id) ON DELETE SET NULL,
    name TEXT NOT NULL CHECK (
        trim(name) <> ''
        AND length(name) <= 200
    ),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    CHECK (parent_folder_id IS NULL OR parent_folder_id <> id)
);
CREATE INDEX idx_notes_folders_project_parent
    ON notes_folders(project_id, parent_folder_id, name COLLATE NOCASE, id);

CREATE TABLE notes_pages (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT REFERENCES "notes_pages"(id) ON DELETE CASCADE,
    parent_block_id TEXT,
    parent_data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    properties TEXT NOT NULL DEFAULT '{"title":{"id":"title","type":"title","title":[]}}' CHECK (json_valid(properties)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    url TEXT,
    public_url TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    trashed_time TEXT CHECK (trashed_time IS NULL OR trim(trashed_time) <> ''),
    folder_id TEXT REFERENCES notes_folders(id) ON DELETE SET NULL,
    CHECK (
        (
            parent_type = 'workspace'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'data_source_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_pages_active
    ON notes_pages(in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_data_source
    ON notes_pages(parent_data_source_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_folder
    ON notes_pages(folder_id, in_trash, archived, last_edited_time DESC, title);
CREATE INDEX idx_notes_pages_parent
    ON notes_pages(parent_type, parent_page_id, parent_block_id, parent_data_source_id);
CREATE INDEX idx_notes_pages_source
    ON notes_pages(source_provider, source_workspace_id, source_object_id);
CREATE INDEX idx_notes_pages_trash_retention
    ON notes_pages(in_trash, trashed_time);
CREATE INDEX idx_notes_pages_visible
    ON notes_pages(in_trash, last_edited_time DESC, title);

CREATE TABLE notes_page_aliases (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    alias TEXT NOT NULL CHECK (trim(alias) <> '' AND length(alias) <= 200),
    normalized_alias TEXT NOT NULL CHECK (trim(normalized_alias) <> ''),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);
CREATE UNIQUE INDEX idx_notes_page_aliases_normalized
    ON notes_page_aliases(normalized_alias);
CREATE INDEX idx_notes_page_aliases_page
    ON notes_page_aliases(page_id, alias COLLATE NOCASE, id);

CREATE TABLE notes_page_templates (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    source_page_id TEXT REFERENCES notes_pages(id) ON DELETE SET NULL,
    properties TEXT NOT NULL DEFAULT '{"title":{"id":"title","type":"title","title":[]}}' CHECK (json_valid(properties)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);
CREATE INDEX idx_notes_page_templates_recent ON notes_page_templates(last_edited_time DESC, name, id);
CREATE INDEX idx_notes_page_templates_source ON notes_page_templates(source_page_id);

CREATE TABLE notes_page_template_blocks (
    template_id TEXT NOT NULL REFERENCES notes_page_templates(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('template', 'block_id')),
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    plain_text TEXT NOT NULL DEFAULT '',
    sort_order REAL NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    type TEXT NOT NULL DEFAULT 'paragraph' CHECK (
        type IN (
            'paragraph',
            'heading_1',
            'heading_2',
            'heading_3',
            'heading_4',
            'heading_5',
            'heading_6',
            'bulleted_list_item',
            'numbered_list_item',
            'to_do',
            'toggle',
            'callout',
            'quote',
            'child_page',
            'child_database',
            'breadcrumb',
            'table_of_contents',
            'column_list',
            'column',
            'table',
            'table_row',
            'tab',
            'image',
            'video',
            'audio',
            'file',
            'pdf',
            'bookmark',
            'link_preview',
            'synced_block',
            'template',
            'button',
            'embed',
            'equation',
            'divider',
            'code',
            'unsupported'
        )
    ),
    PRIMARY KEY (template_id, id),
    FOREIGN KEY (template_id, parent_block_id)
        REFERENCES notes_page_template_blocks(template_id, id)
        ON DELETE CASCADE,
    CHECK (
        (
            parent_type = 'template'
            AND parent_block_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_block_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_page_template_blocks_parent
    ON notes_page_template_blocks(template_id, parent_block_id, sort_order, id);
CREATE INDEX idx_notes_page_template_blocks_root
    ON notes_page_template_blocks(template_id, parent_type, sort_order, id);

CREATE TABLE notes_edit_receipts (
    operation_id TEXT PRIMARY KEY NOT NULL,
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    request_hash TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_notes_edit_receipts_page ON notes_edit_receipts(page_id, created_at);

CREATE TABLE notes_undo_state (
    page_id TEXT PRIMARY KEY REFERENCES notes_pages(id) ON DELETE CASCADE,
    state_payload TEXT NOT NULL CHECK (length(state_payload) <= 524288),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX idx_notes_undo_state_updated_at ON notes_undo_state(updated_at);

CREATE TABLE notes_blocks (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES "notes_blocks"(id) ON DELETE CASCADE,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    plain_text TEXT NOT NULL DEFAULT '',
    sort_order REAL NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    source_provider TEXT,
    source_object_id TEXT,
    source_last_edited_time TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    type TEXT NOT NULL DEFAULT 'paragraph' CHECK (
        type IN (
            'paragraph',
            'heading_1',
            'heading_2',
            'heading_3',
            'heading_4',
            'heading_5',
            'heading_6',
            'bulleted_list_item',
            'numbered_list_item',
            'to_do',
            'toggle',
            'callout',
            'quote',
            'child_page',
            'child_database',
            'breadcrumb',
            'table_of_contents',
            'column_list',
            'column',
            'table',
            'table_row',
            'tab',
            'image',
            'video',
            'audio',
            'file',
            'pdf',
            'bookmark',
            'link_preview',
            'synced_block',
            'template',
            'button',
            'embed',
            'equation',
            'divider',
            'code',
            'unsupported'
        )
    ),
    CHECK (
        (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_page_id = page_id
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_blocks_page ON notes_blocks(page_id, in_trash, sort_order, id);
CREATE INDEX idx_notes_blocks_parent_block ON notes_blocks(parent_block_id, in_trash, sort_order, id);
CREATE INDEX idx_notes_blocks_parent_page ON notes_blocks(parent_page_id, in_trash, sort_order, id);
CREATE INDEX idx_notes_blocks_source ON notes_blocks(source_provider, source_object_id);

CREATE TABLE notes_suggestions (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL REFERENCES notes_blocks(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL REFERENCES notes_local_users(id),
    display_name TEXT NOT NULL CHECK (json_valid(display_name)),
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'accepted', 'rejected')),
    range_start INTEGER NOT NULL CHECK (range_start >= 0),
    range_end INTEGER NOT NULL CHECK (range_end > range_start),
    original_text TEXT NOT NULL CHECK (trim(original_text) <> '' AND length(original_text) <= 2000),
    proposed_text TEXT NOT NULL DEFAULT '' CHECK (length(proposed_text) <= 2000),
    prefix_text TEXT NOT NULL DEFAULT '' CHECK (length(prefix_text) <= 120),
    suffix_text TEXT NOT NULL DEFAULT '' CHECK (length(suffix_text) <= 120),
    accepted_at TEXT,
    accepted_by TEXT REFERENCES notes_local_users(id),
    rejected_at TEXT,
    rejected_by TEXT REFERENCES notes_local_users(id),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    sync_version INTEGER NOT NULL DEFAULT 1 CHECK (sync_version >= 1),
    CHECK (
        (
            status = 'open'
            AND accepted_at IS NULL
            AND accepted_by IS NULL
            AND rejected_at IS NULL
            AND rejected_by IS NULL
        )
        OR (
            status = 'accepted'
            AND accepted_at IS NOT NULL
            AND accepted_by IS NOT NULL
            AND rejected_at IS NULL
            AND rejected_by IS NULL
        )
        OR (
            status = 'rejected'
            AND rejected_at IS NOT NULL
            AND rejected_by IS NOT NULL
            AND accepted_at IS NULL
            AND accepted_by IS NULL
        )
    )
);
CREATE INDEX idx_notes_suggestions_block_range
    ON notes_suggestions(block_id, range_start, range_end, status);
CREATE INDEX idx_notes_suggestions_page
    ON notes_suggestions(page_id, status, created_time, id);

CREATE TABLE notes_databases (
    id TEXT PRIMARY KEY REFERENCES notes_blocks(id) ON DELETE CASCADE CHECK (trim(id) <> ''),
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    title_rich_text TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(title_rich_text)),
    description TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(description)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    is_inline INTEGER NOT NULL DEFAULT 1 CHECK (is_inline IN (0, 1)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    url TEXT,
    public_url TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    CHECK (
        (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_databases_parent ON notes_databases(parent_type, parent_page_id, parent_block_id);
CREATE INDEX idx_notes_databases_source ON notes_databases(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_data_sources (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    database_id TEXT NOT NULL REFERENCES notes_databases(id) ON DELETE CASCADE,
    title TEXT NOT NULL DEFAULT '',
    title_rich_text TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(title_rich_text)),
    description TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(description)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    properties TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(properties)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);
CREATE INDEX idx_notes_data_sources_database ON notes_data_sources(database_id, in_trash, title);
CREATE INDEX idx_notes_data_sources_source ON notes_data_sources(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_data_source_relation_links (
    source_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    source_data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    source_property_id TEXT NOT NULL CHECK (trim(source_property_id) <> ''),
    source_property_name TEXT NOT NULL DEFAULT '',
    target_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    target_data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    PRIMARY KEY (source_page_id, source_property_id, target_page_id)
);
CREATE INDEX idx_notes_data_source_relation_links_source
    ON notes_data_source_relation_links(source_data_source_id, source_property_id, source_page_id);
CREATE INDEX idx_notes_data_source_relation_links_target
    ON notes_data_source_relation_links(target_page_id, source_page_id);
CREATE INDEX idx_notes_data_source_relation_links_target_data_source
    ON notes_data_source_relation_links(target_data_source_id, target_page_id);

CREATE TABLE notes_data_source_rollup_cache (
    source_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    source_data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    source_property_id TEXT NOT NULL,
    source_property_name TEXT NOT NULL,
    relation_property_id TEXT NOT NULL,
    rollup_property_id TEXT NOT NULL,
    target_data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    value TEXT NOT NULL,
    computed_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (source_page_id, source_property_id)
);
CREATE INDEX idx_notes_data_source_rollup_cache_source_data_source
    ON notes_data_source_rollup_cache(source_data_source_id);
CREATE INDEX idx_notes_data_source_rollup_cache_target_data_source
    ON notes_data_source_rollup_cache(target_data_source_id);

CREATE TABLE notes_data_source_row_hierarchy (
    row_page_id TEXT PRIMARY KEY REFERENCES notes_pages(id) ON DELETE CASCADE,
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    parent_row_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    CHECK (row_page_id <> parent_row_page_id)
);
CREATE INDEX idx_notes_data_source_row_hierarchy_parent
    ON notes_data_source_row_hierarchy(data_source_id, parent_row_page_id, row_page_id);

CREATE TABLE notes_data_source_templates (
    id TEXT PRIMARY KEY,
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    source_page_id TEXT REFERENCES notes_pages(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    properties TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE UNIQUE INDEX idx_notes_data_source_templates_default
    ON notes_data_source_templates(data_source_id)
    WHERE is_default = 1;
CREATE INDEX idx_notes_data_source_templates_source
    ON notes_data_source_templates(data_source_id, last_edited_time DESC, name);

CREATE TABLE notes_data_source_template_blocks (
    template_id TEXT NOT NULL REFERENCES notes_data_source_templates(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    parent_type TEXT NOT NULL CHECK (parent_type IN ('template', 'block_id')),
    parent_block_id TEXT,
    has_children INTEGER NOT NULL DEFAULT 0 CHECK (has_children IN (0, 1)),
    type TEXT NOT NULL,
    payload TEXT NOT NULL,
    plain_text TEXT NOT NULL DEFAULT '',
    sort_order REAL NOT NULL,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (template_id, id)
);
CREATE INDEX idx_notes_data_source_template_blocks_parent
    ON notes_data_source_template_blocks(template_id, parent_type, parent_block_id, sort_order, id);

CREATE TABLE notes_database_views (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    database_id TEXT NOT NULL REFERENCES notes_databases(id) ON DELETE CASCADE,
    data_source_id TEXT NOT NULL REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    name TEXT NOT NULL DEFAULT '',
    type TEXT NOT NULL CHECK (
        type IN (
            'table',
            'board',
            'list',
            'calendar',
            'timeline',
            'gallery',
            'form',
            'chart',
            'map',
            'dashboard'
        )
    ),
    filter TEXT CHECK (filter IS NULL OR json_valid(filter)),
    sorts TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(sorts)),
    configuration TEXT CHECK (configuration IS NULL OR json_valid(configuration)),
    sort_order REAL NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    url TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> '')
);
CREATE INDEX idx_notes_database_views_data_source ON notes_database_views(data_source_id, sort_order, id);
CREATE INDEX idx_notes_database_views_database ON notes_database_views(database_id, sort_order, id);
CREATE INDEX idx_notes_database_views_source ON notes_database_views(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_comment_threads (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_type TEXT NOT NULL CHECK (parent_type IN ('page_id', 'block_id')),
    parent_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'resolved')),
    resolved_at TEXT,
    resolved_by TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    sync_version INTEGER NOT NULL DEFAULT 1 CHECK (sync_version >= 1),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT,
    CHECK (
        (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_page_id = page_id
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
        )
    ),
    CHECK (
        (
            status = 'open'
            AND resolved_at IS NULL
            AND resolved_by IS NULL
        )
        OR (
            status = 'resolved'
            AND resolved_at IS NOT NULL
            AND resolved_by IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_comment_threads_page ON notes_comment_threads(page_id, status, created_time, id);
CREATE INDEX idx_notes_comment_threads_parent_block ON notes_comment_threads(parent_block_id, status, created_time, id);
CREATE INDEX idx_notes_comment_threads_source
    ON notes_comment_threads(source_provider, source_workspace_id, source_object_id);

CREATE TABLE notes_comment_thread_anchors (
    thread_id TEXT PRIMARY KEY
        REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    page_id TEXT NOT NULL
        REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT NOT NULL
        REFERENCES notes_blocks(id) ON DELETE CASCADE,
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    anchor_text TEXT NOT NULL CHECK (trim(anchor_text) <> ''),
    prefix_text TEXT NOT NULL DEFAULT '',
    suffix_text TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (length(anchor_text) <= 2000),
    CHECK (length(prefix_text) <= 120),
    CHECK (length(suffix_text) <= 120)
);
CREATE INDEX idx_notes_comment_thread_anchors_block
ON notes_comment_thread_anchors(block_id, start_offset, end_offset);
CREATE INDEX idx_notes_comment_thread_anchors_page
ON notes_comment_thread_anchors(page_id, block_id);

CREATE TABLE notes_comment_thread_reads (
    thread_id TEXT NOT NULL REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES notes_local_users(id) ON DELETE CASCADE,
    read_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(read_at) <> ''),
    PRIMARY KEY (thread_id, user_id)
);
CREATE INDEX idx_notes_comment_thread_reads_user
    ON notes_comment_thread_reads(user_id, read_at DESC, thread_id);

CREATE TABLE notes_comments (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    thread_id TEXT NOT NULL REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    rich_text TEXT NOT NULL CHECK (json_valid(rich_text)),
    plain_text TEXT NOT NULL DEFAULT '',
    created_by TEXT NOT NULL DEFAULT 'local-user' CHECK (trim(created_by) <> ''),
    display_name TEXT NOT NULL DEFAULT '{"type":"user","resolved_name":"You"}' CHECK (json_valid(display_name)),
    attachments TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(attachments)),
    deleted_at TEXT,
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(last_edited_time) <> ''),
    sync_version INTEGER NOT NULL DEFAULT 1 CHECK (sync_version >= 1),
    source_provider TEXT,
    source_object_id TEXT,
    source_workspace_id TEXT,
    source_last_edited_time TEXT
);
CREATE INDEX idx_notes_comments_source
    ON notes_comments(source_provider, source_workspace_id, source_object_id);
CREATE INDEX idx_notes_comments_thread ON notes_comments(thread_id, deleted_at, created_time, id);

CREATE TABLE notes_backlink_index (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    target_type TEXT NOT NULL CHECK (
        target_type IN ('page', 'database', 'local_object', 'alias', 'external_url')
    ),
    target_id TEXT NOT NULL CHECK (trim(target_id) <> ''),
    target_object_type TEXT,
    source_type TEXT NOT NULL CHECK (
        source_type IN ('block', 'comment', 'database_relation', 'alias')
    ),
    source_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    source_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    source_comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    source_property_id TEXT,
    source_property_name TEXT NOT NULL DEFAULT '',
    reference_type TEXT NOT NULL CHECK (
        reference_type IN (
            'child_page',
            'page_mention',
            'link',
            'database_relation',
            'comment_mention',
            'comment_link',
            'database_mention',
            'local_object_mention',
            'alias'
        )
    ),
    snippet TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL CHECK (trim(last_edited_time) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    CHECK (
        (
            target_type = 'local_object'
            AND target_object_type IS NOT NULL
            AND trim(target_object_type) <> ''
        )
        OR (
            target_type != 'local_object'
            AND target_object_type IS NULL
        )
    ),
    CHECK (
        (
            source_type = 'block'
            AND source_block_id IS NOT NULL
            AND source_comment_id IS NULL
        )
        OR (
            source_type = 'comment'
            AND source_comment_id IS NOT NULL
        )
        OR (
            source_type = 'database_relation'
            AND source_block_id IS NULL
            AND source_comment_id IS NULL
            AND source_property_id IS NOT NULL
            AND trim(source_property_id) <> ''
        )
        OR (
            source_type = 'alias'
            AND source_block_id IS NULL
            AND source_comment_id IS NULL
        )
    )
);
CREATE INDEX idx_notes_backlink_index_source_block
    ON notes_backlink_index(source_block_id, reference_type, id)
    WHERE source_block_id IS NOT NULL;
CREATE INDEX idx_notes_backlink_index_source_comment
    ON notes_backlink_index(source_comment_id, reference_type, id)
    WHERE source_comment_id IS NOT NULL;
CREATE INDEX idx_notes_backlink_index_source_page
    ON notes_backlink_index(source_page_id, source_type, last_edited_time DESC, id);
CREATE INDEX idx_notes_backlink_index_target
    ON notes_backlink_index(target_type, target_id, last_edited_time DESC, id);

CREATE TABLE notes_mention_notifications (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('block', 'comment')),
    source_id TEXT NOT NULL CHECK (trim(source_id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('reminder', 'user_mention', 'task_mention')),
    target_type TEXT NOT NULL CHECK (target_type IN ('date', 'user', 'project_task')),
    target_id TEXT,
    trigger_at TEXT,
    plain_text TEXT NOT NULL DEFAULT '' CHECK (length(plain_text) <= 500),
    source_plain_text TEXT NOT NULL DEFAULT '' CHECK (length(source_plain_text) <= 2000),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'delivered', 'dismissed')),
    delivered_at TEXT,
    fingerprint TEXT NOT NULL CHECK (trim(fingerprint) <> ''),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_edited_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    suppressed_by_history_restore INTEGER NOT NULL DEFAULT 0 CHECK (
        suppressed_by_history_restore IN (0, 1)
    ),
    CHECK (
        (source_type = 'block' AND block_id = source_id AND comment_id IS NULL)
        OR (source_type = 'comment' AND comment_id = source_id)
    ),
    CHECK (
        (kind = 'reminder' AND target_type = 'date' AND trigger_at IS NOT NULL)
        OR (kind = 'user_mention' AND target_type = 'user' AND target_id IS NOT NULL)
        OR (kind = 'task_mention' AND target_type = 'project_task' AND target_id IS NOT NULL)
    ),
    UNIQUE (source_type, source_id, fingerprint)
);
CREATE INDEX idx_notes_mention_notifications_page
    ON notes_mention_notifications(page_id, status, created_time, id);
CREATE INDEX idx_notes_mention_notifications_source
    ON notes_mention_notifications(source_type, source_id);
CREATE INDEX idx_notes_mention_notifications_status
    ON notes_mention_notifications(status, kind, trigger_at, created_time, id);

CREATE TABLE notes_search_index (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_type TEXT NOT NULL CHECK (
        source_type IN ('page', 'block', 'comment', 'property', 'file', 'alias', 'metadata')
    ),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    property_id TEXT,
    block_type TEXT,
    title TEXT NOT NULL DEFAULT '',
    body TEXT NOT NULL DEFAULT '',
    metadata TEXT NOT NULL DEFAULT '',
    source_last_edited_time TEXT NOT NULL CHECK (trim(source_last_edited_time) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_notes_search_index_block
    ON notes_search_index(block_id, source_last_edited_time DESC, id);
CREATE INDEX idx_notes_search_index_comment
    ON notes_search_index(comment_id, source_last_edited_time DESC, id);
CREATE INDEX idx_notes_search_index_page
    ON notes_search_index(page_id, source_type, source_last_edited_time DESC, id);

CREATE VIRTUAL TABLE notes_search_fts USING fts5(
    index_id UNINDEXED,
    title,
    body,
    metadata,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE notes_unresolved_link_index (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('block', 'comment')),
    source_page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    source_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    source_comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    raw_url TEXT NOT NULL CHECK (trim(raw_url) <> ''),
    raw_target TEXT NOT NULL CHECK (trim(raw_target) <> ''),
    normalized_target TEXT NOT NULL CHECK (trim(normalized_target) <> ''),
    link_text TEXT NOT NULL DEFAULT '',
    snippet TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL CHECK (trim(last_edited_time) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    CHECK (
        (
            source_type = 'block'
            AND source_block_id IS NOT NULL
            AND source_comment_id IS NULL
        )
        OR (
            source_type = 'comment'
            AND source_comment_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_unresolved_link_index_block
    ON notes_unresolved_link_index(source_block_id, id)
    WHERE source_block_id IS NOT NULL;
CREATE INDEX idx_notes_unresolved_link_index_comment
    ON notes_unresolved_link_index(source_comment_id, id)
    WHERE source_comment_id IS NOT NULL;
CREATE INDEX idx_notes_unresolved_link_index_page
    ON notes_unresolved_link_index(source_page_id, last_edited_time DESC, id);
CREATE INDEX idx_notes_unresolved_link_index_target
    ON notes_unresolved_link_index(normalized_target, source_page_id, id);

CREATE TABLE notes_assets (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    asset_path TEXT NOT NULL UNIQUE CHECK (
        trim(asset_path) <> ''
        AND instr(asset_path, '..') = 0
        AND instr(asset_path, '\') = 0
        AND (
            (
                asset_path GLOB 'notes/page-icons/*'
                AND instr(substr(asset_path, length('notes/page-icons/') + 1), '/') = 0
                AND (
                    lower(asset_path) GLOB '*.png'
                    OR lower(asset_path) GLOB '*.jpg'
                    OR lower(asset_path) GLOB '*.jpeg'
                    OR lower(asset_path) GLOB '*.webp'
                )
            )
            OR (
                asset_path GLOB 'notes/page-covers/*'
                AND instr(substr(asset_path, length('notes/page-covers/') + 1), '/') = 0
                AND (
                    lower(asset_path) GLOB '*.png'
                    OR lower(asset_path) GLOB '*.jpg'
                    OR lower(asset_path) GLOB '*.jpeg'
                    OR lower(asset_path) GLOB '*.webp'
                )
            )
            OR (
                asset_path GLOB 'notes/files/*'
                AND instr(substr(asset_path, length('notes/files/') + 1), '/') = 0
            )
        )
    ),
    kind TEXT NOT NULL CHECK (kind IN ('image', 'video', 'audio', 'pdf', 'file')),
    source_type TEXT NOT NULL CHECK (
        source_type IN ('local_upload', 'generated', 'imported', 'external_reference')
    ),
    original_name TEXT,
    content_type TEXT NOT NULL CHECK (
        trim(content_type) <> ''
        AND instr(content_type, '/') > 1
        AND instr(content_type, ' ') = 0
    ),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    sha256 TEXT NOT NULL CHECK (
        length(sha256) = 64
        AND sha256 = lower(sha256)
        AND sha256 NOT GLOB '*[^0-9a-f]*'
    ),
    storage_state TEXT NOT NULL DEFAULT 'available' CHECK (storage_state IN ('available', 'missing')),
    missing_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    CHECK (
        (
            storage_state = 'available'
            AND missing_at IS NULL
        )
        OR (
            storage_state = 'missing'
            AND missing_at IS NOT NULL
            AND trim(missing_at) <> ''
        )
    ),
    CHECK (
        (
            kind = 'image'
            AND content_type IN ('image/png', 'image/jpeg', 'image/webp')
        )
        OR (
            kind = 'video'
            AND content_type GLOB 'video/*'
        )
        OR (
            kind = 'audio'
            AND content_type GLOB 'audio/*'
        )
        OR (
            kind = 'pdf'
            AND content_type = 'application/pdf'
        )
        OR kind = 'file'
    )
);
CREATE INDEX idx_notes_assets_hash ON notes_assets(sha256, byte_size, content_type);
CREATE INDEX idx_notes_assets_path ON notes_assets(asset_path);
CREATE INDEX idx_notes_assets_state ON notes_assets(storage_state, updated_at DESC, id);

CREATE TABLE notes_asset_references (
    asset_id TEXT NOT NULL REFERENCES notes_assets(id) ON DELETE CASCADE,
    owner_type TEXT NOT NULL CHECK (
        owner_type IN ('page', 'block', 'data_source_property', 'comment', 'import')
    ),
    owner_id TEXT NOT NULL CHECK (trim(owner_id) <> ''),
    page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    property_id TEXT,
    role TEXT NOT NULL CHECK (
        role IN (
            'page_icon',
            'page_cover',
            'block_file',
            'callout_icon',
            'property_file',
            'comment_attachment',
            'import_source'
        )
    ),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (asset_id, owner_type, owner_id, role),
    CHECK (
        (
            owner_type = 'page'
            AND role IN ('page_icon', 'page_cover')
            AND page_id = owner_id
            AND block_id IS NULL
            AND data_source_id IS NULL
            AND comment_id IS NULL
            AND property_id IS NULL
        )
        OR (
            owner_type = 'block'
            AND role IN ('block_file', 'callout_icon')
            AND page_id IS NOT NULL
            AND block_id = owner_id
            AND data_source_id IS NULL
            AND comment_id IS NULL
            AND property_id IS NULL
        )
        OR (
            owner_type = 'data_source_property'
            AND role = 'property_file'
            AND page_id IS NULL
            AND block_id IS NULL
            AND data_source_id = owner_id
            AND comment_id IS NULL
            AND property_id IS NOT NULL
            AND trim(property_id) <> ''
        )
        OR (
            owner_type = 'comment'
            AND role = 'comment_attachment'
            AND page_id IS NOT NULL
            AND block_id IS NULL
            AND data_source_id IS NULL
            AND comment_id = owner_id
            AND property_id IS NULL
        )
        OR (
            owner_type = 'import'
            AND role = 'import_source'
            AND page_id IS NULL
            AND block_id IS NULL
            AND data_source_id IS NULL
            AND comment_id IS NULL
            AND property_id IS NULL
        )
    )
);
CREATE INDEX idx_notes_asset_references_block
    ON notes_asset_references(block_id, role, asset_id)
    WHERE block_id IS NOT NULL;
CREATE INDEX idx_notes_asset_references_comment
    ON notes_asset_references(comment_id, asset_id)
    WHERE comment_id IS NOT NULL;
CREATE INDEX idx_notes_asset_references_data_source
    ON notes_asset_references(data_source_id, property_id, asset_id)
    WHERE data_source_id IS NOT NULL;
CREATE INDEX idx_notes_asset_references_owner
    ON notes_asset_references(owner_type, owner_id, role, asset_id);
CREATE INDEX idx_notes_asset_references_page
    ON notes_asset_references(page_id, role, asset_id)
    WHERE page_id IS NOT NULL;

CREATE TABLE notes_link_facts (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_object_type TEXT NOT NULL CHECK (
        source_object_type IN ('page', 'database_row', 'block', 'property', 'comment', 'import')
    ),
    source_object_id TEXT NOT NULL CHECK (trim(source_object_id) <> ''),
    source_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    source_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    source_comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    source_data_source_id TEXT REFERENCES notes_data_sources(id) ON DELETE CASCADE,
    source_property_id TEXT,
    source_property_name TEXT NOT NULL DEFAULT '',
    target_object_type TEXT NOT NULL CHECK (
        target_object_type IN (
            'page',
            'database_row',
            'block',
            'database',
            'data_source',
            'property',
            'comment',
            'file',
            'project',
            'project_task',
            'calendar_event',
            'pomodoro_run',
            'music_item',
            'external_url'
        )
    ),
    target_object_id TEXT NOT NULL CHECK (trim(target_object_id) <> ''),
    target_page_id TEXT REFERENCES notes_pages(id) ON DELETE CASCADE,
    target_block_id TEXT REFERENCES notes_blocks(id) ON DELETE CASCADE,
    target_comment_id TEXT REFERENCES notes_comments(id) ON DELETE CASCADE,
    target_asset_id TEXT REFERENCES notes_assets(id) ON DELETE CASCADE,
    target_url TEXT,
    link_type TEXT NOT NULL CHECK (
        link_type IN (
            'child_page',
            'page_mention',
            'page_link',
            'block_link',
            'database_mention',
            'database_relation',
            'local_object_mention',
            'external_url',
            'page_icon',
            'page_cover',
            'block_file',
            'callout_icon',
            'property_file',
            'comment_attachment',
            'import_source'
        )
    ),
    snippet TEXT NOT NULL DEFAULT '',
    created_time TEXT NOT NULL CHECK (trim(created_time) <> ''),
    last_edited_time TEXT NOT NULL CHECK (trim(last_edited_time) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    CHECK (
        (
            source_object_type IN ('page', 'database_row')
            AND source_page_id IS NOT NULL
            AND source_page_id = source_object_id
            AND source_block_id IS NULL
            AND source_comment_id IS NULL
        )
        OR (
            source_object_type = 'block'
            AND source_block_id IS NOT NULL
            AND source_block_id = source_object_id
            AND source_page_id IS NOT NULL
            AND source_comment_id IS NULL
        )
        OR (
            source_object_type = 'comment'
            AND source_comment_id IS NOT NULL
            AND source_comment_id = source_object_id
            AND source_page_id IS NOT NULL
        )
        OR (
            source_object_type = 'property'
            AND source_data_source_id IS NOT NULL
            AND source_property_id IS NOT NULL
            AND trim(source_property_id) <> ''
        )
        OR (
            source_object_type = 'import'
            AND source_page_id IS NULL
            AND source_block_id IS NULL
            AND source_comment_id IS NULL
        )
    ),
    CHECK (
        (
            target_object_type IN ('page', 'database_row')
            AND target_page_id IS NOT NULL
            AND target_page_id = target_object_id
        )
        OR (
            target_object_type = 'block'
            AND target_block_id IS NOT NULL
            AND target_block_id = target_object_id
        )
        OR (
            target_object_type = 'comment'
            AND target_comment_id IS NOT NULL
            AND target_comment_id = target_object_id
        )
        OR (
            target_object_type = 'file'
            AND target_asset_id IS NOT NULL
            AND target_asset_id = target_object_id
        )
        OR (
            target_object_type = 'external_url'
            AND target_url IS NOT NULL
            AND target_url = target_object_id
        )
        OR (
            target_object_type NOT IN ('page', 'database_row', 'block', 'comment', 'file', 'external_url')
        )
    )
);
CREATE INDEX idx_notes_link_facts_source
    ON notes_link_facts(source_object_type, source_object_id, link_type, id);
CREATE INDEX idx_notes_link_facts_source_block
    ON notes_link_facts(source_block_id, link_type, target_object_type, id)
    WHERE source_block_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_source_comment
    ON notes_link_facts(source_comment_id, link_type, target_object_type, id)
    WHERE source_comment_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_source_page
    ON notes_link_facts(source_page_id, link_type, target_object_type, id)
    WHERE source_page_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_source_property
    ON notes_link_facts(source_data_source_id, source_property_id, target_object_type, id)
    WHERE source_data_source_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_target
    ON notes_link_facts(target_object_type, target_object_id, link_type, id);
CREATE INDEX idx_notes_link_facts_target_asset
    ON notes_link_facts(target_asset_id, source_object_type, link_type, id)
    WHERE target_asset_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_target_block
    ON notes_link_facts(target_block_id, source_object_type, link_type, id)
    WHERE target_block_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_target_page
    ON notes_link_facts(target_page_id, source_object_type, link_type, id)
    WHERE target_page_id IS NOT NULL;
CREATE INDEX idx_notes_link_facts_target_url
    ON notes_link_facts(target_url, source_object_type, id)
    WHERE target_url IS NOT NULL;

CREATE TABLE notes_history_bundles (
    hash TEXT PRIMARY KEY CHECK (
        length(hash) = 64
        AND hash = lower(hash)
        AND hash NOT GLOB '*[^0-9a-f]*'
    ),
    kind TEXT NOT NULL CHECK (kind IN ('row', 'manifest', 'chunk')),
    encoding TEXT NOT NULL CHECK (
        encoding IN (
            'raw-json-v1',
            'zlib-json-v1',
            'raw-bytes-v1',
            'zlib-bytes-v1',
            'chunked-json-v1'
        )
    ),
    payload BLOB NOT NULL,
    uncompressed_bytes INTEGER NOT NULL CHECK (uncompressed_bytes >= 0),
    stored_bytes INTEGER NOT NULL CHECK (stored_bytes >= 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_at) <> ''),
    CHECK (length(payload) = stored_bytes)
);
CREATE INDEX idx_notes_history_bundles_created
    ON notes_history_bundles(created_at, hash);

CREATE TABLE notes_history_bundle_chunks (
    parent_hash TEXT NOT NULL REFERENCES notes_history_bundles(hash) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
    chunk_hash TEXT NOT NULL REFERENCES notes_history_bundles(hash) ON DELETE NO ACTION,
    PRIMARY KEY (parent_hash, chunk_index),
    UNIQUE (parent_hash, chunk_hash)
);
CREATE INDEX idx_notes_history_bundle_chunks_chunk
    ON notes_history_bundle_chunks(chunk_hash, parent_hash);

CREATE TABLE notes_page_history_snapshots (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    parent_type TEXT NOT NULL CHECK (parent_type IN ('workspace', 'page_id', 'block_id', 'data_source_id')),
    parent_page_id TEXT,
    parent_block_id TEXT,
    parent_data_source_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    properties TEXT NOT NULL CHECK (json_valid(properties)),
    icon TEXT CHECK (icon IS NULL OR json_valid(icon)),
    cover TEXT CHECK (cover IS NULL OR json_valid(cover)),
    in_trash INTEGER NOT NULL DEFAULT 0 CHECK (in_trash IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    blocks TEXT NOT NULL CHECK (json_valid(blocks)),
    block_count INTEGER NOT NULL DEFAULT 0 CHECK (block_count >= 0),
    reason TEXT NOT NULL CHECK (trim(reason) <> ''),
    created_by TEXT NOT NULL REFERENCES notes_local_users(id),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_time) <> ''),
    page_created_time TEXT NOT NULL CHECK (trim(page_created_time) <> ''),
    page_last_edited_time TEXT NOT NULL CHECK (trim(page_last_edited_time) <> ''),
    block_bundle_hash TEXT REFERENCES notes_history_bundles(hash) ON DELETE NO ACTION,
    folder_id TEXT,
    CHECK (
        (
            parent_type = 'workspace'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'page_id'
            AND parent_page_id IS NOT NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'block_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NOT NULL
            AND parent_data_source_id IS NULL
        )
        OR (
            parent_type = 'data_source_id'
            AND parent_page_id IS NULL
            AND parent_block_id IS NULL
            AND parent_data_source_id IS NOT NULL
        )
    )
);
CREATE INDEX idx_notes_page_history_snapshots_bundle
    ON notes_page_history_snapshots(block_bundle_hash)
    WHERE block_bundle_hash IS NOT NULL;
CREATE INDEX idx_notes_page_history_snapshots_created
    ON notes_page_history_snapshots(created_time);
CREATE INDEX idx_notes_page_history_snapshots_created_by
    ON notes_page_history_snapshots(created_by, created_time DESC, id);
CREATE INDEX idx_notes_page_history_snapshots_page
    ON notes_page_history_snapshots(page_id, created_time DESC, id);

CREATE TABLE notes_page_history_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    retention_days INTEGER NOT NULL CHECK (
        retention_days IN (0, 7, 30, 90, 180, 365)
    ),
    updated_at TEXT NOT NULL DEFAULT (
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    )
);

CREATE TABLE notes_history_maintenance_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_run_at TEXT CHECK (last_run_at IS NULL OR trim(last_run_at) <> '')
);

CREATE TABLE notes_project_history_versions (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    manifest_hash TEXT NOT NULL REFERENCES notes_history_bundles(hash) ON DELETE NO ACTION,
    reason TEXT NOT NULL CHECK (trim(reason) <> '' AND length(reason) <= 80),
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    display_name TEXT NOT NULL CHECK (json_valid(display_name)),
    changed_note_summary TEXT NOT NULL DEFAULT '',
    page_count INTEGER NOT NULL DEFAULT 0 CHECK (page_count >= 0),
    active_page_count INTEGER NOT NULL DEFAULT 0 CHECK (active_page_count >= 0),
    archived_page_count INTEGER NOT NULL DEFAULT 0 CHECK (archived_page_count >= 0),
    deleted_page_count INTEGER NOT NULL DEFAULT 0 CHECK (deleted_page_count >= 0),
    manifest_uncompressed_bytes INTEGER NOT NULL DEFAULT 0
        CHECK (manifest_uncompressed_bytes >= 0),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_time) <> '')
);
CREATE INDEX idx_notes_project_history_versions_project
    ON notes_project_history_versions(project_id, created_time DESC, id DESC);

CREATE TABLE notes_project_history_asset_pins (
    version_id TEXT NOT NULL REFERENCES notes_project_history_versions(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES notes_assets(id) ON DELETE NO ACTION,
    PRIMARY KEY (version_id, asset_id)
);
CREATE INDEX idx_notes_project_history_asset_pins_asset
    ON notes_project_history_asset_pins(asset_id, version_id);

CREATE TABLE notes_project_history_bundle_references (
    version_id TEXT NOT NULL REFERENCES notes_project_history_versions(id) ON DELETE CASCADE,
    bundle_hash TEXT NOT NULL REFERENCES notes_history_bundles(hash) ON DELETE NO ACTION,
    PRIMARY KEY (version_id, bundle_hash)
);
CREATE INDEX idx_notes_project_history_bundle_references_hash
    ON notes_project_history_bundle_references(bundle_hash, version_id);

CREATE TABLE notes_backlink_index_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);

CREATE TABLE notes_link_facts_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);

CREATE TABLE notes_page_cover_assets (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    asset_path TEXT NOT NULL UNIQUE CHECK (
        trim(asset_path) <> ''
        AND asset_path GLOB 'notes/page-covers/*'
        AND instr(substr(asset_path, length('notes/page-covers/') + 1), '/') = 0
        AND instr(asset_path, '..') = 0
        AND instr(asset_path, '\') = 0
        AND (
            lower(asset_path) GLOB '*.png'
            OR lower(asset_path) GLOB '*.jpg'
            OR lower(asset_path) GLOB '*.jpeg'
            OR lower(asset_path) GLOB '*.webp'
        )
    ),
    original_name TEXT,
    content_type TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_notes_page_cover_assets_recent ON notes_page_cover_assets(updated_at DESC, id);

CREATE TABLE notes_page_icon_assets (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    asset_path TEXT NOT NULL UNIQUE CHECK (
        trim(asset_path) <> ''
        AND asset_path GLOB 'notes/page-icons/*'
        AND instr(substr(asset_path, length('notes/page-icons/') + 1), '/') = 0
        AND instr(asset_path, '..') = 0
        AND instr(asset_path, '\') = 0
        AND (
            lower(asset_path) GLOB '*.png'
            OR lower(asset_path) GLOB '*.jpg'
            OR lower(asset_path) GLOB '*.jpeg'
            OR lower(asset_path) GLOB '*.webp'
        )
    ),
    original_name TEXT,
    content_type TEXT NOT NULL CHECK (content_type IN ('image/png', 'image/jpeg', 'image/webp')),
    byte_size INTEGER NOT NULL CHECK (byte_size > 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_notes_page_icon_assets_recent ON notes_page_icon_assets(updated_at DESC, id);

CREATE TABLE notes_project_history_dirty (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    first_dirty_at TEXT NOT NULL CHECK (trim(first_dirty_at) <> ''),
    last_dirty_at TEXT NOT NULL CHECK (trim(last_dirty_at) <> ''),
    actor_id TEXT NOT NULL CHECK (trim(actor_id) <> ''),
    actor_display_name TEXT NOT NULL CHECK (json_valid(actor_display_name)),
    changed_note_summary TEXT NOT NULL DEFAULT '',
    force_checkpoint INTEGER NOT NULL DEFAULT 0 CHECK (force_checkpoint IN (0, 1))
);
CREATE INDEX idx_notes_project_history_dirty_deadlines
    ON notes_project_history_dirty(force_checkpoint, first_dirty_at, last_dirty_at, project_id);
CREATE INDEX idx_notes_project_history_dirty_due
    ON notes_project_history_dirty(force_checkpoint DESC, last_dirty_at, first_dirty_at, project_id);

CREATE TABLE notes_search_index_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);

CREATE TABLE notes_unresolved_link_index_state (
    key TEXT PRIMARY KEY CHECK (trim(key) <> ''),
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);

-- Notes triggers.

CREATE TRIGGER notes_folders_validate_parent_insert
BEFORE INSERT ON notes_folders
WHEN NEW.parent_folder_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.parent_folder_id = NEW.id
        THEN RAISE(ABORT, 'notes folder cannot parent itself')
    END;
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM notes_folders AS parent
            WHERE parent.id = NEW.parent_folder_id
              AND parent.project_id = NEW.project_id
        )
        THEN RAISE(ABORT, 'notes folder parent must belong to the same project')
    END;
END;

CREATE TRIGGER notes_folders_validate_parent_update
BEFORE UPDATE OF parent_folder_id, project_id ON notes_folders
WHEN NEW.parent_folder_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.parent_folder_id = NEW.id
        THEN RAISE(ABORT, 'notes folder cannot parent itself')
    END;
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM notes_folders AS parent
            WHERE parent.id = NEW.parent_folder_id
              AND parent.project_id = NEW.project_id
        )
        THEN RAISE(ABORT, 'notes folder parent must belong to the same project')
    END;
    SELECT CASE
        WHEN EXISTS (
            WITH RECURSIVE descendants(id) AS (
                SELECT id
                FROM notes_folders
                WHERE parent_folder_id = OLD.id
                UNION
                SELECT child.id
                FROM notes_folders AS child
                JOIN descendants AS parent ON child.parent_folder_id = parent.id
            )
            SELECT 1
            FROM descendants
            WHERE id = NEW.parent_folder_id
        )
        THEN RAISE(ABORT, 'notes folder cannot be moved under its descendant')
    END;
END;

CREATE TRIGGER notes_folders_validate_project_update
BEFORE UPDATE OF project_id ON notes_folders
BEGIN
    SELECT CASE
        WHEN EXISTS (
            SELECT 1
            FROM notes_folders AS child
            WHERE child.parent_folder_id = OLD.id
              AND child.project_id <> NEW.project_id
        )
        THEN RAISE(ABORT, 'notes folder children must belong to the same project')
    END;
    SELECT CASE
        WHEN EXISTS (
            SELECT 1
            FROM notes_pages AS page
            WHERE page.folder_id = OLD.id
              AND (
                  json_type(page.properties, '$.__ganbaru_project_id') <> 'text'
                  OR trim(json_extract(page.properties, '$.__ganbaru_project_id')) <> NEW.project_id
              )
        )
        THEN RAISE(ABORT, 'notes folder pages must belong to the same project')
    END;
END;

CREATE TRIGGER notes_pages_validate_folder_insert
BEFORE INSERT ON notes_pages
WHEN NEW.folder_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.parent_type <> 'workspace'
        THEN RAISE(ABORT, 'notes folder pages must have a workspace parent')
    END;
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM notes_folders AS folder
            WHERE folder.id = NEW.folder_id
              AND json_type(NEW.properties, '$.__ganbaru_project_id') = 'text'
              AND folder.project_id = trim(json_extract(
                  NEW.properties,
                  '$.__ganbaru_project_id'
              ))
        )
        THEN RAISE(ABORT, 'notes folder page must belong to the same project')
    END;
END;

CREATE TRIGGER notes_pages_validate_folder_update
BEFORE UPDATE OF folder_id, parent_type, parent_page_id, parent_block_id, parent_data_source_id, properties
ON notes_pages
WHEN NEW.folder_id IS NOT NULL
BEGIN
    SELECT CASE
        WHEN NEW.parent_type <> 'workspace'
        THEN RAISE(ABORT, 'notes folder pages must have a workspace parent')
    END;
    SELECT CASE
        WHEN NOT EXISTS (
            SELECT 1
            FROM notes_folders AS folder
            WHERE folder.id = NEW.folder_id
              AND json_type(NEW.properties, '$.__ganbaru_project_id') = 'text'
              AND folder.project_id = trim(json_extract(
                  NEW.properties,
                  '$.__ganbaru_project_id'
              ))
        )
        THEN RAISE(ABORT, 'notes folder page must belong to the same project')
    END;
END;

CREATE TRIGGER notes_row_hierarchy_detach_moved_page
AFTER UPDATE OF parent_type, parent_data_source_id ON notes_pages
BEGIN
    DELETE FROM notes_data_source_row_hierarchy
    WHERE (row_page_id = NEW.id OR parent_row_page_id = NEW.id)
      AND (NEW.parent_type <> 'data_source_id'
        OR NEW.parent_data_source_id IS NULL
        OR data_source_id <> NEW.parent_data_source_id);
END;


-- Quick notes.

CREATE TABLE quick_note_tags (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 40),
    sort_order INTEGER NOT NULL CHECK (sort_order >= 0 AND sort_order < 9),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    UNIQUE (name),
    UNIQUE (sort_order)
);

CREATE TABLE quick_notes (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 200),
    body_plain_text TEXT NOT NULL DEFAULT '' CHECK (length(body_plain_text) <= 65536),
    color INTEGER NOT NULL DEFAULT 30 CHECK (color >= 0 AND color < 32),
    tag_id TEXT REFERENCES quick_note_tags(id) ON DELETE SET NULL,
    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1)),
    trashed_at TEXT CHECK (trashed_at IS NULL OR trim(trashed_at) <> ''),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    manual_order REAL NOT NULL DEFAULT 0,
    CHECK (pinned = 0 OR (archived = 0 AND trashed_at IS NULL))
);
CREATE INDEX idx_quick_notes_active
    ON quick_notes(pinned DESC, manual_order ASC, id ASC)
    WHERE archived = 0 AND trashed_at IS NULL;
CREATE INDEX idx_quick_notes_archive
    ON quick_notes(updated_at DESC, id)
    WHERE archived = 1 AND trashed_at IS NULL;
CREATE INDEX idx_quick_notes_tag_active
    ON quick_notes(tag_id, pinned DESC, manual_order ASC, id ASC)
    WHERE archived = 0 AND trashed_at IS NULL;
CREATE INDEX idx_quick_notes_trash
    ON quick_notes(trashed_at DESC, id)
    WHERE trashed_at IS NOT NULL;

CREATE VIRTUAL TABLE quick_notes_search_fts USING fts5(
    note_id UNINDEXED,
    title,
    body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE quick_note_text_runs (
    note_id TEXT NOT NULL REFERENCES quick_notes(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL CHECK (sort_order >= 0),
    content TEXT NOT NULL CHECK (content <> ''),
    bold INTEGER NOT NULL DEFAULT 0 CHECK (bold IN (0, 1)),
    italic INTEGER NOT NULL DEFAULT 0 CHECK (italic IN (0, 1)),
    underline INTEGER NOT NULL DEFAULT 0 CHECK (underline IN (0, 1)),
    PRIMARY KEY (note_id, sort_order)
);
CREATE INDEX idx_quick_note_text_runs_note ON quick_note_text_runs(note_id, sort_order);

-- Quick notes triggers.

CREATE TRIGGER quick_notes_search_delete
AFTER DELETE ON quick_notes
BEGIN
    DELETE FROM quick_notes_search_fts WHERE note_id = OLD.id;
END;

CREATE TRIGGER quick_notes_search_insert
AFTER INSERT ON quick_notes
BEGIN
    INSERT INTO quick_notes_search_fts(note_id, title, body)
    VALUES (NEW.id, NEW.title, NEW.body_plain_text);
END;

CREATE TRIGGER quick_notes_search_update
AFTER UPDATE OF title, body_plain_text ON quick_notes
BEGIN
    DELETE FROM quick_notes_search_fts WHERE note_id = OLD.id;
    INSERT INTO quick_notes_search_fts(note_id, title, body)
    VALUES (NEW.id, NEW.title, NEW.body_plain_text);
END;


-- Themes.

CREATE TABLE themes (
    id TEXT PRIMARY KEY CHECK (id NOT IN ('light', 'dark')),
    display_name TEXT NOT NULL,
    blend_canvas TEXT NOT NULL,
    seed_blend_canvas TEXT NOT NULL,
    derivation_engine_version INTEGER NOT NULL,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    icon_label TEXT NOT NULL CHECK (icon_label IN ('light', 'dark')),
    seed_icon_label TEXT NOT NULL CHECK (seed_icon_label IN ('light', 'dark')),
    calendar_default_mode TEXT NOT NULL DEFAULT 'app-canvas' CHECK (calendar_default_mode IN ('light', 'dark', 'app-canvas', 'custom')),
    calendar_default_custom TEXT NOT NULL DEFAULT '#27282A',
    seed_calendar_default_mode TEXT NOT NULL DEFAULT 'app-canvas' CHECK (seed_calendar_default_mode IN ('light', 'dark', 'app-canvas', 'custom')),
    seed_calendar_default_custom TEXT NOT NULL DEFAULT '#27282A'
);

CREATE TABLE theme_event_palette (
    theme_id TEXT NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    slot INTEGER NOT NULL CHECK (slot >= 0 AND slot < 32),
    value TEXT NOT NULL,
    PRIMARY KEY (theme_id, slot)
);

CREATE TABLE theme_seed_event_palette (
    theme_id TEXT NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    slot INTEGER NOT NULL CHECK (slot >= 0 AND slot < 32),
    value TEXT NOT NULL,
    PRIMARY KEY (theme_id, slot)
);

CREATE TABLE theme_seed_tokens (
    theme_id TEXT NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('source', 'app', 'calendar')),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    isolated INTEGER NOT NULL DEFAULT 0 CHECK (isolated IN (0, 1)),
    PRIMARY KEY (theme_id, kind, key)
);
CREATE INDEX idx_theme_seed_tokens_kind ON theme_seed_tokens(theme_id, kind);

CREATE TABLE theme_tokens (
    theme_id TEXT NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('source', 'app', 'calendar')),
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    isolated INTEGER NOT NULL DEFAULT 0 CHECK (isolated IN (0, 1)),
    PRIMARY KEY (theme_id, kind, key)
);
CREATE INDEX idx_theme_tokens_kind ON theme_tokens(theme_id, kind);

CREATE TABLE theme_upgrade_dismissals (
    theme_id TEXT NOT NULL REFERENCES themes(id) ON DELETE CASCADE,
    engine_version INTEGER NOT NULL,
    dismissed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (theme_id, engine_version)
);


-- Chat: Participants, conversations and channels, agent threads, organizational coordination and authorization, scratch workspaces, and resources.

CREATE TABLE chat_participants (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    participant_kind TEXT NOT NULL CHECK (
        participant_kind IN ('local_user', 'ai_teammate', 'human')
    ),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
    avatar_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (avatar_schema_version >= 1),
    avatar_data TEXT NOT NULL DEFAULT '{"kind":"initials"}' CHECK (
        json_valid(avatar_data) AND json_type(avatar_data) = 'object'
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE UNIQUE INDEX idx_chat_participants_local_user
ON chat_participants(participant_kind)
WHERE participant_kind = 'local_user';
CREATE UNIQUE INDEX idx_chat_participants_teammate_display_name
ON chat_participants(lower(trim(display_name)))
WHERE participant_kind = 'ai_teammate';

CREATE TABLE chat_teammates (
    participant_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    role TEXT NOT NULL CHECK (length(trim(role)) BETWEEN 1 AND 1000),
    instructions TEXT NOT NULL DEFAULT '' CHECK (length(instructions) <= 65536),
    latest_policy_revision INTEGER NOT NULL DEFAULT 0 CHECK (latest_policy_revision >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE chat_teammate_policy_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        model_selection_schema_version >= 1
    ),
    model_selection_data TEXT NOT NULL CHECK (
        json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
    ),
    effort TEXT CHECK (
        effort IS NULL OR effort IN ('none', 'minimal', 'low', 'medium', 'high', 'xhigh')
    ),
    speed TEXT CHECK (speed IS NULL OR speed IN ('standard', 'fast')),
    provider_options_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        provider_options_schema_version >= 1
    ),
    provider_options_data TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(provider_options_data) AND json_type(provider_options_data) = 'object'
    ),
    interaction_mode TEXT NOT NULL DEFAULT 'build' CHECK (interaction_mode = 'build'),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    safety_mode TEXT NOT NULL DEFAULT 'ask_for_approval' CHECK (
        safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
    ),
    UNIQUE (teammate_id, revision)
) STRICT;

CREATE TABLE chat_teammate_access_state (
    teammate_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    access_revision INTEGER NOT NULL DEFAULT 0 CHECK (access_revision >= 0),
    runtime_approval_policy TEXT NOT NULL DEFAULT 'ask' CHECK (
        runtime_approval_policy IN ('ask', 'auto_approve', 'unattended', 'provider_custom')
    ),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE chat_access_profiles (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    builtin_key TEXT CHECK (
        builtin_key IS NULL OR builtin_key IN (
            'conversation_only', 'read_only', 'edit_files',
            'build_and_test', 'publish_changes'
        )
    ),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
    latest_revision INTEGER NOT NULL DEFAULT 1 CHECK (latest_revision >= 1),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE UNIQUE INDEX idx_chat_access_profiles_builtin
ON chat_access_profiles(builtin_key)
WHERE builtin_key IS NOT NULL;
CREATE UNIQUE INDEX idx_chat_access_profiles_custom_name
ON chat_access_profiles(lower(trim(display_name)))
WHERE builtin_key IS NULL AND archived_at IS NULL;

CREATE TABLE chat_access_profile_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    access_profile_id TEXT NOT NULL REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    default_read_history INTEGER NOT NULL CHECK (default_read_history IN (0, 1)),
    default_participate INTEGER NOT NULL CHECK (default_participate IN (0, 1)),
    default_history_boundary TEXT NOT NULL CHECK (
        default_history_boundary IN ('entire', 'from_grant')
    ),
    maximum_folder_capability TEXT NOT NULL CHECK (
        maximum_folder_capability IN ('none', 'read', 'edit', 'execute', 'publish')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (access_profile_id, revision)
) STRICT;

CREATE TABLE chat_conversations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    project_id TEXT NOT NULL REFERENCES projects(id) ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_kind TEXT NOT NULL CHECK (
        conversation_kind IN ('channel', 'direct_message', 'task_discussion')
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    last_activity_at TEXT NOT NULL CHECK (length(last_activity_at) >= 20),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    UNIQUE (id, project_id)
) STRICT;

CREATE TABLE chat_conversation_memberships (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    membership_role TEXT NOT NULL DEFAULT 'member' CHECK (
        membership_role IN ('owner', 'member')
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (conversation_id, participant_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_chat_conversation_memberships_participant
ON chat_conversation_memberships(participant_id, removed_at, conversation_id);

CREATE TABLE chat_teammate_channel_memberships (
    conversation_id TEXT NOT NULL,
    teammate_id TEXT NOT NULL,
    access_profile_id TEXT NOT NULL,
    read_history INTEGER NOT NULL DEFAULT 0 CHECK (read_history IN (0, 1)),
    read_history_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        read_history_inherits_profile IN (0, 1)
    ),
    participate INTEGER NOT NULL DEFAULT 0 CHECK (participate IN (0, 1)),
    participate_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        participate_inherits_profile IN (0, 1)
    ),
    history_boundary TEXT NOT NULL DEFAULT 'entire' CHECK (
        history_boundary IN ('entire', 'from_grant')
    ),
    history_boundary_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        history_boundary_inherits_profile IN (0, 1)
    ),
    history_from_ordinal INTEGER CHECK (
        history_from_ordinal IS NULL OR history_from_ordinal >= 1
    ),
    runtime_approval_policy TEXT CHECK (
        runtime_approval_policy IS NULL OR runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    scratch_runtime_approval_policy TEXT CHECK (
        scratch_runtime_approval_policy IS NULL OR scratch_runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (conversation_id, teammate_id),
    FOREIGN KEY (conversation_id, teammate_id)
        REFERENCES chat_conversation_memberships(conversation_id, participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (access_profile_id)
        REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    CHECK (
        (history_boundary = 'entire' AND history_from_ordinal IS NULL)
        OR (history_boundary = 'from_grant' AND history_from_ordinal IS NOT NULL)
    )
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_chat_teammate_channel_memberships_teammate
ON chat_teammate_channel_memberships(teammate_id, conversation_id);

CREATE TABLE chat_teammate_working_folder_grants (
    conversation_id TEXT NOT NULL,
    teammate_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    working_folder_id TEXT NOT NULL,
    capability TEXT NOT NULL CHECK (
        capability IN ('none', 'read', 'edit', 'execute', 'publish')
    ),
    capability_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        capability_inherits_profile IN (0, 1)
    ),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    runtime_approval_policy TEXT CHECK (
        runtime_approval_policy IS NULL OR runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
    PRIMARY KEY (conversation_id, teammate_id, working_folder_id),
    FOREIGN KEY (conversation_id, teammate_id)
        REFERENCES chat_conversation_memberships(conversation_id, participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (conversation_id, project_id)
        REFERENCES chat_conversations(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE NO ACTION
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX idx_chat_teammate_working_folder_grants_default
ON chat_teammate_working_folder_grants(conversation_id, teammate_id)
WHERE is_default = 1 AND revoked_at IS NULL;

CREATE TABLE chat_conversation_read_cursors (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    last_read_root_ordinal INTEGER NOT NULL DEFAULT 0 CHECK (last_read_root_ordinal >= 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (conversation_id, participant_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_conversation_audience_state (
    conversation_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE chat_channels (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    project_id TEXT NOT NULL REFERENCES projects(id) ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    topic TEXT NOT NULL DEFAULT '' CHECK (length(topic) <= 250),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (conversation_id, project_id)
        REFERENCES chat_conversations(id, project_id)
        ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;
CREATE INDEX idx_chat_channels_active_project
ON chat_channels(project_id, archived_at, updated_at DESC, id);
CREATE UNIQUE INDEX idx_chat_channels_project_default
ON chat_channels(project_id)
WHERE is_default = 1;
CREATE UNIQUE INDEX idx_chat_channels_project_name
ON chat_channels(project_id, name COLLATE NOCASE);

CREATE TABLE chat_reply_threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    root_item_id TEXT NOT NULL UNIQUE CHECK (length(root_item_id) BETWEEN 1 AND 1024),
    reply_count INTEGER NOT NULL DEFAULT 0 CHECK (reply_count >= 0),
    last_activity_at TEXT NOT NULL CHECK (length(last_activity_at) >= 20),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_reply_threads_conversation
ON chat_reply_threads(conversation_id, last_activity_at DESC, id);

CREATE TABLE chat_reply_thread_read_cursors (
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    last_read_reply_ordinal INTEGER NOT NULL DEFAULT 0 CHECK (last_read_reply_ordinal >= 0),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (reply_thread_id, participant_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_scheduled_messages (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    client_command_id TEXT NOT NULL UNIQUE CHECK (length(client_command_id) BETWEEN 1 AND 1024),
    channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reply_thread_id TEXT REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    request_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (request_schema_version = 1),
    request_data TEXT NOT NULL CHECK (
        json_valid(request_data) AND json_type(request_data) = 'object'
    ),
    state TEXT NOT NULL DEFAULT 'scheduled' CHECK (
        state IN ('scheduled', 'dispatching', 'failed')
    ),
    scheduled_for TEXT NOT NULL CHECK (length(scheduled_for) >= 20),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    claimed_at TEXT CHECK (claimed_at IS NULL OR length(claimed_at) >= 20),
    claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 1024),
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_scheduled_messages_destination
ON chat_scheduled_messages(channel_id, reply_thread_id, scheduled_for, id);
CREATE INDEX idx_chat_scheduled_messages_due
ON chat_scheduled_messages(state, available_at, scheduled_for, id);

CREATE TABLE chat_conversation_items (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reply_thread_id TEXT REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    item_kind TEXT NOT NULL CHECK (
        item_kind IN ('message', 'work_update', 'approval_request', 'question', 'review_packet')
    ),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (conversation_id, reply_thread_id, ordinal),
    CHECK (reply_thread_id IS NULL OR item_kind != 'message' OR ordinal >= 1)
) STRICT;
CREATE INDEX idx_chat_conversation_items_page
ON chat_conversation_items(conversation_id, reply_thread_id, ordinal DESC, id);
CREATE UNIQUE INDEX idx_chat_conversation_items_reply_ordinal
ON chat_conversation_items(reply_thread_id, ordinal)
WHERE reply_thread_id IS NOT NULL;
CREATE UNIQUE INDEX idx_chat_conversation_items_root_ordinal
ON chat_conversation_items(conversation_id, ordinal)
WHERE reply_thread_id IS NULL;

CREATE TABLE chat_communication_messages (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    author_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    current_revision_id TEXT CHECK (
        current_revision_id IS NULL OR length(current_revision_id) BETWEEN 1 AND 1024
    ),
    edited_at TEXT CHECK (edited_at IS NULL OR length(edited_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    author_label_snapshot TEXT NOT NULL DEFAULT 'Unknown participant' CHECK (
        length(trim(author_label_snapshot)) BETWEEN 1 AND 160
    )
) STRICT;

CREATE VIRTUAL TABLE chat_communication_search_fts USING fts5(
    message_item_id UNINDEXED,
    conversation_id UNINDEXED,
    reply_thread_id UNINDEXED,
    author_display_name,
    normalized_markdown,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE chat_communication_message_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    normalized_markdown TEXT NOT NULL CHECK (length(normalized_markdown) <= 131072),
    rich_content_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (rich_content_schema_version >= 1),
    rich_content_data TEXT NOT NULL DEFAULT '{"type":"doc","content":[]}' CHECK (
        json_valid(rich_content_data) AND json_type(rich_content_data) = 'object'
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (message_item_id, revision),
    UNIQUE (id, message_item_id)
) STRICT;

CREATE TABLE chat_communication_message_revision_ordinals (
    ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
    message_revision_id TEXT NOT NULL UNIQUE
        REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE
) STRICT;

CREATE TABLE chat_message_references (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reference_kind TEXT NOT NULL CHECK (
        reference_kind IN (
            'participant', 'channel', 'working_folder',
            'workspace_path', 'execution_environment'
        )
    ),
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    plain_text_projection TEXT NOT NULL CHECK (length(plain_text_projection) BETWEEN 1 AND 4096),
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (message_revision_id, start_offset, end_offset),
    UNIQUE (message_revision_id, ordinal)
) STRICT;

CREATE TABLE chat_channel_reference_targets (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    source_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    source_lower_ordinal INTEGER NOT NULL CHECK (source_lower_ordinal >= 0),
    source_high_ordinal INTEGER NOT NULL CHECK (source_high_ordinal >= source_lower_ordinal),
    source_revision_cutoff_id TEXT REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_audience_revision INTEGER NOT NULL CHECK (destination_audience_revision >= 1),
    CHECK (
        (source_high_ordinal = 0 AND source_revision_cutoff_id IS NULL)
        OR (source_high_ordinal > 0 AND source_revision_cutoff_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE chat_participant_reference_targets (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION
) STRICT;

CREATE TABLE chat_working_folder_reference_targets (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION
) STRICT;

CREATE TABLE chat_workspace_path_reference_targets (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    path_kind TEXT NOT NULL CHECK (path_kind IN ('file', 'folder')),
    relative_path TEXT NOT NULL CHECK (
        length(relative_path) BETWEEN 1 AND 4096
        AND relative_path NOT LIKE '/%'
        AND relative_path NOT LIKE '../%'
        AND relative_path NOT LIKE '%/../%'
        AND relative_path NOT LIKE '%/..'
        AND relative_path NOT LIKE '%\%'
    )
) STRICT;

CREATE TABLE chat_execution_environments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 2048),
    working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('current_folder', 'worktree', 'scratch')),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 240),
    repository_identity TEXT CHECK (
        repository_identity IS NULL OR length(repository_identity) BETWEEN 1 AND 1024
    ),
    lifecycle_state TEXT NOT NULL DEFAULT 'available' CHECK (
        lifecycle_state IN (
            'creating', 'available', 'missing',
            'cleanup_pending', 'cleanup_failed', 'removed'
        )
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    CHECK (
        (
            kind IN ('current_folder', 'worktree')
            AND working_folder_id IS NOT NULL
            AND scratch_generation_id IS NULL
        ) OR (
            kind = 'scratch'
            AND working_folder_id IS NULL
            AND scratch_generation_id IS NOT NULL
            AND repository_identity IS NULL
        )
    )
) STRICT;
CREATE UNIQUE INDEX idx_chat_execution_environments_current_folder
ON chat_execution_environments(working_folder_id)
WHERE kind = 'current_folder' AND archived_at IS NULL;
CREATE UNIQUE INDEX idx_chat_execution_environments_scratch
ON chat_execution_environments(scratch_generation_id)
WHERE kind = 'scratch' AND archived_at IS NULL;

CREATE TABLE chat_execution_environment_reference_targets (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    execution_environment_id TEXT NOT NULL REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION
) STRICT;

CREATE TABLE chat_scheduled_message_references (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reference_kind TEXT NOT NULL CHECK (
        reference_kind IN (
            'participant', 'channel', 'working_folder',
            'workspace_path', 'execution_environment'
        )
    ),
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    plain_text_projection TEXT NOT NULL CHECK (length(plain_text_projection) BETWEEN 1 AND 4096),
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    participant_id TEXT REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    participant_kind TEXT CHECK (
        participant_kind IS NULL OR participant_kind IN ('local_user', 'ai_teammate', 'human')
    ),
    channel_id TEXT REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    path_kind TEXT CHECK (path_kind IS NULL OR path_kind IN ('file', 'folder')),
    relative_path TEXT CHECK (
        relative_path IS NULL OR (
            length(relative_path) BETWEEN 1 AND 4096
            AND relative_path NOT LIKE '/%'
            AND relative_path NOT LIKE '../%'
            AND relative_path NOT LIKE '%/../%'
            AND relative_path NOT LIKE '%/..'
            AND relative_path NOT LIKE '%\%'
        )
    ),
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    UNIQUE (scheduled_message_id, start_offset, end_offset),
    UNIQUE (scheduled_message_id, ordinal),
    CHECK (
        (reference_kind = 'participant' AND participant_id IS NOT NULL
            AND participant_kind IS NOT NULL AND channel_id IS NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'channel' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NOT NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'working_folder' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NOT NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'workspace_path' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NOT NULL AND path_kind IS NOT NULL
            AND relative_path IS NOT NULL AND execution_environment_id IS NULL)
        OR (reference_kind = 'execution_environment' AND participant_id IS NULL
            AND participant_kind IS NULL AND channel_id IS NULL
            AND working_folder_id IS NULL AND path_kind IS NULL
            AND relative_path IS NULL AND execution_environment_id IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_scheduled_message_references_target
ON chat_scheduled_message_references(scheduled_message_id, reference_kind, ordinal);

CREATE TABLE chat_worktrees (
    execution_environment_id TEXT PRIMARY KEY NOT NULL
        REFERENCES chat_execution_environments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    branch_name TEXT NOT NULL CHECK (length(trim(branch_name)) BETWEEN 1 AND 1024),
    base_reference TEXT NOT NULL CHECK (length(trim(base_reference)) BETWEEN 1 AND 1024),
    remote_name TEXT CHECK (
        remote_name IS NULL OR length(trim(remote_name)) BETWEEN 1 AND 240
    ),
    head_object_id TEXT CHECK (
        head_object_id IS NULL OR length(head_object_id) BETWEEN 40 AND 128
    ),
    cleanup_policy TEXT NOT NULL DEFAULT 'ask' CHECK (
        cleanup_policy IN ('ask', 'retain', 'remove_when_clean')
    ),
    cleanup_state TEXT NOT NULL DEFAULT 'retained' CHECK (
        cleanup_state IN ('retained', 'queued', 'checking', 'cleaned', 'failed', 'dirty')
    ),
    cleanup_error_code TEXT CHECK (
        cleanup_error_code IS NULL OR length(cleanup_error_code) BETWEEN 1 AND 128
    ),
    cleanup_error_detail TEXT CHECK (
        cleanup_error_detail IS NULL OR length(cleanup_error_detail) <= 2000
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_worktrees_cleanup
ON chat_worktrees(cleanup_state, updated_at, execution_environment_id);

CREATE TABLE chat_threads (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT,
    project_id TEXT NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 1000),
    title_search TEXT GENERATED ALWAYS AS (lower(trim(title))) STORED,
    title_source TEXT NOT NULL DEFAULT 'user' CHECK (title_source IN ('user', 'provider')),
    provider_family_id TEXT NOT NULL CHECK (length(provider_family_id) BETWEEN 1 AND 1024),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    continuation_group_id TEXT NOT NULL CHECK (length(continuation_group_id) BETWEEN 1 AND 1024),
    provider_thread_id TEXT CHECK (
        provider_thread_id IS NULL OR length(provider_thread_id) BETWEEN 1 AND 1024
    ),
    resume_cursor_schema_version INTEGER CHECK (
        resume_cursor_schema_version IS NULL OR resume_cursor_schema_version >= 1
    ),
    resume_cursor_data TEXT CHECK (resume_cursor_data IS NULL OR json_valid(resume_cursor_data)),
    model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (
        model_selection_schema_version >= 1
    ),
    model_selection_data TEXT NOT NULL DEFAULT '{"modelId":null,"options":[]}' CHECK (
        json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
    ),
    safety_mode TEXT NOT NULL CHECK (
        safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
    ),
    interaction_mode TEXT NOT NULL CHECK (interaction_mode IN ('build', 'plan')),
    state TEXT NOT NULL CHECK (
        state IN ('draft', 'active', 'waiting', 'idle', 'error', 'archived', 'closed')
    ),
    latest_turn_state TEXT CHECK (
        latest_turn_state IS NULL OR latest_turn_state IN (
            'pending', 'dispatching', 'active', 'waiting_for_approval',
            'waiting_for_user_input', 'completed', 'interrupted', 'failed'
        )
    ),
    latest_preview TEXT CHECK (latest_preview IS NULL OR length(latest_preview) <= 2000),
    changed_file_summary_schema_version INTEGER CHECK (
        changed_file_summary_schema_version IS NULL OR changed_file_summary_schema_version >= 1
    ),
    changed_file_summary_data TEXT CHECK (
        changed_file_summary_data IS NULL OR (
            json_valid(changed_file_summary_data)
            AND json_type(changed_file_summary_data) = 'object'
        )
    ),
    message_count INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    last_event_sequence INTEGER NOT NULL DEFAULT 0 CHECK (last_event_sequence >= 0),
    last_projected_sequence INTEGER NOT NULL DEFAULT 0 CHECK (
        last_projected_sequence >= 0 AND last_projected_sequence <= last_event_sequence
    ),
    read_revision INTEGER NOT NULL DEFAULT 0 CHECK (read_revision >= 0),
    unread_at TEXT CHECK (unread_at IS NULL OR length(unread_at) >= 20),
    last_activity_at TEXT NOT NULL CHECK (length(last_activity_at) >= 20),
    archived_at TEXT CHECK (archived_at IS NULL OR length(archived_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    CHECK (
        (working_folder_id IS NULL AND execution_environment_id IS NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NOT NULL AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NOT NULL)
    ),
    CHECK (
        (resume_cursor_schema_version IS NULL AND resume_cursor_data IS NULL)
        OR (resume_cursor_schema_version IS NOT NULL AND resume_cursor_data IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_threads_active_project
ON chat_threads(project_id, last_activity_at DESC, id)
WHERE archived_at IS NULL AND state != 'closed';
CREATE INDEX idx_chat_threads_active_working_folder
ON chat_threads(working_folder_id, last_activity_at DESC, id)
WHERE working_folder_id IS NOT NULL AND archived_at IS NULL AND state != 'closed';
CREATE INDEX idx_chat_threads_archived
ON chat_threads(archived_at DESC, id)
WHERE archived_at IS NOT NULL;
CREATE INDEX idx_chat_threads_execution_environment
ON chat_threads(execution_environment_id, last_activity_at DESC, id)
WHERE execution_environment_id IS NOT NULL;
CREATE INDEX idx_chat_threads_scratch_generation
ON chat_threads(scratch_generation_id, last_activity_at DESC, id)
WHERE scratch_generation_id IS NOT NULL;
CREATE INDEX idx_chat_threads_title_search
ON chat_threads(title_search, last_activity_at DESC, id);

CREATE TABLE chat_drafts (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    thread_id TEXT REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    text TEXT NOT NULL DEFAULT '' CHECK (length(text) <= 16777216),
    mentions_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (mentions_schema_version >= 1),
    mentions_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(mentions_data) AND json_type(mentions_data) = 'array'
    ),
    provider_instance_id TEXT CHECK (
        provider_instance_id IS NULL OR length(provider_instance_id) BETWEEN 1 AND 1024
    ),
    model_selection_schema_version INTEGER CHECK (
        model_selection_schema_version IS NULL OR model_selection_schema_version >= 1
    ),
    model_selection_data TEXT CHECK (model_selection_data IS NULL OR json_valid(model_selection_data)),
    safety_mode TEXT CHECK (
        safety_mode IS NULL OR safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
    ),
    interaction_mode TEXT CHECK (interaction_mode IS NULL OR interaction_mode IN ('build', 'plan')),
    sent_snapshot_schema_version INTEGER CHECK (
        sent_snapshot_schema_version IS NULL OR sent_snapshot_schema_version >= 1
    ),
    sent_snapshot_data TEXT CHECK (sent_snapshot_data IS NULL OR json_valid(sent_snapshot_data)),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    rich_content_schema_version INTEGER
        CHECK (rich_content_schema_version IS NULL OR rich_content_schema_version >= 1),
    rich_content_data TEXT
        CHECK (rich_content_data IS NULL OR json_valid(rich_content_data)),
    CHECK (
        (model_selection_schema_version IS NULL AND model_selection_data IS NULL)
        OR (model_selection_schema_version IS NOT NULL AND model_selection_data IS NOT NULL)
    )
) STRICT;
CREATE UNIQUE INDEX idx_chat_drafts_thread
ON chat_drafts(thread_id)
WHERE thread_id IS NOT NULL;

CREATE TABLE chat_events (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    event_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (event_schema_version >= 1),
    turn_id TEXT,
    provider_turn_id TEXT CHECK (provider_turn_id IS NULL OR length(provider_turn_id) BETWEEN 1 AND 1024),
    provider_item_id TEXT CHECK (provider_item_id IS NULL OR length(provider_item_id) BETWEEN 1 AND 1024),
    provider_request_id TEXT CHECK (provider_request_id IS NULL OR length(provider_request_id) BETWEEN 1 AND 1024),
    provider_task_id TEXT CHECK (provider_task_id IS NULL OR length(provider_task_id) BETWEEN 1 AND 1024),
    provider_family_id TEXT NOT NULL CHECK (length(provider_family_id) BETWEEN 1 AND 1024),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    event_type TEXT NOT NULL CHECK (length(event_type) BETWEEN 1 AND 128),
    payload_schema_version INTEGER NOT NULL CHECK (payload_schema_version >= 1),
    payload_data TEXT NOT NULL CHECK (json_valid(payload_data)),
    provider_reference_schema_version INTEGER CHECK (
        provider_reference_schema_version IS NULL OR provider_reference_schema_version >= 1
    ),
    provider_reference_data TEXT CHECK (
        provider_reference_data IS NULL OR json_valid(provider_reference_data)
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    ingested_at TEXT NOT NULL CHECK (length(ingested_at) >= 20),
    redacted_diagnostic_schema_version INTEGER CHECK (
        redacted_diagnostic_schema_version IS NULL OR redacted_diagnostic_schema_version >= 1
    ),
    redacted_diagnostic_data TEXT CHECK (
        redacted_diagnostic_data IS NULL OR json_valid(redacted_diagnostic_data)
    ),
    diagnostic_expires_at TEXT CHECK (
        diagnostic_expires_at IS NULL OR length(diagnostic_expires_at) >= 20
    ),
    invalidated_at TEXT
        CHECK (invalidated_at IS NULL OR length(invalidated_at) >= 20),
    invalidation_reason TEXT
        CHECK (invalidation_reason IS NULL OR length(invalidation_reason) BETWEEN 1 AND 1000),
    UNIQUE (thread_id, sequence),
    CHECK (
        (provider_reference_schema_version IS NULL AND provider_reference_data IS NULL)
        OR (provider_reference_schema_version IS NOT NULL AND provider_reference_data IS NOT NULL)
    ),
    CHECK (
        (redacted_diagnostic_schema_version IS NULL AND redacted_diagnostic_data IS NULL AND diagnostic_expires_at IS NULL)
        OR (redacted_diagnostic_schema_version IS NOT NULL AND redacted_diagnostic_data IS NOT NULL AND diagnostic_expires_at IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_events_diagnostic_expiry
ON chat_events(diagnostic_expires_at, id)
WHERE diagnostic_expires_at IS NOT NULL;
CREATE INDEX idx_chat_events_thread_sequence
ON chat_events(thread_id, sequence, id);
CREATE INDEX idx_chat_events_turn_sequence
ON chat_events(turn_id, sequence, id);
CREATE INDEX idx_chat_events_valid_thread_sequence
ON chat_events(thread_id, sequence, id)
WHERE invalidated_at IS NULL;

CREATE TABLE chat_plans (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    origin_turn_id TEXT,
    sequence_anchor INTEGER NOT NULL CHECK (sequence_anchor >= 0),
    markdown TEXT NOT NULL DEFAULT '' CHECK (length(markdown) <= 16777216),
    steps_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (steps_schema_version >= 1),
    steps_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(steps_data) AND json_type(steps_data) = 'array'
    ),
    state TEXT NOT NULL CHECK (state IN ('proposed', 'accepted', 'dismissed', 'implemented')),
    implementation_turn_id TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_plans_thread_sequence
ON chat_plans(thread_id, sequence_anchor, id);

CREATE TABLE chat_preview_tabs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    current_url TEXT NOT NULL DEFAULT '' CHECK (length(current_url) <= 8192),
    title TEXT NOT NULL DEFAULT '' CHECK (length(title) <= 2000),
    viewport_kind TEXT NOT NULL DEFAULT 'responsive' CHECK (
        viewport_kind IN ('responsive', 'mobile', 'tablet', 'desktop', 'freeform')
    ),
    viewport_width INTEGER CHECK (viewport_width IS NULL OR viewport_width BETWEEN 1 AND 16384),
    viewport_height INTEGER CHECK (viewport_height IS NULL OR viewport_height BETWEEN 1 AND 16384),
    visible INTEGER NOT NULL DEFAULT 0 CHECK (visible IN (0, 1)),
    loading_state TEXT NOT NULL DEFAULT 'idle' CHECK (loading_state IN ('idle', 'loading', 'loaded', 'failed')),
    history_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (history_schema_version >= 1),
    history_data TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(history_data) AND json_type(history_data) = 'array'),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    UNIQUE (thread_id, position)
) STRICT;

CREATE TABLE chat_provider_cleanup_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE SET NULL,
    provider_family_id TEXT NOT NULL CHECK (length(provider_family_id) BETWEEN 1 AND 1024),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    provider_thread_id TEXT NOT NULL CHECK (length(provider_thread_id) BETWEEN 1 AND 1024),
    operation TEXT NOT NULL CHECK (operation IN ('rename', 'archive', 'delete', 'unsubscribe', 'cleanup')),
    payload_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (payload_schema_version >= 1),
    payload_data TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_data) AND json_type(payload_data) = 'object'),
    state TEXT NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128),
    last_error_detail TEXT CHECK (last_error_detail IS NULL OR length(last_error_detail) <= 2000),
    retry_after TEXT CHECK (retry_after IS NULL OR length(retry_after) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_provider_cleanup_jobs_retry
ON chat_provider_cleanup_jobs(state, retry_after, created_at, id);

CREATE TABLE chat_queued_followups (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 16777216),
    provider_instance_id TEXT NOT NULL CHECK (length(provider_instance_id) BETWEEN 1 AND 1024),
    model_selection_schema_version INTEGER NOT NULL CHECK (model_selection_schema_version >= 1),
    model_selection_data TEXT NOT NULL CHECK (json_valid(model_selection_data)),
    safety_mode TEXT NOT NULL CHECK (safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')),
    interaction_mode TEXT NOT NULL CHECK (interaction_mode IN ('build', 'plan')),
    attachment_ids_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (attachment_ids_schema_version >= 1),
    attachment_ids_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(attachment_ids_data) AND json_type(attachment_ids_data) = 'array'
    ),
    mentions_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (mentions_schema_version >= 1),
    mentions_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(mentions_data) AND json_type(mentions_data) = 'array'
    ),
    state TEXT NOT NULL CHECK (state IN ('queued', 'dispatched', 'cancelled')),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE UNIQUE INDEX idx_chat_queued_followups_active
ON chat_queued_followups(thread_id)
WHERE state = 'queued';
CREATE INDEX idx_chat_queued_followups_thread
ON chat_queued_followups(thread_id, updated_at DESC, id);

CREATE TABLE chat_terminal_layouts (
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    layout_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (layout_schema_version >= 1),
    layout_data TEXT NOT NULL DEFAULT '{"groups":[]}' CHECK (
        json_valid(layout_data) AND json_type(layout_data) = 'object'
    ),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE chat_command_receipts (
    client_command_id TEXT PRIMARY KEY NOT NULL CHECK (length(client_command_id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    command_kind TEXT NOT NULL CHECK (length(command_kind) BETWEEN 1 AND 128),
    submitted_revision INTEGER CHECK (submitted_revision IS NULL OR submitted_revision >= 0),
    state TEXT NOT NULL CHECK (state IN ('accepted', 'completed', 'failed')),
    result_schema_version INTEGER CHECK (result_schema_version IS NULL OR result_schema_version >= 1),
    result_data TEXT CHECK (result_data IS NULL OR json_valid(result_data)),
    error_schema_version INTEGER CHECK (error_schema_version IS NULL OR error_schema_version >= 1),
    error_data TEXT CHECK (error_data IS NULL OR json_valid(error_data)),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    CHECK (result_data IS NULL OR error_data IS NULL)
) STRICT;
CREATE INDEX idx_chat_command_receipts_thread
ON chat_command_receipts(thread_id, created_at DESC, client_command_id);

CREATE TABLE chat_turns (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    user_message_id TEXT,
    provider_turn_id TEXT CHECK (provider_turn_id IS NULL OR length(provider_turn_id) BETWEEN 1 AND 1024),
    source_proposed_plan_id TEXT,
    state TEXT NOT NULL CHECK (state IN (
            'pending', 'dispatching', 'active', 'waiting_for_approval',
            'waiting_for_user_input', 'completed', 'interrupted', 'failed'
        )),
    started_at TEXT CHECK (started_at IS NULL OR length(started_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20),
    stop_reason TEXT CHECK (stop_reason IS NULL OR length(stop_reason) <= 1000),
    error_schema_version INTEGER CHECK (error_schema_version IS NULL OR error_schema_version >= 1),
    error_data TEXT CHECK (error_data IS NULL OR json_valid(error_data)),
    model_selection_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (model_selection_schema_version >= 1),
    model_selection_data TEXT NOT NULL DEFAULT '{"modelId":null,"options":[]}' CHECK (
        json_valid(model_selection_data) AND json_type(model_selection_data) = 'object'
    ),
    safety_mode TEXT NOT NULL CHECK (safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')),
    interaction_mode TEXT NOT NULL CHECK (interaction_mode IN ('build', 'plan')),
    usage_schema_version INTEGER CHECK (usage_schema_version IS NULL OR usage_schema_version >= 1),
    usage_data TEXT CHECK (usage_data IS NULL OR json_valid(usage_data)),
    pre_checkpoint_id TEXT REFERENCES chat_checkpoints(id) ON UPDATE CASCADE ON DELETE SET NULL,
    post_checkpoint_id TEXT REFERENCES chat_checkpoints(id) ON UPDATE CASCADE ON DELETE SET NULL,
    changed_file_summary_schema_version INTEGER CHECK (
        changed_file_summary_schema_version IS NULL OR changed_file_summary_schema_version >= 1
    ),
    changed_file_summary_data TEXT CHECK (
        changed_file_summary_data IS NULL OR json_valid(changed_file_summary_data)
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    invalidated_at TEXT
        CHECK (invalidated_at IS NULL OR length(invalidated_at) >= 20),
    invalidation_reason TEXT
        CHECK (invalidation_reason IS NULL OR length(invalidation_reason) BETWEEN 1 AND 1000),
    UNIQUE (thread_id, ordinal),
    CHECK (
        (error_schema_version IS NULL AND error_data IS NULL)
        OR (error_schema_version IS NOT NULL AND error_data IS NOT NULL)
    ),
    CHECK (
        (usage_schema_version IS NULL AND usage_data IS NULL)
        OR (usage_schema_version IS NOT NULL AND usage_data IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_turns_thread_ordinal
ON chat_turns(thread_id, ordinal DESC, id);

CREATE TABLE chat_activities (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE CASCADE,
    sequence_anchor INTEGER NOT NULL CHECK (sequence_anchor >= 0),
    item_kind TEXT NOT NULL CHECK (length(item_kind) BETWEEN 1 AND 128),
    status TEXT NOT NULL CHECK (length(status) BETWEEN 1 AND 128),
    title TEXT NOT NULL CHECK (length(title) <= 2000),
    detail TEXT CHECK (detail IS NULL OR length(detail) <= 16777216),
    provider_item_id TEXT CHECK (provider_item_id IS NULL OR length(provider_item_id) BETWEEN 1 AND 1024),
    safe_metadata_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (safe_metadata_schema_version >= 1),
    safe_metadata_data TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(safe_metadata_data) AND json_type(safe_metadata_data) = 'object'
    ),
    started_at TEXT CHECK (started_at IS NULL OR length(started_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20),
    source_event_type TEXT NOT NULL CHECK (length(source_event_type) BETWEEN 1 AND 128),
    output_artifact_reference TEXT CHECK (
        output_artifact_reference IS NULL OR length(output_artifact_reference) BETWEEN 1 AND 1024
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_activities_thread_sequence
ON chat_activities(thread_id, sequence_anchor, id);
CREATE INDEX idx_chat_activities_turn
ON chat_activities(turn_id, sequence_anchor, id);

CREATE TABLE chat_checkpoint_failures (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE CASCADE,
    checkpoint_kind TEXT NOT NULL CHECK (checkpoint_kind IN ('initial', 'pre_turn', 'post_turn')),
    error_code TEXT NOT NULL CHECK (length(error_code) BETWEEN 1 AND 128),
    detail TEXT NOT NULL CHECK (length(detail) BETWEEN 1 AND 2000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_checkpoint_failures_thread
ON chat_checkpoint_failures(thread_id, created_at DESC, id);

CREATE TABLE chat_checkpoints (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_count INTEGER NOT NULL CHECK (turn_count >= 0),
    repository_identity TEXT NOT NULL CHECK (length(repository_identity) BETWEEN 1 AND 1024),
    hidden_ref_name TEXT NOT NULL UNIQUE CHECK (
        hidden_ref_name GLOB 'refs/ganbaru-ai/chat/*'
        AND hidden_ref_name NOT GLOB '*[[:space:]]*'
    ),
    git_object_id TEXT NOT NULL CHECK (length(git_object_id) BETWEEN 40 AND 128),
    status TEXT NOT NULL CHECK (status IN ('capturing', 'available', 'invalid', 'cleanup_pending', 'cleanup_failed')),
    changed_files_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (changed_files_schema_version >= 1),
    changed_files_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(changed_files_data) AND json_type(changed_files_data) = 'array'
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    cleanup_state TEXT NOT NULL DEFAULT 'retained' CHECK (
        cleanup_state IN ('retained', 'queued', 'cleaned', 'failed')
    ),
    checkpoint_kind TEXT
        CHECK (checkpoint_kind IS NULL OR checkpoint_kind IN ('initial', 'pre_turn', 'post_turn', 'recovery')),
    turn_id TEXT
        REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE SET NULL,
    index_commit_oid TEXT
        CHECK (index_commit_oid IS NULL OR length(index_commit_oid) BETWEEN 40 AND 128),
    index_tree_oid TEXT
        CHECK (index_tree_oid IS NULL OR length(index_tree_oid) BETWEEN 40 AND 128),
    worktree_tree_oid TEXT
        CHECK (worktree_tree_oid IS NULL OR length(worktree_tree_oid) BETWEEN 40 AND 128),
    head_oid TEXT
        CHECK (head_oid IS NULL OR length(head_oid) BETWEEN 40 AND 128),
    head_ref TEXT
        CHECK (head_ref IS NULL OR length(head_ref) BETWEEN 1 AND 4096),
    index_fingerprint TEXT
        CHECK (index_fingerprint IS NULL OR length(index_fingerprint) BETWEEN 16 AND 128),
    invalidated_at TEXT
        CHECK (invalidated_at IS NULL OR length(invalidated_at) >= 20),
    invalidated_by_checkpoint_id TEXT
        REFERENCES chat_checkpoints(id) ON UPDATE CASCADE ON DELETE SET NULL,
    UNIQUE (thread_id, turn_count)
) STRICT;
CREATE INDEX idx_chat_checkpoints_thread_turn
ON chat_checkpoints(thread_id, turn_count DESC, id);

CREATE TABLE chat_restore_previews (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    checkpoint_id TEXT NOT NULL REFERENCES chat_checkpoints(id) ON UPDATE CASCADE ON DELETE CASCADE,
    expected_thread_revision INTEGER NOT NULL CHECK (expected_thread_revision >= 0),
    repository_identity TEXT NOT NULL CHECK (length(repository_identity) BETWEEN 1 AND 1024),
    head_oid TEXT,
    head_ref TEXT,
    current_worktree_tree_oid TEXT NOT NULL CHECK (length(current_worktree_tree_oid) BETWEEN 40 AND 128),
    current_index_tree_oid TEXT NOT NULL CHECK (length(current_index_tree_oid) BETWEEN 40 AND 128),
    current_index_fingerprint TEXT NOT NULL CHECK (length(current_index_fingerprint) BETWEEN 16 AND 128),
    affected_files_data TEXT NOT NULL CHECK (
        json_valid(affected_files_data) AND json_type(affected_files_data) = 'array'
    ),
    state TEXT NOT NULL CHECK (state IN ('ready', 'stale', 'executing', 'completed', 'failed')),
    expires_at TEXT NOT NULL CHECK (length(expires_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_restore_previews_thread
ON chat_restore_previews(thread_id, created_at DESC, id);

CREATE TABLE chat_restore_operations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    checkpoint_id TEXT NOT NULL REFERENCES chat_checkpoints(id) ON UPDATE CASCADE ON DELETE NO ACTION,
    preview_id TEXT REFERENCES chat_restore_previews(id) ON UPDATE CASCADE ON DELETE SET NULL,
    provider_history_action TEXT CHECK (
        provider_history_action IS NULL OR provider_history_action IN ('rolled_back', 'fork_required')
    ),
    recovery_state TEXT NOT NULL CHECK (
        recovery_state IN ('pending', 'complete', 'recovered', 'recovery_required')
    ),
    recovery_ref_name TEXT,
    recovery_object_id TEXT,
    error_code TEXT,
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_restore_operations_thread
ON chat_restore_operations(thread_id, created_at DESC, id);

CREATE TABLE chat_pending_requests (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE CASCADE,
    provider_request_id TEXT NOT NULL CHECK (length(provider_request_id) BETWEEN 1 AND 1024),
    request_kind TEXT NOT NULL CHECK (request_kind IN ('approval', 'user_input')),
    safe_display_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (safe_display_schema_version >= 1),
    safe_display_data TEXT NOT NULL CHECK (json_valid(safe_display_data)),
    allowed_decisions_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (allowed_decisions_schema_version >= 1),
    allowed_decisions_data TEXT NOT NULL CHECK (
        json_valid(allowed_decisions_data) AND json_type(allowed_decisions_data) = 'array'
    ),
    opened_sequence INTEGER NOT NULL CHECK (opened_sequence >= 1),
    resolution_state TEXT NOT NULL DEFAULT 'open' CHECK (
        resolution_state IN ('open', 'resolved', 'stale', 'interrupted')
    ),
    resolution_schema_version INTEGER CHECK (
        resolution_schema_version IS NULL OR resolution_schema_version >= 1
    ),
    resolution_data TEXT CHECK (resolution_data IS NULL OR json_valid(resolution_data)),
    opened_at TEXT NOT NULL CHECK (length(opened_at) >= 20),
    resolved_at TEXT CHECK (resolved_at IS NULL OR length(resolved_at) >= 20),
    CHECK (
        (resolution_state = 'open' AND resolution_data IS NULL AND resolved_at IS NULL)
        OR (resolution_state != 'open' AND resolved_at IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_pending_requests_thread
ON chat_pending_requests(thread_id, opened_sequence, id);
CREATE UNIQUE INDEX idx_chat_pending_requests_unresolved
ON chat_pending_requests(thread_id, provider_request_id)
WHERE resolution_state = 'open';

CREATE TABLE chat_user_input_drafts (
    request_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_pending_requests(id) ON UPDATE CASCADE ON DELETE CASCADE,
    answers_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (answers_schema_version >= 1),
    answers_data TEXT NOT NULL DEFAULT '[]' CHECK (
        json_valid(answers_data) AND json_type(answers_data) = 'array'
    ),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE chat_review_comments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE SET NULL,
    relative_path TEXT NOT NULL CHECK (
        length(relative_path) BETWEEN 1 AND 4096
        AND relative_path NOT LIKE '/%'
        AND relative_path NOT LIKE '../%'
        AND relative_path NOT LIKE '%/../%'
        AND relative_path NOT LIKE '%\\%'
    ),
    content_revision TEXT NOT NULL CHECK (length(content_revision) BETWEEN 16 AND 128),
    start_line INTEGER NOT NULL CHECK (start_line >= 1),
    start_column INTEGER NOT NULL DEFAULT 1 CHECK (start_column >= 1),
    end_line INTEGER NOT NULL CHECK (end_line >= start_line),
    end_column INTEGER NOT NULL DEFAULT 1 CHECK (end_column >= 1),
    selected_text TEXT NOT NULL DEFAULT '' CHECK (length(selected_text) <= 1048576),
    comment_text TEXT NOT NULL CHECK (length(trim(comment_text)) BETWEEN 1 AND 65536),
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'resolved')),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    resolved_at TEXT CHECK (resolved_at IS NULL OR length(resolved_at) >= 20),
    source_kind TEXT NOT NULL DEFAULT 'file'
        CHECK (source_kind IN (
            'file', 'working_tree', 'checkpoint', 'commit', 'branch',
            'change_request', 'provider_turn'
        )),
    source_data TEXT
        CHECK (source_data IS NULL OR json_valid(source_data)),
    review_revision TEXT
        CHECK (review_revision IS NULL OR length(review_revision) BETWEEN 16 AND 128),
    snapshot_id TEXT
        CHECK (snapshot_id IS NULL OR length(snapshot_id) BETWEEN 1 AND 1024),
    file_id TEXT
        CHECK (file_id IS NULL OR length(file_id) BETWEEN 1 AND 1024),
    selection_side TEXT NOT NULL DEFAULT 'file'
        CHECK (selection_side IN ('file', 'old', 'new')),
    previous_relative_path TEXT
        CHECK (
        previous_relative_path IS NULL
        OR (
            length(previous_relative_path) BETWEEN 1 AND 4096
            AND previous_relative_path NOT LIKE '/%'
            AND previous_relative_path NOT LIKE '../%'
            AND previous_relative_path NOT LIKE '%/../%'
            AND previous_relative_path NOT LIKE '%\%'
        )
    ),
    applicability TEXT NOT NULL DEFAULT 'current'
        CHECK (applicability IN ('current', 'outdated', 'source_unavailable')),
    queued_for_send INTEGER NOT NULL DEFAULT 0
        CHECK (queued_for_send IN (0, 1)),
    CHECK ((state = 'open' AND resolved_at IS NULL) OR state = 'resolved')
) STRICT;
CREATE INDEX idx_chat_review_comments_thread_path
ON chat_review_comments(thread_id, relative_path, state, created_at, id);
CREATE INDEX idx_chat_review_comments_thread_queue
ON chat_review_comments(thread_id, queued_for_send, state, created_at, id);

CREATE TABLE chat_thread_relations (
    child_thread_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    parent_thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    source_turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE SET NULL,
    relation_kind TEXT NOT NULL CHECK (relation_kind IN ('fork', 'continuation')),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    CHECK (child_thread_id != parent_thread_id)
) STRICT;
CREATE INDEX idx_chat_thread_relations_parent
ON chat_thread_relations(parent_thread_id, created_at DESC, child_thread_id);

CREATE TABLE chat_messages (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    turn_id TEXT REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE CASCADE,
    sequence_anchor INTEGER NOT NULL CHECK (sequence_anchor >= 0),
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    normalized_markdown TEXT NOT NULL DEFAULT '' CHECK (length(normalized_markdown) <= 16777216),
    streaming_state TEXT NOT NULL CHECK (streaming_state IN ('pending', 'streaming', 'complete', 'interrupted', 'failed')),
    provider_item_id TEXT CHECK (provider_item_id IS NULL OR length(provider_item_id) BETWEEN 1 AND 1024),
    content_metadata_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (content_metadata_schema_version >= 1),
    content_metadata_data TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(content_metadata_data) AND json_type(content_metadata_data) = 'object'
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_messages_thread_sequence
ON chat_messages(thread_id, sequence_anchor, id);
CREATE INDEX idx_chat_messages_turn
ON chat_messages(turn_id, sequence_anchor, id);

CREATE TABLE chat_work_assignments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    previous_assignment_id TEXT REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'working', 'waiting_for_answer', 'waiting_for_approval',
            'ready_for_review', 'completed', 'failed', 'cancelled'
        )
    ),
    state_reason TEXT CHECK (state_reason IS NULL OR length(state_reason) <= 4000),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE UNIQUE INDEX idx_chat_work_assignments_one_active
ON chat_work_assignments(reply_thread_id)
WHERE state IN (
    'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'
);
CREATE INDEX idx_chat_work_assignments_teammate
ON chat_work_assignments(teammate_id, state, updated_at DESC, id);

CREATE TABLE chat_assignment_authorization_revisions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    requester_participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_policy_revision_id TEXT NOT NULL REFERENCES chat_teammate_policy_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_access_revision INTEGER NOT NULL CHECK (teammate_access_revision >= 1),
    destination_conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    access_profile_revision_id TEXT NOT NULL REFERENCES chat_access_profile_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    resolved_runtime_approval_policy TEXT NOT NULL CHECK (
        resolved_runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    scope_digest TEXT NOT NULL CHECK (length(scope_digest) = 64),
    decision_state TEXT NOT NULL CHECK (decision_state IN ('allowed', 'blocked', 'revoked')),
    reason TEXT NOT NULL DEFAULT '' CHECK (length(reason) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
    UNIQUE (assignment_id, revision),
    CHECK (
        (execution_environment_id IS NULL AND working_folder_id IS NULL)
        OR execution_environment_id IS NOT NULL
    )
) STRICT;
CREATE INDEX idx_chat_assignment_authorization_revisions_active
ON chat_assignment_authorization_revisions(assignment_id, decision_state, revision DESC);

CREATE TABLE chat_access_revocation_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    conversation_id TEXT REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    authorization_revision_id TEXT REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (
        state IN ('queued', 'claimed', 'completed', 'failed', 'cancelled')
    ),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 4000),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    claimed_at TEXT CHECK (claimed_at IS NULL OR length(claimed_at) >= 20),
    claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 1024),
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_access_revocation_jobs_ready
ON chat_access_revocation_jobs(state, available_at, created_at, id);

CREATE TABLE chat_assignment_authorized_channel_sources (
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_handle TEXT NOT NULL UNIQUE CHECK (length(source_handle) BETWEEN 32 AND 1024),
    message_reference_id TEXT NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    lower_ordinal INTEGER NOT NULL CHECK (lower_ordinal >= 1),
    high_ordinal INTEGER NOT NULL CHECK (high_ordinal >= lower_ordinal),
    source_revision_cutoff_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_audience_revision INTEGER NOT NULL CHECK (destination_audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (authorization_revision_id, conversation_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_assignment_authorized_folder_sources (
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    root_handle TEXT NOT NULL UNIQUE CHECK (length(root_handle) BETWEEN 32 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    capability TEXT NOT NULL CHECK (
        capability IN ('read', 'edit', 'execute', 'publish')
    ),
    is_execution_target INTEGER NOT NULL DEFAULT 0 CHECK (is_execution_target IN (0, 1)),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    resolved_runtime_approval_policy TEXT CHECK (
        resolved_runtime_approval_policy IS NULL OR resolved_runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    PRIMARY KEY (authorization_revision_id, working_folder_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX idx_chat_assignment_authorized_folder_sources_execution_target
ON chat_assignment_authorized_folder_sources(authorization_revision_id)
WHERE is_execution_target = 1;

CREATE TABLE chat_host_tool_invocations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    tool_name TEXT NOT NULL CHECK (length(tool_name) BETWEEN 1 AND 160),
    request_hash TEXT NOT NULL CHECK (length(request_hash) = 64),
    decision_state TEXT NOT NULL CHECK (decision_state IN ('allowed', 'denied')),
    denial_category TEXT CHECK (
        denial_category IS NULL OR length(denial_category) BETWEEN 1 AND 160
    ),
    response_bytes INTEGER NOT NULL DEFAULT 0 CHECK (response_bytes >= 0),
    truncated INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0, 1)),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_host_tool_invocations_authorization
ON chat_host_tool_invocations(authorization_revision_id, created_at, id);

CREATE TABLE chat_host_tool_returned_message_revisions (
    invocation_id TEXT NOT NULL REFERENCES chat_host_tool_invocations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (invocation_id, message_revision_id),
    UNIQUE (invocation_id, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_assignment_context_packages (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    serialized_text TEXT NOT NULL CHECK (length(serialized_text) <= 131072),
    serialized_bytes INTEGER NOT NULL CHECK (serialized_bytes BETWEEN 0 AND 131072),
    excluded_thread_reply_count INTEGER NOT NULL DEFAULT 0 CHECK (excluded_thread_reply_count >= 0),
    excluded_channel_message_count INTEGER NOT NULL DEFAULT 0 CHECK (excluded_channel_message_count >= 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (assignment_id, revision)
) STRICT;

CREATE TABLE chat_assignment_context_sources (
    context_package_id TEXT NOT NULL REFERENCES chat_assignment_context_packages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (
        source_kind IN ('trigger', 'thread_root', 'thread_reply', 'channel_message', 'attachment', 'resource')
    ),
    source_id TEXT NOT NULL CHECK (length(source_id) BETWEEN 1 AND 1024),
    source_revision INTEGER NOT NULL DEFAULT 1 CHECK (source_revision >= 1),
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (context_package_id, source_kind, source_id),
    UNIQUE (context_package_id, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_assignment_dispatch_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL UNIQUE REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT 'queued' CHECK (
        state IN ('queued', 'claimed', 'completed', 'failed', 'cancelled')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    claimed_at TEXT CHECK (claimed_at IS NULL OR length(claimed_at) >= 20),
    claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 1024),
    last_error TEXT CHECK (last_error IS NULL OR length(last_error) <= 4000),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_assignment_dispatch_jobs_ready
ON chat_assignment_dispatch_jobs(state, available_at, created_at, id);

CREATE TABLE chat_work_assignment_inputs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    message_item_id TEXT NOT NULL UNIQUE REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    routing_kind TEXT NOT NULL CHECK (
        routing_kind IN ('trigger', 'steer', 'queued_continuation', 'follow_up')
    ),
    delivery_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        delivery_state IN ('pending', 'delivered', 'failed', 'ignored')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    delivered_at TEXT CHECK (delivered_at IS NULL OR length(delivered_at) >= 20),
    UNIQUE (assignment_id, ordinal)
) STRICT;

CREATE TABLE chat_work_semantic_updates (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    update_kind TEXT NOT NULL CHECK (
        update_kind IN ('plan', 'replan', 'question', 'approval', 'failure', 'result', 'review')
    ),
    payload_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (payload_schema_version >= 1),
    payload_data TEXT NOT NULL CHECK (json_valid(payload_data) AND json_type(payload_data) = 'object'),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

CREATE TABLE chat_scratch_scopes (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    lifecycle_state TEXT NOT NULL DEFAULT 'active' CHECK (
        lifecycle_state IN ('active', 'archived', 'cleanup_pending', 'cleanup_failed', 'removed')
    ),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    UNIQUE (reply_thread_id, teammate_id)
) STRICT;

CREATE TABLE chat_scratch_generations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scratch_scope_id TEXT NOT NULL REFERENCES chat_scratch_scopes(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    generation INTEGER NOT NULL CHECK (generation >= 1),
    lifecycle_state TEXT NOT NULL DEFAULT 'active' CHECK (
        lifecycle_state IN ('active', 'quarantined', 'cleanup_pending', 'cleanup_failed', 'removed')
    ),
    byte_size INTEGER NOT NULL DEFAULT 0 CHECK (byte_size >= 0),
    provenance_digest TEXT CHECK (provenance_digest IS NULL OR length(provenance_digest) = 64),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    UNIQUE (scratch_scope_id, generation)
) STRICT;
CREATE UNIQUE INDEX idx_chat_scratch_generations_active
ON chat_scratch_generations(scratch_scope_id)
WHERE lifecycle_state = 'active';

CREATE TABLE chat_agent_runs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    project_id TEXT NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    working_folder_id TEXT,
    execution_environment_id TEXT REFERENCES chat_execution_environments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    scratch_generation_id TEXT REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    teammate_policy_revision_id TEXT NOT NULL REFERENCES chat_teammate_policy_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    authorization_scope_digest TEXT NOT NULL CHECK (length(authorization_scope_digest) = 64),
    provider_turn_id TEXT NOT NULL UNIQUE REFERENCES chat_turns(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    provider_thread_id TEXT REFERENCES chat_threads(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'starting', 'working', 'waiting', 'completed', 'failed', 'cancelled')
    ),
    run_ordinal INTEGER NOT NULL CHECK (run_ordinal >= 1),
    started_at TEXT CHECK (started_at IS NULL OR length(started_at) >= 20),
    settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    FOREIGN KEY (working_folder_id, project_id)
        REFERENCES project_working_folders(id, project_id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    UNIQUE (assignment_id, run_ordinal),
    CHECK (
        (working_folder_id IS NULL AND execution_environment_id IS NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NOT NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NULL)
        OR (working_folder_id IS NULL AND execution_environment_id IS NOT NULL
            AND scratch_generation_id IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_agent_runs_authorization
ON chat_agent_runs(authorization_revision_id, state, id);
CREATE INDEX idx_chat_agent_runs_provider_thread
ON chat_agent_runs(provider_thread_id, assignment_id);

CREATE TABLE chat_scratch_cleanup_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scratch_scope_id TEXT NOT NULL REFERENCES chat_scratch_scopes(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    scratch_generation_id TEXT NOT NULL UNIQUE REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    expected_scope_revision INTEGER NOT NULL CHECK (expected_scope_revision >= 1),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'running', 'completed', 'failed', 'unavailable_on_device')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    removed_bytes INTEGER NOT NULL DEFAULT 0 CHECK (removed_bytes >= 0),
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128
    ),
    confirmed_at TEXT NOT NULL CHECK (length(confirmed_at) >= 20),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_scratch_cleanup_jobs_ready
ON chat_scratch_cleanup_jobs(state, available_at, created_at, id);

CREATE TABLE chat_scratch_generation_sources (
    scratch_generation_id TEXT NOT NULL REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    lower_ordinal INTEGER NOT NULL CHECK (lower_ordinal >= 1),
    high_ordinal INTEGER NOT NULL CHECK (high_ordinal >= lower_ordinal),
    audience_revision INTEGER NOT NULL CHECK (audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (scratch_generation_id, conversation_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_scratch_promotions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    scratch_generation_id TEXT NOT NULL REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    source_relative_path TEXT NOT NULL CHECK (
        length(source_relative_path) BETWEEN 1 AND 4096
        AND source_relative_path NOT LIKE '/%'
        AND source_relative_path NOT LIKE '../%'
        AND source_relative_path NOT LIKE '%/../%'
        AND source_relative_path NOT LIKE '%/..'
        AND source_relative_path NOT LIKE '%\%'
    ),
    source_content_revision TEXT NOT NULL CHECK (length(source_content_revision) = 64),
    source_sha256 TEXT CHECK (source_sha256 IS NULL OR length(source_sha256) = 64),
    request_digest TEXT CHECK (
        request_digest IS NULL OR (
            length(request_digest) = 64
            AND request_digest NOT GLOB '*[^0-9a-f]*'
        )
    ),
    destination_kind TEXT NOT NULL CHECK (
        destination_kind IN ('working_folder', 'managed_attachment')
    ),
    destination_channel_id TEXT NOT NULL REFERENCES chat_channels(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_working_folder_id TEXT REFERENCES project_working_folders(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    destination_relative_path TEXT CHECK (
        destination_relative_path IS NULL OR (
            length(destination_relative_path) BETWEEN 1 AND 4096
            AND destination_relative_path NOT LIKE '/%'
            AND destination_relative_path NOT LIKE '../%'
            AND destination_relative_path NOT LIKE '%/../%'
            AND destination_relative_path NOT LIKE '%/..'
            AND destination_relative_path NOT LIKE '%\%'
        )
    ),
    attachment_id TEXT CHECK (
        attachment_id IS NULL OR length(attachment_id) BETWEEN 1 AND 1024
    ),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'completed', 'failed')
    ),
    last_error_code TEXT CHECK (
        last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    completed_at TEXT CHECK (completed_at IS NULL OR length(completed_at) >= 20),
    CHECK (
        (destination_kind = 'working_folder'
            AND destination_working_folder_id IS NOT NULL
            AND destination_relative_path IS NOT NULL
            AND attachment_id IS NULL)
        OR (destination_kind = 'managed_attachment'
            AND destination_working_folder_id IS NULL
            AND destination_relative_path IS NULL
            AND attachment_id IS NOT NULL)
    )
) STRICT;
CREATE INDEX idx_chat_scratch_promotions_destination
ON chat_scratch_promotions(destination_channel_id, created_at DESC, id);
CREATE INDEX idx_chat_scratch_promotions_generation
ON chat_scratch_promotions(scratch_generation_id, created_at DESC, id);

CREATE TABLE chat_attachments (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE NO ACTION,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'text_snippet')),
    original_display_name TEXT NOT NULL CHECK (length(trim(original_display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 52428800),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL UNIQUE CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/..'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    signature_kind TEXT NOT NULL CHECK (length(signature_kind) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deletion_state TEXT NOT NULL DEFAULT 'active' CHECK (
        deletion_state IN ('active', 'pending_delete', 'deleted', 'cleanup_failed')
    ),
    unreferenced_at TEXT CHECK (unreferenced_at IS NULL OR length(unreferenced_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_attachments_cleanup
ON chat_attachments(deletion_state, unreferenced_at, id);
CREATE INDEX idx_chat_attachments_hash
ON chat_attachments(sha256, byte_size, id);

CREATE TABLE chat_attachment_references (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    attachment_id TEXT NOT NULL REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    message_id TEXT REFERENCES chat_messages(id) ON UPDATE CASCADE ON DELETE CASCADE,
    draft_id TEXT REFERENCES chat_drafts(id) ON UPDATE CASCADE ON DELETE CASCADE,
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    CHECK ((message_id IS NOT NULL) != (draft_id IS NOT NULL))
) STRICT;
CREATE INDEX idx_chat_attachment_references_draft
ON chat_attachment_references(draft_id, attachment_id);
CREATE INDEX idx_chat_attachment_references_message
ON chat_attachment_references(message_id, attachment_id);
CREATE UNIQUE INDEX idx_chat_attachment_references_unique_draft
ON chat_attachment_references(attachment_id, draft_id)
WHERE draft_id IS NOT NULL;
CREATE UNIQUE INDEX idx_chat_attachment_references_unique_message
ON chat_attachment_references(attachment_id, message_id)
WHERE message_id IS NOT NULL;

CREATE TABLE chat_communication_attachment_references (
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    attachment_id TEXT NOT NULL REFERENCES chat_attachments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (message_revision_id, attachment_id),
    UNIQUE (message_revision_id, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_queued_attachment_references (
    queued_followup_id TEXT NOT NULL REFERENCES chat_queued_followups(id) ON UPDATE CASCADE ON DELETE CASCADE,
    attachment_id TEXT NOT NULL REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (queued_followup_id, attachment_id)
) WITHOUT ROWID, STRICT;
CREATE INDEX idx_chat_queued_attachment_references_attachment
ON chat_queued_attachment_references(attachment_id, queued_followup_id);

CREATE TABLE chat_scheduled_message_attachment_references (
    scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    attachment_id TEXT NOT NULL REFERENCES chat_attachments(id)
        ON UPDATE CASCADE ON DELETE NO ACTION,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (scheduled_message_id, attachment_id),
    UNIQUE (scheduled_message_id, ordinal)
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_chat_scheduled_message_attachment_references_attachment
ON chat_scheduled_message_attachment_references(attachment_id, scheduled_message_id);

CREATE TABLE chat_terminal_attachment_contexts (
    attachment_id TEXT PRIMARY KEY NOT NULL
        REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    terminal_runtime_id TEXT NOT NULL CHECK (length(terminal_runtime_id) BETWEEN 1 AND 1024),
    terminal_name_snapshot TEXT NOT NULL CHECK (length(terminal_name_snapshot) BETWEEN 1 AND 240),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('selection', 'last_command_output')),
    start_output_sequence INTEGER CHECK (start_output_sequence IS NULL OR start_output_sequence >= 0),
    end_output_sequence INTEGER CHECK (end_output_sequence IS NULL OR end_output_sequence >= 0),
    line_count INTEGER NOT NULL CHECK (line_count >= 0),
    truncated INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0, 1)),
    captured_at TEXT NOT NULL CHECK (length(captured_at) >= 20)
) STRICT;

CREATE TABLE chat_resources (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE NO ACTION,
    attachment_id TEXT UNIQUE REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('image', 'text_snippet', 'browser_screenshot', 'browser_recording')),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 1073741824),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL UNIQUE CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    resource_uri TEXT NOT NULL UNIQUE CHECK (resource_uri GLOB 'ganbaru://chat/resource/*'),
    integrity_state TEXT NOT NULL DEFAULT 'verified' CHECK (
        integrity_state IN ('verified', 'missing', 'hash_mismatch', 'unsafe_path', 'deleted')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_resources_workspace_kind
ON chat_resources(working_folder_id, resource_kind, created_at DESC, id);

CREATE TABLE chat_browser_artifacts (
    resource_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_resources(id) ON UPDATE CASCADE ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    preview_tab_id TEXT REFERENCES chat_preview_tabs(id) ON UPDATE CASCADE ON DELETE SET NULL,
    artifact_kind TEXT NOT NULL CHECK (artifact_kind IN ('screenshot', 'recording')),
    source_url TEXT NOT NULL CHECK (length(source_url) <= 8192),
    viewport_width INTEGER NOT NULL CHECK (viewport_width BETWEEN 1 AND 16384),
    viewport_height INTEGER NOT NULL CHECK (viewport_height BETWEEN 1 AND 16384),
    duration_milliseconds INTEGER CHECK (duration_milliseconds IS NULL OR duration_milliseconds >= 0),
    frame_count INTEGER CHECK (frame_count IS NULL OR frame_count >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_browser_artifacts_thread
ON chat_browser_artifacts(thread_id, created_at DESC, resource_id);

CREATE TABLE chat_resource_thread_references (
    resource_id TEXT NOT NULL REFERENCES chat_resources(id) ON UPDATE CASCADE ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
    first_message_id TEXT REFERENCES chat_messages(id) ON UPDATE CASCADE ON DELETE SET NULL,
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (resource_id, thread_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE chat_cleanup_jobs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE NO ACTION,
    source_thread_id TEXT CHECK (source_thread_id IS NULL OR length(source_thread_id) BETWEEN 1 AND 1024),
    cleanup_kind TEXT NOT NULL CHECK (
        cleanup_kind IN ('checkpoint_ref', 'attachment_file', 'diagnostic_event')
    ),
    exact_target TEXT NOT NULL CHECK (length(exact_target) BETWEEN 1 AND 4096),
    repository_identity TEXT CHECK (
        repository_identity IS NULL OR length(repository_identity) BETWEEN 1 AND 1024
    ),
    expected_object_id TEXT CHECK (
        expected_object_id IS NULL OR length(expected_object_id) BETWEEN 40 AND 128
    ),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'running', 'failed', 'completed')
    ),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    available_at TEXT NOT NULL CHECK (length(available_at) >= 20),
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    UNIQUE (cleanup_kind, exact_target)
) STRICT;
CREATE INDEX idx_chat_cleanup_jobs_due
ON chat_cleanup_jobs(state, available_at, id);

CREATE TABLE chat_organizational_command_receipts (
    client_command_id TEXT PRIMARY KEY NOT NULL CHECK (length(client_command_id) BETWEEN 1 AND 1024),
    command_kind TEXT NOT NULL CHECK (
        command_kind IN ('post_message', 'publish_teammate', 'update_membership', 'assignment_action')
    ),
    state TEXT NOT NULL CHECK (state IN ('accepted', 'completed', 'failed')),
    result_schema_version INTEGER CHECK (result_schema_version IS NULL OR result_schema_version >= 1),
    result_data TEXT CHECK (result_data IS NULL OR json_valid(result_data)),
    error_data TEXT CHECK (error_data IS NULL OR json_valid(error_data)),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

-- Chat triggers.

CREATE TRIGGER chat_teammate_create_access_state
AFTER INSERT ON chat_teammates
BEGIN
    INSERT INTO chat_teammate_access_state (teammate_id, updated_at)
    VALUES (NEW.participant_id, NEW.updated_at);
END;

CREATE TRIGGER chat_teammate_policy_identity_insert
BEFORE INSERT ON chat_teammate_policy_revisions
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_participants participant
    WHERE participant.id = NEW.teammate_id
      AND participant.participant_kind = 'ai_teammate'
)
BEGIN
    SELECT RAISE(ABORT, 'Teammate policies require an AI teammate participant');
END;

CREATE TRIGGER chat_teammate_policy_immutable_delete
BEFORE DELETE ON chat_teammate_policy_revisions
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_participants participant
    WHERE participant.id = OLD.teammate_id
      AND participant.participant_kind = 'ai_teammate'
      AND participant.archived_at IS NOT NULL
)
OR EXISTS (
    SELECT 1
    FROM chat_communication_messages message
    WHERE message.author_participant_id = OLD.teammate_id
)
OR EXISTS (
    SELECT 1
    FROM chat_participant_reference_targets target
    WHERE target.participant_id = OLD.teammate_id
)
OR EXISTS (
    SELECT 1
    FROM chat_work_assignments assignment
    WHERE assignment.teammate_id = OLD.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'Teammate policy revisions are immutable');
END;

CREATE TRIGGER chat_teammate_policy_immutable_update
BEFORE UPDATE ON chat_teammate_policy_revisions
BEGIN
    SELECT RAISE(ABORT, 'Teammate policy revisions are immutable');
END;

CREATE TRIGGER chat_teammate_policy_publish
AFTER INSERT ON chat_teammate_policy_revisions
BEGIN
    UPDATE chat_teammates
    SET latest_policy_revision = NEW.revision,
        updated_at = NEW.created_at
    WHERE participant_id = NEW.teammate_id
      AND latest_policy_revision < NEW.revision;
END;

CREATE TRIGGER chat_teammate_access_revision_monotonic
BEFORE UPDATE OF access_revision ON chat_teammate_access_state
WHEN NEW.access_revision <= OLD.access_revision
BEGIN
    SELECT RAISE(ABORT, 'Teammate access revisions must increase');
END;

CREATE TRIGGER chat_access_profile_builtin_protected
BEFORE UPDATE ON chat_access_profiles
WHEN OLD.builtin_key IS NOT NULL AND (
    NEW.id IS NOT OLD.id
    OR NEW.builtin_key IS NOT OLD.builtin_key
    OR NEW.display_name IS NOT OLD.display_name
    OR NEW.latest_revision IS NOT OLD.latest_revision
    OR NEW.revision IS NOT OLD.revision
    OR NEW.archived_at IS NOT OLD.archived_at
    OR NEW.created_at IS NOT OLD.created_at
    OR NEW.updated_at IS NOT OLD.updated_at
)
BEGIN
    SELECT RAISE(ABORT, 'Built-in access profiles are protected');
END;

CREATE TRIGGER chat_access_profile_latest_revision_monotonic
BEFORE UPDATE OF latest_revision ON chat_access_profiles
WHEN NEW.latest_revision <= OLD.latest_revision
BEGIN
    SELECT RAISE(ABORT, 'Latest access profile revisions must increase');
END;

CREATE TRIGGER chat_access_profile_revision_counter_monotonic
BEFORE UPDATE OF revision ON chat_access_profiles
WHEN NEW.revision <= OLD.revision
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions must increase');
END;

CREATE TRIGGER chat_access_profile_audience_publish
AFTER INSERT ON chat_access_profile_revisions
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.created_at
    WHERE conversation_id IN (
        SELECT membership.conversation_id
        FROM chat_teammate_channel_memberships membership
        WHERE membership.access_profile_id = NEW.access_profile_id
    );
END;

CREATE TRIGGER chat_access_profile_builtin_revision_protected
BEFORE INSERT ON chat_access_profile_revisions
WHEN NEW.revision > 1 AND EXISTS (
    SELECT 1 FROM chat_access_profiles profile
    WHERE profile.id = NEW.access_profile_id
      AND profile.builtin_key IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Built-in access profiles are immutable');
END;

CREATE TRIGGER chat_access_profile_publish
AFTER INSERT ON chat_access_profile_revisions
BEGIN
    UPDATE chat_access_profiles
    SET latest_revision = NEW.revision,
        revision = revision + 1,
        updated_at = NEW.created_at
    WHERE id = NEW.access_profile_id
      AND latest_revision < NEW.revision;
END;

CREATE TRIGGER chat_access_profile_revision_immutable_delete
BEFORE DELETE ON chat_access_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions are immutable');
END;

CREATE TRIGGER chat_access_profile_revision_immutable_update
BEFORE UPDATE ON chat_access_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'Access profile revisions are immutable');
END;

CREATE TRIGGER chat_conversation_create_audience_state
AFTER INSERT ON chat_conversations
BEGIN
    INSERT INTO chat_conversation_audience_state (conversation_id, updated_at)
    VALUES (NEW.id, NEW.updated_at);
END;

CREATE TRIGGER chat_generic_membership_audience_delete
AFTER DELETE ON chat_conversation_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_generic_membership_audience_insert
AFTER INSERT ON chat_conversation_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_generic_membership_audience_update
AFTER UPDATE ON chat_conversation_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_delete
AFTER DELETE ON chat_teammate_channel_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_insert
AFTER INSERT ON chat_teammate_channel_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_update
AFTER UPDATE ON chat_teammate_channel_memberships
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

CREATE TRIGGER chat_teammate_channel_membership_identity_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM chat_teammates teammate
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires an AI teammate');
END;

CREATE TRIGGER chat_teammate_folder_grant_identity_insert
BEFORE INSERT ON chat_teammate_working_folder_grants
WHEN NOT EXISTS (
    SELECT 1
    FROM chat_teammates teammate
    JOIN chat_conversation_memberships membership
      ON membership.conversation_id = NEW.conversation_id
     AND membership.participant_id = NEW.teammate_id
     AND membership.removed_at IS NULL
    JOIN chat_teammate_channel_memberships ai_membership
      ON ai_membership.conversation_id = membership.conversation_id
     AND ai_membership.teammate_id = membership.participant_id
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'Working folder grants require active AI channel access');
END;

CREATE TRIGGER chat_conversation_audience_revision_monotonic
BEFORE UPDATE OF revision ON chat_conversation_audience_state
WHEN NEW.revision <= OLD.revision
BEGIN
    SELECT RAISE(ABORT, 'Conversation audience revisions must increase');
END;

CREATE TRIGGER chat_channels_conversation_kind_insert
BEFORE INSERT ON chat_channels
WHEN NOT EXISTS (
    SELECT 1 FROM chat_conversations conversation
    WHERE conversation.id = NEW.conversation_id
      AND conversation.project_id = NEW.project_id
      AND conversation.conversation_kind = 'channel'
)
BEGIN
    SELECT RAISE(ABORT, 'Chat channel conversation is invalid');
END;

CREATE TRIGGER chat_channels_default_name_insert
BEFORE INSERT ON chat_channels
WHEN NEW.is_default = 1 AND NEW.name != 'general'
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel must be named general');
END;

CREATE TRIGGER chat_channels_default_protected_update
BEFORE UPDATE OF name, is_default, archived_at ON chat_channels
WHEN OLD.is_default = 1 AND (
    NEW.name != OLD.name OR NEW.is_default != 1 OR NEW.archived_at IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'The default Chat channel is protected');
END;

CREATE TRIGGER chat_reply_threads_root_item_insert
BEFORE INSERT ON chat_reply_threads
WHEN NOT EXISTS (
    SELECT 1 FROM chat_conversation_items item
    WHERE item.id = NEW.root_item_id
      AND item.conversation_id = NEW.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.item_kind = 'message'
)
BEGIN
    SELECT RAISE(ABORT, 'Reply thread root must be a root communication message');
END;

CREATE TRIGGER chat_scheduled_messages_reply_thread_insert
BEFORE INSERT ON chat_scheduled_messages
WHEN NEW.reply_thread_id IS NOT NULL AND NOT EXISTS (
    SELECT 1
    FROM chat_reply_threads thread
    JOIN chat_channels channel ON channel.conversation_id = thread.conversation_id
    WHERE thread.id = NEW.reply_thread_id
      AND channel.id = NEW.channel_id
)
BEGIN
    SELECT RAISE(ABORT, 'Scheduled reply belongs to another channel');
END;

CREATE TRIGGER chat_conversation_items_reply_thread_insert
BEFORE INSERT ON chat_conversation_items
WHEN NEW.reply_thread_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM chat_reply_threads thread
    WHERE thread.id = NEW.reply_thread_id
      AND thread.conversation_id = NEW.conversation_id
)
BEGIN
    SELECT RAISE(ABORT, 'Reply item belongs to another conversation');
END;

CREATE TRIGGER chat_communication_author_snapshot_immutable
BEFORE UPDATE OF author_label_snapshot ON chat_communication_messages
WHEN NEW.author_label_snapshot IS NOT OLD.author_label_snapshot
BEGIN
    SELECT RAISE(ABORT, 'Communication message author snapshots are immutable');
END;

CREATE TRIGGER chat_communication_message_current_revision_update
BEFORE UPDATE OF current_revision_id ON chat_communication_messages
WHEN NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM chat_communication_message_revisions revision
    WHERE revision.id = NEW.current_revision_id
      AND revision.message_item_id = NEW.item_id
)
BEGIN
    SELECT RAISE(ABORT, 'Current message revision belongs to another message');
END;

CREATE TRIGGER chat_communication_search_delete
AFTER DELETE ON chat_communication_messages
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = OLD.item_id;
END;

CREATE TRIGGER chat_communication_search_insert
AFTER UPDATE OF current_revision_id ON chat_communication_messages
WHEN NEW.current_revision_id IS NOT NULL
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = NEW.item_id;
    INSERT INTO chat_communication_search_fts (
        message_item_id, conversation_id, reply_thread_id,
        author_display_name, normalized_markdown
    )
    SELECT
        NEW.item_id,
        item.conversation_id,
        item.reply_thread_id,
        participant.display_name,
        revision.normalized_markdown
    FROM chat_conversation_items item
    JOIN chat_participants participant ON participant.id = NEW.author_participant_id
    JOIN chat_communication_message_revisions revision
      ON revision.id = NEW.current_revision_id
    WHERE item.id = NEW.item_id;
END;

CREATE TRIGGER chat_communication_message_revision_assign_ordinal
AFTER INSERT ON chat_communication_message_revisions
BEGIN
    INSERT INTO chat_communication_message_revision_ordinals (message_revision_id)
    VALUES (NEW.id);
END;

CREATE TRIGGER chat_communication_message_revision_immutable_update
BEFORE UPDATE ON chat_communication_message_revisions
BEGIN
    SELECT RAISE(ABORT, 'Communication message revisions are immutable');
END;

CREATE TRIGGER chat_execution_environment_authority_immutable
BEFORE UPDATE OF
    id, working_folder_id, scratch_generation_id, kind, repository_identity
ON chat_execution_environments
BEGIN
    SELECT RAISE(ABORT, 'Execution environment authority is immutable');
END;

CREATE TRIGGER chat_threads_assign_current_environment
AFTER INSERT ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NEW.execution_environment_id IS NULL
BEGIN
    UPDATE chat_threads
    SET execution_environment_id = 'current-folder:' || NEW.working_folder_id
    WHERE id = NEW.id;
END;

CREATE TRIGGER chat_threads_execution_target_insert
BEFORE INSERT ON chat_threads
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR (NEW.working_folder_id IS NOT NULL
        AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.working_folder_id = NEW.working_folder_id
                AND NEW.scratch_generation_id IS NULL
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND NEW.scratch_generation_id IS NOT NULL
                AND environment.scratch_generation_id = NEW.scratch_generation_id
                AND environment.working_folder_id IS NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread execution target is inconsistent');
END;

CREATE TRIGGER chat_threads_execution_target_update
BEFORE UPDATE OF working_folder_id, execution_environment_id, scratch_generation_id ON chat_threads
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.working_folder_id = NEW.working_folder_id
                AND NEW.scratch_generation_id IS NULL
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND NEW.scratch_generation_id IS NOT NULL
                AND environment.scratch_generation_id = NEW.scratch_generation_id
                AND environment.working_folder_id IS NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread execution target is inconsistent');
END;

CREATE TRIGGER chat_threads_project_matches_working_folder_insert
BEFORE INSERT ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM project_working_folders
    WHERE id = NEW.working_folder_id AND project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread project must match its working folder');
END;

CREATE TRIGGER chat_threads_project_matches_working_folder_update
BEFORE UPDATE OF working_folder_id, project_id ON chat_threads
WHEN NEW.working_folder_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM project_working_folders
    WHERE id = NEW.working_folder_id AND project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat thread project must match its working folder');
END;

CREATE TRIGGER chat_events_require_next_sequence
BEFORE INSERT ON chat_events
WHEN NEW.sequence != (
    SELECT last_event_sequence + 1 FROM chat_threads WHERE id = NEW.thread_id
)
BEGIN
    SELECT RAISE(ABORT, 'Chat event sequence must be contiguous');
END;

CREATE TRIGGER chat_assignment_authorization_delete_denied
BEFORE DELETE ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization revisions are immutable');
END;

CREATE TRIGGER chat_assignment_authorization_id_immutable
BEFORE UPDATE OF id ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization identity is immutable');
END;

CREATE TRIGGER chat_assignment_authorization_reason_immutable
BEFORE UPDATE OF reason ON chat_assignment_authorization_revisions
WHEN NEW.reason IS NOT OLD.reason
 AND NOT (OLD.decision_state = 'allowed' AND NEW.decision_state = 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'Authorization reasons are immutable before revocation');
END;

CREATE TRIGGER chat_assignment_authorization_revocation_insert
BEFORE INSERT ON chat_assignment_authorization_revisions
WHEN (NEW.decision_state = 'revoked') != (NEW.revoked_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Authorization revocation state is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_revocation_update
BEFORE UPDATE OF decision_state, revoked_at ON chat_assignment_authorization_revisions
WHEN (NEW.decision_state = 'revoked') != (NEW.revoked_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'Authorization revocation state is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_scope_immutable
BEFORE UPDATE OF
    assignment_id, revision, requester_participant_id,
    teammate_policy_revision_id, teammate_access_revision,
    destination_conversation_id, access_profile_revision_id,
    execution_environment_id, working_folder_id,
    resolved_runtime_approval_policy, scope_digest, created_at
ON chat_assignment_authorization_revisions
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization scopes are immutable');
END;

CREATE TRIGGER chat_assignment_authorization_state_transition
BEFORE UPDATE OF decision_state ON chat_assignment_authorization_revisions
WHEN NEW.decision_state IS NOT OLD.decision_state
 AND NOT (OLD.decision_state = 'allowed' AND NEW.decision_state = 'revoked')
BEGIN
    SELECT RAISE(ABORT, 'Authorization decisions require a new revision');
END;

CREATE TRIGGER chat_assignment_authorization_target_required_insert
BEFORE INSERT ON chat_assignment_authorization_revisions
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.kind IN ('current_folder', 'worktree')
                AND environment.working_folder_id = NEW.working_folder_id
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND environment.kind = 'scratch'
                AND environment.working_folder_id IS NULL
                AND environment.scratch_generation_id IS NOT NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Assignment authorization target is inconsistent');
END;

CREATE TRIGGER chat_assignment_authorization_terminal_immutable
BEFORE UPDATE ON chat_assignment_authorization_revisions
WHEN OLD.decision_state = 'revoked' OR OLD.revoked_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'Revoked authorization revisions are immutable');
END;

CREATE TRIGGER chat_authorized_channel_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_channel_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

CREATE TRIGGER chat_authorized_channel_source_immutable_update
BEFORE UPDATE ON chat_assignment_authorized_channel_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

CREATE TRIGGER chat_authorized_folder_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_folder_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources are immutable');
END;

CREATE TRIGGER chat_authorized_folder_source_immutable_update
BEFORE UPDATE ON chat_assignment_authorized_folder_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources are immutable');
END;

CREATE TRIGGER chat_authorized_folder_source_requires_approval
BEFORE INSERT ON chat_assignment_authorized_folder_sources
WHEN NEW.resolved_runtime_approval_policy IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Authorized folder sources require a frozen approval policy');
END;

CREATE TRIGGER chat_scratch_scope_identity_immutable
BEFORE UPDATE OF id, reply_thread_id, teammate_id, created_at ON chat_scratch_scopes
BEGIN
    SELECT RAISE(ABORT, 'Scratch scope identity is immutable');
END;

CREATE TRIGGER chat_scratch_generation_identity_immutable
BEFORE UPDATE OF id, scratch_scope_id, generation, created_at ON chat_scratch_generations
BEGIN
    SELECT RAISE(ABORT, 'Scratch generation identity is immutable');
END;

CREATE TRIGGER chat_agent_run_authority_immutable
BEFORE UPDATE OF
    assignment_id, project_id, working_folder_id,
    execution_environment_id, scratch_generation_id,
    teammate_policy_revision_id, authorization_revision_id,
    authorization_scope_digest, provider_turn_id,
    provider_thread_id, run_ordinal, created_at
ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run authority is immutable');
END;

CREATE TRIGGER chat_agent_run_authorization_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions authorization
    WHERE authorization.id = NEW.authorization_revision_id
      AND authorization.assignment_id = NEW.assignment_id
      AND authorization.scope_digest = NEW.authorization_scope_digest
      AND authorization.decision_state = 'allowed'
      AND authorization.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run authorization is invalid');
END;

CREATE TRIGGER chat_agent_run_authorization_target_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions authorization
    WHERE authorization.id = NEW.authorization_revision_id
      AND authorization.assignment_id = NEW.assignment_id
      AND authorization.scope_digest = NEW.authorization_scope_digest
      AND authorization.execution_environment_id IS NEW.execution_environment_id
      AND authorization.working_folder_id IS NEW.working_folder_id
      AND authorization.decision_state = 'allowed'
      AND authorization.revoked_at IS NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run target does not match its authorization');
END;

CREATE TRIGGER chat_agent_run_execution_target_insert
BEFORE INSERT ON chat_agent_runs
WHEN NOT (
    (NEW.working_folder_id IS NULL AND NEW.execution_environment_id IS NULL
        AND NEW.scratch_generation_id IS NULL)
    OR EXISTS (
        SELECT 1 FROM chat_execution_environments environment
        WHERE environment.id = NEW.execution_environment_id
          AND (
            (NEW.working_folder_id IS NOT NULL
                AND environment.working_folder_id = NEW.working_folder_id
                AND NEW.scratch_generation_id IS NULL
                AND environment.scratch_generation_id IS NULL)
            OR (NEW.working_folder_id IS NULL
                AND NEW.scratch_generation_id IS NOT NULL
                AND environment.scratch_generation_id = NEW.scratch_generation_id
                AND environment.working_folder_id IS NULL)
          )
    )
)
BEGIN
    SELECT RAISE(ABORT, 'Agent run execution target is inconsistent');
END;

CREATE TRIGGER chat_agent_run_id_immutable
BEFORE UPDATE OF id ON chat_agent_runs
BEGIN
    SELECT RAISE(ABORT, 'Agent run identity is immutable');
END;

CREATE TRIGGER chat_scratch_generation_source_delete_denied
BEFORE DELETE ON chat_scratch_generation_sources
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints are immutable');
END;

CREATE TRIGGER chat_scratch_generation_source_monotonic
BEFORE UPDATE ON chat_scratch_generation_sources
WHEN NEW.scratch_generation_id IS NOT OLD.scratch_generation_id
  OR NEW.conversation_id IS NOT OLD.conversation_id
  OR NEW.lower_ordinal > OLD.lower_ordinal
  OR NEW.high_ordinal < OLD.high_ordinal
  OR NEW.audience_revision < OLD.audience_revision
  OR NEW.created_at IS NOT OLD.created_at
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints can only expand');
END;

CREATE TRIGGER chat_scratch_attachment_promotion_completed
BEFORE UPDATE OF state ON chat_scratch_promotions
WHEN NEW.state = 'completed'
 AND NEW.destination_kind = 'managed_attachment'
 AND NOT EXISTS (
    SELECT 1
    FROM chat_attachments attachment
    JOIN project_working_folders storage_folder
      ON storage_folder.id = attachment.working_folder_id
     AND storage_folder.kind = 'managed'
     AND storage_folder.archived_at IS NULL
    JOIN chat_channels channel
      ON channel.id = NEW.destination_channel_id
     AND channel.project_id = storage_folder.project_id
    WHERE attachment.id = NEW.attachment_id
 )
BEGIN
    SELECT RAISE(ABORT, 'Completed scratch attachment promotion requires its channel attachment');
END;

CREATE TRIGGER chat_scratch_promotion_audit_delete_denied
BEFORE DELETE ON chat_scratch_promotions
BEGIN
    SELECT RAISE(ABORT, 'Scratch promotion audit rows cannot be deleted');
END;

CREATE TRIGGER chat_scratch_promotion_authority_immutable
BEFORE UPDATE OF scratch_generation_id, source_relative_path, source_content_revision,
    request_digest, destination_kind, destination_channel_id,
    destination_working_folder_id, destination_relative_path, attachment_id, created_at
ON chat_scratch_promotions
BEGIN
    SELECT RAISE(ABORT, 'Scratch promotion authority is immutable');
END;

CREATE TRIGGER chat_scratch_promotion_completion_provenance
BEFORE UPDATE OF state ON chat_scratch_promotions
WHEN NEW.state = 'completed' AND NEW.source_sha256 IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Completed scratch promotion requires source provenance');
END;

CREATE TRIGGER chat_scratch_promotion_request_digest_required
BEFORE INSERT ON chat_scratch_promotions
WHEN NEW.request_digest IS NULL
BEGIN
    SELECT RAISE(ABORT, 'New scratch promotions require an idempotency digest');
END;

CREATE TRIGGER chat_scratch_promotion_terminal_immutable
BEFORE UPDATE ON chat_scratch_promotions
WHEN OLD.state != 'pending'
BEGIN
    SELECT RAISE(ABORT, 'Terminal scratch promotion is immutable');
END;

CREATE TRIGGER chat_attachments_create_resource
AFTER INSERT ON chat_attachments
BEGIN
    INSERT INTO chat_resources (
        id,
        working_folder_id,
        attachment_id,
        resource_kind,
        display_name,
        mime_type,
        byte_size,
        sha256,
        managed_relative_path,
        resource_uri,
        integrity_state,
        created_at
    ) VALUES (
        NEW.id,
        NEW.working_folder_id,
        NEW.id,
        NEW.kind,
        NEW.original_display_name,
        NEW.mime_type,
        NEW.byte_size,
        NEW.sha256,
        NEW.managed_relative_path,
        'ganbaru://chat/resource/' || NEW.id,
        'verified',
        NEW.created_at
    );
END;

CREATE TRIGGER chat_attachments_update_resource_integrity
AFTER UPDATE OF deletion_state, deleted_at ON chat_attachments
BEGIN
    UPDATE chat_resources
    SET integrity_state = CASE NEW.deletion_state
            WHEN 'deleted' THEN 'deleted'
            ELSE integrity_state
        END,
        deleted_at = NEW.deleted_at
    WHERE attachment_id = NEW.id;
END;

CREATE TRIGGER chat_attachment_messages_expose_resource
AFTER INSERT ON chat_attachment_references
WHEN NEW.message_id IS NOT NULL
BEGIN
    INSERT OR IGNORE INTO chat_resource_thread_references (
        resource_id,
        thread_id,
        first_message_id,
        created_at
    )
    SELECT NEW.attachment_id, thread_id, id, NEW.created_at
    FROM chat_messages
    WHERE id = NEW.message_id;
END;


-- Seed data.

INSERT INTO calendars (id, name, color, source, visible, read_only)
VALUES ('local', 'Ganbaru AI', '', 'local', 1, 0);

INSERT INTO project_groups (id, name, icon, color, sort_order)
VALUES ('group-routine', 'Routine', 'lucide:repeat', 0, 0);

INSERT INTO chat_participants (
    id, participant_kind, display_name, created_at, updated_at
) VALUES (
    'participant:local-owner',
    'local_user',
    'You',
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);

INSERT INTO chat_access_profiles (
    id, builtin_key, display_name, created_at, updated_at
) VALUES
    (
        'access-profile:conversation-only', 'conversation_only', 'Conversation only',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile:read-only', 'read_only', 'Read only',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile:edit-files', 'edit_files', 'Edit files',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile:build-and-test', 'build_and_test', 'Build and test',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile:publish-changes', 'publish_changes', 'Publish changes',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );

INSERT INTO chat_access_profile_revisions (
    id, access_profile_id, revision, default_read_history, default_participate,
    default_history_boundary, maximum_folder_capability, created_at
) VALUES
    (
        'access-profile-revision:conversation-only:1',
        'access-profile:conversation-only', 1, 1, 1, 'entire', 'none',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile-revision:read-only:1',
        'access-profile:read-only', 1, 1, 1, 'entire', 'read',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile-revision:edit-files:1',
        'access-profile:edit-files', 1, 1, 1, 'entire', 'edit',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile-revision:build-and-test:1',
        'access-profile:build-and-test', 1, 1, 1, 'entire', 'execute',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    ),
    (
        'access-profile-revision:publish-changes:1',
        'access-profile:publish-changes', 1, 1, 1, 'entire', 'publish',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );

INSERT INTO projects (
    id, group_id, name, icon, color, sort_order,
    default_pomodoro_mode, default_pomodoro_preset_key
)
VALUES
    ('project-routine-learning', 'group-routine', 'Learning', 'lucide:graduation-cap', 8, 0, 'preset', 'adaptive'),
    ('project-routine-reading', 'group-routine', 'Reading', 'lucide:book-open', 25, 10, 'none', NULL),
    ('project-routine-exercise', 'group-routine', 'Exercise', 'lucide:sport-shoe', 0, 20, 'none', NULL),
    ('project-routine-hygiene', 'group-routine', 'Hygiene', 'lucide:bath', 15, 30, 'none', NULL),
    ('project-routine-eat', 'group-routine', 'Eating', 'lucide:apple', 13, 40, 'none', NULL),
    ('project-routine-commute', 'group-routine', 'Commute', 'lucide:bike', 17, 50, 'none', NULL),
    ('project-routine-social', 'group-routine', 'Social', 'lucide:heart', 21, 60, 'none', NULL),
    ('project-routine-chores', 'group-routine', 'Chores', 'lucide:shopping-cart', 4, 70, 'none', NULL),
    ('project-routine-leisure', 'group-routine', 'Leisure', 'lucide:clapperboard', 31, 80, 'none', NULL),
    ('project-routine-meditate', 'group-routine', 'Meditate', 'lucide:smile', 23, 90, 'none', NULL),
    ('project-routine-health', 'group-routine', 'Health', 'lucide:pill', 3, 100, 'none', NULL),
    ('project-routine-sleep', 'group-routine', 'Sleep', 'lucide:bed', 30, 110, 'none', NULL);

INSERT INTO project_working_folders (
    id, project_id, display_name, kind, managed_relative_path, sort_order
)
SELECT 'working-folder-' || substr(id, length('project-') + 1),
       id,
       name,
       'managed',
       'projects/' || id,
       0
FROM projects;

INSERT INTO project_sections (id, project_id, name, sort_order)
SELECT 'section-' || substr(id, length('project-') + 1) || '-general', id, 'General', 0
FROM projects;

INSERT INTO project_statuses (
    id, project_id, name, category, color, sort_order, terminal
)
SELECT 'status-' || substr(id, length('project-') + 1) || '-' || status.slug,
       id, status.name, status.category, status.color, status.sort_order, status.terminal
FROM projects
CROSS JOIN (
    SELECT 'backlog' AS slug, 'Backlog' AS name, 'not_started' AS category, 30 AS color, 0 AS sort_order, 0 AS terminal
    UNION ALL SELECT 'todo', 'To do', 'not_started', 31, 10, 0
    UNION ALL SELECT 'in-progress', 'In progress', 'active', 19, 20, 0
    UNION ALL SELECT 'in-review', 'In review', 'active', 23, 30, 0
    UNION ALL SELECT 'blocked', 'Blocked', 'blocked', 2, 40, 0
    UNION ALL SELECT 'done', 'Done', 'done', 13, 50, 1
) AS status;

INSERT INTO project_priorities (
    id, project_id, name, color, sort_order
)
SELECT priority.id, project.id, priority.name, priority.color, priority.sort_order
FROM projects AS project
CROSS JOIN (
    SELECT 'urgent' AS id, 'Urgent' AS name, 2 AS color, 0 AS sort_order
    UNION ALL SELECT 'high', 'High', 7, 10
    UNION ALL SELECT 'normal', 'Normal', 19, 20
    UNION ALL SELECT 'low', 'Low', 30, 30
) AS priority;

INSERT INTO notes_local_users (id, display_name)
VALUES ('69bb434b-3734-3467-574f-fb8175def649', 'You');

INSERT INTO notes_page_history_settings (id, retention_days)
VALUES (1, 30);

INSERT INTO notes_history_maintenance_state (id, last_run_at)
VALUES (1, NULL);

INSERT INTO music_soundscapes (
    id, source_kind, generated_kind, bundled_identity, name,
    availability, created_at_ms, updated_at_ms, version
) VALUES
    ('generated-white-noise', 'generated-noise', 'white', NULL, 'White noise', 'available', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1),
    ('generated-pink-noise', 'generated-noise', 'pink', NULL, 'Pink noise', 'available', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1),
    ('generated-brown-noise', 'generated-noise', 'brown', NULL, 'Brown noise', 'available', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1);

INSERT INTO music_soundscape_state (
    singleton, active_soundscape_id, desired_playing, volume, updated_at_ms, version
) VALUES (1, NULL, 0, 0.1, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1);

INSERT INTO music_playlists (
    id, name, icon, shuffle_enabled, repeat_mode, sort_order,
    created_at_ms, updated_at_ms, version
)
VALUES
    ('playlist-default-start-of-day', 'Start of the day!', 'lucide:sunrise', 1, 'all', 0, 1, 1, 1),
    ('playlist-default-work-focus', 'Work (focus)', 'lucide:laptop', 1, 'all', 1, 1, 1, 1),
    ('playlist-default-work-ganbare', 'Work (ganbare!)', 'lucide:coffee', 1, 'all', 2, 1, 1, 1),
    ('playlist-default-break-calm', 'Break (calm)', 'lucide:armchair', 1, 'all', 3, 1, 1, 1),
    ('playlist-default-break-active', 'Break (active)', 'lucide:footprints', 1, 'all', 4, 1, 1, 1),
    ('playlist-default-meditate', 'Meditate', 'lucide:smile', 1, 'all', 5, 1, 1, 1),
    ('playlist-default-exercise', 'Exercise', 'lucide:sport-shoe', 1, 'all', 6, 1, 1, 1),
    ('playlist-default-hygiene', 'Hygiene', 'lucide:bath', 1, 'all', 7, 1, 1, 1),
    ('playlist-default-chores', 'Chores', 'lucide:shopping-cart', 1, 'all', 8, 1, 1, 1),
    ('playlist-default-cooking', 'Cooking', 'lucide:apple', 1, 'all', 9, 1, 1, 1),
    ('playlist-default-commute', 'Commute', 'lucide:bike', 1, 'all', 10, 1, 1, 1);
INSERT INTO pomodoro_execution_state (singleton, state_json, updated_at_ms)
VALUES (1, '{}', 0);

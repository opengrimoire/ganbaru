-- no-transaction
-- Calendar replicates through the vault manifest. Every child table of an event, an archive, an
-- iCalendar object, or a component is owned: its primary key is the owner and a data column, or
-- an ordinal fixed at zero for a one-row table, and its only foreign key cascades from the owner.
-- Links between owned rows (a parameter's property, a value node's parent, an override property's
-- override, a count rhythm's configuration) are plain columns that Calendar writers keep
-- consistent. Override, alarm, and attendee component ids are plain optional text, read as no
-- component when the component is gone. Event coordinates are integer millionths of a degree,
-- and preserved number values hold their JSON literal as text, so no replicated column is REAL.
-- An event cascades with its calendar. Every iCalendar object records the import that wrote it,
-- one id per earlier import batch. Archive import objects and task event links take a sync_key
-- over their composite key; placeholder keys 'pending:' || rowid are rewritten by the Calendar
-- and Projects conversion passes at vault open.
--
-- SQLite changes a CHECK or a table constraint only by rebuilding the table. Foreign keys are
-- disabled for the rebuild and every copy keeps its values. Legacy rename keeps SQLite from
-- reparsing references on other tables while a rebuilt table is briefly absent. The rebuild
-- commits only when the foreign key check reports no violations.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- icalendar_objects: one import id per earlier batch of objects written together.
CREATE TEMP TABLE calendar_replication_imports (
    calendar_id TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    import_id TEXT
);
INSERT INTO calendar_replication_imports (calendar_id, source_kind, source_name, created_at)
SELECT DISTINCT calendar_id, source_kind, source_name, created_at FROM icalendar_objects;
UPDATE calendar_replication_imports SET import_id = lower(hex(randomblob(16)));

CREATE TABLE icalendar_objects_next (
    id TEXT PRIMARY KEY,
    import_id TEXT NOT NULL CHECK (trim(import_id) <> ''),
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
INSERT INTO icalendar_objects_next (
    id, import_id, calendar_id, source_kind, source_name, source_fingerprint, prodid, version,
    method, calendar_scale, created_at, updated_at
)
SELECT
    o.id, i.import_id, o.calendar_id, o.source_kind, o.source_name, o.source_fingerprint,
    o.prodid, o.version, o.method, o.calendar_scale, o.created_at, o.updated_at
FROM icalendar_objects o
JOIN calendar_replication_imports i
    ON i.calendar_id = o.calendar_id
    AND i.source_kind = o.source_kind
    AND i.source_name = o.source_name
    AND i.created_at = o.created_at;
DROP TABLE calendar_replication_imports;
DROP TABLE icalendar_objects;
ALTER TABLE icalendar_objects_next RENAME TO icalendar_objects;
CREATE INDEX idx_icalendar_objects_calendar ON icalendar_objects(calendar_id);
CREATE INDEX idx_icalendar_objects_fingerprint ON icalendar_objects(source_fingerprint);
CREATE INDEX idx_icalendar_objects_import ON icalendar_objects(import_id);
CREATE INDEX idx_icalendar_objects_source ON icalendar_objects(calendar_id, source_kind, source_name);

-- icalendar_object_diagnostics
CREATE TABLE icalendar_object_diagnostics_next (
    object_id TEXT NOT NULL REFERENCES icalendar_objects(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    message TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (object_id, id)
);
INSERT INTO icalendar_object_diagnostics_next (object_id, id, message, sort_order)
SELECT object_id, id, message, max(sort_order, 0) FROM icalendar_object_diagnostics;
DROP TABLE icalendar_object_diagnostics;
ALTER TABLE icalendar_object_diagnostics_next RENAME TO icalendar_object_diagnostics;

-- icalendar_components keeps its columns; the graph below is flattened under it.
-- icalendar_component_projection_warnings
CREATE TABLE icalendar_component_projection_warnings_next (
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    message TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (component_id, id)
);
INSERT INTO icalendar_component_projection_warnings_next (component_id, id, message, sort_order)
SELECT component_id, id, message, max(sort_order, 0) FROM icalendar_component_projection_warnings;
DROP TABLE icalendar_component_projection_warnings;
ALTER TABLE icalendar_component_projection_warnings_next
    RENAME TO icalendar_component_projection_warnings;

-- icalendar_component_properties
CREATE TABLE icalendar_component_properties_next (
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    name TEXT NOT NULL,
    value_type TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (component_id, id)
);
INSERT INTO icalendar_component_properties_next (component_id, id, name, value_type, sort_order)
SELECT component_id, id, name, value_type, max(sort_order, 0) FROM icalendar_component_properties;

-- icalendar_property_parameters, flat under the component.
CREATE TABLE icalendar_property_parameters_next (
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_id TEXT NOT NULL CHECK (trim(property_id) <> ''),
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (component_id, id)
);
INSERT INTO icalendar_property_parameters_next (component_id, id, property_id, name, sort_order)
SELECT p.component_id, a.id, a.property_id, a.name, max(a.sort_order, 0)
FROM icalendar_property_parameters a
JOIN icalendar_component_properties p ON p.id = a.property_id;

-- icalendar_value_nodes, flat under the component. CAST of a REAL to TEXT keeps 15 significant
-- digits, which is the precision the REAL column stored.
CREATE TABLE icalendar_value_nodes_next (
    component_id TEXT NOT NULL REFERENCES icalendar_components(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_id TEXT NOT NULL CHECK (trim(property_id) <> ''),
    parameter_id TEXT,
    parent_node_id TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    value_kind TEXT NOT NULL CHECK (value_kind IN ('array', 'object', 'text', 'number', 'boolean', 'null')),
    object_key TEXT,
    text_value TEXT,
    number_value TEXT,
    boolean_value INTEGER,
    PRIMARY KEY (component_id, id)
);
INSERT INTO icalendar_value_nodes_next (
    component_id, id, property_id, parameter_id, parent_node_id, sort_order, value_kind,
    object_key, text_value, number_value, boolean_value
)
SELECT
    p.component_id, n.id, n.property_id, n.parameter_id, n.parent_node_id, max(n.sort_order, 0),
    n.value_kind, n.object_key, n.text_value,
    CASE
        WHEN typeof(n.number_value) = 'integer' THEN CAST(n.number_value AS TEXT)
        WHEN n.number_value IS NULL THEN NULL
        ELSE CAST(n.number_value AS TEXT)
    END,
    n.boolean_value
FROM icalendar_value_nodes n
JOIN icalendar_component_properties p ON p.id = n.property_id;
DROP TABLE icalendar_value_nodes;
ALTER TABLE icalendar_value_nodes_next RENAME TO icalendar_value_nodes;
CREATE INDEX idx_icalendar_value_nodes_property ON icalendar_value_nodes(property_id, parent_node_id, sort_order);
CREATE INDEX idx_icalendar_value_nodes_parameter ON icalendar_value_nodes(parameter_id, parent_node_id, sort_order);
CREATE INDEX idx_icalendar_value_nodes_parent ON icalendar_value_nodes(parent_node_id, sort_order);

DROP TABLE icalendar_property_parameters;
ALTER TABLE icalendar_property_parameters_next RENAME TO icalendar_property_parameters;
CREATE INDEX idx_icalendar_property_parameters_property ON icalendar_property_parameters(property_id, sort_order);

DROP TABLE icalendar_component_properties;
ALTER TABLE icalendar_component_properties_next RENAME TO icalendar_component_properties;

-- calendar_events: geo in millionths of a degree, and the event cascades with its calendar.
CREATE TABLE calendar_events_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    title TEXT NOT NULL DEFAULT '',
    start_time TEXT NOT NULL CHECK (trim(start_time) <> ''),
    end_time TEXT NOT NULL CHECK (trim(end_time) <> ''),
    timezone TEXT NOT NULL DEFAULT 'UTC' CHECK (trim(timezone) <> ''),
    calendar_id TEXT NOT NULL DEFAULT 'local' REFERENCES calendars(id) ON DELETE CASCADE,
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
    geo_lat INTEGER CHECK (geo_lat IS NULL OR (geo_lat >= -90000000 AND geo_lat <= 90000000)),
    geo_lng INTEGER CHECK (geo_lng IS NULL OR (geo_lng >= -180000000 AND geo_lng <= 180000000)),
    sequence INTEGER NOT NULL DEFAULT 0 CHECK (sequence >= 0),
    guest_can_modify INTEGER NOT NULL DEFAULT 0 CHECK (guest_can_modify IN (0, 1)),
    guest_can_invite_others INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_invite_others IN (0, 1)),
    guest_can_see_other_guests INTEGER NOT NULL DEFAULT 1 CHECK (guest_can_see_other_guests IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    icalendar_component_id TEXT REFERENCES icalendar_components(id) ON DELETE SET NULL,
    local_rsvp_status TEXT CHECK (local_rsvp_status IS NULL OR local_rsvp_status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    meeting_enabled INTEGER NOT NULL DEFAULT 0 CHECK (meeting_enabled IN (0, 1)),
    CHECK ((geo_lat IS NULL) = (geo_lng IS NULL))
);
INSERT INTO calendar_events_next (
    id, title, start_time, end_time, timezone, calendar_id, project_id, color, description, rrule,
    repeat_until, environment_id, playlist_id, all_day, location, url, transparency, status,
    source_uid, visibility, priority, geo_lat, geo_lng, sequence, guest_can_modify,
    guest_can_invite_others, guest_can_see_other_guests, created_at, updated_at,
    icalendar_component_id, local_rsvp_status, meeting_enabled
)
SELECT
    id, title, start_time, end_time, timezone, calendar_id, project_id, color, description, rrule,
    repeat_until, environment_id, playlist_id, all_day, location, url, transparency, status,
    source_uid, visibility, priority,
    CAST(round(geo_lat * 1000000) AS INTEGER), CAST(round(geo_lng * 1000000) AS INTEGER),
    sequence, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests, created_at,
    updated_at, icalendar_component_id, local_rsvp_status, meeting_enabled
FROM calendar_events;

-- calendar_event_exdates, keyed by the excluded date.
CREATE TABLE calendar_event_exdates_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    occurrence_date TEXT NOT NULL CHECK (trim(occurrence_date) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    PRIMARY KEY (event_id, occurrence_date)
);
INSERT INTO calendar_event_exdates_next (event_id, occurrence_date, id)
SELECT event_id, occurrence_date, id FROM calendar_event_exdates;
DROP TABLE calendar_event_exdates;
ALTER TABLE calendar_event_exdates_next RENAME TO calendar_event_exdates;

-- calendar_event_rdates, keyed by the added start.
CREATE TABLE calendar_event_rdates_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    occurrence_start TEXT NOT NULL CHECK (trim(occurrence_start) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    PRIMARY KEY (event_id, occurrence_start)
);
INSERT INTO calendar_event_rdates_next (event_id, occurrence_start, id)
SELECT event_id, occurrence_start, id FROM calendar_event_rdates;
DROP TABLE calendar_event_rdates;
ALTER TABLE calendar_event_rdates_next RENAME TO calendar_event_rdates;

-- calendar_event_overrides, keyed by the recurrence id.
CREATE TABLE calendar_event_overrides_next (
    parent_event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    recurrence_id TEXT NOT NULL CHECK (trim(recurrence_id) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
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
    icalendar_component_id TEXT,
    recurrence_range TEXT CHECK (recurrence_range IS NULL OR recurrence_range = 'this-and-future'),
    PRIMARY KEY (parent_event_id, recurrence_id)
);
INSERT INTO calendar_event_overrides_next (
    parent_event_id, recurrence_id, id, title, start_time, end_time, description, location, url,
    color, status, transparency, visibility, created_at, updated_at, icalendar_component_id,
    recurrence_range
)
SELECT
    parent_event_id, recurrence_id, id, title, start_time, end_time, description, location, url,
    color, status, transparency, visibility, created_at, updated_at, icalendar_component_id,
    recurrence_range
FROM calendar_event_overrides;

-- calendar_event_override_extended_properties, flat under the event.
CREATE TABLE calendar_event_override_extended_properties_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    override_id TEXT NOT NULL CHECK (trim(override_id) <> ''),
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (event_id, override_id, property_key)
);
INSERT INTO calendar_event_override_extended_properties_next (
    event_id, override_id, property_key, id, property_value, sort_order
)
SELECT o.parent_event_id, p.override_id, p.property_key, p.id, p.property_value, p.sort_order
FROM calendar_event_override_extended_properties p
JOIN calendar_event_overrides o ON o.id = p.override_id;
DROP TABLE calendar_event_override_extended_properties;
ALTER TABLE calendar_event_override_extended_properties_next
    RENAME TO calendar_event_override_extended_properties;
CREATE INDEX idx_calendar_event_override_extended_properties_override
    ON calendar_event_override_extended_properties(override_id);

DROP TABLE calendar_event_overrides;
ALTER TABLE calendar_event_overrides_next RENAME TO calendar_event_overrides;
CREATE INDEX idx_calendar_event_overrides_icalendar_component ON calendar_event_overrides(icalendar_component_id);
CREATE INDEX idx_calendar_event_overrides_id ON calendar_event_overrides(id);

-- calendar_event_alarms
CREATE TABLE calendar_event_alarms_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    action TEXT NOT NULL DEFAULT 'display' CHECK (action IN ('display', 'audio', 'email')),
    trigger_type TEXT NOT NULL DEFAULT 'relative' CHECK (trigger_type IN ('relative', 'absolute')),
    trigger_value TEXT NOT NULL CHECK (trim(trigger_value) <> ''),
    description TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT,
    PRIMARY KEY (event_id, id)
);
INSERT INTO calendar_event_alarms_next (
    event_id, id, action, trigger_type, trigger_value, description, sort_order,
    icalendar_component_id
)
SELECT
    event_id, id, action, trigger_type, trigger_value, description, sort_order,
    icalendar_component_id
FROM calendar_event_alarms;
DROP TABLE calendar_event_alarms;
ALTER TABLE calendar_event_alarms_next RENAME TO calendar_event_alarms;
CREATE INDEX idx_calendar_event_alarms_icalendar_component ON calendar_event_alarms(icalendar_component_id);

-- calendar_event_attendees
CREATE TABLE calendar_event_attendees_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    role TEXT NOT NULL DEFAULT 'req-participant' CHECK (role IN ('chair', 'req-participant', 'opt-participant', 'non-participant')),
    status TEXT NOT NULL DEFAULT 'needs-action' CHECK (status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    rsvp INTEGER NOT NULL DEFAULT 0 CHECK (rsvp IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT,
    icalendar_property_index INTEGER CHECK (icalendar_property_index IS NULL OR icalendar_property_index >= 0),
    PRIMARY KEY (event_id, id)
);
INSERT INTO calendar_event_attendees_next (
    event_id, id, name, email, role, status, rsvp, sort_order, icalendar_component_id,
    icalendar_property_index
)
SELECT
    event_id, id, name, email, role, status, rsvp, sort_order, icalendar_component_id,
    icalendar_property_index
FROM calendar_event_attendees;
DROP TABLE calendar_event_attendees;
ALTER TABLE calendar_event_attendees_next RENAME TO calendar_event_attendees;
CREATE INDEX idx_calendar_event_attendees_icalendar_component ON calendar_event_attendees(icalendar_component_id);

-- calendar_event_categories
CREATE TABLE calendar_event_categories_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    category TEXT NOT NULL CHECK (trim(category) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (event_id, id)
);
INSERT INTO calendar_event_categories_next (event_id, id, category, sort_order)
SELECT event_id, id, category, sort_order FROM calendar_event_categories;
DROP TABLE calendar_event_categories;
ALTER TABLE calendar_event_categories_next RENAME TO calendar_event_categories;

-- calendar_event_extended_properties, keyed by the property key.
CREATE TABLE calendar_event_extended_properties_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (event_id, property_key)
);
INSERT INTO calendar_event_extended_properties_next (
    event_id, property_key, id, property_value, sort_order
)
SELECT event_id, property_key, id, property_value, sort_order
FROM calendar_event_extended_properties;
DROP TABLE calendar_event_extended_properties;
ALTER TABLE calendar_event_extended_properties_next RENAME TO calendar_event_extended_properties;

-- calendar_event_notifications
CREATE TABLE calendar_event_notifications_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    offset_minutes INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (event_id, id)
);
INSERT INTO calendar_event_notifications_next (event_id, id, offset_minutes, sort_order)
SELECT event_id, id, offset_minutes, sort_order FROM calendar_event_notifications;
DROP TABLE calendar_event_notifications;
ALTER TABLE calendar_event_notifications_next RENAME TO calendar_event_notifications;

-- calendar_event_organizers
CREATE TABLE calendar_event_organizers_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    PRIMARY KEY (event_id, ordinal)
);
INSERT INTO calendar_event_organizers_next (event_id, ordinal, name, email)
SELECT event_id, 0, name, email FROM calendar_event_organizers;
DROP TABLE calendar_event_organizers;
ALTER TABLE calendar_event_organizers_next RENAME TO calendar_event_organizers;

-- calendar_event_pomodoro_configs
CREATE TABLE calendar_event_pomodoro_configs_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER CHECK (idle_timeout_minutes IS NULL OR idle_timeout_minutes > 0),
    PRIMARY KEY (event_id, ordinal)
);
INSERT INTO calendar_event_pomodoro_configs_next (
    event_id, ordinal, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes
)
SELECT event_id, 0, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes
FROM calendar_event_pomodoro_configs;

-- calendar_event_pomodoro_config_count_rhythms, flat under the event.
CREATE TABLE calendar_event_pomodoro_config_count_rhythms_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    ),
    PRIMARY KEY (event_id, ordinal)
);
INSERT INTO calendar_event_pomodoro_config_count_rhythms_next (
    event_id, ordinal, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
)
SELECT
    event_id, 0, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
FROM calendar_event_pomodoro_config_count_rhythms;

-- calendar_event_pomodoro_config_sequence_steps, flat under the event.
CREATE TABLE calendar_event_pomodoro_config_sequence_steps_next (
    event_id TEXT NOT NULL REFERENCES calendar_events(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL CHECK (break_phase IN ('short_break', 'long_break')),
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (event_id, step_index)
);
INSERT INTO calendar_event_pomodoro_config_sequence_steps_next (
    event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
)
SELECT event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
FROM calendar_event_pomodoro_config_sequence_steps;
DROP TABLE calendar_event_pomodoro_config_sequence_steps;
DROP TABLE calendar_event_pomodoro_config_count_rhythms;
DROP TABLE calendar_event_pomodoro_configs;
ALTER TABLE calendar_event_pomodoro_configs_next RENAME TO calendar_event_pomodoro_configs;
ALTER TABLE calendar_event_pomodoro_config_count_rhythms_next
    RENAME TO calendar_event_pomodoro_config_count_rhythms;
ALTER TABLE calendar_event_pomodoro_config_sequence_steps_next
    RENAME TO calendar_event_pomodoro_config_sequence_steps;

DROP TABLE calendar_events;
ALTER TABLE calendar_events_next RENAME TO calendar_events;
CREATE INDEX idx_calendar_events_calendar ON calendar_events(calendar_id);
CREATE INDEX idx_calendar_events_end ON calendar_events(end_time);
CREATE INDEX idx_calendar_events_icalendar_component ON calendar_events(icalendar_component_id);
CREATE INDEX idx_calendar_events_project ON calendar_events(project_id, start_time);
CREATE UNIQUE INDEX idx_calendar_events_source_uid ON calendar_events(calendar_id, source_uid);
CREATE INDEX idx_calendar_events_start ON calendar_events(start_time);

-- calendar_event_archives: geo in millionths of a degree.
CREATE TABLE calendar_event_archives_next (
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
    geo_lat INTEGER CHECK (geo_lat IS NULL OR (geo_lat >= -90000000 AND geo_lat <= 90000000)),
    geo_lng INTEGER CHECK (geo_lng IS NULL OR (geo_lng >= -180000000 AND geo_lng <= 180000000)),
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
    CHECK ((geo_lat IS NULL) = (geo_lng IS NULL))
);
INSERT INTO calendar_event_archives_next (
    id, source_event_id, archived_at, title, start_time, end_time, timezone, calendar_id,
    project_id, color, description, rrule, repeat_until, environment_id, playlist_id, all_day,
    location, url, transparency, status, source_uid, visibility, priority, geo_lat, geo_lng,
    sequence, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests, created_at,
    updated_at, icalendar_component_id, local_rsvp_status, meeting_enabled,
    original_occurrence_id, recurrence_date
)
SELECT
    id, source_event_id, archived_at, title, start_time, end_time, timezone, calendar_id,
    project_id, color, description, rrule, repeat_until, environment_id, playlist_id, all_day,
    location, url, transparency, status, source_uid, visibility, priority,
    CAST(round(geo_lat * 1000000) AS INTEGER), CAST(round(geo_lng * 1000000) AS INTEGER),
    sequence, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests, created_at,
    updated_at, icalendar_component_id, local_rsvp_status, meeting_enabled,
    original_occurrence_id, recurrence_date
FROM calendar_event_archives;

-- calendar_event_archive_alarms
CREATE TABLE calendar_event_archive_alarms_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    source_alarm_id TEXT NOT NULL CHECK (trim(source_alarm_id) <> ''),
    action TEXT NOT NULL DEFAULT 'display' CHECK (action IN ('display', 'audio', 'email')),
    trigger_type TEXT NOT NULL DEFAULT 'relative' CHECK (trigger_type IN ('relative', 'absolute')),
    trigger_value TEXT NOT NULL CHECK (trim(trigger_value) <> ''),
    description TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT,
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_alarms_next (
    archive_event_id, id, source_alarm_id, action, trigger_type, trigger_value, description,
    sort_order, icalendar_component_id
)
SELECT
    archive_event_id, id, source_alarm_id, action, trigger_type, trigger_value, description,
    sort_order, icalendar_component_id
FROM calendar_event_archive_alarms;
DROP TABLE calendar_event_archive_alarms;
ALTER TABLE calendar_event_archive_alarms_next RENAME TO calendar_event_archive_alarms;
CREATE INDEX idx_calendar_event_archive_alarms_icalendar_component ON calendar_event_archive_alarms(icalendar_component_id);

-- calendar_event_archive_attendees
CREATE TABLE calendar_event_archive_attendees_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    source_attendee_id TEXT NOT NULL CHECK (trim(source_attendee_id) <> ''),
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    role TEXT NOT NULL DEFAULT 'req-participant' CHECK (role IN ('chair', 'req-participant', 'opt-participant', 'non-participant')),
    status TEXT NOT NULL DEFAULT 'needs-action' CHECK (status IN ('needs-action', 'accepted', 'declined', 'tentative', 'delegated')),
    rsvp INTEGER NOT NULL DEFAULT 0 CHECK (rsvp IN (0, 1)),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    icalendar_component_id TEXT,
    icalendar_property_index INTEGER CHECK (icalendar_property_index IS NULL OR icalendar_property_index >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_attendees_next (
    archive_event_id, id, source_attendee_id, name, email, role, status, rsvp, sort_order,
    icalendar_component_id, icalendar_property_index
)
SELECT
    archive_event_id, id, source_attendee_id, name, email, role, status, rsvp, sort_order,
    icalendar_component_id, icalendar_property_index
FROM calendar_event_archive_attendees;
DROP TABLE calendar_event_archive_attendees;
ALTER TABLE calendar_event_archive_attendees_next RENAME TO calendar_event_archive_attendees;
CREATE INDEX idx_calendar_event_archive_attendees_icalendar_component ON calendar_event_archive_attendees(icalendar_component_id);

-- calendar_event_archive_categories
CREATE TABLE calendar_event_archive_categories_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    category TEXT NOT NULL CHECK (trim(category) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_categories_next (archive_event_id, id, category, sort_order)
SELECT archive_event_id, id, category, sort_order FROM calendar_event_archive_categories;
DROP TABLE calendar_event_archive_categories;
ALTER TABLE calendar_event_archive_categories_next RENAME TO calendar_event_archive_categories;

-- calendar_event_archive_exdates
CREATE TABLE calendar_event_archive_exdates_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    occurrence_date TEXT NOT NULL CHECK (trim(occurrence_date) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_exdates_next (archive_event_id, id, occurrence_date, sort_order)
SELECT archive_event_id, id, occurrence_date, sort_order FROM calendar_event_archive_exdates;
DROP TABLE calendar_event_archive_exdates;
ALTER TABLE calendar_event_archive_exdates_next RENAME TO calendar_event_archive_exdates;

-- calendar_event_archive_extended_properties
CREATE TABLE calendar_event_archive_extended_properties_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_extended_properties_next (
    archive_event_id, id, property_key, property_value, sort_order
)
SELECT archive_event_id, id, property_key, property_value, sort_order
FROM calendar_event_archive_extended_properties;
DROP TABLE calendar_event_archive_extended_properties;
ALTER TABLE calendar_event_archive_extended_properties_next
    RENAME TO calendar_event_archive_extended_properties;

-- calendar_event_archive_music_assignments keeps its key and columns.
CREATE TABLE calendar_event_archive_music_assignments_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('event-snapshot', 'event-override')),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short-break', 'long-break')),
    behavior TEXT NOT NULL CHECK (behavior IN (
            'inherit', 'play-automatically', 'prepare-silently', 'pause-music', 'keep-current-music'
        )),
    playlist_id TEXT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN ('explicit', 'copied-project', 'work-environment')),
    provenance_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL CHECK (version > 0),
    soundscape_behavior TEXT NOT NULL CHECK (soundscape_behavior IN (
            'inherit', 'play-selected', 'pause-soundscape', 'keep-current-soundscape'
        )),
    soundscape_preset_id TEXT CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> ''),
    PRIMARY KEY (archive_event_id, owner_kind, phase)
);
INSERT INTO calendar_event_archive_music_assignments_next (
    archive_event_id, owner_kind, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id
)
SELECT
    archive_event_id, owner_kind, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id
FROM calendar_event_archive_music_assignments;
DROP TABLE calendar_event_archive_music_assignments;
ALTER TABLE calendar_event_archive_music_assignments_next
    RENAME TO calendar_event_archive_music_assignments;

-- calendar_event_archive_notifications
CREATE TABLE calendar_event_archive_notifications_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    offset_minutes INTEGER NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_notifications_next (
    archive_event_id, id, offset_minutes, sort_order
)
SELECT archive_event_id, id, offset_minutes, sort_order FROM calendar_event_archive_notifications;
DROP TABLE calendar_event_archive_notifications;
ALTER TABLE calendar_event_archive_notifications_next RENAME TO calendar_event_archive_notifications;

-- calendar_event_archive_organizers
CREATE TABLE calendar_event_archive_organizers_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    name TEXT,
    email TEXT NOT NULL CHECK (trim(email) <> ''),
    PRIMARY KEY (archive_event_id, ordinal)
);
INSERT INTO calendar_event_archive_organizers_next (archive_event_id, ordinal, name, email)
SELECT archive_event_id, 0, name, email FROM calendar_event_archive_organizers;
DROP TABLE calendar_event_archive_organizers;
ALTER TABLE calendar_event_archive_organizers_next RENAME TO calendar_event_archive_organizers;

-- calendar_event_archive_overrides, keyed by the archived override's id.
CREATE TABLE calendar_event_archive_overrides_next (
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
    recurrence_range TEXT CHECK (recurrence_range IS NULL OR recurrence_range = 'this-and-future'),
    PRIMARY KEY (archive_event_id, source_override_id)
);
INSERT INTO calendar_event_archive_overrides_next (
    archive_event_id, source_override_id, recurrence_id, title, start_time, end_time, description,
    location, url, color, status, transparency, visibility, created_at, updated_at,
    icalendar_component_id, recurrence_range
)
SELECT
    archive_event_id, source_override_id, recurrence_id, title, start_time, end_time, description,
    location, url, color, status, transparency, visibility, created_at, updated_at,
    icalendar_component_id, recurrence_range
FROM calendar_event_archive_overrides;

-- calendar_event_archive_override_extended_properties, flat under the archive.
CREATE TABLE calendar_event_archive_override_extended_properties_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    source_override_id TEXT NOT NULL CHECK (trim(source_override_id) <> ''),
    property_key TEXT NOT NULL CHECK (trim(property_key) <> ''),
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    property_value TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, source_override_id, property_key)
);
INSERT INTO calendar_event_archive_override_extended_properties_next (
    archive_event_id, source_override_id, property_key, id, property_value, sort_order
)
SELECT o.archive_event_id, o.source_override_id, p.property_key, p.id, p.property_value, p.sort_order
FROM calendar_event_archive_override_extended_properties p
JOIN calendar_event_archive_overrides o ON o.id = p.archive_override_id;
DROP TABLE calendar_event_archive_override_extended_properties;
ALTER TABLE calendar_event_archive_override_extended_properties_next
    RENAME TO calendar_event_archive_override_extended_properties;
DROP TABLE calendar_event_archive_overrides;
ALTER TABLE calendar_event_archive_overrides_next RENAME TO calendar_event_archive_overrides;
CREATE INDEX idx_calendar_event_archive_overrides_icalendar_component ON calendar_event_archive_overrides(icalendar_component_id);

-- calendar_event_archive_pomodoro_configs
CREATE TABLE calendar_event_archive_pomodoro_configs_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER CHECK (idle_timeout_minutes IS NULL OR idle_timeout_minutes > 0),
    PRIMARY KEY (archive_event_id, ordinal)
);
INSERT INTO calendar_event_archive_pomodoro_configs_next (
    archive_event_id, ordinal, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes
)
SELECT archive_event_id, 0, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes
FROM calendar_event_archive_pomodoro_configs;

-- calendar_event_archive_pomodoro_config_count_rhythms, flat under the archive.
CREATE TABLE calendar_event_archive_pomodoro_config_count_rhythms_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    ),
    PRIMARY KEY (archive_event_id, ordinal)
);
INSERT INTO calendar_event_archive_pomodoro_config_count_rhythms_next (
    archive_event_id, ordinal, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
)
SELECT
    archive_event_id, 0, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
FROM calendar_event_archive_pomodoro_config_count_rhythms;

-- calendar_event_archive_pomodoro_config_sequence_steps, flat under the archive.
CREATE TABLE calendar_event_archive_pomodoro_config_sequence_steps_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL CHECK (break_phase IN ('short_break', 'long_break')),
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (archive_event_id, step_index)
);
INSERT INTO calendar_event_archive_pomodoro_config_sequence_steps_next (
    archive_event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
)
SELECT archive_event_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
FROM calendar_event_archive_pomodoro_config_sequence_steps;
DROP TABLE calendar_event_archive_pomodoro_config_sequence_steps;
DROP TABLE calendar_event_archive_pomodoro_config_count_rhythms;
DROP TABLE calendar_event_archive_pomodoro_configs;
ALTER TABLE calendar_event_archive_pomodoro_configs_next
    RENAME TO calendar_event_archive_pomodoro_configs;
ALTER TABLE calendar_event_archive_pomodoro_config_count_rhythms_next
    RENAME TO calendar_event_archive_pomodoro_config_count_rhythms;
ALTER TABLE calendar_event_archive_pomodoro_config_sequence_steps_next
    RENAME TO calendar_event_archive_pomodoro_config_sequence_steps;

-- calendar_event_archive_rdates
CREATE TABLE calendar_event_archive_rdates_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    id TEXT NOT NULL CHECK (trim(id) <> ''),
    occurrence_start TEXT NOT NULL CHECK (trim(occurrence_start) <> ''),
    sort_order INTEGER NOT NULL DEFAULT 0 CHECK (sort_order >= 0),
    PRIMARY KEY (archive_event_id, id)
);
INSERT INTO calendar_event_archive_rdates_next (archive_event_id, id, occurrence_start, sort_order)
SELECT archive_event_id, id, occurrence_start, sort_order FROM calendar_event_archive_rdates;
DROP TABLE calendar_event_archive_rdates;
ALTER TABLE calendar_event_archive_rdates_next RENAME TO calendar_event_archive_rdates;

-- calendar_event_archive_task_links keeps its key and columns; it is rebuilt with its archive.
CREATE TABLE calendar_event_archive_task_links_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL CHECK (trim(task_id) <> ''),
    link_kind TEXT NOT NULL CHECK (link_kind IN ('scheduled', 'reference')),
    created_at TEXT NOT NULL CHECK (trim(created_at) <> ''),
    PRIMARY KEY (archive_event_id, task_id)
);
INSERT INTO calendar_event_archive_task_links_next (archive_event_id, task_id, link_kind, created_at)
SELECT archive_event_id, task_id, link_kind, created_at FROM calendar_event_archive_task_links;
DROP TABLE calendar_event_archive_task_links;
ALTER TABLE calendar_event_archive_task_links_next RENAME TO calendar_event_archive_task_links;

-- calendar_event_archive_import_objects carries a sync_key over its composite key.
CREATE TABLE calendar_event_archive_import_objects_next (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL REFERENCES icalendar_objects(id) ON DELETE CASCADE,
    sync_key TEXT NOT NULL CHECK (length(sync_key) <= 128),
    PRIMARY KEY (archive_event_id, object_id)
);
INSERT INTO calendar_event_archive_import_objects_next (archive_event_id, object_id, sync_key)
SELECT archive_event_id, object_id, 'pending:' || rowid FROM calendar_event_archive_import_objects;
DROP TABLE calendar_event_archive_import_objects;
ALTER TABLE calendar_event_archive_import_objects_next RENAME TO calendar_event_archive_import_objects;
CREATE INDEX idx_calendar_event_archive_import_objects_object
    ON calendar_event_archive_import_objects(object_id);
CREATE UNIQUE INDEX idx_calendar_event_archive_import_objects_sync_key
    ON calendar_event_archive_import_objects(sync_key);

DROP TABLE calendar_event_archives;
ALTER TABLE calendar_event_archives_next RENAME TO calendar_event_archives;
CREATE INDEX idx_calendar_event_archives_calendar ON calendar_event_archives(calendar_id);
CREATE INDEX idx_calendar_event_archives_icalendar_component ON calendar_event_archives(icalendar_component_id);
CREATE INDEX idx_calendar_event_archives_source ON calendar_event_archives(source_event_id);

-- Ordered child reads keep their owner and order indexes across the rebuilds.
CREATE INDEX idx_icalendar_object_diagnostics_object
    ON icalendar_object_diagnostics(object_id, sort_order);
CREATE INDEX idx_icalendar_component_projection_warnings_component
    ON icalendar_component_projection_warnings(component_id, sort_order);
CREATE INDEX idx_icalendar_component_properties_component
    ON icalendar_component_properties(component_id, sort_order);
CREATE INDEX idx_calendar_event_categories_event ON calendar_event_categories(event_id, sort_order);
CREATE INDEX idx_calendar_event_notifications_event
    ON calendar_event_notifications(event_id, sort_order);
CREATE INDEX idx_calendar_event_archive_overrides_source
    ON calendar_event_archive_overrides(source_override_id);

-- project_task_event_links carries a sync_key over its composite key.
ALTER TABLE project_task_event_links ADD COLUMN sync_key TEXT NOT NULL DEFAULT ''
    CHECK (length(sync_key) <= 128);
UPDATE project_task_event_links SET sync_key = 'pending:' || rowid;
CREATE UNIQUE INDEX idx_project_task_event_links_sync_key ON project_task_event_links(sync_key);

CREATE TRIGGER calendar_event_archive_import_objects_sync_key_insert
BEFORE INSERT ON calendar_event_archive_import_objects
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Calendar archive import object sync key is required');
END;

CREATE TRIGGER calendar_event_archive_import_objects_sync_key_update
BEFORE UPDATE OF sync_key ON calendar_event_archive_import_objects
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Calendar archive import object sync key is required');
END;

CREATE TRIGGER project_task_event_links_sync_key_insert
BEFORE INSERT ON project_task_event_links
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task event link sync key is required');
END;

CREATE TRIGGER project_task_event_links_sync_key_update
BEFORE UPDATE OF sync_key ON project_task_event_links
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task event link sync key is required');
END;

CREATE TEMP TABLE calendar_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO calendar_replication_guard (violations)
SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE calendar_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.

CREATE TRIGGER sync_values_calendars_insert
BEFORE INSERT ON calendars
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 262144 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'text' AND instr(CAST(NEW.color AS BLOB), x'00') = 0 AND length(NEW.color) <= 262144 AND length(CAST(NEW.color AS BLOB)) <= 262144)
    AND (typeof(NEW.source) = 'text' AND instr(CAST(NEW.source AS BLOB), x'00') = 0 AND length(NEW.source) <= 32 AND length(CAST(NEW.source AS BLOB)) <= 262144)
    AND (typeof(NEW.read_only) = 'integer' AND NEW.read_only IN (0, 1))
    AND (NEW.source_url IS NULL OR (typeof(NEW.source_url) = 'text' AND instr(CAST(NEW.source_url AS BLOB), x'00') = 0 AND length(NEW.source_url) <= 262144 AND length(CAST(NEW.source_url AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_calendars_update
BEFORE UPDATE OF created_at, name, color, source, read_only, source_url, updated_at ON calendars
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 262144 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'text' AND instr(CAST(NEW.color AS BLOB), x'00') = 0 AND length(NEW.color) <= 262144 AND length(CAST(NEW.color AS BLOB)) <= 262144)
    AND (typeof(NEW.source) = 'text' AND instr(CAST(NEW.source AS BLOB), x'00') = 0 AND length(NEW.source) <= 32 AND length(CAST(NEW.source AS BLOB)) <= 262144)
    AND (typeof(NEW.read_only) = 'integer' AND NEW.read_only IN (0, 1))
    AND (NEW.source_url IS NULL OR (typeof(NEW.source_url) = 'text' AND instr(CAST(NEW.source_url AS BLOB), x'00') = 0 AND length(NEW.source_url) <= 262144 AND length(CAST(NEW.source_url AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_calendars
BEFORE UPDATE OF id, created_at ON calendars
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_calendars
BEFORE INSERT ON calendars
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 160 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM calendars WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_calendars_insert
AFTER INSERT ON calendars
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (160, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_calendars_update
AFTER UPDATE ON calendars
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 160, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.source IS NOT NEW.source COLLATE BINARY OR OLD.read_only IS NOT NEW.read_only OR OLD.source_url IS NOT NEW.source_url COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendars_delete
AFTER DELETE ON calendars
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (160, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_icalendar_objects_insert
BEFORE INSERT ON icalendar_objects
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.import_id) = 'text' AND instr(CAST(NEW.import_id AS BLOB), x'00') = 0 AND length(trim(NEW.import_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.import_id AS BLOB)) <= 262144)
    AND (typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.source_name) = 'text' AND instr(CAST(NEW.source_name AS BLOB), x'00') = 0 AND length(NEW.source_name) <= 262144 AND length(CAST(NEW.source_name AS BLOB)) <= 262144)
    AND (typeof(NEW.source_fingerprint) = 'text' AND instr(CAST(NEW.source_fingerprint AS BLOB), x'00') = 0 AND length(NEW.source_fingerprint) <= 262144 AND length(CAST(NEW.source_fingerprint AS BLOB)) <= 262144)
    AND (NEW.prodid IS NULL OR (typeof(NEW.prodid) = 'text' AND instr(CAST(NEW.prodid AS BLOB), x'00') = 0 AND length(NEW.prodid) <= 262144 AND length(CAST(NEW.prodid AS BLOB)) <= 262144))
    AND (NEW.version IS NULL OR (typeof(NEW.version) = 'text' AND instr(CAST(NEW.version AS BLOB), x'00') = 0 AND length(NEW.version) <= 262144 AND length(CAST(NEW.version AS BLOB)) <= 262144))
    AND (NEW.method IS NULL OR (typeof(NEW.method) = 'text' AND instr(CAST(NEW.method AS BLOB), x'00') = 0 AND length(NEW.method) <= 262144 AND length(CAST(NEW.method AS BLOB)) <= 262144))
    AND (NEW.calendar_scale IS NULL OR (typeof(NEW.calendar_scale) = 'text' AND instr(CAST(NEW.calendar_scale AS BLOB), x'00') = 0 AND length(NEW.calendar_scale) <= 262144 AND length(CAST(NEW.calendar_scale AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_icalendar_objects_update
BEFORE UPDATE OF import_id, source_kind, source_name, source_fingerprint, prodid, version, method, calendar_scale, created_at, calendar_id, updated_at ON icalendar_objects
WHEN ((typeof(NEW.import_id) = 'text' AND instr(CAST(NEW.import_id AS BLOB), x'00') = 0 AND length(trim(NEW.import_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.import_id AS BLOB)) <= 262144)
    AND (typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.source_name) = 'text' AND instr(CAST(NEW.source_name AS BLOB), x'00') = 0 AND length(NEW.source_name) <= 262144 AND length(CAST(NEW.source_name AS BLOB)) <= 262144)
    AND (typeof(NEW.source_fingerprint) = 'text' AND instr(CAST(NEW.source_fingerprint AS BLOB), x'00') = 0 AND length(NEW.source_fingerprint) <= 262144 AND length(CAST(NEW.source_fingerprint AS BLOB)) <= 262144)
    AND (NEW.prodid IS NULL OR (typeof(NEW.prodid) = 'text' AND instr(CAST(NEW.prodid AS BLOB), x'00') = 0 AND length(NEW.prodid) <= 262144 AND length(CAST(NEW.prodid AS BLOB)) <= 262144))
    AND (NEW.version IS NULL OR (typeof(NEW.version) = 'text' AND instr(CAST(NEW.version AS BLOB), x'00') = 0 AND length(NEW.version) <= 262144 AND length(CAST(NEW.version AS BLOB)) <= 262144))
    AND (NEW.method IS NULL OR (typeof(NEW.method) = 'text' AND instr(CAST(NEW.method AS BLOB), x'00') = 0 AND length(NEW.method) <= 262144 AND length(CAST(NEW.method AS BLOB)) <= 262144))
    AND (NEW.calendar_scale IS NULL OR (typeof(NEW.calendar_scale) = 'text' AND instr(CAST(NEW.calendar_scale AS BLOB), x'00') = 0 AND length(NEW.calendar_scale) <= 262144 AND length(CAST(NEW.calendar_scale AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_icalendar_objects
BEFORE UPDATE OF id, import_id, source_kind, source_name, source_fingerprint, prodid, version, method, calendar_scale, created_at ON icalendar_objects
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.import_id IS NOT NEW.import_id COLLATE BINARY OR OLD.source_kind IS NOT NEW.source_kind COLLATE BINARY OR OLD.source_name IS NOT NEW.source_name COLLATE BINARY OR OLD.source_fingerprint IS NOT NEW.source_fingerprint COLLATE BINARY OR OLD.prodid IS NOT NEW.prodid COLLATE BINARY OR OLD.version IS NOT NEW.version COLLATE BINARY OR OLD.method IS NOT NEW.method COLLATE BINARY OR OLD.calendar_scale IS NOT NEW.calendar_scale COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_icalendar_objects
BEFORE INSERT ON icalendar_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 161 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM icalendar_objects WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_icalendar_objects_insert
AFTER INSERT ON icalendar_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (161, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_icalendar_objects_update
AFTER UPDATE ON icalendar_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 161, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.calendar_id IS NOT NEW.calendar_id COLLATE BINARY
                AND NOT (NEW.calendar_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendars WHERE id = OLD.calendar_id))
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_objects_delete
AFTER DELETE ON icalendar_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (161, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_icalendar_object_diagnostics_insert
AFTER INSERT ON icalendar_object_diagnostics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (161, NEW.object_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_object_diagnostics_update
AFTER UPDATE ON icalendar_object_diagnostics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 161, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.object_id AS key UNION SELECT OLD.object_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_object_diagnostics_delete
AFTER DELETE ON icalendar_object_diagnostics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (161, OLD.object_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_icalendar_components_insert
BEFORE INSERT ON icalendar_components
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.object_id IS NULL OR (typeof(NEW.object_id) = 'text' AND instr(CAST(NEW.object_id AS BLOB), x'00') = 0 AND trim(NEW.object_id) <> '' AND length(CAST(NEW.object_id AS BLOB)) <= 128))
    AND (NEW.parent_component_id IS NULL OR (typeof(NEW.parent_component_id) = 'text' AND instr(CAST(NEW.parent_component_id AS BLOB), x'00') = 0 AND trim(NEW.parent_component_id) <> '' AND length(CAST(NEW.parent_component_id AS BLOB)) <= 128))
    AND (typeof(NEW.component_type) = 'text' AND instr(CAST(NEW.component_type AS BLOB), x'00') = 0 AND length(NEW.component_type) <= 262144 AND length(CAST(NEW.component_type AS BLOB)) <= 262144)
    AND (NEW.uid IS NULL OR (typeof(NEW.uid) = 'text' AND instr(CAST(NEW.uid AS BLOB), x'00') = 0 AND length(NEW.uid) <= 262144 AND length(CAST(NEW.uid AS BLOB)) <= 262144))
    AND (NEW.recurrence_id IS NULL OR (typeof(NEW.recurrence_id) = 'text' AND instr(CAST(NEW.recurrence_id AS BLOB), x'00') = 0 AND length(NEW.recurrence_id) <= 262144 AND length(CAST(NEW.recurrence_id AS BLOB)) <= 262144))
    AND (NEW.recurrence_id_value_type IS NULL OR (typeof(NEW.recurrence_id_value_type) = 'text' AND instr(CAST(NEW.recurrence_id_value_type AS BLOB), x'00') = 0 AND length(NEW.recurrence_id_value_type) <= 262144 AND length(CAST(NEW.recurrence_id_value_type AS BLOB)) <= 262144))
    AND (NEW.sequence IS NULL OR (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.dtstart_key IS NULL OR (typeof(NEW.dtstart_key) = 'text' AND instr(CAST(NEW.dtstart_key AS BLOB), x'00') = 0 AND length(NEW.dtstart_key) <= 262144 AND length(CAST(NEW.dtstart_key AS BLOB)) <= 262144))
    AND (typeof(NEW.preservation_status) = 'text' AND instr(CAST(NEW.preservation_status AS BLOB), x'00') = 0 AND length(NEW.preservation_status) <= 32 AND length(CAST(NEW.preservation_status AS BLOB)) <= 262144)
    AND (typeof(NEW.sort_order) = 'integer' AND NEW.sort_order BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_icalendar_components_update
BEFORE UPDATE OF object_id, parent_component_id, component_type, uid, recurrence_id, recurrence_id_value_type, sequence, dtstart_key, preservation_status, sort_order, created_at, calendar_id, updated_at ON icalendar_components
WHEN ((NEW.object_id IS NULL OR (typeof(NEW.object_id) = 'text' AND instr(CAST(NEW.object_id AS BLOB), x'00') = 0 AND trim(NEW.object_id) <> '' AND length(CAST(NEW.object_id AS BLOB)) <= 128))
    AND (NEW.parent_component_id IS NULL OR (typeof(NEW.parent_component_id) = 'text' AND instr(CAST(NEW.parent_component_id AS BLOB), x'00') = 0 AND trim(NEW.parent_component_id) <> '' AND length(CAST(NEW.parent_component_id AS BLOB)) <= 128))
    AND (typeof(NEW.component_type) = 'text' AND instr(CAST(NEW.component_type AS BLOB), x'00') = 0 AND length(NEW.component_type) <= 262144 AND length(CAST(NEW.component_type AS BLOB)) <= 262144)
    AND (NEW.uid IS NULL OR (typeof(NEW.uid) = 'text' AND instr(CAST(NEW.uid AS BLOB), x'00') = 0 AND length(NEW.uid) <= 262144 AND length(CAST(NEW.uid AS BLOB)) <= 262144))
    AND (NEW.recurrence_id IS NULL OR (typeof(NEW.recurrence_id) = 'text' AND instr(CAST(NEW.recurrence_id AS BLOB), x'00') = 0 AND length(NEW.recurrence_id) <= 262144 AND length(CAST(NEW.recurrence_id AS BLOB)) <= 262144))
    AND (NEW.recurrence_id_value_type IS NULL OR (typeof(NEW.recurrence_id_value_type) = 'text' AND instr(CAST(NEW.recurrence_id_value_type AS BLOB), x'00') = 0 AND length(NEW.recurrence_id_value_type) <= 262144 AND length(CAST(NEW.recurrence_id_value_type AS BLOB)) <= 262144))
    AND (NEW.sequence IS NULL OR (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.dtstart_key IS NULL OR (typeof(NEW.dtstart_key) = 'text' AND instr(CAST(NEW.dtstart_key AS BLOB), x'00') = 0 AND length(NEW.dtstart_key) <= 262144 AND length(CAST(NEW.dtstart_key AS BLOB)) <= 262144))
    AND (typeof(NEW.preservation_status) = 'text' AND instr(CAST(NEW.preservation_status AS BLOB), x'00') = 0 AND length(NEW.preservation_status) <= 32 AND length(CAST(NEW.preservation_status AS BLOB)) <= 262144)
    AND (typeof(NEW.sort_order) = 'integer' AND NEW.sort_order BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_icalendar_components
BEFORE UPDATE OF id, object_id, parent_component_id, component_type, uid, recurrence_id, recurrence_id_value_type, sequence, dtstart_key, preservation_status, sort_order, created_at ON icalendar_components
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.object_id IS NOT NEW.object_id COLLATE BINARY OR OLD.parent_component_id IS NOT NEW.parent_component_id COLLATE BINARY OR OLD.component_type IS NOT NEW.component_type COLLATE BINARY OR OLD.uid IS NOT NEW.uid COLLATE BINARY OR OLD.recurrence_id IS NOT NEW.recurrence_id COLLATE BINARY OR OLD.recurrence_id_value_type IS NOT NEW.recurrence_id_value_type COLLATE BINARY OR OLD.sequence IS NOT NEW.sequence OR OLD.dtstart_key IS NOT NEW.dtstart_key COLLATE BINARY OR OLD.preservation_status IS NOT NEW.preservation_status COLLATE BINARY OR OLD.sort_order IS NOT NEW.sort_order OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_icalendar_components
BEFORE INSERT ON icalendar_components
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 162 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM icalendar_components WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_icalendar_components_insert
AFTER INSERT ON icalendar_components
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_icalendar_components_update
AFTER UPDATE ON icalendar_components
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 162, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.calendar_id IS NOT NEW.calendar_id COLLATE BINARY
                AND NOT (NEW.calendar_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendars WHERE id = OLD.calendar_id))
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_components_delete
AFTER DELETE ON icalendar_components
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_icalendar_component_projection_warnings_insert
AFTER INSERT ON icalendar_component_projection_warnings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, NEW.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_component_projection_warnings_update
AFTER UPDATE ON icalendar_component_projection_warnings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 162, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.component_id AS key UNION SELECT OLD.component_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_component_projection_warnings_delete
AFTER DELETE ON icalendar_component_projection_warnings
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, OLD.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_component_properties_insert
AFTER INSERT ON icalendar_component_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, NEW.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_component_properties_update
AFTER UPDATE ON icalendar_component_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 162, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.component_id AS key UNION SELECT OLD.component_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_component_properties_delete
AFTER DELETE ON icalendar_component_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, OLD.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_property_parameters_insert
AFTER INSERT ON icalendar_property_parameters
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, NEW.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_property_parameters_update
AFTER UPDATE ON icalendar_property_parameters
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 162, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.component_id AS key UNION SELECT OLD.component_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_property_parameters_delete
AFTER DELETE ON icalendar_property_parameters
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, OLD.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_value_nodes_insert
AFTER INSERT ON icalendar_value_nodes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, NEW.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_value_nodes_update
AFTER UPDATE ON icalendar_value_nodes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 162, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.component_id AS key UNION SELECT OLD.component_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_icalendar_value_nodes_delete
AFTER DELETE ON icalendar_value_nodes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (162, OLD.component_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_calendar_events_insert
BEFORE INSERT ON calendar_events
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.source_uid IS NULL OR (typeof(NEW.source_uid) = 'text' AND instr(CAST(NEW.source_uid AS BLOB), x'00') = 0 AND length(NEW.source_uid) <= 262144 AND length(CAST(NEW.source_uid AS BLOB)) <= 262144))
    AND (NEW.icalendar_component_id IS NULL OR (typeof(NEW.icalendar_component_id) = 'text' AND instr(CAST(NEW.icalendar_component_id AS BLOB), x'00') = 0 AND trim(NEW.icalendar_component_id) <> '' AND length(CAST(NEW.icalendar_component_id AS BLOB)) <= 128))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(trim(NEW.start_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.start_time AS BLOB)) <= 262144)
    AND (typeof(NEW.end_time) = 'text' AND instr(CAST(NEW.end_time AS BLOB), x'00') = 0 AND length(trim(NEW.end_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.end_time AS BLOB)) <= 262144)
    AND (typeof(NEW.timezone) = 'text' AND instr(CAST(NEW.timezone AS BLOB), x'00') = 0 AND length(trim(NEW.timezone)) BETWEEN 1 AND 262144 AND length(CAST(NEW.timezone AS BLOB)) <= 262144)
    AND (typeof(NEW.all_day) = 'integer' AND NEW.all_day IN (0, 1))
    AND (NEW.rrule IS NULL OR (typeof(NEW.rrule) = 'text' AND instr(CAST(NEW.rrule AS BLOB), x'00') = 0 AND length(NEW.rrule) <= 262144 AND length(CAST(NEW.rrule AS BLOB)) <= 262144))
    AND (NEW.repeat_until IS NULL OR (typeof(NEW.repeat_until) = 'text' AND instr(CAST(NEW.repeat_until AS BLOB), x'00') = 0 AND length(NEW.repeat_until) <= 262144 AND length(CAST(NEW.repeat_until AS BLOB)) <= 262144))
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.location) = 'text' AND instr(CAST(NEW.location AS BLOB), x'00') = 0 AND length(NEW.location) <= 262144 AND length(CAST(NEW.location AS BLOB)) <= 262144)
    AND (NEW.geo_lat IS NULL OR (typeof(NEW.geo_lat) = 'integer' AND NEW.geo_lat BETWEEN -90000000 AND 90000000))
    AND (NEW.geo_lng IS NULL OR (typeof(NEW.geo_lng) = 'integer' AND NEW.geo_lng BETWEEN -180000000 AND 180000000))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.transparency) = 'text' AND instr(CAST(NEW.transparency AS BLOB), x'00') = 0 AND length(NEW.transparency) <= 32 AND length(CAST(NEW.transparency AS BLOB)) <= 262144)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (typeof(NEW.visibility) = 'text' AND instr(CAST(NEW.visibility AS BLOB), x'00') = 0 AND length(NEW.visibility) <= 32 AND length(CAST(NEW.visibility AS BLOB)) <= 262144)
    AND (NEW.priority IS NULL OR (typeof(NEW.priority) = 'integer' AND NEW.priority BETWEEN 0 AND 9))
    AND (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144)
    AND (typeof(NEW.meeting_enabled) = 'integer' AND NEW.meeting_enabled IN (0, 1))
    AND (typeof(NEW.guest_can_modify) = 'integer' AND NEW.guest_can_modify IN (0, 1))
    AND (typeof(NEW.guest_can_invite_others) = 'integer' AND NEW.guest_can_invite_others IN (0, 1))
    AND (typeof(NEW.guest_can_see_other_guests) = 'integer' AND NEW.guest_can_see_other_guests IN (0, 1))
    AND (NEW.local_rsvp_status IS NULL OR (typeof(NEW.local_rsvp_status) = 'text' AND instr(CAST(NEW.local_rsvp_status AS BLOB), x'00') = 0 AND length(NEW.local_rsvp_status) <= 32 AND length(CAST(NEW.local_rsvp_status AS BLOB)) <= 262144))
    AND (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN 0 AND 9223372036854775807)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 262144 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 262144 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_calendar_events_update
BEFORE UPDATE OF created_at, source_uid, icalendar_component_id, title, description, start_time, end_time, timezone, all_day, rrule, repeat_until, calendar_id, project_id, location, geo_lat, geo_lng, color, transparency, status, visibility, priority, url, meeting_enabled, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests, local_rsvp_status, sequence, playlist_id, environment_id, updated_at ON calendar_events
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.source_uid IS NULL OR (typeof(NEW.source_uid) = 'text' AND instr(CAST(NEW.source_uid AS BLOB), x'00') = 0 AND length(NEW.source_uid) <= 262144 AND length(CAST(NEW.source_uid AS BLOB)) <= 262144))
    AND (NEW.icalendar_component_id IS NULL OR (typeof(NEW.icalendar_component_id) = 'text' AND instr(CAST(NEW.icalendar_component_id AS BLOB), x'00') = 0 AND trim(NEW.icalendar_component_id) <> '' AND length(CAST(NEW.icalendar_component_id AS BLOB)) <= 128))
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(trim(NEW.start_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.start_time AS BLOB)) <= 262144)
    AND (typeof(NEW.end_time) = 'text' AND instr(CAST(NEW.end_time AS BLOB), x'00') = 0 AND length(trim(NEW.end_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.end_time AS BLOB)) <= 262144)
    AND (typeof(NEW.timezone) = 'text' AND instr(CAST(NEW.timezone AS BLOB), x'00') = 0 AND length(trim(NEW.timezone)) BETWEEN 1 AND 262144 AND length(CAST(NEW.timezone AS BLOB)) <= 262144)
    AND (typeof(NEW.all_day) = 'integer' AND NEW.all_day IN (0, 1))
    AND (NEW.rrule IS NULL OR (typeof(NEW.rrule) = 'text' AND instr(CAST(NEW.rrule AS BLOB), x'00') = 0 AND length(NEW.rrule) <= 262144 AND length(CAST(NEW.rrule AS BLOB)) <= 262144))
    AND (NEW.repeat_until IS NULL OR (typeof(NEW.repeat_until) = 'text' AND instr(CAST(NEW.repeat_until AS BLOB), x'00') = 0 AND length(NEW.repeat_until) <= 262144 AND length(CAST(NEW.repeat_until AS BLOB)) <= 262144))
    AND (NEW.calendar_id IS NULL OR (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_id) <> '' AND length(CAST(NEW.calendar_id AS BLOB)) <= 128))
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND trim(NEW.project_id) <> '' AND length(CAST(NEW.project_id AS BLOB)) <= 128))
    AND (typeof(NEW.location) = 'text' AND instr(CAST(NEW.location AS BLOB), x'00') = 0 AND length(NEW.location) <= 262144 AND length(CAST(NEW.location AS BLOB)) <= 262144)
    AND (NEW.geo_lat IS NULL OR (typeof(NEW.geo_lat) = 'integer' AND NEW.geo_lat BETWEEN -90000000 AND 90000000))
    AND (NEW.geo_lng IS NULL OR (typeof(NEW.geo_lng) = 'integer' AND NEW.geo_lng BETWEEN -180000000 AND 180000000))
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.transparency) = 'text' AND instr(CAST(NEW.transparency AS BLOB), x'00') = 0 AND length(NEW.transparency) <= 32 AND length(CAST(NEW.transparency AS BLOB)) <= 262144)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (typeof(NEW.visibility) = 'text' AND instr(CAST(NEW.visibility AS BLOB), x'00') = 0 AND length(NEW.visibility) <= 32 AND length(CAST(NEW.visibility AS BLOB)) <= 262144)
    AND (NEW.priority IS NULL OR (typeof(NEW.priority) = 'integer' AND NEW.priority BETWEEN 0 AND 9))
    AND (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144)
    AND (typeof(NEW.meeting_enabled) = 'integer' AND NEW.meeting_enabled IN (0, 1))
    AND (typeof(NEW.guest_can_modify) = 'integer' AND NEW.guest_can_modify IN (0, 1))
    AND (typeof(NEW.guest_can_invite_others) = 'integer' AND NEW.guest_can_invite_others IN (0, 1))
    AND (typeof(NEW.guest_can_see_other_guests) = 'integer' AND NEW.guest_can_see_other_guests IN (0, 1))
    AND (NEW.local_rsvp_status IS NULL OR (typeof(NEW.local_rsvp_status) = 'text' AND instr(CAST(NEW.local_rsvp_status AS BLOB), x'00') = 0 AND length(NEW.local_rsvp_status) <= 32 AND length(CAST(NEW.local_rsvp_status AS BLOB)) <= 262144))
    AND (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN 0 AND 9223372036854775807)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 262144 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 262144 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_calendar_events
BEFORE UPDATE OF id, created_at ON calendar_events
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_calendar_events
BEFORE INSERT ON calendar_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 163 AND row_key = NEW.id AND held_until_ms IS NOT NULL)
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 163 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM calendar_events WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_calendar_events_insert
AFTER INSERT ON calendar_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.id, 134217727, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = CASE WHEN held_until_ms IS NULL THEN 1 ELSE created END, deleted = 0,
        held_until_ms = NULL;
END;

CREATE TRIGGER sync_capture_calendar_events_update
AFTER UPDATE ON calendar_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.source_uid IS NOT NEW.source_uid COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icalendar_component_id IS NOT NEW.icalendar_component_id COLLATE BINARY
                AND NOT (NEW.icalendar_component_id IS NULL AND NOT EXISTS (SELECT 1 FROM icalendar_components WHERE id = OLD.icalendar_component_id))
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.title IS NOT NEW.title COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.description IS NOT NEW.description COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.start_time IS NOT NEW.start_time COLLATE BINARY OR OLD.end_time IS NOT NEW.end_time COLLATE BINARY OR OLD.timezone IS NOT NEW.timezone COLLATE BINARY OR OLD.all_day IS NOT NEW.all_day OR OLD.rrule IS NOT NEW.rrule COLLATE BINARY OR OLD.repeat_until IS NOT NEW.repeat_until COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.calendar_id IS NOT NEW.calendar_id COLLATE BINARY
                AND NOT (NEW.calendar_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendars WHERE id = OLD.calendar_id))
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.project_id IS NOT NEW.project_id COLLATE BINARY
                AND NOT (NEW.project_id IS NULL AND NOT EXISTS (SELECT 1 FROM projects WHERE id = OLD.project_id))
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.location IS NOT NEW.location COLLATE BINARY OR OLD.geo_lat IS NOT NEW.geo_lat OR OLD.geo_lng IS NOT NEW.geo_lng
                THEN 256 ELSE 0 END)
            | (CASE WHEN OLD.color IS NOT NEW.color
                THEN 512 ELSE 0 END)
            | (CASE WHEN OLD.transparency IS NOT NEW.transparency COLLATE BINARY OR OLD.status IS NOT NEW.status COLLATE BINARY
                THEN 1024 ELSE 0 END)
            | (CASE WHEN OLD.visibility IS NOT NEW.visibility COLLATE BINARY
                THEN 2048 ELSE 0 END)
            | (CASE WHEN OLD.priority IS NOT NEW.priority
                THEN 4096 ELSE 0 END)
            | (CASE WHEN OLD.url IS NOT NEW.url COLLATE BINARY OR OLD.meeting_enabled IS NOT NEW.meeting_enabled
                THEN 8192 ELSE 0 END)
            | (CASE WHEN OLD.guest_can_modify IS NOT NEW.guest_can_modify OR OLD.guest_can_invite_others IS NOT NEW.guest_can_invite_others OR OLD.guest_can_see_other_guests IS NOT NEW.guest_can_see_other_guests
                THEN 16384 ELSE 0 END)
            | (CASE WHEN OLD.local_rsvp_status IS NOT NEW.local_rsvp_status COLLATE BINARY
                THEN 32768 ELSE 0 END)
            | (CASE WHEN OLD.sequence IS NOT NEW.sequence
                THEN 65536 ELSE 0 END)
            | (CASE WHEN OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY
                THEN 131072 ELSE 0 END)
            | (CASE WHEN OLD.environment_id IS NOT NEW.environment_id COLLATE BINARY
                THEN 262144 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 67108864 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_events_delete
AFTER DELETE ON calendar_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_calendar_event_exdates_insert
AFTER INSERT ON calendar_event_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_exdates_update
AFTER UPDATE ON calendar_event_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_exdates_delete
AFTER DELETE ON calendar_event_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_rdates_insert
AFTER INSERT ON calendar_event_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_rdates_update
AFTER UPDATE ON calendar_event_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_rdates_delete
AFTER DELETE ON calendar_event_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_overrides_insert
AFTER INSERT ON calendar_event_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.parent_event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_overrides_update
AFTER UPDATE ON calendar_event_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.parent_event_id AS key UNION SELECT OLD.parent_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_overrides_delete
AFTER DELETE ON calendar_event_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.parent_event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_override_extended_properties_insert
AFTER INSERT ON calendar_event_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_override_extended_properties_update
AFTER UPDATE ON calendar_event_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_override_extended_properties_delete
AFTER DELETE ON calendar_event_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_alarms_insert
AFTER INSERT ON calendar_event_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 524288, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_alarms_update
AFTER UPDATE ON calendar_event_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 524288, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_alarms_delete
AFTER DELETE ON calendar_event_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 524288, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_attendees_insert
AFTER INSERT ON calendar_event_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 1048576, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_attendees_update
AFTER UPDATE ON calendar_event_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 1048576, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_attendees_delete
AFTER DELETE ON calendar_event_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 1048576, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_organizers_insert
AFTER INSERT ON calendar_event_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 2097152, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_organizers_update
AFTER UPDATE ON calendar_event_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 2097152, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_organizers_delete
AFTER DELETE ON calendar_event_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 2097152, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_categories_insert
AFTER INSERT ON calendar_event_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 4194304, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_categories_update
AFTER UPDATE ON calendar_event_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 4194304, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_categories_delete
AFTER DELETE ON calendar_event_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 4194304, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_notifications_insert
AFTER INSERT ON calendar_event_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 8388608, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_notifications_update
AFTER UPDATE ON calendar_event_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 8388608, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_notifications_delete
AFTER DELETE ON calendar_event_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 8388608, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_extended_properties_insert
AFTER INSERT ON calendar_event_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 16777216, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_extended_properties_update
AFTER UPDATE ON calendar_event_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 16777216, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_extended_properties_delete
AFTER DELETE ON calendar_event_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 16777216, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_configs_insert
AFTER INSERT ON calendar_event_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_configs_update
AFTER UPDATE ON calendar_event_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_configs_delete
AFTER DELETE ON calendar_event_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_count_rhythms_insert
AFTER INSERT ON calendar_event_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_count_rhythms_update
AFTER UPDATE ON calendar_event_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_count_rhythms_delete
AFTER DELETE ON calendar_event_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_sequence_steps_insert
AFTER INSERT ON calendar_event_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, NEW.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_sequence_steps_update
AFTER UPDATE ON calendar_event_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 163, parent.key, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.event_id AS key UNION SELECT OLD.event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_pomodoro_config_sequence_steps_delete
AFTER DELETE ON calendar_event_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (163, OLD.event_id, 33554432, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_calendar_event_archives_insert
BEFORE INSERT ON calendar_event_archives
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.source_event_id) = 'text' AND instr(CAST(NEW.source_event_id AS BLOB), x'00') = 0 AND length(trim(NEW.source_event_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.source_event_id AS BLOB)) <= 262144)
    AND (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64)
    AND (NEW.original_occurrence_id IS NULL OR (typeof(NEW.original_occurrence_id) = 'text' AND instr(CAST(NEW.original_occurrence_id AS BLOB), x'00') = 0 AND length(NEW.original_occurrence_id) <= 262144 AND length(CAST(NEW.original_occurrence_id AS BLOB)) <= 262144))
    AND (NEW.recurrence_date IS NULL OR (typeof(NEW.recurrence_date) = 'text' AND instr(CAST(NEW.recurrence_date AS BLOB), x'00') = 0 AND length(NEW.recurrence_date) <= 262144 AND length(CAST(NEW.recurrence_date AS BLOB)) <= 262144))
    AND (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND length(trim(NEW.calendar_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.calendar_id AS BLOB)) <= 262144)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND length(NEW.project_id) <= 262144 AND length(CAST(NEW.project_id AS BLOB)) <= 262144))
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 262144 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 262144 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (NEW.source_uid IS NULL OR (typeof(NEW.source_uid) = 'text' AND instr(CAST(NEW.source_uid AS BLOB), x'00') = 0 AND length(NEW.source_uid) <= 262144 AND length(CAST(NEW.source_uid AS BLOB)) <= 262144))
    AND (NEW.icalendar_component_id IS NULL OR (typeof(NEW.icalendar_component_id) = 'text' AND instr(CAST(NEW.icalendar_component_id AS BLOB), x'00') = 0 AND length(NEW.icalendar_component_id) <= 262144 AND length(CAST(NEW.icalendar_component_id AS BLOB)) <= 262144))
    AND (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(trim(NEW.start_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.start_time AS BLOB)) <= 262144)
    AND (typeof(NEW.end_time) = 'text' AND instr(CAST(NEW.end_time AS BLOB), x'00') = 0 AND length(trim(NEW.end_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.end_time AS BLOB)) <= 262144)
    AND (typeof(NEW.timezone) = 'text' AND instr(CAST(NEW.timezone AS BLOB), x'00') = 0 AND length(trim(NEW.timezone)) BETWEEN 1 AND 262144 AND length(CAST(NEW.timezone AS BLOB)) <= 262144)
    AND (typeof(NEW.all_day) = 'integer' AND NEW.all_day IN (0, 1))
    AND (NEW.rrule IS NULL OR (typeof(NEW.rrule) = 'text' AND instr(CAST(NEW.rrule AS BLOB), x'00') = 0 AND length(NEW.rrule) <= 262144 AND length(CAST(NEW.rrule AS BLOB)) <= 262144))
    AND (NEW.repeat_until IS NULL OR (typeof(NEW.repeat_until) = 'text' AND instr(CAST(NEW.repeat_until AS BLOB), x'00') = 0 AND length(NEW.repeat_until) <= 262144 AND length(CAST(NEW.repeat_until AS BLOB)) <= 262144))
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.location) = 'text' AND instr(CAST(NEW.location AS BLOB), x'00') = 0 AND length(NEW.location) <= 262144 AND length(CAST(NEW.location AS BLOB)) <= 262144)
    AND (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144)
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.transparency) = 'text' AND instr(CAST(NEW.transparency AS BLOB), x'00') = 0 AND length(NEW.transparency) <= 32 AND length(CAST(NEW.transparency AS BLOB)) <= 262144)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (typeof(NEW.visibility) = 'text' AND instr(CAST(NEW.visibility AS BLOB), x'00') = 0 AND length(NEW.visibility) <= 32 AND length(CAST(NEW.visibility AS BLOB)) <= 262144)
    AND (NEW.priority IS NULL OR (typeof(NEW.priority) = 'integer' AND NEW.priority BETWEEN 0 AND 9))
    AND (NEW.geo_lat IS NULL OR (typeof(NEW.geo_lat) = 'integer' AND NEW.geo_lat BETWEEN -90000000 AND 90000000))
    AND (NEW.geo_lng IS NULL OR (typeof(NEW.geo_lng) = 'integer' AND NEW.geo_lng BETWEEN -180000000 AND 180000000))
    AND (NEW.local_rsvp_status IS NULL OR (typeof(NEW.local_rsvp_status) = 'text' AND instr(CAST(NEW.local_rsvp_status AS BLOB), x'00') = 0 AND length(NEW.local_rsvp_status) <= 32 AND length(CAST(NEW.local_rsvp_status AS BLOB)) <= 262144))
    AND (typeof(NEW.meeting_enabled) = 'integer' AND NEW.meeting_enabled IN (0, 1))
    AND (typeof(NEW.guest_can_modify) = 'integer' AND NEW.guest_can_modify IN (0, 1))
    AND (typeof(NEW.guest_can_invite_others) = 'integer' AND NEW.guest_can_invite_others IN (0, 1))
    AND (typeof(NEW.guest_can_see_other_guests) = 'integer' AND NEW.guest_can_see_other_guests IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_calendar_event_archives_update
BEFORE UPDATE OF source_event_id, archived_at, original_occurrence_id, recurrence_date, calendar_id, project_id, playlist_id, environment_id, source_uid, icalendar_component_id, sequence, created_at, updated_at, title, start_time, end_time, timezone, all_day, rrule, repeat_until, description, location, url, color, transparency, status, visibility, priority, geo_lat, geo_lng, local_rsvp_status, meeting_enabled, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests ON calendar_event_archives
WHEN ((typeof(NEW.source_event_id) = 'text' AND instr(CAST(NEW.source_event_id AS BLOB), x'00') = 0 AND length(trim(NEW.source_event_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.source_event_id AS BLOB)) <= 262144)
    AND (typeof(NEW.archived_at) = 'text' AND instr(CAST(NEW.archived_at AS BLOB), x'00') = 0 AND trim(NEW.archived_at) <> '' AND length(CAST(NEW.archived_at AS BLOB)) <= 64)
    AND (NEW.original_occurrence_id IS NULL OR (typeof(NEW.original_occurrence_id) = 'text' AND instr(CAST(NEW.original_occurrence_id AS BLOB), x'00') = 0 AND length(NEW.original_occurrence_id) <= 262144 AND length(CAST(NEW.original_occurrence_id AS BLOB)) <= 262144))
    AND (NEW.recurrence_date IS NULL OR (typeof(NEW.recurrence_date) = 'text' AND instr(CAST(NEW.recurrence_date AS BLOB), x'00') = 0 AND length(NEW.recurrence_date) <= 262144 AND length(CAST(NEW.recurrence_date AS BLOB)) <= 262144))
    AND (typeof(NEW.calendar_id) = 'text' AND instr(CAST(NEW.calendar_id AS BLOB), x'00') = 0 AND length(trim(NEW.calendar_id)) BETWEEN 1 AND 262144 AND length(CAST(NEW.calendar_id AS BLOB)) <= 262144)
    AND (NEW.project_id IS NULL OR (typeof(NEW.project_id) = 'text' AND instr(CAST(NEW.project_id AS BLOB), x'00') = 0 AND length(NEW.project_id) <= 262144 AND length(CAST(NEW.project_id AS BLOB)) <= 262144))
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 262144 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 262144 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (NEW.source_uid IS NULL OR (typeof(NEW.source_uid) = 'text' AND instr(CAST(NEW.source_uid AS BLOB), x'00') = 0 AND length(NEW.source_uid) <= 262144 AND length(CAST(NEW.source_uid AS BLOB)) <= 262144))
    AND (NEW.icalendar_component_id IS NULL OR (typeof(NEW.icalendar_component_id) = 'text' AND instr(CAST(NEW.icalendar_component_id AS BLOB), x'00') = 0 AND length(NEW.icalendar_component_id) <= 262144 AND length(CAST(NEW.icalendar_component_id AS BLOB)) <= 262144))
    AND (typeof(NEW.sequence) = 'integer' AND NEW.sequence BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)
    AND (typeof(NEW.title) = 'text' AND instr(CAST(NEW.title AS BLOB), x'00') = 0 AND length(NEW.title) <= 262144 AND length(CAST(NEW.title AS BLOB)) <= 262144)
    AND (typeof(NEW.start_time) = 'text' AND instr(CAST(NEW.start_time AS BLOB), x'00') = 0 AND length(trim(NEW.start_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.start_time AS BLOB)) <= 262144)
    AND (typeof(NEW.end_time) = 'text' AND instr(CAST(NEW.end_time AS BLOB), x'00') = 0 AND length(trim(NEW.end_time)) BETWEEN 1 AND 262144 AND length(CAST(NEW.end_time AS BLOB)) <= 262144)
    AND (typeof(NEW.timezone) = 'text' AND instr(CAST(NEW.timezone AS BLOB), x'00') = 0 AND length(trim(NEW.timezone)) BETWEEN 1 AND 262144 AND length(CAST(NEW.timezone AS BLOB)) <= 262144)
    AND (typeof(NEW.all_day) = 'integer' AND NEW.all_day IN (0, 1))
    AND (NEW.rrule IS NULL OR (typeof(NEW.rrule) = 'text' AND instr(CAST(NEW.rrule AS BLOB), x'00') = 0 AND length(NEW.rrule) <= 262144 AND length(CAST(NEW.rrule AS BLOB)) <= 262144))
    AND (NEW.repeat_until IS NULL OR (typeof(NEW.repeat_until) = 'text' AND instr(CAST(NEW.repeat_until AS BLOB), x'00') = 0 AND length(NEW.repeat_until) <= 262144 AND length(CAST(NEW.repeat_until AS BLOB)) <= 262144))
    AND (typeof(NEW.description) = 'text' AND instr(CAST(NEW.description AS BLOB), x'00') = 0 AND length(NEW.description) <= 262144 AND length(CAST(NEW.description AS BLOB)) <= 262144)
    AND (typeof(NEW.location) = 'text' AND instr(CAST(NEW.location AS BLOB), x'00') = 0 AND length(NEW.location) <= 262144 AND length(CAST(NEW.location AS BLOB)) <= 262144)
    AND (typeof(NEW.url) = 'text' AND instr(CAST(NEW.url AS BLOB), x'00') = 0 AND length(NEW.url) <= 262144 AND length(CAST(NEW.url AS BLOB)) <= 262144)
    AND (NEW.color IS NULL OR (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31))
    AND (typeof(NEW.transparency) = 'text' AND instr(CAST(NEW.transparency AS BLOB), x'00') = 0 AND length(NEW.transparency) <= 32 AND length(CAST(NEW.transparency AS BLOB)) <= 262144)
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (typeof(NEW.visibility) = 'text' AND instr(CAST(NEW.visibility AS BLOB), x'00') = 0 AND length(NEW.visibility) <= 32 AND length(CAST(NEW.visibility AS BLOB)) <= 262144)
    AND (NEW.priority IS NULL OR (typeof(NEW.priority) = 'integer' AND NEW.priority BETWEEN 0 AND 9))
    AND (NEW.geo_lat IS NULL OR (typeof(NEW.geo_lat) = 'integer' AND NEW.geo_lat BETWEEN -90000000 AND 90000000))
    AND (NEW.geo_lng IS NULL OR (typeof(NEW.geo_lng) = 'integer' AND NEW.geo_lng BETWEEN -180000000 AND 180000000))
    AND (NEW.local_rsvp_status IS NULL OR (typeof(NEW.local_rsvp_status) = 'text' AND instr(CAST(NEW.local_rsvp_status AS BLOB), x'00') = 0 AND length(NEW.local_rsvp_status) <= 32 AND length(CAST(NEW.local_rsvp_status AS BLOB)) <= 262144))
    AND (typeof(NEW.meeting_enabled) = 'integer' AND NEW.meeting_enabled IN (0, 1))
    AND (typeof(NEW.guest_can_modify) = 'integer' AND NEW.guest_can_modify IN (0, 1))
    AND (typeof(NEW.guest_can_invite_others) = 'integer' AND NEW.guest_can_invite_others IN (0, 1))
    AND (typeof(NEW.guest_can_see_other_guests) = 'integer' AND NEW.guest_can_see_other_guests IN (0, 1))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_calendar_event_archives
BEFORE UPDATE OF id, source_event_id, archived_at, original_occurrence_id, recurrence_date, calendar_id, project_id, playlist_id, environment_id, source_uid, icalendar_component_id, sequence, created_at, updated_at, title, start_time, end_time, timezone, all_day, rrule, repeat_until, description, location, url, color, transparency, status, visibility, priority, geo_lat, geo_lng, local_rsvp_status, meeting_enabled, guest_can_modify, guest_can_invite_others, guest_can_see_other_guests ON calendar_event_archives
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.source_event_id IS NOT NEW.source_event_id COLLATE BINARY OR OLD.archived_at IS NOT NEW.archived_at COLLATE BINARY OR OLD.original_occurrence_id IS NOT NEW.original_occurrence_id COLLATE BINARY OR OLD.recurrence_date IS NOT NEW.recurrence_date COLLATE BINARY OR OLD.calendar_id IS NOT NEW.calendar_id COLLATE BINARY OR OLD.project_id IS NOT NEW.project_id COLLATE BINARY OR OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY OR OLD.environment_id IS NOT NEW.environment_id COLLATE BINARY OR OLD.source_uid IS NOT NEW.source_uid COLLATE BINARY OR OLD.icalendar_component_id IS NOT NEW.icalendar_component_id COLLATE BINARY OR OLD.sequence IS NOT NEW.sequence OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY OR OLD.title IS NOT NEW.title COLLATE BINARY OR OLD.start_time IS NOT NEW.start_time COLLATE BINARY OR OLD.end_time IS NOT NEW.end_time COLLATE BINARY OR OLD.timezone IS NOT NEW.timezone COLLATE BINARY OR OLD.all_day IS NOT NEW.all_day OR OLD.rrule IS NOT NEW.rrule COLLATE BINARY OR OLD.repeat_until IS NOT NEW.repeat_until COLLATE BINARY OR OLD.description IS NOT NEW.description COLLATE BINARY OR OLD.location IS NOT NEW.location COLLATE BINARY OR OLD.url IS NOT NEW.url COLLATE BINARY OR OLD.color IS NOT NEW.color OR OLD.transparency IS NOT NEW.transparency COLLATE BINARY OR OLD.status IS NOT NEW.status COLLATE BINARY OR OLD.visibility IS NOT NEW.visibility COLLATE BINARY OR OLD.priority IS NOT NEW.priority OR OLD.geo_lat IS NOT NEW.geo_lat OR OLD.geo_lng IS NOT NEW.geo_lng OR OLD.local_rsvp_status IS NOT NEW.local_rsvp_status COLLATE BINARY OR OLD.meeting_enabled IS NOT NEW.meeting_enabled OR OLD.guest_can_modify IS NOT NEW.guest_can_modify OR OLD.guest_can_invite_others IS NOT NEW.guest_can_invite_others OR OLD.guest_can_see_other_guests IS NOT NEW.guest_can_see_other_guests) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_calendar_event_archives
BEFORE INSERT ON calendar_event_archives
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 164 AND row_key = NEW.id AND held_until_ms IS NOT NULL)
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 164 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM calendar_event_archives WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_calendar_event_archives_insert
AFTER INSERT ON calendar_event_archives
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.id, 65535, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = CASE WHEN held_until_ms IS NULL THEN 1 ELSE created END, deleted = 0,
        held_until_ms = NULL;
END;

CREATE TRIGGER sync_capture_calendar_event_archives_update
AFTER UPDATE ON calendar_event_archives
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archives_delete
AFTER DELETE ON calendar_event_archives
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_alarms_insert
AFTER INSERT ON calendar_event_archive_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_alarms_update
AFTER UPDATE ON calendar_event_archive_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_alarms_delete
AFTER DELETE ON calendar_event_archive_alarms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_attendees_insert
AFTER INSERT ON calendar_event_archive_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_attendees_update
AFTER UPDATE ON calendar_event_archive_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_attendees_delete
AFTER DELETE ON calendar_event_archive_attendees
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 32, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_categories_insert
AFTER INSERT ON calendar_event_archive_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 64, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_categories_update
AFTER UPDATE ON calendar_event_archive_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 64, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_categories_delete
AFTER DELETE ON calendar_event_archive_categories
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 64, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_exdates_insert
AFTER INSERT ON calendar_event_archive_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 128, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_exdates_update
AFTER UPDATE ON calendar_event_archive_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 128, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_exdates_delete
AFTER DELETE ON calendar_event_archive_exdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 128, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_extended_properties_insert
AFTER INSERT ON calendar_event_archive_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_extended_properties_update
AFTER UPDATE ON calendar_event_archive_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_extended_properties_delete
AFTER DELETE ON calendar_event_archive_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_music_assignments_insert
AFTER INSERT ON calendar_event_archive_music_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 512, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_music_assignments_update
AFTER UPDATE ON calendar_event_archive_music_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 512, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_music_assignments_delete
AFTER DELETE ON calendar_event_archive_music_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 512, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_notifications_insert
AFTER INSERT ON calendar_event_archive_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 1024, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_notifications_update
AFTER UPDATE ON calendar_event_archive_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 1024, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_notifications_delete
AFTER DELETE ON calendar_event_archive_notifications
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 1024, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_organizers_insert
AFTER INSERT ON calendar_event_archive_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 2048, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_organizers_update
AFTER UPDATE ON calendar_event_archive_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 2048, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_organizers_delete
AFTER DELETE ON calendar_event_archive_organizers
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 2048, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_overrides_insert
AFTER INSERT ON calendar_event_archive_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_overrides_update
AFTER UPDATE ON calendar_event_archive_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_overrides_delete
AFTER DELETE ON calendar_event_archive_overrides
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_override_extended_properties_insert
AFTER INSERT ON calendar_event_archive_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_override_extended_properties_update
AFTER UPDATE ON calendar_event_archive_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_override_extended_properties_delete
AFTER DELETE ON calendar_event_archive_override_extended_properties
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 4096, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_configs_insert
AFTER INSERT ON calendar_event_archive_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_configs_update
AFTER UPDATE ON calendar_event_archive_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_configs_delete
AFTER DELETE ON calendar_event_archive_pomodoro_configs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_count_rhythms_insert
AFTER INSERT ON calendar_event_archive_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_count_rhythms_update
AFTER UPDATE ON calendar_event_archive_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_count_rhythms_delete
AFTER DELETE ON calendar_event_archive_pomodoro_config_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_sequence_steps_insert
AFTER INSERT ON calendar_event_archive_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_sequence_steps_update
AFTER UPDATE ON calendar_event_archive_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_pomodoro_config_sequence_steps_delete
AFTER DELETE ON calendar_event_archive_pomodoro_config_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 8192, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_rdates_insert
AFTER INSERT ON calendar_event_archive_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 16384, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_rdates_update
AFTER UPDATE ON calendar_event_archive_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 16384, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_rdates_delete
AFTER DELETE ON calendar_event_archive_rdates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 16384, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_task_links_insert
AFTER INSERT ON calendar_event_archive_task_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, NEW.archive_event_id, 32768, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_task_links_update
AFTER UPDATE ON calendar_event_archive_task_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 164, parent.key, 32768, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.archive_event_id AS key UNION SELECT OLD.archive_event_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_task_links_delete
AFTER DELETE ON calendar_event_archive_task_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (164, OLD.archive_event_id, 32768, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_calendar_event_archive_import_objects_insert
BEFORE INSERT ON calendar_event_archive_import_objects
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.archive_event_id IS NULL OR (typeof(NEW.archive_event_id) = 'text' AND instr(CAST(NEW.archive_event_id AS BLOB), x'00') = 0 AND trim(NEW.archive_event_id) <> '' AND length(CAST(NEW.archive_event_id AS BLOB)) <= 128))
    AND (NEW.object_id IS NULL OR (typeof(NEW.object_id) = 'text' AND instr(CAST(NEW.object_id AS BLOB), x'00') = 0 AND trim(NEW.object_id) <> '' AND length(CAST(NEW.object_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_calendar_event_archive_import_objects_update
BEFORE UPDATE OF archive_event_id, object_id ON calendar_event_archive_import_objects
WHEN ((NEW.archive_event_id IS NULL OR (typeof(NEW.archive_event_id) = 'text' AND instr(CAST(NEW.archive_event_id AS BLOB), x'00') = 0 AND trim(NEW.archive_event_id) <> '' AND length(CAST(NEW.archive_event_id AS BLOB)) <= 128))
    AND (NEW.object_id IS NULL OR (typeof(NEW.object_id) = 'text' AND instr(CAST(NEW.object_id AS BLOB), x'00') = 0 AND trim(NEW.object_id) <> '' AND length(CAST(NEW.object_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_calendar_event_archive_import_objects
BEFORE UPDATE OF sync_key, archive_event_id, object_id ON calendar_event_archive_import_objects
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.archive_event_id IS NOT NEW.archive_event_id COLLATE BINARY OR OLD.object_id IS NOT NEW.object_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_calendar_event_archive_import_objects_insert
AFTER INSERT ON calendar_event_archive_import_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (165, NEW.sync_key, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = CASE WHEN held_until_ms IS NULL THEN 1 ELSE created END, deleted = 0,
        held_until_ms = NULL;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_import_objects_update
AFTER UPDATE ON calendar_event_archive_import_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 165, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 165, NEW.sync_key, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_calendar_event_archive_import_objects_delete
AFTER DELETE ON calendar_event_archive_import_objects
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (165, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_project_task_event_links_insert
BEFORE INSERT ON project_task_event_links
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.link_kind) = 'text' AND instr(CAST(NEW.link_kind AS BLOB), x'00') = 0 AND length(NEW.link_kind) <= 32 AND length(CAST(NEW.link_kind AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_project_task_event_links_update
BEFORE UPDATE OF task_id, event_id, created_at, link_kind ON project_task_event_links
WHEN ((NEW.task_id IS NULL OR (typeof(NEW.task_id) = 'text' AND instr(CAST(NEW.task_id AS BLOB), x'00') = 0 AND trim(NEW.task_id) <> '' AND length(CAST(NEW.task_id AS BLOB)) <= 128))
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.link_kind) = 'text' AND instr(CAST(NEW.link_kind AS BLOB), x'00') = 0 AND length(NEW.link_kind) <= 32 AND length(CAST(NEW.link_kind AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_project_task_event_links
BEFORE UPDATE OF sync_key, task_id, event_id ON project_task_event_links
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.task_id IS NOT NEW.task_id COLLATE BINARY OR OLD.event_id IS NOT NEW.event_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_project_task_event_links_insert
AFTER INSERT ON project_task_event_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (166, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = CASE WHEN held_until_ms IS NULL THEN 1 ELSE created END, deleted = 0,
        held_until_ms = NULL;
END;

CREATE TRIGGER sync_capture_project_task_event_links_update
AFTER UPDATE ON project_task_event_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 166, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.link_kind IS NOT NEW.link_kind COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 166, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_project_task_event_links_delete
AFTER DELETE ON project_task_event_links
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (166, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

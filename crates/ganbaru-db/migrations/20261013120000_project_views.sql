-- Projects views: each project lists its own ordered views, and view preferences belong to one view row.
-- The view type is validated by the Projects domain so new types need no table rebuild. A NULL name shows the
-- localized label of the type. Notes and Chat open the project's pages and channels, so a project has at most one of
-- each.

CREATE TABLE project_views (
    id TEXT PRIMARY KEY NOT NULL CHECK (trim(id) <> ''),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    view_type TEXT NOT NULL CHECK (trim(view_type) <> ''),
    name TEXT CHECK (name IS NULL OR trim(name) <> ''),
    sort_order INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(created_at) <> ''),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> '')
);
CREATE INDEX idx_project_views_project_sort ON project_views(project_id, sort_order, id);
CREATE UNIQUE INDEX ux_project_views_single_hosted ON project_views(project_id, view_type)
    WHERE view_type IN ('notes', 'chat');

INSERT INTO project_views (id, project_id, view_type, sort_order)
SELECT 'view-' || projects.id || '-' || defaults.view_type, projects.id, defaults.view_type, defaults.sort_order
FROM projects
CROSS JOIN (
    SELECT 'dashboard' AS view_type, 1000 AS sort_order
    UNION ALL SELECT 'list', 2000
    UNION ALL SELECT 'notes', 3000
    UNION ALL SELECT 'chat', 4000
) AS defaults;

CREATE TABLE project_view_preferences_rebuild (
    view_id TEXT NOT NULL REFERENCES project_views(id) ON DELETE CASCADE,
    preference_key TEXT NOT NULL CHECK (trim(preference_key) <> ''),
    preference_value TEXT NOT NULL DEFAULT '',
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (view_id, preference_key)
);

INSERT INTO project_view_preferences_rebuild (view_id, preference_key, preference_value, updated_at)
SELECT project_views.id, preferences.preference_key, preferences.preference_value, preferences.updated_at
FROM project_view_preferences AS preferences
JOIN project_views
    ON project_views.project_id = preferences.project_id
    AND project_views.view_type = preferences.view_id;

DROP TABLE project_view_preferences;
ALTER TABLE project_view_preferences_rebuild RENAME TO project_view_preferences;

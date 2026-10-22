-- Projects tables take the shape concurrent sync needs. Ordered rows use fractional order keys instead of integer and
-- REAL sort orders, statuses and priorities are deleted softly, composite-key rows carry a sync_key, dependency ids
-- become derived keys, custom field numbers are integer millionths, select values own their option list, revisions
-- are bumped by commands, and icon references carry the exact file size. Tables with cascade children are converted
-- in place. Placeholder keys 'pending:' || rowid are rewritten by the Projects conversion pass at vault open.

-- Existing rows get rank keys: 'c' and three base 62 digits, which covers 238,328 rows per ordered group.
CREATE TEMP TABLE projects_rank_capacity (
    row_count INTEGER NOT NULL CHECK (row_count <= 238328)
);
INSERT INTO projects_rank_capacity (row_count)
SELECT count(*) FROM project_groups
UNION ALL
SELECT count(*) FROM projects GROUP BY group_id
UNION ALL
SELECT count(*) FROM project_sections GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_statuses GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_priorities GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_tags GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_tasks GROUP BY section_id
UNION ALL
SELECT count(*) FROM project_tasks GROUP BY status_id
UNION ALL
SELECT count(*) FROM project_checklist_items GROUP BY task_id
UNION ALL
SELECT count(*) FROM project_custom_fields GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_custom_field_options GROUP BY field_id
UNION ALL
SELECT count(*) FROM project_views GROUP BY project_id
UNION ALL
SELECT count(*) FROM project_custom_emojis;
DROP TABLE projects_rank_capacity;

-- Commands bump revisions explicitly, and order key writes never bump them.
DROP TRIGGER project_task_revision_after_update;
DROP TRIGGER project_custom_field_revision;
DROP TRIGGER project_custom_field_option_revision;

-- Duplicate names and a second notes or chat view can arrive from another device; commands refuse them locally.
DROP INDEX idx_project_tags_project_name;
DROP INDEX idx_project_custom_fields_project_name;
DROP INDEX idx_project_custom_field_options_field_name;
DROP INDEX ux_project_views_single_hosted;

-- Idempotency receipts hold responses in the previous shape.
DELETE FROM project_dependency_cascade_receipts;
DELETE FROM project_reorder_receipts;
DELETE FROM project_task_bulk_receipts;

-- Groups order globally by their previous sort order, name, and id.
DROP INDEX idx_project_groups_sort;
ALTER TABLE project_groups ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_groups
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_groups
    )
) AS ranked
WHERE project_groups.id = ranked.id;
ALTER TABLE project_groups DROP COLUMN sort_order;
CREATE INDEX idx_project_groups_order ON project_groups(order_key, id);

-- Projects order within their group.
DROP INDEX idx_projects_group_sort;
ALTER TABLE projects ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE projects
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY group_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM projects
    )
) AS ranked
WHERE projects.id = ranked.id;
ALTER TABLE projects DROP COLUMN sort_order;
CREATE INDEX idx_projects_group_order ON projects(group_id, order_key, id);

-- Sections order within their project.
DROP INDEX idx_project_sections_project_sort;
ALTER TABLE project_sections ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_sections
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_sections
    )
) AS ranked
WHERE project_sections.id = ranked.id;
ALTER TABLE project_sections DROP COLUMN sort_order;
CREATE INDEX idx_project_sections_project_order ON project_sections(project_id, order_key, id);

-- Statuses order within their project and are deleted softly, since tasks may still name them.
DROP INDEX idx_project_statuses_project_sort;
ALTER TABLE project_statuses ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_statuses
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_statuses
    )
) AS ranked
WHERE project_statuses.id = ranked.id;
ALTER TABLE project_statuses DROP COLUMN sort_order;
ALTER TABLE project_statuses ADD COLUMN deleted_at TEXT CHECK (deleted_at IS NULL OR trim(deleted_at) <> '');
CREATE INDEX idx_project_statuses_project_order ON project_statuses(project_id, order_key, id);

-- Priorities order within their project, are deleted softly, and carry a sync_key over their composite key.
DROP INDEX idx_project_priorities_project_sort;
ALTER TABLE project_priorities ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_priorities
SET order_key = ranked.order_key
FROM (
    SELECT
        project_id,
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT project_id, id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_priorities
    )
) AS ranked
WHERE project_priorities.project_id = ranked.project_id AND project_priorities.id = ranked.id;
ALTER TABLE project_priorities DROP COLUMN sort_order;
ALTER TABLE project_priorities ADD COLUMN deleted_at TEXT CHECK (deleted_at IS NULL OR trim(deleted_at) <> '');
ALTER TABLE project_priorities ADD COLUMN sync_key TEXT NOT NULL DEFAULT '' CHECK (length(sync_key) <= 128);
UPDATE project_priorities SET sync_key = 'pending:' || rowid;
CREATE UNIQUE INDEX idx_project_priorities_sync_key ON project_priorities(sync_key);
CREATE INDEX idx_project_priorities_project_order ON project_priorities(project_id, order_key, id);

-- Tags order within their project.
DROP INDEX idx_project_tags_project_sort;
ALTER TABLE project_tags ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_tags
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_tags
    )
) AS ranked
WHERE project_tags.id = ranked.id;
ALTER TABLE project_tags DROP COLUMN sort_order;
CREATE INDEX idx_project_tags_project_order ON project_tags(project_id, order_key, id);

-- Tasks order within their section and within their status, ranked by their previous order, creation, and id.
DROP INDEX idx_project_tasks_parent;
DROP INDEX idx_project_tasks_project_section;
DROP INDEX idx_project_tasks_project_status;
ALTER TABLE project_tasks ADD COLUMN section_order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(section_order_key) <= 128);
ALTER TABLE project_tasks ADD COLUMN status_order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(status_order_key) <= 128);
UPDATE project_tasks
SET section_order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT
            id,
            ROW_NUMBER() OVER (PARTITION BY section_id ORDER BY section_sort_order, created_at, id) - 1 AS rank
        FROM project_tasks
    )
) AS ranked
WHERE project_tasks.id = ranked.id;
UPDATE project_tasks
SET status_order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT
            id,
            ROW_NUMBER() OVER (PARTITION BY status_id ORDER BY status_sort_order, created_at, id) - 1 AS rank
        FROM project_tasks
    )
) AS ranked
WHERE project_tasks.id = ranked.id;
ALTER TABLE project_tasks DROP COLUMN section_sort_order;
ALTER TABLE project_tasks DROP COLUMN status_sort_order;
CREATE INDEX idx_project_tasks_parent ON project_tasks(parent_task_id, section_order_key, id);
CREATE INDEX idx_project_tasks_project_section
    ON project_tasks(project_id, section_id, section_order_key, id);
CREATE INDEX idx_project_tasks_project_status ON project_tasks(project_id, status_id, status_order_key, id);

-- Checklist items order within their task.
DROP INDEX idx_project_checklist_items_task;
ALTER TABLE project_checklist_items ADD COLUMN order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(order_key) <= 128);
UPDATE project_checklist_items
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY task_id ORDER BY sort_order, created_at, id) - 1 AS rank
        FROM project_checklist_items
    )
) AS ranked
WHERE project_checklist_items.id = ranked.id;
ALTER TABLE project_checklist_items DROP COLUMN sort_order;
CREATE INDEX idx_project_checklist_items_task ON project_checklist_items(task_id, order_key, id);

-- Dependencies are keyed by a derived key over the blocking and blocked task; id and sync_key hold the same value.
ALTER TABLE project_task_dependencies ADD COLUMN sync_key TEXT NOT NULL DEFAULT ''
    CHECK (length(sync_key) <= 128);
UPDATE project_task_dependencies SET id = 'pending:' || rowid, sync_key = 'pending:' || rowid;
CREATE UNIQUE INDEX idx_project_task_dependencies_sync_key ON project_task_dependencies(sync_key);

-- Tag links carry a sync_key over their composite key.
ALTER TABLE project_task_tag_links ADD COLUMN sync_key TEXT NOT NULL DEFAULT '' CHECK (length(sync_key) <= 128);
UPDATE project_task_tag_links SET sync_key = 'pending:' || rowid;
CREATE UNIQUE INDEX idx_project_task_tag_links_sync_key ON project_task_tag_links(sync_key);

-- Custom fields order within their project.
DROP INDEX idx_project_custom_fields_project_sort;
ALTER TABLE project_custom_fields ADD COLUMN order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(order_key) <= 128);
UPDATE project_custom_fields
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_custom_fields
    )
) AS ranked
WHERE project_custom_fields.id = ranked.id;
ALTER TABLE project_custom_fields DROP COLUMN sort_order;
CREATE INDEX idx_project_custom_fields_project_order ON project_custom_fields(project_id, order_key, id);

-- Options order within their field.
DROP INDEX idx_project_custom_field_options_field_sort;
ALTER TABLE project_custom_field_options ADD COLUMN order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(order_key) <= 128);
UPDATE project_custom_field_options
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY field_id ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_custom_field_options
    )
) AS ranked
WHERE project_custom_field_options.id = ranked.id;
ALTER TABLE project_custom_field_options DROP COLUMN sort_order;
CREATE INDEX idx_project_custom_field_options_field_order
    ON project_custom_field_options(field_id, order_key, id);

-- Values are rebuilt: at most one scalar per row, numbers in integer millionths, a sync_key over the composite key,
-- and a parent row for every select value, whose options become owned rows ordered by ordinal.
CREATE TEMP TABLE projects_number_capacity (
    scaled REAL NOT NULL CHECK (scaled < 9223372036854774784.0)
);
INSERT INTO projects_number_capacity (scaled)
SELECT abs(number_value) * 1000000.0 FROM project_custom_field_values WHERE number_value IS NOT NULL;
DROP TABLE projects_number_capacity;

CREATE TEMP TABLE projects_option_values_copy AS
SELECT task_id, field_id, option_id, created_at, rowid AS source_rowid
FROM project_custom_field_option_values;

CREATE TABLE project_custom_field_values_rebuild (
    task_id TEXT NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    field_id TEXT NOT NULL REFERENCES project_custom_fields(id) ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    text_value TEXT,
    number_value INTEGER CHECK (number_value IS NULL OR typeof(number_value) = 'integer'),
    date_value TEXT,
    checkbox_value INTEGER CHECK (checkbox_value IS NULL OR checkbox_value IN (0, 1)),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) CHECK (trim(updated_at) <> ''),
    PRIMARY KEY (task_id, field_id),
    CHECK (
        (text_value IS NOT NULL) + (number_value IS NOT NULL) + (date_value IS NOT NULL) + (checkbox_value IS NOT NULL)
            <= 1
    )
);

INSERT INTO project_custom_field_values_rebuild (
    task_id, field_id, sync_key, text_value, number_value, date_value, checkbox_value, updated_at
)
SELECT
    task_id,
    field_id,
    'pending:' || ROW_NUMBER() OVER (ORDER BY task_id, field_id),
    text_value,
    number_value,
    date_value,
    checkbox_value,
    updated_at
FROM (
    SELECT
        task_id,
        field_id,
        text_value,
        CAST(round(number_value * 1000000) AS INTEGER) AS number_value,
        date_value,
        checkbox_value,
        updated_at
    FROM project_custom_field_values
    UNION ALL
    SELECT task_id, field_id, NULL, NULL, NULL, NULL, max(created_at)
    FROM projects_option_values_copy AS selected
    WHERE NOT EXISTS (
        SELECT 1 FROM project_custom_field_values AS scalar
        WHERE scalar.task_id = selected.task_id AND scalar.field_id = selected.field_id
    )
    GROUP BY task_id, field_id
);

DROP INDEX idx_project_custom_field_option_values_field;
DROP TABLE project_custom_field_option_values;
DROP INDEX idx_project_custom_field_values_date;
DROP INDEX idx_project_custom_field_values_field;
DROP INDEX idx_project_custom_field_values_number;
DROP TABLE project_custom_field_values;
ALTER TABLE project_custom_field_values_rebuild RENAME TO project_custom_field_values;

CREATE INDEX idx_project_custom_field_values_date ON project_custom_field_values(field_id, date_value);
CREATE INDEX idx_project_custom_field_values_field ON project_custom_field_values(field_id);
CREATE INDEX idx_project_custom_field_values_number ON project_custom_field_values(field_id, number_value);

CREATE TABLE project_custom_field_option_values (
    value_key TEXT NOT NULL REFERENCES project_custom_field_values(sync_key) ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    option_id TEXT NOT NULL REFERENCES project_custom_field_options(id) ON DELETE CASCADE,
    PRIMARY KEY (value_key, ordinal)
);
CREATE INDEX idx_project_custom_field_option_values_option ON project_custom_field_option_values(option_id);

INSERT INTO project_custom_field_option_values (value_key, ordinal, option_id)
SELECT
    parent.sync_key,
    ROW_NUMBER() OVER (
        PARTITION BY selected.task_id, selected.field_id
        ORDER BY selected.created_at, selected.source_rowid
    ) - 1,
    selected.option_id
FROM projects_option_values_copy AS selected
JOIN project_custom_field_values AS parent
    ON parent.task_id = selected.task_id AND parent.field_id = selected.field_id;

DROP TABLE projects_option_values_copy;

-- Views order within their project; more than one notes or chat view may exist after a merge.
DROP INDEX idx_project_views_project_sort;
ALTER TABLE project_views ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);
UPDATE project_views
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY sort_order, id) - 1 AS rank
        FROM project_views
    )
) AS ranked
WHERE project_views.id = ranked.id;
ALTER TABLE project_views DROP COLUMN sort_order;
CREATE INDEX idx_project_views_project_order ON project_views(project_id, order_key, id);

-- View preferences carry a sync_key over their composite key.
ALTER TABLE project_view_preferences ADD COLUMN sync_key TEXT NOT NULL DEFAULT ''
    CHECK (length(sync_key) <= 128);
UPDATE project_view_preferences SET sync_key = 'pending:' || rowid;
CREATE UNIQUE INDEX idx_project_view_preferences_sync_key ON project_view_preferences(sync_key);

-- Custom emojis order globally and record the exact size of their icon file.
DROP INDEX idx_project_custom_emojis_sort;
ALTER TABLE project_custom_emojis ADD COLUMN order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(order_key) <= 128);
UPDATE project_custom_emojis
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (ORDER BY sort_order, name, id) - 1 AS rank
        FROM project_custom_emojis
    )
) AS ranked
WHERE project_custom_emojis.id = ranked.id;
ALTER TABLE project_custom_emojis DROP COLUMN sort_order;
CREATE INDEX idx_project_custom_emojis_order ON project_custom_emojis(order_key, id);

-- Icon files are managed assets of at most 3 MiB, and a reference records the exact file size. Existing references
-- get their size from the conversion pass; writers fill it, and the triggers below require it on every write.
ALTER TABLE project_groups ADD COLUMN icon_byte_size INTEGER CHECK (
    icon_byte_size IS NULL
    OR (substr(icon, 1, 20) = 'asset:project-icons/' AND icon_byte_size BETWEEN 1 AND 3145728)
);
ALTER TABLE projects ADD COLUMN icon_byte_size INTEGER CHECK (
    icon_byte_size IS NULL
    OR (substr(icon, 1, 20) = 'asset:project-icons/' AND icon_byte_size BETWEEN 1 AND 3145728)
);
ALTER TABLE project_custom_emojis ADD COLUMN asset_byte_size INTEGER CHECK (
    asset_byte_size IS NULL OR asset_byte_size BETWEEN 1 AND 3145728
);

CREATE TRIGGER project_groups_icon_size_insert
BEFORE INSERT ON project_groups
WHEN substr(NEW.icon, 1, 20) = 'asset:project-icons/' AND NEW.icon_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project group icon size is required');
END;

CREATE TRIGGER project_groups_icon_size_update
BEFORE UPDATE OF icon, icon_byte_size ON project_groups
WHEN substr(NEW.icon, 1, 20) = 'asset:project-icons/' AND NEW.icon_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project group icon size is required');
END;

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

CREATE TRIGGER project_custom_emojis_asset_size_insert
BEFORE INSERT ON project_custom_emojis
WHEN NEW.asset_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project custom emoji size is required');
END;

CREATE TRIGGER project_custom_emojis_asset_size_update
BEFORE UPDATE OF asset_path, asset_byte_size ON project_custom_emojis
WHEN NEW.asset_byte_size IS NULL
BEGIN
    SELECT RAISE(ABORT, 'Project custom emoji size is required');
END;

-- Writers must provide every order key and sync_key.
CREATE TRIGGER project_groups_order_key_insert
BEFORE INSERT ON project_groups
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project group order key is required');
END;

CREATE TRIGGER project_groups_order_key_update
BEFORE UPDATE OF order_key ON project_groups
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project group order key is required');
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

CREATE TRIGGER project_sections_order_key_insert
BEFORE INSERT ON project_sections
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project section order key is required');
END;

CREATE TRIGGER project_sections_order_key_update
BEFORE UPDATE OF order_key ON project_sections
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project section order key is required');
END;

CREATE TRIGGER project_statuses_order_key_insert
BEFORE INSERT ON project_statuses
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project status order key is required');
END;

CREATE TRIGGER project_statuses_order_key_update
BEFORE UPDATE OF order_key ON project_statuses
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project status order key is required');
END;

CREATE TRIGGER project_priorities_order_key_insert
BEFORE INSERT ON project_priorities
WHEN NEW.order_key = '' OR NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project priority order key and sync key are required');
END;

CREATE TRIGGER project_priorities_order_key_update
BEFORE UPDATE OF order_key, sync_key ON project_priorities
WHEN NEW.order_key = '' OR NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project priority order key and sync key are required');
END;

CREATE TRIGGER project_tags_order_key_insert
BEFORE INSERT ON project_tags
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project tag order key is required');
END;

CREATE TRIGGER project_tags_order_key_update
BEFORE UPDATE OF order_key ON project_tags
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project tag order key is required');
END;

CREATE TRIGGER project_tasks_order_key_insert
BEFORE INSERT ON project_tasks
WHEN NEW.section_order_key = '' OR NEW.status_order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task order keys are required');
END;

CREATE TRIGGER project_tasks_order_key_update
BEFORE UPDATE OF section_order_key, status_order_key ON project_tasks
WHEN NEW.section_order_key = '' OR NEW.status_order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project task order keys are required');
END;

CREATE TRIGGER project_checklist_items_order_key_insert
BEFORE INSERT ON project_checklist_items
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project checklist item order key is required');
END;

CREATE TRIGGER project_checklist_items_order_key_update
BEFORE UPDATE OF order_key ON project_checklist_items
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project checklist item order key is required');
END;

CREATE TRIGGER project_task_dependencies_sync_key_insert
BEFORE INSERT ON project_task_dependencies
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project dependency sync key is required');
END;

CREATE TRIGGER project_task_dependencies_sync_key_update
BEFORE UPDATE OF sync_key ON project_task_dependencies
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project dependency sync key is required');
END;

CREATE TRIGGER project_task_tag_links_sync_key_insert
BEFORE INSERT ON project_task_tag_links
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project tag link sync key is required');
END;

CREATE TRIGGER project_task_tag_links_sync_key_update
BEFORE UPDATE OF sync_key ON project_task_tag_links
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project tag link sync key is required');
END;

CREATE TRIGGER project_custom_fields_order_key_insert
BEFORE INSERT ON project_custom_fields
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom field order key is required');
END;

CREATE TRIGGER project_custom_fields_order_key_update
BEFORE UPDATE OF order_key ON project_custom_fields
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom field order key is required');
END;

CREATE TRIGGER project_custom_field_options_order_key_insert
BEFORE INSERT ON project_custom_field_options
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom field option order key is required');
END;

CREATE TRIGGER project_custom_field_options_order_key_update
BEFORE UPDATE OF order_key ON project_custom_field_options
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom field option order key is required');
END;

CREATE TRIGGER project_views_order_key_insert
BEFORE INSERT ON project_views
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project view order key is required');
END;

CREATE TRIGGER project_views_order_key_update
BEFORE UPDATE OF order_key ON project_views
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project view order key is required');
END;

CREATE TRIGGER project_view_preferences_sync_key_insert
BEFORE INSERT ON project_view_preferences
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project view preference sync key is required');
END;

CREATE TRIGGER project_view_preferences_sync_key_update
BEFORE UPDATE OF sync_key ON project_view_preferences
WHEN NEW.sync_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project view preference sync key is required');
END;

CREATE TRIGGER project_custom_emojis_order_key_insert
BEFORE INSERT ON project_custom_emojis
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom emoji order key is required');
END;

CREATE TRIGGER project_custom_emojis_order_key_update
BEFORE UPDATE OF order_key ON project_custom_emojis
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Project custom emoji order key is required');
END;

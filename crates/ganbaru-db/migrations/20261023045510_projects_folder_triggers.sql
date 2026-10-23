-- Working folder rows replicate between devices, so inserting one must not create local Chat rows and the primary
-- folder row must not depend on the folder row arriving first. Chat creates the default channel and the current folder
-- environments on demand, project creation inserts the primary row, and readers fall back to the managed folder when
-- the stored primary folder is missing, belongs to another project, or is archived.

DROP TRIGGER IF EXISTS chat_channels_create_project_general;
DROP TRIGGER IF EXISTS project_working_folders_create_chat_environment;
DROP TRIGGER IF EXISTS project_primary_working_folder_prevent_archive;
DROP TRIGGER IF EXISTS project_primary_working_folder_active_insert;
DROP TRIGGER IF EXISTS project_primary_working_folder_active_update;

CREATE TABLE project_primary_working_folders_next (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL CHECK (length(working_folder_id) BETWEEN 1 AND 1024),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO project_primary_working_folders_next (
    project_id, working_folder_id, revision, created_at, updated_at
)
SELECT project_id, working_folder_id, revision, created_at, updated_at
FROM project_primary_working_folders;

DROP TABLE project_primary_working_folders;
ALTER TABLE project_primary_working_folders_next RENAME TO project_primary_working_folders;

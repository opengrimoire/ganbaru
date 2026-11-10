-- Where this vault keeps each managed project folder. A managed working folder row replicates the
-- folder name its creating device chose; every vault copy claims its own folder for the row here,
-- so two projects that concurrent devices gave the same name never share a folder. The table is
-- not replicated: it travels with the vault database beside the `projects/` folders it names, in
-- snapshot copies and backups alike. `folder_key` is the case-folded name, since Windows and macOS
-- file systems ignore case.
CREATE TABLE project_managed_folder_locations (
  working_folder_id TEXT PRIMARY KEY NOT NULL
      REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
  folder_name TEXT NOT NULL CHECK (
      length(CAST(folder_name AS BLOB)) BETWEEN 1 AND 240
      AND instr(folder_name, '/') = 0
      AND instr(folder_name, char(92)) = 0
      AND folder_name NOT LIKE '.%'
  ),
  folder_key TEXT NOT NULL UNIQUE CHECK (length(CAST(folder_key AS BLOB)) BETWEEN 1 AND 960)
) STRICT;

-- Seeded routine folders are named after their projects. The change is made only before project
-- working folders (table 98) are seeded for sync, so seeding publishes the readable name; a vault
-- seeded earlier keeps the names it already published.
UPDATE sync_apply_state
SET applying = 1
WHERE singleton = 1
  AND NOT EXISTS (SELECT 1 FROM sync_seeded_tables WHERE table_id = 98);

WITH routine(id, folder_name) AS (
  VALUES
    ('working-folder-routine-learning', 'Learning'),
    ('working-folder-routine-reading', 'Reading'),
    ('working-folder-routine-exercise', 'Exercise'),
    ('working-folder-routine-hygiene', 'Hygiene'),
    ('working-folder-routine-eat', 'Eating'),
    ('working-folder-routine-commute', 'Commute'),
    ('working-folder-routine-social', 'Social'),
    ('working-folder-routine-chores', 'Chores'),
    ('working-folder-routine-leisure', 'Leisure'),
    ('working-folder-routine-meditate', 'Meditate'),
    ('working-folder-routine-health', 'Health'),
    ('working-folder-routine-sleep', 'Sleep')
)
UPDATE project_working_folders
SET managed_relative_path = 'projects/' || (
    SELECT routine.folder_name FROM routine WHERE routine.id = project_working_folders.id
)
WHERE id IN (SELECT id FROM routine)
  AND kind = 'managed'
  AND managed_relative_path = 'projects/' || project_id
  AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 1;

UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;

-- Every managed folder that exists keeps the folder its stored path names.
INSERT INTO project_managed_folder_locations (working_folder_id, folder_name, folder_key)
SELECT id, substr(managed_relative_path, 10), lower(substr(managed_relative_path, 10))
FROM project_working_folders
WHERE kind = 'managed'
  AND managed_relative_path LIKE 'projects/_%'
  AND instr(substr(managed_relative_path, 10), '/') = 0
  AND substr(managed_relative_path, 10) NOT LIKE '.%'
ON CONFLICT DO NOTHING;

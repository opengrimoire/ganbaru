-- Workspace panel terminals belong to a channel and one of its project's working folders, so the
-- device-local layout is keyed by both. Thread-keyed layouts have no channel scope and are dropped.
DROP TABLE chat_terminal_layouts;

CREATE TABLE chat_terminal_layouts (
  channel_id TEXT NOT NULL REFERENCES chat_channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
  working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  layout_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (layout_schema_version >= 1),
  layout_data TEXT NOT NULL DEFAULT '{"groups":[]}' CHECK (
      json_valid(layout_data) AND json_type(layout_data) = 'object'
  ),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  PRIMARY KEY (channel_id, working_folder_id)
) STRICT;

CREATE INDEX idx_chat_terminal_layouts_working_folder
  ON chat_terminal_layouts(working_folder_id);

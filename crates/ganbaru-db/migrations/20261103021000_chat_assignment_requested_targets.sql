-- The execution target a post chose for work it created on this device without queueing its
-- dispatch, as when no one here consented to the post. The dispatch that Start here or a
-- recorded acceptance queues later freezes the context on this target, then drops the row.
CREATE TABLE chat_assignment_requested_targets (
  assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  target_data TEXT NOT NULL CHECK (
      length(target_data) <= 4096
      AND json_valid(target_data) AND json_type(target_data) = 'object'
  ),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

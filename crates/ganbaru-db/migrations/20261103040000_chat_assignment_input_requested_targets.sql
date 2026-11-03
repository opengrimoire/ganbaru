-- The execution target a post chose when it became a pending reply to work awaiting review on
-- this device without settling it, as when no one here consented to the post. The follow-up
-- that Start here or restart recovery creates from that reply runs on this target, and the
-- row goes once the reply leaves the reviewed work.
CREATE TABLE chat_assignment_input_requested_targets (
  input_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_work_assignment_inputs(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  target_data TEXT NOT NULL CHECK (
      length(target_data) <= 4096
      AND json_valid(target_data) AND json_type(target_data) = 'object'
  ),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

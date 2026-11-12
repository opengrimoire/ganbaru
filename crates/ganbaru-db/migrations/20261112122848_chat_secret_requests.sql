-- Secret requests a running teammate opens so a person can hand it a value through a private
-- file. The row never holds the value. Runs are device-local, so the requests of a run stay on
-- the device that executes it.
CREATE TABLE IF NOT EXISTS chat_secret_requests (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  run_id TEXT NOT NULL REFERENCES chat_agent_runs(id) ON UPDATE CASCADE ON DELETE CASCADE,
  assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  label TEXT NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 120),
  purpose TEXT NOT NULL CHECK (length(trim(purpose)) BETWEEN 1 AND 500),
  state TEXT NOT NULL CHECK (state IN ('pending', 'provided', 'declined', 'expired')),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  expires_at TEXT NOT NULL CHECK (length(expires_at) >= 20),
  resolved_at TEXT CHECK (resolved_at IS NULL OR length(resolved_at) >= 20)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_chat_secret_requests_run
  ON chat_secret_requests(run_id, state);

CREATE INDEX IF NOT EXISTS idx_chat_secret_requests_assignment
  ON chat_secret_requests(assignment_id);

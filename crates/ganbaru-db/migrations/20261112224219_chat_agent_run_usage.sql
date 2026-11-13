-- Usage of each agent run on the device that executes it. Token counts follow the provider
-- usage convention (input excludes cache reads and writes, output includes reasoning), and
-- counted tokens are input plus cache writes plus output. The thread columns hold the last
-- thread totals a provider reported, so cumulative reports count only their increase. Working
-- time accumulates while the run is working and never while it waits for a person.
CREATE TABLE IF NOT EXISTS chat_agent_run_usage (
  run_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_agent_runs(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
  cached_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cached_input_tokens >= 0),
  cache_write_input_tokens INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_input_tokens >= 0),
  output_tokens INTEGER NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
  counted_tokens INTEGER NOT NULL DEFAULT 0 CHECK (
      counted_tokens >= 0
      AND counted_tokens = input_tokens + cache_write_input_tokens + output_tokens
  ),
  cost_amount REAL CHECK (cost_amount IS NULL OR cost_amount >= 0),
  cost_currency TEXT CHECK (cost_currency IS NULL OR length(cost_currency) BETWEEN 1 AND 16),
  thread_input_tokens_last INTEGER CHECK (
      thread_input_tokens_last IS NULL OR thread_input_tokens_last >= 0
  ),
  thread_cached_input_tokens_last INTEGER CHECK (
      thread_cached_input_tokens_last IS NULL OR thread_cached_input_tokens_last >= 0
  ),
  thread_cache_write_input_tokens_last INTEGER CHECK (
      thread_cache_write_input_tokens_last IS NULL OR thread_cache_write_input_tokens_last >= 0
  ),
  thread_output_tokens_last INTEGER CHECK (
      thread_output_tokens_last IS NULL OR thread_output_tokens_last >= 0
  ),
  thread_cost_last REAL CHECK (thread_cost_last IS NULL OR thread_cost_last >= 0),
  working_seconds INTEGER NOT NULL DEFAULT 0 CHECK (working_seconds >= 0),
  working_since TEXT CHECK (working_since IS NULL OR length(working_since) >= 20),
  scope_unknown_logged INTEGER NOT NULL DEFAULT 0 CHECK (scope_unknown_logged IN (0, 1)),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  CHECK ((cost_amount IS NULL) = (cost_currency IS NULL)),
  CHECK (
      (thread_input_tokens_last IS NULL) = (thread_cached_input_tokens_last IS NULL)
      AND (thread_input_tokens_last IS NULL) = (thread_cache_write_input_tokens_last IS NULL)
      AND (thread_input_tokens_last IS NULL) = (thread_output_tokens_last IS NULL)
  )
) STRICT;

-- Working time follows run state changes made by this device. The interval between entering
-- and leaving `working` is added when the run leaves it; a malformed or reversed interval adds
-- nothing rather than blocking the state change.
CREATE TRIGGER chat_agent_run_usage_working_insert
AFTER INSERT ON chat_agent_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_agent_run_usage (run_id, working_since, updated_at)
    VALUES (
        NEW.id,
        CASE WHEN NEW.state = 'working' THEN NEW.updated_at ELSE NULL END,
        NEW.updated_at
    )
    ON CONFLICT(run_id) DO NOTHING;
END;

CREATE TRIGGER chat_agent_run_usage_working_update
AFTER UPDATE OF state ON chat_agent_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND OLD.state IS NOT NEW.state
BEGIN
    INSERT INTO chat_agent_run_usage (run_id, updated_at)
    VALUES (NEW.id, NEW.updated_at)
    ON CONFLICT(run_id) DO NOTHING;
    UPDATE chat_agent_run_usage
    SET working_seconds = working_seconds + CASE
            WHEN working_since IS NOT NULL AND NEW.state <> 'working' THEN coalesce(max(0,
                CAST(round((julianday(NEW.updated_at) - julianday(working_since)) * 86400)
                    AS INTEGER)), 0)
            ELSE 0
        END,
        working_since = CASE
            WHEN NEW.state = 'working' THEN coalesce(working_since, NEW.updated_at)
            ELSE NULL
        END,
        updated_at = NEW.updated_at
    WHERE run_id = NEW.id;
END;

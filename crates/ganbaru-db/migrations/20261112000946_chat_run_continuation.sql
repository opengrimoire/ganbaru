-- Record why each agent run started and what context it delivered, so a later run in the same
-- reply thread can decide whether to continue the native session and which sources it still
-- has to send. Rows written before these columns exist hold no continuation key and therefore
-- never continue.
ALTER TABLE chat_agent_runs ADD COLUMN attempt_reason TEXT NOT NULL DEFAULT 'fresh' CHECK (
    attempt_reason IN ('fresh', 'continuation', 'handoff', 'retry')
);
ALTER TABLE chat_agent_runs ADD COLUMN handoff_reason TEXT CHECK (
    handoff_reason IS NULL
    OR handoff_reason IN ('key_changed', 'authority_revoked', 'native_session_lost')
);
ALTER TABLE chat_agent_runs ADD COLUMN instructions_sha256 TEXT CHECK (
    instructions_sha256 IS NULL OR length(instructions_sha256) = 64
);
ALTER TABLE chat_agent_runs ADD COLUMN model_selection_sha256 TEXT CHECK (
    model_selection_sha256 IS NULL OR length(model_selection_sha256) = 64
);
ALTER TABLE chat_agent_runs ADD COLUMN context_delivery TEXT NOT NULL DEFAULT 'full' CHECK (
    context_delivery IN ('full', 'delta')
);
ALTER TABLE chat_agent_runs ADD COLUMN delivered_context_bytes INTEGER NOT NULL DEFAULT 0 CHECK (
    delivered_context_bytes BETWEEN 0 AND 131072
);

-- The attempt is fixed once the run leaves `starting`. While it is starting, the launch may
-- still find that the provider could not resume the native session and record the run as a
-- handoff before the provider turn begins.
CREATE TRIGGER chat_agent_run_attempt_immutable
BEFORE UPDATE OF
    attempt_reason, handoff_reason, instructions_sha256,
    model_selection_sha256, context_delivery, delivered_context_bytes
ON chat_agent_runs
WHEN OLD.state <> 'starting'
BEGIN
    SELECT RAISE(ABORT, 'Agent run attempt is immutable');
END;

-- Context sources a run delivered to its native session, keyed by content so an edited source
-- counts as undelivered. Rows exist only for runs whose launch reached the provider.
CREATE TABLE chat_agent_run_delivered_sources (
  run_id TEXT NOT NULL REFERENCES chat_agent_runs(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  source_id TEXT NOT NULL CHECK (length(source_id) BETWEEN 1 AND 1024),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  delivery_kind TEXT NOT NULL CHECK (
      delivery_kind IN ('package', 'pointer', 'steer', 'queued_continuation', 'removal')
  ),
  delivered_at TEXT NOT NULL CHECK (length(delivered_at) >= 20),
  PRIMARY KEY (run_id, source_id, content_sha256)
) STRICT, WITHOUT ROWID;

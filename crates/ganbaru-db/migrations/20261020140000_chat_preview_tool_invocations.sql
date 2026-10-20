-- App-owned record and per-turn budget of model preview tool calls in direct Chat sessions.
CREATE TABLE IF NOT EXISTS chat_preview_tool_invocations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    turn_id TEXT NOT NULL REFERENCES chat_turns(id) ON UPDATE CASCADE ON DELETE CASCADE,
    tool_name TEXT NOT NULL CHECK (length(tool_name) BETWEEN 1 AND 160),
    request_hash TEXT NOT NULL CHECK (length(request_hash) = 64),
    outcome TEXT NOT NULL CHECK (outcome IN ('pending', 'succeeded', 'failed', 'refused')),
    response_bytes INTEGER NOT NULL DEFAULT 0 CHECK (response_bytes >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

CREATE INDEX IF NOT EXISTS idx_chat_preview_tool_invocations_turn
ON chat_preview_tool_invocations(turn_id, created_at, id);

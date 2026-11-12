-- Device-local change clock per provider thread, so the timeline can reread only the rows that
-- changed. Every insert, update, and delete of a timeline row advances the thread's clock and
-- stamps the row (or a removal record) with the new value. Triggers cover every writer, including
-- checkpoint restore invalidation and projection rebuilds, which write rows without an event.
-- Writers that already know the next value set change_sequence themselves, and the guards skip
-- the trigger's second write of that row. The clock row is created with a NOT EXISTS guard, not
-- INSERT OR IGNORE, because an upsert that fires a trigger replaces the trigger's conflict clause.
-- The triggers do not run while sync applies remote changes; sync never writes these device-local
-- tables.
CREATE TABLE chat_timeline_change_clocks (
  thread_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
  last_change INTEGER NOT NULL CHECK (last_change >= 0),
  removals_pruned_through INTEGER NOT NULL DEFAULT 0 CHECK (removals_pruned_through >= 0)
) STRICT;

CREATE TABLE chat_timeline_removals (
  thread_id TEXT NOT NULL REFERENCES chat_threads(id) ON UPDATE CASCADE ON DELETE CASCADE,
  change_sequence INTEGER NOT NULL CHECK (change_sequence >= 1),
  item_id TEXT NOT NULL CHECK (length(item_id) BETWEEN 1 AND 1024),
  removed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
      CHECK (length(removed_at) >= 20),
  PRIMARY KEY (thread_id, change_sequence)
) STRICT;

CREATE INDEX idx_chat_timeline_removals_removed_at ON chat_timeline_removals(removed_at);

ALTER TABLE chat_messages ADD COLUMN change_sequence INTEGER NOT NULL DEFAULT 0 CHECK (change_sequence >= 0);
CREATE INDEX idx_chat_messages_thread_change ON chat_messages(thread_id, change_sequence);

CREATE TRIGGER chat_messages_timeline_change_insert
AFTER INSERT ON chat_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = 0
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_messages SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

CREATE TRIGGER chat_messages_timeline_change_update
AFTER UPDATE ON chat_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = OLD.change_sequence
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_messages SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

ALTER TABLE chat_activities ADD COLUMN change_sequence INTEGER NOT NULL DEFAULT 0 CHECK (change_sequence >= 0);
CREATE INDEX idx_chat_activities_thread_change ON chat_activities(thread_id, change_sequence);

CREATE TRIGGER chat_activities_timeline_change_insert
AFTER INSERT ON chat_activities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = 0
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_activities SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

CREATE TRIGGER chat_activities_timeline_change_update
AFTER UPDATE ON chat_activities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = OLD.change_sequence
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_activities SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

ALTER TABLE chat_plans ADD COLUMN change_sequence INTEGER NOT NULL DEFAULT 0 CHECK (change_sequence >= 0);
CREATE INDEX idx_chat_plans_thread_change ON chat_plans(thread_id, change_sequence);

CREATE TRIGGER chat_plans_timeline_change_insert
AFTER INSERT ON chat_plans
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = 0
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_plans SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

CREATE TRIGGER chat_plans_timeline_change_update
AFTER UPDATE ON chat_plans
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = OLD.change_sequence
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_plans SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

ALTER TABLE chat_turns ADD COLUMN change_sequence INTEGER NOT NULL DEFAULT 0 CHECK (change_sequence >= 0);
CREATE INDEX idx_chat_turns_thread_change ON chat_turns(thread_id, change_sequence);

CREATE TRIGGER chat_turns_timeline_change_insert
AFTER INSERT ON chat_turns
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = 0
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_turns SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

CREATE TRIGGER chat_turns_timeline_change_update
AFTER UPDATE ON chat_turns
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.change_sequence = OLD.change_sequence
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT NEW.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = NEW.thread_id;
  UPDATE chat_turns SET change_sequence = (
      SELECT last_change FROM chat_timeline_change_clocks WHERE thread_id = NEW.thread_id
  ) WHERE id = NEW.id;
END;

CREATE TRIGGER chat_messages_timeline_change_delete
AFTER DELETE ON chat_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM chat_threads WHERE id = OLD.thread_id)
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT OLD.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = OLD.thread_id;
  INSERT INTO chat_timeline_removals (thread_id, change_sequence, item_id)
  SELECT OLD.thread_id, last_change, OLD.id FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id;
END;

CREATE TRIGGER chat_activities_timeline_change_delete
AFTER DELETE ON chat_activities
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM chat_threads WHERE id = OLD.thread_id)
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT OLD.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = OLD.thread_id;
  INSERT INTO chat_timeline_removals (thread_id, change_sequence, item_id)
  SELECT OLD.thread_id, last_change, OLD.id FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id;
END;

CREATE TRIGGER chat_plans_timeline_change_delete
AFTER DELETE ON chat_plans
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM chat_threads WHERE id = OLD.thread_id)
BEGIN
  INSERT INTO chat_timeline_change_clocks (thread_id, last_change)
  SELECT OLD.thread_id, 0
  WHERE NOT EXISTS (SELECT 1 FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id);
  UPDATE chat_timeline_change_clocks SET last_change = last_change + 1 WHERE thread_id = OLD.thread_id;
  INSERT INTO chat_timeline_removals (thread_id, change_sequence, item_id)
  SELECT OLD.thread_id, last_change, OLD.id FROM chat_timeline_change_clocks WHERE thread_id = OLD.thread_id;
END;

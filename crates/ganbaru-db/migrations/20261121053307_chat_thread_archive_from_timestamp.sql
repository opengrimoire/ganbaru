-- Archiving a Chat thread is recorded only by archived_at. The state column keeps the thread's
-- activity, reads report archived from the timestamp, and these triggers stand in for a CHECK
-- that SQLite cannot add to the existing table without rebuilding it.
UPDATE chat_threads SET state = 'idle' WHERE state = 'archived';

CREATE TRIGGER chat_threads_state_not_archived_insert
BEFORE INSERT ON chat_threads
WHEN NEW.state = 'archived'
BEGIN
    SELECT RAISE(ABORT, 'Chat thread archiving is recorded by archived_at');
END;

CREATE TRIGGER chat_threads_state_not_archived_update
BEFORE UPDATE OF state ON chat_threads
WHEN NEW.state = 'archived'
BEGIN
    SELECT RAISE(ABORT, 'Chat thread archiving is recorded by archived_at');
END;

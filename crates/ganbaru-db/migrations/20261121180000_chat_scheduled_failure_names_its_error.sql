-- A scheduled message that failed always carries the error that failed it, so the failed state
-- it reads as can always say why. The rule guards local writers only; replicated failures are
-- checked by the Chat sync domain, which carries the failure time and error as one group.
CREATE TRIGGER chat_scheduled_messages_failure_names_its_error_insert
BEFORE INSERT ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.failed_at IS NOT NULL AND NEW.last_error IS NULL
BEGIN
    SELECT RAISE(ABORT, 'A failed Chat scheduled message must carry its error');
END;

CREATE TRIGGER chat_scheduled_messages_failure_names_its_error_update
BEFORE UPDATE OF failed_at, last_error ON chat_scheduled_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.failed_at IS NOT NULL AND NEW.last_error IS NULL
BEGIN
    SELECT RAISE(ABORT, 'A failed Chat scheduled message must carry its error');
END;

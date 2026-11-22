-- An active Focus segment has neither an actual end nor an end reason, and a completed or
-- interrupted segment has both, so a closed segment always says when and why it ended. A
-- Focus pause names one of the reasons the native Focus owner records. Both rules guard
-- local writers only; replicated rows are checked by the Focus sync domain, which also
-- publishes a segment only with its closed run.
CREATE TRIGGER pomodoro_segments_lifecycle_columns_agree_insert
BEFORE INSERT ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        (NEW.status = 'active' AND NEW.actual_end IS NULL AND NEW.end_reason IS NULL)
        OR (NEW.status IN ('completed', 'interrupted')
            AND NEW.actual_end IS NOT NULL AND NEW.end_reason IS NOT NULL)
    )
BEGIN
    SELECT RAISE(ABORT, 'A Focus segment end must agree with its status');
END;

CREATE TRIGGER pomodoro_segments_lifecycle_columns_agree_update
BEFORE UPDATE OF status, actual_end, end_reason ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        (NEW.status = 'active' AND NEW.actual_end IS NULL AND NEW.end_reason IS NULL)
        OR (NEW.status IN ('completed', 'interrupted')
            AND NEW.actual_end IS NOT NULL AND NEW.end_reason IS NOT NULL)
    )
BEGIN
    SELECT RAISE(ABORT, 'A Focus segment end must agree with its status');
END;

CREATE TRIGGER pomodoro_pauses_known_reason_insert
BEFORE INSERT ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.reason NOT IN ('manual', 'idle', 'suspend')
BEGIN
    SELECT RAISE(ABORT, 'A Focus pause reason must be manual, idle, or suspend');
END;

CREATE TRIGGER pomodoro_pauses_known_reason_update
BEFORE UPDATE OF reason ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.reason NOT IN ('manual', 'idle', 'suspend')
BEGIN
    SELECT RAISE(ABORT, 'A Focus pause reason must be manual, idle, or suspend');
END;

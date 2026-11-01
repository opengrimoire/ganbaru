-- One active segment across open runs: a segment left active on a closed run never blocks a
-- local start or phase change.
DROP TRIGGER pomodoro_segments_single_active_insert;
DROP TRIGGER pomodoro_segments_single_active_update;

CREATE TRIGGER pomodoro_segments_single_active_insert
BEFORE INSERT ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.status = 'active'
    AND EXISTS (
        SELECT 1 FROM pomodoro_segments AS s
        JOIN pomodoro_runs AS r ON r.id = s.run_id
        WHERE s.status = 'active' AND r.ended_at IS NULL
    )
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one active segment');
END;

CREATE TRIGGER pomodoro_segments_single_active_update
BEFORE UPDATE OF status ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.status = 'active'
    AND EXISTS (
        SELECT 1 FROM pomodoro_segments AS s
        JOIN pomodoro_runs AS r ON r.id = s.run_id
        WHERE s.status = 'active' AND r.ended_at IS NULL AND s.id IS NOT NEW.id
    )
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one active segment');
END;

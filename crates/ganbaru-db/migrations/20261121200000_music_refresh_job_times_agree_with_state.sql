-- A queued Music refresh has neither started nor finished, a running one has started but not
-- finished, a completed, partial, or failed one has both times, and a cancelled one has
-- finished, whether or not it had started. Refresh jobs are device-local and never replicate,
-- so the rule applies to every write.
CREATE TRIGGER music_refresh_jobs_times_agree_insert
BEFORE INSERT ON music_refresh_jobs
WHEN NOT (
    (NEW.state = 'queued' AND NEW.started_at_ms IS NULL AND NEW.finished_at_ms IS NULL)
    OR (NEW.state = 'running' AND NEW.started_at_ms IS NOT NULL AND NEW.finished_at_ms IS NULL)
    OR (NEW.state IN ('completed', 'partial', 'failed')
        AND NEW.started_at_ms IS NOT NULL AND NEW.finished_at_ms IS NOT NULL)
    OR (NEW.state = 'cancelled' AND NEW.finished_at_ms IS NOT NULL)
)
BEGIN
    SELECT RAISE(ABORT, 'A Music refresh job start and finish times must agree with its state');
END;

CREATE TRIGGER music_refresh_jobs_times_agree_update
BEFORE UPDATE OF state, started_at_ms, finished_at_ms ON music_refresh_jobs
WHEN NOT (
    (NEW.state = 'queued' AND NEW.started_at_ms IS NULL AND NEW.finished_at_ms IS NULL)
    OR (NEW.state = 'running' AND NEW.started_at_ms IS NOT NULL AND NEW.finished_at_ms IS NULL)
    OR (NEW.state IN ('completed', 'partial', 'failed')
        AND NEW.started_at_ms IS NOT NULL AND NEW.finished_at_ms IS NOT NULL)
    OR (NEW.state = 'cancelled' AND NEW.finished_at_ms IS NOT NULL)
)
BEGIN
    SELECT RAISE(ABORT, 'A Music refresh job start and finish times must agree with its state');
END;

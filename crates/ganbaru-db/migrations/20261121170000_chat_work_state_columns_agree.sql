-- A Chat work assignment carries a settled time exactly when it has completed, failed, or
-- been cancelled, so open work never looks settled and ended work always says when it
-- ended. A claimed dispatch job always carries the claim token that fences its launch.
-- Both rules guard local writers only; replicated assignments are checked by the Chat sync
-- domain, and dispatch jobs never replicate.
CREATE TRIGGER chat_work_assignments_settled_at_agrees_insert
BEFORE INSERT ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (NEW.settled_at IS NOT NULL) <> (NEW.state IN ('completed', 'failed', 'cancelled'))
BEGIN
    SELECT RAISE(ABORT, 'A Chat work assignment settled time must agree with its state');
END;

CREATE TRIGGER chat_work_assignments_settled_at_agrees_update
BEFORE UPDATE OF state, settled_at ON chat_work_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (NEW.settled_at IS NOT NULL) <> (NEW.state IN ('completed', 'failed', 'cancelled'))
BEGIN
    SELECT RAISE(ABORT, 'A Chat work assignment settled time must agree with its state');
END;

CREATE TRIGGER chat_assignment_dispatch_jobs_claim_token_agrees_insert
BEFORE INSERT ON chat_assignment_dispatch_jobs
WHEN NEW.state = 'claimed' AND NEW.claim_token IS NULL
BEGIN
    SELECT RAISE(ABORT, 'A claimed Chat dispatch job must carry its claim token');
END;

CREATE TRIGGER chat_assignment_dispatch_jobs_claim_token_agrees_update
BEFORE UPDATE OF state, claim_token ON chat_assignment_dispatch_jobs
WHEN NEW.state = 'claimed' AND NEW.claim_token IS NULL
BEGIN
    SELECT RAISE(ABORT, 'A claimed Chat dispatch job must carry its claim token');
END;

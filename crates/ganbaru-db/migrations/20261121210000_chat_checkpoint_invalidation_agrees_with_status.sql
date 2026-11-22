-- A capturing or available checkpoint has never been invalidated, so it carries neither an
-- invalidation time nor an invalidating checkpoint. An invalid checkpoint always records when it
-- was invalidated; its invalidating checkpoint may later be deleted. Cleanup states follow either
-- path, so they keep whatever invalidation they had. Checkpoints are device-local and never
-- replicate, so the rule applies to every write.
CREATE TRIGGER chat_checkpoints_invalidation_agrees_insert
BEFORE INSERT ON chat_checkpoints
WHEN NOT (
    (NEW.status IN ('capturing', 'available')
        AND NEW.invalidated_at IS NULL AND NEW.invalidated_by_checkpoint_id IS NULL)
    OR (NEW.status = 'invalid' AND NEW.invalidated_at IS NOT NULL)
    OR NEW.status IN ('cleanup_pending', 'cleanup_failed')
)
BEGIN
    SELECT RAISE(ABORT, 'A Chat checkpoint invalidation must agree with its status');
END;

CREATE TRIGGER chat_checkpoints_invalidation_agrees_update
BEFORE UPDATE OF status, invalidated_at, invalidated_by_checkpoint_id ON chat_checkpoints
WHEN NOT (
    (NEW.status IN ('capturing', 'available')
        AND NEW.invalidated_at IS NULL AND NEW.invalidated_by_checkpoint_id IS NULL)
    OR (NEW.status = 'invalid' AND NEW.invalidated_at IS NOT NULL)
    OR NEW.status IN ('cleanup_pending', 'cleanup_failed')
)
BEGIN
    SELECT RAISE(ABORT, 'A Chat checkpoint invalidation must agree with its status');
END;

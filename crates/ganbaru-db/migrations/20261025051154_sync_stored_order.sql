-- Storage order of operations per space.
--
-- Every stored operation takes the number after the space's latest one, so chain opening
-- follows storage order even when the wall clock steps back. `stored_at_ms` stays the wall
-- clock for acknowledgement intervals and dormancy. The lookup runs on every insert.

ALTER TABLE sync_ops ADD COLUMN stored_order INTEGER NOT NULL DEFAULT 0
    CHECK (stored_order >= 0);

CREATE INDEX idx_sync_ops_stored_order ON sync_ops(space_id, stored_order);

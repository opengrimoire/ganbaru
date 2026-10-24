-- Whether an operation's signed header says the writer's next operation belongs to the same
-- apply group, copied so apply finds group boundaries without decoding envelopes.
ALTER TABLE sync_ops ADD COLUMN group_continues INTEGER NOT NULL DEFAULT 0
    CHECK (group_continues IN (0, 1));

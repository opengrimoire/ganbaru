-- Operations whose flag ends their writer's apply chain.
--
-- Apply finds where a writer's chain starts from its last such operation, so the lookup stays
-- bounded when a writer flags every operation to continue.

CREATE INDEX idx_sync_ops_chain_ends ON sync_ops(space_id, writer_id, seq)
    WHERE group_continues = 0;

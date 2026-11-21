-- An excluded sync operation is held as invalid: the engine stores the excluded state as
-- `state = 'held'`, `hold_reason = 'invalid'`, and the removal revision, and no writer stores an
-- exclusion revision on a waiting, applied, or differently held operation. SQLite adds a CHECK
-- only by rebuilding the table. No table, view, or trigger refers to sync_ops, so the rebuild
-- keeps every row and the indexes are created again as they were.
CREATE TABLE sync_ops_next (
  space_id BLOB NOT NULL,
  writer_id BLOB NOT NULL,
  seq INTEGER NOT NULL CHECK (seq >= 1),
  kind INTEGER NOT NULL CHECK (kind BETWEEN 0 AND 255),
  hash BLOB NOT NULL CHECK (length(hash) = 32),
  clock INTEGER NOT NULL CHECK (clock >= 0),
  context BLOB NOT NULL,
  envelope BLOB NOT NULL CHECK (length(envelope) <= 524288),
  state TEXT NOT NULL CHECK (state IN ('waiting', 'applied', 'held')),
  hold_reason TEXT CHECK (hold_reason IN ('newer_format', 'newer_manifest', 'invalid')),
  stored_at_ms INTEGER NOT NULL CHECK (stored_at_ms >= 0),
  authorization_revision INTEGER NOT NULL DEFAULT 0
    CHECK (authorization_revision >= 0),
  excluded_revision INTEGER
    CHECK (excluded_revision IS NULL OR excluded_revision >= 1),
  group_continues INTEGER NOT NULL DEFAULT 0
    CHECK (group_continues IN (0, 1)),
  stored_order INTEGER NOT NULL DEFAULT 0
    CHECK (stored_order >= 0),
  PRIMARY KEY (space_id, writer_id, seq),
  FOREIGN KEY (space_id, writer_id) REFERENCES sync_writers(space_id, writer_id) ON DELETE CASCADE,
  CHECK ((state = 'held') = (hold_reason IS NOT NULL)),
  CHECK (excluded_revision IS NULL OR (state = 'held' AND hold_reason = 'invalid'))
) STRICT;

INSERT INTO sync_ops_next
    (space_id, writer_id, seq, kind, hash, clock, context, envelope, state, hold_reason,
     stored_at_ms, authorization_revision, excluded_revision, group_continues, stored_order)
SELECT space_id, writer_id, seq, kind, hash, clock, context, envelope, state, hold_reason,
    stored_at_ms, authorization_revision, excluded_revision, group_continues, stored_order
FROM sync_ops;

DROP TABLE sync_ops;
ALTER TABLE sync_ops_next RENAME TO sync_ops;

CREATE INDEX idx_sync_ops_pending ON sync_ops(state, space_id, writer_id, seq)
    WHERE state <> 'applied';
CREATE INDEX idx_sync_ops_excluded ON sync_ops(space_id, excluded_revision, writer_id, seq)
    WHERE excluded_revision IS NOT NULL;
CREATE INDEX idx_sync_ops_chain_ends ON sync_ops(space_id, writer_id, seq)
    WHERE group_continues = 0;
CREATE INDEX idx_sync_ops_stored_order ON sync_ops(space_id, stored_order);

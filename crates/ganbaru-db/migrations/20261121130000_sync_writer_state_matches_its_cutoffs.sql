-- no-transaction
-- A sync writer's state is derived from replicated facts only, so its stored state and cutoffs
-- cannot contradict each other: an active writer has no cutoff and no removal cutoff, a revoked
-- writer's cutoff is its removal cutoff, and a forked writer's cutoff is the last sequence before
-- its fork, whatever its removal cutoff. The installation-local retired state goes away, since a
-- writer is replaced on its own device without changing how replicas treat it. SQLite adds a
-- CHECK only by rebuilding the table, and the rows are copied with their state derived the same
-- way the engine derives it.
--
-- sync_ops cascades from sync_writers, so dropping the old table would drop every stored
-- operation: foreign keys are off for this connection and the rebuild commits as one transaction.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

CREATE TABLE sync_writers_next (
  space_id BLOB NOT NULL REFERENCES sync_spaces(space_id) ON DELETE CASCADE,
  writer_id BLOB NOT NULL CHECK (length(writer_id) = 16),
  public_key BLOB NOT NULL UNIQUE CHECK (length(public_key) = 32),
  device_id TEXT NOT NULL CHECK (length(CAST(device_id AS BLOB)) BETWEEN 1 AND 160),
  certificate BLOB NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('active', 'revoked', 'forked')),
  predecessor BLOB CHECK (predecessor IS NULL OR length(predecessor) = 16),
  cutoff_seq INTEGER CHECK (cutoff_seq IS NULL OR cutoff_seq >= 0),
  stored_seq INTEGER NOT NULL CHECK (stored_seq >= 1),
  applied_seq INTEGER NOT NULL CHECK (applied_seq >= 0 AND applied_seq <= stored_seq),
  head_hash BLOB NOT NULL CHECK (length(head_hash) = 32),
  head_clock INTEGER NOT NULL CHECK (head_clock >= 0),
  created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
  compacted_seq INTEGER NOT NULL DEFAULT 0 CHECK (compacted_seq >= 0),
  removal_cutoff_seq INTEGER
    CHECK (removal_cutoff_seq IS NULL OR removal_cutoff_seq >= 0),
  PRIMARY KEY (space_id, writer_id),
  CHECK (
    (state = 'active' AND cutoff_seq IS NULL AND removal_cutoff_seq IS NULL)
    OR (state = 'revoked' AND removal_cutoff_seq IS NOT NULL AND cutoff_seq IS removal_cutoff_seq)
    OR (state = 'forked' AND cutoff_seq IS NOT NULL)
  )
) STRICT;

INSERT INTO sync_writers_next
    (space_id, writer_id, public_key, device_id, certificate, state, predecessor, cutoff_seq,
     stored_seq, applied_seq, head_hash, head_clock, created_at_ms, compacted_seq,
     removal_cutoff_seq)
SELECT w.space_id, w.writer_id, w.public_key, w.device_id, w.certificate,
    CASE
      WHEN e.seq IS NOT NULL THEN 'forked'
      WHEN w.removal_cutoff_seq IS NOT NULL THEN 'revoked'
      ELSE 'active'
    END,
    w.predecessor,
    CASE
      WHEN e.seq IS NOT NULL THEN e.seq - 1
      ELSE w.removal_cutoff_seq
    END,
    w.stored_seq, w.applied_seq, w.head_hash, w.head_clock, w.created_at_ms, w.compacted_seq,
    w.removal_cutoff_seq
FROM sync_writers w
LEFT JOIN sync_fork_evidence e ON e.space_id = w.space_id AND e.writer_id = w.writer_id;

DROP TABLE sync_writers;
ALTER TABLE sync_writers_next RENAME TO sync_writers;

COMMIT;

PRAGMA foreign_keys = ON;

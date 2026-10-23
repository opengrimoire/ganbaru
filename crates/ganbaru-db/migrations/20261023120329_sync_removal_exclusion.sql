-- Removal exclusion state the authorization log derives.
--
-- `reachable_revision` is the highest revision any chain the space holds reaches from the
-- charter, selected or superseded; readiness on an operation's authorization revision compares
-- against it. `anchor_device_id` is the anchor of the selected chain, whose confirmations bound
-- which operations a removal can still exclude. `local_device_id` is the device of the local
-- writer, whose own operations are never excluded on this replica. `removal_cutoff_seq` is the
-- cutoff the selected removal of a writer's device lists for it (0 when it lists none), which
-- excludes every later operation of the writer whatever its lifecycle state. A late change
-- records whether an admin restored or discarded it, so a discarded row created by a removed
-- device stays hidden.

ALTER TABLE sync_spaces ADD COLUMN reachable_revision INTEGER NOT NULL DEFAULT 0
    CHECK (reachable_revision >= 0);

ALTER TABLE sync_spaces ADD COLUMN anchor_device_id TEXT
    CHECK (anchor_device_id IS NULL OR length(CAST(anchor_device_id AS BLOB)) BETWEEN 1 AND 160);

ALTER TABLE sync_spaces ADD COLUMN local_device_id TEXT
    CHECK (local_device_id IS NULL OR length(CAST(local_device_id AS BLOB)) BETWEEN 1 AND 160);

ALTER TABLE sync_writers ADD COLUMN removal_cutoff_seq INTEGER
    CHECK (removal_cutoff_seq IS NULL OR removal_cutoff_seq >= 0);

ALTER TABLE sync_late_changes ADD COLUMN resolution TEXT
    CHECK (resolution IS NULL OR resolution IN ('restore', 'discard'));

CREATE INDEX idx_sync_ops_excluded ON sync_ops(space_id, excluded_revision, writer_id, seq)
    WHERE excluded_revision IS NOT NULL;

CREATE INDEX idx_sync_late_changes_row ON sync_late_changes(table_id, row_key);

CREATE INDEX idx_sync_displaced_versions_displacer
    ON sync_displaced_versions(displaced_by_writer, displaced_by_seq);

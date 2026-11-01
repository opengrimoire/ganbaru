CREATE TABLE sync_refused_rows (
  table_id INTEGER NOT NULL,
  row_key TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('hidden', 'kept')),
  reason TEXT NOT NULL,
  attempt INTEGER NOT NULL,
  PRIMARY KEY (table_id, row_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_sync_refused_rows_attempt ON sync_refused_rows(attempt);

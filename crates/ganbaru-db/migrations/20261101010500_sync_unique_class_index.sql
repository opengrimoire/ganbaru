CREATE TABLE sync_unique_classes (
  table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
  rule_index INTEGER NOT NULL CHECK (rule_index >= 0),
  class_key BLOB NOT NULL CHECK (length(class_key) >= 1),
  row_key TEXT NOT NULL CHECK (length(CAST(row_key AS BLOB)) BETWEEN 1 AND 128),
  PRIMARY KEY (table_id, rule_index, class_key, row_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_sync_unique_classes_row ON sync_unique_classes(table_id, row_key);

CREATE TABLE sync_contested_classes (
  table_id INTEGER NOT NULL CHECK (table_id BETWEEN 1 AND 65535),
  rule_index INTEGER NOT NULL CHECK (rule_index >= 0),
  class_key BLOB NOT NULL CHECK (length(class_key) >= 1),
  PRIMARY KEY (table_id, rule_index, class_key)
) STRICT, WITHOUT ROWID;

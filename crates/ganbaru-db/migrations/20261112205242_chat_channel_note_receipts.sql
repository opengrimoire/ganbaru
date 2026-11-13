-- no-transaction
-- Organizational command receipts gain the channel note kind, so saving, editing, deleting,
-- accepting, and dismissing a channel note is idempotent by its client command id. SQLite
-- cannot change a CHECK in place, so the device-local table is rebuilt once with every kind. No
-- table references the receipts and they carry no triggers or indexes; foreign keys are still
-- off for the rebuild, and the rebuild repeats safely if it stops before sqlx records it.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP TABLE IF EXISTS chat_organizational_command_receipts_rebuild;

CREATE TABLE chat_organizational_command_receipts_rebuild (
  client_command_id TEXT PRIMARY KEY NOT NULL CHECK (length(client_command_id) BETWEEN 1 AND 1024),
  command_kind TEXT NOT NULL CHECK (
      command_kind IN (
          'post_message', 'publish_teammate', 'update_membership', 'assignment_action',
          'channel_note'
      )
  ),
  state TEXT NOT NULL CHECK (state IN ('accepted', 'completed', 'failed')),
  result_schema_version INTEGER CHECK (result_schema_version IS NULL OR result_schema_version >= 1),
  result_data TEXT CHECK (result_data IS NULL OR json_valid(result_data)),
  error_data TEXT CHECK (error_data IS NULL OR json_valid(error_data)),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_organizational_command_receipts_rebuild
SELECT * FROM chat_organizational_command_receipts;

DROP TABLE chat_organizational_command_receipts;

PRAGMA legacy_alter_table = ON;
ALTER TABLE chat_organizational_command_receipts_rebuild
RENAME TO chat_organizational_command_receipts;
PRAGMA legacy_alter_table = OFF;

COMMIT;

PRAGMA foreign_keys = ON;

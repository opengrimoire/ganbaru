-- no-transaction
-- Context package sources gain the channel note kind, so a package records which channel notes
-- it sent with their content hashes. SQLite cannot change a CHECK in place, so the device-local
-- table is rebuilt once with every kind. No table references the sources and they carry no
-- triggers or indexes; foreign keys are still off for the rebuild, and the rebuild repeats safely
-- if it stops before sqlx records it.
PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

DROP TABLE IF EXISTS chat_assignment_context_sources_rebuild;

CREATE TABLE chat_assignment_context_sources_rebuild (
  context_package_id TEXT NOT NULL REFERENCES chat_assignment_context_packages(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  source_kind TEXT NOT NULL CHECK (
      source_kind IN (
          'trigger', 'thread_root', 'thread_reply', 'channel_message', 'attachment', 'resource',
          'channel_note'
      )
  ),
  source_id TEXT NOT NULL CHECK (length(source_id) BETWEEN 1 AND 1024),
  source_revision INTEGER NOT NULL DEFAULT 1 CHECK (source_revision >= 1),
  content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
  ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
  PRIMARY KEY (context_package_id, source_kind, source_id),
  UNIQUE (context_package_id, ordinal)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_assignment_context_sources_rebuild
SELECT * FROM chat_assignment_context_sources;

DROP TABLE chat_assignment_context_sources;

PRAGMA legacy_alter_table = ON;
ALTER TABLE chat_assignment_context_sources_rebuild
RENAME TO chat_assignment_context_sources;
PRAGMA legacy_alter_table = OFF;

COMMIT;

PRAGMA foreign_keys = ON;

-- no-transaction
-- Chat timeline replication: reply threads, conversation items, communication messages, message
-- revisions, message references, their five target tables, and both read cursors join the vault
-- manifest.
--
-- Items order by an order key within their scope `(conversation_id, reply_thread_id)` instead of
-- an integer ordinal; existing items get rank keys ('c' and three base 62 digits) in ordinal
-- order, and a scope at or above 238,328 items aborts the migration, which then needs a
-- development reset. Every position that pointed at an ordinal points at an item instead: read
-- cursors hold the last read `(order_key, item_id)` ('' when nothing is read), channel reference
-- ranges and the local authorized channel and scratch sources hold `(order_key, item_id)` bounds
-- whose lower item id is '' (a bound before every item with that key), and AI channel history
-- starts at an order key. Revision numbers, the global revision ordinal table, the stored reply
-- count, and the uniques two devices could break concurrently are dropped; revision display
-- numbers come from `(created_at, id)` order through a view. Read cursors gain a `sync_key` that
-- holds a placeholder until the Chat conversion pass rewrites it at vault open. Mention targets
-- keep their values without foreign keys, and foreign keys from work assignments and inputs into
-- messages cascade.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TEMP TABLE chat_timeline_scope_sizes (
    row_count INTEGER NOT NULL,
    CONSTRAINT chat_timeline_scope_exceeds_rank_keys_reset_the_development_vault
        CHECK (row_count < 238328)
);
INSERT INTO chat_timeline_scope_sizes (row_count)
SELECT count(*) FROM chat_conversation_items GROUP BY conversation_id, reply_thread_id;
CREATE TEMP TABLE chat_timeline_rank_limit AS
SELECT coalesce(max(row_count), 0) AS max_count FROM chat_timeline_scope_sizes;
DROP TABLE chat_timeline_scope_sizes;

-- Rank keys for every position up to the largest scope, and each item's key in its scope.
CREATE TEMP TABLE chat_timeline_rank_keys (
    position INTEGER PRIMARY KEY NOT NULL,
    order_key TEXT NOT NULL
);
WITH RECURSIVE positions(position) AS (
    SELECT 0
    UNION ALL
    SELECT positions.position + 1 FROM positions, chat_timeline_rank_limit
    WHERE positions.position < chat_timeline_rank_limit.max_count
)
INSERT INTO chat_timeline_rank_keys (position, order_key)
SELECT
    position,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', position / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', position / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', position % 62 + 1, 1)
FROM positions;

CREATE TEMP TABLE chat_timeline_item_keys (
    id TEXT PRIMARY KEY NOT NULL,
    conversation_id TEXT NOT NULL,
    reply_thread_id TEXT,
    ordinal INTEGER NOT NULL,
    order_key TEXT NOT NULL
);
INSERT INTO chat_timeline_item_keys (id, conversation_id, reply_thread_id, ordinal, order_key)
SELECT ranked.id, ranked.conversation_id, ranked.reply_thread_id, ranked.ordinal, rank_key.order_key
FROM (
    SELECT
        id, conversation_id, reply_thread_id, ordinal,
        ROW_NUMBER() OVER (
            PARTITION BY conversation_id, reply_thread_id ORDER BY ordinal, id
        ) - 1 AS position
    FROM chat_conversation_items
) ranked
JOIN chat_timeline_rank_keys rank_key ON rank_key.position = ranked.position;
CREATE INDEX chat_timeline_item_keys_scope
ON chat_timeline_item_keys(conversation_id, reply_thread_id, ordinal);

-- The key before which a root ordinal starts, so `order_key >= key` admits exactly the root
-- items at or after that ordinal.
CREATE TEMP TABLE chat_timeline_root_boundaries (
    conversation_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    order_key TEXT NOT NULL,
    PRIMARY KEY (conversation_id, ordinal)
);
INSERT OR IGNORE INTO chat_timeline_root_boundaries (conversation_id, ordinal, order_key)
SELECT bound.conversation_id, bound.ordinal, rank_key.order_key
FROM (
    SELECT conversation_id, history_from_ordinal AS ordinal
    FROM chat_teammate_channel_memberships WHERE history_from_ordinal IS NOT NULL
    UNION
    SELECT source_conversation_id, source_lower_ordinal
    FROM chat_channel_reference_targets WHERE source_high_ordinal > 0
    UNION
    SELECT conversation_id, lower_ordinal FROM chat_assignment_authorized_channel_sources
    UNION
    SELECT conversation_id, lower_ordinal FROM chat_scratch_generation_sources
) bound
JOIN chat_timeline_rank_keys rank_key ON rank_key.position = (
    SELECT count(*) FROM chat_timeline_item_keys item
    WHERE item.conversation_id = bound.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.ordinal < bound.ordinal
);

-- Reply threads count their replies through the timeline index instead of a stored count.
CREATE TABLE chat_reply_threads_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    root_item_id TEXT NOT NULL UNIQUE CHECK (length(root_item_id) BETWEEN 1 AND 1024),
    last_activity_at TEXT NOT NULL CHECK (length(last_activity_at) >= 20),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_reply_threads_next (
    id, conversation_id, root_item_id, last_activity_at, revision, created_at, updated_at
)
SELECT id, conversation_id, root_item_id, last_activity_at, revision, created_at, updated_at
FROM chat_reply_threads;

DROP TABLE chat_reply_threads;
ALTER TABLE chat_reply_threads_next RENAME TO chat_reply_threads;

CREATE INDEX idx_chat_reply_threads_conversation
ON chat_reply_threads(conversation_id, last_activity_at DESC, id);

CREATE TRIGGER chat_reply_threads_root_item_insert
BEFORE INSERT ON chat_reply_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1 FROM chat_conversation_items item
    WHERE item.id = NEW.root_item_id
      AND item.conversation_id = NEW.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.item_kind = 'message'
)
BEGIN
    SELECT RAISE(ABORT, 'Reply thread root must be a root communication message');
END;

-- Items order by key within their scope; equal keys from concurrent posts order by id.
CREATE TABLE chat_conversation_items_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reply_thread_id TEXT REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    item_kind TEXT NOT NULL CHECK (
        item_kind IN ('message', 'work_update', 'approval_request', 'question', 'review_packet')
    ),
    order_key TEXT NOT NULL CHECK (length(order_key) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

INSERT INTO chat_conversation_items_next (
    id, conversation_id, reply_thread_id, item_kind, order_key, created_at
)
SELECT item.id, item.conversation_id, item.reply_thread_id, item.item_kind, keyed.order_key,
    item.created_at
FROM chat_conversation_items item
JOIN chat_timeline_item_keys keyed ON keyed.id = item.id;

DROP TABLE chat_conversation_items;
ALTER TABLE chat_conversation_items_next RENAME TO chat_conversation_items;

CREATE INDEX idx_chat_conversation_items_order
ON chat_conversation_items(conversation_id, reply_thread_id, order_key, id);

CREATE TRIGGER chat_conversation_items_reply_thread_insert
BEFORE INSERT ON chat_conversation_items
WHEN NEW.reply_thread_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM chat_reply_threads thread
    WHERE thread.id = NEW.reply_thread_id
      AND thread.conversation_id = NEW.conversation_id
)
BEGIN
    SELECT RAISE(ABORT, 'Reply item belongs to another conversation');
END;

-- A message keeps its author label when the author participant is gone. Search rows follow the
-- current revision for local writes; the Chat sync hook rebuilds them for applied rows.
CREATE TABLE chat_communication_messages_next (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    author_participant_id TEXT REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    current_revision_id TEXT CHECK (
        current_revision_id IS NULL OR length(current_revision_id) BETWEEN 1 AND 1024
    ),
    edited_at TEXT CHECK (edited_at IS NULL OR length(edited_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    author_label_snapshot TEXT NOT NULL DEFAULT 'Unknown participant' CHECK (
        length(trim(author_label_snapshot)) BETWEEN 1 AND 160
    )
) STRICT;

INSERT INTO chat_communication_messages_next (
    item_id, author_participant_id, current_revision_id, edited_at, deleted_at, created_at,
    author_label_snapshot
)
SELECT item_id, author_participant_id, current_revision_id, edited_at, deleted_at, created_at,
    author_label_snapshot
FROM chat_communication_messages;

DROP TABLE chat_communication_messages;
ALTER TABLE chat_communication_messages_next RENAME TO chat_communication_messages;

CREATE TRIGGER chat_communication_message_current_revision_update
BEFORE UPDATE OF current_revision_id ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.current_revision_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM chat_communication_message_revisions revision
    WHERE revision.id = NEW.current_revision_id
      AND revision.message_item_id = NEW.item_id
)
BEGIN
    SELECT RAISE(ABORT, 'Current message revision belongs to another message');
END;

CREATE TRIGGER chat_communication_search_delete
AFTER DELETE ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = OLD.item_id;
END;

CREATE TRIGGER chat_communication_search_insert
AFTER UPDATE OF current_revision_id ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NEW.current_revision_id IS NOT NULL
BEGIN
    DELETE FROM chat_communication_search_fts WHERE message_item_id = NEW.item_id;
    INSERT INTO chat_communication_search_fts (
        message_item_id, conversation_id, reply_thread_id,
        author_display_name, normalized_markdown
    )
    SELECT
        NEW.item_id,
        item.conversation_id,
        item.reply_thread_id,
        coalesce(participant.display_name, NEW.author_label_snapshot),
        revision.normalized_markdown
    FROM chat_conversation_items item
    LEFT JOIN chat_participants participant ON participant.id = NEW.author_participant_id
    JOIN chat_communication_message_revisions revision
      ON revision.id = NEW.current_revision_id
    WHERE item.id = NEW.item_id;
END;

-- Revisions are immutable and bounded so one always fits a sync operation: the Markdown to
-- 131,072 bytes and the rich content to 196,608 bytes.
DROP TABLE chat_communication_message_revision_ordinals;

CREATE TABLE chat_communication_message_revisions_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    normalized_markdown TEXT NOT NULL CHECK (
        length(CAST(normalized_markdown AS BLOB)) <= 131072
    ),
    rich_content_schema_version INTEGER NOT NULL DEFAULT 1 CHECK (rich_content_schema_version >= 1),
    rich_content_data TEXT NOT NULL DEFAULT '{"type":"doc","content":[]}' CHECK (
        length(CAST(rich_content_data AS BLOB)) <= 196608
        AND json_valid(rich_content_data) AND json_type(rich_content_data) = 'object'
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    UNIQUE (id, message_item_id)
) STRICT;

INSERT INTO chat_communication_message_revisions_next (
    id, message_item_id, normalized_markdown, rich_content_schema_version, rich_content_data,
    created_at
)
SELECT id, message_item_id, normalized_markdown, rich_content_schema_version, rich_content_data,
    created_at
FROM chat_communication_message_revisions;

DROP TABLE chat_communication_message_revisions;
ALTER TABLE chat_communication_message_revisions_next RENAME TO chat_communication_message_revisions;

CREATE TRIGGER chat_communication_message_revision_immutable_update
BEFORE UPDATE ON chat_communication_message_revisions
BEGIN
    SELECT RAISE(ABORT, 'Communication message revisions are immutable');
END;

CREATE VIEW chat_communication_message_revisions_numbered AS
SELECT
    revision.id,
    revision.message_item_id,
    revision.normalized_markdown,
    revision.rich_content_schema_version,
    revision.rich_content_data,
    revision.created_at,
    (
        SELECT count(*) FROM chat_communication_message_revisions earlier
        WHERE earlier.message_item_id = revision.message_item_id
          AND (earlier.created_at, earlier.id) <= (revision.created_at, revision.id)
    ) AS revision
FROM chat_communication_message_revisions revision;

-- References are immutable; commands keep spans and ordinals distinct within a revision.
CREATE TABLE chat_message_references_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    reference_kind TEXT NOT NULL CHECK (
        reference_kind IN (
            'participant', 'channel', 'working_folder',
            'workspace_path', 'execution_environment'
        )
    ),
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    plain_text_projection TEXT NOT NULL CHECK (length(plain_text_projection) BETWEEN 1 AND 4096),
    start_offset INTEGER NOT NULL CHECK (start_offset >= 0),
    end_offset INTEGER NOT NULL CHECK (end_offset > start_offset),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;

INSERT INTO chat_message_references_next (
    id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
    start_offset, end_offset, ordinal, created_at
)
SELECT id, message_revision_id, reference_kind, label_snapshot, plain_text_projection,
    start_offset, end_offset, ordinal, created_at
FROM chat_message_references;

DROP TABLE chat_message_references;
ALTER TABLE chat_message_references_next RENAME TO chat_message_references;

CREATE INDEX idx_chat_message_references_revision
ON chat_message_references(message_revision_id, ordinal, id);

-- Mention targets keep their values without foreign keys; a missing target renders the label
-- snapshot. Channel reference ranges hold `(order_key, item_id)` bounds, all null for an empty
-- range, and the cutoff revision compares through its `(created_at, id)`.
CREATE TABLE chat_channel_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    channel_id TEXT NOT NULL CHECK (length(channel_id) BETWEEN 1 AND 1024),
    source_conversation_id TEXT NOT NULL CHECK (length(source_conversation_id) BETWEEN 1 AND 1024),
    destination_conversation_id TEXT NOT NULL CHECK (
        length(destination_conversation_id) BETWEEN 1 AND 1024
    ),
    source_lower_order_key TEXT CHECK (
        source_lower_order_key IS NULL OR length(source_lower_order_key) BETWEEN 1 AND 128
    ),
    source_lower_item_id TEXT CHECK (
        source_lower_item_id IS NULL OR length(source_lower_item_id) <= 1024
    ),
    source_high_order_key TEXT CHECK (
        source_high_order_key IS NULL OR length(source_high_order_key) BETWEEN 1 AND 128
    ),
    source_high_item_id TEXT CHECK (
        source_high_item_id IS NULL OR length(source_high_item_id) BETWEEN 1 AND 1024
    ),
    source_revision_cutoff_id TEXT CHECK (
        source_revision_cutoff_id IS NULL OR length(source_revision_cutoff_id) BETWEEN 1 AND 1024
    ),
    CHECK (
        (source_lower_order_key IS NULL) = (source_lower_item_id IS NULL)
        AND (source_lower_order_key IS NULL) = (source_high_order_key IS NULL)
        AND (source_lower_order_key IS NULL) = (source_high_item_id IS NULL)
        AND (source_lower_order_key IS NULL) = (source_revision_cutoff_id IS NULL)
    )
) STRICT;

INSERT INTO chat_channel_reference_targets_next (
    reference_id, channel_id, source_conversation_id, destination_conversation_id,
    source_lower_order_key, source_lower_item_id, source_high_order_key, source_high_item_id,
    source_revision_cutoff_id
)
SELECT
    target.reference_id,
    target.channel_id,
    target.source_conversation_id,
    target.destination_conversation_id,
    CASE WHEN high.id IS NULL THEN NULL ELSE lower.order_key END,
    CASE WHEN high.id IS NULL THEN NULL ELSE '' END,
    high.order_key,
    high.id,
    CASE WHEN high.id IS NULL THEN NULL ELSE target.source_revision_cutoff_id END
FROM chat_channel_reference_targets target
LEFT JOIN chat_timeline_root_boundaries lower
  ON lower.conversation_id = target.source_conversation_id
 AND lower.ordinal = target.source_lower_ordinal
LEFT JOIN chat_timeline_item_keys high ON high.id = (
    SELECT item.id FROM chat_timeline_item_keys item
    WHERE target.source_high_ordinal > 0
      AND target.source_revision_cutoff_id IS NOT NULL
      AND item.conversation_id = target.source_conversation_id
      AND item.reply_thread_id IS NULL
      AND item.ordinal <= target.source_high_ordinal
    ORDER BY item.ordinal DESC
    LIMIT 1
);

DROP TABLE chat_channel_reference_targets;
ALTER TABLE chat_channel_reference_targets_next RENAME TO chat_channel_reference_targets;

CREATE TABLE chat_execution_environment_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    execution_environment_id TEXT NOT NULL CHECK (
        length(execution_environment_id) BETWEEN 1 AND 2048
    )
) STRICT;

INSERT INTO chat_execution_environment_reference_targets_next (
    reference_id, execution_environment_id
)
SELECT reference_id, execution_environment_id FROM chat_execution_environment_reference_targets;

DROP TABLE chat_execution_environment_reference_targets;
ALTER TABLE chat_execution_environment_reference_targets_next
RENAME TO chat_execution_environment_reference_targets;

CREATE TABLE chat_participant_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL CHECK (length(participant_id) BETWEEN 1 AND 1024)
) STRICT;

INSERT INTO chat_participant_reference_targets_next (reference_id, participant_id)
SELECT reference_id, participant_id FROM chat_participant_reference_targets;

DROP TABLE chat_participant_reference_targets;
ALTER TABLE chat_participant_reference_targets_next RENAME TO chat_participant_reference_targets;

CREATE TABLE chat_working_folder_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL CHECK (length(working_folder_id) BETWEEN 1 AND 1024)
) STRICT;

INSERT INTO chat_working_folder_reference_targets_next (reference_id, working_folder_id)
SELECT reference_id, working_folder_id FROM chat_working_folder_reference_targets;

DROP TABLE chat_working_folder_reference_targets;
ALTER TABLE chat_working_folder_reference_targets_next
RENAME TO chat_working_folder_reference_targets;

CREATE TABLE chat_workspace_path_reference_targets_next (
    reference_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    working_folder_id TEXT NOT NULL CHECK (length(working_folder_id) BETWEEN 1 AND 1024),
    path_kind TEXT NOT NULL CHECK (path_kind IN ('file', 'folder')),
    relative_path TEXT NOT NULL CHECK (
        length(relative_path) BETWEEN 1 AND 4096
        AND relative_path NOT LIKE '/%'
        AND relative_path NOT LIKE '../%'
        AND relative_path NOT LIKE '%/../%'
        AND relative_path NOT LIKE '%/..'
        AND relative_path NOT LIKE '%\%'
    )
) STRICT;

INSERT INTO chat_workspace_path_reference_targets_next (
    reference_id, working_folder_id, path_kind, relative_path
)
SELECT reference_id, working_folder_id, path_kind, relative_path
FROM chat_workspace_path_reference_targets;

DROP TABLE chat_workspace_path_reference_targets;
ALTER TABLE chat_workspace_path_reference_targets_next
RENAME TO chat_workspace_path_reference_targets;

-- Read cursors hold the last read item, or '' for both columns when nothing is read. The
-- `sync_key` placeholder is rewritten by the Chat conversion pass.
CREATE TABLE chat_conversation_read_cursors_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    last_read_order_key TEXT NOT NULL DEFAULT '' CHECK (length(last_read_order_key) <= 128),
    last_read_item_id TEXT NOT NULL DEFAULT '' CHECK (length(last_read_item_id) <= 1024),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (conversation_id, participant_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_conversation_read_cursors_next (
    conversation_id, participant_id, sync_key, last_read_order_key, last_read_item_id, updated_at
)
SELECT
    cursor.conversation_id,
    cursor.participant_id,
    'pending:' || ROW_NUMBER() OVER (ORDER BY cursor.conversation_id, cursor.participant_id),
    coalesce(last_read.order_key, ''),
    coalesce(last_read.id, ''),
    cursor.updated_at
FROM chat_conversation_read_cursors cursor
LEFT JOIN chat_timeline_item_keys last_read ON last_read.id = (
    SELECT item.id FROM chat_timeline_item_keys item
    WHERE item.conversation_id = cursor.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.ordinal <= cursor.last_read_root_ordinal
    ORDER BY item.ordinal DESC
    LIMIT 1
);

DROP TABLE chat_conversation_read_cursors;
ALTER TABLE chat_conversation_read_cursors_next RENAME TO chat_conversation_read_cursors;

CREATE TABLE chat_reply_thread_read_cursors_next (
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    participant_id TEXT NOT NULL REFERENCES chat_participants(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    last_read_order_key TEXT NOT NULL DEFAULT '' CHECK (length(last_read_order_key) <= 128),
    last_read_item_id TEXT NOT NULL DEFAULT '' CHECK (length(last_read_item_id) <= 1024),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    PRIMARY KEY (reply_thread_id, participant_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_reply_thread_read_cursors_next (
    reply_thread_id, participant_id, sync_key, last_read_order_key, last_read_item_id, updated_at
)
SELECT
    cursor.reply_thread_id,
    cursor.participant_id,
    'pending:' || ROW_NUMBER() OVER (ORDER BY cursor.reply_thread_id, cursor.participant_id),
    coalesce(last_read.order_key, ''),
    coalesce(last_read.id, ''),
    cursor.updated_at
FROM chat_reply_thread_read_cursors cursor
LEFT JOIN chat_timeline_item_keys last_read ON last_read.id = (
    SELECT item.id FROM chat_timeline_item_keys item
    WHERE item.reply_thread_id = cursor.reply_thread_id
      AND item.ordinal <= cursor.last_read_reply_ordinal
    ORDER BY item.ordinal DESC
    LIMIT 1
);

DROP TABLE chat_reply_thread_read_cursors;
ALTER TABLE chat_reply_thread_read_cursors_next RENAME TO chat_reply_thread_read_cursors;

-- AI channel history granted from a point starts at an order key; an item is visible when its
-- key is at or after it.
CREATE TABLE chat_teammate_channel_memberships_next (
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
    access_profile_id TEXT NOT NULL REFERENCES chat_access_profiles(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    read_history INTEGER NOT NULL DEFAULT 0 CHECK (read_history IN (0, 1)),
    read_history_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        read_history_inherits_profile IN (0, 1)
    ),
    participate INTEGER NOT NULL DEFAULT 0 CHECK (participate IN (0, 1)),
    participate_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        participate_inherits_profile IN (0, 1)
    ),
    history_boundary TEXT NOT NULL DEFAULT 'entire' CHECK (
        history_boundary IN ('entire', 'from_grant')
    ),
    history_boundary_inherits_profile INTEGER NOT NULL DEFAULT 0 CHECK (
        history_boundary_inherits_profile IN (0, 1)
    ),
    history_from_order_key TEXT CHECK (
        history_from_order_key IS NULL OR length(history_from_order_key) BETWEEN 1 AND 128
    ),
    runtime_approval_policy TEXT CHECK (
        runtime_approval_policy IS NULL OR runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    scratch_runtime_approval_policy TEXT CHECK (
        scratch_runtime_approval_policy IS NULL OR scratch_runtime_approval_policy IN (
            'ask', 'auto_approve', 'unattended', 'provider_custom'
        )
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
    removed_at TEXT CHECK (removed_at IS NULL OR length(removed_at) >= 20),
    PRIMARY KEY (conversation_id, teammate_id),
    CHECK (
        (history_boundary = 'entire' AND history_from_order_key IS NULL)
        OR (history_boundary = 'from_grant' AND history_from_order_key IS NOT NULL)
    )
) STRICT, WITHOUT ROWID;

INSERT INTO chat_teammate_channel_memberships_next (
    conversation_id, teammate_id, sync_key, access_profile_id, read_history,
    read_history_inherits_profile, participate, participate_inherits_profile, history_boundary,
    history_boundary_inherits_profile, history_from_order_key, runtime_approval_policy,
    scratch_runtime_approval_policy, created_at, updated_at, removed_at
)
SELECT
    membership.conversation_id, membership.teammate_id, membership.sync_key,
    membership.access_profile_id, membership.read_history,
    membership.read_history_inherits_profile, membership.participate,
    membership.participate_inherits_profile, membership.history_boundary,
    membership.history_boundary_inherits_profile, boundary.order_key,
    membership.runtime_approval_policy, membership.scratch_runtime_approval_policy,
    membership.created_at, membership.updated_at, membership.removed_at
FROM chat_teammate_channel_memberships membership
LEFT JOIN chat_timeline_root_boundaries boundary
  ON boundary.conversation_id = membership.conversation_id
 AND boundary.ordinal = membership.history_from_ordinal;

DROP TABLE chat_teammate_channel_memberships;
ALTER TABLE chat_teammate_channel_memberships_next RENAME TO chat_teammate_channel_memberships;

CREATE INDEX idx_chat_teammate_channel_memberships_teammate
ON chat_teammate_channel_memberships(teammate_id, conversation_id);

CREATE TRIGGER chat_teammate_channel_membership_audience_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE conversation_id = OLD.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_insert
AFTER INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id = NEW.conversation_id;
END;

CREATE TRIGGER chat_teammate_channel_membership_audience_update
AFTER UPDATE OF conversation_id, teammate_id, access_profile_id, read_history,
    read_history_inherits_profile, participate, participate_inherits_profile, history_boundary,
    history_boundary_inherits_profile, history_from_order_key, runtime_approval_policy,
    scratch_runtime_approval_policy, created_at, updated_at, removed_at
ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_conversation_audience_state
    SET revision = revision + 1,
        updated_at = NEW.updated_at
    WHERE conversation_id IN (OLD.conversation_id, NEW.conversation_id);
END;

CREATE TRIGGER chat_teammate_channel_membership_identity_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN NOT EXISTS (
    SELECT 1 FROM chat_teammates teammate
    WHERE teammate.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires an AI teammate');
END;

CREATE TRIGGER chat_teammate_channel_membership_requires_membership_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND NOT EXISTS (
    SELECT 1 FROM chat_conversation_memberships membership
    WHERE membership.conversation_id = NEW.conversation_id
      AND membership.participant_id = NEW.teammate_id
)
BEGIN
    SELECT RAISE(ABORT, 'AI channel access requires a conversation membership');
END;

-- Local authorized channel and scratch sources hold `(order_key, item_id)` bounds. The high
-- bound is ('', '') when no root item was in range.
CREATE TABLE chat_assignment_authorized_channel_sources_next (
    authorization_revision_id TEXT NOT NULL REFERENCES chat_assignment_authorization_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    source_handle TEXT NOT NULL UNIQUE CHECK (length(source_handle) BETWEEN 32 AND 1024),
    message_reference_id TEXT NOT NULL REFERENCES chat_message_references(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    label_snapshot TEXT NOT NULL CHECK (length(label_snapshot) BETWEEN 1 AND 4096),
    lower_order_key TEXT NOT NULL CHECK (length(lower_order_key) BETWEEN 1 AND 128),
    lower_item_id TEXT NOT NULL CHECK (length(lower_item_id) <= 1024),
    high_order_key TEXT NOT NULL CHECK (length(high_order_key) <= 128),
    high_item_id TEXT NOT NULL CHECK (length(high_item_id) <= 1024),
    source_revision_cutoff_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    destination_audience_revision INTEGER NOT NULL CHECK (destination_audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (authorization_revision_id, conversation_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_assignment_authorized_channel_sources_next (
    authorization_revision_id, source_handle, message_reference_id, conversation_id,
    label_snapshot, lower_order_key, lower_item_id, high_order_key, high_item_id,
    source_revision_cutoff_id, destination_audience_revision, created_at
)
SELECT
    source.authorization_revision_id, source.source_handle, source.message_reference_id,
    source.conversation_id, source.label_snapshot, lower.order_key, '',
    coalesce(high.order_key, ''), coalesce(high.id, ''), source.source_revision_cutoff_id,
    source.destination_audience_revision, source.created_at
FROM chat_assignment_authorized_channel_sources source
JOIN chat_timeline_root_boundaries lower
  ON lower.conversation_id = source.conversation_id
 AND lower.ordinal = source.lower_ordinal
LEFT JOIN chat_timeline_item_keys high ON high.id = (
    SELECT item.id FROM chat_timeline_item_keys item
    WHERE item.conversation_id = source.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.ordinal <= source.high_ordinal
    ORDER BY item.ordinal DESC
    LIMIT 1
);

DROP TABLE chat_assignment_authorized_channel_sources;
ALTER TABLE chat_assignment_authorized_channel_sources_next
RENAME TO chat_assignment_authorized_channel_sources;

CREATE TRIGGER chat_authorized_channel_source_immutable_delete
BEFORE DELETE ON chat_assignment_authorized_channel_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (
    SELECT 1 FROM chat_assignment_authorization_revisions
    WHERE id = OLD.authorization_revision_id
)
 AND EXISTS (SELECT 1 FROM chat_message_references WHERE id = OLD.message_reference_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id)
 AND EXISTS (
    SELECT 1 FROM chat_communication_message_revisions
    WHERE id = OLD.source_revision_cutoff_id
)
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

CREATE TRIGGER chat_authorized_channel_source_immutable_update
BEFORE UPDATE ON chat_assignment_authorized_channel_sources
BEGIN
    SELECT RAISE(ABORT, 'Authorized channel sources are immutable');
END;

CREATE TABLE chat_scratch_generation_sources_next (
    scratch_generation_id TEXT NOT NULL REFERENCES chat_scratch_generations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    lower_order_key TEXT NOT NULL CHECK (length(lower_order_key) BETWEEN 1 AND 128),
    lower_item_id TEXT NOT NULL CHECK (length(lower_item_id) <= 1024),
    high_order_key TEXT NOT NULL CHECK (length(high_order_key) <= 128),
    high_item_id TEXT NOT NULL CHECK (length(high_item_id) <= 1024),
    audience_revision INTEGER NOT NULL CHECK (audience_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    PRIMARY KEY (scratch_generation_id, conversation_id)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_scratch_generation_sources_next (
    scratch_generation_id, conversation_id, lower_order_key, lower_item_id, high_order_key,
    high_item_id, audience_revision, created_at
)
SELECT
    source.scratch_generation_id, source.conversation_id, lower.order_key, '',
    coalesce(high.order_key, ''), coalesce(high.id, ''), source.audience_revision,
    source.created_at
FROM chat_scratch_generation_sources source
JOIN chat_timeline_root_boundaries lower
  ON lower.conversation_id = source.conversation_id
 AND lower.ordinal = source.lower_ordinal
LEFT JOIN chat_timeline_item_keys high ON high.id = (
    SELECT item.id FROM chat_timeline_item_keys item
    WHERE item.conversation_id = source.conversation_id
      AND item.reply_thread_id IS NULL
      AND item.ordinal <= source.high_ordinal
    ORDER BY item.ordinal DESC
    LIMIT 1
);

DROP TABLE chat_scratch_generation_sources;
ALTER TABLE chat_scratch_generation_sources_next RENAME TO chat_scratch_generation_sources;

CREATE TRIGGER chat_scratch_generation_source_delete_denied
BEFORE DELETE ON chat_scratch_generation_sources
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND EXISTS (SELECT 1 FROM chat_scratch_generations WHERE id = OLD.scratch_generation_id)
 AND EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id)
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints are immutable');
END;

CREATE TRIGGER chat_scratch_generation_source_monotonic
BEFORE UPDATE ON chat_scratch_generation_sources
WHEN NEW.scratch_generation_id IS NOT OLD.scratch_generation_id
  OR NEW.conversation_id IS NOT OLD.conversation_id
  OR (NEW.lower_order_key, NEW.lower_item_id) > (OLD.lower_order_key, OLD.lower_item_id)
  OR (NEW.high_order_key, NEW.high_item_id) < (OLD.high_order_key, OLD.high_item_id)
  OR NEW.audience_revision < OLD.audience_revision
  OR NEW.created_at IS NOT OLD.created_at
BEGIN
    SELECT RAISE(ABORT, 'Scratch source constraints can only expand');
END;

-- Work assignments and their inputs cascade from their messages.
CREATE TABLE chat_work_assignments_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    reply_thread_id TEXT NOT NULL REFERENCES chat_reply_threads(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    triggering_message_item_id TEXT NOT NULL REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    previous_assignment_id TEXT REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (
        state IN (
            'queued', 'working', 'waiting_for_answer', 'waiting_for_approval',
            'ready_for_review', 'completed', 'failed', 'cancelled'
        )
    ),
    state_reason TEXT CHECK (state_reason IS NULL OR length(state_reason) <= 4000),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    settled_at TEXT CHECK (settled_at IS NULL OR length(settled_at) >= 20),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

INSERT INTO chat_work_assignments_next (
    id, reply_thread_id, teammate_id, triggering_message_item_id, previous_assignment_id, state,
    state_reason, revision, settled_at, created_at, updated_at
)
SELECT id, reply_thread_id, teammate_id, triggering_message_item_id, previous_assignment_id,
    state, state_reason, revision, settled_at, created_at, updated_at
FROM chat_work_assignments;

DROP TABLE chat_work_assignments;
ALTER TABLE chat_work_assignments_next RENAME TO chat_work_assignments;

CREATE UNIQUE INDEX idx_chat_work_assignments_one_active
ON chat_work_assignments(reply_thread_id)
WHERE state IN (
    'queued', 'working', 'waiting_for_answer', 'waiting_for_approval', 'ready_for_review'
);

CREATE INDEX idx_chat_work_assignments_teammate
ON chat_work_assignments(teammate_id, state, updated_at DESC, id);

CREATE TABLE chat_work_assignment_inputs_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    assignment_id TEXT NOT NULL REFERENCES chat_work_assignments(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    message_item_id TEXT NOT NULL UNIQUE REFERENCES chat_communication_messages(item_id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 1),
    routing_kind TEXT NOT NULL CHECK (
        routing_kind IN ('trigger', 'steer', 'queued_continuation', 'follow_up')
    ),
    delivery_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        delivery_state IN ('pending', 'delivered', 'failed', 'ignored')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    delivered_at TEXT CHECK (delivered_at IS NULL OR length(delivered_at) >= 20),
    UNIQUE (assignment_id, ordinal)
) STRICT;

INSERT INTO chat_work_assignment_inputs_next (
    id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state, created_at,
    delivered_at
)
SELECT id, assignment_id, message_item_id, ordinal, routing_kind, delivery_state, created_at,
    delivered_at
FROM chat_work_assignment_inputs;

DROP TABLE chat_work_assignment_inputs;
ALTER TABLE chat_work_assignment_inputs_next RENAME TO chat_work_assignment_inputs;

DROP TABLE chat_timeline_root_boundaries;
DROP TABLE chat_timeline_item_keys;
DROP TABLE chat_timeline_rank_keys;
DROP TABLE chat_timeline_rank_limit;

CREATE TEMP TABLE chat_timeline_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_timeline_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_timeline_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_chat_teammate_channel_memberships_insert
BEFORE INSERT ON chat_teammate_channel_memberships
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_order_key IS NULL OR (typeof(NEW.history_from_order_key) = 'text' AND instr(CAST(NEW.history_from_order_key AS BLOB), x'00') = 0 AND length(NEW.history_from_order_key) <= 128 AND length(CAST(NEW.history_from_order_key AS BLOB)) <= 262144))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_teammate_channel_memberships_update
BEFORE UPDATE OF conversation_id, teammate_id, access_profile_id, read_history, read_history_inherits_profile, participate, participate_inherits_profile, history_boundary, history_boundary_inherits_profile, history_from_order_key, runtime_approval_policy, scratch_runtime_approval_policy, removed_at, created_at, updated_at ON chat_teammate_channel_memberships
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.access_profile_id IS NULL OR (typeof(NEW.access_profile_id) = 'text' AND instr(CAST(NEW.access_profile_id AS BLOB), x'00') = 0 AND trim(NEW.access_profile_id) <> '' AND length(CAST(NEW.access_profile_id AS BLOB)) <= 128))
    AND (typeof(NEW.read_history) = 'integer' AND NEW.read_history IN (0, 1))
    AND (typeof(NEW.read_history_inherits_profile) = 'integer' AND NEW.read_history_inherits_profile IN (0, 1))
    AND (typeof(NEW.participate) = 'integer' AND NEW.participate IN (0, 1))
    AND (typeof(NEW.participate_inherits_profile) = 'integer' AND NEW.participate_inherits_profile IN (0, 1))
    AND (typeof(NEW.history_boundary) = 'text' AND instr(CAST(NEW.history_boundary AS BLOB), x'00') = 0 AND length(NEW.history_boundary) <= 32 AND length(CAST(NEW.history_boundary AS BLOB)) <= 262144)
    AND (typeof(NEW.history_boundary_inherits_profile) = 'integer' AND NEW.history_boundary_inherits_profile IN (0, 1))
    AND (NEW.history_from_order_key IS NULL OR (typeof(NEW.history_from_order_key) = 'text' AND instr(CAST(NEW.history_from_order_key AS BLOB), x'00') = 0 AND length(NEW.history_from_order_key) <= 128 AND length(CAST(NEW.history_from_order_key AS BLOB)) <= 262144))
    AND (NEW.runtime_approval_policy IS NULL OR (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.scratch_runtime_approval_policy IS NULL OR (typeof(NEW.scratch_runtime_approval_policy) = 'text' AND instr(CAST(NEW.scratch_runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.scratch_runtime_approval_policy) <= 32 AND length(CAST(NEW.scratch_runtime_approval_policy AS BLOB)) <= 262144))
    AND (NEW.removed_at IS NULL OR (typeof(NEW.removed_at) = 'text' AND instr(CAST(NEW.removed_at AS BLOB), x'00') = 0 AND trim(NEW.removed_at) <> '' AND length(CAST(NEW.removed_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_teammate_channel_memberships
BEFORE UPDATE OF sync_key, conversation_id, teammate_id ON chat_teammate_channel_memberships
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_insert
AFTER INSERT ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_update
AFTER UPDATE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.access_profile_id IS NOT NEW.access_profile_id COLLATE BINARY
                AND NOT (NEW.access_profile_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_access_profiles WHERE id = OLD.access_profile_id))) OR OLD.read_history IS NOT NEW.read_history OR OLD.read_history_inherits_profile IS NOT NEW.read_history_inherits_profile OR OLD.participate IS NOT NEW.participate OR OLD.participate_inherits_profile IS NOT NEW.participate_inherits_profile OR OLD.history_boundary IS NOT NEW.history_boundary COLLATE BINARY OR OLD.history_boundary_inherits_profile IS NOT NEW.history_boundary_inherits_profile OR OLD.history_from_order_key IS NOT NEW.history_from_order_key COLLATE BINARY OR OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.scratch_runtime_approval_policy IS NOT NEW.scratch_runtime_approval_policy COLLATE BINARY OR OLD.removed_at IS NOT NEW.removed_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 362, NEW.sync_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_teammate_channel_memberships_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (362, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_reply_threads_insert
BEFORE INSERT ON chat_reply_threads
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.root_item_id IS NULL OR (typeof(NEW.root_item_id) = 'text' AND instr(CAST(NEW.root_item_id AS BLOB), x'00') = 0 AND trim(NEW.root_item_id) <> '' AND length(CAST(NEW.root_item_id AS BLOB)) <= 128))
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.last_activity_at) = 'text' AND instr(CAST(NEW.last_activity_at AS BLOB), x'00') = 0 AND trim(NEW.last_activity_at) <> '' AND length(CAST(NEW.last_activity_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_reply_threads_update
BEFORE UPDATE OF root_item_id, conversation_id, created_at, last_activity_at, updated_at ON chat_reply_threads
WHEN ((NEW.root_item_id IS NULL OR (typeof(NEW.root_item_id) = 'text' AND instr(CAST(NEW.root_item_id AS BLOB), x'00') = 0 AND trim(NEW.root_item_id) <> '' AND length(CAST(NEW.root_item_id AS BLOB)) <= 128))
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.last_activity_at) = 'text' AND instr(CAST(NEW.last_activity_at AS BLOB), x'00') = 0 AND trim(NEW.last_activity_at) <> '' AND length(CAST(NEW.last_activity_at AS BLOB)) <= 64)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_reply_threads
BEFORE UPDATE OF id, root_item_id ON chat_reply_threads
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.root_item_id IS NOT NEW.root_item_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_reply_threads_insert
AFTER INSERT ON chat_reply_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (370, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_reply_threads_update
AFTER UPDATE ON chat_reply_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 370, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY
                AND NOT (NEW.conversation_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_conversations WHERE id = OLD.conversation_id))) OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.last_activity_at IS NOT NEW.last_activity_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_reply_threads_delete
AFTER DELETE ON chat_reply_threads
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (370, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_conversation_items_insert
BEFORE INSERT ON chat_conversation_items
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (typeof(NEW.item_kind) = 'text' AND instr(CAST(NEW.item_kind AS BLOB), x'00') = 0 AND length(NEW.item_kind) <= 32 AND length(CAST(NEW.item_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_conversation_items_update
BEFORE UPDATE OF conversation_id, reply_thread_id, item_kind, order_key, created_at ON chat_conversation_items
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (typeof(NEW.item_kind) = 'text' AND instr(CAST(NEW.item_kind AS BLOB), x'00') = 0 AND length(NEW.item_kind) <= 32 AND length(CAST(NEW.item_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_conversation_items
BEFORE UPDATE OF id, conversation_id, reply_thread_id, item_kind, order_key, created_at ON chat_conversation_items
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.item_kind IS NOT NEW.item_kind COLLATE BINARY OR OLD.order_key IS NOT NEW.order_key COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_conversation_items
BEFORE INSERT ON chat_conversation_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 371 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_conversation_items WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_conversation_items_insert
AFTER INSERT ON chat_conversation_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (371, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversation_items_update
AFTER UPDATE ON chat_conversation_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 371, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_conversation_items_delete
AFTER DELETE ON chat_conversation_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (371, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_communication_messages_insert
BEFORE INSERT ON chat_communication_messages
WHEN ((typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128)
    AND (typeof(NEW.author_label_snapshot) = 'text' AND instr(CAST(NEW.author_label_snapshot AS BLOB), x'00') = 0 AND length(trim(NEW.author_label_snapshot)) BETWEEN 1 AND 160 AND length(CAST(NEW.author_label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.author_participant_id IS NULL OR (typeof(NEW.author_participant_id) = 'text' AND instr(CAST(NEW.author_participant_id AS BLOB), x'00') = 0 AND trim(NEW.author_participant_id) <> '' AND length(CAST(NEW.author_participant_id AS BLOB)) <= 128))
    AND (NEW.current_revision_id IS NULL OR (typeof(NEW.current_revision_id) = 'text' AND instr(CAST(NEW.current_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_revision_id) <> '' AND length(CAST(NEW.current_revision_id AS BLOB)) <= 128))
    AND (NEW.edited_at IS NULL OR (typeof(NEW.edited_at) = 'text' AND instr(CAST(NEW.edited_at AS BLOB), x'00') = 0 AND trim(NEW.edited_at) <> '' AND length(CAST(NEW.edited_at AS BLOB)) <= 64))
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_communication_messages_update
BEFORE UPDATE OF author_label_snapshot, created_at, author_participant_id, current_revision_id, edited_at, deleted_at ON chat_communication_messages
WHEN ((typeof(NEW.author_label_snapshot) = 'text' AND instr(CAST(NEW.author_label_snapshot AS BLOB), x'00') = 0 AND length(trim(NEW.author_label_snapshot)) BETWEEN 1 AND 160 AND length(CAST(NEW.author_label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.author_participant_id IS NULL OR (typeof(NEW.author_participant_id) = 'text' AND instr(CAST(NEW.author_participant_id AS BLOB), x'00') = 0 AND trim(NEW.author_participant_id) <> '' AND length(CAST(NEW.author_participant_id AS BLOB)) <= 128))
    AND (NEW.current_revision_id IS NULL OR (typeof(NEW.current_revision_id) = 'text' AND instr(CAST(NEW.current_revision_id AS BLOB), x'00') = 0 AND trim(NEW.current_revision_id) <> '' AND length(CAST(NEW.current_revision_id AS BLOB)) <= 128))
    AND (NEW.edited_at IS NULL OR (typeof(NEW.edited_at) = 'text' AND instr(CAST(NEW.edited_at AS BLOB), x'00') = 0 AND trim(NEW.edited_at) <> '' AND length(CAST(NEW.edited_at AS BLOB)) <= 64))
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_communication_messages
BEFORE UPDATE OF item_id, author_label_snapshot, created_at ON chat_communication_messages
WHEN OLD.item_id IS NOT NEW.item_id COLLATE BINARY
    OR ((OLD.author_label_snapshot IS NOT NEW.author_label_snapshot COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_communication_messages
BEFORE INSERT ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 372 AND row_key = NEW.item_id)
    AND NOT EXISTS (SELECT 1 FROM chat_communication_messages WHERE item_id = NEW.item_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_communication_messages_insert
AFTER INSERT ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (372, NEW.item_id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_communication_messages_update
AFTER UPDATE ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 372, NEW.item_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.author_participant_id IS NOT NEW.author_participant_id COLLATE BINARY
                AND NOT (NEW.author_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.author_participant_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.current_revision_id IS NOT NEW.current_revision_id COLLATE BINARY OR OLD.edited_at IS NOT NEW.edited_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_communication_messages_delete
AFTER DELETE ON chat_communication_messages
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (372, OLD.item_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_communication_message_revisions_insert
BEFORE INSERT ON chat_communication_message_revisions
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.normalized_markdown) = 'text' AND instr(CAST(NEW.normalized_markdown AS BLOB), x'00') = 0 AND length(NEW.normalized_markdown) <= 131072 AND length(CAST(NEW.normalized_markdown AS BLOB)) <= 262144)
    AND (typeof(NEW.rich_content_schema_version) = 'integer' AND NEW.rich_content_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.rich_content_data) = 'text' AND instr(CAST(NEW.rich_content_data AS BLOB), x'00') = 0 AND length(NEW.rich_content_data) <= 196608 AND length(CAST(NEW.rich_content_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_communication_message_revisions_update
BEFORE UPDATE OF message_item_id, normalized_markdown, rich_content_schema_version, rich_content_data, created_at ON chat_communication_message_revisions
WHEN ((NEW.message_item_id IS NULL OR (typeof(NEW.message_item_id) = 'text' AND instr(CAST(NEW.message_item_id AS BLOB), x'00') = 0 AND trim(NEW.message_item_id) <> '' AND length(CAST(NEW.message_item_id AS BLOB)) <= 128))
    AND (typeof(NEW.normalized_markdown) = 'text' AND instr(CAST(NEW.normalized_markdown AS BLOB), x'00') = 0 AND length(NEW.normalized_markdown) <= 131072 AND length(CAST(NEW.normalized_markdown AS BLOB)) <= 262144)
    AND (typeof(NEW.rich_content_schema_version) = 'integer' AND NEW.rich_content_schema_version BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.rich_content_data) = 'text' AND instr(CAST(NEW.rich_content_data AS BLOB), x'00') = 0 AND length(NEW.rich_content_data) <= 196608 AND length(CAST(NEW.rich_content_data AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_communication_message_revisions
BEFORE UPDATE OF id, message_item_id, normalized_markdown, rich_content_schema_version, rich_content_data, created_at ON chat_communication_message_revisions
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.message_item_id IS NOT NEW.message_item_id COLLATE BINARY OR OLD.normalized_markdown IS NOT NEW.normalized_markdown COLLATE BINARY OR OLD.rich_content_schema_version IS NOT NEW.rich_content_schema_version OR OLD.rich_content_data IS NOT NEW.rich_content_data COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_communication_message_revisions
BEFORE INSERT ON chat_communication_message_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 373 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_communication_message_revisions WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_communication_message_revisions_insert
AFTER INSERT ON chat_communication_message_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (373, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_communication_message_revisions_update
AFTER UPDATE ON chat_communication_message_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 373, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_communication_message_revisions_delete
AFTER DELETE ON chat_communication_message_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (373, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_message_references_insert
BEFORE INSERT ON chat_message_references
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.message_revision_id IS NULL OR (typeof(NEW.message_revision_id) = 'text' AND instr(CAST(NEW.message_revision_id AS BLOB), x'00') = 0 AND trim(NEW.message_revision_id) <> '' AND length(CAST(NEW.message_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.reference_kind) = 'text' AND instr(CAST(NEW.reference_kind AS BLOB), x'00') = 0 AND length(NEW.reference_kind) <= 32 AND length(CAST(NEW.reference_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.label_snapshot) = 'text' AND instr(CAST(NEW.label_snapshot AS BLOB), x'00') = 0 AND length(NEW.label_snapshot) <= 4096 AND length(CAST(NEW.label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text_projection) = 'text' AND instr(CAST(NEW.plain_text_projection AS BLOB), x'00') = 0 AND length(NEW.plain_text_projection) <= 4096 AND length(CAST(NEW.plain_text_projection AS BLOB)) <= 262144)
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_message_references_update
BEFORE UPDATE OF message_revision_id, reference_kind, label_snapshot, plain_text_projection, start_offset, end_offset, ordinal, created_at ON chat_message_references
WHEN ((NEW.message_revision_id IS NULL OR (typeof(NEW.message_revision_id) = 'text' AND instr(CAST(NEW.message_revision_id AS BLOB), x'00') = 0 AND trim(NEW.message_revision_id) <> '' AND length(CAST(NEW.message_revision_id AS BLOB)) <= 128))
    AND (typeof(NEW.reference_kind) = 'text' AND instr(CAST(NEW.reference_kind AS BLOB), x'00') = 0 AND length(NEW.reference_kind) <= 32 AND length(CAST(NEW.reference_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.label_snapshot) = 'text' AND instr(CAST(NEW.label_snapshot AS BLOB), x'00') = 0 AND length(NEW.label_snapshot) <= 4096 AND length(CAST(NEW.label_snapshot AS BLOB)) <= 262144)
    AND (typeof(NEW.plain_text_projection) = 'text' AND instr(CAST(NEW.plain_text_projection AS BLOB), x'00') = 0 AND length(NEW.plain_text_projection) <= 4096 AND length(CAST(NEW.plain_text_projection AS BLOB)) <= 262144)
    AND (typeof(NEW.start_offset) = 'integer' AND NEW.start_offset BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_offset) = 'integer' AND NEW.end_offset BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.ordinal) = 'integer' AND NEW.ordinal BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_message_references
BEFORE UPDATE OF id, message_revision_id, reference_kind, label_snapshot, plain_text_projection, start_offset, end_offset, ordinal, created_at ON chat_message_references
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.message_revision_id IS NOT NEW.message_revision_id COLLATE BINARY OR OLD.reference_kind IS NOT NEW.reference_kind COLLATE BINARY OR OLD.label_snapshot IS NOT NEW.label_snapshot COLLATE BINARY OR OLD.plain_text_projection IS NOT NEW.plain_text_projection COLLATE BINARY OR OLD.start_offset IS NOT NEW.start_offset OR OLD.end_offset IS NOT NEW.end_offset OR OLD.ordinal IS NOT NEW.ordinal OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_message_references
BEFORE INSERT ON chat_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 374 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_message_references WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_message_references_insert
AFTER INSERT ON chat_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (374, NEW.id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_message_references_update
AFTER UPDATE ON chat_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 374, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_message_references_delete
AFTER DELETE ON chat_message_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (374, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_channel_reference_targets_insert
BEFORE INSERT ON chat_channel_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.source_conversation_id IS NULL OR (typeof(NEW.source_conversation_id) = 'text' AND instr(CAST(NEW.source_conversation_id AS BLOB), x'00') = 0 AND trim(NEW.source_conversation_id) <> '' AND length(CAST(NEW.source_conversation_id AS BLOB)) <= 128))
    AND (NEW.destination_conversation_id IS NULL OR (typeof(NEW.destination_conversation_id) = 'text' AND instr(CAST(NEW.destination_conversation_id AS BLOB), x'00') = 0 AND trim(NEW.destination_conversation_id) <> '' AND length(CAST(NEW.destination_conversation_id AS BLOB)) <= 128))
    AND (NEW.source_lower_order_key IS NULL OR (typeof(NEW.source_lower_order_key) = 'text' AND instr(CAST(NEW.source_lower_order_key AS BLOB), x'00') = 0 AND length(NEW.source_lower_order_key) <= 128 AND length(CAST(NEW.source_lower_order_key AS BLOB)) <= 262144))
    AND (NEW.source_lower_item_id IS NULL OR (typeof(NEW.source_lower_item_id) = 'text' AND instr(CAST(NEW.source_lower_item_id AS BLOB), x'00') = 0 AND length(NEW.source_lower_item_id) <= 1024 AND length(CAST(NEW.source_lower_item_id AS BLOB)) <= 262144))
    AND (NEW.source_high_order_key IS NULL OR (typeof(NEW.source_high_order_key) = 'text' AND instr(CAST(NEW.source_high_order_key AS BLOB), x'00') = 0 AND length(NEW.source_high_order_key) <= 128 AND length(CAST(NEW.source_high_order_key AS BLOB)) <= 262144))
    AND (NEW.source_high_item_id IS NULL OR (typeof(NEW.source_high_item_id) = 'text' AND instr(CAST(NEW.source_high_item_id AS BLOB), x'00') = 0 AND trim(NEW.source_high_item_id) <> '' AND length(CAST(NEW.source_high_item_id AS BLOB)) <= 128))
    AND (NEW.source_revision_cutoff_id IS NULL OR (typeof(NEW.source_revision_cutoff_id) = 'text' AND instr(CAST(NEW.source_revision_cutoff_id AS BLOB), x'00') = 0 AND trim(NEW.source_revision_cutoff_id) <> '' AND length(CAST(NEW.source_revision_cutoff_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_channel_reference_targets_update
BEFORE UPDATE OF channel_id, source_conversation_id, destination_conversation_id, source_lower_order_key, source_lower_item_id, source_high_order_key, source_high_item_id, source_revision_cutoff_id ON chat_channel_reference_targets
WHEN ((NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (NEW.source_conversation_id IS NULL OR (typeof(NEW.source_conversation_id) = 'text' AND instr(CAST(NEW.source_conversation_id AS BLOB), x'00') = 0 AND trim(NEW.source_conversation_id) <> '' AND length(CAST(NEW.source_conversation_id AS BLOB)) <= 128))
    AND (NEW.destination_conversation_id IS NULL OR (typeof(NEW.destination_conversation_id) = 'text' AND instr(CAST(NEW.destination_conversation_id AS BLOB), x'00') = 0 AND trim(NEW.destination_conversation_id) <> '' AND length(CAST(NEW.destination_conversation_id AS BLOB)) <= 128))
    AND (NEW.source_lower_order_key IS NULL OR (typeof(NEW.source_lower_order_key) = 'text' AND instr(CAST(NEW.source_lower_order_key AS BLOB), x'00') = 0 AND length(NEW.source_lower_order_key) <= 128 AND length(CAST(NEW.source_lower_order_key AS BLOB)) <= 262144))
    AND (NEW.source_lower_item_id IS NULL OR (typeof(NEW.source_lower_item_id) = 'text' AND instr(CAST(NEW.source_lower_item_id AS BLOB), x'00') = 0 AND length(NEW.source_lower_item_id) <= 1024 AND length(CAST(NEW.source_lower_item_id AS BLOB)) <= 262144))
    AND (NEW.source_high_order_key IS NULL OR (typeof(NEW.source_high_order_key) = 'text' AND instr(CAST(NEW.source_high_order_key AS BLOB), x'00') = 0 AND length(NEW.source_high_order_key) <= 128 AND length(CAST(NEW.source_high_order_key AS BLOB)) <= 262144))
    AND (NEW.source_high_item_id IS NULL OR (typeof(NEW.source_high_item_id) = 'text' AND instr(CAST(NEW.source_high_item_id AS BLOB), x'00') = 0 AND trim(NEW.source_high_item_id) <> '' AND length(CAST(NEW.source_high_item_id AS BLOB)) <= 128))
    AND (NEW.source_revision_cutoff_id IS NULL OR (typeof(NEW.source_revision_cutoff_id) = 'text' AND instr(CAST(NEW.source_revision_cutoff_id AS BLOB), x'00') = 0 AND trim(NEW.source_revision_cutoff_id) <> '' AND length(CAST(NEW.source_revision_cutoff_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_channel_reference_targets
BEFORE UPDATE OF reference_id, channel_id, source_conversation_id, destination_conversation_id, source_lower_order_key, source_lower_item_id, source_high_order_key, source_high_item_id, source_revision_cutoff_id ON chat_channel_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.channel_id IS NOT NEW.channel_id COLLATE BINARY OR OLD.source_conversation_id IS NOT NEW.source_conversation_id COLLATE BINARY OR OLD.destination_conversation_id IS NOT NEW.destination_conversation_id COLLATE BINARY OR OLD.source_lower_order_key IS NOT NEW.source_lower_order_key COLLATE BINARY OR OLD.source_lower_item_id IS NOT NEW.source_lower_item_id COLLATE BINARY OR OLD.source_high_order_key IS NOT NEW.source_high_order_key COLLATE BINARY OR OLD.source_high_item_id IS NOT NEW.source_high_item_id COLLATE BINARY OR OLD.source_revision_cutoff_id IS NOT NEW.source_revision_cutoff_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_channel_reference_targets
BEFORE INSERT ON chat_channel_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 375 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_channel_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_channel_reference_targets_insert
AFTER INSERT ON chat_channel_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (375, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_channel_reference_targets_update
AFTER UPDATE ON chat_channel_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 375, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_channel_reference_targets_delete
AFTER DELETE ON chat_channel_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (375, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_execution_environment_reference_targets_insert
BEFORE INSERT ON chat_execution_environment_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (typeof(NEW.execution_environment_id) = 'text' AND instr(CAST(NEW.execution_environment_id AS BLOB), x'00') = 0 AND length(NEW.execution_environment_id) <= 2048 AND length(CAST(NEW.execution_environment_id AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_execution_environment_reference_targets_update
BEFORE UPDATE OF execution_environment_id ON chat_execution_environment_reference_targets
WHEN ((typeof(NEW.execution_environment_id) = 'text' AND instr(CAST(NEW.execution_environment_id AS BLOB), x'00') = 0 AND length(NEW.execution_environment_id) <= 2048 AND length(CAST(NEW.execution_environment_id AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_execution_environment_reference_targets
BEFORE UPDATE OF reference_id, execution_environment_id ON chat_execution_environment_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.execution_environment_id IS NOT NEW.execution_environment_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_execution_environment_reference_targets
BEFORE INSERT ON chat_execution_environment_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 376 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_execution_environment_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_execution_environment_reference_targets_insert
AFTER INSERT ON chat_execution_environment_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (376, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_execution_environment_reference_targets_update
AFTER UPDATE ON chat_execution_environment_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 376, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_execution_environment_reference_targets_delete
AFTER DELETE ON chat_execution_environment_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (376, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_participant_reference_targets_insert
BEFORE INSERT ON chat_participant_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_participant_reference_targets_update
BEFORE UPDATE OF participant_id ON chat_participant_reference_targets
WHEN ((NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_participant_reference_targets
BEFORE UPDATE OF reference_id, participant_id ON chat_participant_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_participant_reference_targets
BEFORE INSERT ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 377 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_participant_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_insert
AFTER INSERT ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (377, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_update
AFTER UPDATE ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 377, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_participant_reference_targets_delete
AFTER DELETE ON chat_participant_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (377, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_working_folder_reference_targets_insert
BEFORE INSERT ON chat_working_folder_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_working_folder_reference_targets_update
BEFORE UPDATE OF working_folder_id ON chat_working_folder_reference_targets
WHEN ((NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_working_folder_reference_targets
BEFORE UPDATE OF reference_id, working_folder_id ON chat_working_folder_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_working_folder_reference_targets
BEFORE INSERT ON chat_working_folder_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 378 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_working_folder_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_working_folder_reference_targets_insert
AFTER INSERT ON chat_working_folder_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (378, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_working_folder_reference_targets_update
AFTER UPDATE ON chat_working_folder_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 378, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_working_folder_reference_targets_delete
AFTER DELETE ON chat_working_folder_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (378, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_workspace_path_reference_targets_insert
BEFORE INSERT ON chat_workspace_path_reference_targets
WHEN ((typeof(NEW.reference_id) = 'text' AND instr(CAST(NEW.reference_id AS BLOB), x'00') = 0 AND trim(NEW.reference_id) <> '' AND length(CAST(NEW.reference_id AS BLOB)) <= 128)
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.path_kind) = 'text' AND instr(CAST(NEW.path_kind AS BLOB), x'00') = 0 AND length(NEW.path_kind) <= 32 AND length(CAST(NEW.path_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(NEW.relative_path) <= 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_workspace_path_reference_targets_update
BEFORE UPDATE OF working_folder_id, path_kind, relative_path ON chat_workspace_path_reference_targets
WHEN ((NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.path_kind) = 'text' AND instr(CAST(NEW.path_kind AS BLOB), x'00') = 0 AND length(NEW.path_kind) <= 32 AND length(CAST(NEW.path_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(NEW.relative_path) <= 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_workspace_path_reference_targets
BEFORE UPDATE OF reference_id, working_folder_id, path_kind, relative_path ON chat_workspace_path_reference_targets
WHEN OLD.reference_id IS NOT NEW.reference_id COLLATE BINARY
    OR ((OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY OR OLD.path_kind IS NOT NEW.path_kind COLLATE BINARY OR OLD.relative_path IS NOT NEW.relative_path COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_workspace_path_reference_targets
BEFORE INSERT ON chat_workspace_path_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 379 AND row_key = NEW.reference_id)
    AND NOT EXISTS (SELECT 1 FROM chat_workspace_path_reference_targets WHERE reference_id = NEW.reference_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_workspace_path_reference_targets_insert
AFTER INSERT ON chat_workspace_path_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (379, NEW.reference_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_workspace_path_reference_targets_update
AFTER UPDATE ON chat_workspace_path_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 379, NEW.reference_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_workspace_path_reference_targets_delete
AFTER DELETE ON chat_workspace_path_reference_targets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (379, OLD.reference_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_conversation_read_cursors_insert
BEFORE INSERT ON chat_conversation_read_cursors
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_read_order_key) = 'text' AND instr(CAST(NEW.last_read_order_key AS BLOB), x'00') = 0 AND length(NEW.last_read_order_key) <= 128 AND length(CAST(NEW.last_read_order_key AS BLOB)) <= 262144)
    AND (typeof(NEW.last_read_item_id) = 'text' AND instr(CAST(NEW.last_read_item_id AS BLOB), x'00') = 0 AND length(NEW.last_read_item_id) <= 1024 AND length(CAST(NEW.last_read_item_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_conversation_read_cursors_update
BEFORE UPDATE OF conversation_id, participant_id, last_read_order_key, last_read_item_id, updated_at ON chat_conversation_read_cursors
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_read_order_key) = 'text' AND instr(CAST(NEW.last_read_order_key AS BLOB), x'00') = 0 AND length(NEW.last_read_order_key) <= 128 AND length(CAST(NEW.last_read_order_key AS BLOB)) <= 262144)
    AND (typeof(NEW.last_read_item_id) = 'text' AND instr(CAST(NEW.last_read_item_id AS BLOB), x'00') = 0 AND length(NEW.last_read_item_id) <= 1024 AND length(CAST(NEW.last_read_item_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_conversation_read_cursors
BEFORE UPDATE OF sync_key, conversation_id, participant_id ON chat_conversation_read_cursors
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_conversation_read_cursors_insert
AFTER INSERT ON chat_conversation_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (380, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversation_read_cursors_update
AFTER UPDATE ON chat_conversation_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 380, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.last_read_order_key IS NOT NEW.last_read_order_key COLLATE BINARY OR OLD.last_read_item_id IS NOT NEW.last_read_item_id COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 380, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_conversation_read_cursors_delete
AFTER DELETE ON chat_conversation_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (380, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_chat_reply_thread_read_cursors_insert
BEFORE INSERT ON chat_reply_thread_read_cursors
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_read_order_key) = 'text' AND instr(CAST(NEW.last_read_order_key AS BLOB), x'00') = 0 AND length(NEW.last_read_order_key) <= 128 AND length(CAST(NEW.last_read_order_key AS BLOB)) <= 262144)
    AND (typeof(NEW.last_read_item_id) = 'text' AND instr(CAST(NEW.last_read_item_id AS BLOB), x'00') = 0 AND length(NEW.last_read_item_id) <= 1024 AND length(CAST(NEW.last_read_item_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_reply_thread_read_cursors_update
BEFORE UPDATE OF reply_thread_id, participant_id, last_read_order_key, last_read_item_id, updated_at ON chat_reply_thread_read_cursors
WHEN ((NEW.reply_thread_id IS NULL OR (typeof(NEW.reply_thread_id) = 'text' AND instr(CAST(NEW.reply_thread_id AS BLOB), x'00') = 0 AND trim(NEW.reply_thread_id) <> '' AND length(CAST(NEW.reply_thread_id AS BLOB)) <= 128))
    AND (NEW.participant_id IS NULL OR (typeof(NEW.participant_id) = 'text' AND instr(CAST(NEW.participant_id AS BLOB), x'00') = 0 AND trim(NEW.participant_id) <> '' AND length(CAST(NEW.participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.last_read_order_key) = 'text' AND instr(CAST(NEW.last_read_order_key AS BLOB), x'00') = 0 AND length(NEW.last_read_order_key) <= 128 AND length(CAST(NEW.last_read_order_key AS BLOB)) <= 262144)
    AND (typeof(NEW.last_read_item_id) = 'text' AND instr(CAST(NEW.last_read_item_id AS BLOB), x'00') = 0 AND length(NEW.last_read_item_id) <= 1024 AND length(CAST(NEW.last_read_item_id AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_reply_thread_read_cursors
BEFORE UPDATE OF sync_key, reply_thread_id, participant_id ON chat_reply_thread_read_cursors
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.participant_id IS NOT NEW.participant_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_reply_thread_read_cursors_insert
AFTER INSERT ON chat_reply_thread_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (381, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_reply_thread_read_cursors_update
AFTER UPDATE ON chat_reply_thread_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 381, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.last_read_order_key IS NOT NEW.last_read_order_key COLLATE BINARY OR OLD.last_read_item_id IS NOT NEW.last_read_item_id COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 381, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_reply_thread_read_cursors_delete
AFTER DELETE ON chat_reply_thread_read_cursors
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (381, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

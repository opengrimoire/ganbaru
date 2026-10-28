-- no-transaction
-- Chat attachments replication: message revision attachment references join the vault manifest
-- as rows owned by their revision, and scheduled message attachment references take the same
-- shape. Both carry the attachment by value (working folder, kind, display name, content type,
-- size, and digest) instead of pointing at the device's attachment row, so a replica can show
-- and fetch an attachment it never imported. Attachment files are content addressed: several
-- attachment and resource rows may name one `{sha256}.{ext}` file, so their path uniques are
-- dropped. The Chat conversion pass renames existing files at vault open. Attachment rows stay
-- on the device; creating their resource and marking it deleted stand down while sync applies,
-- and the Chat hook creates both rows for applied references instead.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

-- chat_attachments
CREATE TABLE chat_attachments_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('image', 'text_snippet')),
    original_display_name TEXT NOT NULL CHECK (length(trim(original_display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 52428800),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/..'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    signature_kind TEXT NOT NULL CHECK (length(signature_kind) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deletion_state TEXT NOT NULL DEFAULT 'active' CHECK (
        deletion_state IN ('active', 'pending_delete', 'deleted', 'cleanup_failed')
    ),
    unreferenced_at TEXT CHECK (unreferenced_at IS NULL OR length(unreferenced_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;

INSERT INTO chat_attachments_next (
    id, working_folder_id, kind, original_display_name, mime_type, byte_size, sha256,
    managed_relative_path, signature_kind, created_at, deletion_state, unreferenced_at, deleted_at
)
SELECT
    id, working_folder_id, kind, original_display_name, mime_type, byte_size, sha256,
    managed_relative_path, signature_kind, created_at, deletion_state, unreferenced_at, deleted_at
FROM chat_attachments;

DROP TABLE chat_attachments;
ALTER TABLE chat_attachments_next RENAME TO chat_attachments;

CREATE INDEX idx_chat_attachments_cleanup
ON chat_attachments(deletion_state, unreferenced_at, id);
CREATE INDEX idx_chat_attachments_hash
ON chat_attachments(sha256, byte_size, id);
CREATE INDEX idx_chat_attachments_path
ON chat_attachments(managed_relative_path);

CREATE TRIGGER chat_attachments_create_resource
AFTER INSERT ON chat_attachments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO chat_resources (
        id, working_folder_id, attachment_id, resource_kind, display_name, mime_type, byte_size,
        sha256, managed_relative_path, resource_uri, integrity_state, created_at
    ) VALUES (
        NEW.id, NEW.working_folder_id, NEW.id, NEW.kind, NEW.original_display_name, NEW.mime_type,
        NEW.byte_size, NEW.sha256, NEW.managed_relative_path,
        'ganbaru://chat/resource/' || NEW.id, 'verified', NEW.created_at
    );
END;

CREATE TRIGGER chat_attachments_managed_relative_path_shape_insert
BEFORE INSERT ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_attachments_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_attachments_update_resource_integrity
AFTER UPDATE OF deletion_state, deleted_at ON chat_attachments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    UPDATE chat_resources
    SET integrity_state = CASE NEW.deletion_state
            WHEN 'deleted' THEN 'deleted'
            ELSE integrity_state
        END,
        deleted_at = NEW.deleted_at
    WHERE attachment_id = NEW.id;
END;

-- chat_resources
CREATE TABLE chat_resources_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 1024),
    working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id) ON UPDATE CASCADE ON DELETE CASCADE,
    attachment_id TEXT UNIQUE REFERENCES chat_attachments(id) ON UPDATE CASCADE ON DELETE CASCADE,
    resource_kind TEXT NOT NULL CHECK (resource_kind IN ('image', 'text_snippet', 'browser_screenshot', 'browser_recording')),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (length(mime_type) BETWEEN 1 AND 255),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 1073741824),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    managed_relative_path TEXT NOT NULL CHECK (
        length(managed_relative_path) BETWEEN 1 AND 2048
        AND managed_relative_path NOT LIKE '/%'
        AND managed_relative_path NOT LIKE '../%'
        AND managed_relative_path NOT LIKE '%/../%'
        AND managed_relative_path NOT LIKE '%\\%'
    ),
    resource_uri TEXT NOT NULL UNIQUE CHECK (resource_uri GLOB 'ganbaru://chat/resource/*'),
    integrity_state TEXT NOT NULL DEFAULT 'verified' CHECK (
        integrity_state IN ('verified', 'missing', 'hash_mismatch', 'unsafe_path', 'deleted')
    ),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20)
) STRICT;

INSERT INTO chat_resources_next (
    id, working_folder_id, attachment_id, resource_kind, display_name, mime_type, byte_size,
    sha256, managed_relative_path, resource_uri, integrity_state, created_at, deleted_at
)
SELECT
    id, working_folder_id, attachment_id, resource_kind, display_name, mime_type, byte_size,
    sha256, managed_relative_path, resource_uri, integrity_state, created_at, deleted_at
FROM chat_resources;

DROP TABLE chat_resources;
ALTER TABLE chat_resources_next RENAME TO chat_resources;

CREATE INDEX idx_chat_resources_workspace_kind
ON chat_resources(working_folder_id, resource_kind, created_at DESC, id);
CREATE INDEX idx_chat_resources_path
ON chat_resources(managed_relative_path);

CREATE TRIGGER chat_resources_managed_relative_path_shape_insert
BEFORE INSERT ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER chat_resources_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

-- chat_communication_attachment_references: owned by their revision, ordered by ordinal, and
-- holding the attachment by value. A revision holds at most 20 attachments.
CREATE TABLE chat_communication_attachment_references_next (
    message_revision_id TEXT NOT NULL REFERENCES chat_communication_message_revisions(id)
        ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 19),
    attachment_id TEXT NOT NULL CHECK (length(attachment_id) BETWEEN 1 AND 128),
    working_folder_id TEXT NOT NULL CHECK (length(working_folder_id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('image', 'text_snippet')),
    original_display_name TEXT NOT NULL CHECK (length(trim(original_display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (
        mime_type IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp', 'text/plain')
    ),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 52428800),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (message_revision_id, ordinal)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_communication_attachment_references_next (
    message_revision_id, ordinal, attachment_id, working_folder_id, kind, original_display_name,
    mime_type, byte_size, sha256
)
SELECT
    reference.message_revision_id,
    row_number() OVER (
        PARTITION BY reference.message_revision_id ORDER BY reference.ordinal, reference.attachment_id
    ) - 1,
    attachment.id, attachment.working_folder_id, attachment.kind,
    attachment.original_display_name, attachment.mime_type, attachment.byte_size,
    attachment.sha256
FROM chat_communication_attachment_references reference
JOIN chat_attachments attachment ON attachment.id = reference.attachment_id;

DROP TABLE chat_communication_attachment_references;
ALTER TABLE chat_communication_attachment_references_next
RENAME TO chat_communication_attachment_references;

CREATE INDEX idx_chat_communication_attachment_references_attachment
ON chat_communication_attachment_references(attachment_id);
CREATE INDEX idx_chat_communication_attachment_references_digest
ON chat_communication_attachment_references(sha256);

-- chat_scheduled_message_attachment_references: the same shape, owned by the scheduled message.
CREATE TABLE chat_scheduled_message_attachment_references_next (
    scheduled_message_id TEXT NOT NULL REFERENCES chat_scheduled_messages(id)
        ON UPDATE CASCADE ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 19),
    attachment_id TEXT NOT NULL CHECK (length(attachment_id) BETWEEN 1 AND 128),
    working_folder_id TEXT NOT NULL CHECK (length(working_folder_id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('image', 'text_snippet')),
    original_display_name TEXT NOT NULL CHECK (length(trim(original_display_name)) BETWEEN 1 AND 1000),
    mime_type TEXT NOT NULL CHECK (
        mime_type IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp', 'text/plain')
    ),
    byte_size INTEGER NOT NULL CHECK (byte_size BETWEEN 0 AND 52428800),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (scheduled_message_id, ordinal)
) STRICT, WITHOUT ROWID;

INSERT INTO chat_scheduled_message_attachment_references_next (
    scheduled_message_id, ordinal, attachment_id, working_folder_id, kind, original_display_name,
    mime_type, byte_size, sha256
)
SELECT
    reference.scheduled_message_id,
    row_number() OVER (
        PARTITION BY reference.scheduled_message_id ORDER BY reference.ordinal, reference.attachment_id
    ) - 1,
    attachment.id, attachment.working_folder_id, attachment.kind,
    attachment.original_display_name, attachment.mime_type, attachment.byte_size,
    attachment.sha256
FROM chat_scheduled_message_attachment_references reference
JOIN chat_attachments attachment ON attachment.id = reference.attachment_id;

DROP TABLE chat_scheduled_message_attachment_references;
ALTER TABLE chat_scheduled_message_attachment_references_next
RENAME TO chat_scheduled_message_attachment_references;

CREATE INDEX idx_chat_scheduled_message_attachment_references_attachment
ON chat_scheduled_message_attachment_references(attachment_id, scheduled_message_id);
CREATE INDEX idx_chat_scheduled_message_attachment_references_digest
ON chat_scheduled_message_attachment_references(sha256);

CREATE TEMP TABLE chat_attachments_replication_guard (
    violations INTEGER NOT NULL CHECK (violations = 0)
);
INSERT INTO chat_attachments_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE chat_attachments_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.
DROP TRIGGER IF EXISTS sync_capture_chat_communication_message_revisions_insert;

CREATE TRIGGER sync_capture_chat_communication_message_revisions_insert
AFTER INSERT ON chat_communication_message_revisions
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (373, NEW.id, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_communication_attachment_references_insert
AFTER INSERT ON chat_communication_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (373, NEW.message_revision_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_communication_attachment_references_update
AFTER UPDATE ON chat_communication_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 373, parent.key, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.message_revision_id AS key UNION SELECT OLD.message_revision_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_communication_attachment_references_delete
AFTER DELETE ON chat_communication_attachment_references
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (373, OLD.message_revision_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

-- Revisions written before the attachments group existed capture it once, so their
-- attachments replicate.
INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
SELECT DISTINCT 373, message_revision_id, 2, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM chat_communication_attachment_references
WHERE (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

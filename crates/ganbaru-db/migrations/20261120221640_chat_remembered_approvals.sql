-- A remembered approval lets one agent repeat one kind of request in one channel without asking:
-- pages of one web origin, one exact command, or every call of one tool. It holds while the
-- channel's effective access stays at or above the access it was given under.
CREATE TABLE chat_remembered_approvals (
  conversation_id TEXT NOT NULL REFERENCES chat_conversations(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  agent_id TEXT NOT NULL REFERENCES chat_agents(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  subject_kind TEXT NOT NULL CHECK (subject_kind IN ('url_origin', 'command', 'tool')),
  subject_key TEXT NOT NULL CHECK (length(CAST(subject_key AS BLOB)) BETWEEN 1 AND 2048),
  sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
  approved_by_participant_id TEXT REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  approved_at TEXT NOT NULL CHECK (length(approved_at) >= 20),
  revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
  granted_access TEXT NOT NULL CHECK (granted_access IN ('talk', 'read', 'ask', 'own')),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  PRIMARY KEY (conversation_id, agent_id, subject_kind, subject_key)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_chat_remembered_approvals_approved_by
ON chat_remembered_approvals(approved_by_participant_id);

CREATE INDEX idx_chat_remembered_approvals_agent
ON chat_remembered_approvals(agent_id, conversation_id);

CREATE TRIGGER chat_remembered_approvals_approver_is_person_insert
BEFORE INSERT ON chat_remembered_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Remembered approvals are made by people');
END;

CREATE TRIGGER chat_remembered_approvals_approver_is_person_update
BEFORE UPDATE OF approved_by_participant_id, approved_at
ON chat_remembered_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        NEW.approved_by_participant_id IS NULL
        AND OLD.approved_by_participant_id IS NOT NULL
        AND NEW.approved_at IS OLD.approved_at
    )
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Remembered approvals are made by people');
END;

CREATE TRIGGER chat_agent_channel_membership_remembered_approvals_delete
AFTER DELETE ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
BEGIN
    UPDATE chat_remembered_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE conversation_id = OLD.conversation_id AND agent_id = OLD.agent_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_agent_channel_membership_remembered_approvals_update
AFTER UPDATE OF removed_at
ON chat_agent_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
    AND NEW.removed_at IS NOT NULL
BEGIN
    UPDATE chat_remembered_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE conversation_id = NEW.conversation_id AND agent_id = NEW.agent_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_participant_remembered_approvals_archive
AFTER UPDATE OF archived_at ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL
BEGIN
    UPDATE chat_remembered_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE agent_id = NEW.id AND revoked_at IS NULL;
END;

-- Sync triggers rendered from the manifest.

CREATE TRIGGER sync_values_chat_remembered_approvals_insert
BEFORE INSERT ON chat_remembered_approvals
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.subject_kind) = 'text' AND instr(CAST(NEW.subject_kind AS BLOB), x'00') = 0 AND length(NEW.subject_kind) <= 32 AND length(CAST(NEW.subject_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.subject_key) = 'text' AND instr(CAST(NEW.subject_key AS BLOB), x'00') = 0 AND length(NEW.subject_key) <= 2048 AND length(CAST(NEW.subject_key AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (typeof(NEW.granted_access) = 'text' AND instr(CAST(NEW.granted_access AS BLOB), x'00') = 0 AND length(NEW.granted_access) <= 32 AND length(CAST(NEW.granted_access AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_remembered_approvals_update
BEFORE UPDATE OF conversation_id, agent_id, subject_kind, subject_key, approved_by_participant_id, approved_at, revoked_at, granted_access ON chat_remembered_approvals
WHEN ((NEW.conversation_id IS NULL OR (typeof(NEW.conversation_id) = 'text' AND instr(CAST(NEW.conversation_id AS BLOB), x'00') = 0 AND trim(NEW.conversation_id) <> '' AND length(CAST(NEW.conversation_id AS BLOB)) <= 128))
    AND (NEW.agent_id IS NULL OR (typeof(NEW.agent_id) = 'text' AND instr(CAST(NEW.agent_id AS BLOB), x'00') = 0 AND trim(NEW.agent_id) <> '' AND length(CAST(NEW.agent_id AS BLOB)) <= 128))
    AND (typeof(NEW.subject_kind) = 'text' AND instr(CAST(NEW.subject_kind AS BLOB), x'00') = 0 AND length(NEW.subject_kind) <= 32 AND length(CAST(NEW.subject_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.subject_key) = 'text' AND instr(CAST(NEW.subject_key AS BLOB), x'00') = 0 AND length(NEW.subject_key) <= 2048 AND length(CAST(NEW.subject_key AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))
    AND (typeof(NEW.granted_access) = 'text' AND instr(CAST(NEW.granted_access AS BLOB), x'00') = 0 AND length(NEW.granted_access) <= 32 AND length(CAST(NEW.granted_access AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_remembered_approvals
BEFORE UPDATE OF sync_key, conversation_id, agent_id, subject_kind, subject_key ON chat_remembered_approvals
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.agent_id IS NOT NEW.agent_id COLLATE BINARY OR OLD.subject_kind IS NOT NEW.subject_kind COLLATE BINARY OR OLD.subject_key IS NOT NEW.subject_key COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_remembered_approvals_insert
AFTER INSERT ON chat_remembered_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (366, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_remembered_approvals_update
AFTER UPDATE ON chat_remembered_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 366, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.approved_by_participant_id IS NOT NEW.approved_by_participant_id COLLATE BINARY
                AND NOT (NEW.approved_by_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.approved_by_participant_id))) OR OLD.approved_at IS NOT NEW.approved_at COLLATE BINARY OR OLD.revoked_at IS NOT NEW.revoked_at COLLATE BINARY OR OLD.granted_access IS NOT NEW.granted_access COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 366, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_remembered_approvals_delete
AFTER DELETE ON chat_remembered_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (366, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

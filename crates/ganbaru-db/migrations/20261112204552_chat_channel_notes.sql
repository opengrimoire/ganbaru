-- Channel notes: short facts and preferences people save for a channel's teammates. A note is
-- written by a person or accepted by one from a teammate proposal; the local triggers refuse a
-- note whose current wording names no person, and removing a person keeps their notes with a
-- null editor. A deleted note keeps its row so a concurrent edit on another device stays deleted.
CREATE TABLE chat_channel_notes (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  channel_id TEXT NOT NULL REFERENCES chat_channels(id) ON UPDATE CASCADE ON DELETE CASCADE,
  source_item_id TEXT REFERENCES chat_conversation_items(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  proposed_by_participant_id TEXT REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  body TEXT NOT NULL CHECK (length(trim(body)) BETWEEN 1 AND 1000),
  edited_by_participant_id TEXT REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  deleted_at TEXT CHECK (deleted_at IS NULL OR length(deleted_at) >= 20),
  created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
CREATE INDEX idx_chat_channel_notes_channel
ON chat_channel_notes(channel_id, deleted_at, updated_at);
CREATE INDEX idx_chat_channel_notes_source_item
ON chat_channel_notes(source_item_id);
CREATE INDEX idx_chat_channel_notes_proposed_by
ON chat_channel_notes(proposed_by_participant_id);
CREATE INDEX idx_chat_channel_notes_edited_by
ON chat_channel_notes(edited_by_participant_id);

CREATE TRIGGER chat_channel_notes_editor_is_person_insert
BEFORE INSERT ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.edited_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Channel notes are edited by people');
END;

-- Removing the editing person sets the editor to null without touching the wording; that
-- update passes so the person's notes stay.
CREATE TRIGGER chat_channel_notes_editor_is_person_update
BEFORE UPDATE OF body, edited_by_participant_id, deleted_at ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        NEW.edited_by_participant_id IS NULL
        AND OLD.edited_by_participant_id IS NOT NULL
        AND NEW.body IS OLD.body
        AND NEW.deleted_at IS OLD.deleted_at
    )
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.edited_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Channel notes are edited by people');
END;

CREATE TRIGGER sync_values_chat_channel_notes_insert
BEFORE INSERT ON chat_channel_notes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.source_item_id IS NULL OR (typeof(NEW.source_item_id) = 'text' AND instr(CAST(NEW.source_item_id AS BLOB), x'00') = 0 AND trim(NEW.source_item_id) <> '' AND length(CAST(NEW.source_item_id AS BLOB)) <= 128))
    AND (NEW.proposed_by_participant_id IS NULL OR (typeof(NEW.proposed_by_participant_id) = 'text' AND instr(CAST(NEW.proposed_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.proposed_by_participant_id) <> '' AND length(CAST(NEW.proposed_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.body) = 'text' AND instr(CAST(NEW.body AS BLOB), x'00') = 0 AND length(trim(NEW.body)) BETWEEN 1 AND 1000 AND length(CAST(NEW.body AS BLOB)) <= 262144)
    AND (NEW.edited_by_participant_id IS NULL OR (typeof(NEW.edited_by_participant_id) = 'text' AND instr(CAST(NEW.edited_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.edited_by_participant_id) <> '' AND length(CAST(NEW.edited_by_participant_id AS BLOB)) <= 128))
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_channel_notes_update
BEFORE UPDATE OF channel_id, created_at, source_item_id, proposed_by_participant_id, body, edited_by_participant_id, deleted_at, updated_at ON chat_channel_notes
WHEN ((NEW.channel_id IS NULL OR (typeof(NEW.channel_id) = 'text' AND instr(CAST(NEW.channel_id AS BLOB), x'00') = 0 AND trim(NEW.channel_id) <> '' AND length(CAST(NEW.channel_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.source_item_id IS NULL OR (typeof(NEW.source_item_id) = 'text' AND instr(CAST(NEW.source_item_id AS BLOB), x'00') = 0 AND trim(NEW.source_item_id) <> '' AND length(CAST(NEW.source_item_id AS BLOB)) <= 128))
    AND (NEW.proposed_by_participant_id IS NULL OR (typeof(NEW.proposed_by_participant_id) = 'text' AND instr(CAST(NEW.proposed_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.proposed_by_participant_id) <> '' AND length(CAST(NEW.proposed_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.body) = 'text' AND instr(CAST(NEW.body AS BLOB), x'00') = 0 AND length(trim(NEW.body)) BETWEEN 1 AND 1000 AND length(CAST(NEW.body AS BLOB)) <= 262144)
    AND (NEW.edited_by_participant_id IS NULL OR (typeof(NEW.edited_by_participant_id) = 'text' AND instr(CAST(NEW.edited_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.edited_by_participant_id) <> '' AND length(CAST(NEW.edited_by_participant_id AS BLOB)) <= 128))
    AND (NEW.deleted_at IS NULL OR (typeof(NEW.deleted_at) = 'text' AND instr(CAST(NEW.deleted_at AS BLOB), x'00') = 0 AND trim(NEW.deleted_at) <> '' AND length(CAST(NEW.deleted_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_channel_notes
BEFORE UPDATE OF id, channel_id, created_at ON chat_channel_notes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.channel_id IS NOT NEW.channel_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_channel_notes
BEFORE INSERT ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 400 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_channel_notes WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_channel_notes_insert
AFTER INSERT ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (400, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_channel_notes_update
AFTER UPDATE ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 400, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN (OLD.source_item_id IS NOT NEW.source_item_id COLLATE BINARY
                AND NOT (NEW.source_item_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_conversation_items WHERE id = OLD.source_item_id))) OR (OLD.proposed_by_participant_id IS NOT NEW.proposed_by_participant_id COLLATE BINARY
                AND NOT (NEW.proposed_by_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.proposed_by_participant_id)))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.body IS NOT NEW.body COLLATE BINARY OR (OLD.edited_by_participant_id IS NOT NEW.edited_by_participant_id COLLATE BINARY
                AND NOT (NEW.edited_by_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.edited_by_participant_id)))
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.deleted_at IS NOT NEW.deleted_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_channel_notes_delete
AFTER DELETE ON chat_channel_notes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (400, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

-- A reply that waits for running work takes its place in the thread when the work receives it.
-- `order_key` and `created_at` replicate as one placement register, so the executing device can
-- move the reply to the end of its thread and every device shows it there.
DROP TRIGGER sync_immutable_chat_conversation_items;
DROP TRIGGER sync_capture_chat_conversation_items_insert;
DROP TRIGGER sync_capture_chat_conversation_items_update;

CREATE TRIGGER sync_immutable_chat_conversation_items
BEFORE UPDATE OF id, conversation_id, reply_thread_id, item_kind ON chat_conversation_items
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.conversation_id IS NOT NEW.conversation_id COLLATE BINARY OR OLD.reply_thread_id IS NOT NEW.reply_thread_id COLLATE BINARY OR OLD.item_kind IS NOT NEW.item_kind COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_conversation_items_insert
AFTER INSERT ON chat_conversation_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (371, NEW.id, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
            (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

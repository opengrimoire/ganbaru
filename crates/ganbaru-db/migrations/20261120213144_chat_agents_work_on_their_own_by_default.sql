-- New agents work on their own by default. Only a vault whose app access was never saved or
-- received moves, so a level a person chose stays. In a vault that already published the row,
-- the update is captured like any app settings edit, and a concurrent narrower choice from a
-- linked device still wins the merge; a vault that has not seeded the table yet publishes the
-- current row when it does, so the capture is dropped there.
UPDATE chat_agent_app_settings
SET agent_access = 'own',
    revision = revision + 1,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'vault' AND revision = 1 AND agent_access = 'ask';

DELETE FROM sync_capture
WHERE table_id = 357
  AND row_key = 'vault'
  AND NOT EXISTS (SELECT 1 FROM sync_seeded_tables WHERE table_id = 357);

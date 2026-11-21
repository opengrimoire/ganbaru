-- An agent a person named and never used is deleted rather than archived, so its participant,
-- agent row, and policy revisions become deletable and replicate their deletion. A policy
-- revision still refuses any delete while its agent exists, including one sync applies, so it
-- leaves only through the agent's cascade.
DROP TRIGGER IF EXISTS chat_agent_policy_immutable_delete;

CREATE TRIGGER chat_agent_policy_immutable_delete
BEFORE DELETE ON chat_agent_policy_revisions
WHEN EXISTS (SELECT 1 FROM chat_agents WHERE participant_id = OLD.agent_id)
BEGIN
    SELECT RAISE(ABORT, 'Agent policy revisions are immutable');
END;

-- Only a named agent, keyed `agent:` and 32 lowercase hex digits, is ever deleted. Sync refuses
-- any other participant deletion, so a local one would leave this device apart for good.
CREATE TRIGGER chat_participants_named_agent_delete
BEFORE DELETE ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
 AND (length(OLD.id) <> 38
   OR OLD.id NOT GLOB 'agent:[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]')
BEGIN
    SELECT RAISE(ABORT, 'Only a named agent can be deleted');
END;

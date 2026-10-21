-- A teammate's default runtime approval derives from its policy revision's approval behavior, so the access state no
-- longer stores a separate default that could disagree with it.
ALTER TABLE chat_teammate_access_state DROP COLUMN runtime_approval_policy;

-- Standing push approvals: a person's "push and don't ask again" for one teammate, working
-- folder, push URL, and destination branch. A later matching push request of that teammate
-- pushes without asking while the run's approval policy and the teammate's safety mode still
-- equal the ones recorded here. Revoking sets `revoked_at` and keeps the row, so a concurrent
-- approval on another device stays revoked. The local triggers refuse an approval no person
-- made, and revoke the matching approvals when the teammate is archived, its safety mode
-- changes, or a grant, channel membership, or access profile that let it publish narrows.
CREATE TABLE chat_push_approvals (
  teammate_id TEXT NOT NULL REFERENCES chat_teammates(participant_id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  working_folder_id TEXT NOT NULL REFERENCES project_working_folders(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  push_url TEXT NOT NULL CHECK (length(CAST(push_url AS BLOB)) BETWEEN 1 AND 2048),
  destination_branch TEXT NOT NULL CHECK (
      length(CAST(destination_branch AS BLOB)) BETWEEN 1 AND 1024
  ),
  sync_key TEXT NOT NULL UNIQUE CHECK (length(sync_key) BETWEEN 1 AND 128),
  runtime_approval_policy TEXT NOT NULL CHECK (
      runtime_approval_policy IN ('ask', 'auto_approve', 'unattended', 'provider_custom')
  ),
  safety_mode TEXT NOT NULL CHECK (
      safety_mode IN ('ask_for_approval', 'approve_for_me', 'full_access', 'custom')
  ),
  approved_by_participant_id TEXT REFERENCES chat_participants(id)
      ON UPDATE CASCADE ON DELETE SET NULL,
  approved_at TEXT NOT NULL CHECK (length(approved_at) >= 20),
  revoked_at TEXT CHECK (revoked_at IS NULL OR length(revoked_at) >= 20),
  revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
  PRIMARY KEY (teammate_id, working_folder_id, push_url, destination_branch)
) STRICT, WITHOUT ROWID;
CREATE INDEX idx_chat_push_approvals_folder
ON chat_push_approvals(working_folder_id);
CREATE INDEX idx_chat_push_approvals_approved_by
ON chat_push_approvals(approved_by_participant_id);

CREATE TRIGGER chat_push_approvals_approver_is_person_insert
BEFORE INSERT ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Push approvals are made by people');
END;

-- Removing the approving person sets the approver to null without approving anything again;
-- that update passes so the approval stays.
CREATE TRIGGER chat_push_approvals_approver_is_person_update
BEFORE UPDATE OF approved_by_participant_id, approved_at, runtime_approval_policy, safety_mode
ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NOT (
        NEW.approved_by_participant_id IS NULL
        AND OLD.approved_by_participant_id IS NOT NULL
        AND NEW.approved_at IS OLD.approved_at
        AND NEW.runtime_approval_policy IS OLD.runtime_approval_policy
        AND NEW.safety_mode IS OLD.safety_mode
    )
    AND NOT EXISTS (
        SELECT 1 FROM chat_participants participant
        WHERE participant.id = NEW.approved_by_participant_id
          AND participant.participant_kind IN ('local_user', 'human')
    )
BEGIN
    SELECT RAISE(ABORT, 'Push approvals are made by people');
END;

-- A grant that let the teammate publish from the folder, by its own capability or through its
-- access profile, drops the teammate's approvals for the folder when it is revoked, deleted, or
-- changes its capability or approval override.
CREATE TRIGGER chat_teammate_folder_grant_push_approvals_update
AFTER UPDATE OF capability, capability_inherits_profile, runtime_approval_policy, revoked_at
ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.revoked_at IS NULL
    AND (OLD.capability = 'publish' OR OLD.capability_inherits_profile = 1)
    AND (
        NEW.revoked_at IS NOT NULL
        OR NEW.capability IS NOT OLD.capability
        OR NEW.capability_inherits_profile IS NOT OLD.capability_inherits_profile
        OR NEW.runtime_approval_policy IS NOT OLD.runtime_approval_policy
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = NEW.teammate_id AND working_folder_id = NEW.working_folder_id
      AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_teammate_folder_grant_push_approvals_delete
AFTER DELETE ON chat_teammate_working_folder_grants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.revoked_at IS NULL
    AND (OLD.capability = 'publish' OR OLD.capability_inherits_profile = 1)
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = OLD.teammate_id AND working_folder_id = OLD.working_folder_id
      AND revoked_at IS NULL;
END;

-- A channel membership supplies the access profile inheriting grants read and an approval
-- override, so removing it or changing either drops the teammate's approvals.
CREATE TRIGGER chat_teammate_channel_membership_push_approvals_update
AFTER UPDATE OF access_profile_id, runtime_approval_policy, removed_at
ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
    AND (
        NEW.removed_at IS NOT NULL
        OR NEW.access_profile_id IS NOT OLD.access_profile_id
        OR NEW.runtime_approval_policy IS NOT OLD.runtime_approval_policy
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = NEW.teammate_id AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_teammate_channel_membership_push_approvals_delete
AFTER DELETE ON chat_teammate_channel_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.removed_at IS NULL
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = OLD.teammate_id AND revoked_at IS NULL;
END;

-- An access profile that stops allowing publish drops the approvals of every teammate whose
-- channel membership uses it.
CREATE TRIGGER chat_access_profile_push_approvals_update
AFTER UPDATE OF current_maximum_folder_capability, archived_at ON chat_access_profiles
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.current_maximum_folder_capability = 'publish'
    AND (
        NEW.current_maximum_folder_capability IS NOT 'publish'
        OR (NEW.archived_at IS NOT NULL AND OLD.archived_at IS NULL)
    )
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE revoked_at IS NULL
      AND teammate_id IN (
          SELECT membership.teammate_id FROM chat_teammate_channel_memberships membership
          WHERE membership.access_profile_id = NEW.id
      );
END;

CREATE TRIGGER chat_participant_push_approvals_archive
AFTER UPDATE OF archived_at ON chat_participants
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND OLD.archived_at IS NULL AND NEW.archived_at IS NOT NULL
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = NEW.id AND revoked_at IS NULL;
END;

CREATE TRIGGER chat_teammate_push_approvals_safety_mode
AFTER UPDATE OF current_policy_safety_mode ON chat_teammates
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.current_policy_safety_mode IS NOT OLD.current_policy_safety_mode
BEGIN
    UPDATE chat_push_approvals
    SET revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), revision = revision + 1
    WHERE teammate_id = NEW.participant_id AND revoked_at IS NULL;
END;

-- Replication triggers rendered from the sync manifest.
CREATE TRIGGER sync_values_chat_push_approvals_insert
BEFORE INSERT ON chat_push_approvals
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_push_approvals_update
BEFORE UPDATE OF teammate_id, working_folder_id, push_url, destination_branch, runtime_approval_policy, safety_mode, approved_by_participant_id, approved_at, revoked_at ON chat_push_approvals
WHEN ((NEW.teammate_id IS NULL OR (typeof(NEW.teammate_id) = 'text' AND instr(CAST(NEW.teammate_id AS BLOB), x'00') = 0 AND trim(NEW.teammate_id) <> '' AND length(CAST(NEW.teammate_id AS BLOB)) <= 128))
    AND (NEW.working_folder_id IS NULL OR (typeof(NEW.working_folder_id) = 'text' AND instr(CAST(NEW.working_folder_id AS BLOB), x'00') = 0 AND trim(NEW.working_folder_id) <> '' AND length(CAST(NEW.working_folder_id AS BLOB)) <= 128))
    AND (typeof(NEW.push_url) = 'text' AND instr(CAST(NEW.push_url AS BLOB), x'00') = 0 AND length(NEW.push_url) <= 2048 AND length(CAST(NEW.push_url AS BLOB)) <= 262144)
    AND (typeof(NEW.destination_branch) = 'text' AND instr(CAST(NEW.destination_branch AS BLOB), x'00') = 0 AND length(NEW.destination_branch) <= 1024 AND length(CAST(NEW.destination_branch AS BLOB)) <= 262144)
    AND (typeof(NEW.runtime_approval_policy) = 'text' AND instr(CAST(NEW.runtime_approval_policy AS BLOB), x'00') = 0 AND length(NEW.runtime_approval_policy) <= 32 AND length(CAST(NEW.runtime_approval_policy AS BLOB)) <= 262144)
    AND (typeof(NEW.safety_mode) = 'text' AND instr(CAST(NEW.safety_mode AS BLOB), x'00') = 0 AND length(NEW.safety_mode) <= 32 AND length(CAST(NEW.safety_mode AS BLOB)) <= 262144)
    AND (NEW.approved_by_participant_id IS NULL OR (typeof(NEW.approved_by_participant_id) = 'text' AND instr(CAST(NEW.approved_by_participant_id AS BLOB), x'00') = 0 AND trim(NEW.approved_by_participant_id) <> '' AND length(CAST(NEW.approved_by_participant_id AS BLOB)) <= 128))
    AND (typeof(NEW.approved_at) = 'text' AND instr(CAST(NEW.approved_at AS BLOB), x'00') = 0 AND trim(NEW.approved_at) <> '' AND length(CAST(NEW.approved_at AS BLOB)) <= 64)
    AND (NEW.revoked_at IS NULL OR (typeof(NEW.revoked_at) = 'text' AND instr(CAST(NEW.revoked_at AS BLOB), x'00') = 0 AND trim(NEW.revoked_at) <> '' AND length(CAST(NEW.revoked_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_push_approvals
BEFORE UPDATE OF sync_key, teammate_id, working_folder_id, push_url, destination_branch ON chat_push_approvals
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.teammate_id IS NOT NEW.teammate_id COLLATE BINARY OR OLD.working_folder_id IS NOT NEW.working_folder_id COLLATE BINARY OR OLD.push_url IS NOT NEW.push_url COLLATE BINARY OR OLD.destination_branch IS NOT NEW.destination_branch COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_chat_push_approvals_insert
AFTER INSERT ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (365, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_push_approvals_update
AFTER UPDATE ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 365, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.runtime_approval_policy IS NOT NEW.runtime_approval_policy COLLATE BINARY OR OLD.safety_mode IS NOT NEW.safety_mode COLLATE BINARY OR (OLD.approved_by_participant_id IS NOT NEW.approved_by_participant_id COLLATE BINARY
                AND NOT (NEW.approved_by_participant_id IS NULL AND NOT EXISTS (SELECT 1 FROM chat_participants WHERE id = OLD.approved_by_participant_id))) OR OLD.approved_at IS NOT NEW.approved_at COLLATE BINARY OR OLD.revoked_at IS NOT NEW.revoked_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 365, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_push_approvals_delete
AFTER DELETE ON chat_push_approvals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (365, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

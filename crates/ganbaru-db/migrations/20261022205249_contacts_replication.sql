-- Contacts replicate between linked devices. The card nonce and revision move from the person
-- identity into the single row of contact_card, so any device that may write contacts can
-- rotate the card; the identity keeps only the person key, never changes, and stays on the
-- device as the anchor of the sync space. Contacts are keyed by the person public key, with
-- the id derived from it. Contact requests keep a local id; a received request records the id
-- the requester chose in peer_request_id, and duplicate pending rows from concurrent devices
-- are allowed.
CREATE TABLE contact_card (
    key TEXT PRIMARY KEY NOT NULL CHECK (key = 'card'),
    card_nonce TEXT NOT NULL CHECK (length(card_nonce) = 22),
    card_revision INTEGER NOT NULL DEFAULT 1 CHECK (card_revision >= 1),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO contact_card (key, card_nonce, card_revision, updated_at)
SELECT 'card', card_nonce, card_revision, updated_at FROM contacts_local_identity;
CREATE TRIGGER contact_card_prevent_delete
BEFORE DELETE ON contact_card
BEGIN
    SELECT RAISE(ABORT, 'The contact card cannot be deleted');
END;

CREATE TABLE contacts_local_identity_next (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    public_key TEXT NOT NULL CHECK (length(public_key) = 43),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20)
) STRICT;
INSERT INTO contacts_local_identity_next (singleton, public_key, created_at)
SELECT singleton, public_key, created_at FROM contacts_local_identity;
DROP TABLE contacts_local_identity;
ALTER TABLE contacts_local_identity_next RENAME TO contacts_local_identity;
CREATE TRIGGER contacts_local_identity_immutable_update
BEFORE UPDATE ON contacts_local_identity
BEGIN
    SELECT RAISE(ABORT, 'The person identity cannot change');
END;
CREATE TRIGGER contacts_local_identity_immutable_delete
BEFORE DELETE ON contacts_local_identity
BEGIN
    SELECT RAISE(ABORT, 'The person identity cannot change');
END;

CREATE TABLE contacts_next (
    public_key TEXT PRIMARY KEY NOT NULL CHECK (length(public_key) = 43),
    id TEXT NOT NULL CHECK (id = 'contact-' || public_key),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
    color INTEGER NOT NULL DEFAULT 30 CHECK (color BETWEEN 0 AND 31),
    state TEXT NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'blocked')),
    invite_trust TEXT NOT NULL DEFAULT 'not_allowed' CHECK (
        invite_trust IN ('not_allowed', 'once', 'seven_days', 'thirty_days', 'until_revoked')
    ),
    invite_trust_expires_at TEXT CHECK (
        invite_trust_expires_at IS NULL OR length(invite_trust_expires_at) >= 20
    ),
    message_trust TEXT NOT NULL DEFAULT 'not_allowed' CHECK (
        message_trust IN ('not_allowed', 'once', 'seven_days', 'thirty_days', 'until_revoked')
    ),
    message_trust_expires_at TEXT CHECK (
        message_trust_expires_at IS NULL OR length(message_trust_expires_at) >= 20
    ),
    accepted_at TEXT CHECK (accepted_at IS NULL OR length(accepted_at) >= 20),
    blocked_at TEXT CHECK (blocked_at IS NULL OR length(blocked_at) >= 20),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO contacts_next (
    public_key, id, display_name, color, state, invite_trust, invite_trust_expires_at,
    message_trust, message_trust_expires_at, accepted_at, blocked_at, revision, created_at,
    updated_at
)
SELECT
    public_key, 'contact-' || public_key, display_name, color, state, invite_trust,
    invite_trust_expires_at, message_trust, message_trust_expires_at, accepted_at, blocked_at,
    revision, created_at, updated_at
FROM contacts;
DROP TABLE contacts;
ALTER TABLE contacts_next RENAME TO contacts;
CREATE UNIQUE INDEX idx_contacts_id ON contacts(id);
CREATE INDEX idx_contacts_state_name ON contacts(state, lower(display_name));
CREATE TRIGGER contacts_revision_after_update
AFTER UPDATE ON contacts
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE contacts SET revision = OLD.revision + 1 WHERE public_key = NEW.public_key;
END;

CREATE TABLE contact_requests_next (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 160),
    direction TEXT NOT NULL CHECK (direction IN ('sent', 'received')),
    public_key TEXT NOT NULL CHECK (length(public_key) = 43),
    peer_request_id TEXT CHECK (
        peer_request_id IS NULL OR length(peer_request_id) BETWEEN 1 AND 160
    ),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) BETWEEN 1 AND 160),
    color INTEGER NOT NULL DEFAULT 30 CHECK (color BETWEEN 0 AND 31),
    card TEXT NOT NULL CHECK (length(card) BETWEEN 1 AND 1024),
    card_digest TEXT NOT NULL CHECK (length(card_digest) = 64),
    endpoint_hint TEXT NOT NULL DEFAULT '' CHECK (length(endpoint_hint) <= 64),
    coordinator_fingerprint TEXT NOT NULL DEFAULT '' CHECK (
        length(coordinator_fingerprint) IN (0, 64)
    ),
    invite_trust TEXT NOT NULL DEFAULT 'not_allowed' CHECK (
        invite_trust IN ('not_allowed', 'once', 'seven_days', 'thirty_days', 'until_revoked')
    ),
    message_trust TEXT NOT NULL DEFAULT 'not_allowed' CHECK (
        message_trust IN ('not_allowed', 'once', 'seven_days', 'thirty_days', 'until_revoked')
    ),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (
        state IN ('pending', 'accepted', 'declined', 'expired')
    ),
    expires_at TEXT NOT NULL CHECK (length(expires_at) >= 20),
    last_attempt_at TEXT CHECK (last_attempt_at IS NULL OR length(last_attempt_at) >= 20),
    last_error_code TEXT CHECK (last_error_code IS NULL OR length(last_error_code) <= 64),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;
INSERT INTO contact_requests_next (
    id, direction, public_key, peer_request_id, display_name, color, card, card_digest,
    endpoint_hint, coordinator_fingerprint, invite_trust, message_trust, state, expires_at,
    last_attempt_at, last_error_code, revision, created_at, updated_at
)
SELECT
    id, direction, public_key, CASE direction WHEN 'received' THEN id END, display_name, color,
    card, card_digest, endpoint_hint, coordinator_fingerprint, invite_trust, message_trust,
    state, expires_at, last_attempt_at, last_error_code, revision, created_at, updated_at
FROM contact_requests;
DROP TABLE contact_requests;
ALTER TABLE contact_requests_next RENAME TO contact_requests;
CREATE INDEX idx_contact_requests_peer ON contact_requests(direction, public_key, state);
CREATE INDEX idx_contact_requests_peer_request
ON contact_requests(public_key, peer_request_id);
CREATE INDEX idx_contact_requests_state
ON contact_requests(direction, state, expires_at);
CREATE TRIGGER contact_requests_revision_after_update
AFTER UPDATE ON contact_requests
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE contact_requests SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.
CREATE TRIGGER sync_values_contacts_insert
BEFORE INSERT ON contacts
WHEN ((typeof(NEW.public_key) = 'text' AND instr(CAST(NEW.public_key AS BLOB), x'00') = 0 AND trim(NEW.public_key) <> '' AND length(CAST(NEW.public_key AS BLOB)) <= 128)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 7 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.accepted_at IS NULL OR (typeof(NEW.accepted_at) = 'text' AND instr(CAST(NEW.accepted_at AS BLOB), x'00') = 0 AND trim(NEW.accepted_at) <> '' AND length(CAST(NEW.accepted_at AS BLOB)) <= 64))
    AND (NEW.blocked_at IS NULL OR (typeof(NEW.blocked_at) = 'text' AND instr(CAST(NEW.blocked_at AS BLOB), x'00') = 0 AND trim(NEW.blocked_at) <> '' AND length(CAST(NEW.blocked_at AS BLOB)) <= 64))
    AND (typeof(NEW.invite_trust) = 'text' AND instr(CAST(NEW.invite_trust AS BLOB), x'00') = 0 AND length(NEW.invite_trust) <= 13 AND length(CAST(NEW.invite_trust AS BLOB)) <= 262144)
    AND (NEW.invite_trust_expires_at IS NULL OR (typeof(NEW.invite_trust_expires_at) = 'text' AND instr(CAST(NEW.invite_trust_expires_at AS BLOB), x'00') = 0 AND trim(NEW.invite_trust_expires_at) <> '' AND length(CAST(NEW.invite_trust_expires_at AS BLOB)) <= 64))
    AND (typeof(NEW.message_trust) = 'text' AND instr(CAST(NEW.message_trust AS BLOB), x'00') = 0 AND length(NEW.message_trust) <= 13 AND length(CAST(NEW.message_trust AS BLOB)) <= 262144)
    AND (NEW.message_trust_expires_at IS NULL OR (typeof(NEW.message_trust_expires_at) = 'text' AND instr(CAST(NEW.message_trust_expires_at AS BLOB), x'00') = 0 AND trim(NEW.message_trust_expires_at) <> '' AND length(CAST(NEW.message_trust_expires_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_contacts_update
BEFORE UPDATE OF created_at, display_name, color, state, accepted_at, blocked_at, invite_trust, invite_trust_expires_at, message_trust, message_trust_expires_at, updated_at ON contacts
WHEN ((typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 7 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.accepted_at IS NULL OR (typeof(NEW.accepted_at) = 'text' AND instr(CAST(NEW.accepted_at AS BLOB), x'00') = 0 AND trim(NEW.accepted_at) <> '' AND length(CAST(NEW.accepted_at AS BLOB)) <= 64))
    AND (NEW.blocked_at IS NULL OR (typeof(NEW.blocked_at) = 'text' AND instr(CAST(NEW.blocked_at AS BLOB), x'00') = 0 AND trim(NEW.blocked_at) <> '' AND length(CAST(NEW.blocked_at AS BLOB)) <= 64))
    AND (typeof(NEW.invite_trust) = 'text' AND instr(CAST(NEW.invite_trust AS BLOB), x'00') = 0 AND length(NEW.invite_trust) <= 13 AND length(CAST(NEW.invite_trust AS BLOB)) <= 262144)
    AND (NEW.invite_trust_expires_at IS NULL OR (typeof(NEW.invite_trust_expires_at) = 'text' AND instr(CAST(NEW.invite_trust_expires_at AS BLOB), x'00') = 0 AND trim(NEW.invite_trust_expires_at) <> '' AND length(CAST(NEW.invite_trust_expires_at AS BLOB)) <= 64))
    AND (typeof(NEW.message_trust) = 'text' AND instr(CAST(NEW.message_trust AS BLOB), x'00') = 0 AND length(NEW.message_trust) <= 13 AND length(CAST(NEW.message_trust AS BLOB)) <= 262144)
    AND (NEW.message_trust_expires_at IS NULL OR (typeof(NEW.message_trust_expires_at) = 'text' AND instr(CAST(NEW.message_trust_expires_at AS BLOB), x'00') = 0 AND trim(NEW.message_trust_expires_at) <> '' AND length(CAST(NEW.message_trust_expires_at AS BLOB)) <= 64))
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_contacts
BEFORE UPDATE OF public_key ON contacts
WHEN OLD.public_key IS NOT NEW.public_key COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_contacts_insert
AFTER INSERT ON contacts
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (32, NEW.public_key, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_contacts_update
AFTER UPDATE ON contacts
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 32, NEW.public_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.color IS NOT NEW.color
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.accepted_at IS NOT NEW.accepted_at COLLATE BINARY OR OLD.blocked_at IS NOT NEW.blocked_at COLLATE BINARY OR OLD.invite_trust IS NOT NEW.invite_trust COLLATE BINARY OR OLD.invite_trust_expires_at IS NOT NEW.invite_trust_expires_at COLLATE BINARY OR OLD.message_trust IS NOT NEW.message_trust COLLATE BINARY OR OLD.message_trust_expires_at IS NOT NEW.message_trust_expires_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_contacts_delete
AFTER DELETE ON contacts
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (32, OLD.public_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_contact_requests_insert
BEFORE INSERT ON contact_requests
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.direction) = 'text' AND instr(CAST(NEW.direction AS BLOB), x'00') = 0 AND length(NEW.direction) <= 8 AND length(CAST(NEW.direction AS BLOB)) <= 262144)
    AND (typeof(NEW.public_key) = 'text' AND instr(CAST(NEW.public_key AS BLOB), x'00') = 0 AND length(NEW.public_key) <= 43 AND length(CAST(NEW.public_key AS BLOB)) <= 262144)
    AND (NEW.peer_request_id IS NULL OR (typeof(NEW.peer_request_id) = 'text' AND instr(CAST(NEW.peer_request_id AS BLOB), x'00') = 0 AND length(NEW.peer_request_id) <= 160 AND length(CAST(NEW.peer_request_id AS BLOB)) <= 262144))
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.card) = 'text' AND instr(CAST(NEW.card AS BLOB), x'00') = 0 AND length(NEW.card) <= 1024 AND length(CAST(NEW.card AS BLOB)) <= 262144)
    AND (typeof(NEW.card_digest) = 'text' AND instr(CAST(NEW.card_digest AS BLOB), x'00') = 0 AND length(NEW.card_digest) <= 64 AND length(CAST(NEW.card_digest AS BLOB)) <= 262144)
    AND (typeof(NEW.endpoint_hint) = 'text' AND instr(CAST(NEW.endpoint_hint AS BLOB), x'00') = 0 AND length(NEW.endpoint_hint) <= 64 AND length(CAST(NEW.endpoint_hint AS BLOB)) <= 262144)
    AND (typeof(NEW.coordinator_fingerprint) = 'text' AND instr(CAST(NEW.coordinator_fingerprint AS BLOB), x'00') = 0 AND length(NEW.coordinator_fingerprint) <= 64 AND length(CAST(NEW.coordinator_fingerprint AS BLOB)) <= 262144)
    AND (typeof(NEW.expires_at) = 'text' AND instr(CAST(NEW.expires_at AS BLOB), x'00') = 0 AND trim(NEW.expires_at) <> '' AND length(CAST(NEW.expires_at AS BLOB)) <= 64)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 8 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (typeof(NEW.invite_trust) = 'text' AND instr(CAST(NEW.invite_trust AS BLOB), x'00') = 0 AND length(NEW.invite_trust) <= 13 AND length(CAST(NEW.invite_trust AS BLOB)) <= 262144)
    AND (typeof(NEW.message_trust) = 'text' AND instr(CAST(NEW.message_trust AS BLOB), x'00') = 0 AND length(NEW.message_trust) <= 13 AND length(CAST(NEW.message_trust AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_contact_requests_update
BEFORE UPDATE OF direction, public_key, peer_request_id, display_name, color, card, card_digest, endpoint_hint, coordinator_fingerprint, expires_at, created_at, state, invite_trust, message_trust, updated_at ON contact_requests
WHEN ((typeof(NEW.direction) = 'text' AND instr(CAST(NEW.direction AS BLOB), x'00') = 0 AND length(NEW.direction) <= 8 AND length(CAST(NEW.direction AS BLOB)) <= 262144)
    AND (typeof(NEW.public_key) = 'text' AND instr(CAST(NEW.public_key AS BLOB), x'00') = 0 AND length(NEW.public_key) <= 43 AND length(CAST(NEW.public_key AS BLOB)) <= 262144)
    AND (NEW.peer_request_id IS NULL OR (typeof(NEW.peer_request_id) = 'text' AND instr(CAST(NEW.peer_request_id AS BLOB), x'00') = 0 AND length(NEW.peer_request_id) <= 160 AND length(CAST(NEW.peer_request_id AS BLOB)) <= 262144))
    AND (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(trim(NEW.display_name)) BETWEEN 1 AND 160 AND length(CAST(NEW.display_name AS BLOB)) <= 262144)
    AND (typeof(NEW.color) = 'integer' AND NEW.color BETWEEN 0 AND 31)
    AND (typeof(NEW.card) = 'text' AND instr(CAST(NEW.card AS BLOB), x'00') = 0 AND length(NEW.card) <= 1024 AND length(CAST(NEW.card AS BLOB)) <= 262144)
    AND (typeof(NEW.card_digest) = 'text' AND instr(CAST(NEW.card_digest AS BLOB), x'00') = 0 AND length(NEW.card_digest) <= 64 AND length(CAST(NEW.card_digest AS BLOB)) <= 262144)
    AND (typeof(NEW.endpoint_hint) = 'text' AND instr(CAST(NEW.endpoint_hint AS BLOB), x'00') = 0 AND length(NEW.endpoint_hint) <= 64 AND length(CAST(NEW.endpoint_hint AS BLOB)) <= 262144)
    AND (typeof(NEW.coordinator_fingerprint) = 'text' AND instr(CAST(NEW.coordinator_fingerprint AS BLOB), x'00') = 0 AND length(NEW.coordinator_fingerprint) <= 64 AND length(CAST(NEW.coordinator_fingerprint AS BLOB)) <= 262144)
    AND (typeof(NEW.expires_at) = 'text' AND instr(CAST(NEW.expires_at AS BLOB), x'00') = 0 AND trim(NEW.expires_at) <> '' AND length(CAST(NEW.expires_at AS BLOB)) <= 64)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 8 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (typeof(NEW.invite_trust) = 'text' AND instr(CAST(NEW.invite_trust AS BLOB), x'00') = 0 AND length(NEW.invite_trust) <= 13 AND length(CAST(NEW.invite_trust AS BLOB)) <= 262144)
    AND (typeof(NEW.message_trust) = 'text' AND instr(CAST(NEW.message_trust AS BLOB), x'00') = 0 AND length(NEW.message_trust) <= 13 AND length(CAST(NEW.message_trust AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_contact_requests
BEFORE UPDATE OF id, direction, public_key, peer_request_id, display_name, color, card, card_digest, endpoint_hint, coordinator_fingerprint, expires_at, created_at ON contact_requests
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.direction IS NOT NEW.direction COLLATE BINARY OR OLD.public_key IS NOT NEW.public_key COLLATE BINARY OR OLD.peer_request_id IS NOT NEW.peer_request_id COLLATE BINARY OR OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.color IS NOT NEW.color OR OLD.card IS NOT NEW.card COLLATE BINARY OR OLD.card_digest IS NOT NEW.card_digest COLLATE BINARY OR OLD.endpoint_hint IS NOT NEW.endpoint_hint COLLATE BINARY OR OLD.coordinator_fingerprint IS NOT NEW.coordinator_fingerprint COLLATE BINARY OR OLD.expires_at IS NOT NEW.expires_at COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_contact_requests
BEFORE INSERT ON contact_requests
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 33 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM contact_requests WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_contact_requests_insert
AFTER INSERT ON contact_requests
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (33, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_contact_requests_update
AFTER UPDATE ON contact_requests
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 33, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.invite_trust IS NOT NEW.invite_trust COLLATE BINARY OR OLD.message_trust IS NOT NEW.message_trust COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_contact_requests_delete
AFTER DELETE ON contact_requests
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (33, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_contact_card_insert
BEFORE INSERT ON contact_card
WHEN ((typeof(NEW.key) = 'text' AND instr(CAST(NEW.key AS BLOB), x'00') = 0 AND trim(NEW.key) <> '' AND length(CAST(NEW.key AS BLOB)) <= 128)
    AND (typeof(NEW.card_nonce) = 'text' AND instr(CAST(NEW.card_nonce AS BLOB), x'00') = 0 AND length(NEW.card_nonce) <= 22 AND length(CAST(NEW.card_nonce AS BLOB)) <= 262144)
    AND (typeof(NEW.card_revision) = 'integer' AND NEW.card_revision BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_contact_card_update
BEFORE UPDATE OF card_nonce, card_revision, updated_at ON contact_card
WHEN ((typeof(NEW.card_nonce) = 'text' AND instr(CAST(NEW.card_nonce AS BLOB), x'00') = 0 AND length(NEW.card_nonce) <= 22 AND length(CAST(NEW.card_nonce AS BLOB)) <= 262144)
    AND (typeof(NEW.card_revision) = 'integer' AND NEW.card_revision BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_contact_card
BEFORE UPDATE OF key ON contact_card
WHEN OLD.key IS NOT NEW.key COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_contact_card
BEFORE INSERT ON contact_card
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 34 AND row_key = NEW.key)
    AND NOT EXISTS (SELECT 1 FROM contact_card WHERE key = NEW.key)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_contact_card_insert
AFTER INSERT ON contact_card
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (34, NEW.key, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_contact_card_update
AFTER UPDATE ON contact_card
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 34, NEW.key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.card_nonce IS NOT NEW.card_nonce COLLATE BINARY OR OLD.card_revision IS NOT NEW.card_revision
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.updated_at IS NOT NEW.updated_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_contact_card_delete
AFTER DELETE ON contact_card
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (34, OLD.key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

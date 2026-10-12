-- Contacts: the local person's identity, contacts with trust scopes, and contact requests.
-- Private keys never live here; they stay in native credential storage per device.

CREATE TABLE contacts_local_identity (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    public_key TEXT NOT NULL CHECK (length(public_key) = 43),
    card_nonce TEXT NOT NULL CHECK (length(card_nonce) = 22),
    card_revision INTEGER NOT NULL DEFAULT 1 CHECK (card_revision >= 1),
    created_at TEXT NOT NULL CHECK (length(created_at) >= 20),
    updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20)
) STRICT;

CREATE TABLE contacts (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 160),
    public_key TEXT NOT NULL UNIQUE CHECK (length(public_key) = 43),
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
CREATE INDEX idx_contacts_state_name ON contacts(state, lower(display_name));
CREATE TRIGGER contacts_revision_after_update
AFTER UPDATE ON contacts
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE contacts SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

CREATE TABLE contact_requests (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 160),
    direction TEXT NOT NULL CHECK (direction IN ('sent', 'received')),
    public_key TEXT NOT NULL CHECK (length(public_key) = 43),
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
CREATE UNIQUE INDEX idx_contact_requests_pending_peer
ON contact_requests(direction, public_key)
WHERE state = 'pending';
CREATE INDEX idx_contact_requests_state
ON contact_requests(direction, state, expires_at);
CREATE TRIGGER contact_requests_revision_after_update
AFTER UPDATE ON contact_requests
WHEN NEW.revision = OLD.revision
BEGIN
    UPDATE contact_requests SET revision = OLD.revision + 1 WHERE id = NEW.id;
END;

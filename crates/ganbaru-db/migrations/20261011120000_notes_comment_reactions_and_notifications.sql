-- Notes comments: emoji reactions, per-person discussion mutes, and per-page comment notification levels.

-- One row per person and emoji on a comment. The author name is captured like comment authors so reactions
-- from people without a local user row still render.
CREATE TABLE IF NOT EXISTS notes_comment_reactions (
    comment_id TEXT NOT NULL REFERENCES notes_comments(id) ON DELETE CASCADE,
    created_by TEXT NOT NULL CHECK (trim(created_by) <> ''),
    display_name TEXT NOT NULL CHECK (json_valid(display_name)),
    emoji TEXT NOT NULL CHECK (trim(emoji) <> '' AND length(emoji) <= 16),
    created_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(created_time) <> ''),
    PRIMARY KEY (comment_id, created_by, emoji)
);

-- Device-local preference: a muted discussion stops notifying this person about its comments.
CREATE TABLE IF NOT EXISTS notes_comment_thread_mutes (
    thread_id TEXT NOT NULL REFERENCES notes_comment_threads(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES notes_local_users(id) ON DELETE CASCADE,
    muted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(muted_at) <> ''),
    PRIMARY KEY (thread_id, user_id)
);
CREATE INDEX IF NOT EXISTS idx_notes_comment_thread_mutes_user
    ON notes_comment_thread_mutes(user_id, thread_id);

-- Device-local preference: which notifications from one page reach this person. No row means 'all'.
CREATE TABLE IF NOT EXISTS notes_page_notification_settings (
    page_id TEXT NOT NULL REFERENCES notes_pages(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES notes_local_users(id) ON DELETE CASCADE,
    level TEXT NOT NULL CHECK (level IN ('all', 'mentions', 'none')),
    updated_time TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        CHECK (trim(updated_time) <> ''),
    PRIMARY KEY (page_id, user_id)
);

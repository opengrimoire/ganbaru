-- A context package counts the live channel notes of its channel that did not fit the notes
-- budget, beside its excluded replies and channel messages.
ALTER TABLE chat_assignment_context_packages
ADD COLUMN excluded_channel_note_count INTEGER NOT NULL DEFAULT 0
    CHECK (excluded_channel_note_count >= 0);

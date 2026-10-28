-- Device attachment rows record whether this device imported the attachment or created the row for an
-- attachment a replicated message revision names. Rows of the second kind exist only so received attachments
-- read like local ones, so the sync hook removes them once no reference on this device names them.
ALTER TABLE chat_attachments ADD COLUMN origin TEXT NOT NULL DEFAULT 'imported'
    CHECK (origin IN ('imported', 'replicated'));

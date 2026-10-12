-- Notes date reminders can fire a set number of minutes before their date. The offset is copied from the mention so
-- the scheduler can subtract it from the reminder's floating wall time; 0 fires at the date itself.
ALTER TABLE notes_mention_notifications
    ADD COLUMN remind_minutes_before INTEGER NOT NULL DEFAULT 0
        CHECK (remind_minutes_before BETWEEN 0 AND 40320);

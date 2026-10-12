-- A task's start and due dates can each remind a set number of minutes before them, with the same range Notes date
-- reminders use; 0 fires at the date itself. A reminder needs its date, as a time does.
ALTER TABLE project_tasks
    ADD COLUMN start_reminder_minutes INTEGER
        CHECK (start_reminder_minutes IS NULL OR (start_date IS NOT NULL AND start_reminder_minutes BETWEEN 0 AND 40320));
ALTER TABLE project_tasks
    ADD COLUMN due_reminder_minutes INTEGER
        CHECK (due_reminder_minutes IS NULL OR (due_date IS NOT NULL AND due_reminder_minutes BETWEEN 0 AND 40320));

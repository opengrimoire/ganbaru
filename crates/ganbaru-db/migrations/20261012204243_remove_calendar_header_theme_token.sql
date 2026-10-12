-- Calendar day and time headers paint the Calendar surface, so user themes no longer carry a separate header color.
DELETE FROM theme_tokens WHERE kind = 'app' AND key = '--cal-header-bg';
DELETE FROM theme_seed_tokens WHERE kind = 'app' AND key = '--cal-header-bg';

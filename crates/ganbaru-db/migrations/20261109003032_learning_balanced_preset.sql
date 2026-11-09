-- Plan the built-in Learning project with the Balanced preset, since Adaptive is experimental and
-- hidden by default. A Learning project whose Focus defaults were changed from the seed keeps them.
-- The change is captured for sync only once projects (table 97) are seeded; before that, seeding
-- publishes the row with its current value, as it does for every other seeded row.
UPDATE sync_apply_state
SET applying = 1
WHERE singleton = 1
  AND NOT EXISTS (SELECT 1 FROM sync_seeded_tables WHERE table_id = 97);

UPDATE projects
SET default_pomodoro_preset_key = 'balanced',
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id = 'project-routine-learning'
  AND default_pomodoro_mode = 'preset'
  AND default_pomodoro_preset_key = 'adaptive';

UPDATE sync_apply_state SET applying = 0 WHERE singleton = 1;

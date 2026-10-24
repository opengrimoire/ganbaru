-- no-transaction
-- Distraction block history replicates through the vault manifest. An event during a Focus run
-- publishes with the closed run and is deleted with it, so run_id cascades; an event outside a
-- run publishes at once. segment_id still clears, as on Focus run events, because segments are
-- deleted only with their run. Rule snapshots take an ordinal fixed at zero so they are owned
-- rows keyed by their event; their rule kind and blocker mode are checked by the writers and the
-- sync adapter instead of a CHECK constraint.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE distractions_block_events_next (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    run_id TEXT REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    segment_id TEXT REFERENCES pomodoro_segments(id) ON DELETE SET NULL,
    occurred_at TEXT NOT NULL CHECK (trim(occurred_at) <> ''),
    source_type TEXT NOT NULL CHECK (source_type IN ('browser', 'desktop_app', 'mobile_app')),
    source_key TEXT NOT NULL CHECK (trim(source_key) <> '' AND instr(source_key, '://') = 0),
    display_name TEXT,
    phase TEXT CHECK (
        phase IS NULL OR
        phase IN ('focus', 'short_break', 'long_break', 'manual_pause', 'idle_pause', 'suspend_pause')
    ),
    decision TEXT NOT NULL CHECK (
        decision IN ('blocked', 'temporary_allowed', 'false_positive_reported', 'limit_exhausted')
    ),
    rule_id TEXT,
    category_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO distractions_block_events_next (
    id, run_id, segment_id, occurred_at, source_type, source_key, display_name, phase, decision,
    rule_id, category_id, created_at
)
SELECT
    id, run_id, segment_id, occurred_at, source_type, source_key, display_name, phase, decision,
    rule_id, category_id, created_at
FROM distractions_block_events;
DROP TABLE distractions_block_events;
ALTER TABLE distractions_block_events_next RENAME TO distractions_block_events;
CREATE INDEX idx_distractions_block_events_run
ON distractions_block_events(run_id, occurred_at);
CREATE INDEX idx_distractions_block_events_source
ON distractions_block_events(source_type, source_key, occurred_at);

CREATE TABLE distractions_block_event_rule_snapshots_next (
    block_event_id TEXT NOT NULL REFERENCES distractions_block_events(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    rule_id TEXT,
    rule_kind TEXT,
    rule_label TEXT,
    environment_id TEXT,
    blocker_mode TEXT,
    PRIMARY KEY (block_event_id, ordinal)
);
INSERT INTO distractions_block_event_rule_snapshots_next (
    block_event_id, ordinal, rule_id, rule_kind, rule_label, environment_id, blocker_mode
)
SELECT block_event_id, 0, rule_id, rule_kind, rule_label, environment_id, blocker_mode
FROM distractions_block_event_rule_snapshots;
DROP TABLE distractions_block_event_rule_snapshots;
ALTER TABLE distractions_block_event_rule_snapshots_next
RENAME TO distractions_block_event_rule_snapshots;

CREATE TEMP TABLE distraction_block_history_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO distraction_block_history_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE distraction_block_history_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

DROP TRIGGER IF EXISTS sync_capture_pomodoro_runs_update;
CREATE TRIGGER sync_capture_pomodoro_runs_update
AFTER UPDATE ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM (SELECT NEW.id AS id, NEW.original_event_id AS original_event_id, NEW.event_date AS event_date, NEW.planned_start AS planned_start, NEW.planned_end AS planned_end, NEW.rhythm_kind AS rhythm_kind, NEW.rhythm_source AS rhythm_source, NEW.preset_key AS preset_key, NEW.idle_timeout_minutes AS idle_timeout_minutes, NEW.event_title_snapshot AS event_title_snapshot, NEW.start_trigger AS start_trigger, NEW.created_at AS created_at, NEW.started_at AS started_at, NEW.ended_at AS ended_at, NEW.end_reason AS end_reason, NEW.last_heartbeat AS last_heartbeat, NEW.inherited_focus_minutes AS inherited_focus_minutes, NEW.inherited_rhythm_position AS inherited_rhythm_position, NEW.inherited_focus_milliseconds AS inherited_focus_milliseconds, NEW.inherited_phase_milliseconds AS inherited_phase_milliseconds, NEW.inherited_from_run_id AS inherited_from_run_id, NEW.event_id AS event_id, NEW.current_occurrence_id AS current_occurrence_id, NEW.current_event_date AS current_event_date, NEW.current_event_title AS current_event_title, NEW.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.original_event_id IS NOT NEW.original_event_id COLLATE BINARY OR OLD.event_date IS NOT NEW.event_date COLLATE BINARY OR OLD.planned_start IS NOT NEW.planned_start COLLATE BINARY OR OLD.planned_end IS NOT NEW.planned_end COLLATE BINARY OR OLD.rhythm_kind IS NOT NEW.rhythm_kind COLLATE BINARY OR OLD.rhythm_source IS NOT NEW.rhythm_source COLLATE BINARY OR OLD.preset_key IS NOT NEW.preset_key COLLATE BINARY OR OLD.idle_timeout_minutes IS NOT NEW.idle_timeout_minutes OR OLD.event_title_snapshot IS NOT NEW.event_title_snapshot COLLATE BINARY OR OLD.start_trigger IS NOT NEW.start_trigger COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 1 ELSE 0 END)
            | (CASE WHEN OLD.started_at IS NOT NEW.started_at COLLATE BINARY OR OLD.ended_at IS NOT NEW.ended_at COLLATE BINARY OR OLD.end_reason IS NOT NEW.end_reason COLLATE BINARY OR OLD.last_heartbeat IS NOT NEW.last_heartbeat COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.inherited_focus_minutes IS NOT NEW.inherited_focus_minutes OR OLD.inherited_rhythm_position IS NOT NEW.inherited_rhythm_position OR OLD.inherited_focus_milliseconds IS NOT NEW.inherited_focus_milliseconds OR OLD.inherited_phase_milliseconds IS NOT NEW.inherited_phase_milliseconds
                THEN 4 ELSE 0 END)
            | (CASE WHEN (OLD.event_id IS NOT NEW.event_id COLLATE BINARY
                AND NOT (NEW.event_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendar_events WHERE id = OLD.event_id))) OR OLD.current_occurrence_id IS NOT NEW.current_occurrence_id COLLATE BINARY OR OLD.current_event_date IS NOT NEW.current_event_date COLLATE BINARY OR OLD.current_event_title IS NOT NEW.current_event_title COLLATE BINARY OR (OLD.calendar_archive_id IS NOT NEW.calendar_archive_id COLLATE BINARY
                AND NOT (NEW.calendar_archive_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendar_event_archives WHERE id = OLD.calendar_archive_id)))
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE NOT EXISTS (SELECT 1 FROM (SELECT OLD.id AS id, OLD.original_event_id AS original_event_id, OLD.event_date AS event_date, OLD.planned_start AS planned_start, OLD.planned_end AS planned_end, OLD.rhythm_kind AS rhythm_kind, OLD.rhythm_source AS rhythm_source, OLD.preset_key AS preset_key, OLD.idle_timeout_minutes AS idle_timeout_minutes, OLD.event_title_snapshot AS event_title_snapshot, OLD.start_trigger AS start_trigger, OLD.created_at AS created_at, OLD.started_at AS started_at, OLD.ended_at AS ended_at, OLD.end_reason AS end_reason, OLD.last_heartbeat AS last_heartbeat, OLD.inherited_focus_minutes AS inherited_focus_minutes, OLD.inherited_rhythm_position AS inherited_rhythm_position, OLD.inherited_focus_milliseconds AS inherited_focus_milliseconds, OLD.inherited_phase_milliseconds AS inherited_phase_milliseconds, OLD.inherited_from_run_id AS inherited_from_run_id, OLD.event_id AS event_id, OLD.current_occurrence_id AS current_occurrence_id, OLD.current_event_date AS current_event_date, OLD.current_event_title AS current_event_title, OLD.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 225, child.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM pomodoro_segments AS child
    WHERE child.run_id = NEW.id AND NOT EXISTS (SELECT 1 FROM (SELECT OLD.id AS id, OLD.original_event_id AS original_event_id, OLD.event_date AS event_date, OLD.planned_start AS planned_start, OLD.planned_end AS planned_end, OLD.rhythm_kind AS rhythm_kind, OLD.rhythm_source AS rhythm_source, OLD.preset_key AS preset_key, OLD.idle_timeout_minutes AS idle_timeout_minutes, OLD.event_title_snapshot AS event_title_snapshot, OLD.start_trigger AS start_trigger, OLD.created_at AS created_at, OLD.started_at AS started_at, OLD.ended_at AS ended_at, OLD.end_reason AS end_reason, OLD.last_heartbeat AS last_heartbeat, OLD.inherited_focus_minutes AS inherited_focus_minutes, OLD.inherited_rhythm_position AS inherited_rhythm_position, OLD.inherited_focus_milliseconds AS inherited_focus_milliseconds, OLD.inherited_phase_milliseconds AS inherited_phase_milliseconds, OLD.inherited_from_run_id AS inherited_from_run_id, OLD.event_id AS event_id, OLD.current_occurrence_id AS current_occurrence_id, OLD.current_event_date AS current_event_date, OLD.current_event_title AS current_event_title, OLD.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 226, child.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM pomodoro_run_events AS child
    WHERE child.run_id = NEW.id AND NOT EXISTS (SELECT 1 FROM (SELECT OLD.id AS id, OLD.original_event_id AS original_event_id, OLD.event_date AS event_date, OLD.planned_start AS planned_start, OLD.planned_end AS planned_end, OLD.rhythm_kind AS rhythm_kind, OLD.rhythm_source AS rhythm_source, OLD.preset_key AS preset_key, OLD.idle_timeout_minutes AS idle_timeout_minutes, OLD.event_title_snapshot AS event_title_snapshot, OLD.start_trigger AS start_trigger, OLD.created_at AS created_at, OLD.started_at AS started_at, OLD.ended_at AS ended_at, OLD.end_reason AS end_reason, OLD.last_heartbeat AS last_heartbeat, OLD.inherited_focus_minutes AS inherited_focus_minutes, OLD.inherited_rhythm_position AS inherited_rhythm_position, OLD.inherited_focus_milliseconds AS inherited_focus_milliseconds, OLD.inherited_phase_milliseconds AS inherited_phase_milliseconds, OLD.inherited_from_run_id AS inherited_from_run_id, OLD.event_id AS event_id, OLD.current_occurrence_id AS current_occurrence_id, OLD.current_event_date AS current_event_date, OLD.current_event_title AS current_event_title, OLD.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 279, child.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM distractions_block_events AS child
    WHERE child.run_id = NEW.id AND NOT EXISTS (SELECT 1 FROM (SELECT OLD.id AS id, OLD.original_event_id AS original_event_id, OLD.event_date AS event_date, OLD.planned_start AS planned_start, OLD.planned_end AS planned_end, OLD.rhythm_kind AS rhythm_kind, OLD.rhythm_source AS rhythm_source, OLD.preset_key AS preset_key, OLD.idle_timeout_minutes AS idle_timeout_minutes, OLD.event_title_snapshot AS event_title_snapshot, OLD.start_trigger AS start_trigger, OLD.created_at AS created_at, OLD.started_at AS started_at, OLD.ended_at AS ended_at, OLD.end_reason AS end_reason, OLD.last_heartbeat AS last_heartbeat, OLD.inherited_focus_minutes AS inherited_focus_minutes, OLD.inherited_rhythm_position AS inherited_rhythm_position, OLD.inherited_focus_milliseconds AS inherited_focus_milliseconds, OLD.inherited_phase_milliseconds AS inherited_phase_milliseconds, OLD.inherited_from_run_id AS inherited_from_run_id, OLD.event_id AS event_id, OLD.current_occurrence_id AS current_occurrence_id, OLD.current_event_date AS current_event_date, OLD.current_event_title AS current_event_title, OLD.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;


CREATE TRIGGER sync_values_distractions_block_events_insert
BEFORE INSERT ON distractions_block_events
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 32 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (typeof(NEW.source_key) = 'text' AND instr(CAST(NEW.source_key AS BLOB), x'00') = 0 AND length(trim(NEW.source_key)) BETWEEN 1 AND 1024 AND length(CAST(NEW.source_key AS BLOB)) <= 262144)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 1024 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (NEW.phase IS NULL OR (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144))
    AND (typeof(NEW.decision) = 'text' AND instr(CAST(NEW.decision AS BLOB), x'00') = 0 AND length(NEW.decision) <= 32 AND length(CAST(NEW.decision AS BLOB)) <= 262144)
    AND (NEW.rule_id IS NULL OR (typeof(NEW.rule_id) = 'text' AND instr(CAST(NEW.rule_id AS BLOB), x'00') = 0 AND length(NEW.rule_id) <= 1024 AND length(CAST(NEW.rule_id AS BLOB)) <= 262144))
    AND (NEW.category_id IS NULL OR (typeof(NEW.category_id) = 'text' AND instr(CAST(NEW.category_id AS BLOB), x'00') = 0 AND length(NEW.category_id) <= 1024 AND length(CAST(NEW.category_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (NEW.segment_id IS NULL OR (typeof(NEW.segment_id) = 'text' AND instr(CAST(NEW.segment_id AS BLOB), x'00') = 0 AND trim(NEW.segment_id) <> '' AND length(CAST(NEW.segment_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_block_events_update
BEFORE UPDATE OF occurred_at, source_type, source_key, display_name, phase, decision, rule_id, category_id, created_at, run_id, segment_id ON distractions_block_events
WHEN ((typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)
    AND (typeof(NEW.source_type) = 'text' AND instr(CAST(NEW.source_type AS BLOB), x'00') = 0 AND length(NEW.source_type) <= 32 AND length(CAST(NEW.source_type AS BLOB)) <= 262144)
    AND (typeof(NEW.source_key) = 'text' AND instr(CAST(NEW.source_key AS BLOB), x'00') = 0 AND length(trim(NEW.source_key)) BETWEEN 1 AND 1024 AND length(CAST(NEW.source_key AS BLOB)) <= 262144)
    AND (NEW.display_name IS NULL OR (typeof(NEW.display_name) = 'text' AND instr(CAST(NEW.display_name AS BLOB), x'00') = 0 AND length(NEW.display_name) <= 1024 AND length(CAST(NEW.display_name AS BLOB)) <= 262144))
    AND (NEW.phase IS NULL OR (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144))
    AND (typeof(NEW.decision) = 'text' AND instr(CAST(NEW.decision AS BLOB), x'00') = 0 AND length(NEW.decision) <= 32 AND length(CAST(NEW.decision AS BLOB)) <= 262144)
    AND (NEW.rule_id IS NULL OR (typeof(NEW.rule_id) = 'text' AND instr(CAST(NEW.rule_id AS BLOB), x'00') = 0 AND length(NEW.rule_id) <= 1024 AND length(CAST(NEW.rule_id AS BLOB)) <= 262144))
    AND (NEW.category_id IS NULL OR (typeof(NEW.category_id) = 'text' AND instr(CAST(NEW.category_id AS BLOB), x'00') = 0 AND length(NEW.category_id) <= 1024 AND length(CAST(NEW.category_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (NEW.segment_id IS NULL OR (typeof(NEW.segment_id) = 'text' AND instr(CAST(NEW.segment_id AS BLOB), x'00') = 0 AND trim(NEW.segment_id) <> '' AND length(CAST(NEW.segment_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_distractions_block_events
BEFORE UPDATE OF id, occurred_at, source_type, source_key, display_name, phase, decision, rule_id, category_id, created_at, run_id ON distractions_block_events
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.occurred_at IS NOT NEW.occurred_at COLLATE BINARY OR OLD.source_type IS NOT NEW.source_type COLLATE BINARY OR OLD.source_key IS NOT NEW.source_key COLLATE BINARY OR OLD.display_name IS NOT NEW.display_name COLLATE BINARY OR OLD.phase IS NOT NEW.phase COLLATE BINARY OR OLD.decision IS NOT NEW.decision COLLATE BINARY OR OLD.rule_id IS NOT NEW.rule_id COLLATE BINARY OR OLD.category_id IS NOT NEW.category_id COLLATE BINARY OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY OR OLD.run_id IS NOT NEW.run_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_distractions_block_events
BEFORE INSERT ON distractions_block_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 279 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM distractions_block_events WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_distractions_block_events_insert
AFTER INSERT ON distractions_block_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (NEW.run_id IS NULL OR EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL)))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (279, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_distractions_block_events_update
AFTER UPDATE ON distractions_block_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (NEW.run_id IS NULL OR EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL)))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 279, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.segment_id IS NOT NEW.segment_id COLLATE BINARY
                AND NOT (NEW.segment_id IS NULL AND NOT EXISTS (SELECT 1 FROM pomodoro_segments WHERE id = OLD.segment_id))
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_block_events_delete
AFTER DELETE ON distractions_block_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 279 AND row_key = OLD.id)
        OR EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 279 AND row_key = OLD.id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (279, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_distractions_block_event_rule_snapshots_insert
BEFORE INSERT ON distractions_block_event_rule_snapshots
WHEN ((NEW.rule_id IS NULL OR (typeof(NEW.rule_id) = 'text' AND instr(CAST(NEW.rule_id AS BLOB), x'00') = 0 AND length(NEW.rule_id) <= 1024 AND length(CAST(NEW.rule_id AS BLOB)) <= 262144))
    AND (NEW.rule_kind IS NULL OR (typeof(NEW.rule_kind) = 'text' AND instr(CAST(NEW.rule_kind AS BLOB), x'00') = 0 AND length(NEW.rule_kind) <= 32 AND length(CAST(NEW.rule_kind AS BLOB)) <= 262144))
    AND (NEW.rule_label IS NULL OR (typeof(NEW.rule_label) = 'text' AND instr(CAST(NEW.rule_label AS BLOB), x'00') = 0 AND length(NEW.rule_label) <= 1024 AND length(CAST(NEW.rule_label AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 1024 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (NEW.blocker_mode IS NULL OR (typeof(NEW.blocker_mode) = 'text' AND instr(CAST(NEW.blocker_mode AS BLOB), x'00') = 0 AND length(NEW.blocker_mode) <= 32 AND length(CAST(NEW.blocker_mode AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_distractions_block_event_rule_snapshots_update
BEFORE UPDATE OF rule_id, rule_kind, rule_label, environment_id, blocker_mode ON distractions_block_event_rule_snapshots
WHEN ((NEW.rule_id IS NULL OR (typeof(NEW.rule_id) = 'text' AND instr(CAST(NEW.rule_id AS BLOB), x'00') = 0 AND length(NEW.rule_id) <= 1024 AND length(CAST(NEW.rule_id AS BLOB)) <= 262144))
    AND (NEW.rule_kind IS NULL OR (typeof(NEW.rule_kind) = 'text' AND instr(CAST(NEW.rule_kind AS BLOB), x'00') = 0 AND length(NEW.rule_kind) <= 32 AND length(CAST(NEW.rule_kind AS BLOB)) <= 262144))
    AND (NEW.rule_label IS NULL OR (typeof(NEW.rule_label) = 'text' AND instr(CAST(NEW.rule_label AS BLOB), x'00') = 0 AND length(NEW.rule_label) <= 1024 AND length(CAST(NEW.rule_label AS BLOB)) <= 262144))
    AND (NEW.environment_id IS NULL OR (typeof(NEW.environment_id) = 'text' AND instr(CAST(NEW.environment_id AS BLOB), x'00') = 0 AND length(NEW.environment_id) <= 1024 AND length(CAST(NEW.environment_id AS BLOB)) <= 262144))
    AND (NEW.blocker_mode IS NULL OR (typeof(NEW.blocker_mode) = 'text' AND instr(CAST(NEW.blocker_mode AS BLOB), x'00') = 0 AND length(NEW.blocker_mode) <= 32 AND length(CAST(NEW.blocker_mode AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_distractions_block_event_rule_snapshots_insert
AFTER INSERT ON distractions_block_event_rule_snapshots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 279, NEW.block_event_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM distractions_block_events AS sync_owner WHERE sync_owner.id = NEW.block_event_id AND (sync_owner.run_id IS NULL OR EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL))))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_block_event_rule_snapshots_update
AFTER UPDATE ON distractions_block_event_rule_snapshots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 279, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.block_event_id AS key UNION SELECT OLD.block_event_id) AS parent
    WHERE EXISTS (SELECT 1 FROM distractions_block_events AS sync_owner WHERE sync_owner.id = parent.key AND (sync_owner.run_id IS NULL OR EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL))))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_distractions_block_event_rule_snapshots_delete
AFTER DELETE ON distractions_block_event_rule_snapshots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 279, OLD.block_event_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM distractions_block_events AS sync_owner WHERE sync_owner.id = OLD.block_event_id AND (sync_owner.run_id IS NULL OR EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL))))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;


COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

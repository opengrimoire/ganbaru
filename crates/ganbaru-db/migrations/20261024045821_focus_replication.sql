-- no-transaction
-- Focus history replicates through the vault manifest once a run is closed: the run, its rhythm,
-- segments, pauses, and run events arrive together, and open runs never leave their device.
-- A run's inherited_from_run_id is plain text because only rows published with a run may
-- reference it. Count rhythms take an ordinal fixed at zero and pauses take their zero-based
-- position in start order, so both are owned rows keyed by their owner; pause ids are dropped.
-- Owned rows replicate through the generic codec, so a sequence step's break phase and a pause
-- reason are checked by the Focus writers and the sync adapter instead of a CHECK constraint.
-- Partial unique indexes cannot be replicated, so triggers outside apply keep one open run, one
-- active segment, and one open pause per segment. Every device records its Focus activity in
-- pomodoro_device_activity, keyed by its stable device id, so other devices can warn before a
-- second start.

PRAGMA foreign_keys = OFF;
PRAGMA legacy_alter_table = ON;

BEGIN;

CREATE TABLE pomodoro_runs_next (
    id TEXT PRIMARY KEY,
    event_id TEXT REFERENCES calendar_events(id) ON DELETE SET NULL,
    original_event_id TEXT NOT NULL,
    event_date TEXT NOT NULL,
    planned_start TEXT NOT NULL,
    planned_end TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    end_reason TEXT CHECK (end_reason IS NULL OR end_reason IN ('completed', 'stopped', 'interrupted', 'reconfigured', 'block_transition')),
    rhythm_kind TEXT NOT NULL CHECK (rhythm_kind IN ('count', 'sequence')),
    rhythm_source TEXT NOT NULL CHECK (rhythm_source IN ('preset', 'custom')),
    preset_key TEXT CHECK (
        preset_key IS NULL OR preset_key IN ('adaptive', 'creative', 'balanced', 'deep', 'extended')
    ),
    idle_timeout_minutes INTEGER,
    last_heartbeat TEXT NOT NULL,
    event_title_snapshot TEXT,
    inherited_focus_minutes INTEGER NOT NULL DEFAULT 0,
    inherited_rhythm_position INTEGER NOT NULL DEFAULT 1,
    inherited_from_run_id TEXT,
    start_trigger TEXT NOT NULL DEFAULT 'manual' CHECK (start_trigger IN ('manual', 'block_auto', 'block_transition', 'reconfigure', 'crash_recovery')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    inherited_focus_milliseconds INTEGER
        CHECK (inherited_focus_milliseconds IS NULL OR inherited_focus_milliseconds >= 0),
    inherited_phase_milliseconds INTEGER
        CHECK (inherited_phase_milliseconds IS NULL OR inherited_phase_milliseconds >= 0),
    current_occurrence_id TEXT
        CHECK (current_occurrence_id IS NULL OR trim(current_occurrence_id) <> ''),
    current_event_date TEXT
        CHECK (current_event_date IS NULL OR trim(current_event_date) <> ''),
    current_event_title TEXT,
    calendar_archive_id TEXT
        REFERENCES calendar_event_archives(id) ON DELETE SET NULL
);
INSERT INTO pomodoro_runs_next (
    id, event_id, original_event_id, event_date, planned_start, planned_end, started_at,
    ended_at, end_reason, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes,
    last_heartbeat, event_title_snapshot, inherited_focus_minutes, inherited_rhythm_position,
    inherited_from_run_id, start_trigger, created_at, inherited_focus_milliseconds,
    inherited_phase_milliseconds, current_occurrence_id, current_event_date,
    current_event_title, calendar_archive_id
)
SELECT
    id, event_id, original_event_id, event_date, planned_start, planned_end, started_at,
    ended_at, end_reason, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes,
    last_heartbeat, event_title_snapshot, inherited_focus_minutes, inherited_rhythm_position,
    inherited_from_run_id, start_trigger, created_at, inherited_focus_milliseconds,
    inherited_phase_milliseconds, current_occurrence_id, current_event_date,
    current_event_title, calendar_archive_id
FROM pomodoro_runs;
DROP TABLE pomodoro_runs;
ALTER TABLE pomodoro_runs_next RENAME TO pomodoro_runs;
CREATE INDEX idx_pomodoro_runs_calendar_archive ON pomodoro_runs(calendar_archive_id);
CREATE INDEX idx_pomodoro_runs_current_occurrence
ON pomodoro_runs(current_occurrence_id)
WHERE current_occurrence_id IS NOT NULL;
CREATE INDEX idx_pomodoro_runs_event_date ON pomodoro_runs(event_id, event_date);
CREATE INDEX idx_pomodoro_runs_open ON pomodoro_runs(ended_at);
CREATE INDEX idx_pomodoro_runs_original_event ON pomodoro_runs(original_event_id);

CREATE TABLE pomodoro_run_count_rhythms_next (
    run_id TEXT NOT NULL REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL DEFAULT 0 CHECK (ordinal = 0),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    short_break_minutes INTEGER NOT NULL CHECK (short_break_minutes > 0),
    long_break_minutes INTEGER NOT NULL CHECK (long_break_minutes > 0),
    long_break_after_focus_count INTEGER NOT NULL CHECK (
        long_break_after_focus_count >= 1 AND long_break_after_focus_count <= 12
    ),
    PRIMARY KEY (run_id, ordinal)
);
INSERT INTO pomodoro_run_count_rhythms_next (
    run_id, ordinal, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
)
SELECT
    run_id, 0, focus_duration_minutes, short_break_minutes, long_break_minutes,
    long_break_after_focus_count
FROM pomodoro_run_count_rhythms;
DROP TABLE pomodoro_run_count_rhythms;
ALTER TABLE pomodoro_run_count_rhythms_next RENAME TO pomodoro_run_count_rhythms;

CREATE TABLE pomodoro_run_sequence_steps_next (
    run_id TEXT NOT NULL REFERENCES pomodoro_runs(id) ON DELETE CASCADE,
    step_index INTEGER NOT NULL CHECK (step_index >= 0 AND step_index < 12),
    focus_duration_minutes INTEGER NOT NULL CHECK (focus_duration_minutes > 0),
    break_phase TEXT NOT NULL,
    break_duration_minutes INTEGER NOT NULL CHECK (break_duration_minutes > 0),
    PRIMARY KEY (run_id, step_index)
);
INSERT INTO pomodoro_run_sequence_steps_next (
    run_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
)
SELECT run_id, step_index, focus_duration_minutes, break_phase, break_duration_minutes
FROM pomodoro_run_sequence_steps;
DROP TABLE pomodoro_run_sequence_steps;
ALTER TABLE pomodoro_run_sequence_steps_next RENAME TO pomodoro_run_sequence_steps;

CREATE TABLE pomodoro_pauses_next (
    segment_id TEXT NOT NULL REFERENCES pomodoro_segments(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    started_at TEXT NOT NULL,
    ended_at TEXT,
    reason TEXT NOT NULL,
    detected_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (segment_id, ordinal)
);
INSERT INTO pomodoro_pauses_next (
    segment_id, ordinal, started_at, ended_at, reason, detected_at, created_at
)
SELECT
    segment_id,
    ROW_NUMBER() OVER (PARTITION BY segment_id ORDER BY started_at, id) - 1,
    started_at, ended_at, reason, detected_at, created_at
FROM pomodoro_pauses;
DROP TABLE pomodoro_pauses;
ALTER TABLE pomodoro_pauses_next RENAME TO pomodoro_pauses;
CREATE INDEX idx_pomodoro_pauses_reason ON pomodoro_pauses(reason, started_at);
CREATE INDEX idx_pomodoro_pauses_segment ON pomodoro_pauses(segment_id, started_at);
CREATE INDEX idx_pomodoro_pauses_open ON pomodoro_pauses(segment_id) WHERE ended_at IS NULL;

DROP INDEX idx_pomodoro_segments_single_active;
CREATE INDEX idx_pomodoro_segments_active ON pomodoro_segments(status) WHERE status = 'active';

CREATE TABLE pomodoro_device_activity (
    device_id TEXT PRIMARY KEY,
    state TEXT NOT NULL CHECK (state IN ('idle', 'focus', 'short_break', 'long_break', 'paused')),
    run_id TEXT,
    occurrence_id TEXT,
    event_title TEXT,
    phase_started_at_ms INTEGER CHECK (phase_started_at_ms IS NULL OR phase_started_at_ms >= 0),
    phase_ends_at_ms INTEGER CHECK (phase_ends_at_ms IS NULL OR phase_ends_at_ms >= 0),
    expires_at_ms INTEGER NOT NULL CHECK (expires_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
);

CREATE TRIGGER pomodoro_runs_single_open_insert
BEFORE INSERT ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.ended_at IS NULL
    AND EXISTS (SELECT 1 FROM pomodoro_runs WHERE ended_at IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one open run');
END;

CREATE TRIGGER pomodoro_runs_single_open_update
BEFORE UPDATE OF ended_at ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.ended_at IS NULL
    AND EXISTS (SELECT 1 FROM pomodoro_runs WHERE ended_at IS NULL AND id IS NOT NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one open run');
END;

CREATE TRIGGER pomodoro_segments_single_active_insert
BEFORE INSERT ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.status = 'active'
    AND EXISTS (SELECT 1 FROM pomodoro_segments WHERE status = 'active')
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one active segment');
END;

CREATE TRIGGER pomodoro_segments_single_active_update
BEFORE UPDATE OF status ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.status = 'active'
    AND EXISTS (SELECT 1 FROM pomodoro_segments WHERE status = 'active' AND id IS NOT NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one active segment');
END;

CREATE TRIGGER pomodoro_pauses_single_open_insert
BEFORE INSERT ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.ended_at IS NULL
    AND EXISTS (
        SELECT 1 FROM pomodoro_pauses
        WHERE segment_id = NEW.segment_id AND ended_at IS NULL
    )
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one open pause per segment');
END;

CREATE TRIGGER pomodoro_pauses_single_open_update
BEFORE UPDATE OF ended_at, segment_id ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.ended_at IS NULL
    AND EXISTS (
        SELECT 1 FROM pomodoro_pauses
        WHERE segment_id = NEW.segment_id AND ended_at IS NULL
            AND NOT (segment_id = OLD.segment_id AND ordinal = OLD.ordinal)
    )
BEGIN
    SELECT RAISE(ABORT, 'Focus keeps one open pause per segment');
END;

CREATE TEMP TABLE focus_replication_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO focus_replication_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE focus_replication_guard;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails when these differ from the current rendering.

CREATE TRIGGER sync_values_pomodoro_runs_insert
BEFORE INSERT ON pomodoro_runs
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.original_event_id) = 'text' AND instr(CAST(NEW.original_event_id AS BLOB), x'00') = 0 AND length(NEW.original_event_id) <= 262144 AND length(CAST(NEW.original_event_id AS BLOB)) <= 262144)
    AND (typeof(NEW.event_date) = 'text' AND instr(CAST(NEW.event_date AS BLOB), x'00') = 0 AND length(NEW.event_date) <= 262144 AND length(CAST(NEW.event_date AS BLOB)) <= 262144)
    AND (typeof(NEW.planned_start) = 'text' AND instr(CAST(NEW.planned_start AS BLOB), x'00') = 0 AND trim(NEW.planned_start) <> '' AND length(CAST(NEW.planned_start AS BLOB)) <= 64)
    AND (typeof(NEW.planned_end) = 'text' AND instr(CAST(NEW.planned_end AS BLOB), x'00') = 0 AND trim(NEW.planned_end) <> '' AND length(CAST(NEW.planned_end AS BLOB)) <= 64)
    AND (typeof(NEW.rhythm_kind) = 'text' AND instr(CAST(NEW.rhythm_kind AS BLOB), x'00') = 0 AND length(NEW.rhythm_kind) <= 32 AND length(CAST(NEW.rhythm_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.rhythm_source) = 'text' AND instr(CAST(NEW.rhythm_source AS BLOB), x'00') = 0 AND length(NEW.rhythm_source) <= 32 AND length(CAST(NEW.rhythm_source AS BLOB)) <= 262144)
    AND (NEW.preset_key IS NULL OR (typeof(NEW.preset_key) = 'text' AND instr(CAST(NEW.preset_key AS BLOB), x'00') = 0 AND length(NEW.preset_key) <= 32 AND length(CAST(NEW.preset_key AS BLOB)) <= 262144))
    AND (NEW.idle_timeout_minutes IS NULL OR (typeof(NEW.idle_timeout_minutes) = 'integer' AND NEW.idle_timeout_minutes BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.event_title_snapshot IS NULL OR (typeof(NEW.event_title_snapshot) = 'text' AND instr(CAST(NEW.event_title_snapshot AS BLOB), x'00') = 0 AND length(NEW.event_title_snapshot) <= 262144 AND length(CAST(NEW.event_title_snapshot AS BLOB)) <= 262144))
    AND (typeof(NEW.start_trigger) = 'text' AND instr(CAST(NEW.start_trigger AS BLOB), x'00') = 0 AND length(NEW.start_trigger) <= 32 AND length(CAST(NEW.start_trigger AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.started_at) = 'text' AND instr(CAST(NEW.started_at AS BLOB), x'00') = 0 AND trim(NEW.started_at) <> '' AND length(CAST(NEW.started_at AS BLOB)) <= 64)
    AND (NEW.ended_at IS NULL OR (typeof(NEW.ended_at) = 'text' AND instr(CAST(NEW.ended_at AS BLOB), x'00') = 0 AND trim(NEW.ended_at) <> '' AND length(CAST(NEW.ended_at AS BLOB)) <= 64))
    AND (NEW.end_reason IS NULL OR (typeof(NEW.end_reason) = 'text' AND instr(CAST(NEW.end_reason AS BLOB), x'00') = 0 AND length(NEW.end_reason) <= 32 AND length(CAST(NEW.end_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.last_heartbeat) = 'text' AND instr(CAST(NEW.last_heartbeat AS BLOB), x'00') = 0 AND trim(NEW.last_heartbeat) <> '' AND length(CAST(NEW.last_heartbeat AS BLOB)) <= 64)
    AND (typeof(NEW.inherited_focus_minutes) = 'integer' AND NEW.inherited_focus_minutes BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.inherited_rhythm_position) = 'integer' AND NEW.inherited_rhythm_position BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (NEW.inherited_focus_milliseconds IS NULL OR (typeof(NEW.inherited_focus_milliseconds) = 'integer' AND NEW.inherited_focus_milliseconds BETWEEN 0 AND 9223372036854775807))
    AND (NEW.inherited_phase_milliseconds IS NULL OR (typeof(NEW.inherited_phase_milliseconds) = 'integer' AND NEW.inherited_phase_milliseconds BETWEEN 0 AND 9223372036854775807))
    AND (NEW.inherited_from_run_id IS NULL OR (typeof(NEW.inherited_from_run_id) = 'text' AND instr(CAST(NEW.inherited_from_run_id AS BLOB), x'00') = 0 AND length(NEW.inherited_from_run_id) <= 128 AND length(CAST(NEW.inherited_from_run_id AS BLOB)) <= 262144))
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))
    AND (NEW.current_occurrence_id IS NULL OR (typeof(NEW.current_occurrence_id) = 'text' AND instr(CAST(NEW.current_occurrence_id AS BLOB), x'00') = 0 AND length(NEW.current_occurrence_id) <= 262144 AND length(CAST(NEW.current_occurrence_id AS BLOB)) <= 262144))
    AND (NEW.current_event_date IS NULL OR (typeof(NEW.current_event_date) = 'text' AND instr(CAST(NEW.current_event_date AS BLOB), x'00') = 0 AND length(NEW.current_event_date) <= 262144 AND length(CAST(NEW.current_event_date AS BLOB)) <= 262144))
    AND (NEW.current_event_title IS NULL OR (typeof(NEW.current_event_title) = 'text' AND instr(CAST(NEW.current_event_title AS BLOB), x'00') = 0 AND length(NEW.current_event_title) <= 262144 AND length(CAST(NEW.current_event_title AS BLOB)) <= 262144))
    AND (NEW.calendar_archive_id IS NULL OR (typeof(NEW.calendar_archive_id) = 'text' AND instr(CAST(NEW.calendar_archive_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_archive_id) <> '' AND length(CAST(NEW.calendar_archive_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_runs_update
BEFORE UPDATE OF original_event_id, event_date, planned_start, planned_end, rhythm_kind, rhythm_source, preset_key, idle_timeout_minutes, event_title_snapshot, start_trigger, created_at, started_at, ended_at, end_reason, last_heartbeat, inherited_focus_minutes, inherited_rhythm_position, inherited_focus_milliseconds, inherited_phase_milliseconds, inherited_from_run_id, event_id, current_occurrence_id, current_event_date, current_event_title, calendar_archive_id ON pomodoro_runs
WHEN ((typeof(NEW.original_event_id) = 'text' AND instr(CAST(NEW.original_event_id AS BLOB), x'00') = 0 AND length(NEW.original_event_id) <= 262144 AND length(CAST(NEW.original_event_id AS BLOB)) <= 262144)
    AND (typeof(NEW.event_date) = 'text' AND instr(CAST(NEW.event_date AS BLOB), x'00') = 0 AND length(NEW.event_date) <= 262144 AND length(CAST(NEW.event_date AS BLOB)) <= 262144)
    AND (typeof(NEW.planned_start) = 'text' AND instr(CAST(NEW.planned_start AS BLOB), x'00') = 0 AND trim(NEW.planned_start) <> '' AND length(CAST(NEW.planned_start AS BLOB)) <= 64)
    AND (typeof(NEW.planned_end) = 'text' AND instr(CAST(NEW.planned_end AS BLOB), x'00') = 0 AND trim(NEW.planned_end) <> '' AND length(CAST(NEW.planned_end AS BLOB)) <= 64)
    AND (typeof(NEW.rhythm_kind) = 'text' AND instr(CAST(NEW.rhythm_kind AS BLOB), x'00') = 0 AND length(NEW.rhythm_kind) <= 32 AND length(CAST(NEW.rhythm_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.rhythm_source) = 'text' AND instr(CAST(NEW.rhythm_source AS BLOB), x'00') = 0 AND length(NEW.rhythm_source) <= 32 AND length(CAST(NEW.rhythm_source AS BLOB)) <= 262144)
    AND (NEW.preset_key IS NULL OR (typeof(NEW.preset_key) = 'text' AND instr(CAST(NEW.preset_key AS BLOB), x'00') = 0 AND length(NEW.preset_key) <= 32 AND length(CAST(NEW.preset_key AS BLOB)) <= 262144))
    AND (NEW.idle_timeout_minutes IS NULL OR (typeof(NEW.idle_timeout_minutes) = 'integer' AND NEW.idle_timeout_minutes BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.event_title_snapshot IS NULL OR (typeof(NEW.event_title_snapshot) = 'text' AND instr(CAST(NEW.event_title_snapshot AS BLOB), x'00') = 0 AND length(NEW.event_title_snapshot) <= 262144 AND length(CAST(NEW.event_title_snapshot AS BLOB)) <= 262144))
    AND (typeof(NEW.start_trigger) = 'text' AND instr(CAST(NEW.start_trigger AS BLOB), x'00') = 0 AND length(NEW.start_trigger) <= 32 AND length(CAST(NEW.start_trigger AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (typeof(NEW.started_at) = 'text' AND instr(CAST(NEW.started_at AS BLOB), x'00') = 0 AND trim(NEW.started_at) <> '' AND length(CAST(NEW.started_at AS BLOB)) <= 64)
    AND (NEW.ended_at IS NULL OR (typeof(NEW.ended_at) = 'text' AND instr(CAST(NEW.ended_at AS BLOB), x'00') = 0 AND trim(NEW.ended_at) <> '' AND length(CAST(NEW.ended_at AS BLOB)) <= 64))
    AND (NEW.end_reason IS NULL OR (typeof(NEW.end_reason) = 'text' AND instr(CAST(NEW.end_reason AS BLOB), x'00') = 0 AND length(NEW.end_reason) <= 32 AND length(CAST(NEW.end_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.last_heartbeat) = 'text' AND instr(CAST(NEW.last_heartbeat AS BLOB), x'00') = 0 AND trim(NEW.last_heartbeat) <> '' AND length(CAST(NEW.last_heartbeat AS BLOB)) <= 64)
    AND (typeof(NEW.inherited_focus_minutes) = 'integer' AND NEW.inherited_focus_minutes BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.inherited_rhythm_position) = 'integer' AND NEW.inherited_rhythm_position BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (NEW.inherited_focus_milliseconds IS NULL OR (typeof(NEW.inherited_focus_milliseconds) = 'integer' AND NEW.inherited_focus_milliseconds BETWEEN 0 AND 9223372036854775807))
    AND (NEW.inherited_phase_milliseconds IS NULL OR (typeof(NEW.inherited_phase_milliseconds) = 'integer' AND NEW.inherited_phase_milliseconds BETWEEN 0 AND 9223372036854775807))
    AND (NEW.inherited_from_run_id IS NULL OR (typeof(NEW.inherited_from_run_id) = 'text' AND instr(CAST(NEW.inherited_from_run_id AS BLOB), x'00') = 0 AND length(NEW.inherited_from_run_id) <= 128 AND length(CAST(NEW.inherited_from_run_id AS BLOB)) <= 262144))
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))
    AND (NEW.current_occurrence_id IS NULL OR (typeof(NEW.current_occurrence_id) = 'text' AND instr(CAST(NEW.current_occurrence_id AS BLOB), x'00') = 0 AND length(NEW.current_occurrence_id) <= 262144 AND length(CAST(NEW.current_occurrence_id AS BLOB)) <= 262144))
    AND (NEW.current_event_date IS NULL OR (typeof(NEW.current_event_date) = 'text' AND instr(CAST(NEW.current_event_date AS BLOB), x'00') = 0 AND length(NEW.current_event_date) <= 262144 AND length(CAST(NEW.current_event_date AS BLOB)) <= 262144))
    AND (NEW.current_event_title IS NULL OR (typeof(NEW.current_event_title) = 'text' AND instr(CAST(NEW.current_event_title AS BLOB), x'00') = 0 AND length(NEW.current_event_title) <= 262144 AND length(CAST(NEW.current_event_title AS BLOB)) <= 262144))
    AND (NEW.calendar_archive_id IS NULL OR (typeof(NEW.calendar_archive_id) = 'text' AND instr(CAST(NEW.calendar_archive_id AS BLOB), x'00') = 0 AND trim(NEW.calendar_archive_id) <> '' AND length(CAST(NEW.calendar_archive_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_pomodoro_runs
BEFORE UPDATE OF id, inherited_from_run_id ON pomodoro_runs
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.inherited_from_run_id IS NOT NEW.inherited_from_run_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_pomodoro_runs
BEFORE INSERT ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 224 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM pomodoro_runs WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_publish_pomodoro_runs
BEFORE UPDATE ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM (SELECT OLD.id AS id, OLD.original_event_id AS original_event_id, OLD.event_date AS event_date, OLD.planned_start AS planned_start, OLD.planned_end AS planned_end, OLD.rhythm_kind AS rhythm_kind, OLD.rhythm_source AS rhythm_source, OLD.preset_key AS preset_key, OLD.idle_timeout_minutes AS idle_timeout_minutes, OLD.event_title_snapshot AS event_title_snapshot, OLD.start_trigger AS start_trigger, OLD.created_at AS created_at, OLD.started_at AS started_at, OLD.ended_at AS ended_at, OLD.end_reason AS end_reason, OLD.last_heartbeat AS last_heartbeat, OLD.inherited_focus_minutes AS inherited_focus_minutes, OLD.inherited_rhythm_position AS inherited_rhythm_position, OLD.inherited_focus_milliseconds AS inherited_focus_milliseconds, OLD.inherited_phase_milliseconds AS inherited_phase_milliseconds, OLD.inherited_from_run_id AS inherited_from_run_id, OLD.event_id AS event_id, OLD.current_occurrence_id AS current_occurrence_id, OLD.current_event_date AS current_event_date, OLD.current_event_title AS current_event_title, OLD.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
    AND NOT EXISTS (SELECT 1 FROM (SELECT NEW.id AS id, NEW.original_event_id AS original_event_id, NEW.event_date AS event_date, NEW.planned_start AS planned_start, NEW.planned_end AS planned_end, NEW.rhythm_kind AS rhythm_kind, NEW.rhythm_source AS rhythm_source, NEW.preset_key AS preset_key, NEW.idle_timeout_minutes AS idle_timeout_minutes, NEW.event_title_snapshot AS event_title_snapshot, NEW.start_trigger AS start_trigger, NEW.created_at AS created_at, NEW.started_at AS started_at, NEW.ended_at AS ended_at, NEW.end_reason AS end_reason, NEW.last_heartbeat AS last_heartbeat, NEW.inherited_focus_minutes AS inherited_focus_minutes, NEW.inherited_rhythm_position AS inherited_rhythm_position, NEW.inherited_focus_milliseconds AS inherited_focus_milliseconds, NEW.inherited_phase_milliseconds AS inherited_phase_milliseconds, NEW.inherited_from_run_id AS inherited_from_run_id, NEW.event_id AS event_id, NEW.current_occurrence_id AS current_occurrence_id, NEW.current_event_date AS current_event_date, NEW.current_event_title AS current_event_title, NEW.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'A published row cannot become a draft again');
END;

CREATE TRIGGER sync_capture_pomodoro_runs_insert
AFTER INSERT ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM (SELECT NEW.id AS id, NEW.original_event_id AS original_event_id, NEW.event_date AS event_date, NEW.planned_start AS planned_start, NEW.planned_end AS planned_end, NEW.rhythm_kind AS rhythm_kind, NEW.rhythm_source AS rhythm_source, NEW.preset_key AS preset_key, NEW.idle_timeout_minutes AS idle_timeout_minutes, NEW.event_title_snapshot AS event_title_snapshot, NEW.start_trigger AS start_trigger, NEW.created_at AS created_at, NEW.started_at AS started_at, NEW.ended_at AS ended_at, NEW.end_reason AS end_reason, NEW.last_heartbeat AS last_heartbeat, NEW.inherited_focus_minutes AS inherited_focus_minutes, NEW.inherited_rhythm_position AS inherited_rhythm_position, NEW.inherited_focus_milliseconds AS inherited_focus_milliseconds, NEW.inherited_phase_milliseconds AS inherited_phase_milliseconds, NEW.inherited_from_run_id AS inherited_from_run_id, NEW.event_id AS event_id, NEW.current_occurrence_id AS current_occurrence_id, NEW.current_event_date AS current_event_date, NEW.current_event_title AS current_event_title, NEW.calendar_archive_id AS calendar_archive_id) WHERE ended_at IS NOT NULL)
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (224, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

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
END;

CREATE TRIGGER sync_capture_pomodoro_runs_delete
AFTER DELETE ON pomodoro_runs
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 224 AND row_key = OLD.id)
        OR EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 224 AND row_key = OLD.id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (224, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_pomodoro_run_count_rhythms_insert
BEFORE INSERT ON pomodoro_run_count_rhythms
WHEN ((typeof(NEW.focus_duration_minutes) = 'integer' AND NEW.focus_duration_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.short_break_minutes) = 'integer' AND NEW.short_break_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.long_break_minutes) = 'integer' AND NEW.long_break_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.long_break_after_focus_count) = 'integer' AND NEW.long_break_after_focus_count BETWEEN 1 AND 12)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_run_count_rhythms_update
BEFORE UPDATE OF focus_duration_minutes, short_break_minutes, long_break_minutes, long_break_after_focus_count ON pomodoro_run_count_rhythms
WHEN ((typeof(NEW.focus_duration_minutes) = 'integer' AND NEW.focus_duration_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.short_break_minutes) = 'integer' AND NEW.short_break_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.long_break_minutes) = 'integer' AND NEW.long_break_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.long_break_after_focus_count) = 'integer' AND NEW.long_break_after_focus_count BETWEEN 1 AND 12)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_pomodoro_run_count_rhythms_insert
AFTER INSERT ON pomodoro_run_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, NEW.run_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = NEW.run_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_run_count_rhythms_update
AFTER UPDATE ON pomodoro_run_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.run_id AS key UNION SELECT OLD.run_id) AS parent
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = parent.key AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_run_count_rhythms_delete
AFTER DELETE ON pomodoro_run_count_rhythms
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, OLD.run_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = OLD.run_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_pomodoro_run_sequence_steps_insert
BEFORE INSERT ON pomodoro_run_sequence_steps
WHEN ((typeof(NEW.focus_duration_minutes) = 'integer' AND NEW.focus_duration_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.break_phase) = 'text' AND instr(CAST(NEW.break_phase AS BLOB), x'00') = 0 AND length(NEW.break_phase) <= 32 AND length(CAST(NEW.break_phase AS BLOB)) <= 262144)
    AND (typeof(NEW.break_duration_minutes) = 'integer' AND NEW.break_duration_minutes BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_run_sequence_steps_update
BEFORE UPDATE OF focus_duration_minutes, break_phase, break_duration_minutes ON pomodoro_run_sequence_steps
WHEN ((typeof(NEW.focus_duration_minutes) = 'integer' AND NEW.focus_duration_minutes BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.break_phase) = 'text' AND instr(CAST(NEW.break_phase AS BLOB), x'00') = 0 AND length(NEW.break_phase) <= 32 AND length(CAST(NEW.break_phase AS BLOB)) <= 262144)
    AND (typeof(NEW.break_duration_minutes) = 'integer' AND NEW.break_duration_minutes BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_pomodoro_run_sequence_steps_insert
AFTER INSERT ON pomodoro_run_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, NEW.run_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = NEW.run_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_run_sequence_steps_update
AFTER UPDATE ON pomodoro_run_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, parent.key, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.run_id AS key UNION SELECT OLD.run_id) AS parent
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = parent.key AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_run_sequence_steps_delete
AFTER DELETE ON pomodoro_run_sequence_steps
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 224, OLD.run_id, 16, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_runs AS sync_owner WHERE sync_owner.id = OLD.run_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_pomodoro_segments_insert
BEFORE INSERT ON pomodoro_segments
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (typeof(NEW.rhythm_position) = 'integer' AND NEW.rhythm_position BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144)
    AND (typeof(NEW.planned_start) = 'text' AND instr(CAST(NEW.planned_start AS BLOB), x'00') = 0 AND trim(NEW.planned_start) <> '' AND length(CAST(NEW.planned_start AS BLOB)) <= 64)
    AND (typeof(NEW.planned_end) = 'text' AND instr(CAST(NEW.planned_end AS BLOB), x'00') = 0 AND trim(NEW.planned_end) <> '' AND length(CAST(NEW.planned_end AS BLOB)) <= 64)
    AND (typeof(NEW.actual_start) = 'text' AND instr(CAST(NEW.actual_start AS BLOB), x'00') = 0 AND trim(NEW.actual_start) <> '' AND length(CAST(NEW.actual_start AS BLOB)) <= 64)
    AND (NEW.actual_end IS NULL OR (typeof(NEW.actual_end) = 'text' AND instr(CAST(NEW.actual_end AS BLOB), x'00') = 0 AND trim(NEW.actual_end) <> '' AND length(CAST(NEW.actual_end AS BLOB)) <= 64))
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.end_reason IS NULL OR (typeof(NEW.end_reason) = 'text' AND instr(CAST(NEW.end_reason AS BLOB), x'00') = 0 AND length(NEW.end_reason) <= 32 AND length(CAST(NEW.end_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.event_date) = 'text' AND instr(CAST(NEW.event_date AS BLOB), x'00') = 0 AND length(NEW.event_date) <= 262144 AND length(CAST(NEW.event_date AS BLOB)) <= 262144)
    AND (NEW.chosen_duration_ms IS NULL OR (typeof(NEW.chosen_duration_ms) = 'integer' AND NEW.chosen_duration_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_segments_update
BEFORE UPDATE OF run_id, rhythm_position, phase, planned_start, planned_end, actual_start, actual_end, status, end_reason, event_date, chosen_duration_ms, created_at, event_id ON pomodoro_segments
WHEN ((NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (typeof(NEW.rhythm_position) = 'integer' AND NEW.rhythm_position BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144)
    AND (typeof(NEW.planned_start) = 'text' AND instr(CAST(NEW.planned_start AS BLOB), x'00') = 0 AND trim(NEW.planned_start) <> '' AND length(CAST(NEW.planned_start AS BLOB)) <= 64)
    AND (typeof(NEW.planned_end) = 'text' AND instr(CAST(NEW.planned_end AS BLOB), x'00') = 0 AND trim(NEW.planned_end) <> '' AND length(CAST(NEW.planned_end AS BLOB)) <= 64)
    AND (typeof(NEW.actual_start) = 'text' AND instr(CAST(NEW.actual_start AS BLOB), x'00') = 0 AND trim(NEW.actual_start) <> '' AND length(CAST(NEW.actual_start AS BLOB)) <= 64)
    AND (NEW.actual_end IS NULL OR (typeof(NEW.actual_end) = 'text' AND instr(CAST(NEW.actual_end AS BLOB), x'00') = 0 AND trim(NEW.actual_end) <> '' AND length(CAST(NEW.actual_end AS BLOB)) <= 64))
    AND (typeof(NEW.status) = 'text' AND instr(CAST(NEW.status AS BLOB), x'00') = 0 AND length(NEW.status) <= 32 AND length(CAST(NEW.status AS BLOB)) <= 262144)
    AND (NEW.end_reason IS NULL OR (typeof(NEW.end_reason) = 'text' AND instr(CAST(NEW.end_reason AS BLOB), x'00') = 0 AND length(NEW.end_reason) <= 32 AND length(CAST(NEW.end_reason AS BLOB)) <= 262144))
    AND (typeof(NEW.event_date) = 'text' AND instr(CAST(NEW.event_date AS BLOB), x'00') = 0 AND length(NEW.event_date) <= 262144 AND length(CAST(NEW.event_date AS BLOB)) <= 262144)
    AND (NEW.chosen_duration_ms IS NULL OR (typeof(NEW.chosen_duration_ms) = 'integer' AND NEW.chosen_duration_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)
    AND (NEW.event_id IS NULL OR (typeof(NEW.event_id) = 'text' AND instr(CAST(NEW.event_id AS BLOB), x'00') = 0 AND trim(NEW.event_id) <> '' AND length(CAST(NEW.event_id AS BLOB)) <= 128))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_pomodoro_segments
BEFORE UPDATE OF id, run_id ON pomodoro_segments
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.run_id IS NOT NEW.run_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_pomodoro_segments
BEFORE INSERT ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 225 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM pomodoro_segments WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_pomodoro_segments_insert
AFTER INSERT ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (225, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_pomodoro_segments_update
AFTER UPDATE ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 225, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.rhythm_position IS NOT NEW.rhythm_position OR OLD.phase IS NOT NEW.phase COLLATE BINARY OR OLD.planned_start IS NOT NEW.planned_start COLLATE BINARY OR OLD.planned_end IS NOT NEW.planned_end COLLATE BINARY OR OLD.actual_start IS NOT NEW.actual_start COLLATE BINARY OR OLD.actual_end IS NOT NEW.actual_end COLLATE BINARY OR OLD.status IS NOT NEW.status COLLATE BINARY OR OLD.end_reason IS NOT NEW.end_reason COLLATE BINARY OR OLD.event_date IS NOT NEW.event_date COLLATE BINARY OR OLD.chosen_duration_ms IS NOT NEW.chosen_duration_ms OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.event_id IS NOT NEW.event_id COLLATE BINARY
                AND NOT (NEW.event_id IS NULL AND NOT EXISTS (SELECT 1 FROM calendar_events WHERE id = OLD.event_id))
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_segments_delete
AFTER DELETE ON pomodoro_segments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 225 AND row_key = OLD.id)
        OR EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 225 AND row_key = OLD.id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (225, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_pomodoro_pauses_insert
BEFORE INSERT ON pomodoro_pauses
WHEN ((typeof(NEW.started_at) = 'text' AND instr(CAST(NEW.started_at AS BLOB), x'00') = 0 AND trim(NEW.started_at) <> '' AND length(CAST(NEW.started_at AS BLOB)) <= 64)
    AND (NEW.ended_at IS NULL OR (typeof(NEW.ended_at) = 'text' AND instr(CAST(NEW.ended_at AS BLOB), x'00') = 0 AND trim(NEW.ended_at) <> '' AND length(CAST(NEW.ended_at AS BLOB)) <= 64))
    AND (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 32 AND length(CAST(NEW.reason AS BLOB)) <= 262144)
    AND (NEW.detected_at IS NULL OR (typeof(NEW.detected_at) = 'text' AND instr(CAST(NEW.detected_at AS BLOB), x'00') = 0 AND trim(NEW.detected_at) <> '' AND length(CAST(NEW.detected_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_pauses_update
BEFORE UPDATE OF started_at, ended_at, reason, detected_at, created_at ON pomodoro_pauses
WHEN ((typeof(NEW.started_at) = 'text' AND instr(CAST(NEW.started_at AS BLOB), x'00') = 0 AND trim(NEW.started_at) <> '' AND length(CAST(NEW.started_at AS BLOB)) <= 64)
    AND (NEW.ended_at IS NULL OR (typeof(NEW.ended_at) = 'text' AND instr(CAST(NEW.ended_at AS BLOB), x'00') = 0 AND trim(NEW.ended_at) <> '' AND length(CAST(NEW.ended_at AS BLOB)) <= 64))
    AND (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 32 AND length(CAST(NEW.reason AS BLOB)) <= 262144)
    AND (NEW.detected_at IS NULL OR (typeof(NEW.detected_at) = 'text' AND instr(CAST(NEW.detected_at AS BLOB), x'00') = 0 AND trim(NEW.detected_at) <> '' AND length(CAST(NEW.detected_at AS BLOB)) <= 64))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_pomodoro_pauses_insert
AFTER INSERT ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 225, NEW.segment_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_segments AS sync_owner WHERE sync_owner.id = NEW.segment_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_pauses_update
AFTER UPDATE ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 225, parent.key, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.segment_id AS key UNION SELECT OLD.segment_id) AS parent
    WHERE EXISTS (SELECT 1 FROM pomodoro_segments AS sync_owner WHERE sync_owner.id = parent.key AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_pauses_delete
AFTER DELETE ON pomodoro_pauses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 225, OLD.segment_id, 8, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE EXISTS (SELECT 1 FROM pomodoro_segments AS sync_owner WHERE sync_owner.id = OLD.segment_id AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = sync_owner.run_id AND (ended_at IS NOT NULL)))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_pomodoro_run_events_insert
BEFORE INSERT ON pomodoro_run_events
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (NEW.segment_id IS NULL OR (typeof(NEW.segment_id) = 'text' AND instr(CAST(NEW.segment_id AS BLOB), x'00') = 0 AND trim(NEW.segment_id) <> '' AND length(CAST(NEW.segment_id AS BLOB)) <= 128))
    AND (typeof(NEW.event_type) = 'text' AND instr(CAST(NEW.event_type AS BLOB), x'00') = 0 AND length(NEW.event_type) <= 32 AND length(CAST(NEW.event_type AS BLOB)) <= 262144)
    AND (typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)
    AND (NEW.phase IS NULL OR (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144))
    AND (NEW.reason IS NULL OR (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 262144 AND length(CAST(NEW.reason AS BLOB)) <= 262144))
    AND (NEW.duration_seconds IS NULL OR (typeof(NEW.duration_seconds) = 'integer' AND NEW.duration_seconds BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_run_events_update
BEFORE UPDATE OF run_id, segment_id, event_type, occurred_at, phase, reason, duration_seconds, created_at ON pomodoro_run_events
WHEN ((NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND trim(NEW.run_id) <> '' AND length(CAST(NEW.run_id AS BLOB)) <= 128))
    AND (NEW.segment_id IS NULL OR (typeof(NEW.segment_id) = 'text' AND instr(CAST(NEW.segment_id AS BLOB), x'00') = 0 AND trim(NEW.segment_id) <> '' AND length(CAST(NEW.segment_id AS BLOB)) <= 128))
    AND (typeof(NEW.event_type) = 'text' AND instr(CAST(NEW.event_type AS BLOB), x'00') = 0 AND length(NEW.event_type) <= 32 AND length(CAST(NEW.event_type AS BLOB)) <= 262144)
    AND (typeof(NEW.occurred_at) = 'text' AND instr(CAST(NEW.occurred_at AS BLOB), x'00') = 0 AND trim(NEW.occurred_at) <> '' AND length(CAST(NEW.occurred_at AS BLOB)) <= 64)
    AND (NEW.phase IS NULL OR (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144))
    AND (NEW.reason IS NULL OR (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 262144 AND length(CAST(NEW.reason AS BLOB)) <= 262144))
    AND (NEW.duration_seconds IS NULL OR (typeof(NEW.duration_seconds) = 'integer' AND NEW.duration_seconds BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.created_at) = 'text' AND instr(CAST(NEW.created_at AS BLOB), x'00') = 0 AND trim(NEW.created_at) <> '' AND length(CAST(NEW.created_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_pomodoro_run_events
BEFORE UPDATE OF id, run_id, event_type, occurred_at, phase, reason, duration_seconds, created_at ON pomodoro_run_events
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.run_id IS NOT NEW.run_id COLLATE BINARY OR OLD.event_type IS NOT NEW.event_type COLLATE BINARY OR OLD.occurred_at IS NOT NEW.occurred_at COLLATE BINARY OR OLD.phase IS NOT NEW.phase COLLATE BINARY OR OLD.reason IS NOT NEW.reason COLLATE BINARY OR OLD.duration_seconds IS NOT NEW.duration_seconds OR OLD.created_at IS NOT NEW.created_at COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_pomodoro_run_events
BEFORE INSERT ON pomodoro_run_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 226 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM pomodoro_run_events WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_pomodoro_run_events_insert
AFTER INSERT ON pomodoro_run_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (226, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_pomodoro_run_events_update
AFTER UPDATE ON pomodoro_run_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM pomodoro_runs AS sync_publisher WHERE sync_publisher.id = NEW.run_id AND (ended_at IS NOT NULL))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 226, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.segment_id IS NOT NEW.segment_id COLLATE BINARY
                AND NOT (NEW.segment_id IS NULL AND NOT EXISTS (SELECT 1 FROM pomodoro_segments WHERE id = OLD.segment_id))
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_run_events_delete
AFTER DELETE ON pomodoro_run_events
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND (EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 226 AND row_key = OLD.id)
        OR EXISTS (SELECT 1 FROM sync_capture WHERE table_id = 226 AND row_key = OLD.id))
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (226, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_pomodoro_device_activity_insert
BEFORE INSERT ON pomodoro_device_activity
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND trim(NEW.device_id) <> '' AND length(CAST(NEW.device_id AS BLOB)) <= 128)
    AND (typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND length(NEW.run_id) <= 128 AND length(CAST(NEW.run_id AS BLOB)) <= 262144))
    AND (NEW.occurrence_id IS NULL OR (typeof(NEW.occurrence_id) = 'text' AND instr(CAST(NEW.occurrence_id AS BLOB), x'00') = 0 AND length(NEW.occurrence_id) <= 262144 AND length(CAST(NEW.occurrence_id AS BLOB)) <= 262144))
    AND (NEW.event_title IS NULL OR (typeof(NEW.event_title) = 'text' AND instr(CAST(NEW.event_title AS BLOB), x'00') = 0 AND length(NEW.event_title) <= 262144 AND length(CAST(NEW.event_title AS BLOB)) <= 262144))
    AND (NEW.phase_started_at_ms IS NULL OR (typeof(NEW.phase_started_at_ms) = 'integer' AND NEW.phase_started_at_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.phase_ends_at_ms IS NULL OR (typeof(NEW.phase_ends_at_ms) = 'integer' AND NEW.phase_ends_at_ms BETWEEN 0 AND 9223372036854775807))
    AND (typeof(NEW.expires_at_ms) = 'integer' AND NEW.expires_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_pomodoro_device_activity_update
BEFORE UPDATE OF state, run_id, occurrence_id, event_title, phase_started_at_ms, phase_ends_at_ms, expires_at_ms, updated_at_ms ON pomodoro_device_activity
WHEN ((typeof(NEW.state) = 'text' AND instr(CAST(NEW.state AS BLOB), x'00') = 0 AND length(NEW.state) <= 32 AND length(CAST(NEW.state AS BLOB)) <= 262144)
    AND (NEW.run_id IS NULL OR (typeof(NEW.run_id) = 'text' AND instr(CAST(NEW.run_id AS BLOB), x'00') = 0 AND length(NEW.run_id) <= 128 AND length(CAST(NEW.run_id AS BLOB)) <= 262144))
    AND (NEW.occurrence_id IS NULL OR (typeof(NEW.occurrence_id) = 'text' AND instr(CAST(NEW.occurrence_id AS BLOB), x'00') = 0 AND length(NEW.occurrence_id) <= 262144 AND length(CAST(NEW.occurrence_id AS BLOB)) <= 262144))
    AND (NEW.event_title IS NULL OR (typeof(NEW.event_title) = 'text' AND instr(CAST(NEW.event_title AS BLOB), x'00') = 0 AND length(NEW.event_title) <= 262144 AND length(CAST(NEW.event_title AS BLOB)) <= 262144))
    AND (NEW.phase_started_at_ms IS NULL OR (typeof(NEW.phase_started_at_ms) = 'integer' AND NEW.phase_started_at_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.phase_ends_at_ms IS NULL OR (typeof(NEW.phase_ends_at_ms) = 'integer' AND NEW.phase_ends_at_ms BETWEEN 0 AND 9223372036854775807))
    AND (typeof(NEW.expires_at_ms) = 'integer' AND NEW.expires_at_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_pomodoro_device_activity
BEFORE UPDATE OF device_id ON pomodoro_device_activity
WHEN OLD.device_id IS NOT NEW.device_id COLLATE BINARY
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_pomodoro_device_activity
BEFORE INSERT ON pomodoro_device_activity
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 227 AND row_key = NEW.device_id)
    AND NOT EXISTS (SELECT 1 FROM pomodoro_device_activity WHERE device_id = NEW.device_id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_pomodoro_device_activity_insert
AFTER INSERT ON pomodoro_device_activity
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (227, NEW.device_id, 1, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_pomodoro_device_activity_update
AFTER UPDATE ON pomodoro_device_activity
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 227, NEW.device_id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.state IS NOT NEW.state COLLATE BINARY OR OLD.run_id IS NOT NEW.run_id COLLATE BINARY OR OLD.occurrence_id IS NOT NEW.occurrence_id COLLATE BINARY OR OLD.event_title IS NOT NEW.event_title COLLATE BINARY OR OLD.phase_started_at_ms IS NOT NEW.phase_started_at_ms OR OLD.phase_ends_at_ms IS NOT NEW.phase_ends_at_ms OR OLD.expires_at_ms IS NOT NEW.expires_at_ms OR OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 1 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_pomodoro_device_activity_delete
AFTER DELETE ON pomodoro_device_activity
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (227, OLD.device_id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

COMMIT;

PRAGMA legacy_alter_table = OFF;
PRAGMA foreign_keys = ON;

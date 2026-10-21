-- Soundscapes play only presets. A preset mixes its sounds with per-sound levels and plays relative to the Music
-- volume setting through its own level, so the soundscape no longer stores an absolute volume, a layered mode,
-- per-section levels, or a selection of individual sounds. Every phase that chose one sound now names the preset
-- made of only that sound.

-- Preset level multiplies the Music volume: 0 is silent, 1 matches Music, 2 doubles it before output clamping.
ALTER TABLE music_soundscape_presets ADD COLUMN level REAL NOT NULL DEFAULT 0.5
    CHECK (level >= 0.0 AND level <= 2.0);

-- Sound level is the share of the preset each sound plays at, from silent to full.
ALTER TABLE music_soundscape_preset_sounds ADD COLUMN level REAL NOT NULL DEFAULT 1.0
    CHECK (level >= 0.0 AND level <= 1.0);

-- Each generated noise starts as a one-layer rain soundscape so the soundscape controls are never empty. The
-- brightest texture sits lowest and the deepest sits above Music, so the three rains sound evenly loud.
INSERT OR IGNORE INTO music_soundscape_presets (id, name, icon, level, created_at_ms, updated_at_ms, version)
SELECT 'preset-' || sounds.id, seeds.name, seeds.icon, seeds.level,
       CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1
FROM music_soundscapes AS sounds
JOIN (
    SELECT 'generated-brown-noise' AS id, 'Rain (soft)' AS name, 'lucide:cloud-hail' AS icon, 1.25 AS level
    UNION ALL SELECT 'generated-pink-noise', 'Rain (moderate)', 'lucide:cloud-rain', 0.75
    UNION ALL SELECT 'generated-white-noise', 'Rain (heavy)', 'lucide:cloud-rain-wind', 0.5
) AS seeds ON seeds.id = sounds.id;

-- A sound some phase still names becomes its own preset under the same derived id.
INSERT OR IGNORE INTO music_soundscape_presets (id, name, icon, created_at_ms, updated_at_ms, version)
SELECT 'preset-' || sounds.id, sounds.name, sounds.icon,
       CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1
FROM music_soundscapes AS sounds
WHERE sounds.id IN (
    SELECT soundscape_id FROM music_context_assignments WHERE soundscape_id IS NOT NULL
    UNION SELECT soundscape_id FROM calendar_event_archive_music_assignments WHERE soundscape_id IS NOT NULL
);

INSERT OR IGNORE INTO music_soundscape_preset_sounds (preset_id, position, soundscape_id)
SELECT presets.id, 0, sounds.id
FROM music_soundscapes AS sounds
JOIN music_soundscape_presets AS presets ON presets.id = 'preset-' || sounds.id
WHERE NOT EXISTS (SELECT 1 FROM music_soundscape_preset_sounds AS existing WHERE existing.preset_id = presets.id);

-- Assignments name presets only. A sound id that no longer exists keeps a dangling preset id, which phases report
-- as unavailable instead of silently changing meaning.
CREATE TABLE music_context_assignments_rebuild (
    owner_kind TEXT NOT NULL CHECK (owner_kind IN (
            'project-default',
            'event-snapshot',
            'event-override',
            'work-environment'
        )),
    owner_id TEXT NOT NULL CHECK (trim(owner_id) <> ''),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short-break', 'long-break')),
    behavior TEXT NOT NULL CHECK (behavior IN (
            'inherit',
            'play-automatically',
            'prepare-silently',
            'pause-music',
            'keep-current-music'
        )),
    playlist_id TEXT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN (
            'explicit',
            'copied-project',
            'work-environment'
        )),
    provenance_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    soundscape_behavior TEXT NOT NULL DEFAULT 'inherit'
        CHECK (soundscape_behavior IN (
            'inherit',
            'play-selected',
            'pause-soundscape',
            'keep-current-soundscape'
        )),
    soundscape_preset_id TEXT,
    PRIMARY KEY (owner_kind, owner_id, phase),
    CHECK (playlist_id IS NULL OR trim(playlist_id) <> ''),
    CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> ''),
    CHECK (provenance_id IS NULL OR trim(provenance_id) <> '')
);

INSERT INTO music_context_assignments_rebuild (
    owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id
)
SELECT owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, provenance_id,
       updated_at_ms, version, soundscape_behavior,
       COALESCE(soundscape_preset_id, CASE WHEN soundscape_id IS NOT NULL THEN 'preset-' || soundscape_id END)
FROM music_context_assignments;

DROP TABLE music_context_assignments;
ALTER TABLE music_context_assignments_rebuild RENAME TO music_context_assignments;
CREATE INDEX idx_music_context_assignments_owner
    ON music_context_assignments(owner_kind, owner_id, phase);
CREATE INDEX idx_music_context_assignments_playlist
    ON music_context_assignments(playlist_id, owner_kind, owner_id);

CREATE TABLE calendar_event_archive_music_assignments_rebuild (
    archive_event_id TEXT NOT NULL REFERENCES calendar_event_archives(id) ON DELETE CASCADE,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('event-snapshot', 'event-override')),
    phase TEXT NOT NULL CHECK (phase IN ('focus', 'short-break', 'long-break')),
    behavior TEXT NOT NULL CHECK (behavior IN (
            'inherit', 'play-automatically', 'prepare-silently', 'pause-music', 'keep-current-music'
        )),
    playlist_id TEXT,
    provenance_kind TEXT NOT NULL CHECK (provenance_kind IN ('explicit', 'copied-project', 'work-environment')),
    provenance_id TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL CHECK (version > 0),
    soundscape_behavior TEXT NOT NULL CHECK (soundscape_behavior IN (
            'inherit', 'play-selected', 'pause-soundscape', 'keep-current-soundscape'
        )),
    soundscape_preset_id TEXT CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> ''),
    PRIMARY KEY (archive_event_id, owner_kind, phase)
);

INSERT INTO calendar_event_archive_music_assignments_rebuild (
    archive_event_id, owner_kind, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id
)
SELECT archive_event_id, owner_kind, phase, behavior, playlist_id, provenance_kind, provenance_id,
       updated_at_ms, version, soundscape_behavior,
       COALESCE(soundscape_preset_id, CASE WHEN soundscape_id IS NOT NULL THEN 'preset-' || soundscape_id END)
FROM calendar_event_archive_music_assignments;

DROP TABLE calendar_event_archive_music_assignments;
ALTER TABLE calendar_event_archive_music_assignments_rebuild RENAME TO calendar_event_archive_music_assignments;

-- The soundscape state remembers one preset and whether it should play.
CREATE TABLE music_soundscape_state_rebuild (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    active_preset_id TEXT REFERENCES music_soundscape_presets(id) ON DELETE SET NULL,
    desired_playing INTEGER NOT NULL DEFAULT 0 CHECK (desired_playing IN (0, 1)),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    automatic_intent INTEGER NOT NULL DEFAULT 0 CHECK (automatic_intent IN (0, 1))
);

INSERT INTO music_soundscape_state_rebuild (
    singleton, active_preset_id, desired_playing, updated_at_ms, version, automatic_intent
)
SELECT state.singleton, presets.id,
       CASE WHEN presets.id IS NULL THEN 0 ELSE state.desired_playing END,
       state.updated_at_ms, state.version, state.automatic_intent
FROM music_soundscape_state AS state
LEFT JOIN music_soundscape_presets AS presets ON presets.id = 'preset-' || state.active_soundscape_id;

DROP TABLE music_soundscape_state;
ALTER TABLE music_soundscape_state_rebuild RENAME TO music_soundscape_state;

DROP TABLE music_soundscape_active_selections;

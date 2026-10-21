-- Soundscape layers are not grouped: rebuild the layer table without its group reference and drop the groups.
-- No preset table references layers yet, so dropping the old table cannot cascade into presets.
CREATE TABLE music_soundscapes_rebuild (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_kind TEXT NOT NULL CHECK (source_kind IN (
            'generated-noise',
            'local-loop',
            'bundled-loop'
        )),
    generated_kind TEXT CHECK (generated_kind IN ('white', 'pink', 'brown')),
    bundled_identity TEXT,
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL DEFAULT 'lucide:audio-lines' CHECK (trim(icon) <> ''),
    availability TEXT NOT NULL CHECK (availability IN ('available', 'missing', 'unsupported')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    CHECK (
        (source_kind = 'generated-noise' AND generated_kind IS NOT NULL AND bundled_identity IS NULL)
        OR (source_kind = 'local-loop' AND generated_kind IS NULL AND bundled_identity IS NULL)
        OR (source_kind = 'bundled-loop' AND generated_kind IS NULL AND trim(bundled_identity) <> '')
    )
);

INSERT INTO music_soundscapes_rebuild (
    id, source_kind, generated_kind, bundled_identity, name, icon, availability,
    created_at_ms, updated_at_ms, version
)
SELECT id, source_kind, generated_kind, bundled_identity, name, icon, availability,
       created_at_ms, updated_at_ms, version
FROM music_soundscapes;

DROP TABLE music_soundscapes;
ALTER TABLE music_soundscapes_rebuild RENAME TO music_soundscapes;
CREATE UNIQUE INDEX idx_music_soundscapes_generated_kind
    ON music_soundscapes(generated_kind)
    WHERE generated_kind IS NOT NULL;
DROP TABLE music_soundscape_groups;

-- Background sound presets: a named, ordered mix of background sounds that Focus phases and the background sound
-- controls can apply as one choice. Removing a sound drops it from every preset; a preset left without sounds stays
-- so its references can report it as unavailable instead of silently changing meaning.
-- Context assignments reference either one sound or one preset; the Music domain validates that pairing.

CREATE TABLE music_soundscape_presets (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    icon TEXT NOT NULL CHECK (trim(icon) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0)
);

CREATE TABLE music_soundscape_preset_sounds (
    preset_id TEXT NOT NULL REFERENCES music_soundscape_presets(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    soundscape_id TEXT NOT NULL REFERENCES music_soundscapes(id) ON DELETE CASCADE,
    PRIMARY KEY (preset_id, position),
    UNIQUE (preset_id, soundscape_id)
);
CREATE INDEX idx_music_soundscape_preset_sounds_sound
    ON music_soundscape_preset_sounds(soundscape_id);

ALTER TABLE music_context_assignments ADD COLUMN soundscape_preset_id TEXT
    CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> '');

ALTER TABLE calendar_event_archive_music_assignments ADD COLUMN soundscape_preset_id TEXT
    CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> '');

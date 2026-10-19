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

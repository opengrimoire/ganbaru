-- Music tables take the shape concurrent sync needs: playlists and memberships order by fractional order keys,
-- membership break items fold into their membership, skip ranges are owned rows keyed by ordinal, levels and
-- playback values are integer thousandths, and listening statistics are kept per device. Row keys stay as they are.

-- Existing rows get rank keys: 'c' and three base 62 digits, which covers 238,328 rows per ordered group.
CREATE TEMP TABLE music_rank_capacity (
    row_count INTEGER NOT NULL CHECK (row_count <= 238328)
);
INSERT INTO music_rank_capacity (row_count)
SELECT count(*) FROM music_playlists
UNION ALL
SELECT count(*) FROM music_playlist_memberships GROUP BY playlist_id
UNION ALL
SELECT count(*) FROM music_soundscape_preset_sounds GROUP BY preset_id;
DROP TABLE music_rank_capacity;

-- Playlists order by key, ranked by their previous sort order, name, and id.
DROP INDEX idx_music_playlists_sort;

ALTER TABLE music_playlists ADD COLUMN order_key TEXT NOT NULL DEFAULT '' CHECK (length(order_key) <= 128);

UPDATE music_playlists
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (ORDER BY sort_order, name COLLATE NOCASE, id) - 1 AS rank
        FROM music_playlists
    )
) AS ranked
WHERE music_playlists.id = ranked.id;

ALTER TABLE music_playlists DROP COLUMN sort_order;

CREATE INDEX idx_music_playlists_order ON music_playlists(order_key, id);

CREATE TRIGGER music_playlists_order_key_insert
BEFORE INSERT ON music_playlists
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Music playlist order key is required');
END;

CREATE TRIGGER music_playlists_order_key_update
BEFORE UPDATE OF order_key ON music_playlists
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Music playlist order key is required');
END;

-- Memberships order by key within their playlist, ranked by their previous position and id.
DROP INDEX idx_music_playlist_memberships_eligibility;
DROP INDEX idx_music_playlist_memberships_item;
DROP INDEX idx_music_playlist_memberships_order;

ALTER TABLE music_playlist_memberships ADD COLUMN order_key TEXT NOT NULL DEFAULT ''
    CHECK (length(order_key) <= 128);

UPDATE music_playlist_memberships
SET order_key = ranked.order_key
FROM (
    SELECT
        id,
        'c'
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
            || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1)
            AS order_key
    FROM (
        SELECT id, ROW_NUMBER() OVER (PARTITION BY playlist_id ORDER BY position, id) - 1 AS rank
        FROM music_playlist_memberships
    )
) AS ranked
WHERE music_playlist_memberships.id = ranked.id;

ALTER TABLE music_playlist_memberships DROP COLUMN position;

-- Volume and rate are integer thousandths: volume from silent to full, rate from a quarter to double speed.
ALTER TABLE music_playlist_memberships ADD COLUMN volume_milli INTEGER
    CHECK (volume_milli IS NULL OR volume_milli BETWEEN 0 AND 1000);
ALTER TABLE music_playlist_memberships ADD COLUMN rate_milli INTEGER
    CHECK (rate_milli IS NULL OR rate_milli BETWEEN 250 AND 2000);

UPDATE music_playlist_memberships
SET volume_milli = CAST(round(volume * 1000) AS INTEGER),
    rate_milli = CAST(round(rate * 1000) AS INTEGER)
WHERE volume IS NOT NULL OR rate IS NOT NULL;

ALTER TABLE music_playlist_memberships DROP COLUMN volume;
ALTER TABLE music_playlist_memberships DROP COLUMN rate;

-- The break item and its range live on the membership. Deleting the break item clears only the reference;
-- writers clear the range with it, and playback ignores a range without an item.
ALTER TABLE music_playlist_memberships ADD COLUMN break_item_id TEXT
    REFERENCES music_library_items(id) ON DELETE SET NULL;
ALTER TABLE music_playlist_memberships ADD COLUMN break_start_ms INTEGER
    CHECK (break_start_ms IS NULL OR break_start_ms >= 0);
ALTER TABLE music_playlist_memberships ADD COLUMN break_end_ms INTEGER
    CHECK (break_end_ms IS NULL OR break_end_ms >= 0)
    CHECK (break_start_ms IS NULL OR break_end_ms IS NULL OR break_end_ms >= break_start_ms);
ALTER TABLE music_playlist_memberships ADD COLUMN break_volume_milli INTEGER
    CHECK (break_volume_milli IS NULL OR break_volume_milli BETWEEN 0 AND 1000);
ALTER TABLE music_playlist_memberships ADD COLUMN break_rate_milli INTEGER
    CHECK (break_rate_milli IS NULL OR break_rate_milli BETWEEN 250 AND 2000);

UPDATE music_playlist_memberships
SET break_item_id = breaks.item_id,
    break_start_ms = breaks.start_ms,
    break_end_ms = breaks.end_ms,
    break_volume_milli = CAST(round(breaks.volume * 1000) AS INTEGER),
    break_rate_milli = CAST(round(breaks.rate * 1000) AS INTEGER)
FROM music_membership_break_items AS breaks
WHERE music_playlist_memberships.id = breaks.membership_id;

DROP INDEX idx_music_membership_break_items_item;
DROP TABLE music_membership_break_items;

CREATE INDEX idx_music_playlist_memberships_eligibility
    ON music_playlist_memberships(playlist_id, enabled, weight, order_key);
CREATE INDEX idx_music_playlist_memberships_item
    ON music_playlist_memberships(item_id, playlist_id);
CREATE INDEX idx_music_playlist_memberships_order
    ON music_playlist_memberships(playlist_id, order_key, id);
CREATE INDEX idx_music_playlist_memberships_break_item
    ON music_playlist_memberships(break_item_id) WHERE break_item_id IS NOT NULL;

CREATE TRIGGER music_playlist_memberships_order_key_insert
BEFORE INSERT ON music_playlist_memberships
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Music membership order key is required');
END;

CREATE TRIGGER music_playlist_memberships_order_key_update
BEFORE UPDATE OF order_key ON music_playlist_memberships
WHEN NEW.order_key = ''
BEGIN
    SELECT RAISE(ABORT, 'Music membership order key is required');
END;

-- Skip ranges belong to their membership as an ordered list of at most 64 ranges, keyed by ordinal.
CREATE TABLE music_membership_skip_ranges_next (
    membership_id TEXT NOT NULL REFERENCES music_playlist_memberships(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms >= start_ms),
    PRIMARY KEY (membership_id, ordinal)
);

INSERT INTO music_membership_skip_ranges_next (membership_id, ordinal, start_ms, end_ms)
SELECT membership_id, ordinal, start_ms, end_ms
FROM (
    SELECT
        membership_id,
        start_ms,
        end_ms,
        ROW_NUMBER() OVER (PARTITION BY membership_id ORDER BY start_ms, end_ms, sort_order, id) - 1 AS ordinal
    FROM music_membership_skip_ranges
)
WHERE ordinal < 64;

DROP TABLE music_membership_skip_ranges;
ALTER TABLE music_membership_skip_ranges_next RENAME TO music_membership_skip_ranges;

-- Preset levels are integer thousandths of the Music volume, from silent to double.
ALTER TABLE music_soundscape_presets ADD COLUMN level_milli INTEGER NOT NULL DEFAULT 500
    CHECK (level_milli BETWEEN 0 AND 2000);
UPDATE music_soundscape_presets SET level_milli = CAST(round(level * 1000) AS INTEGER);
ALTER TABLE music_soundscape_presets DROP COLUMN level;

-- Preset sounds are keyed by sound, order by key, and play at an integer thousandth of the preset level.
CREATE TABLE music_soundscape_preset_sounds_next (
    preset_id TEXT NOT NULL REFERENCES music_soundscape_presets(id) ON DELETE CASCADE,
    soundscape_id TEXT NOT NULL REFERENCES music_soundscapes(id) ON DELETE CASCADE,
    order_key TEXT NOT NULL CHECK (length(order_key) BETWEEN 1 AND 128),
    level_milli INTEGER NOT NULL DEFAULT 1000 CHECK (level_milli BETWEEN 0 AND 1000),
    PRIMARY KEY (preset_id, soundscape_id)
);

INSERT INTO music_soundscape_preset_sounds_next (preset_id, soundscape_id, order_key, level_milli)
SELECT
    preset_id,
    soundscape_id,
    'c'
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 3844 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank / 62 % 62 + 1, 1)
        || substr('0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz', rank % 62 + 1, 1),
    CAST(round(level * 1000) AS INTEGER)
FROM (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY preset_id ORDER BY position, soundscape_id) - 1 AS rank
    FROM music_soundscape_preset_sounds
);

DROP INDEX idx_music_soundscape_preset_sounds_sound;
DROP TABLE music_soundscape_preset_sounds;
ALTER TABLE music_soundscape_preset_sounds_next RENAME TO music_soundscape_preset_sounds;
CREATE INDEX idx_music_soundscape_preset_sounds_sound
    ON music_soundscape_preset_sounds(soundscape_id);

-- Each device counts its own listening; reads add the counts and take the latest play per item. Existing
-- counts move to the fixed bucket 'vault', which no device writes again.
CREATE TABLE music_listening_statistics_next (
    device_id TEXT NOT NULL CHECK (trim(device_id) <> ''),
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    last_played_at_ms INTEGER,
    play_count INTEGER NOT NULL DEFAULT 0 CHECK (play_count >= 0),
    completion_count INTEGER NOT NULL DEFAULT 0 CHECK (completion_count >= 0),
    skip_count INTEGER NOT NULL DEFAULT 0 CHECK (skip_count >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    PRIMARY KEY (device_id, item_id)
);

INSERT INTO music_listening_statistics_next
    (device_id, item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms)
SELECT 'vault', item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms
FROM music_listening_statistics;

DROP INDEX idx_music_listening_statistics_last_played;
DROP TABLE music_listening_statistics;
ALTER TABLE music_listening_statistics_next RENAME TO music_listening_statistics;
CREATE INDEX idx_music_listening_statistics_item ON music_listening_statistics(item_id);
